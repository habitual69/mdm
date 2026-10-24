use std::path::Path;

pub fn send_system_notification(title: &str, body: &str, icon_path: Option<&Path>) {
    let title = title.to_string();
    let body = body.to_string();
    #[allow(unused_variables)]
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
            let script = format!(
                "[Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType = WindowsRuntime] > $null; \
                 $template = [Windows.UI.Notifications.ToastNotificationManager]::GetTemplateContent([Windows.UI.Notifications.ToastTemplateType]::ToastText02); \
                 $template.GetElementsByTagName('text')[0].AppendChild($template.CreateTextNode('{}')) > $null; \
                 $template.GetElementsByTagName('text')[1].AppendChild($template.CreateTextNode('{}')) > $null; \
                 $toast = [Windows.UI.Notifications.ToastNotification]::new($template); \
                 [Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier('mDm').Show($toast);",
                title.replace('\'', "''"),
                body.replace('\'', "''")
            );
            let _ = std::process::Command::new("powershell")
                .args(["-NoProfile", "-WindowStyle", "Hidden", "-Command", &script])
                .output();
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
