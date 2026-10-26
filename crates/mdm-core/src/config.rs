use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// Application data dir: ~/.local/share/mdm on Linux, %APPDATA%\mdm on
/// Windows, ~/Library/Application Support/mdm on macOS.
pub fn data_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join(crate::APP_NAME)
}

pub fn config_path() -> PathBuf {
    data_dir().join("config.toml")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Max concurrently running downloads.
    pub max_concurrent: u32,
    /// Default directory for finished files.
    pub output_dir: PathBuf,
    /// Extra directory to search for engine binaries (Phase 2: auto-download lands here).
    pub bin_dir: Option<PathBuf>,
    /// Minutes to keep seeding finished torrents (0 = stop immediately).
    pub seed_minutes: u32,
    /// Local REST/WS listener (127.0.0.1 only in Phase 1).
    pub api_addr: String,
    /// Automatic retries per job before it fails permanently.
    pub max_retries: u32,
    /// Overall download speed cap shown as "5M"/"500K"/"1G" (None = unlimited).
    pub speed_limit: Option<String>,
    /// Proxy URL applied to every download engine (http:// or socks5://).
    pub proxy: Option<String>,
    /// yt-dlp: write subtitles next to / into the output file.
    pub write_subtitles: bool,
    /// yt-dlp: also write machine-generated captions.
    pub write_auto_subs: bool,
    /// yt-dlp: embed media metadata (tags, chapters) into the file.
    pub embed_metadata: bool,
    /// yt-dlp: embed the thumbnail as cover art.
    pub embed_thumbnail: bool,
    /// yt-dlp: hand media/direct downloads to the bundled aria2c
    /// (segmented, multi-connection) instead of yt-dlp's native downloader.
    pub aria2_offload: bool,
    /// aria2: max connections per server for HTTP downloads (1..=16).
    pub aria2_connections: u32,
    /// aria2: segments per download (HTTP), 1..=16.
    pub aria2_splits: u32,
    /// aria2: seconds before a stalled connection gives up (5..=300).
    pub aria2_connect_timeout: u32,
    /// aria2 offload failed -> retry the yt-dlp job with yt-dlp's own
    /// downloader so downloads still complete on networks that block
    /// aria2's transfer path.
    pub aria2_fallback: bool,
    /// aria2: RAM disk cache in MB before flushes to disk (P3TERX style).
    /// 0 = disabled.
    pub aria2_disk_cache: u32,
    /// aria2: file preallocation mode (none | prealloc | trunc | falloc).
    /// falloc suits spinning HDDs; leave `none` on SSDs.
    pub aria2_file_allocation: String,
    /// aria2: smallest piece a download is divided into, in MB (1..=1024).
    pub aria2_min_split_size: u32,
    /// aria2: attempts per URI before giving up. 0 = unlimited.
    pub aria2_max_tries: u32,
    /// aria2: pause between retry attempts, seconds (0..=60).
    pub aria2_retry_wait: u32,
    /// aria2: drop connections slower than this KiB/s (0 = off). Retry
    /// builds reconnect instead of stalling.
    pub aria2_lowest_speed_limit: u32,
    /// aria2: max peers per torrent (0 = unlimited).
    pub aria2_bt_max_peers: u32,
    /// aria2: HTTP user agent for direct downloads. Empty = aria2 default.
    /// Browser-like agents dodge "aria2/*" server bans.
    pub aria2_user_agent: String,
    /// aria2: custom BitTorrent tracker announce URLs (comma-separated).
    pub aria2_custom_tracker_url: String,
    /// yt-dlp: cap video height in px for "best" requests. 0 = no cap.
    pub ytdlp_max_height: u32,
    /// yt-dlp: container used when merging separate streams (mkv|mp4|webm).
    pub ytdlp_merge_format: String,
    /// yt-dlp: audio conversion target for audio jobs
    /// (mp3|m4a|opus|flac|wav|vorbis|best). "best" keeps the source codec.
    pub ytdlp_audio_format: String,
    /// yt-dlp: parallel fragment downloads for its native downloader,
    /// 1..=16 (the aria2 fallback path uses this).
    pub ytdlp_fragments: u32,
    /// yt-dlp: pack downloaded subtitles into the output file (needs ffmpeg).
    pub ytdlp_embed_subs: bool,
    /// UI theme mode: "dark" | "light" | "system". Default: "dark".
    pub theme: String,
    /// UI accent color hex. Default: "#e8934a" (Amber Orange).
    pub accent_color: String,
    /// Floating desktop widget style: "pill" | "card" | "minimalistic". Default: "pill".
    pub widget_shape: String,
    /// yt-dlp: extract cookies from an installed browser (chrome|brave|firefox|edge|chromium|opera|vivaldi|safari|"").
    pub ytdlp_cookies_from_browser: String,
    /// Desktop notifications enabled (system toast on download complete/failed).
    pub notifications_enabled: bool,
    /// Notification sound chime enabled.
    pub notification_sound: bool,
}

impl Default for Config {
    fn default() -> Self {
        let output_dir = dirs::download_dir()
            .unwrap_or_else(data_dir)
            .join(crate::APP_NAME);
        Config {
            max_concurrent: 3,
            output_dir,
            bin_dir: None,
            seed_minutes: 5,
            api_addr: "127.0.0.1:5757".into(),
            max_retries: 2,
            speed_limit: None,
            proxy: None,
            write_subtitles: false,
            write_auto_subs: false,
            embed_metadata: true,
            embed_thumbnail: true,
            aria2_offload: true,
            aria2_connections: 4,
            aria2_splits: 4,
            aria2_connect_timeout: 30,
            aria2_fallback: true,
            aria2_disk_cache: 64,
            aria2_file_allocation: "none".into(),
            aria2_min_split_size: 4,
            aria2_max_tries: 3,
            aria2_retry_wait: 5,
            aria2_lowest_speed_limit: 0,
            aria2_bt_max_peers: 128,
            aria2_user_agent: String::new(),
            aria2_custom_tracker_url: String::new(),
            ytdlp_max_height: 0,
            ytdlp_merge_format: "mkv".into(),
            ytdlp_audio_format: "mp3".into(),
            ytdlp_fragments: 4,
            ytdlp_embed_subs: false,
            theme: "dark".into(),
            accent_color: "#e8934a".into(),
            widget_shape: "pill".into(),
            ytdlp_cookies_from_browser: String::new(),
            notifications_enabled: true,
            notification_sound: true,
        }
    }
}

pub fn load_config() -> Result<Config> {
    let path = config_path();
    if path.exists() {
        let raw = fs::read_to_string(&path)?;
        let cfg: Config = toml::from_str(&raw)
            .map_err(|e| Error::Config(format!("{}: {}", path.display(), e)))?;
        Ok(cfg)
    } else {
        let cfg = Config::default();
        save_config(&cfg)?;
        Ok(cfg)
    }
}

pub fn save_config(cfg: &Config) -> Result<()> {
    let path = config_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let raw = toml::to_string_pretty(cfg)
        .map_err(|e| Error::Config(format!("serialize: {}", e)))?;
    fs::write(&path, raw)?;
    Ok(())
}

/// Retrieve an existing directory, creating it if missing.
#[allow(dead_code)] // used by mdm-cli (daemon bootstrap)
pub fn ensure_dir(p: &Path) -> Result<PathBuf> {
    fs::create_dir_all(p)?;
    Ok(p.to_path_buf())
}
