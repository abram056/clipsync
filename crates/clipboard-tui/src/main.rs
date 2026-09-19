use std::path::PathBuf;

use clipboard_core::config::AppConfig;
use clipboard_core::start;

mod clipboard_monitor;
mod tui;
mod ui;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let data_dir = dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("clipboard-sync");
    let _ = std::fs::create_dir_all(&data_dir);

    let log_file = tracing_appender::rolling::never(&data_dir, "clipboard-sync.log");
    let _file_writer = tracing_appender::non_blocking(log_file);

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("info".parse().unwrap()),
        )
        .with_writer(std::io::stderr)
        .init();

    let mut config = AppConfig::load()?;

    let args: Vec<String> = std::env::args().collect();
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--listen-port" => {
                i += 1;
                if let Ok(port) = args[i].parse::<u16>() {
                    config.network.listen_port = port;
                }
            }
            "--discovery-port" => {
                i += 1;
                if let Ok(port) = args[i].parse::<u16>() {
                    config.network.discovery_port = port;
                }
            }
            "--db" => {
                i += 1;
                let path = PathBuf::from(&args[i]);
                if let Some(parent) = path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                let mut db_path = path;
                db_path.push("clipboard.db");
                config.storage.path = db_path;
            }
            "--peer" => {
                i += 1;
                if let Ok(addr) = args[i].parse::<std::net::SocketAddr>() {
                    config.network.discovery_targets.push(addr);
                }
            }
            _ => {}
        }
        i += 1;
    }

    let handle = start(config.clone())?;

    tracing::info!(
        "Clipboard TUI started (listen: {}, discovery: {}, db: {})",
        config.network.listen_port,
        config.network.discovery_port,
        config.storage.path.display()
    );

    tui::run(handle, config.platform.clipboard_poll_interval_ms)?;

    Ok(())
}
