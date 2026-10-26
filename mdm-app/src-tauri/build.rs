use std::path::PathBuf;

fn main() {
    tauri_build::build();

    // On Windows, copy WebView2Loader.dll and bundled engine binaries to target directory
    #[cfg(windows)]
    {
        copy_windows_binaries();
    }
}

#[cfg(windows)]
fn copy_windows_binaries() {
    let out_dir = match std::env::var("OUT_DIR") {
        Ok(dir) => PathBuf::from(dir),
        Err(_) => return,
    };

    // Find target/{profile} directory: OUT_DIR is target/{target-triple}/{profile}/build/... or target/{profile}/build/...
    let mut target_dir = out_dir.clone();
    while let Some(parent) = target_dir.parent() {
        if parent.ends_with("target") {
            break;
        }
        target_dir = parent.to_path_buf();
    }

    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".into()));
    let dll_source = manifest_dir.join("WebView2Loader.dll");
    let bin_source = manifest_dir.join("bin");

    // Copy WebView2Loader.dll next to output binaries
    if dll_source.is_file() {
        let _ = std::fs::copy(&dll_source, target_dir.join("WebView2Loader.dll"));
        let _ = std::fs::copy(&dll_source, out_dir.join("WebView2Loader.dll"));
    }

    // Copy bundled binaries to target_dir/bin
    if bin_source.is_dir() {
        let target_bin = target_dir.join("bin");
        let _ = std::fs::create_dir_all(&target_bin);
        if let Ok(entries) = std::fs::read_dir(&bin_source) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() {
                    if let Some(file_name) = path.file_name() {
                        let _ = std::fs::copy(&path, target_bin.join(file_name));
                    }
                }
            }
        }
    }
}
