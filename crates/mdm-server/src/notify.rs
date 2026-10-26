use std::path::{Path, PathBuf};

pub fn send_system_notification(title: &str, body: &str, icon_path: Option<&Path>) {
    let title = title.to_string();
    let body = body.to_string();
    let icon = icon_path.map(|p| p.to_string_lossy().into_owned());

    std::thread::spawn(move || {
        #[cfg(target_os = "linux")]
        {
            let mut cmd = std::process::Command::new("notify-send");
            cmd.args(["--app-name=mDm", &title, &body]);
            if let Some(ref ic) = icon {
                cmd.args(["--icon", ic]);
            } else {
                let default_icon = mdm_core::data_dir()
                    .join("icons/hicolor/128x128/apps/mdm-app.png");
                if default_icon.is_file() {
                    cmd.args(["--icon", &default_icon.to_string_lossy()]);
                }
            }
            let _ = cmd.output();
        }

        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;

            // Resolve an icon path for the Windows Toast logo override
            let icon_resolved: Option<PathBuf> = if let Some(ref ic) = icon {
                let p = Path::new(ic);
                if p.is_file() {
                    Some(p.to_path_buf())
                } else {
                    None
                }
            } else {
                let mut candidate = None;
                if let Ok(exe) = std::env::current_exe() {
                    if let Some(parent) = exe.parent() {
                        let c1 = parent.join("app-icon.ico");
                        let c2 = parent.join("resources").join("app-icon.ico");
                        let c3 = parent.join("icon.png");
                        if c1.is_file() {
                            candidate = Some(c1);
                        } else if c2.is_file() {
                            candidate = Some(c2);
                        } else if c3.is_file() {
                            candidate = Some(c3);
                        }
                    }
                }
                if candidate.is_none() {
                    let user_icon = mdm_core::data_dir().join("app-icon.ico");
                    if user_icon.is_file() {
                        candidate = Some(user_icon);
                    }
                }
                candidate
            };

            let img_tag = if let Some(ref p) = icon_resolved {
                let norm = p.display().to_string().replace('\\', "/");
                format!("<image placement='appLogoOverride' hint-crop='circle' src='file:///{norm}'/>")
            } else {
                String::new()
            };

            let esc_title = title
                .replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;")
                .replace('\'', "''")
                .replace('"', "&quot;");
            let esc_body = body
                .replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;")
                .replace('\'', "''")
                .replace('"', "&quot;");

            let script = format!(
                "[Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType = WindowsRuntime] | Out-Null; \
                 [Windows.Data.Xml.Dom.XmlDocument, Windows.Data.Xml.Dom.XmlDocument, ContentType = WindowsRuntime] | Out-Null; \
                 $xml = [Windows.Data.Xml.Dom.XmlDocument]::new(); \
                 $xml.LoadXml(@\"\n<toast>\n  <visual>\n    <binding template='ToastGeneric'>\n      <text>{esc_title}</text>\n      <text>{esc_body}</text>\n      {img_tag}\n    </binding>\n  </visual>\n</toast>\n\"@); \
                 $toast = [Windows.UI.Notifications.ToastNotification]::new($xml); \
                 [Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier('mDm Media Download Manager').Show($toast);"
            );

            let mut cmd = std::process::Command::new("powershell");
            cmd.args(["-NoProfile", "-WindowStyle", "Hidden", "-Command", &script])
                .creation_flags(0x0800_0000); // CREATE_NO_WINDOW
            let _ = cmd.output();
        }

        #[cfg(target_os = "macos")]
        {
            let script = format!(
                "display notification \"{}\" with title \"{}\"",
                body.replace('"', "\\\""),
                title.replace('"', "\\\"")
            );
            let _ = std::process::Command::new("osascript")
                .args(["-e", &script])
                .output();
        }
    });
}
