use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use tokio::sync::{broadcast, mpsc, oneshot};
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use crate::config::Config;
use crate::db::Db;
use crate::engine::{Engine, EngineCtx, JobDone, JobError, RunnerMsg, RunnerTx};
use crate::error::{Error, Result};
use crate::events::Event;
use crate::model::{EngineKind, Job, JobKind, JobPatch, JobState, now_millis};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NewJobReq {
    pub url: String,
    /// Auto-detects when None.
    pub kind: Option<JobKind>,
    /// Engine format selector (yt-dlp `-f`), default "best".
    pub format: Option<String>,
    /// Torrent file selection (1-based indices into the .torrent metadata).
    #[serde(default)]
    pub select_files: Option<Vec<i64>>,
    /// Overrides cfg.output_dir.
    pub output_dir: Option<PathBuf>,
    /// Netscape-format cookie content passed from the browser extension or user.
    #[serde(default)]
    pub cookies: Option<String>,
    /// Optional filename or display title (from extension or user).
    #[serde(default)]
    pub title: Option<String>,
}

enum Cmd {
    Pause { id: String, reply: oneshot::Sender<Result<()>> },
    Resume { id: String, reply: oneshot::Sender<Result<()>> },
    Remove { id: String, reply: oneshot::Sender<Result<()>> },
    Wake,
    Shutdown { reply: oneshot::Sender<()> },
}

struct Inner {
    db: Db,
    cfg: RwLock<Config>,
    engines: HashMap<EngineKind, Arc<dyn Engine>>,
    events: broadcast::Sender<Event>,
    runner_tx: RunnerTx,
    cmd_tx: mpsc::UnboundedSender<Cmd>,
}

/// Owned cloneable handle to the queue.
#[derive(Clone)]
pub struct Queue {
    inner: Arc<Inner>,
}

impl Queue {
    /// Builds the queue, demotes stale running rows from prior sessions, and
    /// starts the async manager loop.
    pub fn new(cfg: Config, engines: Vec<Arc<dyn Engine>>) -> Result<Arc<Queue>> {
        let db = Db::open_default()?;
        Self::new_with_db(cfg, engines, db)
    }

    pub fn new_with_db(cfg: Config, engines: Vec<Arc<dyn Engine>>, db: Db) -> Result<Arc<Queue>> {
        let stale = db.reset_stale_running()?;
        if stale > 0 {
            info!("resumed {stale} unfinished job(s) from previous session");
        }

        let mut map = HashMap::new();
        for e in engines {
            map.insert(e.engine_kind(), e);
        }
        for k in [EngineKind::Ytdlp, EngineKind::Aria2] {
            if !map.contains_key(&k) {
                return Err(Error::Engine(format!("engine '{}' not registered", k.as_str())));
            }
        }

        let (events, _) = broadcast::channel::<Event>(256);
        let (runner_tx, mut runner_rx) = mpsc::unbounded_channel::<RunnerMsg>();
        let (cmd_tx, mut cmd_rx) = mpsc::unbounded_channel::<Cmd>();

        let inner = Arc::new(Inner {
            db,
            cfg: RwLock::new(cfg),
            engines: map,
            events,
            runner_tx,
            cmd_tx,
        });

        let state = ManagerShared { inner: inner.clone(), running: HashMap::new(), removals: HashSet::new() };
        tokio::spawn(async move {
            manager_loop(state, &mut runner_rx, &mut cmd_rx).await;
        });

        Ok(Arc::new(Queue { inner }))
    }

    pub fn subscribe_events(&self) -> broadcast::Receiver<Event> {
        self.inner.events.subscribe()
    }

    pub fn config(&self) -> Config {
        self.inner.cfg.read().unwrap().clone()
    }

    /// Hot-swaps the in-memory config (settings page); callers persist to
    /// disk separately.
    pub fn replace_config(&self, cfg: Config) {
        *self.inner.cfg.write().unwrap() = cfg;
    }

    /// Push a swapped config to engines that own long-lived state (aria2
    /// globals). Per-run engines pick `ctx.cfg` up automatically.
    pub async fn notify_engines(&self, cfg: &Config) {
        for e in self.inner.engines.values() {
            e.update_config(cfg).await;
        }
    }

    pub fn db(&self) -> &Db {
        &self.inner.db
    }

    pub fn add(&self, req: NewJobReq) -> Result<Job> {
        if req.url.trim().is_empty() {
            return Err(Error::Engine("empty URL".into()));
        }
        if req.url.trim().starts_with('-') && !req.url.contains(':') {
            return Err(Error::Engine("refusing flag-like URL".into()));
        }
        let kind = req.kind.unwrap_or_else(|| {
            if let Some(ref fmt) = req.format {
                let f_low = fmt.to_ascii_lowercase();
                if f_low.contains("bestaudio")
                    || f_low.contains("audio")
                    || matches!(f_low.as_str(), "mp3" | "flac" | "m4a" | "opus" | "wav" | "aac" | "vorbis")
                {
                    return JobKind::Audio;
                }
            }
            if let Some(t) = req.title.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
                let detected = JobKind::detect(t);
                if detected != JobKind::Video {
                    return detected;
                }
            }
            JobKind::detect(&req.url)
        });

        // Ensure binaries, executables, packages and archives never route to media engines (yt-dlp/ffmpeg)
        let kind = match kind {
            JobKind::Video | JobKind::Audio => {
                let lower = req.url.to_ascii_lowercase();
                let path_part = lower.split('?').next().unwrap_or(&lower);
                let ext = path_part.rsplit('.').next().unwrap_or("");
                if matches!(
                    ext,
                    "exe" | "msi" | "apk" | "appimage" | "deb" | "rpm" | "dmg" | "pkg" | "bin" | "zip" | "7z" | "rar" | "tar" | "gz" | "iso"
                ) {
                    JobKind::Direct
                } else {
                    kind
                }
            }
            _ => kind,
        };

        let cfg = self.inner.cfg.read().unwrap();
        let base_dir = req.output_dir.unwrap_or_else(|| cfg.output_dir.clone());
        let category = crate::model::category_for_with_title(&req.url, req.title.as_deref(), kind);
        let final_dir = if base_dir.ends_with(category) {
            base_dir
        } else {
            base_dir.join(category)
        };
        let _ = std::fs::create_dir_all(&final_dir);
        let initial_format = match kind {
            JobKind::Audio => {
                let raw_fmt = req.format.as_deref().unwrap_or("").trim().to_ascii_lowercase();
                let stripped = raw_fmt.strip_prefix("audio:").unwrap_or(&raw_fmt).trim();
                if matches!(stripped, "flac" | "mp3" | "m4a" | "opus" | "wav" | "aac" | "vorbis") {
                    stripped.to_string()
                } else if !cfg.ytdlp_audio_format.is_empty() && !matches!(cfg.ytdlp_audio_format.as_str(), "best" | "original") {
                    cfg.ytdlp_audio_format.clone()
                } else {
                    "mp3".into()
                }
            }
            _ => req.format.unwrap_or_else(|| "best".into()),
        };
        let mut job = Job::new(
            req.url.trim(),
            kind,
            final_dir,
            initial_format,
        );
        job.select_files = req.select_files.clone().filter(|l| !l.is_empty());
        if let Some(t) = req.title.clone().filter(|s| !s.trim().is_empty()) {
            job.title = Some(t.trim().to_string());
        }

        if let Some(cookie_data) = req.cookies.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            let cookie_dir = crate::config::data_dir().join("cookies");
            let _ = std::fs::create_dir_all(&cookie_dir);
            let _ = std::fs::write(crate::config::data_dir().join("cookies.txt"), cookie_data);
            if let Some(host) = req.url.split("://").nth(1).and_then(|s| s.split('/').next()).and_then(|s| s.split(':').next()) {
                let host_clean: String = host.chars().map(|c| if c.is_alphanumeric() || c == '.' || c == '-' { c } else { '_' }).collect();
                let _ = std::fs::write(cookie_dir.join(format!("{host_clean}.txt")), cookie_data);
            }
        }

        self.inner.db.insert_job(&job)?;
        let _ = self.inner.events.send(Event::JobUpdated { job: Box::new(job.clone()) });
        let _ = self.inner.cmd_tx.send(Cmd::Wake);
        Ok(job)
    }

    pub fn get(&self, id: &str) -> Result<Option<Job>> {
        self.inner.db.get_job(id)
    }

    pub fn list(&self, states: Option<&[JobState]>) -> Result<Vec<Job>> {
        self.inner.db.list_jobs(states)
    }

    pub async fn pause(&self, id: &str) -> Result<()> {
        self.cmd(|reply| Cmd::Pause { id: id.into(), reply }).await
    }

    pub async fn resume(&self, id: &str) -> Result<()> {
        self.cmd(|reply| Cmd::Resume { id: id.into(), reply }).await
    }

    pub async fn pause_all(&self) -> Result<usize> {
        let active = self.inner.db.list_jobs(Some(&[JobState::Running, JobState::Queued]))?;
        let mut count = 0;
        for j in active {
            if self.pause(&j.id).await.is_ok() {
                count += 1;
            }
        }
        Ok(count)
    }

    pub async fn resume_all(&self) -> Result<usize> {
        let paused_or_failed = self.inner.db.list_jobs(Some(&[JobState::Paused, JobState::Failed]))?;
        let mut count = 0;
        for j in paused_or_failed {
            if self.resume(&j.id).await.is_ok() {
                count += 1;
            }
        }
        Ok(count)
    }

    pub async fn remove(&self, id: &str) -> Result<()> {
        self.cmd(|reply| Cmd::Remove { id: id.into(), reply }).await
    }

    pub async fn shutdown(&self) {
        for e in self.inner.engines.values() {
            e.shutdown().await;
        }
        let (tx, rx) = oneshot::channel();
        let _ = self.inner.cmd_tx.send(Cmd::Shutdown { reply: tx });
        let _ = tokio::time::timeout(Duration::from_secs(5), rx).await;
    }

    async fn cmd(&self, mk: impl FnOnce(oneshot::Sender<Result<()>>) -> Cmd) -> Result<()> {
        let (tx, rx) = oneshot::channel();
        self.inner
            .cmd_tx
            .send(mk(tx))
            .map_err(|_| Error::Engine("queue manager stopped".into()))?;
        rx.await
            .map_err(|_| Error::Engine("queue manager dropped reply".into()))?
    }
}

struct ManagerShared {
    inner: Arc<Inner>,
    running: HashMap<String, CancellationToken>,
    removals: HashSet<String>,
}

impl ManagerShared {
    fn emit(&self, ev: Event) {
        let _ = self.inner.events.send(ev);
    }
}

async fn manager_loop(
    mut st: ManagerShared,
    runner_rx: &mut mpsc::UnboundedReceiver<RunnerMsg>,
    cmd_rx: &mut mpsc::UnboundedReceiver<Cmd>,
) {
    let mut tick = tokio::time::interval(Duration::from_millis(400));
    loop {
        tokio::select! {
            biased;
            msg = runner_rx.recv() => {
                match msg {
                    Some(RunnerMsg::Patch { job_id, patch }) => {
                        let mut patch = patch;
                        patch.id = job_id;
                        if let Ok(Some(job)) = st.inner.db.apply_patch(&patch) {
                            st.emit(Event::JobUpdated { job: Box::new(job) });
                        }
                    }
                    Some(RunnerMsg::Done { job_id, result }) => {
                        finalize(&mut st, &job_id, result);
                        admit(&mut st).await;
                    }
                    None => break, // runner channel closed -> queue dropped
                }
            }
            cmd = cmd_rx.recv() => {
                match cmd {
                    Some(c) => {
                        if handle_cmd(&mut st, c).await {
                            break;
                        }
                    }
                    None => break,
                }
            }
            _ = tick.tick() => {
                admit(&mut st).await;
            }
        }
    }
    debug!("queue manager stopped");
}

fn resolve_engine(st: &ManagerShared, job: &Job) -> Arc<dyn Engine> {
    let kind = EngineKind::from_job_kind(job.kind);
    st.inner.engines[&kind].clone()
}

async fn admit(st: &mut ManagerShared) {
    let max = st.inner.cfg.read().unwrap().max_concurrent.max(1) as usize;
    while st.running.len() < max {
        let job = match st.inner.db.claim_next_queued() {
            Ok(Some(j)) => j,
            Ok(None) => break,
            Err(e) => {
                warn!("claim_next_queued failed: {e}");
                break;
            }
        };

        let engine = resolve_engine(st, &job);
        let token = CancellationToken::new();
        st.running.insert(job.id.clone(), token.clone());
        let ctx = EngineCtx {
            cfg: st.inner.cfg.read().unwrap().clone(),
            cancel: token,
            runner_tx: st.inner.runner_tx.clone(),
            events: st.inner.events.clone(),
        };
        debug!("starting job {} via {}", job.short_id(), job.engine.as_str());
        let runner_tx = st.inner.runner_tx.clone();
        let job_id = job.id.clone();
        tokio::spawn(async move {
            let result = engine.run(job.clone(), ctx).await;
            let _ = runner_tx.send(RunnerMsg::Done { job_id, result });
        });
    }

    if st.running.is_empty() && st.inner.db.count_queued().unwrap_or(0) == 0 {
        st.emit(Event::QueueIdle);
    }
}

fn finalize(st: &mut ManagerShared, job_id: &str, result: std::result::Result<JobDone, JobError>) {
    st.running.remove(job_id);
    let removal_requested = st.removals.remove(job_id);

    if removal_requested {
        if let Ok(Some(job)) = st.inner.db.get_job(job_id) {
            let engine = resolve_engine(st, &job);
            tokio::spawn(async move {
                engine.remove(&job).await;
            });
        }
        let _ = st.inner.db.delete_job(job_id);
        st.emit(Event::JobRemoved { id: job_id.into() });
        return;
    }

    match result {
        Ok(done) => {
            let mut patch = JobPatch::for_id(job_id, JobState::Done);
            patch.progress = Some(100.0);
            patch.output_path = done.output_path;
            patch.title = done.title;
            patch.clear_error = true;
            apply_and_emit(st, patch);
        }
        Err(JobError::Cancelled) => {
            if let Ok(Some(job)) = st.inner.db.get_job(job_id)
                && job.state == JobState::Running
            {
                apply_and_emit(st, JobPatch::for_id(job_id, JobState::Paused));
            }
        }
        Err(err) => {
            let retries = st
                .inner
                .db
                .get_job(job_id)
                .ok()
                .flatten()
                .map(|j| j.retries)
                .unwrap_or(0);
            let max = st.inner.cfg.read().unwrap().max_retries as i64;
            let msg = err.message();
            match err {
                JobError::Retry(_) if retries < max => {
                    warn!("job {job_id} retryable: {msg} (attempt {}/{})", retries + 1, max);
                    let mut patch = JobPatch::for_id(job_id, JobState::Queued);
                    let backoff_secs: u64 =
                        30u64.saturating_mul(1 << (retries as u32).min(4)).min(600);
                    patch.retry_at = Some(Some(now_millis() + backoff_secs as i64 * 1000));
                    patch.bump_retries = true;
                    patch.error = Some(msg);
                    apply_and_emit(st, patch);
                }
                other => {
                    warn!("job {job_id} failed permanently: {}", other.message());
                    let mut patch = JobPatch::for_id(job_id, JobState::Failed);
                    patch.error = Some(other.message().chars().take(600).collect());
                    apply_and_emit(st, patch);
                }
            }
        }
    }
}

fn apply_and_emit(st: &mut ManagerShared, patch: JobPatch) {
    match st.inner.db.apply_patch(&patch) {
        Ok(Some(job)) => st.emit(Event::JobUpdated { job: Box::new(job) }),
        Ok(None) => {}
        Err(e) => warn!("db patch failed: {e}"),
    }
}

async fn handle_cmd(st: &mut ManagerShared, cmd: Cmd) -> bool {
    match cmd {
        Cmd::Wake => {
            admit(st).await;
            false
        }
        Cmd::Shutdown { reply } => {
            for token in st.running.values() {
                token.cancel();
            }
            st.running.clear();
            let _ = reply.send(());
            true
        }
        Cmd::Pause { id, reply } => {
            let result = pause(st, &id).await;
            let _ = reply.send(result);
            false
        }
        Cmd::Resume { id, reply } => {
            let result = resume(st, &id);
            let ok = result.is_ok();
            let _ = reply.send(result);
            if ok {
                admit(st).await;
            }
            false
        }
        Cmd::Remove { id, reply } => {
            let result = remove(st, &id).await;
            let ok = result.is_ok();
            let _ = reply.send(result);
            if ok {
                admit(st).await;
            }
            false
        }
    }
}

async fn pause(st: &mut ManagerShared, id: &str) -> Result<()> {
    let job = st
        .inner
        .db
        .get_job(id)?
        .ok_or_else(|| Error::Engine(format!("job {id} not found")))?;
    match job.state {
        JobState::Running => {
            if let Some(token) = st.running.get(id) {
                token.cancel();
            } else {
                warn!("pause: job {id} running without registry entry; demoting");
                apply_and_emit(st, JobPatch::for_id(id, JobState::Paused));
            }
        }
        JobState::Queued => {
            apply_and_emit(st, JobPatch::for_id(id, JobState::Paused));
        }
        _ => {}
    }
    Ok(())
}

fn resume(st: &mut ManagerShared, id: &str) -> Result<()> {
    let job = st
        .inner
        .db
        .get_job(id)?
        .ok_or_else(|| Error::Engine(format!("job {id} not found")))?;
    match job.state {
        JobState::Paused | JobState::Failed => {
            let mut patch = JobPatch::for_id(id, JobState::Queued);
            patch.retries = Some(0);
            patch.retry_at = Some(None);
            patch.clear_error = true;
            apply_and_emit(st, patch);
            Ok(())
        }
        _ => Err(Error::Engine(format!(
            "job {} is {} (not resumable)",
            job.short_id(),
            job.state.as_str()
        ))),
    }
}

async fn remove(st: &mut ManagerShared, id: &str) -> Result<()> {
    let job = st
        .inner
        .db
        .get_job(id)?
        .ok_or_else(|| Error::Engine(format!("job {id} not found")))?;

    let engine = resolve_engine(st, &job);
    engine.remove(&job).await;

    if job.state == JobState::Running && st.running.contains_key(id) {
        st.removals.insert(id.to_string());
        if let Some(token) = st.running.get(id) {
            token.cancel();
        }
    } else {
        let _ = st.inner.db.delete_job(id);
        st.emit(Event::JobRemoved { id: id.to_string() });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TestEngine {
        kind: EngineKind,
        removed: Arc<AtomicBool>,
    }

    #[async_trait]
    impl Engine for TestEngine {
        fn engine_kind(&self) -> EngineKind {
            self.kind
        }

        async fn run(&self, _job: Job, ctx: EngineCtx) -> std::result::Result<JobDone, JobError> {
            let _ = ctx.cancel.cancelled().await;
            Err(JobError::Cancelled)
        }

        async fn remove(&self, _job: &Job) {
            self.removed.store(true, Ordering::SeqCst);
        }
    }

    fn tmp_db(tag: &str) -> Db {
        let mut name = format!("mdm_queue_test_{}_", tag);
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        name.push_str(&seed.to_string());
        Db::open(&std::env::temp_dir().join(name)).unwrap()
    }

    #[tokio::test]
    async fn remove_paused_or_queued_invokes_engine_remove() {
        let db = tmp_db("paused_remove");
        let removed_ytdlp = Arc::new(AtomicBool::new(false));
        let removed_aria2 = Arc::new(AtomicBool::new(false));

        let e1 = Arc::new(TestEngine {
            kind: EngineKind::Ytdlp,
            removed: removed_ytdlp.clone(),
        });
        let e2 = Arc::new(TestEngine {
            kind: EngineKind::Aria2,
            removed: removed_aria2.clone(),
        });

        let mut cfg = Config::default();
        cfg.max_concurrent = 0; // Prevent auto-admitting so it stays queued
        let q = Queue::new_with_db(cfg, vec![e1, e2], db).unwrap();

        let job = q
            .add(NewJobReq {
                url: "https://example.com/file.zip".into(),
                kind: Some(JobKind::Direct),
                format: None,
                select_files: None,
                output_dir: None,
                cookies: None,
                title: None,
            })
            .unwrap();
        let job_id = job.id;

        // Ensure job exists
        assert!(q.get(&job_id).unwrap().is_some());

        // Remove the queued job
        q.remove(&job_id).await.unwrap();

        // Verify engine.remove was called
        assert!(removed_aria2.load(Ordering::SeqCst));
        // Verify job is removed from queue/db
        assert!(q.get(&job_id).unwrap().is_none());
    }

    #[tokio::test]
    async fn remove_running_job_stops_and_removes_from_engine() {
        let db = tmp_db("running_remove");
        let removed_ytdlp = Arc::new(AtomicBool::new(false));
        let removed_aria2 = Arc::new(AtomicBool::new(false));

        let e1 = Arc::new(TestEngine {
            kind: EngineKind::Ytdlp,
            removed: removed_ytdlp.clone(),
        });
        let e2 = Arc::new(TestEngine {
            kind: EngineKind::Aria2,
            removed: removed_aria2.clone(),
        });

        let mut cfg = Config::default();
        cfg.max_concurrent = 1;
        let q = Queue::new_with_db(cfg, vec![e1, e2], db).unwrap();

        let job = q
            .add(NewJobReq {
                url: "https://example.com/video.mp4".into(),
                kind: Some(JobKind::Video),
                format: None,
                select_files: None,
                output_dir: None,
                cookies: None,
                title: None,
            })
            .unwrap();
        let job_id = job.id;

        // Wait briefly for job to be admitted and start running
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Remove the running job
        q.remove(&job_id).await.unwrap();

        // Verify engine.remove was called
        assert!(removed_ytdlp.load(Ordering::SeqCst));

        // Wait for manager finalize loop to process cancellation
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(q.get(&job_id).unwrap().is_none());
    }

    #[tokio::test]
    async fn pause_all_and_resume_all_updates_jobs() {
        let db = tmp_db("pause_resume_all");
        let removed_ytdlp = Arc::new(AtomicBool::new(false));
        let removed_aria2 = Arc::new(AtomicBool::new(false));

        let e1 = Arc::new(TestEngine {
            kind: EngineKind::Ytdlp,
            removed: removed_ytdlp.clone(),
        });
        let e2 = Arc::new(TestEngine {
            kind: EngineKind::Aria2,
            removed: removed_aria2.clone(),
        });

        let mut cfg = Config::default();
        cfg.max_concurrent = 0; // Keep queued
        let q = Queue::new_with_db(cfg, vec![e1, e2], db).unwrap();

        let job1 = q
            .add(NewJobReq {
                url: "https://example.com/f1.zip".into(),
                kind: Some(JobKind::Direct),
                format: None,
                select_files: None,
                output_dir: None,
                cookies: None,
                title: None,
            })
            .unwrap();

        let job2 = q
            .add(NewJobReq {
                url: "https://example.com/f2.zip".into(),
                kind: Some(JobKind::Direct),
                format: None,
                select_files: None,
                output_dir: None,
                cookies: None,
                title: None,
            })
            .unwrap();

        assert_eq!(q.get(&job1.id).unwrap().unwrap().state, JobState::Queued);
        assert_eq!(q.get(&job2.id).unwrap().unwrap().state, JobState::Queued);

        let paused_count = q.pause_all().await.unwrap();
        assert_eq!(paused_count, 2);

        assert_eq!(q.get(&job1.id).unwrap().unwrap().state, JobState::Paused);
        assert_eq!(q.get(&job2.id).unwrap().unwrap().state, JobState::Paused);

        let resumed_count = q.resume_all().await.unwrap();
        assert_eq!(resumed_count, 2);

        let s1 = q.get(&job1.id).unwrap().unwrap().state;
        let s2 = q.get(&job2.id).unwrap().unwrap().state;
        assert!(matches!(s1, JobState::Queued | JobState::Running));
        assert!(matches!(s2, JobState::Queued | JobState::Running));
    }
}
