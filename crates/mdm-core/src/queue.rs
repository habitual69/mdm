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
        let kind = req.kind.unwrap_or_else(|| JobKind::detect(&req.url));
        let cfg = self.inner.cfg.read().unwrap();
        let mut job = Job::new(
            req.url.trim(),
            kind,
            req.output_dir.unwrap_or_else(|| cfg.output_dir.clone()),
            req.format.unwrap_or_else(|| "best".into()),
        );
        job.select_files = req.select_files.clone().filter(|l| !l.is_empty());
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
            if removal_requested {
                let _ = st.inner.db.delete_job(job_id);
                st.emit(Event::JobRemoved { id: job_id.into() });
            } else if let Ok(Some(job)) = st.inner.db.get_job(job_id)
                && job.state == JobState::Running
            {
                apply_and_emit(st, JobPatch::for_id(job_id, JobState::Paused));
            }
        }
        Err(err) => {
            if removal_requested {
                let _ = st.inner.db.delete_job(job_id);
                st.emit(Event::JobRemoved { id: job_id.into() });
                return;
            }
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

    if job.state == JobState::Running {
        st.removals.insert(id.to_string());
        if let Some(token) = st.running.get(id) {
            token.cancel();
        }
        let engine = resolve_engine(st, &job);
        engine.remove(&job).await;
    } else {
        let _ = st.inner.db.delete_job(id);
        st.emit(Event::JobRemoved { id: id.to_string() });
    }
    Ok(())
}
