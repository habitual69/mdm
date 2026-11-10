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
    Show,
    SetAutostart { enabled: bool },
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
        .route("/api/window/show", post(window_show))
        .route("/api/window/toggle-floating", post(window_toggle_floating))
        .route("/api/window/resize-floating", post(window_resize_floating))
        .route("/api/window/state", get(window_state))
        .route("/api/media-info", post(media_info))
        .route("/api/playlist-info", post(playlist_info))
        .route("/api/jobs/batch", post(batch_add_jobs))
        .route("/api/bins/status", get(get_bins_status))
        .route("/api/bins/update", post(update_bin))
        .route("/api/extension/status", get(get_extension_status))
        .route("/api/extension/open-browser", post(open_browser_extension))
        .route("/api/extension/open-folder", post(open_extension_folder))
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
                                "Download Complete — mDm",
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
                                "Download Failed — mDm",
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

async fn window_show(State(st): State<AppState>) -> StatusCode {
    if let Some(tx) = &st.window_tx {
        let _ = tx.send(WindowAction::Show);
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

// --- binary management -----------------------------------------------------

#[derive(serde::Deserialize)]
struct UpdateBinReq {
    name: String,
}

async fn get_bins_status(State(st): State<AppState>) -> Json<Vec<mdm_bins::BinStatus>> {
    let cfg = st.queue.config();
    let installer = mdm_bins::Installer::new(&cfg);
    Json(installer.status_all(&cfg).await)
}

async fn update_bin(
    State(st): State<AppState>,
    Json(req): Json<UpdateBinReq>,
) -> (StatusCode, Json<serde_json::Value>) {
    let cfg = st.queue.config();
    let installer = mdm_bins::Installer::new(&cfg);
    let target = req.name.trim();
    if target != "yt-dlp" && target != "aria2c" && target != "ffmpeg" {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "ok": false,
                "error": format!("unsupported binary: {target}")
            })),
        );
    }
    let res = installer.install(target).await;
    match res.action {
        mdm_bins::BinAction::Installed { version, path, sha_ok } => {
            (
                StatusCode::OK,
                Json(json!({
                    "ok": true,
                    "name": target,
                    "version": version,
                    "path": path,
                    "sha_ok": sha_ok,
                    "message": format!("Successfully updated {target} to {version}")
                })),
            )
        }
        mdm_bins::BinAction::AlreadyOk { version, path } => {
            (
                StatusCode::OK,
                Json(json!({
                    "ok": true,
                    "name": target,
                    "version": version,
                    "path": path,
                    "message": format!("{target} is already up to date ({version})")
                })),
            )
        }
        mdm_bins::BinAction::Failed { error } => {
            (
                StatusCode::OK,
                Json(json!({
                    "ok": false,
                    "name": target,
                    "error": error.clone(),
                    "message": format!("Update failed: {error}")
                })),
            )
        }
        mdm_bins::BinAction::Skipped { reason } => {
            (
                StatusCode::OK,
                Json(json!({
                    "ok": false,
                    "name": target,
                    "error": reason.clone(),
                    "message": format!("Update skipped: {reason}")
                })),
            )
        }
    }
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
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    cmd.args(["--dump-single-json", "--no-warnings", "--no-config", "--no-check-certificates", "--flat-playlist"]);

    if url.contains("youtube.com") || url.contains("youtu.be") {
        cmd.args(["--extractor-args", "youtube:player_client=default,web,android"]);
    }

    if let Some(ref js) = mdm_core::find_js_runtime() {
        let name = js.file_stem().and_then(|s| s.to_str()).unwrap_or("node");
        cmd.args(["--js-runtimes", &format!("{name}:{}", js.display())]);
    }

    let mut path_dirs = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            path_dirs.push(parent.join("bin"));
            path_dirs.push(parent.join("resources").join("bin"));
            path_dirs.push(parent.to_path_buf());
        }
    }
    path_dirs.push(mdm_core::data_dir().join("bin"));
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
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    cmd.args(["--dump-single-json", "--no-warnings", "--no-config", "--no-check-certificates", "--flat-playlist"]);

    if url.contains("youtube.com") || url.contains("youtu.be") {
        cmd.args(["--extractor-args", "youtube:player_client=default,web,android"]);
    }

    if let Some(ref js) = mdm_core::find_js_runtime() {
        let name = js.file_stem().and_then(|s| s.to_str()).unwrap_or("node");
        cmd.args(["--js-runtimes", &format!("{name}:{}", js.display())]);
    }

    let mut path_dirs = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            path_dirs.push(parent.join("bin"));
            path_dirs.push(parent.join("resources").join("bin"));
            path_dirs.push(parent.to_path_buf());
        }
    }
    path_dirs.push(mdm_core::data_dir().join("bin"));
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
    autostart: Option<bool>,
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
    let autostart_changed = match up.autostart {
        Some(v) => v != cfg.autostart,
        None => false,
    };
    if let Some(v) = up.autostart {
        cfg.autostart = v;
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
    if autostart_changed {
        if let Some(tx) = &st.window_tx {
            let _ = tx.send(WindowAction::SetAutostart { enabled: cfg.autostart });
        }
    }
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
        use std::os::windows::process::CommandExt;
        let mut cmd = std::process::Command::new("explorer.exe");
        let norm = path.to_string_lossy().replace('/', "\\");
        if path.is_file() {
            cmd.raw_arg(format!("/select,\"{norm}\""));
        } else {
            cmd.raw_arg(format!("\"{norm}\""));
        }
        cmd.spawn()?;
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
        if let Some(parent) = target_path.parent() {
            let _ = std::fs::create_dir_all(parent);
            if parent.exists() {
                return reveal_in_folder(parent)
                    .map(|_| StatusCode::OK)
                    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("failed to open file manager: {e}")));
            }
        }
        let _ = std::fs::create_dir_all(&job.output_dir);
        return reveal_in_folder(&job.output_dir)
            .map(|_| StatusCode::OK)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("failed to open file manager: {e}")));
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

// --- browser extension management & detection -----------------------------

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DetectedBrowser {
    pub id: String,
    pub name: String,
    pub installed: bool,
    pub exe_path: Option<String>,
    pub extensions_url: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ExtensionStatusResp {
    pub folder_exists: bool,
    pub folder_path: Option<String>,
    pub manifest_exists: bool,
    pub version: Option<String>,
    pub browsers: Vec<DetectedBrowser>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct OpenBrowserReq {
    pub browser: String,
}

fn find_extension_dir() -> Option<std::path::PathBuf> {
    // 1. Next to current executable (installed or portable package)
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            let candidate = parent.join("extension");
            if candidate.join("manifest.json").exists() {
                return Some(candidate);
            }
        }
    }

    // 2. Current working directory or relative project roots
    if let Ok(cwd) = std::env::current_dir() {
        let candidate = cwd.join("extension");
        if candidate.join("manifest.json").exists() {
            return Some(candidate);
        }
        if let Some(p) = cwd.parent() {
            let candidate = p.join("extension");
            if candidate.join("manifest.json").exists() {
                return Some(candidate);
            }
            if let Some(gp) = p.parent() {
                let candidate = gp.join("extension");
                if candidate.join("manifest.json").exists() {
                    return Some(candidate);
                }
            }
        }
    }

    // 3. Fallback to default user install path (%LOCALAPPDATA%\Programs\mDm\extension)
    if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
        let candidate = std::path::PathBuf::from(local_app_data)
            .join("Programs")
            .join("mDm")
            .join("extension");
        if candidate.join("manifest.json").exists() {
            return Some(candidate);
        }
    }

    None
}

fn detect_installed_browsers() -> Vec<DetectedBrowser> {
    let mut browsers = Vec::new();

    #[cfg(target_os = "windows")]
    {
        let local_app_data = std::env::var("LOCALAPPDATA").unwrap_or_default();
        let prog_files = std::env::var("ProgramFiles").unwrap_or_default();
        let prog_files_x86 = std::env::var("ProgramFiles(x86)").unwrap_or_default();

        // 1. Google Chrome
        let chrome_candidates = [
            format!("{local_app_data}\\Google\\Chrome\\Application\\chrome.exe"),
            format!("{prog_files}\\Google\\Chrome\\Application\\chrome.exe"),
            format!("{prog_files_x86}\\Google\\Chrome\\Application\\chrome.exe"),
        ];
        let chrome_path = chrome_candidates.into_iter().find(|p| std::path::Path::new(p).exists());
        browsers.push(DetectedBrowser {
            id: "chrome".to_string(),
            name: "Google Chrome".to_string(),
            installed: chrome_path.is_some(),
            exe_path: chrome_path,
            extensions_url: "chrome://extensions".to_string(),
        });

        // 2. Microsoft Edge
        let edge_candidates = [
            format!("{prog_files_x86}\\Microsoft\\Edge\\Application\\msedge.exe"),
            format!("{prog_files}\\Microsoft\\Edge\\Application\\msedge.exe"),
            format!("{local_app_data}\\Microsoft\\Edge\\Application\\msedge.exe"),
        ];
        let edge_path = edge_candidates.into_iter().find(|p| std::path::Path::new(p).exists());
        browsers.push(DetectedBrowser {
            id: "edge".to_string(),
            name: "Microsoft Edge".to_string(),
            installed: edge_path.is_some(),
            exe_path: edge_path,
            extensions_url: "edge://extensions".to_string(),
        });

        // 3. Brave
        let brave_candidates = [
            format!("{local_app_data}\\BraveSoftware\\Brave-Browser\\Application\\brave.exe"),
            format!("{prog_files}\\BraveSoftware\\Brave-Browser\\Application\\brave.exe"),
            format!("{prog_files_x86}\\BraveSoftware\\Brave-Browser\\Application\\brave.exe"),
        ];
        let brave_path = brave_candidates.into_iter().find(|p| std::path::Path::new(p).exists());
        browsers.push(DetectedBrowser {
            id: "brave".to_string(),
            name: "Brave Browser".to_string(),
            installed: brave_path.is_some(),
            exe_path: brave_path,
            extensions_url: "brave://extensions".to_string(),
        });

        // 4. Opera
        let opera_candidates = [
            format!("{local_app_data}\\Programs\\Opera\\launcher.exe"),
            format!("{prog_files}\\Opera\\launcher.exe"),
            format!("{prog_files_x86}\\Opera\\launcher.exe"),
        ];
        let opera_path = opera_candidates.into_iter().find(|p| std::path::Path::new(p).exists());
        browsers.push(DetectedBrowser {
            id: "opera".to_string(),
            name: "Opera".to_string(),
            installed: opera_path.is_some(),
            exe_path: opera_path,
            extensions_url: "opera://extensions".to_string(),
        });

        // 5. Vivaldi
        let vivaldi_candidates = [
            format!("{local_app_data}\\Vivaldi\\Application\\vivaldi.exe"),
            format!("{prog_files}\\Vivaldi\\Application\\vivaldi.exe"),
        ];
        let vivaldi_path = vivaldi_candidates.into_iter().find(|p| std::path::Path::new(p).exists());
        browsers.push(DetectedBrowser {
            id: "vivaldi".to_string(),
            name: "Vivaldi".to_string(),
            installed: vivaldi_path.is_some(),
            exe_path: vivaldi_path,
            extensions_url: "vivaldi://extensions".to_string(),
        });

        // 6. Mozilla Firefox
        let firefox_candidates = [
            format!("{prog_files}\\Mozilla Firefox\\firefox.exe"),
            format!("{prog_files_x86}\\Mozilla Firefox\\firefox.exe"),
        ];
        let firefox_path = firefox_candidates.into_iter().find(|p| std::path::Path::new(p).exists());
        browsers.push(DetectedBrowser {
            id: "firefox".to_string(),
            name: "Mozilla Firefox".to_string(),
            installed: firefox_path.is_some(),
            exe_path: firefox_path,
            extensions_url: "about:addons".to_string(),
        });
    }

    #[cfg(not(target_os = "windows"))]
    {
        browsers.push(DetectedBrowser {
            id: "chrome".to_string(),
            name: "Google Chrome".to_string(),
            installed: true,
            exe_path: Some("google-chrome".to_string()),
            extensions_url: "chrome://extensions".to_string(),
        });
    }

    browsers
}

async fn get_extension_status() -> Result<Json<ExtensionStatusResp>, (StatusCode, String)> {
    let ext_dir = find_extension_dir();
    let folder_exists = ext_dir.is_some();
    let manifest_exists = ext_dir.as_ref().map(|p| p.join("manifest.json").exists()).unwrap_or(false);

    let mut version = None;
    if let Some(ref dir) = ext_dir {
        let manifest_file = dir.join("manifest.json");
        if let Ok(content) = std::fs::read_to_string(&manifest_file) {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&content) {
                version = val.get("version").and_then(|v| v.as_str()).map(|s| s.to_string());
            }
        }
    }

    let browsers = detect_installed_browsers();
    let folder_path = ext_dir.map(|p| p.to_string_lossy().to_string());

    Ok(Json(ExtensionStatusResp {
        folder_exists,
        folder_path,
        manifest_exists,
        version,
        browsers,
    }))
}

async fn open_browser_extension(
    Json(req): Json<OpenBrowserReq>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let browsers = detect_installed_browsers();
    let target = browsers.into_iter().find(|b| b.id.eq_ignore_ascii_case(&req.browser) || b.name.eq_ignore_ascii_case(&req.browser))
        .ok_or((StatusCode::NOT_FOUND, format!("Browser '{}' not found or unsupported", req.browser)))?;

    let exe = target.exe_path.ok_or((StatusCode::BAD_REQUEST, format!("{} is not installed on this system", target.name)))?;

    // Reveal extension directory in Explorer so user can easily drag-and-drop or select it
    if let Some(ext_dir) = find_extension_dir() {
        let _ = reveal_in_folder(&ext_dir);
    }

    // Launch the browser with its extensions URL
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new(&exe)
            .arg(&target.extensions_url)
            .spawn()
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to launch {}: {e}", target.name)))?;
    }
    #[cfg(not(target_os = "windows"))]
    {
        std::process::Command::new(&exe)
            .arg(&target.extensions_url)
            .spawn()
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to launch {}: {e}", target.name)))?;
    }

    Ok(Json(json!({
        "ok": true,
        "browser": target.name,
        "url": target.extensions_url,
        "message": format!("Launched {} and revealed extension folder in Explorer", target.name)
    })))
}

async fn open_extension_folder() -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let ext_dir = find_extension_dir().ok_or((StatusCode::NOT_FOUND, "Extension folder not found on disk".to_string()))?;
    reveal_in_folder(&ext_dir).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to open folder: {e}")))?;
    Ok(Json(json!({
        "ok": true,
        "path": ext_dir.to_string_lossy().to_string()
    })))
}

// Keep JobState referenced (Phase 1 okay) to avoid dead import warnings.
#[allow(dead_code)]
fn _uses(s: JobState) -> &'static str {
    s.as_str()
}
