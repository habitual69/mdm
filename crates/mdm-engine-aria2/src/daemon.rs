//! Spawn and lifecycle-manage the local aria2c daemon (RPC mode).

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use mdm_core::{bin_path, Config, Error, Result};
use tokio::sync::{Mutex, OnceCell};
use tracing::{info, warn};

use crate::rpc::Aria2Rpc;

pub struct Aria2Daemon {
    pub rpc_addr: String,
    pub secret: String,
    child: Mutex<Option<tokio::process::Child>>,
    session_file: PathBuf,
}

fn free_port() -> std::io::Result<u16> {
    std::net::TcpListener::bind(("127.0.0.1", 0))?
        .local_addr()
        .map(|a| a.port())
}

impl Aria2Daemon {
    pub async fn spawn(cfg: &Config, data_dir: &std::path::Path) -> Result<Arc<Aria2Daemon>> {
        let bin = bin_path("aria2c", cfg)?;
        let port = free_port().map_err(|e| Error::Engine(format!("free port: {e}")))?;
        let secret = uuid::Uuid::new_v4().simple().to_string();
        let session_file = data_dir.join("aria2.session");
        let log_file = data_dir.join("aria2.log");
        if !session_file.exists() {
            std::fs::write(&session_file, b"")?;
        }
        let out_dir = cfg.output_dir.display().to_string();
        let rpc_addr = format!("127.0.0.1:{port}");

        let mut args: Vec<String> = vec![
            "--enable-rpc".into(),
            format!("--rpc-listen-port={port}"),
            format!("--rpc-secret={secret}"),
            format!("--dir={out_dir}"),
            "--continue=true".into(),
            "--always-resume=true".into(),
            "--auto-file-renaming=false".into(),
            format!("--save-session={}", session_file.display()),
            "--save-session-interval=30".into(),
            format!("--input-file={}", session_file.display()),
            format!("--seed-time={}", cfg.seed_minutes),
            "--console-log-level=warn".into(),
            format!("--log={}", log_file.display()),
            "--log-level=notice".into(),
            "--enable-dht=true".into(),
            "--bt-stop-timeout=0".into(),
            "--check-integrity=true".into(),
        ];

        // Tunables from the aria2 settings card (modeled on P3TERX's
        // aria2.conf): disk cache, preallocation, split sizing, retries,
        // slow-connection cutoff, BT peers and UA masquerade.
        if cfg.aria2_disk_cache > 0 {
            args.push(format!("--disk-cache={}M", cfg.aria2_disk_cache.min(1024)));
        }
        args.push(format!("--file-allocation={}", cfg.aria2_file_allocation));
        args.push(format!("--min-split-size={}M", cfg.aria2_min_split_size.clamp(1, 1024)));
        args.push(format!("--max-tries={}", cfg.aria2_max_tries.min(20)));
        args.push(format!("--retry-wait={}", cfg.aria2_retry_wait.min(60)));
        if cfg.aria2_lowest_speed_limit > 0 {
            args.push(format!(
                "--lowest-speed-limit={}K",
                cfg.aria2_lowest_speed_limit.min(10240)
            ));
        }
        args.push(format!("--bt-max-peers={}", cfg.aria2_bt_max_peers.min(500)));
        if !cfg.aria2_user_agent.trim().is_empty() {
            args.push(format!("--user-agent={}", cfg.aria2_user_agent.trim()));
        }

        let mut cmd = tokio::process::Command::new(&bin);
        cmd.args(&args).stdout(Stdio::null()).stderr(Stdio::null());

        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }

        let child = cmd
            .spawn()
            .map_err(|e| Error::BinMissing { name: "aria2c".into(), hint: e.to_string() })?;

        let daemon = Arc::new(Aria2Daemon {
            rpc_addr,
            secret,
            child: Mutex::new(Some(child)),
            session_file,
        });

        let rpc = Aria2Rpc::new(daemon.rpc_addr.clone(), daemon.secret.clone());
        let mut ready = false;
        for _ in 0..50 {
            if rpc.version().await.is_ok() {
                ready = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        if ready {
            info!(
                "aria2 daemon up on {} (session {})",
                daemon.rpc_addr,
                daemon.session_file.display()
            );
        } else {
            warn!("aria2 RPC did not answer within 5s; continuing and hoping");
        }

        Ok(daemon)
    }

    pub async fn shutdown(&self) {
        let rpc = Aria2Rpc::new(self.rpc_addr.clone(), self.secret.clone());
        let _ = rpc.shutdown().await;
        // Give it a moment to flush the session file.
        tokio::time::sleep(Duration::from_millis(300)).await;
        let mut guard = self.child.lock().await;
        if let Some(child) = guard.as_mut() {
            if let Err(e) = child.start_kill() {
                warn!("aria2 kill: {e}");
            }
            let _ = child.wait().await;
        }
        *guard = None;
    }
}

/// Lazily spawns the daemon the first time a job needs it; shared for all jobs.
pub struct DaemonHandle {
    cell: OnceCell<Arc<Aria2Daemon>>,
    cfg: Config,
    data_dir: PathBuf,
}

impl DaemonHandle {
    pub fn new(cfg: Config, data_dir: PathBuf) -> DaemonHandle {
        DaemonHandle { cell: OnceCell::new(), cfg, data_dir }
    }

    pub async fn get(&self) -> Result<Arc<Aria2Daemon>> {
        self.cell
            .get_or_try_init(|| async { Aria2Daemon::spawn(&self.cfg, &self.data_dir).await })
            .await
            .map(Arc::clone)
    }

    /// Returns the running daemon without spawning one.
    pub fn peek(&self) -> Option<Arc<Aria2Daemon>> {
        self.cell.get().cloned()
    }
}
