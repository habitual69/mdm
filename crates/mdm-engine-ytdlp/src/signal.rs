//! Kill a spawned process tree (yt-dlp + its aria2c external downloader).

#[cfg(unix)]
pub fn kill_tree(pid: u32) {
    use nix::sys::signal::{killpg, Signal};
    use nix::unistd::Pid;
    if pid == 0 {
        return;
    }
    // Children are spawned with process_group(0), so pgid == child pid.
    let pgid = Pid::from_raw(pid as i32);
    // Graceful first so yt-dlp can shut down its external downloader.
    let _ = killpg(pgid, Signal::SIGINT);
    std::thread::sleep(std::time::Duration::from_millis(250));
    let _ = killpg(pgid, Signal::SIGKILL);
}

#[cfg(not(unix))]
pub fn kill_tree(pid: u32) {
    use std::os::windows::process::CommandExt;
    if pid == 0 {
        return;
    }
    let _ = std::process::Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
        .output();
}
