//! mdm — Media Download Manager CLI + local daemon.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use mdm_core::{Config, JobKind, JobState};
use serde_json::json;

#[derive(Parser)]
#[command(name = "mdm", version, about = "Media Download Manager (Phase 1: CLI + local daemon)")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Clone, Copy, ValueEnum)]
enum KindArg {
    Video,
    Audio,
    Direct,
    Torrent,
    Magnet,
}

impl KindArg {
    fn to_kind(self) -> JobKind {
        match self {
            KindArg::Video => JobKind::Video,
            KindArg::Audio => JobKind::Audio,
            KindArg::Direct => JobKind::Direct,
            KindArg::Torrent => JobKind::Torrent,
            KindArg::Magnet => JobKind::Magnet,
        }
    }
}

#[derive(Subcommand)]
enum Cmd {
    /// Queue URL(s). Starts the daemon if it is not running.
    Add {
        urls: Vec<String>,
        #[arg(short = 'f', long, help = "yt-dlp format selector (default: best)")]
        format: Option<String>,
        #[arg(short = 'k', long, help = "override auto-detection")]
        kind: Option<KindArg>,
        #[arg(short = 'o', long = "out", help = "override output dir")]
        out: Option<PathBuf>,
    },
    /// List jobs (live with --watch).
    Ls {
        #[arg(long)]
        watch: bool,
    },
    /// Pause a job by (prefix of) id.
    Pause { id: String },
    /// Resume a job by (prefix of) id.
    Resume { id: String },
    /// Remove a job (files stay on disk).
    Rm { id: String },
    /// Stream live queue updates (Ctrl-C to stop).
    Watch,
    /// Foreground daemon: queue engine + local REST/WS API.
    Daemon,
    /// Engine + server health report.
    Doctor,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    dispatch(cli.cmd).await
}

async fn dispatch(cmd: Cmd) -> Result<()> {
    match cmd {
        Cmd::Daemon => run_daemon().await,
        Cmd::Add { urls, format, kind, out } => {
            if urls.is_empty() {
                bail!("add: give at least one URL");
            }
            ensure_daemon()?;
            let client = http_client()?;
            for url in urls {
                let body = json!({
                    "url": url,
                    "kind": kind.map(|k| k.to_kind().as_str()),
                    "format": format.clone(),
                    "output_dir": out.as_ref().map(|p| p.display().to_string()),
                });
                let resp = client
                    .post(base_url().join("api/jobs")?)
                    .json(&body)
                    .send()
                    .await
                    .context("add: server request failed")?;
                let bytes = resp.bytes().await?;
                let v: serde_json::Value = serde_json::from_slice(&bytes)?;
                match v.get("id").and_then(|i| i.as_str()) {
                    Some(id) => println!("queued {}  {}", &id[..id.len().min(8)], v["url"].as_str().unwrap_or(&url)),
                    None => bail!("add failed: {v}"),
                }
            }
            println!("watch: mdm ls --watch");
            Ok(())
        }
        Cmd::Ls { watch } => {
            ensure_daemon()?;
            if !watch {
                print_table(&fetch_jobs(None).await?);
                Ok(())
            } else {
                loop {
                    let jobs = fetch_jobs(None).await?;
                    print!("\x1b[2J\x1b[H");
                    print_table(&jobs);
                    tokio::time::sleep(Duration::from_millis(700)).await;
                }
            }
        }
        Cmd::Watch => {
            ensure_daemon()?;
            watch_ws().await
        }
        Cmd::Pause { id } => job_action("pause", &id).await,
        Cmd::Resume { id } => job_action("resume", &id).await,
        Cmd::Rm { id } => job_action("remove", &id).await,
        Cmd::Doctor => doctor().await,
    }
}

async fn job_action(action: &str, id: &str) -> Result<()> {
    ensure_daemon()?;
    let resolved = resolve_prefix(id).await?;
    let url = match action {
        "pause" => format!("api/jobs/{resolved}/pause"),
        "resume" => format!("api/jobs/{resolved}/resume"),
        _ => format!("api/jobs/{resolved}"),
    };
    let client = http_client()?;
    let resp = match action {
        "remove" => client.delete(base_url().join(&url)?).send().await?,
        _ => client.post(base_url().join(&url)?).send().await?,
    };
    if resp.status().is_success() {
        let msg = match action {
            "remove" => format!("removed {} (files on disk untouched)", short(&resolved)),
            other => format!("{other}ed {}", short(&resolved)),
        };
        println!("{msg}");
        Ok(())
    } else {
        bail!("{action} failed: {} {}", resp.status(), resp.text().await?)
    }
}

// --- helpers ---------------------------------------------------------------

fn base_url() -> reqwest::Url {
    let cfg = load_cfg();
    reqwest::Url::parse(&format!("http://{}", cfg.api_addr)).expect("valid addr in config")
}

fn load_cfg() -> Config {
    mdm_core::load_config().expect("load config")
}

fn http_client() -> Result<reqwest::Client> {
    reqwest::Client::builder().build().context("http client")
}

fn short(id: &str) -> &str {
    &id[..id.len().min(8)]
}

async fn fetch_jobs(states: Option<Vec<JobState>>) -> Result<Vec<mdm_core::Job>> {
    let mut url = base_url().join("api/jobs")?;
    if let Some(list) = &states {
        let joined: Vec<String> = list.iter().map(|s| s.as_str().to_string()).collect();
        url.query_pairs_mut().append_pair("state", &joined.join(","));
    }
    let client = http_client()?;
    let resp = client.get(url).send().await?;
    let bytes = resp.bytes().await?;
    serde_json::from_slice(&bytes).context("parse jobs response")
}

async fn resolve_prefix(prefix: &str) -> Result<String> {
    let jobs = fetch_jobs(None).await?;
    let matches: Vec<&mdm_core::Job> =
        jobs.iter().filter(|j| j.id.starts_with(prefix)).collect();
    match matches.len() {
        0 => bail!("no job matches prefix {prefix}"),
        1 => Ok(matches[0].id.clone()),
        n => bail!("prefix {prefix} matches {n} jobs; be more specific"),
    }
}

/// Probe the local API; spawn a detached `mdm daemon` when nothing answers.
pub fn ensure_daemon() -> Result<()> {
    let cfg = load_cfg();
    let addr = cfg.api_addr.clone();
    let up = std::net::TcpStream::connect_timeout(
        &format!("127.0.0.1:{}", addr.split(':').nth(1).unwrap_or("5757"))
            .parse()
            .context("parse addr")?,
        Duration::from_millis(250),
    )
    .is_ok();
    if up {
        return Ok(());
    }
    spawn_detached_daemon()?;
    for _ in 0..40 {
        if std::net::TcpStream::connect_timeout(
            &addr.parse().context("parse addr")?,
            Duration::from_millis(200),
        )
        .is_ok()
        {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(120));
    }
    bail!("daemon did not start; run `mdm daemon` manually for logs")
}

fn spawn_detached_daemon() -> Result<()> {
    let exe = std::env::current_exe().context("locate mdm binary")?;
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("daemon").stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    cmd.spawn().context("spawn daemon")?;
    Ok(())
}

async fn run_daemon() -> Result<()> {
    let cfg = mdm_core::load_config()?;
    let data_dir = mdm_core::ensure_dir(&mdm_core::data_dir())?;

    // File logging; console stays free for the user's terminal.
    let log_path = data_dir.join("mdm.log");
    let log_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)?;
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("info".parse().unwrap()),
        )
        .with_writer(std::sync::Mutex::new(log_file))
        .with_ansi(false)
        .init();

    println!("mdm daemon -> config {}, log {}", load_config_public(), log_path.display());

    let queue = mdm_server::build_queue(cfg.clone()).context("build queue")?;

    // Signals: shut the queue down, then stop axum.
    let shutdown_q = queue.clone();
    let shutdown_signal = async move {
        let ctrl_c = tokio::signal::ctrl_c();
        #[cfg(unix)]
        let mut sigterm = tokio::signal::unix::signal(
            tokio::signal::unix::SignalKind::terminate(),
        )
        .expect("sigterm handler");
        tokio::select! {
            _ = ctrl_c => {},
            _ = async {
                #[cfg(unix)]
                { sigterm.recv().await }
                #[cfg(not(unix))]
                { std::future::pending::<()>().await }
            } => {},
        }
        let _ = shutdown_q.shutdown().await;
    };

    mdm_server::serve_with_shutdown(queue, &cfg.api_addr, shutdown_signal).await?;
    Ok(())
}

fn load_config_public() -> String {
    mdm_core::config_path().display().to_string()
}

async fn watch_ws() -> Result<()> {
    // Phase 1: polling stream (WS plumbing exists server-side).
    loop {
        let jobs = fetch_jobs(None).await?;
        print!("\x1b[2J\x1b[H");
        print_table(&jobs);
        tokio::time::sleep(Duration::from_millis(700)).await;
    }
}

fn human_bytes(b: Option<i64>) -> String {
    match b {
        None => "-".into(),
        Some(n) if n <= 0 => "-".into(),
        Some(n) => {
            let units = ["B", "KiB", "MiB", "GiB", "TiB"];
            let mut v = n as f64;
            let mut idx = 0;
            while v >= 1024.0 && idx < units.len() - 1 {
                v /= 1024.0;
                idx += 1;
            }
            if idx == 0 {
                format!("{v:.0}{u}", u = units[idx])
            } else {
                format!("{v:.1}{u}", u = units[idx])
            }
        }
    }
}

fn eta_text(eta: Option<i64>) -> String {
    match eta {
        None => "-".into(),
        Some(s) if s <= 0 => "-".into(),
        Some(s) => {
            let h = s / 3600;
            let m = (s % 3600) / 60;
            let sec = s % 60;
            if h > 0 {
                format!("{h}:{m:02}:{sec:02}")
            } else {
                format!("{m}:{sec:02}")
            }
        }
    }
}

fn print_table(jobs: &[mdm_core::Job]) {
    println!(
        "{:<9} {:<7} {:<8} {:>6} {:>9} {:>10} {:>6}  {}",
        "ID", "KIND", "STATE", "PCT", "SIZE", "SPEED", "ETA", "NAME"
    );
    for j in jobs {
        let name = j
            .title
            .clone()
            .or_else(|| Some(j.url.clone()))
            .unwrap_or_default();
        let name: String = name
            .chars()
            .take(52)
            .collect::<String>()
            .replace('\n', " ");
        println!(
            "{:<9} {:<7} {:<8} {:>6} {:>9} {:>10} {:>6}  {}",
            short(&j.id),
            j.kind.as_str(),
            j.state.as_str(),
            format!("{:.0}%", j.progress),
            human_bytes(j.total_bytes),
            if j.state == JobState::Running {
                format!("{}/s", human_bytes(j.speed_bps))
            } else {
                "-".into()
            },
            eta_text(j.eta_sec),
            name
        );
    }
    println!(
        "count: {}",
        jobs.len()
    );
}

async fn doctor() -> Result<()> {
    let cfg = mdm_core::load_config()?;
    println!("config:    {}", mdm_core::config_path().display());
    println!("data dir:  {}", mdm_core::data_dir().display());
    println!("output:    {}", cfg.output_dir.display());
    println!("api addr:  {}", cfg.api_addr);

    for bin in ["yt-dlp", "aria2c", "ffmpeg"] {
        let found = mdm_core::bin_path(bin, &cfg);
        match found {
            Ok(p) => {
                let ver = tokio::process::Command::new(&p)
                    .arg("--version")
                    .output()
                    .await
                    .ok()
                    .map(|o| String::from_utf8_lossy(&o.stdout).lines().next().unwrap_or("").to_string())
                    .unwrap_or_default();
                println!(
                    "bin {bin:<7} OK   {}  ({ver})",
                    p.display()
                );
            }
            Err(e) => println!("bin {bin:<7} MISSING ({e})"),
        }
    }

    let daemon_up = std::net::TcpStream::connect_timeout(
        &cfg.api_addr.parse().context("api addr")?,
        Duration::from_millis(250),
    )
    .is_ok();
    println!(
        "daemon:    {}",
        if daemon_up { "running" } else { "not running (mdm add starts it)" }
    );
    Ok(())
}
