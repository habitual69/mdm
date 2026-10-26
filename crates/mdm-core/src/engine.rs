use std::path::PathBuf;

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

use crate::config::Config;
use crate::error::{Error, Result};
use crate::events::EventTx;
use crate::model::{EngineKind, Job, JobPatch};

/// Outcome reported by an engine when a job run loop ends.
#[derive(Debug, Clone, Default)]
pub struct JobDone {
    pub output_path: Option<PathBuf>,
    pub title: Option<String>,
}

#[derive(Debug, Clone)]
pub enum JobError {
    /// User-initiated pause -> job lands in Paused (or Deleted on removal).
    Cancelled,
    /// Transient; queue will retry up to `max_retries` with backoff.
    Retry(String),
    /// Permanent; no retry.
    Fatal(String),
}

impl JobError {
    pub fn message(&self) -> String {
        match self {
            JobError::Cancelled => "cancelled".into(),
            JobError::Retry(m) => m.clone(),
            JobError::Fatal(m) => m.clone(),
        }
    }
}

/// Best-effort metadata before/at run start.
#[derive(Debug, Clone, Default)]
pub struct ResolveInfo {
    pub title: Option<String>,
    pub n_items: Option<u64>,
}

/// Per-job message stream back to the queue manager.
#[derive(Debug, Clone)]
pub enum RunnerMsg {
    Patch { job_id: String, patch: JobPatch },
    Done { job_id: String, result: std::result::Result<JobDone, JobError> },
}

pub type RunnerTx = tokio::sync::mpsc::UnboundedSender<RunnerMsg>;

/// What an engine needs to run one job.
#[derive(Clone)]
pub struct EngineCtx {
    pub cfg: Config,
    pub cancel: CancellationToken,
    pub runner_tx: RunnerTx,
    pub events: EventTx,
}

impl EngineCtx {
    pub fn send_patch(&self, job_id: impl Into<String>, patch: JobPatch) {
        let _ = self.runner_tx.send(RunnerMsg::Patch { job_id: job_id.into(), patch });
    }
}

/// One binary-backed download engine (yt-dlp or aria2).
#[async_trait]
pub trait Engine: Send + Sync + 'static {
    fn engine_kind(&self) -> EngineKind;

    /// Best-effort metadata fetch; default returns nothing.
    async fn resolve(&self, _job: &Job, _ctx: &EngineCtx) -> Result<ResolveInfo> {
        Ok(ResolveInfo::default())
    }

    /// Live progress of one job to completion. Must observe `ctx.cancel`.
    async fn run(&self, job: Job, ctx: EngineCtx) -> std::result::Result<JobDone, JobError>;

    /// Best-effort kill while the job is running (used on job removal).
    async fn remove(&self, _job: &Job) {}

    /// Adopt configuration changes live (settings hot-swap). Default: no-op;
    /// engines that read a fresh config per run don't need this.
    async fn update_config(&self, _cfg: &Config) {}

    /// Queue stop: shut engine-owned daemons/processes down.
    async fn shutdown(&self) {}
}

// ---------------------------------------------------------------------------
// Binary discovery
// ---------------------------------------------------------------------------

fn is_exec(p: &PathBuf) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        match std::fs::metadata(p) {
            Ok(m) => m.is_file() && (m.permissions().mode() & 0o111 != 0),
            Err(_) => false,
        }
    }
    #[cfg(not(unix))]
    {
        std::fs::metadata(p).map(|m| m.is_file()).unwrap_or(false)
    }
}

fn candidates(name: &str, extra: Option<&[PathBuf]>, include_path: bool) -> Vec<PathBuf> {
    let exe_names: Vec<String> = if cfg!(windows) {
        vec![format!("{name}.exe"), name.to_string()]
    } else {
        vec![name.to_string()]
    };

    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(list) = extra {
        dirs.extend(list.iter().cloned());
    }
    if include_path {
        if let Ok(path) = std::env::var("PATH") {
            dirs.extend(std::env::split_paths(&path));
        }
    }

    let mut out = Vec::new();
    for d in dirs {
        for n in &exe_names {
            let p = d.join(n);
            if is_exec(&p) && !out.contains(&p) {
                out.push(p);
            }
        }
    }
    out
}

/// Locate an engine binary:
/// 1. Application-bundled directories (exe_dir, exe_dir/bin, exe_dir/resources/bin, exe_dir/resources)
/// 2. Explicit `extra` dirs (cfg.bin_dir)
/// 3. Application data bin dir (~data/mdm/bin or %APPDATA%/mdm/bin)
/// 4. System PATH as fallback only.
pub fn find_bin(name: &str, extra: Option<&[PathBuf]>) -> Option<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();

    // 1. Check bundled directories relative to the running executable
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            dirs.push(dir.to_path_buf());
            dirs.push(dir.join("bin"));
            dirs.push(dir.join("resources").join("bin"));
            dirs.push(dir.join("resources"));
            // If running in development (target/debug or target/release), also inspect workspace roots
            if let Some(target_dir) = dir.parent() {
                if let Some(ws_root) = target_dir.parent() {
                    dirs.push(ws_root.join("bin"));
                    dirs.push(ws_root.join("mdm-app").join("src-tauri").join("bin"));
                    dirs.push(ws_root.join("crates").join("mdm-bins").join("bin"));
                }
            }
        }
    }

    // 2. Add extra dirs from config (cfg.bin_dir)
    if let Some(list) = extra {
        dirs.extend(list.iter().cloned());
    }

    // 3. User data directory (~/.local/share/mdm/bin or %APPDATA%/mdm/bin)
    dirs.push(crate::config::data_dir().join("bin"));

    // Check application-managed and bundled directories first (NO system PATH)
    if let Some(found) = candidates(name, Some(&dirs), false).into_iter().next() {
        return Some(found);
    }

    // 4. Fallback to system PATH only if no bundled or user-installed binary was found
    candidates(name, None, true).into_iter().next()
}

/// Locate a JavaScript runtime (node, bun, deno, quickjs) needed by yt-dlp to solve
/// YouTube signature and player challenges.
pub fn find_js_runtime() -> Option<PathBuf> {
    // 1. Check for bundled or installed JS runtimes (node, bun, deno, quickjs)
    for js in ["node", "deno", "bun", "qjs"] {
        if let Some(p) = find_bin(js, None) {
            return Some(p);
        }
    }

    // 2. Check helper bin directory with platform extension
    let mdm_node = if cfg!(windows) {
        crate::config::data_dir().join("bin/node.exe")
    } else {
        crate::config::data_dir().join("bin/node")
    };
    if mdm_node.is_file() {
        return Some(mdm_node);
    }

    // 3. Scan user environments (NVM, fnm, asdf, bun, deno)
    if let Some(home) = dirs::home_dir() {
        let nvm_pattern = home.join(".nvm/versions/node");
        if let Ok(entries) = std::fs::read_dir(&nvm_pattern) {
            for entry in entries.flatten() {
                let candidate = if cfg!(windows) {
                    entry.path().join("bin").join("node.exe")
                } else {
                    entry.path().join("bin").join("node")
                };
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
        let common = [
            home.join(".fnm/current/bin/node"),
            home.join(".fnm/current/bin/node.exe"),
            home.join(".asdf/shims/node"),
            home.join(".bun/bin/bun"),
            home.join(".bun/bin/bun.exe"),
            home.join(".deno/bin/deno"),
            home.join(".deno/bin/deno.exe"),
            home.join(".local/bin/node"),
            PathBuf::from("/usr/local/bin/node"),
            PathBuf::from("/usr/bin/node"),
            PathBuf::from(r"C:\Program Files\nodejs\node.exe"),
            PathBuf::from(r"C:\Program Files (x86)\nodejs\node.exe"),
        ];
        for c in common {
            if c.is_file() {
                return Some(c);
            }
        }
    }
    None
}

pub fn bin_path(name: &str, cfg: &Config) -> Result<PathBuf> {
    let extra: Option<Vec<PathBuf>> = cfg.bin_dir.as_ref().map(|d| vec![d.clone()]);
    find_bin(name, extra.as_deref()).ok_or_else(|| Error::BinMissing {
        name: name.to_string(),
        hint: format!(
            "not on PATH and not in {}; install it (e.g. `mdm doctor` shows how) or set bin_dir in config",
            cfg.bin_dir.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "any bin_dir".into())
        ),
    })
}
