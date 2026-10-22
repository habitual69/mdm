use std::path::PathBuf;

use chrono::{DateTime, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JobKind {
    Video,
    Audio,
    Direct,
    Torrent,
    Magnet,
}

impl JobKind {
    pub const ALL: [JobKind; 5] = [
        JobKind::Video,
        JobKind::Audio,
        JobKind::Direct,
        JobKind::Torrent,
        JobKind::Magnet,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            JobKind::Video => "video",
            JobKind::Audio => "audio",
            JobKind::Direct => "direct",
            JobKind::Torrent => "torrent",
            JobKind::Magnet => "magnet",
        }
    }

    pub fn parse(s: &str) -> Option<JobKind> {
        let s = s.to_ascii_lowercase();
        JobKind::ALL.iter().copied().find(|k| k.as_str() == s)
    }

    /// Best-effort auto-detection from a URL / path.
    pub fn detect(input: &str) -> JobKind {
        let lower = input.to_ascii_lowercase();
        if input.starts_with("magnet:") {
            return JobKind::Magnet;
        }
        // Strip query string before checking extension.
        let path_part = lower.split('?').next().unwrap_or(&lower);
        if path_part.ends_with(".torrent") {
            return JobKind::Torrent;
        }
        JobKind::Video
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EngineKind {
    Ytdlp,
    Aria2,
}

impl EngineKind {
    pub fn as_str(self) -> &'static str {
        match self {
            EngineKind::Ytdlp => "ytdlp",
            EngineKind::Aria2 => "aria2",
        }
    }

    pub fn from_job_kind(kind: JobKind) -> EngineKind {
        match kind {
            JobKind::Video | JobKind::Audio => EngineKind::Ytdlp,
            JobKind::Direct | JobKind::Torrent | JobKind::Magnet => EngineKind::Aria2,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JobState {
    Queued,
    Running,
    Paused,
    Done,
    Failed,
}

impl JobState {
    pub const ACTIVE: [JobState; 2] = [JobState::Queued, JobState::Running];
    pub const ALL: [JobState; 5] = [
        JobState::Queued,
        JobState::Running,
        JobState::Paused,
        JobState::Done,
        JobState::Failed,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            JobState::Queued => "queued",
            JobState::Running => "running",
            JobState::Paused => "paused",
            JobState::Done => "done",
            JobState::Failed => "failed",
        }
    }

    pub fn parse(s: &str) -> Option<JobState> {
        let s = s.to_ascii_lowercase();
        JobState::ALL.iter().copied().find(|k| k.as_str() == s)
    }

    pub fn is_terminal(self) -> bool {
        matches!(self, JobState::Done | JobState::Failed)
    }
}

pub fn now_millis() -> i64 {
    Utc::now().timestamp_millis()
}

pub(crate) fn dt_from_millis(ms: i64) -> DateTime<Utc> {
    Utc.timestamp_millis_opt(ms).single().unwrap_or_default()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    pub id: String,
    #[serde(default)]
    pub parent_id: Option<String>,
    pub url: String,
    pub kind: JobKind,
    pub engine: EngineKind,
    pub state: JobState,
    #[serde(default)]
    pub title: Option<String>,
    pub output_dir: PathBuf,
    #[serde(default)]
    pub output_path: Option<PathBuf>,
    pub format: String,
    pub progress: f32,
    #[serde(default)]
    pub downloaded_bytes: Option<i64>,
    #[serde(default)]
    pub total_bytes: Option<i64>,
    #[serde(default)]
    pub speed_bps: Option<i64>,
    #[serde(default)]
    pub eta_sec: Option<i64>,
    /// aria2 gid, populated while a job is alive in the aria2c daemon.
    #[serde(default)]
    pub external_id: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    pub retries: i64,
    #[serde(default)]
    pub retry_at: Option<i64>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Job {
    pub fn new(url: impl Into<String>, kind: JobKind, output_dir: PathBuf, format: String) -> Job {
        let now = now_millis();
        Job {
            id: Uuid::new_v4().to_string(),
            parent_id: None,
            url: url.into(),
            kind,
            engine: EngineKind::from_job_kind(kind),
            state: JobState::Queued,
            title: None,
            output_dir,
            output_path: None,
            format,
            progress: 0.0,
            downloaded_bytes: None,
            total_bytes: None,
            speed_bps: None,
            eta_sec: None,
            external_id: None,
            error: None,
            retries: 0,
            retry_at: None,
            created_at: dt_from_millis(now),
            updated_at: dt_from_millis(now),
        }
    }

    pub fn short_id(&self) -> &str {
        let end = self.id.len().min(8);
        &self.id[..end]
    }
}

/// Partial update for a job row; `None` fields are left untouched.
#[derive(Debug, Clone, Default)]
pub struct JobPatch {
    pub id: String,
    pub state: Option<JobState>,
    pub title: Option<String>,
    pub progress: Option<f32>,
    pub downloaded_bytes: Option<i64>,
    pub total_bytes: Option<i64>,
    pub speed_bps: Option<i64>,
    pub eta_sec: Option<i64>,
    pub external_id: Option<String>,
    pub error: Option<String>,
    /// When true, sets `error` to NULL (used on resume).
    pub clear_error: bool,
    pub output_path: Option<PathBuf>,
    /// Set retries to a specific value.
    pub retries: Option<i64>,
    /// Increment retries by 1 when set.
    pub bump_retries: bool,
    /// Some(Some(ts)) sets; Some(None) clears.
    pub retry_at: Option<Option<i64>>,
    /// Force `updated_at` to a specific time (defaults to now).
    pub updated_at: Option<i64>,
}

impl JobPatch {
    pub fn with_state(state: JobState) -> JobPatch {
        JobPatch { state: Some(state), ..Default::default() }
    }

    pub fn for_id(id: impl Into<String>, state: JobState) -> JobPatch {
        let mut p = Self::with_state(state);
        p.id = id.into();
        p
    }
}
