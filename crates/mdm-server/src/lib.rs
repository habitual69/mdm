//! HTTP API around the queue. Binds 127.0.0.1 in Phase 1.

use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use mdm_core::{Config, Event, Job, JobState, NewJobReq, Queue};
use mdm_engine_aria2::Aria2Engine;
use mdm_engine_ytdlp::Ytdlp;
use serde_json::json;

pub fn build_queue(cfg: Config) -> mdm_core::Result<Arc<Queue>> {
    let ytdlp = Ytdlp::from_config(&cfg)?;
    let data_dir = mdm_core::data_dir();
    let aria2 = Aria2Engine::new(cfg.clone(), data_dir);
    let engines: Vec<Arc<dyn mdm_core::Engine>> =
        vec![Arc::new(ytdlp), Arc::new(aria2)];
    Queue::new(cfg, engines)
}

#[derive(Clone)]
pub struct AppState {
    pub queue: Arc<Queue>,
}

pub fn router(q: Arc<Queue>) -> Router {
    Router::new()
        .route("/", get(ui_index))
        .route("/static/{*path}", get(ui_static))
        .route("/api/health", get(health))
        .route("/api/config", get(get_config).put(put_config))
        .route("/api/jobs", post(add_job).get(list_jobs))
        .route("/api/jobs/{id}", get(get_job).delete(delete_job))
        .route("/api/jobs/{id}/pause", post(pause_job))
        .route("/api/jobs/{id}/resume", post(resume_job))
        .route("/api/events", get(ws_events))
        .with_state(AppState { queue: q })
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

pub async fn serve(q: Arc<Queue>, addr: &str) -> std::io::Result<()> {
    serve_with_shutdown(q, addr, std::future::pending()).await
}

pub async fn serve_with_shutdown(
    q: Arc<Queue>,
    addr: &str,
    shutdown: impl std::future::Future<Output = ()> + Send + 'static,
) -> std::io::Result<()> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("mdm server listening on http://{addr}");
    axum::serve(listener, router(q))
        .with_graceful_shutdown(shutdown)
        .await
}

// --- download settings ------------------------------------------------------

#[derive(Debug, Clone, serde::Deserialize)]
struct ConfigUpdate {
    max_concurrent: Option<u32>,
    output_dir: Option<std::path::PathBuf>,
    seed_minutes: Option<u32>,
    max_retries: Option<u32>,
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
