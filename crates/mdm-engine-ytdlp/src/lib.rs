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
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tracing::{debug, warn};

pub struct Ytdlp {
    bin: PathBuf,
    /// yt-dlp's external_downloader passthrough requires aria2c; optional.
    with_aria2: bool,
    /// job_id -> yt-dlp child pid (for out-of-band kills on remove()).
    children: Mutex<HashMap<String, u32>>,
}

/// Errors that will not get better by retrying.
const FATAL_PATTERNS: [&str; 6] = [
    "Unsupported URL",
    "not a valid URL",
    "Private video",
    "Sign in to confirm",
    "HTTP Error 404",
    "HTTP Error 403",
];

fn is_fatal(msg: &str) -> bool {
    FATAL_PATTERNS.iter().any(|p| msg.contains(p))
}

pub use mdm_core::find_js_runtime;

impl Ytdlp {
    async fn run_once(
        &self,
        job: &Job,
        ctx: &EngineCtx,
        offload: bool,
        embed_thumbnail: bool,
    ) -> std::result::Result<JobDone, JobError> {
        let args = build_args(job, offload, &ctx.cfg, embed_thumbnail);
        debug!("yt-dlp args: {args:?}");

        let mut cmd = tokio::process::Command::new(&self.bin);
        #[cfg(unix)]
        cmd.process_group(0);
        let _ = std::fs::create_dir_all(&job.output_dir);
        cmd.current_dir(&job.output_dir);

        // Enrich PATH so yt-dlp can locate node/aria2c even when launched from GUI environments
        let mut path_dirs = vec![
            mdm_core::data_dir().join("bin"),
        ];
        if let Some(ref js) = find_js_runtime() {
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
        let mut raw_chunk = [0u8; 2048];
        let mut pending = String::new();

        // Main streaming loop: exit when stdout EOFs or the child terminates.
        loop {
            tokio::select! {
                biased;
                _ = ctx.cancel.cancelled() => {
                    let _ = child.start_kill();
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
                                n = reader.read(&mut raw_chunk) => {
                                    let count = n.unwrap_or(0);
                                    if count == 0 { break; }
                                    pending.push_str(&String::from_utf8_lossy(&raw_chunk[..count]));
                                    while let Some(idx) = pending.find(['\r', '\n']) {
                                        let line: String = pending.drain(..idx).collect();
                                        while pending.starts_with('\r') || pending.starts_with('\n') {
                                            pending.drain(..1);
                                        }
                                        let trimmed = line.trim();
                                        if !trimmed.is_empty() {
                                            handle_line(
                                                trimmed,
                                                job,
                                                ctx,
                                                &mut final_path,
                                                &mut last_emit,
                                            );
                                        }
                                    }
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
                            // If postprocessing (like AVIF thumbnail conversion / cover art embedding) failed,
                            // but the media file itself was already completely downloaded to disk, accept it!
                            if msg.contains("Postprocessing") || msg.contains("ThumbnailsConvertor") || msg.contains("EmbedThumbnail") {
                                if let Some(ref p) = final_path {
                                    if p.is_file() && p.metadata().map(|m| m.len() > 0).unwrap_or(false) {
                                        warn!("yt-dlp postprocessing non-fatal warning; media file exists at {:?}", p);
                                        let title = p.file_stem().map(|stem| stem.to_string_lossy().into_owned());
                                        return Ok(JobDone { output_path: Some(p.clone()), title });
                                    }
                                }
                                if let Ok(entries) = std::fs::read_dir(&job.output_dir) {
                                    for entry in entries.flatten() {
                                        let p = entry.path();
                                        let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("");
                                        if !p.to_string_lossy().ends_with(".part") && matches!(ext, "mp4" | "mkv" | "webm" | "mp3" | "m4a") {
                                            if let Ok(meta) = p.metadata() {
                                                if meta.len() > 1024 * 512 {
                                                    let stem = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                                                    if job.title.as_deref().map(|t| stem.contains(t) || t.contains(&stem)).unwrap_or(false)
                                                        || job.url.split('/').any(|seg| !seg.is_empty() && stem.contains(seg)) {
                                                        warn!("yt-dlp postprocessing non-fatal warning; found downloaded media at {:?}", p);
                                                        return Ok(JobDone { output_path: Some(p), title: Some(stem) });
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            if is_fatal(&msg) {
                                Err(JobError::Fatal(msg))
                            } else {
                                Err(JobError::Retry(msg))
                            }
                        }
                        Err(e) => Err(JobError::Retry(format!("yt-dlp wait error: {e}"))),
                    };
                }
                n = reader.read(&mut raw_chunk) => {
                    match n {
                        Ok(0) => { /* EOF: fall through to wait branch */ }
                        Ok(count) => {
                            pending.push_str(&String::from_utf8_lossy(&raw_chunk[..count]));
                            while let Some(idx) = pending.find(['\r', '\n']) {
                                let line: String = pending.drain(..idx).collect();
                                while pending.starts_with('\r') || pending.starts_with('\n') {
                                    pending.drain(..1);
                                }
                                let trimmed = line.trim();
                                if !trimmed.is_empty() {
                                    handle_line(
                                        trimmed,
                                        job,
                                        ctx,
                                        &mut final_path,
                                        &mut last_emit,
                                    );
                                }
                            }
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
                let _ = child.start_kill();
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
        parse::Line::Title(title) => {
            if !title.is_empty() {
                let mut patch = JobPatch::for_id(&job.id, JobState::Running);
                patch.title = Some(title);
                ctx.send_patch(&job.id, patch);
            }
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
        let offload = self.with_aria2 && ctx.cfg.aria2_offload;
        let result = self.run_once(&job, &ctx, offload, ctx.cfg.embed_thumbnail).await;
        match result {
            // Postprocessing / thumbnail error -> retry cleanly without thumbnail embedding
            Err(JobError::Retry(ref msg)) | Err(JobError::Fatal(ref msg))
                if msg.contains("Postprocessing") || msg.contains("ThumbnailsConvertor") =>
            {
                warn!(
                    "job {}: thumbnail postprocessing failed — retrying cleanly without thumbnail embedding",
                    job.short_id()
                );
                self.run_once(&job, &ctx, false, false).await
            }
            // aria2 offload failed -> let yt-dlp download natively so the
            // job still completes on networks that block aria2's path.
            Err(e) if offload && ctx.cfg.aria2_fallback => {
                warn!(
                    "job {}: aria2 offload failed ({e:?}) — retrying with yt-dlp's native downloader",
                    job.short_id()
                );
                self.run_once(&job, &ctx, false, ctx.cfg.embed_thumbnail).await
            }
            // Format unavailable error -> retry with universal best selector
            Err(JobError::Retry(ref msg)) | Err(JobError::Fatal(ref msg))
                if msg.to_lowercase().contains("requested format is not available")
                    || msg.to_lowercase().contains("no suitable format") =>
            {
                warn!(
                    "job {}: format unavailable — retrying with universal auto format",
                    job.short_id()
                );
                let mut fallback_job = job.clone();
                fallback_job.format = "bv*+ba/b[acodec!=none]/best".into();
                let mut fallback_ctx = ctx.clone();
                fallback_ctx.cfg.ytdlp_max_height = 0;
                self.run_once(&fallback_job, &fallback_ctx, false, ctx.cfg.embed_thumbnail).await
            }
            other => other,
        }
    }



    async fn remove(&self, job: &Job) {
        let pid = self.children.lock().unwrap().remove(&job.id);
        if let Some(pid) = pid {
            signal::kill_tree(pid);
        }
    }
}

fn build_args(job: &Job, offload: bool, cfg: &Config, embed_thumbnail: bool) -> Vec<String> {
    let out = job.output_dir.display().to_string();
    let mut args: Vec<String> = vec![
        "--no-config".into(),
        "--no-warnings".into(),
        "--newline".into(),
        "--progress".into(),
        "--continue".into(),
        "--print".into(),
        "after_move:filepath".into(),
        // Early title line for the queue card (parsed as mdm_title:).
        "--print".into(),
        "mdm_title:%(title)s".into(),
        "-P".into(),
        out,
        "--trim-filenames".into(),
        "120".into(),
        "--windows-filenames".into(),
        "-o".into(),
        "%(title).100B [%(id).30B].%(ext)s".into(),
    ];

    // App-level network settings (settings page): cap + proxy.
    if let Some(limit) = cfg.speed_limit.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        args.extend(["--limit-rate".into(), limit.to_string()]);
    }
    if let Some(proxy) = cfg.proxy.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        args.extend(["--proxy".into(), proxy.to_string()]);
    }

    // JavaScript runtime for YouTube and streaming n-challenge / signature solver
    if let Some(ref js) = find_js_runtime() {
        let name = js.file_stem().and_then(|s| s.to_str()).unwrap_or("node");
        args.extend(["--js-runtimes".into(), format!("{name}:{}", js.display())]);
    }

    // Cookie session management: prioritize domain-specific cookie file, then global cookies.txt, then browser extraction
    let mut cookie_path: Option<PathBuf> = None;
    if let Some(host) = job.url.split("://").nth(1).and_then(|s| s.split('/').next()).and_then(|s| s.split(':').next()) {
        let host_clean: String = host.chars().map(|c| if c.is_alphanumeric() || c == '.' || c == '-' { c } else { '_' }).collect();
        let domain_file = mdm_core::data_dir().join("cookies").join(format!("{host_clean}.txt"));
        if domain_file.is_file() {
            cookie_path = Some(domain_file);
        }
    }
    if cookie_path.is_none() {
        let global_file = mdm_core::data_dir().join("cookies.txt");
        if global_file.is_file() {
            cookie_path = Some(global_file);
        }
    }

    if let Some(p) = cookie_path {
        args.extend(["--cookies".into(), p.display().to_string()]);
    } else {
        let browser = cfg.ytdlp_cookies_from_browser.trim();
        if !browser.is_empty() && browser != "none" {
            args.extend(["--cookies-from-browser".into(), browser.to_string()]);
        }
    }

    // Offload: when enabled, yt-dlp hands the byte transfers to the bundled
    // aria2c (segmented, multi-connection). A connect/read timeout keeps
    // dead hosts from hanging the job forever.
    if offload {
        let timeout = cfg.aria2_connect_timeout.clamp(5, 300);
        let mut dl_args = format!(
            "-x {} -s {} --connect-timeout {} --timeout {} --max-tries {} --retry-wait {} --summary-interval=1 --console-log-level=warn",
            cfg.aria2_connections.clamp(1, 16).max(1),
            cfg.aria2_splits.clamp(1, 16).max(1),
            timeout,
            timeout.max(60),
            cfg.aria2_max_tries.min(20),
            cfg.aria2_retry_wait.min(60),
        );
        // Transfer-tuning from the aria2 card. The user agent is left alone:
        // yt-dlp supplies its own headers/cookies for media URLs.
        if cfg.aria2_disk_cache > 0 {
            dl_args.push_str(&format!(" --disk-cache={}M", cfg.aria2_disk_cache.min(1024)));
        }
        dl_args.push_str(&format!(
            " --min-split-size={}M --file-allocation={}",
            cfg.aria2_min_split_size.clamp(1, 1024),
            cfg.aria2_file_allocation,
        ));
        if cfg.aria2_lowest_speed_limit > 0 {
            dl_args.push_str(&format!(
                " --lowest-speed-limit={}K",
                cfg.aria2_lowest_speed_limit.min(10240)
            ));
        }
        args.extend([
            "--external-downloader".into(),
            "aria2c".into(),
            "--external-downloader-args".into(),
            format!("aria2c:{dl_args}"),
        ]);
    }

    // Media extras: subtitles + embedding. Embeds need ffmpeg on hand.
    let have_ffmpeg = bin_path("ffmpeg", cfg).is_ok();
    if let Ok(ref ffmpeg_path) = bin_path("ffmpeg", cfg) {
        if let Some(parent) = ffmpeg_path.parent() {
            args.extend(["--ffmpeg-location".into(), parent.display().to_string()]);
        } else {
            args.extend(["--ffmpeg-location".into(), ffmpeg_path.display().to_string()]);
        }
    }
    if cfg.write_subtitles {
        args.push("--write-subs".into());
    }
    if cfg.write_auto_subs {
        args.push("--write-auto-subs".into());
    }
    if have_ffmpeg {
        let should_embed_meta = cfg.embed_metadata || job.kind == mdm_core::JobKind::Audio;
        let should_embed_thumb = embed_thumbnail || job.kind == mdm_core::JobKind::Audio;
        if should_embed_meta {
            args.push("--embed-metadata".into());
        }
        if should_embed_thumb {
            args.push("--embed-thumbnail".into());
            args.extend(["--convert-thumbnails".into(), "jpg".into()]);
        }
        if cfg.ytdlp_embed_subs && (cfg.write_subtitles || cfg.write_auto_subs) {
            args.push("--embed-subs".into());
        }
    }

    // yt-dlp engine card: parallel native-downloader fragments and the
    // merge container. Fragments only bite when the native downloader runs
    // (aria2 fallback); the merger flag applies to split video+audio jobs.
    if matches!(job.kind, mdm_core::JobKind::Video | mdm_core::JobKind::Audio) {
        args.extend([
            "--concurrent-fragments".into(),
            cfg.ytdlp_fragments.clamp(1, 16).to_string(),
        ]);
    }
    if job.kind == mdm_core::JobKind::Video && cfg.ytdlp_merge_format != "mkv" {
        args.extend(["--merge-output-format".into(), cfg.ytdlp_merge_format.clone()]);
    }

    match job.kind {
        mdm_core::JobKind::Video => {
            // "best" is yt-dlp's default; passing -f best is rejected in
            // recent releases.
            if job.format != "best" {
                args.extend(["-f".into(), job.format.clone()]);
            } else if cfg.ytdlp_max_height > 0 {
                // Settings page quality cap applies to default "best" jobs.
                // Always require audio alongside video and support multi-language dubbed tracks.
                let h = cfg.ytdlp_max_height.min(2160);
                args.extend([
                    "-f".into(),
                    format!("bv*[height<={h}]+ba/b[height<={h}][acodec!=none]/bv*+ba/b[acodec!=none]/best"),
                ]);
            } else {
                args.extend([
                    "-f".into(),
                    "bv*+ba/b[acodec!=none]/best".into(),
                ]);
            }
        }
        mdm_core::JobKind::Audio => {
            args.extend([
                "-f".into(),
                "ba*/bestaudio/b".into(),
                "--extract-audio".into(),
            ]);
            let mut target_fmt = "";
            let job_f = job.format.trim().to_ascii_lowercase();
            let stripped = job_f.strip_prefix("audio:").unwrap_or(&job_f).trim();
            if matches!(stripped, "flac" | "mp3" | "m4a" | "opus" | "wav" | "aac" | "vorbis") {
                target_fmt = stripped;
            } else if !cfg.ytdlp_audio_format.is_empty() && !matches!(cfg.ytdlp_audio_format.as_str(), "best" | "original") {
                target_fmt = cfg.ytdlp_audio_format.as_str();
            }
            let final_fmt = if target_fmt.is_empty() { "mp3" } else { target_fmt };
            args.extend([
                "--audio-format".into(),
                final_fmt.to_string(),
                "--audio-quality".into(),
                "0".into(),
            ]);

            // Embed rich metadata & album art tags:
            // 1. Fallback artist to uploader/channel if missing
            // 2. Extract clean artist and song title from "Artist - Title" strings
            // 3. Set album to playlist title, album, or uploader
            // 4. Set track number from playlist index if part of a playlist
            args.extend([
                "--parse-metadata".into(),
                "%(artist,creator,uploader,channel)s:%(artist)s".into(),
                "--parse-metadata".into(),
                "title:(?P<artist>.+?)\\s*-\\s*(?P<title>.+)".into(),
                "--parse-metadata".into(),
                "%(album,playlist_title,uploader)s:%(album)s".into(),
                "--parse-metadata".into(),
                "%(playlist_index&{}|)s:%(track_number)s".into(),
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
