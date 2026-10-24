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
        let input_clean = input.trim();
        if input_clean.starts_with("magnet:") {
            return JobKind::Magnet;
        }
        let lower = input_clean.to_ascii_lowercase();
        // Strip query string and fragment before checking extension.
        let path_part = lower.split('?').next().unwrap_or(&lower);
        let path_part = path_part.split('#').next().unwrap_or(path_part);
        if path_part.ends_with(".torrent") {
            return JobKind::Torrent;
        }

        let ext = path_part.rsplit('.').next().unwrap_or("");
        match ext {
            // Executables, packages, installers, archives, disk images, documents -> Direct (aria2)
            "exe" | "msi" | "apk" | "appimage" | "deb" | "rpm" | "dmg" | "pkg" | "bin"
            | "zip" | "rar" | "7z" | "tar" | "gz" | "bz2" | "xz" | "zst" | "tgz" | "tbz2" | "txz"
            | "iso" | "img" | "cab" | "vhd" | "vdi" | "vmdk" | "jar" | "war" | "wim" | "7zip"
            | "pdf" | "doc" | "docx" | "rtf" | "txt" | "md" | "markdown" | "epub" | "mobi" | "csv" | "tsv"
            | "xls" | "xlsx" | "xlsm" | "ppt" | "pptx" | "pps" | "ppsx" | "odt" | "ods" | "odp"
            | "html" | "htm" | "xml" | "json" | "yaml" | "yml" | "tex" | "sql" | "log"
            | "dat" | "bak" | "dump" | "db" | "sqlite" | "cfg" | "conf" => JobKind::Direct,
            // Audio files
            "mp3" | "flac" | "wav" | "ogg" | "opus" | "m4a" | "aac" | "wma" | "alac" | "mid" | "midi"
            | "aiff" | "aif" | "mka" | "ape" | "ac3" | "dts" | "amr" => JobKind::Audio,
            // Video files
            "mp4" | "mkv" | "webm" | "avi" | "mov" | "m4v" | "ts" | "wmv" | "flv" | "mpg" | "mpeg"
            | "3gp" | "3g2" | "ogv" | "vob" | "m2ts" | "mts" | "f4v" | "asf" | "rm" | "rmvb" | "divx" => JobKind::Video,
            _ => {
                // If extension not found at the end of path, check if any direct extension appears in query params
                if lower.contains(".exe") || lower.contains(".deb") || lower.contains(".rpm") || lower.contains(".apk")
                    || lower.contains(".zip") || lower.contains(".iso") || lower.contains(".msi") || lower.contains(".appimage")
                    || lower.contains(".7z") || lower.contains(".tar.gz") || lower.contains(".pdf") || lower.contains(".rar") {
                    JobKind::Direct
                } else {
                    JobKind::Video
                }
            }
        }
    }
}

/// Helper to extract clean file extension from a URL or title string.
pub fn extract_extension(text: &str) -> Option<String> {
    let lower = text.to_ascii_lowercase();
    let path = lower.split('?').next().unwrap_or(&lower);
    let path = path.split('#').next().unwrap_or(path);
    if let Some(idx) = path.rfind('.') {
        let ext = &path[idx + 1..];
        if !ext.is_empty() && ext.len() <= 10 && ext.chars().all(|c| c.is_alphanumeric()) {
            return Some(ext.to_string());
        }
    }
    // Check if filename parameter exists in query string
    for part in lower.split(['?', '&', ';']) {
        if let Some((_, val)) = part.split_once('=') {
            if let Some(idx) = val.rfind('.') {
                let ext = &val[idx + 1..];
                if !ext.is_empty() && ext.len() <= 10 && ext.chars().all(|c| c.is_alphanumeric()) {
                    return Some(ext.to_string());
                }
            }
        }
    }
    None
}

/// Returns one of the organized folder categories matching IDM semantics:
/// "Compress", "Document", "Videos", "Audios", "Torrents", or "Miscellaneous".
pub fn category_for_with_title(url: &str, title: Option<&str>, kind: JobKind) -> &'static str {
    if kind == JobKind::Torrent || kind == JobKind::Magnet || url.starts_with("magnet:") {
        return "Torrents";
    }

    let ext = title.and_then(extract_extension).or_else(|| extract_extension(url));

    if let Some(ref ext) = ext {
        match ext.as_str() {
            "torrent" => "Torrents",
            // Compress: Compressed archives, disk images, packages, installers
            "zip" | "rar" | "7z" | "tar" | "gz" | "bz2" | "xz" | "zst" | "tgz" | "tbz2" | "txz"
            | "iso" | "img" | "dmg" | "cab" | "vhd" | "vdi" | "vmdk" | "apk" | "deb" | "rpm"
            | "exe" | "msi" | "appimage" | "bin" | "jar" | "war" | "ear" | "arj" | "lzh" | "ace"
            | "uue" | "bz" | "lz" | "lzma" | "cpio" | "ar" | "wim" | "7zip" | "pkg" | "run" => "Compress",
            // Document: doc, docx, pdf, txt, md, markdown, excel, presentation, subtitles, code/markup
            "pdf" | "doc" | "docx" | "rtf" | "txt" | "md" | "markdown" | "csv" | "tsv" | "xls"
            | "xlsx" | "xlsm" | "xlsb" | "ppt" | "pptx" | "pps" | "ppsx" | "odt" | "ods" | "odp"
            | "epub" | "mobi" | "azw" | "azw3" | "html" | "htm" | "xhtml" | "xml" | "json"
            | "yaml" | "yml" | "tex" | "log" | "sql" | "srt" | "vtt" | "ass" => "Document",
            // Audios: audio formats
            "mp3" | "flac" | "wav" | "ogg" | "oga" | "opus" | "m4a" | "aac" | "wma" | "alac"
            | "mid" | "midi" | "aiff" | "aif" | "mka" | "ape" | "ac3" | "dts" | "amr" => "Audios",
            // Videos: video formats
            "mp4" | "mkv" | "webm" | "avi" | "mov" | "m4v" | "ts" | "wmv" | "flv" | "mpg"
            | "mpeg" | "m2v" | "3gp" | "3g2" | "ogv" | "vob" | "m2ts" | "mts" | "f4v" | "asf"
            | "rm" | "rmvb" | "divx" => "Videos",
            // Any other extension -> Miscellaneous
            _ => "Miscellaneous",
        }
    } else {
        match kind {
            JobKind::Audio => "Audios",
            JobKind::Video => "Videos",
            JobKind::Torrent | JobKind::Magnet => "Torrents",
            JobKind::Direct => "Miscellaneous",
        }
    }
}

pub fn category_for(url: &str, kind: JobKind) -> &'static str {
    category_for_with_title(url, None, kind)
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
    /// Torrent file selection (1-based indices), when the user picked files.
    #[serde(default)]
    pub select_files: Option<Vec<i64>>,
    pub progress: f32,
    #[serde(default)]
    pub downloaded_bytes: Option<i64>,
    #[serde(default)]
    pub total_bytes: Option<i64>,
    #[serde(default)]
    pub speed_bps: Option<i64>,
    #[serde(default)]
    pub upload_speed_bps: Option<i64>,
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
            select_files: None,
            progress: 0.0,
            downloaded_bytes: None,
            total_bytes: None,
            speed_bps: None,
            upload_speed_bps: None,
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
    pub upload_speed_bps: Option<i64>,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_category_for() {
        assert_eq!(category_for("magnet:?xt=urn:btih:123", JobKind::Magnet), "Torrents");
        assert_eq!(category_for("https://example.com/arch.zip", JobKind::Direct), "Compress");
        assert_eq!(category_for("https://example.com/movie.mp4", JobKind::Direct), "Videos");
        assert_eq!(category_for("https://example.com/song.mp3", JobKind::Direct), "Audios");
        assert_eq!(category_for("https://example.com/doc.pdf", JobKind::Direct), "Document");
        assert_eq!(category_for("https://example.com/notes.md", JobKind::Direct), "Document");
        assert_eq!(category_for("https://example.com/sheet.xlsx", JobKind::Direct), "Document");
        assert_eq!(category_for("https://example.com/index.html", JobKind::Direct), "Document");
        assert_eq!(category_for("https://youtube.com/watch?v=123", JobKind::Video), "Videos");
        assert_eq!(category_for("https://example.com/unknown.xyz", JobKind::Direct), "Miscellaneous");
        assert_eq!(category_for("https://example.com/rawfile", JobKind::Direct), "Miscellaneous");
        assert_eq!(category_for_with_title("https://example.com/get?id=42", Some("readme.md"), JobKind::Direct), "Document");
    }

    #[test]
    fn test_job_kind_detect() {
        assert_eq!(JobKind::detect("magnet:?xt=urn:btih:123"), JobKind::Magnet);
        assert_eq!(JobKind::detect("https://example.com/file.torrent"), JobKind::Torrent);
        assert_eq!(JobKind::detect("https://example.com/setup.exe"), JobKind::Direct);
        assert_eq!(JobKind::detect("https://example.com/package.deb"), JobKind::Direct);
        assert_eq!(JobKind::detect("https://example.com/archive.zip"), JobKind::Direct);
        assert_eq!(JobKind::detect("https://example.com/doc.pdf"), JobKind::Direct);
        assert_eq!(JobKind::detect("https://example.com/song.mp3"), JobKind::Audio);
        assert_eq!(JobKind::detect("https://youtube.com/watch?v=123"), JobKind::Video);
        assert_eq!(JobKind::detect("https://x.com/user/status/123"), JobKind::Video);
    }
}
