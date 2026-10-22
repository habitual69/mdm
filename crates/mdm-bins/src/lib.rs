//! Download + verify engine binaries into mdm's bin dir (no root needed).

mod util;

use std::path::{Path, PathBuf};

use mdm_core::{bin_path, Config, Error, Result};
use serde::Serialize;
use tracing::{info, warn};

pub struct Installer {
    pub bin_dir: PathBuf,
    http: reqwest::Client,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "action")]
pub enum BinAction {
    AlreadyOk { version: String, path: String },
    Installed { version: String, path: String, sha_ok: Option<bool> },
    Skipped { reason: String },
    Failed { error: String },
}

#[derive(Debug, Clone, Serialize)]
pub struct BinStatus {
    pub name: &'static str,
    #[serde(flatten)]
    pub action: BinAction,
}

const ENGINES: [&str; 3] = ["yt-dlp", "aria2c", "ffmpeg"];

impl Installer {
    pub fn effective_bin_dir(cfg: &Config) -> PathBuf {
        cfg.bin_dir
            .clone()
            .unwrap_or_else(|| mdm_core::data_dir().join("bin"))
    }

    pub fn new(cfg: &Config) -> Installer {
        let bin_dir = Self::effective_bin_dir(cfg);
        let _ = std::fs::create_dir_all(&bin_dir);
        Installer {
            bin_dir,
            http: reqwest::Client::builder()
                .user_agent("mdm-installer")
                .build()
                .unwrap_or_default(),
        }
    }

    /// Ensure every engine binary exists (PATH or our bin dir); install the missing.
    pub async fn ensure_all(&self, cfg: &Config) -> Vec<BinStatus> {
        for name in ENGINES {
            if bin_path(name, cfg).is_ok() {
                continue;
            }
            let status = self.install(name).await;
            info!("setup {name}: {:?}", status.action);
            if let BinAction::Failed { error } = &status.action {
                warn!("setup {name} failed: {error}");
            }
        }
        self.status_all(cfg).await
    }

    /// Report on each binary without installing.
    pub async fn status_all(&self, cfg: &Config) -> Vec<BinStatus> {
        let mut out = Vec::new();
        for name in ENGINES {
            match bin_path(name, cfg) {
                Ok(p) => {
                    let version = util::run_version(&p).await;
                    out.push(BinStatus {
                        name,
                        action: BinAction::AlreadyOk {
                            version,
                            path: p.display().to_string(),
                        },
                    });
                }
                Err(_) => out.push(BinStatus {
                    name,
                    action: BinAction::Skipped { reason: "still missing".into() },
                }),
            }
        }
        out
    }

    /// Force-install one binary into the bin dir.
    pub async fn install(&self, name: &str) -> BinStatus {
        let res = match name {
            "yt-dlp" => self.install_ytdlp().await,
            "ffmpeg" => self.install_ffmpeg().await,
            "aria2c" => self.install_aria2().await,
            other => Err(Error::Engine(format!("unknown engine '{other}'"))),
        };
        let status_name = ENGINES.iter().find(|e| **e == name).copied().unwrap_or("?");
        match res {
            Ok((path, version, sha_ok)) => BinStatus {
                name: status_name,
                action: BinAction::Installed {
                    path: path.display().to_string(),
                    version,
                    sha_ok,
                },
            },
            Err(e) => BinStatus {
                name: status_name,
                action: BinAction::Failed { error: e.to_string() },
            },
        }
    }

    /// Download, sha-verify (when wanted), write, chmod.
    async fn fetch_bin(
        &self,
        url: &str,
        target: &Path,
        expected_sha: Option<&str>,
    ) -> Result<Option<bool>> {
        let bytes = util::fetch_bytes(&self.http, url).await?;
        let mut sha_ok: Option<bool> = None;
        if let Some(expected) = expected_sha {
            let actual = util::sha256_bytes(&bytes);
            if !actual.eq_ignore_ascii_case(expected) {
                return Err(Error::Engine(format!(
                    "sha256 mismatch for {url}: expected {expected}, got {actual}"
                )));
            }
            sha_ok = Some(true);
        }
        std::fs::write(target, &bytes)?;
        util::chmod_x(target)?;
        Ok(sha_ok)
    }

    // --- yt-dlp ------------------------------------------------------------

    async fn install_ytdlp(&self) -> Result<(PathBuf, String, Option<bool>)> {
        #[cfg(target_os = "windows")]
        let asset_name = "yt-dlp.exe";
        #[cfg(target_os = "macos")]
        let asset_name = "yt-dlp_macos";
        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        let asset_name = "yt-dlp_linux";

        let rel = self
            .http
            .get("https://api.github.com/repos/yt-dlp/yt-dlp/releases/latest")
            .send()
            .await
            .map_err(|e| Error::Engine(format!("github api: {e}")))?;
        let v: serde_json::Value = rel
            .json()
            .await
            .map_err(|e| Error::Engine(format!("release JSON: {e}")))?;
        let tag = v
            .get("tag_name")
            .and_then(|t| t.as_str())
            .ok_or_else(|| Error::Engine("release JSON missing tag_name".into()))?
            .to_string();

        let mut asset_url = None;
        let mut sums_url = None;
        for a in v
            .get("assets")
            .and_then(|a| a.as_array())
            .map(|a| a.as_slice())
            .unwrap_or(&[])
        {
            let n = a.get("name").and_then(|n| n.as_str()).unwrap_or("");
            let u = a
                .get("browser_download_url")
                .and_then(|u| u.as_str())
                .unwrap_or("");
            if n == asset_name {
                asset_url = Some(u.to_string());
            }
            if n == "SHA2-256SUMS" {
                sums_url = Some(u.to_string());
            }
        }
        let asset_url = asset_url
            .ok_or_else(|| Error::Engine(format!("asset {asset_name} not found in {tag}")))?;
        let target = self
            .bin_dir
            .join(if cfg!(windows) { "yt-dlp.exe" } else { "yt-dlp" });

        let want: Option<String> = match sums_url {
            Some(sums) => match util::fetch_text(&self.http, &sums).await {
                Ok(text) => util::sha_line(&text, asset_name),
                Err(_) => None,
            },
            None => None,
        };

        let sha_ok = self
            .fetch_bin(&asset_url, &target, want.as_deref())
            .await?;
        let version = util::run_version(&target).await;
        if version.is_empty() {
            return Err(Error::Engine("yt-dlp run-check failed after install".into()));
        }
        Ok((target, version, sha_ok))
    }

    // --- ffmpeg ------------------------------------------------------------

    async fn install_ffmpeg(&self) -> Result<(PathBuf, String, Option<bool>)> {
        #[cfg(target_os = "windows")]
        {
            return self
                .install_from_zip(
                    "https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip",
                    "bin/ffmpeg.exe",
                    "ffmpeg.exe",
                )
                .await;
        }
        #[cfg(target_os = "macos")]
        {
            return self
                .install_from_zip(
                    "https://evermeet.cx/ffmpeg/getrelease/zip",
                    "ffmpeg",
                    "ffmpeg",
                )
                .await;
        }
        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        {
            let archive_url =
                "https://johnvansickle.com/ffmpeg/releases/ffmpeg-release-amd64-static.tar.xz";
            let sha_url = format!("{archive_url}.sha256");

            let bytes = util::fetch_bytes(&self.http, archive_url).await?;
            let mut sha_ok: Option<bool> = None;
            if let Ok(text) = util::fetch_text(&self.http, &sha_url).await {
                if let Some(exp) = text
                    .lines()
                    .next()
                    .and_then(|l| l.split_whitespace().next())
                    .filter(|s| s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit()))
                {
                    let actual = util::sha256_bytes(&bytes);
                    if !actual.eq_ignore_ascii_case(exp) {
                        return Err(Error::Engine(format!(
                            "ffmpeg sha256 mismatch: expected {exp}, got {actual}"
                        )));
                    }
                    sha_ok = Some(true);
                }
            }

            let tmp = std::env::temp_dir().join("mdm_ffmpeg.tar.xz");
            std::fs::write(&tmp, &bytes)?;
            let workdir = std::env::temp_dir().join("mdm_ffmpeg_extract");
            let _ = std::fs::remove_dir_all(&workdir);
            std::fs::create_dir_all(&workdir)?;
            util::extract_tar(&tmp, &workdir)?;
            let _ = std::fs::remove_file(&tmp);

            for name in ["ffmpeg", "ffprobe"] {
                let found = util::find_extracted(&workdir, name)
                    .ok_or_else(|| Error::Engine(format!("{name} not found in archive")))?;
                let target = self.bin_dir.join(name);
                std::fs::rename(&found, &target).unwrap_or_else(|_| {
                    let _ = std::fs::copy(&found, &target);
                    let _ = std::fs::remove_file(&found);
                });
                util::chmod_x(&target)?;
            }
            let _ = std::fs::remove_dir_all(&workdir);
            let target = self.bin_dir.join("ffmpeg");
            let version = util::run_version(&target).await;
            Ok((target, version, sha_ok))
        }
    }

    // --- aria2 -------------------------------------------------------------

    async fn install_aria2(&self) -> Result<(PathBuf, String, Option<bool>)> {
        // Static official-style builds (no root); checksums unavailable, so
        // executability is verified with `aria2c --version` afterwards.
        //
        // Linux: P3TERX/aria2-builder static builds (tar.gz). Windows/macOS:
        // official aria2 releases.
        let (api, suffix, is_zip): (&str, &str, bool) = if cfg!(target_os = "windows") {
            (
                "https://api.github.com/repos/aria2/aria2/releases/latest",
                "win-64bit-build1.zip",
                true,
            )
        } else if cfg!(target_os = "macos") {
            (
                "https://api.github.com/repos/aria2/aria2/releases/latest",
                "osx",
                false,
            )
        } else {
            let arch = if cfg!(target_arch = "aarch64") {
                "arm64"
            } else {
                "amd64"
            };
            (
                "https://api.github.com/repos/P3TERX/aria2-builder/releases/latest",
                &format!("-static-linux-{arch}.tar.gz"),
                false,
            )
        };
        let rel = self
            .http
            .get(api)
            .send()
            .await
            .map_err(|e| Error::Engine(format!("github api: {e}")))?;
        let v: serde_json::Value = rel
            .json()
            .await
            .map_err(|e| Error::Engine(format!("release JSON: {e}")))?;

        let asset = v
            .get("assets")
            .and_then(|a| a.as_array())
            .map(|a| a.as_slice())
            .unwrap_or(&[])
            .iter()
            .find(|a| {
                let n = a.get("name").and_then(|n| n.as_str()).unwrap_or("");
                n.starts_with("aria2-") && n.ends_with(suffix)
            })
            .and_then(|a| a.get("browser_download_url").and_then(|u| u.as_str()))
            .map(String::from)
            .ok_or_else(|| Error::Engine(format!("aria2 static build ({suffix}) not found")))?;

        let bytes = util::fetch_bytes(&self.http, &asset).await?;
        let tmp = std::env::temp_dir().join(if is_zip {
            "mdm_aria2.zip"
        } else {
            "mdm_aria2.tar.gz"
        });
        std::fs::write(&tmp, &bytes)?;

        let bin_name = if cfg!(windows) { "aria2c.exe" } else { "aria2c" };
        let workdir = std::env::temp_dir().join("mdm_aria2_extract");
        let _ = std::fs::remove_dir_all(&workdir);
        std::fs::create_dir_all(&workdir)?;
        if is_zip {
            util::extract_zip(&tmp, &workdir)?;
        } else {
            util::extract_tar(&tmp, &workdir)?;
        }
        let found = util::find_extracted(&workdir, bin_name)
            .ok_or_else(|| Error::Engine("aria2c binary not found".into()))?;
        let target = self.bin_dir.join(bin_name);
        std::fs::rename(&found, &target).unwrap_or_else(|_| {
            let _ = std::fs::copy(&found, &target);
            let _ = std::fs::remove_file(&found);
        });
        util::chmod_x(&target)?;
        let _ = std::fs::remove_file(&tmp);
        let _ = std::fs::remove_dir_all(&workdir);
        let version = util::run_version(&target).await;
        if version.is_empty() {
            return Err(Error::Engine("aria2c run-check failed after install".into()));
        }
        Ok((target, version, None))
    }

    // --- shared zip path (windows/mac ffmpeg) ------------------------------

    #[cfg(any(target_os = "windows", target_os = "macos"))]
    async fn install_from_zip(
        &self,
        url: &str,
        inner_match: &str,
        target_name: &str,
    ) -> Result<(PathBuf, String, Option<bool>)> {
        let bytes = util::fetch_bytes(&self.http, url).await?;
        let tmp = std::env::temp_dir().join("mdm_archive.zip");
        std::fs::write(&tmp, &bytes)?;
        let stage = std::env::temp_dir().join("mdm_inner_extract");
        let _ = std::fs::remove_dir_all(&stage);
        std::fs::create_dir_all(&stage)?;
        util::extract_zip(&tmp, &stage)?;
        let _ = std::fs::remove_file(&tmp);

        let found = util::find_extracted(&stage, inner_match.split('/').next_back().unwrap_or(""))
            .ok_or_else(|| Error::Engine(format!("{inner_match} not found in archive")))?;
        let target = self.bin_dir.join(target_name);
        std::fs::rename(&found, &target).unwrap_or_else(|_| {
            let _ = std::fs::copy(&found, &target);
            let _ = std::fs::remove_file(&found);
        });
        util::chmod_x(&target)?;
        let _ = std::fs::remove_dir_all(&stage);
        let version = util::run_version(&target).await;
        Ok((target, version, None))
    }
}
