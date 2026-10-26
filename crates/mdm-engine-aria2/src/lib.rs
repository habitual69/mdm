//! aria2 engine: torrent / magnet / direct downloads through the RPC daemon.

mod daemon;
mod rpc;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use base64::Engine as _;
use mdm_core::{
    Config, Engine, EngineCtx, EngineKind, Job, JobDone, JobError, JobPatch, JobState, ResolveInfo,
};
use serde_json::{json, Value};
use tracing::warn;

pub mod torrent;

pub use daemon::{Aria2Daemon, DaemonHandle};
pub use rpc::RpcError;
pub use torrent::{TorrentInfo, torrent_info};

pub struct Aria2Engine {
    pub daemon: DaemonHandle,
    active_gids: Arc<Mutex<HashMap<String, String>>>,
}

impl Aria2Engine {
    pub fn new(cfg: Config, data_dir: std::path::PathBuf) -> Aria2Engine {
        Aria2Engine {
            daemon: DaemonHandle::new(cfg, data_dir),
            active_gids: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    async fn add_job(&self, job: &Job, cfg: &Config) -> std::result::Result<String, JobError> {
        let daemon = self.daemon.get().await.map_err(|e| JobError::Fatal(e.to_string()))?;
        let rpc = rpc::Aria2Rpc::new(daemon.rpc_addr.clone(), daemon.secret.clone());
        let dir = job.output_dir.display().to_string();

        let gid = match job.kind {
            mdm_core::JobKind::Torrent => {
                // Support both http URLs and local .torrent files.
                let path = std::path::PathBuf::from(&job.url);
                if path.is_file() {
                    let bytes = std::fs::read(&path)
                        .map_err(|e| JobError::Fatal(format!("read torrent: {e}")))?;
                    let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
                    rpc.add_torrent(&b64, &dir, select_file_opts(job, cfg)).await
                } else {
                    rpc.add_uri(vec![job.url.clone()], &dir, select_file_opts(job, cfg)).await
                }
            }
            mdm_core::JobKind::Direct | mdm_core::JobKind::Magnet => {
                rpc.add_uri(vec![job.url.clone()], &dir, select_file_opts(job, cfg)).await
            }
            // Video/audio never route here in Phase 1.
            _ => rpc.add_uri(vec![job.url.clone()], &dir, select_file_opts(job, cfg)).await,
        };
        gid.map_err(map_rpc_err)
    }
}

/// aria2 selects files by 1-based index (`select-file=3,7`), and carries the
/// per-download network knobs from settings (connections, splits, timeouts).
fn select_file_opts(job: &mdm_core::Job, cfg: &Config) -> serde_json::Value {
    let mut opts = match &job.select_files {
        Some(list) if !list.is_empty() => json!({
            "select-file": list.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(",")
        }),
        _ => json!({}),
    };
    if let Some(obj) = opts.as_object_mut() {
        if let Some(title) = &job.title {
            let t = title.trim();
            if !t.is_empty() {
                obj.insert("out".into(), json!(t));
            }
        }
        obj.insert(
            "max-connection-per-server".into(),
            json!(cfg.aria2_connections.clamp(1, 16).to_string()),
        );
        obj.insert(
            "split".into(),
            json!(cfg.aria2_splits.clamp(1, 16).to_string()),
        );
        obj.insert(
            "connect-timeout".into(),
            json!(cfg.aria2_connect_timeout.clamp(5, 300).to_string()),
        );
        obj.insert("use-head".into(), json!("false"));
        let ua = cfg.aria2_user_agent.trim();
        if !ua.is_empty() && !ua.contains("146.0.0.0") {
            obj.insert("user-agent".into(), json!(ua));
        }
    }
    opts
}

impl Aria2Engine {
    async fn reattach_or_add(&self, job: &Job, cfg: &Config) -> std::result::Result<String, JobError> {
        let daemon = self.daemon.get().await.map_err(|e| JobError::Fatal(e.to_string()))?;
        let rpc = rpc::Aria2Rpc::new(daemon.rpc_addr.clone(), daemon.secret.clone());

        if let Some(gid) = job.external_id.clone() {
            match rpc.tell_status(&gid).await {
                Ok(v) => {
                    let status = v.get("status").and_then(|s| s.as_str()).unwrap_or("");
                    match status {
                        "paused" => {
                            if rpc.unpause(&gid).await.is_ok() {
                                return Ok(gid);
                            }
                        }
                        "active" | "waiting" => return Ok(gid),
                        "complete" => {
                            // Seeding finished previously?
                            return Ok(gid);
                        }
                        _ => {}
                    }
                }
                Err(_) => {
                    // gid not known to this daemon session: fall through to re-add.
                }
            }
        }
        self.add_job(job, cfg).await
    }
}

fn map_rpc_err(e: RpcError) -> JobError {
    match e {
        RpcError::Rpc(msg) => JobError::Retry(format!("aria2 rpc: {msg}")),
        RpcError::Http(msg) => JobError::Retry(format!("aria2 http: {msg}")),
    }
}

fn u64_of(v: &Value, key: &str) -> u64 {
    v.get(key).and_then(|x| x.as_str()).and_then(|s| s.parse::<u64>().ok()).unwrap_or(0)
}

fn str_of<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(|x| x.as_str())
}

fn map_status_values(
    v: &Value,
) -> (String, u64, u64, u64, Option<i64>) {
    let status = str_of(v, "status").unwrap_or("").to_string();
    let total = u64_of(v, "totalLength");
    let done = u64_of(v, "completedLength");
    let speed = u64_of(v, "downloadSpeed");
    let eta = if speed > 0 && total > done {
        Some(((total - done) / speed) as i64)
    } else {
        None
    };
    (status, total, done, speed, eta)
}

impl Aria2Engine {
    /// Push app-level speed cap / proxy to aria2's global options. Empty
    /// strings clear existing limits ("0" = unlimited download rate).
    async fn apply_globals(&self, cfg: &Config) -> std::result::Result<(), String> {
        let daemon = self.daemon.get().await.map_err(|e| e.to_string())?;
        let rpc = rpc::Aria2Rpc::new(daemon.rpc_addr.clone(), daemon.secret.clone());
        let limit = cfg.speed_limit.as_deref().map(str::trim).filter(|s| !s.is_empty()).unwrap_or("0");
        let proxy = cfg.proxy.as_deref().map(str::trim).filter(|s| !s.is_empty()).unwrap_or("");
        rpc.change_global(limit, proxy)
            .await
            .map_err(|e| e.to_string())
    }
}

#[async_trait]
impl Engine for Aria2Engine {
    fn engine_kind(&self) -> EngineKind {
        EngineKind::Aria2
    }

    async fn update_config(&self, cfg: &Config) {
        if let Err(e) = self.apply_globals(cfg).await {
            warn!("aria2 global options not applied: {e}");
        }
    }

    async fn resolve(&self, _job: &Job, _ctx: &EngineCtx) -> mdm_core::Result<ResolveInfo> {
        // Magnets/torrents: no cheap metadata until metadata exchange; report as-is.
        Ok(ResolveInfo {
            title: None,
            n_items: None,
        })
    }

    async fn run(&self, job: Job, ctx: EngineCtx) -> std::result::Result<JobDone, JobError> {
        // Re-assert speed cap / proxy with this run's config snapshot.
        if let Err(e) = self.apply_globals(&ctx.cfg).await {
            warn!("aria2 global options not applied: {e}");
        }

        // Local .torrent file: surface the display name as the job title.
        if job.kind == mdm_core::JobKind::Torrent && job.title.is_none() {
            if let Ok(path) = std::path::PathBuf::from(&job.url).canonicalize()
                && let Ok(info) = torrent_info(&path)
                && !info.name.is_empty()
            {
                ctx.send_patch(
                    &job.id,
                    JobPatch { title: Some(info.name), ..Default::default() },
                );
            }
        }

        let gid = self.reattach_or_add(&job, &ctx.cfg).await?;

        self.active_gids
            .lock()
            .unwrap()
            .insert(job.id.clone(), gid.clone());

        struct ActiveGidGuard {
            job_id: String,
            active_gids: Arc<Mutex<HashMap<String, String>>>,
        }
        impl Drop for ActiveGidGuard {
            fn drop(&mut self) {
                let _ = self.active_gids.lock().unwrap().remove(&self.job_id);
            }
        }
        let _gid_guard = ActiveGidGuard {
            job_id: job.id.clone(),
            active_gids: Arc::clone(&self.active_gids),
        };

        // Persist our tracking gid early so a crash keeps the linkage.
        ctx.send_patch(
            &job.id,
            JobPatch {
                external_id: Some(gid.clone()),
                state: Some(JobState::Running),
                ..Default::default()
            },
        );

        let daemon = self.daemon.get().await.map_err(|e| JobError::Fatal(e.to_string()))?;
        let rpc = Arc::new(rpc::Aria2Rpc::new(daemon.rpc_addr.clone(), daemon.secret.clone()));
        let mut tracking_gid = gid.clone();

        let mut interval = tokio::time::interval(Duration::from_millis(900));
        interval.tick().await; // first tick immediate
        loop {
            tokio::select! {
                biased;
                _ = ctx.cancel.cancelled() => {
                    let _ = rpc.force_pause(&tracking_gid).await;
                    return Err(JobError::Cancelled);
                }
                _ = interval.tick() => {
                    let status = match rpc.tell_status(&tracking_gid).await {
                        Ok(v) => v,
                        Err(e) => {
                            warn!("tellStatus {}: {e}", job.short_id());
                            let err_msg = e.to_string();
                            if err_msg.contains("not found") || err_msg.contains("not recognized") {
                                if let Some(title) = &job.title {
                                    let target_file = job.output_dir.join(title);
                                    let aria2_file = job.output_dir.join(format!("{}.aria2", title));
                                    if target_file.is_file() && !aria2_file.is_file() {
                                        let size = target_file.metadata().map(|m| m.len() as i64).unwrap_or(0);
                                        if size > 0 {
                                            ctx.send_patch(
                                                &job.id,
                                                JobPatch {
                                                    state: Some(JobState::Done),
                                                    progress: Some(100.0),
                                                    downloaded_bytes: Some(size),
                                                    total_bytes: Some(size),
                                                    speed_bps: Some(0),
                                                    upload_speed_bps: Some(0),
                                                    output_path: Some(target_file.clone()),
                                                    ..Default::default()
                                                },
                                            );
                                            return Ok(JobDone { output_path: Some(target_file), title: job.title.clone() });
                                        }
                                    }
                                }
                            }
                            continue;
                        }
                    };

                    // Magnet metadata resolution hands off to a child gid.
                    if let Some(children) = status.get("followedBy").and_then(|f| f.as_array())
                        && let Some(child_gid) = children.first().and_then(|c| c.as_str())
                        && child_gid != tracking_gid
                    {
                        tracking_gid = child_gid.to_string();
                        self.active_gids
                            .lock()
                            .unwrap()
                            .insert(job.id.clone(), tracking_gid.clone());
                        ctx.send_patch(
                            &job.id,
                            JobPatch {
                                external_id: Some(tracking_gid.clone()),
                                ..Default::default()
                            },
                        );
                        continue;
                    }

                    let (status_str, total, done, speed, eta) = map_status_values(&status);

                    // Metadata exchange (magnet) phase: total==0 while active.
                    if status_str == "active" && total == 0 {
                        ctx.send_patch(&job.id, JobPatch {
                            state: Some(JobState::Running),
                            progress: Some(0.0),
                            downloaded_bytes: Some(0),
                            total_bytes: Some(0),
                            speed_bps: Some(0),
                            ..Default::default()
                        });
                        continue;
                    }

                    let up_speed = status
                        .get("uploadSpeed")
                        .and_then(|x| x.as_str())
                        .and_then(|s| s.parse::<i64>().ok())
                        .unwrap_or(0);

                    match status_str.as_str() {
                        "active" | "waiting" => {
                            let pct = if total > 0 { (done as f32 / total as f32) * 100.0 } else { 0.0 };
                            ctx.send_patch(&job.id, JobPatch {
                                state: Some(JobState::Running),
                                progress: Some(pct),
                                downloaded_bytes: Some(done as i64),
                                total_bytes: Some(total as i64),
                                speed_bps: Some(speed as i64),
                                upload_speed_bps: Some(up_speed),
                                eta_sec: eta,
                                ..Default::default()
                            });
                        }
                        "paused" => {
                            ctx.send_patch(&job.id, JobPatch {
                                state: Some(JobState::Paused),
                                ..Default::default()
                            });
                            return Err(JobError::Cancelled);
                        }
                        "error" => {
                            let msg = format!(
                                "aria2 error {}: {}",
                                str_of(&status, "errorCode").unwrap_or("?"),
                                str_of(&status, "errorMessage").unwrap_or("download error")
                            );
                            if msg.contains("already downloaded") || msg.contains("complete") {
                                let out_path = first_file_path(&status);
                                ctx.send_patch(&job.id, JobPatch {
                                    state: Some(JobState::Done),
                                    progress: Some(100.0),
                                    output_path: out_path.clone(),
                                    ..Default::default()
                                });
                                return Ok(JobDone { output_path: out_path, title: None });
                            }
                            return Err(JobError::Retry(msg));
                        }
                        "complete" => {
                            let is_torrent = job.kind == mdm_core::JobKind::Torrent
                                || job.kind == mdm_core::JobKind::Magnet
                                || status.get("bittorrent").is_some();
                            let seed_minutes = if is_torrent { ctx.cfg.seed_minutes } else { 0 };
                            let seeded: i64 = status
                                .get("seedingTime")
                                .and_then(|x| x.as_str())
                                .and_then(|s| s.parse().ok())
                                .unwrap_or(0);
                            let done_seeding =
                                !is_torrent || seed_minutes == 0 || seeded >= (seed_minutes as i64) * 60;
                            if done_seeding {
                                let mut out_path = first_file_path(&status);
                                if out_path.is_none() {
                                    if let Some(title) = &job.title {
                                        let candidate = job.output_dir.join(title);
                                        if candidate.exists() {
                                            out_path = Some(candidate);
                                        }
                                    }
                                }
                                let _ = rpc.remove_download_result(&tracking_gid).await;
                                ctx.send_patch(&job.id, JobPatch {
                                    state: Some(JobState::Done),
                                    progress: Some(100.0),
                                    downloaded_bytes: Some(total as i64),
                                    total_bytes: Some(total as i64),
                                    speed_bps: Some(0),
                                    upload_speed_bps: Some(0),
                                    output_path: out_path.clone(),
                                    ..Default::default()
                                });
                                return Ok(JobDone { output_path: out_path, title: job.title.clone() });
                            }
                            // Still seeding (BitTorrent only): report progress 100 with uploadSpeed.
                            ctx.send_patch(&job.id, JobPatch {
                                state: Some(JobState::Running),
                                progress: Some(100.0),
                                downloaded_bytes: Some(total as i64),
                                total_bytes: Some(total as i64),
                                speed_bps: Some(0),
                                upload_speed_bps: Some(up_speed),
                                ..Default::default()
                            });
                        }
                        "removed" => {
                            return Err(JobError::Cancelled);
                        }
                        other => {
                            return Err(JobError::Retry(format!("aria2 status '{other}'")));
                        }
                    }
                }
            }
        }

        fn first_file_path(status: &Value) -> Option<std::path::PathBuf> {
            status
                .get("files")
                .and_then(|f| f.as_array())
                .and_then(|a| {
                    a.iter().find(|f| {
                        f.get("path").and_then(|p| p.as_str()).map(|p| !p.is_empty() && p.starts_with('/'))
                            .unwrap_or(false)
                    })
                })
                .and_then(|f| f.get("path").and_then(|p| p.as_str()))
                .map(std::path::PathBuf::from)
                .or_else(|| {
                    // Windows-style path fallback
                    status
                        .get("files")
                        .and_then(|f| f.as_array())
                        .and_then(|a| a.first())
                        .and_then(|f| f.get("path").and_then(|p| p.as_str()))
                        .map(std::path::PathBuf::from)
                })
        }
    }

    async fn remove(&self, job: &Job) {
        let active_gid = self.active_gids.lock().unwrap().get(&job.id).cloned();
        let db_gid = job.external_id.clone();

        let mut gids = Vec::new();
        if let Some(g) = active_gid {
            gids.push(g);
        }
        if let Some(g) = db_gid {
            if !gids.contains(&g) {
                gids.push(g);
            }
        }

        if !gids.is_empty() {
            if let Ok(daemon) = self.daemon.get().await {
                let rpc = rpc::Aria2Rpc::new(daemon.rpc_addr.clone(), daemon.secret.clone());
                for gid in &gids {
                    if let Ok(status) = rpc.tell_status(gid).await {
                        if let Some(children) = status.get("followedBy").and_then(|f| f.as_array()) {
                            for child in children {
                                if let Some(cgid) = child.as_str() {
                                    let _ = rpc.force_remove(cgid).await;
                                    let _ = rpc.remove_download_result(cgid).await;
                                }
                            }
                        }
                    }
                    let _ = rpc.force_remove(gid).await;
                    let _ = rpc.remove_download_result(gid).await;
                }
            }
        }
    }

    async fn shutdown(&self) {
        if let Some(daemon) = self.daemon.peek() {
            daemon.shutdown().await;
        }
    }
}
