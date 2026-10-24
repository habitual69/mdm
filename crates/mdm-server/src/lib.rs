//! HTTP API around the queue. Binds 127.0.0.1 in Phase 1.

use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use tower_http::cors::{Any, CorsLayer};
use base64::Engine as _;
use mdm_core::{Config, Event, Job, JobState, NewJobReq, Queue};
use mdm_engine_aria2::Aria2Engine;
use mdm_engine_ytdlp::Ytdlp;
use serde_json::json;

mod notify;
use notify::send_system_notification;

pub fn build_queue(cfg: Config) -> mdm_core::Result<Arc<Queue>> {
    let ytdlp = Ytdlp::from_config(&cfg)?;
    let data_dir = mdm_core::data_dir();
    let aria2 = Aria2Engine::new(cfg.clone(), data_dir);
    let engines: Vec<Arc<dyn mdm_core::Engine>> =
        vec![Arc::new(ytdlp), Arc::new(aria2)];
    Queue::new(cfg, engines)
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum WindowAction {
    Minimize,
    ToggleMaximize,
    Close,
    ToggleFloating,
    ResizeFloating { width: f64, height: f64 },
}

#[derive(Clone)]
pub struct AppState {
    pub queue: Arc<Queue>,
    pub window_tx: Option<tokio::sync::broadcast::Sender<WindowAction>>,
}

pub fn router(q: Arc<Queue>) -> Router {
    router_with_window(q, None)
}

pub fn router_with_window(
    q: Arc<Queue>,
    window_tx: Option<tokio::sync::broadcast::Sender<WindowAction>>,
) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        .route("/", get(ui_index))
        .route("/static/{*path}", get(ui_static))
        .route("/api/health", get(health))
        .route("/api/config", get(get_config).put(put_config))
        .route("/api/torrent-info", post(torrent_info))
        .route("/api/torrent-upload", post(upload_torrent))
        .route("/api/jobs", post(add_job).get(list_jobs))
        .route("/api/jobs/pause-all", post(pause_all_jobs))
        .route("/api/jobs/resume-all", post(resume_all_jobs))
        .route("/api/jobs/{id}", get(get_job).delete(delete_job))
        .route("/api/jobs/{id}/pause", post(pause_job))
        .route("/api/jobs/{id}/resume", post(resume_job))
        .route("/api/jobs/{id}/open-folder", post(open_job_folder))
        .route("/api/events", get(ws_events))
        .route("/api/window/minimize", post(window_minimize))
        .route("/api/window/maximize", post(window_maximize))
        .route("/api/window/close", post(window_close))
        .route("/api/window/toggle-floating", post(window_toggle_floating))
        .route("/api/window/resize-floating", post(window_resize_floating))
        .route("/api/window/state", get(window_state))
        .route("/api/media-info", post(media_info))
        .route("/api/playlist-info", post(playlist_info))
        .route("/api/jobs/batch", post(batch_add_jobs))
        .layer(cors)
        .with_state(AppState { queue: q, window_tx })
}

// --- embedded web UI -------------------------------------------------------

#[derive(rust_embed::RustEmbed)]
#[folder = "ui"]
struct UiAssets;

async fn ui_index() -> Response {
    ui_static(Path("index.html".to_string())).await
}

async fn ui_static(Path(path): Path<String>) -> Response {
    let name = path.trim_start_matches('/');
    if name.contains("..") || name.is_empty() {
        return StatusCode::NOT_FOUND.into_response();
    }
    let Some(file) = UiAssets::get(name) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let mime = match name.rsplit('.').next() {
        Some("html") => "text/html; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("js") | Some("mjs") => "text/javascript; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("ico") => "image/x-icon",
        Some("json") => "application/json",
        Some("woff2") => "font/woff2",
        _ => "application/octet-stream",
    };
    let bytes = file.data.into_owned();
    let mut resp = Response::new(axum::body::Body::from(bytes));
    resp.headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static(mime));
    resp
}

fn spawn_notification_watcher(q: Arc<Queue>) {
    tokio::spawn(async move {
        let mut rx = q.subscribe_events();
        let mut notified_done = std::collections::HashSet::new();
        let mut notified_failed = std::collections::HashSet::new();

        while let Ok(event) = rx.recv().await {
            let cfg = q.config();
            if !cfg.notifications_enabled {
                continue;
            }
            if let Event::JobUpdated { job } = event {
                match job.state {
                    JobState::Done => {
                        if notified_done.insert(job.id.clone()) {
                            let title = job.title.as_deref().unwrap_or("Download Complete");
                            let out = job
                                .output_path
                                .as_ref()
                                .or(Some(&job.output_dir))
                                .map(|p| p.display().to_string())
                                .unwrap_or_default();
                            send_system_notification(
                                "mDm — Download Complete",
                                &format!("{title}\nSaved to: {out}"),
                                None,
                            );
                        }
                    }
                    JobState::Failed => {
                        if notified_failed.insert(job.id.clone()) {
                            let title = job.title.as_deref().unwrap_or("Download Failed");
                            let err = job.error.as_deref().unwrap_or("Encountered an error");
                            send_system_notification(
                                "mDm — Download Failed",
                                &format!("{title}\n{err}"),
                                None,
                            );
                        }
                    }
                    JobState::Queued | JobState::Running => {
                        notified_done.remove(&job.id);
                        notified_failed.remove(&job.id);
                    }
                    _ => {}
                }
            }
        }
    });
}

pub async fn serve(q: Arc<Queue>, addr: &str) -> std::io::Result<()> {
    serve_with_shutdown(q, addr, std::future::pending()).await
}

pub async fn serve_with_window(
    q: Arc<Queue>,
    addr: &str,
    window_tx: Option<tokio::sync::broadcast::Sender<WindowAction>>,
) -> std::io::Result<()> {
    spawn_notification_watcher(q.clone());
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("mdm server listening on http://{addr}");
    axum::serve(listener, router_with_window(q, window_tx))
        .await
}

pub async fn serve_with_shutdown(
    q: Arc<Queue>,
    addr: &str,
    shutdown: impl std::future::Future<Output = ()> + Send + 'static,
) -> std::io::Result<()> {
    spawn_notification_watcher(q.clone());
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("mdm server listening on http://{addr}");
    axum::serve(listener, router(q))
        .with_graceful_shutdown(shutdown)
        .await
}

// --- desktop window actions ------------------------------------------------

async fn window_minimize(State(st): State<AppState>) -> StatusCode {
    if let Some(tx) = &st.window_tx {
        let _ = tx.send(WindowAction::Minimize);
        StatusCode::OK
    } else {
        StatusCode::NOT_IMPLEMENTED
    }
}

async fn window_maximize(State(st): State<AppState>) -> StatusCode {
    if let Some(tx) = &st.window_tx {
        let _ = tx.send(WindowAction::ToggleMaximize);
        StatusCode::OK
    } else {
        StatusCode::NOT_IMPLEMENTED
    }
}

async fn window_close(State(st): State<AppState>) -> StatusCode {
    if let Some(tx) = &st.window_tx {
        let _ = tx.send(WindowAction::Close);
        StatusCode::OK
    } else {
        StatusCode::NOT_IMPLEMENTED
    }
}

async fn window_toggle_floating(State(st): State<AppState>) -> StatusCode {
    if let Some(tx) = &st.window_tx {
        let _ = tx.send(WindowAction::ToggleFloating);
        StatusCode::OK
    } else {
        StatusCode::NOT_IMPLEMENTED
    }
}

#[derive(serde::Deserialize)]
struct ResizeFloatingReq {
    width: f64,
    height: f64,
}

async fn window_resize_floating(
    State(st): State<AppState>,
    Json(payload): Json<ResizeFloatingReq>,
) -> StatusCode {
    if let Some(tx) = &st.window_tx {
        let _ = tx.send(WindowAction::ResizeFloating {
            width: payload.width,
            height: payload.height,
        });
        StatusCode::OK
    } else {
        StatusCode::NOT_IMPLEMENTED
    }
}

async fn window_state(State(st): State<AppState>) -> Json<serde_json::Value> {
    Json(json!({
        "is_desktop": st.window_tx.is_some(),
    }))
}

// --- media info ------------------------------------------------------------

#[derive(serde::Deserialize)]
struct MediaInfoReq {
    url: String,
}

#[derive(serde::Serialize)]
struct MediaFormat {
    format_id: String,
    ext: String,
    resolution: String,
    filesize_approx: Option<u64>,
    note: String,
}

#[derive(serde::Serialize)]
struct MediaInfoResp {
    title: String,
    thumbnail: Option<String>,
    duration: Option<f64>,
    uploader: Option<String>,
    extractor: Option<String>,
    formats: Vec<MediaFormat>,
}

async fn media_info(
    State(st): State<AppState>,
    Json(req): Json<MediaInfoReq>,
) -> Result<Json<MediaInfoResp>, (StatusCode, String)> {
    let url = req.url.trim();
    if url.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "empty url".into()));
    }

    let cfg = st.queue.config();
    let ytdlp_path = match mdm_core::bin_path("yt-dlp", &cfg) {
        Ok(p) => p,
        Err(_e) => {
            let name = url.split('?').next().unwrap_or(url).rsplit('/').next().unwrap_or(url);
            return Ok(Json(MediaInfoResp {
                title: name.to_string(),
                thumbnail: None,
                duration: None,
                uploader: None,
                extractor: None,
                formats: vec![],
            }));
        }
    };

    let mut cmd = tokio::process::Command::new(ytdlp_path);
    cmd.args(["--dump-single-json", "--no-warnings", "--no-config", "--flat-playlist"]);

    if let Some(ref js) = mdm_core::find_js_runtime() {
        let name = js.file_stem().and_then(|s| s.to_str()).unwrap_or("node");
        cmd.args(["--js-runtimes", &format!("{name}:{}", js.display())]);
    }

    let mut path_dirs = vec![mdm_core::data_dir().join("bin")];
    if let Some(ref js) = mdm_core::find_js_runtime() {
        if let Some(parent) = js.parent() {
            if !path_dirs.contains(&parent.to_path_buf()) {
                path_dirs.push(parent.to_path_buf());
            }
        }
    }
    if let Ok(cur_path) = std::env::var("PATH") {
        path_dirs.extend(std::env::split_paths(&cur_path));
    }
    if let Ok(joined) = std::env::join_paths(&path_dirs) {
        cmd.env("PATH", joined);
    }

    cmd.arg(url);

    let output = match cmd.output().await {
        Ok(o) => o,
        Err(_) => {
            let name = url.split('?').next().unwrap_or(url).rsplit('/').next().unwrap_or(url);
            return Ok(Json(MediaInfoResp {
                title: name.to_string(),
                thumbnail: None,
                duration: None,
                uploader: None,
                extractor: None,
                formats: vec![],
            }));
        }
    };

    if !output.status.success() {
        let name = url.split('?').next().unwrap_or(url).rsplit('/').next().unwrap_or(url);
        return Ok(Json(MediaInfoResp {
            title: name.to_string(),
            thumbnail: None,
            duration: None,
            uploader: None,
            extractor: None,
            formats: vec![],
        }));
    }

    let val: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("json parse error: {e}")))?;

    let title = val.get("title").and_then(|v| v.as_str()).unwrap_or(url).to_string();
    let thumbnail = val.get("thumbnail").and_then(|v| v.as_str()).map(|s| s.to_string());
    let duration = val.get("duration").and_then(|v| v.as_f64());
    let uploader = val.get("uploader").and_then(|v| v.as_str()).map(|s| s.to_string());
    let extractor = val.get("extractor").and_then(|v| v.as_str()).map(|s| s.to_string());

    let mut formats = Vec::new();
    if let Some(fmts) = val.get("formats").and_then(|v| v.as_array()) {
        for f in fmts {
            let fid = f.get("format_id").and_then(|v| v.as_str()).unwrap_or("");
            if fid.is_empty() { continue; }
            let ext = f.get("ext").and_then(|v| v.as_str()).unwrap_or("mp4").to_string();
            let height = f.get("height").and_then(|v| v.as_i64());
            let vcodec = f.get("vcodec").and_then(|v| v.as_str()).unwrap_or("");
            let acodec = f.get("acodec").and_then(|v| v.as_str()).unwrap_or("");

            let resolution = if let Some(h) = height {
                format!("{h}p")
            } else if vcodec == "none" || (vcodec.is_empty() && acodec != "none") {
                "Audio only".to_string()
            } else {
                f.get("resolution").and_then(|v| v.as_str()).unwrap_or("unknown").to_string()
            };

            let note = f.get("format_note").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let size = f.get("filesize").and_then(|v| v.as_u64())
                .or_else(|| f.get("filesize_approx").and_then(|v| v.as_u64()));

            formats.push(MediaFormat {
                format_id: fid.to_string(),
                ext,
                resolution,
                filesize_approx: size,
                note,
            });
        }
    }

    Ok(Json(MediaInfoResp {
        title,
        thumbnail,
        duration,
        uploader,
        extractor,
        formats,
    }))
}

// --- playlist info & batch download -----------------------------------------

#[derive(serde::Deserialize)]
struct PlaylistInfoReq {
    url: String,
}

#[derive(serde::Serialize)]
struct PlaylistItem {
    index: usize,
    id: String,
    title: String,
    url: String,
    duration: Option<f64>,
    duration_string: Option<String>,
    uploader: Option<String>,
    thumbnail: Option<String>,
}

#[derive(serde::Serialize)]
struct PlaylistInfoResp {
    is_playlist: bool,
    title: String,
    uploader: Option<String>,
    count: usize,
    single_video_url: Option<String>,
    single_video_title: Option<String>,
    entries: Vec<PlaylistItem>,
}

fn fmt_sec_to_time(sec: f64) -> String {
    let total = sec.round() as u64;
    let s = total % 60;
    let m = (total / 60) % 60;
    let h = total / 3600;
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

async fn playlist_info(
    State(st): State<AppState>,
    Json(req): Json<PlaylistInfoReq>,
) -> Result<Json<PlaylistInfoResp>, (StatusCode, String)> {
    let url = req.url.trim();
    if url.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "empty url".into()));
    }

    let cfg = st.queue.config();
    let ytdlp_path = match mdm_core::bin_path("yt-dlp", &cfg) {
        Ok(p) => p,
        Err(e) => return Err((StatusCode::INTERNAL_SERVER_ERROR, format!("yt-dlp binary missing: {e}"))),
    };

    let mut cmd = tokio::process::Command::new(ytdlp_path);
    cmd.args(["--dump-single-json", "--no-warnings", "--no-config", "--flat-playlist"]);

    if let Some(ref js) = mdm_core::find_js_runtime() {
        let name = js.file_stem().and_then(|s| s.to_str()).unwrap_or("node");
        cmd.args(["--js-runtimes", &format!("{name}:{}", js.display())]);
    }

    let mut path_dirs = vec![mdm_core::data_dir().join("bin")];
    if let Some(ref js) = mdm_core::find_js_runtime() {
        if let Some(parent) = js.parent() {
            if !path_dirs.contains(&parent.to_path_buf()) {
                path_dirs.push(parent.to_path_buf());
            }
        }
    }
    if let Ok(cur_path) = std::env::var("PATH") {
        path_dirs.extend(std::env::split_paths(&cur_path));
    }
    if let Ok(joined) = std::env::join_paths(&path_dirs) {
        cmd.env("PATH", joined);
    }

    cmd.arg(url);

    let output = match cmd.output().await {
        Ok(o) => o,
        Err(e) => return Err((StatusCode::INTERNAL_SERVER_ERROR, format!("exec error: {e}"))),
    };

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err((StatusCode::BAD_REQUEST, format!("yt-dlp failed: {err}")));
    }

    let val: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("json parse error: {e}")))?;

    let is_pl = val.get("_type").and_then(|v| v.as_str()) == Some("playlist")
        || val.get("entries").and_then(|v| v.as_array()).map_or(false, |a| !a.is_empty());

    if !is_pl {
        return Ok(Json(PlaylistInfoResp {
            is_playlist: false,
            title: val.get("title").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            uploader: val.get("uploader").and_then(|v| v.as_str()).map(|s| s.to_string()),
            count: 0,
            single_video_url: None,
            single_video_title: None,
            entries: vec![],
        }));
    }

    let title = val.get("title").and_then(|v| v.as_str()).unwrap_or("Playlist").to_string();
    let uploader = val.get("uploader")
        .or_else(|| val.get("channel"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    // Check if the original URL had a specific video ID (e.g. watch?v=XYZ)
    let single_video_id = if let Some(q) = url.split('?').nth(1) {
        q.split('&')
            .find(|pair| pair.starts_with("v="))
            .map(|pair| pair[2..].to_string())
    } else {
        None
    };

    let mut single_video_url = None;
    let mut single_video_title = None;
    let mut entries = Vec::new();

    if let Some(arr) = val.get("entries").and_then(|v| v.as_array()) {
        for (i, item) in arr.iter().enumerate() {
            let id = item.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let item_title = item.get("title").and_then(|v| v.as_str()).unwrap_or("Untitled").to_string();
            let mut item_url = item.get("url").and_then(|v| v.as_str()).unwrap_or("").to_string();
            if !item_url.starts_with("http") && !id.is_empty() {
                item_url = format!("https://www.youtube.com/watch?v={id}");
            }

            if let Some(ref target_id) = single_video_id {
                if target_id == &id {
                    single_video_url = Some(item_url.clone());
                    single_video_title = Some(item_title.clone());
                }
            }

            let dur = item.get("duration").and_then(|v| v.as_f64());
            let dur_str = dur.map(fmt_sec_to_time);
            let item_uploader = item.get("uploader")
                .or_else(|| item.get("channel"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            let thumb = item.get("thumbnails").and_then(|v| v.as_array()).and_then(|thumbs| {
                thumbs.last().and_then(|t| t.get("url")).and_then(|u| u.as_str())
            }).or_else(|| item.get("thumbnail").and_then(|v| v.as_str()))
            .map(|s| s.to_string())
            .or_else(|| {
                if !id.is_empty() {
                    Some(format!("https://i.ytimg.com/vi/{id}/mqdefault.jpg"))
                } else {
                    None
                }
            });

            entries.push(PlaylistItem {
                index: i + 1,
                id,
                title: item_title,
                url: item_url,
                duration: dur,
                duration_string: dur_str,
                uploader: item_uploader,
                thumbnail: thumb,
            });
        }
    }

    let count = entries.len();
    Ok(Json(PlaylistInfoResp {
        is_playlist: true,
        title,
        uploader,
        count,
        single_video_url,
        single_video_title,
        entries,
    }))
}

#[derive(serde::Deserialize)]
struct BatchJobsReq {
    jobs: Vec<mdm_core::NewJobReq>,
}

#[derive(serde::Serialize)]
struct BatchJobsResp {
    added: usize,
}

async fn batch_add_jobs(
    State(st): State<AppState>,
    Json(req): Json<BatchJobsReq>,
) -> Result<Json<BatchJobsResp>, (StatusCode, String)> {
    let mut added = 0;
    for job_req in req.jobs {
        if st.queue.add(job_req).is_ok() {
            added += 1;
        }
    }
    Ok(Json(BatchJobsResp { added }))
}

// --- download settings ------------------------------------------------------

#[derive(Debug, Clone, serde::Deserialize)]
struct TorrentInfoReq {
    path: String,
}

async fn torrent_info(
    Json(req): Json<TorrentInfoReq>,
) -> Result<Json<mdm_engine_aria2::TorrentInfo>, (StatusCode, String)> {
    let p = std::path::PathBuf::from(req.path.trim());
    if !p.is_file() {
        return Err((StatusCode::BAD_REQUEST, format!("not a file: {}", p.display())));
    }
    if p.extension().and_then(|e| e.to_str()) != Some("torrent") {
        return Err((StatusCode::BAD_REQUEST, "not a .torrent file".into()));
    }
    mdm_engine_aria2::torrent_info(&p)
        .map(Json)
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))
}

#[derive(Debug, serde::Deserialize)]
struct TorrentUploadReq {
    /// Original filename, used for display and storage naming.
    name: Option<String>,
    /// Base64-encoded .torrent contents (browser reads the picked file).
    b64: String,
}

/// Store an uploaded .torrent file under the data dir and return the local
/// path it can be added from. The bytes are parsed (bencode) before storing.
async fn upload_torrent(
    State(_st): State<AppState>,
    Json(req): Json<TorrentUploadReq>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(req.b64.trim())
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("bad base64: {e}")))?;
    if bytes.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "empty torrent file".into()));
    }
    if bytes.len() > 8 * 1024 * 1024 {
        return Err((StatusCode::BAD_REQUEST, "torrent file too large (>8 MiB)".into()));
    }

    // Keep a recognizable, sanitized name; a timestamp prefix avoids collisions.
    let base = req
        .name
        .as_deref()
        .map(|n| std::path::Path::new(n).file_name().and_then(|s| s.to_str()).unwrap_or("torrent").to_string())
        .unwrap_or_else(|| "torrent.torrent".to_string());
    let mut name = base.rsplit_once('.').map(|(s, _)| s).unwrap_or(&base).replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|', ' '], "_");
    if name.len() > 64 {
        name.truncate(64);
    }
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let file_name = format!("{stamp}-{name}.torrent");

    let dir = mdm_core::data_dir().join("torrents");
    std::fs::create_dir_all(&dir)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("cannot create {}: {e}", dir.display())))?;
    let path = dir.join(&file_name);
    std::fs::write(&path, &bytes)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("cannot write {}: {e}", path.display())))?;

    // Reject junk that isn't a parseable .torrent before handing it back.
    if let Err(e) = mdm_engine_aria2::torrent_info(&path) {
        let _ = std::fs::remove_file(&path);
        return Err((StatusCode::BAD_REQUEST, format!("not a usable .torrent: {e}")));
    }

    Ok(Json(json!({ "path": path.to_string_lossy(), "name": file_name })))
}

#[derive(Debug, Clone, serde::Deserialize)]
struct ConfigUpdate {
    max_concurrent: Option<u32>,
    output_dir: Option<std::path::PathBuf>,
    seed_minutes: Option<u32>,
    max_retries: Option<u32>,
    speed_limit: Option<Option<String>>,
    proxy: Option<Option<String>>,
    write_subtitles: Option<bool>,
    write_auto_subs: Option<bool>,
    embed_metadata: Option<bool>,
    embed_thumbnail: Option<bool>,
    aria2_offload: Option<bool>,
    aria2_connections: Option<u32>,
    aria2_splits: Option<u32>,
    aria2_connect_timeout: Option<u32>,
    aria2_fallback: Option<bool>,
    aria2_disk_cache: Option<u32>,
    aria2_file_allocation: Option<String>,
    aria2_min_split_size: Option<u32>,
    aria2_max_tries: Option<u32>,
    aria2_retry_wait: Option<u32>,
    aria2_lowest_speed_limit: Option<u32>,
    aria2_bt_max_peers: Option<u32>,
    aria2_user_agent: Option<Option<String>>,
    aria2_custom_tracker_url: Option<Option<String>>,
    ytdlp_max_height: Option<u32>,
    ytdlp_merge_format: Option<String>,
    ytdlp_audio_format: Option<String>,
    ytdlp_fragments: Option<u32>,
    ytdlp_embed_subs: Option<bool>,
    theme: Option<String>,
    accent_color: Option<String>,
    widget_shape: Option<String>,
    notifications_enabled: Option<bool>,
    notification_sound: Option<bool>,
}

async fn get_config(State(st): State<AppState>) -> Json<mdm_core::Config> {
    Json(st.queue.config())
}

async fn put_config(
    State(st): State<AppState>,
    Json(up): Json<ConfigUpdate>,
) -> Result<Json<mdm_core::Config>, (StatusCode, String)> {
    let mut cfg = st.queue.config();
    if let Some(v) = up.max_concurrent {
        cfg.max_concurrent = v.clamp(1, 16);
    }
    if let Some(v) = up.max_retries {
        cfg.max_retries = v.clamp(0, 10);
    }
    if let Some(v) = up.seed_minutes {
        cfg.seed_minutes = v.clamp(0, 7 * 24 * 60);
    }
    if let Some(v) = up.speed_limit {
        if let Some(raw) = v {
            let t = raw.trim().to_string();
            let upper = t.to_ascii_uppercase();
            let num = upper.trim_end_matches(['K', 'M', 'G']);
            if t.is_empty() {
                cfg.speed_limit = None;
            } else if num.is_empty() || !num.chars().all(|c| c.is_ascii_digit()) {
                return Err((StatusCode::BAD_REQUEST, format!("speed_limit: bad value {raw:?} (use e.g. 500K, 5M, 1G)")));
            } else {
                cfg.speed_limit = Some(t);
            }
        } else {
            cfg.speed_limit = None;
        }
    }
    if let Some(proxy) = up.proxy {
        cfg.proxy = proxy;
    }
    if let Some(v) = up.write_subtitles {
        cfg.write_subtitles = v;
    }
    if let Some(v) = up.write_auto_subs {
        cfg.write_auto_subs = v;
    }
    if let Some(v) = up.embed_metadata {
        cfg.embed_metadata = v;
    }
    if let Some(v) = up.embed_thumbnail {
        cfg.embed_thumbnail = v;
    }
    if let Some(v) = up.aria2_offload {
        cfg.aria2_offload = v;
    }
    if let Some(v) = up.aria2_connections {
        cfg.aria2_connections = v.clamp(1, 16);
    }
    if let Some(v) = up.aria2_splits {
        cfg.aria2_splits = v.clamp(1, 16);
    }
    if let Some(v) = up.aria2_connect_timeout {
        cfg.aria2_connect_timeout = v.clamp(5, 300);
    }
    if let Some(v) = up.aria2_fallback {
        cfg.aria2_fallback = v;
    }
    if let Some(v) = up.aria2_disk_cache {
        cfg.aria2_disk_cache = v.min(1024);
    }
    if let Some(v) = up.aria2_file_allocation {
        let s = v.trim().to_ascii_lowercase();
        if matches!(s.as_str(), "none" | "prealloc" | "trunc" | "falloc") {
            cfg.aria2_file_allocation = s;
        }
    }
    if let Some(v) = up.aria2_min_split_size {
        cfg.aria2_min_split_size = v.clamp(1, 1024);
    }
    if let Some(v) = up.aria2_max_tries {
        cfg.aria2_max_tries = v.min(20);
    }
    if let Some(v) = up.aria2_retry_wait {
        cfg.aria2_retry_wait = v.min(60);
    }
    if let Some(v) = up.aria2_lowest_speed_limit {
        cfg.aria2_lowest_speed_limit = v.min(10240);
    }
    if let Some(v) = up.aria2_bt_max_peers {
        cfg.aria2_bt_max_peers = v.min(500);
    }
    if let Some(v) = up.aria2_user_agent {
        cfg.aria2_user_agent = v.unwrap_or_default().trim().to_string();
    }
    if let Some(v) = up.aria2_custom_tracker_url {
        cfg.aria2_custom_tracker_url = v.unwrap_or_default().trim().to_string();
    }
    if let Some(v) = up.ytdlp_max_height {
        if matches!(v, 360 | 480 | 720 | 1080 | 1440 | 2160) || v == 0 {
            cfg.ytdlp_max_height = v;
        }
    }
    if let Some(v) = up.ytdlp_merge_format {
        let s = v.trim().to_ascii_lowercase();
        if matches!(s.as_str(), "mkv" | "mp4" | "webm") {
            cfg.ytdlp_merge_format = s;
        }
    }
    if let Some(v) = up.ytdlp_audio_format {
        let s = v.trim().to_ascii_lowercase();
        if matches!(s.as_str(), "mp3" | "m4a" | "opus" | "flac" | "wav" | "vorbis" | "best") {
            cfg.ytdlp_audio_format = s;
        }
    }
    if let Some(v) = up.ytdlp_fragments {
        cfg.ytdlp_fragments = v.clamp(1, 16);
    }
    if let Some(v) = up.ytdlp_embed_subs {
        cfg.ytdlp_embed_subs = v;
    }
    if let Some(t) = up.theme {
        let t_clean = t.trim().to_lowercase();
        if matches!(t_clean.as_str(), "dark" | "light" | "system") {
            cfg.theme = t_clean;
        }
    }
    if let Some(c) = up.accent_color {
        let c_clean = c.trim().to_lowercase();
        if c_clean.starts_with('#') && (c_clean.len() == 4 || c_clean.len() == 7) {
            cfg.accent_color = c_clean;
        }
    }
    if let Some(s) = up.widget_shape {
        let s_clean = s.trim().to_lowercase();
        if matches!(s_clean.as_str(), "pill" | "card" | "minimalistic") {
            cfg.widget_shape = s_clean;
        }
    }
    if let Some(v) = up.notifications_enabled {
        cfg.notifications_enabled = v;
    }
    if let Some(v) = up.notification_sound {
        cfg.notification_sound = v;
    }
    if let Some(dir) = up.output_dir {
        if dir.as_os_str().is_empty() {
            return Err((StatusCode::BAD_REQUEST, "output_dir must not be empty".into()));
        }
        if let Err(e) = std::fs::create_dir_all(&dir) {
            return Err((StatusCode::BAD_REQUEST, format!("cannot create {}: {e}", dir.display())));
        }
        cfg.output_dir = dir;
    }
    mdm_core::save_config(&cfg).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    st.queue.replace_config(cfg.clone());
    st.queue.notify_engines(&cfg).await;
    Ok(Json(cfg))
}

fn err_reply(e: mdm_core::Error) -> (StatusCode, String) {
    (StatusCode::BAD_REQUEST, e.to_string())
}

async fn health(State(st): State<AppState>) -> impl IntoResponse {
    let queue = st.queue.db().count_queued().unwrap_or(-1);
    let running = st.queue.db().count_running().unwrap_or(-1);
    Json(json!({
        "ok": true,
        "version": env!("CARGO_PKG_VERSION"),
        "queued": queue,
        "running": running,
    }))
}

async fn add_job(
    State(st): State<AppState>,
    Json(req): Json<NewJobReq>,
) -> Result<(StatusCode, Json<Job>), (StatusCode, String)> {
    st.queue
        .add(req)
        .map(|job| (StatusCode::CREATED, Json(job)))
        .map_err(err_reply)
}

async fn list_jobs(
    State(st): State<AppState>,
) -> Result<Json<Vec<Job>>, (StatusCode, String)> {
    st.queue.list(None).map(Json).map_err(err_reply)
}

async fn get_job(
    State(st): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Job>, (StatusCode, String)> {
    st.queue
        .get(&id)
        .map_err(err_reply)?
        .map(Json)
        .ok_or((StatusCode::NOT_FOUND, format!("job {id} not found")))
}

async fn delete_job(
    State(st): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    st.queue.remove(&id).await.map_err(err_reply)?;
    Ok(Json(json!({ "removed": id })))
}

async fn pause_job(
    State(st): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    st.queue.pause(&id).await.map_err(err_reply)?;
    Ok(Json(json!({ "paused": id })))
}

async fn resume_job(
    State(st): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    st.queue.resume(&id).await.map_err(err_reply)?;
    Ok(Json(json!({ "resumed": id })))
}

async fn pause_all_jobs(
    State(st): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let count = st.queue.pause_all().await.map_err(err_reply)?;
    Ok(Json(json!({ "paused": count })))
}

async fn resume_all_jobs(
    State(st): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let count = st.queue.resume_all().await.map_err(err_reply)?;
    Ok(Json(json!({ "resumed": count })))
}

fn reveal_in_folder(path: &std::path::Path) -> std::io::Result<()> {
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(format!("/select,\"{}\"", path.display()))
            .spawn()?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg("-R")
            .arg(path)
            .spawn()?;
    }
    #[cfg(target_os = "linux")]
    {
        let parent = if path.is_file() {
            path.parent().unwrap_or(path)
        } else {
            path
        };
        let mut launched = false;
        if path.is_file() {
            if let Ok(canon) = path.canonicalize().or_else(|_| Ok::<_, std::io::Error>(path.to_path_buf())) {
                let uri = format!("file://{}", canon.display());
                if std::process::Command::new("dbus-send")
                    .args([
                        "--session",
                        "--dest=org.freedesktop.FileManager1",
                        "--type=method_call",
                        "/org/freedesktop/FileManager1",
                        "org.freedesktop.FileManager1.ShowItems",
                        &format!("array:string:{uri}"),
                        "string:",
                    ])
                    .output()
                    .map(|out| out.status.success())
                    .unwrap_or(false)
                {
                    launched = true;
                }
            }
            if !launched {
                if std::process::Command::new("nautilus")
                    .arg("--select")
                    .arg(path)
                    .spawn()
                    .is_ok()
                {
                    launched = true;
                }
            }
        }
        if !launched {
            std::process::Command::new("xdg-open")
                .arg(parent)
                .spawn()?;
        }
    }
    Ok(())
}

async fn open_job_folder(
    State(st): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, (StatusCode, String)> {
    let job = st
        .queue
        .get(&id)
        .map_err(err_reply)?
        .ok_or((StatusCode::NOT_FOUND, format!("job {id} not found")))?;

    let target_path = if let Some(p) = job.output_path.as_ref().filter(|p| p.exists()) {
        p.clone()
    } else if let Some(title) = &job.title {
        let candidate = job.output_dir.join(title);
        if candidate.exists() {
            candidate
        } else {
            job.output_dir.clone()
        }
    } else {
        job.output_dir.clone()
    };

    if !target_path.exists() {
        return Err((StatusCode::NOT_FOUND, format!("path does not exist: {}", target_path.display())));
    }

    reveal_in_folder(&target_path)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("failed to open file manager: {e}")))?;

    Ok(StatusCode::OK)
}

async fn ws_events(
    ws: WebSocketUpgrade,
    State(st): State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| ws_forward(socket, st))
}

async fn ws_forward(mut socket: WebSocket, st: AppState) {
    // Initial snapshot.
    match st.queue.list(None) {
        Ok(jobs) => {
            let payload = json!({ "type": "snapshot", "jobs": jobs }).to_string();
            if socket.send(Message::Text(payload.into())).await.is_err() {
                return;
            }
        }
        Err(_) => return,
    }

    let mut rx = st.queue.subscribe_events();
    loop {
        tokio::select! {
            biased;
            ev = rx.recv() => {
                match ev {
                    Ok(event) => {
                        let text = match &event {
                            Event::JobUpdated { job } => json!({ "type": "job_updated", "job": job }).to_string(),
                            Event::JobRemoved { id } => json!({ "type": "job_removed", "id": id }).to_string(),
                            Event::QueueIdle => json!({ "type": "queue_idle" }).to_string(),
                        };
                        if socket.send(Message::Text(text.into())).await.is_err() {
                            return;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
                }
            }
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Ping(_))) | Some(Ok(Message::Pong(_))) => {}
                    _ => return, // closed or error
                }
            }
        }
    }
}

// Keep JobState referenced (Phase 1 okay) to avoid dead import warnings.
#[allow(dead_code)]
fn _uses(s: JobState) -> &'static str {
    s.as_str()
}
