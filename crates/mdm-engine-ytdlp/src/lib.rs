//! yt-dlp engine: spawn the extractor, stream progress, kill cleanly on cancel.

mod parse;
mod signal;

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Mutex;
use std::time::Duration;

use async_trait::async_trait;
use mdm_core::{
    bin_path, Config, Engine, EngineCtx, EngineKind, Error, Job, JobDone, JobError, JobPatch,
    JobState, Result,
};
use tokio::io::{AsyncBufReadExt, BufReader};
use tracing::{debug, warn};

pub struct Ytdlp {
    bin: PathBuf,
    /// yt-dlp's external_downloader passthrough requires aria2c; optional.
    with_aria2: bool,
    /// job_id -> yt-dlp child pid (for out-of-band kills on remove()).
    children: Mutex<HashMap<String, u32>>,
}

/// Errors that will not get better by retrying.
const FATAL_PATTERNS: [&str; 7] = [
    "Unsupported URL",
    "not a valid URL",
    "Private video",
    "Sign in to confirm",
    "HTTP Error 404",
    "HTTP Error 403",
    "no suitable format",
];

fn is_fatal(msg: &str) -> bool {
    FATAL_PATTERNS.iter().any(|p| msg.contains(p))
}

impl Ytdlp {
    pub fn from_config(cfg: &Config) -> Result<Ytdlp> {
        let bin = bin_path("yt-dlp", cfg)?;
        let with_aria2 = bin_path("aria2c", cfg).is_ok();
        Ok(Ytdlp { bin, with_aria2, children: Mutex::new(HashMap::new()) })
    }

    fn register_child(&self, job_id: &str, pid: u32) {
        self.children.lock().unwrap().insert(job_id.into(), pid);
    }

    fn unregister_child(&self, job_id: &str) {
        let _ = self.children.lock().unwrap().remove(job_id);
    }
}

fn last_lines(s: &str, n: usize) -> String {
    let lines: Vec<&str> = s.lines().collect();
    let start = lines.len().saturating_sub(n);
    lines[start..].join("\n")
}

fn handle_line(
    line: &str,
    job: &Job,
    ctx: &EngineCtx,
    final_path: &mut Option<PathBuf>,
    last_emit: &mut tokio::time::Instant,
) {
    match parse::classify(line) {
        parse::Line::Progress { pct, total, speed, eta, .. } => {
            let now = tokio::time::Instant::now();
            if now - *last_emit >= Duration::from_millis(200) || pct >= 100.0 {
                *last_emit = now;
                let mut patch = JobPatch::for_id(&job.id, JobState::Running);
                patch.progress = Some(pct);
                patch.total_bytes = total;
                patch.speed_bps = speed;
                patch.eta_sec = eta;
                patch.downloaded_bytes =
                    total.map(|t| ((pct / 100.0).clamp(0.0, 1.0) as i64) * t);
                ctx.send_patch(&job.id, patch);
            }
        }
        parse::Line::Item { .. } => {
            // Playlist item boundary: per-item progress resets; acceptable Phase 1.
        }
        parse::Line::Destination(p) => {
            if final_path.is_none() {
                *final_path = Some(p);
            }
        }
        parse::Line::FinalPath(p) => {
            *final_path = Some(p);
        }
        parse::Line::PostProcess(tag) => {
            debug!("job {} postprocess: {tag}", job.short_id());
        }
        parse::Line::Ignore => {}
    }
}

#[async_trait]
impl Engine for Ytdlp {
    fn engine_kind(&self) -> EngineKind {
        EngineKind::Ytdlp
    }

    async fn resolve(&self, job: &Job, _ctx: &EngineCtx) -> Result<mdm_core::ResolveInfo> {
        let out = tokio::process::Command::new(&self.bin)
            .args([
                "--no-config",
                "--no-warnings",
                "--flat-playlist",
                "--dump-single-json",
            ])
            .arg(&job.url)
            .env("NO_COLOR", "1")
            .output()
            .await
            .map_err(|e| Error::Engine(format!("spawn yt-dlp: {e}")))?;

        if !out.status.success() {
            let msg = String::from_utf8_lossy(&out.stderr);
            let tail = last_lines(&msg, 3);
            return Err(Error::Engine(format!("resolve failed: {tail}")));
        }
        let v: serde_json::Value = serde_json::from_slice(&out.stdout)?;
        let title = v.get("title").and_then(|t| t.as_str()).map(String::from);
        if v.get("_type").and_then(|t| t.as_str()) == Some("playlist") {
            let count = v
                .get("playlist_count")
                .and_then(|c| c.as_u64())
                .or_else(|| v.get("entries").and_then(|e| e.as_array()).map(|a| a.len() as u64));
            Ok(mdm_core::ResolveInfo { title, n_items: count })
        } else {
            Ok(mdm_core::ResolveInfo { title, n_items: None })
        }
    }

    async fn run(&self, job: Job, ctx: EngineCtx) -> std::result::Result<JobDone, JobError> {
        let args = build_args(&job, self.with_aria2, &ctx.cfg);
        debug!("yt-dlp args: {args:?}");

        let mut cmd = tokio::process::Command::new(&self.bin);
        cmd.args(args.iter().map(|s| s.as_str()))
            .env("NO_COLOR", "1")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        let mut child = cmd
            .spawn()
            .map_err(|e| JobError::Fatal(format!("spawn yt-dlp: {e}")))?;
        let pid = child.id().unwrap_or(0);
        self.register_child(&job.id, pid);

        let stdout = child.stdout.take().expect("stdout piped");
        let stderr = child.stderr.take().expect("stderr piped");

        // Keep a bounded stderr tail for error reporting.
        let stderr_task = tokio::spawn(async move {
            let mut reader = BufReader::new(stderr);
            let mut buf = String::new();
            loop {
                let mut line = String::new();
                match reader.read_line(&mut line).await {
                    Ok(0) => break,
                    Ok(_) => {
                        buf.push_str(&line);
                        if buf.len() > 16_000 {
                            let cut = buf.len() - 8_000;
                            buf.drain(..cut);
                        }
                    }
                    Err(_) => break,
                }
            }
            buf
        });

        let mut reader = BufReader::new(stdout);
        let mut last_emit = tokio::time::Instant::now() - Duration::from_secs(1);
        let mut final_path: Option<PathBuf> = None;
        let mut line_buf = String::new();

        // Main streaming loop: exit when stdout EOFs or the child terminates.
        loop {
            tokio::select! {
                biased;
                _ = ctx.cancel.cancelled() => {
                    signal::kill_tree(pid);
                    self.unregister_child(&job.id);
                    return Err(JobError::Cancelled);
                }
                status = child.wait() => {
                    // Child exited before stdout EOF; drain the rest quickly.
                    let final_status = status;
                    if let Ok(s) = &final_status
                        && s.success()
                    {
                        // Drain remaining stdout lines best-effort.
                        let deadline = tokio::time::sleep(Duration::from_millis(500));
                        tokio::pin!(deadline);
                        loop {
                            tokio::select! {
                                _ = &mut deadline => break,
                                n = reader.read_line(&mut line_buf) => {
                                    if n.unwrap_or(0) == 0 { break; }
                                    handle_line(
                                        line_buf.trim_end_matches(['\r','\n']),
                                        &job,
                                        &ctx,
                                        &mut final_path,
                                        &mut last_emit,
                                    );
                                    line_buf.clear();
                                }
                            }
                        }
                    }
                    let stderr_tail = stderr_task.await.unwrap_or_default();
                    self.unregister_child(&job.id);
                    return match final_status {
                        Ok(s) if s.success() => {
                            let title = final_path
                                .as_ref()
                                .and_then(|p| p.file_stem())
                                .map(|s| s.to_string_lossy().into_owned());
                            Ok(JobDone { output_path: final_path, title })
                        }
                        Ok(s) => {
                            let msg = format!(
                                "yt-dlp exited with code {}: {}",
                                s.code().unwrap_or(-1),
                                last_lines(&stderr_tail, 12)
                            );
                            if is_fatal(&msg) {
                                Err(JobError::Fatal(msg))
                            } else {
                                Err(JobError::Retry(msg))
                            }
                        }
                        Err(e) => Err(JobError::Retry(format!("yt-dlp wait error: {e}"))),
                    };
                }
                n = reader.read_line(&mut line_buf) => {
                    match n {
                        Ok(0) => { /* EOF: fall through to wait branch */ }
                        Ok(_) => {
                            handle_line(
                                line_buf.trim_end_matches(['\r', '\n']),
                                &job,
                                &ctx,
                                &mut final_path,
                                &mut last_emit,
                            );
                            line_buf.clear();
                        }
                        Err(e) => {
                            warn!("stdout read error: {e}");
                            break;
                        }
                    }
                }
            }
        }

        // stdout EOF before exit: wait for the child to terminate.
        let status = tokio::select! {
            biased;
            _ = ctx.cancel.cancelled() => {
                signal::kill_tree(pid);
                self.unregister_child(&job.id);
                return Err(JobError::Cancelled);
            }
            s = child.wait() => s,
        };

        let stderr_tail = stderr_task.await.unwrap_or_default();
        self.unregister_child(&job.id);

        match status {
            Ok(s) if s.success() => {
                let title = final_path
                    .as_ref()
                    .and_then(|p| p.file_stem())
                    .map(|s| s.to_string_lossy().into_owned());
                Ok(JobDone { output_path: final_path, title })
            }
            Ok(s) => {
                let msg = format!(
                    "yt-dlp exited with code {}: {}",
                    s.code().unwrap_or(-1),
                    last_lines(&stderr_tail, 12)
                );
                if is_fatal(&msg) {
                    Err(JobError::Fatal(msg))
                } else {
                    Err(JobError::Retry(msg))
                }
            }
            Err(e) => Err(JobError::Retry(format!("yt-dlp wait error: {e}"))),
        }
    }

    async fn remove(&self, job: &Job) {
        let pid = self.children.lock().unwrap().get(&job.id).copied();
        if let Some(pid) = pid {
            signal::kill_tree(pid);
        }
    }
}

fn build_args(job: &Job, with_aria2: bool, cfg: &Config) -> Vec<String> {
    let out = job.output_dir.display().to_string();
    let mut args: Vec<String> = vec![
        "--no-config".into(),
        "--no-warnings".into(),
        "--newline".into(),
        "--progress".into(),
        "--continue".into(),
        "--print".into(),
        "after_move:filepath".into(),
        "-P".into(),
        out,
    ];

    // App-level network settings (settings page): cap + proxy.
    if let Some(limit) = cfg.speed_limit.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        args.extend(["--limit-rate".into(), limit.to_string()]);
    }
    if let Some(proxy) = cfg.proxy.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        args.extend(["--proxy".into(), proxy.to_string()]);
    }

    // Media extras: subtitles + embedding. Embeds need ffmpeg on hand.
    let have_ffmpeg = bin_path("ffmpeg", cfg).is_ok();
    if cfg.write_subtitles {
        args.push("--write-subs".into());
    }
    if cfg.write_auto_subs {
        args.push("--write-auto-subs".into());
    }
    if have_ffmpeg {
        if cfg.embed_metadata {
            args.push("--embed-metadata".into());
        }
        if cfg.embed_thumbnail {
            args.push("--embed-thumbnail".into());
        }
    }

    match job.kind {
        mdm_core::JobKind::Video => {
            // "best" is yt-dlp's default; passing -f best is rejected in
            // recent releases.
            if job.format != "best" {
                args.extend(["-f".into(), job.format.clone()]);
            }
            if with_aria2 {
                args.extend([
                    "--external-downloader".into(),
                    "aria2c".into(),
                    "--external-downloader-args".into(),
                    "aria2c:-x 8 -s 8 -j 8".into(),
                ]);
            }
        }
        mdm_core::JobKind::Audio => {
            args.extend([
                "-f".into(),
                "bestaudio".into(),
                "--extract-audio".into(),
                "--audio-format".into(),
                "mp3".into(),
                "--audio-quality".into(),
                "0".into(),
            ]);
        }
        // Direct/torrent kinds never route here in Phase 1.
        _ => {
            args.extend(["-f".into(), job.format.clone()]);
        }
    }

    args.push(job.url.clone());
    args
}
