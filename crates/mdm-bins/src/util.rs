//! Low-level helpers: download, hash, run-check, chmod, extract.

use std::path::{Path, PathBuf};

use mdm_core::Error;
use sha2::{Digest, Sha256};

pub async fn fetch_bytes(http: &reqwest::Client, url: &str) -> Result<Vec<u8>, Error> {
    let resp = http
        .get(url)
        .send()
        .await
        .map_err(|e| Error::Engine(format!("GET {url}: {e}")))?;
    if !resp.status().is_success() {
        return Err(Error::Engine(format!("GET {url}: HTTP {}", resp.status())));
    }
    resp.bytes()
        .await
        .map(|b| b.to_vec())
        .map_err(|e| Error::Engine(format!("GET {url} body: {e}")))
}

pub async fn fetch_text(http: &reqwest::Client, url: &str) -> Result<String, Error> {
    let resp = http
        .get(url)
        .send()
        .await
        .map_err(|e| Error::Engine(format!("GET {url}: {e}")))?;
    if !resp.status().is_success() {
        return Err(Error::Engine(format!("GET {url}: HTTP {}", resp.status())));
    }
    resp.text()
        .await
        .map_err(|e| Error::Engine(format!("GET {url} body: {e}")))
}

pub fn sha256_bytes(b: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(b);
    let d = h.finalize();
    d.iter().map(|x| format!("{x:02x}")).collect()
}

#[allow(dead_code)]
pub fn sha256_file(p: &Path) -> Result<String, Error> {
    let data = std::fs::read(p)?;
    Ok(sha256_bytes(&data))
}

/// Pull the hash for `name` from a `sha256sum`-style list ("<hash>  <file>").
pub fn sha_line(sums: &str, name: &str) -> Option<String> {
    for line in sums.lines() {
        let mut it = line.split_whitespace();
        let hash = it.next()?;
        let file = it.next()?.trim_matches('*');
        if file == name && hash.len() == 64 && hash.chars().all(|c| c.is_ascii_hexdigit()) {
            return Some(hash.to_string());
        }
    }
    None
}

pub async fn run_version(bin: &Path) -> String {
    let out = tokio::process::Command::new(bin).arg("--version").output().await;
    match out {
        Ok(out) => {
            let stdout_line = String::from_utf8_lossy(&out.stdout)
                .lines()
                .next()
                .unwrap_or("")
                .trim()
                .to_string();
            if !stdout_line.is_empty() {
                return stdout_line;
            }
            String::from_utf8_lossy(&out.stderr)
                .lines()
                .next()
                .unwrap_or("")
                .trim()
                .to_string()
        }
        Err(_) => String::new(),
    }
}

#[cfg(unix)]
pub fn chmod_x(p: &Path) -> Result<(), Error> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(p)?.permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(p, perms)?;
    Ok(())
}

#[cfg(not(unix))]
pub fn chmod_x(_p: &Path) -> Result<(), Error> {
    Ok(())
}

/// Extract a .tar / .tar.gz / .tar.bz2 / .tar.xz archive.
pub fn extract_tar(archive: &Path, dest: &Path) -> Result<(), Error> {
    let file = std::fs::File::open(archive)?;
    let ext = archive
        .to_str()
        .map(|p| p.to_ascii_lowercase())
        .unwrap_or_default();
    let reader: Box<dyn std::io::Read> = if ext.ends_with(".tar.xz") || ext.ends_with(".txz") {
        Box::new(xz2::read::XzDecoder::new(std::io::BufReader::new(file)))
    } else if ext.ends_with(".tar.bz2") || ext.ends_with(".tbz2") {
        Box::new(bzip2_rs::DecoderReader::new(std::io::BufReader::new(file)))
    } else if ext.ends_with(".tar.gz") || ext.ends_with(".tgz") {
        Box::new(flate2::read::GzDecoder::new(std::io::BufReader::new(file)))
    } else {
        Box::new(std::io::BufReader::new(file))
    };

    let mut tar = tar::Archive::new(reader);
    tar.unpack(dest)
        .map_err(|e| Error::Engine(format!("tar extract: {e}")))?;
    Ok(())
}

pub fn extract_zip(archive: &Path, dest: &Path) -> Result<(), Error> {
    let file = std::fs::File::open(archive)?;
    let mut zip = zip::ZipArchive::new(std::io::BufReader::new(file))
        .map_err(|e| Error::Engine(format!("zip open: {e}")))?;
    zip.extract(dest)
        .map_err(|e| Error::Engine(format!("zip extract: {e}")))?;
    Ok(())
}

/// Walk a freshly extracted dir looking for a binary by file name.
pub fn find_extracted(root: &Path, wanted: &str) -> Option<PathBuf> {
    fn walk(dir: &Path, wanted: &str) -> Option<PathBuf> {
        for entry in std::fs::read_dir(dir).ok()? {
            let entry = entry.ok()?;
            let path = entry.path();
            if path.is_dir() {
                if let Some(p) = walk(&path, wanted) {
                    return Some(p);
                }
            } else if path.file_name().and_then(|n| n.to_str()) == Some(wanted) {
                return Some(path);
            }
        }
        None
    }
    walk(root, wanted)
}
