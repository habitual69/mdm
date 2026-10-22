//! mdm desktop shell. Boots the same daemon the CLI uses (mdm-server on
//! 127.0.0.1:5757) and shows the embedded web UI in a native window.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use tauri::WebviewUrl;

const ADDR: &str = "127.0.0.1:5757";

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "mdm=info".into()))
        .init();

    tauri::Builder::default()
        .setup(|app| {
            // Boot the queue + HTTP/WS server in the background.
            tauri::async_runtime::spawn(async move {
                let cfg = match mdm_core::load_config() {
                    Ok(cfg) => cfg,
                    Err(e) => {
                        eprintln!("mdm: failed to load config: {e}");
                        return;
                    }
                };
                // Setup installs deps on first run; then the queue is built.
                let installer = mdm_bins::Installer::new(&cfg);
                for st in installer.ensure_all(&cfg).await {
                    if let mdm_bins::BinAction::Failed { error } = &st.action {
                        eprintln!("mdm: setup {}: {error}", st.name);
                    }
                }
                match mdm_server::build_queue(cfg) {
                    Ok(q) => {
                        if let Err(e) = mdm_server::serve(q, ADDR).await {
                            eprintln!("mdm: server exited: {e}");
                        }
                    }
                    Err(e) => eprintln!("mdm: failed to build queue: {e}"),
                }
            });

            // Wait until the server accepts connections (max ~15 s), then
            // open the window pointing at it.
            let ready = || async {
                tokio::net::TcpStream::connect(ADDR).await.is_ok()
            };
            let mut attempts = 0u32;
            while !tauri::async_runtime::block_on(ready()) {
                attempts += 1;
                if attempts > 300 {
                    eprintln!("mdm: server did not come up; opening window anyway");
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }

            let handle = app.handle().clone();
            tauri::WebviewWindowBuilder::new(
                &handle,
                "main",
                WebviewUrl::External(format!("http://{ADDR}/").parse().expect("valid url")),
            )
            .title("mdm — Media Download Manager")
            .inner_size(1180.0, 780.0)
            .min_inner_size(820.0, 560.0)
            .build()?;

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running mdm");
}
