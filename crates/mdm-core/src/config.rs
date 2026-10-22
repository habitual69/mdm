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
            aria2_connections: 8,
            aria2_splits: 8,
            aria2_connect_timeout: 30,
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
