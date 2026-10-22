//! aria2 engine: torrent / magnet / direct downloads through the RPC daemon.

mod daemon;
mod rpc;

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use base64::Engine as _;
use mdm_core::{
    Config, Engine, EngineCtx, EngineKind, Job, JobDone, JobError, JobPatch, JobState, ResolveInfo,
};
use serde_json::{json, Value};
use tracing::warn;

pub use daemon::{Aria2Daemon, DaemonHandle};
pub use rpc::RpcError;

pub struct Aria2Engine {
    pub daemon: DaemonHandle,
}

impl Aria2Engine {
    pub fn new(cfg: Config, data_dir: std::path::PathBuf) -> Aria2Engine {
        Aria2Engine { daemon: DaemonHandle::new(cfg, data_dir) }
    }

    async fn add_job(&self, job: &Job) -> std::result::Result<String, JobError> {
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
                    rpc.add_torrent(&b64, &dir).await
                } else {
                    rpc.add_uri(vec![job.url.clone()], &dir, json!({})).await
                }
            }
            mdm_core::JobKind::Direct | mdm_core::JobKind::Magnet => {
                rpc.add_uri(vec![job.url.clone()], &dir, json!({})).await
            }
            // Video/audio never route here in Phase 1.
            _ => rpc.add_uri(vec![job.url.clone()], &dir, json!({})).await,
        };
        gid.map_err(map_rpc_err)
    }

    async fn reattach_or_add(&self, job: &Job) -> std::result::Result<String, JobError> {
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
        self.add_job(job).await
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

#[async_trait]
impl Engine for Aria2Engine {
    fn engine_kind(&self) -> EngineKind {
        EngineKind::Aria2
    }

    async fn resolve(&self, job: &Job, _ctx: &EngineCtx) -> mdm_core::Result<ResolveInfo> {
        // Magnets/torrents: no cheap metadata until metadata exchange; report as-is.
        Ok(ResolveInfo {
            title: if job.kind == mdm_core::JobKind::Magnet {
                None
            } else {
                None
            },
            n_items: None,
        })
    }

    async fn run(&self, job: Job, ctx: EngineCtx) -> std::result::Result<JobDone, JobError> {
        let gid = self.reattach_or_add(&job).await?;

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
                            continue;
                        }
                    };

                    // Magnet metadata resolution hands off to a child gid.
                    if let Some(children) = status.get("followedBy").and_then(|f| f.as_array()) {
                        if let Some(child) = children.first() {
                            if let Some(child_gid) = child.as_str() {
                                if child_gid != tracking_gid {
                                    tracking_gid = child_gid.to_string();
                                    ctx.send_patch(
                                        &job.id,
                                        JobPatch {
                                            external_id: Some(tracking_gid.clone()),
                                            ..Default::default()
                                        },
                                    );
                                    continue;
                                }
                            }
                        }
                    }

                    let (status_str, total, done, speed, eta) = map_status_values(&status);

                    // Metadata exchange (magnet) phase: total==0 while active.
                    if status_str == "active" && total == 0 {
                        ctx.send_patch(&job.id, JobPatch {
                            state: Some(JobState::Running),
                            progress: Some(0.0),
                            total_bytes: Some(0),
                            ..Default::default()
                        });
                        continue;
                    }

                    match status_str.as_str() {
                        "active" | "waiting" => {
                            let pct = if total > 0 { (done as f32 / total as f32) * 100.0 } else { 0.0 };
                            ctx.send_patch(&job.id, JobPatch {
                                state: Some(JobState::Running),
                                progress: Some(pct),
                                downloaded_bytes: Some(done as i64),
                                total_bytes: Some(total as i64),
                                speed_bps: Some(speed as i64),
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
                            let seed_minutes = ctx.cfg.seed_minutes;
                            let seeded: i64 = status
                                .get("seedingTime")
                                .and_then(|x| x.as_str())
                                .and_then(|s| s.parse().ok())
                                .unwrap_or(0);
                            let done_seeding =
                                seed_minutes == 0 || seeded >= (seed_minutes as i64) * 60;
                            if done_seeding {
                                let out_path = first_file_path(&status);
                                let _ = rpc.remove_download_result(&tracking_gid).await;
                                return Ok(JobDone { output_path: out_path, title: None });
                            }
                            // Still seeding: report progress 100 with uploadSpeed.
                            ctx.send_patch(&job.id, JobPatch {
                                state: Some(JobState::Running),
                                progress: Some(100.0),
                                speed_bps: Some(
                                    status.get("uploadSpeed").and_then(|x| x.as_str())
                                        .and_then(|s| s.parse::<i64>().ok())
                                        .unwrap_or(0),
                                ),
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
        if let Some(gid) = job.external_id.clone() {
            if let Ok(daemon) = self.daemon.get().await {
                let rpc = rpc::Aria2Rpc::new(daemon.rpc_addr.clone(), daemon.secret.clone());
                let _ = rpc.force_remove(&gid).await;
                let _ = rpc.remove_download_result(&gid).await;
            }
        }
    }

    async fn shutdown(&self) {
        if let Some(daemon) = self.daemon.peek() {
            daemon.shutdown().await;
        }
    }
}
