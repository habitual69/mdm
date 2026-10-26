//! mdm desktop shell. Boots the same daemon the CLI uses (mdm-server on
//! 127.0.0.1:5757) and shows the embedded web UI in a native window with
//! system tray support, auto-downloaded dependencies, and floating speed widget.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use tauri::image::Image;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Manager, WebviewUrl};

const ADDR: &str = "127.0.0.1:5757";

fn format_speed(bps: u64) -> String {
    if bps == 0 {
        return "0 B/s".into();
    }
    const UNITS: [&str; 4] = ["B/s", "KB/s", "MB/s", "GB/s"];
    let mut v = bps as f64;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if v >= 100.0 || u == 0 {
        format!("{:.0} {}", v, UNITS[u])
    } else {
        format!("{:.1} {}", v, UNITS[u])
    }
}

#[tauri::command]
fn window_minimize(window: tauri::WebviewWindow) {
    let _ = window.minimize();
}

#[tauri::command]
fn window_toggle_maximize(window: tauri::WebviewWindow) {
    if window.is_maximized().unwrap_or(false) {
        let _ = window.unmaximize();
    } else {
        let _ = window.maximize();
    }
}

#[tauri::command]
fn window_close(window: tauri::WebviewWindow) {
    let _ = window.hide();
}

#[tauri::command]
fn window_toggle_floating(app: tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("floating") {
        if w.is_visible().unwrap_or(false) {
            let _ = w.hide();
        } else {
            let _ = w.show();
            let _ = w.unminimize();
            let _ = w.set_focus();
        }
    }
}

fn ensure_port_free(addr: &str) {
    if std::net::TcpStream::connect(addr).is_ok() {
        eprintln!("mdm: port {addr} in use by previous daemon, reclaiming port...");
        #[cfg(unix)]
        {
            let _ = std::process::Command::new("fuser")
                .args(["-k", "-n", "tcp", "5757"])
                .output();
            std::thread::sleep(std::time::Duration::from_millis(300));
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            let script = "$conns = Get-NetTCPConnection -LocalPort 5757 -ErrorAction SilentlyContinue; foreach ($c in $conns) { Stop-Process -Id $c.OwningProcess -Force -ErrorAction SilentlyContinue }";
            let mut cmd = std::process::Command::new("powershell");
            cmd.args(["-NoProfile", "-WindowStyle", "Hidden", "-Command", script])
                .creation_flags(0x0800_0000);
            let _ = cmd.output();
            std::thread::sleep(std::time::Duration::from_millis(300));
        }
    }
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "mdm=info".into()))
        .init();

    ensure_port_free(ADDR);

    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            window_minimize,
            window_toggle_maximize,
            window_close,
            window_toggle_floating
        ])
        .setup(|app| {
            let icon_bytes = include_bytes!("../icons/icon.png");
            let icon = Image::from_bytes(icon_bytes)?;

            // Build system tray menu with live speed status
            let speed_item = MenuItem::with_id(app, "speed", "mDm: idle", false, None::<&str>)?;
            let show_item = MenuItem::with_id(app, "show", "Open mDm", true, None::<&str>)?;
            let float_item = MenuItem::with_id(app, "floating", "Toggle Floating Speed Widget", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&speed_item, &show_item, &float_item, &quit_item])?;

            // Setup system tray (Ubuntu top bar, Windows task tray, macOS menu bar)
            let tray = TrayIconBuilder::new()
                .icon(icon.clone())
                .tooltip("mDm — Media Download Manager")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| {
                    match event.id().as_ref() {
                        "show" => {
                            if let Some(window) = app.get_webview_window("main") {
                                let _ = window.show();
                                let _ = window.unminimize();
                                let _ = window.set_focus();
                            }
                        }
                        "floating" => {
                            if let Some(w) = app.get_webview_window("floating") {
                                if w.is_visible().unwrap_or(false) {
                                    let _ = w.hide();
                                } else {
                                    let _ = w.show();
                                    let _ = w.unminimize();
                                    let _ = w.set_focus();
                                }
                            }
                        }
                        "quit" => {
                            app.exit(0);
                        }
                        _ => {}
                    }
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                        let app = tray.app_handle();
                        if let Some(window) = app.get_webview_window("main") {
                            if window.is_visible().unwrap_or(false) {
                                let _ = window.hide();
                            } else {
                                let _ = window.show();
                                let _ = window.unminimize();
                                let _ = window.set_focus();
                            }
                        }
                    }
                })
                .build(app)?;

            let (window_tx, mut window_rx) = tokio::sync::broadcast::channel::<mdm_server::WindowAction>(32);

            // Channel listener for window actions originating from web UI (/api/window/*)
            let handle_actions = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                while let Ok(action) = window_rx.recv().await {
                    match action {
                        mdm_server::WindowAction::Minimize => {
                            if let Some(w) = handle_actions.get_webview_window("main") {
                                let _ = w.minimize();
                            }
                        }
                        mdm_server::WindowAction::Show => {
                            if let Some(w) = handle_actions.get_webview_window("main") {
                                let _ = w.show();
                                let _ = w.unminimize();
                                let _ = w.set_focus();
                            }
                        }
                        mdm_server::WindowAction::ToggleMaximize => {
                            if let Some(w) = handle_actions.get_webview_window("main") {
                                if !w.is_visible().unwrap_or(false) {
                                    let _ = w.show();
                                    let _ = w.unminimize();
                                    let _ = w.set_focus();
                                } else if w.is_maximized().unwrap_or(false) {
                                    let _ = w.unmaximize();
                                } else {
                                    let _ = w.maximize();
                                }
                            }
                        }
                        mdm_server::WindowAction::Close => {
                            if let Some(w) = handle_actions.get_webview_window("main") {
                                let _ = w.hide();
                            }
                        }
                        mdm_server::WindowAction::ToggleFloating => {
                            if let Some(w) = handle_actions.get_webview_window("floating") {
                                if w.is_visible().unwrap_or(false) {
                                    let _ = w.hide();
                                } else {
                                    let _ = w.show();
                                    let _ = w.unminimize();
                                    let _ = w.set_focus();
                                }
                            }
                        }
                        mdm_server::WindowAction::ResizeFloating { width, height } => {
                            if let Some(w) = handle_actions.get_webview_window("floating") {
                                let _ = w.set_size(tauri::Size::Logical(tauri::LogicalSize { width, height }));
                            }
                        }
                    }
                }
            });

            // Boot the queue + HTTP/WS server and automatically download all dependencies
            let window_tx_clone = window_tx.clone();
            let tray_handle = tray.clone();
            let speed_item_handle = speed_item.clone();
            tauri::async_runtime::spawn(async move {
                let cfg = match mdm_core::load_config() {
                    Ok(cfg) => cfg,
                    Err(e) => {
                        eprintln!("mdm: failed to load config: {e}");
                        return;
                    }
                };
                let installer = mdm_bins::Installer::new(&cfg);
                for st in installer.ensure_all(&cfg).await {
                    match &st.action {
                        mdm_bins::BinAction::Installed { path, version, .. } => {
                            eprintln!("mdm: installed dependency {} {version} -> {path}", st.name);
                        }
                        mdm_bins::BinAction::Failed { error } => {
                            eprintln!("mdm: setup {}: {error}", st.name);
                        }
                        _ => {}
                    }
                }
                match mdm_server::build_queue(cfg) {
                    Ok(q) => {
                        // Background live speed updater for Ubuntu / macOS top bar & tray
                        let q_tray = q.clone();
                        tauri::async_runtime::spawn(async move {
                            loop {
                                tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
                                if let Ok(running) = q_tray.list(Some(&[mdm_core::JobState::Running])) {
                                    let running_count = running.len();
                                    let mut down_bps = 0u64;
                                    let mut up_bps = 0u64;
                                    let mut total_prog = 0.0f64;
                                    for j in &running {
                                        down_bps += j.speed_bps.unwrap_or(0).max(0) as u64;
                                        up_bps += j.upload_speed_bps.unwrap_or(0).max(0) as u64;
                                        total_prog += j.progress as f64;
                                    }
                                    let avg_prog = if running_count > 0 { total_prog / running_count as f64 } else { 0.0 };
                                    let down_str = format_speed(down_bps);
                                    let up_str = format_speed(up_bps);

                                    let title = if running_count > 0 {
                                        format!("↓ {down_str} ↑ {up_str}")
                                    } else {
                                        "".to_string()
                                    };
                                    let tooltip = if running_count > 0 {
                                        format!("mDm: ↓ {down_str} · ↑ {up_str} ({running_count} active · {:.0}%)", avg_prog)
                                    } else {
                                        "mDm — Media Download Manager (Idle)".to_string()
                                    };

                                    let _ = tray_handle.set_title(if title.is_empty() { None } else { Some(&title) });
                                    let _ = tray_handle.set_tooltip(Some(&tooltip));

                                    let menu_label = if running_count > 0 {
                                        format!("↓ {down_str}  ↑ {up_str} ({running_count} downloading)")
                                    } else {
                                        "mDm: idle".to_string()
                                    };
                                    let _ = speed_item_handle.set_text(&menu_label);
                                }
                            }
                        });

                        if let Err(e) = mdm_server::serve_with_window(q, ADDR, Some(window_tx_clone)).await {
                            eprintln!("mdm: server exited: {e}");
                        }
                    }
                    Err(e) => eprintln!("mdm: failed to build queue: {e}"),
                }
            });

            // Wait until the server accepts connections
            let ready = || async {
                tokio::net::TcpStream::connect(ADDR).await.is_ok()
            };
            let mut attempts = 0u32;
            while !tauri::async_runtime::block_on(ready()) {
                attempts += 1;
                if attempts > 1200 {
                    eprintln!("mdm: server did not come up within timeout; opening window anyway");
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }

            let handle = app.handle().clone();

            // Main App Window
            tauri::WebviewWindowBuilder::new(
                &handle,
                "main",
                WebviewUrl::External(format!("http://{ADDR}/").parse().expect("valid url")),
            )
            .title("mDm — Media Download Manager")
            .icon(icon.clone())?
            .decorations(false)
            .inner_size(820.0, 560.0)
            .min_inner_size(420.0, 480.0)
            .resizable(true)
            .center()
            .build()?;

            // Floating Speed Widget Window (always on top, frameless, compact speed pill & graph)
            let _floating = tauri::WebviewWindowBuilder::new(
                &handle,
                "floating",
                WebviewUrl::External(format!("http://{ADDR}/static/widget.html").parse().expect("valid url")),
            )
            .title("mDm Speed Widget")
            .icon(icon)?
            .decorations(false)
            .transparent(true)
            .always_on_top(true)
            .skip_taskbar(true)
            .resizable(true)
            .inner_size(244.0, 34.0)
            .visible(false)
            .build()?;

            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running mdm");
}
