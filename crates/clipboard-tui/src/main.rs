use std::path::PathBuf;

use clipboard_core::config::AppConfig;
use clipboard_core::start;
use tracing_subscriber::prelude::*;

mod clipboard_monitor;
mod tui;
mod ui;

/// Install the subscriber: INFO records go to stderr and to
/// `clipboard-sync.log` in `data_dir` (doc 09 requires both). The returned
/// guard must be held for the lifetime of the process, otherwise buffered
/// records are dropped when the app exits.
///
/// The file layer carries no ANSI styling so the log stays readable when
/// grepped.
#[must_use = "dropping the guard stops the log file writer immediately"]
fn init_logging(data_dir: &std::path::Path) -> tracing_appender::non_blocking::WorkerGuard {
    let log_file = tracing_appender::rolling::never(data_dir, "clipboard-sync.log");
    let (file_writer, guard) = tracing_appender::non_blocking(log_file);

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("info".parse().unwrap()),
        )
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
        .with(
            tracing_subscriber::fmt::layer()
                .with_ansi(false)
                .with_writer(file_writer),
        )
        .init();

    guard
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let data_dir = dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("clipboard-sync");
    let _ = std::fs::create_dir_all(&data_dir);

    let _log_guard = init_logging(&data_dir);

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

    tui::run(
        handle,
        config.platform.clipboard_poll_interval_ms,
        config.sync.pairing_timeout_secs,
    )?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::init_logging;
    use std::path::Path;

    /// Doc 09: logs go to stderr *and* a local file. This is the regression
    /// test for the wiring: the appender used to be built and never attached
    /// to the subscriber, so `clipboard-sync.log` stayed empty forever.
    #[test]
    fn init_logging_writes_records_to_the_file() {
        let dir = std::env::temp_dir().join(format!(
            "clipboard-sync-log-test-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).expect("create temp log dir");

        let guard = init_logging(&dir);
        tracing::info!("init_logging smoke marker {}", 0xCAFE_u32);
        // Dropping the guard flushes the non-blocking writer.
        drop(guard);

        let log_path_buf = dir.join("clipboard-sync.log");
        let log_path: &Path = log_path_buf.as_path();
        let contents = std::fs::read_to_string(log_path)
            .map_err(|e| format!("{}: {e}", log_path.display()))
            .expect("log file must exist after writing");
        assert!(
            contents.contains("init_logging smoke marker"),
            "record missing from {}: {contents:?}",
            log_path.display()
        );
        assert!(
            contents.contains("51966"), // 0xCAFE: the formatted argument
            "message arguments must be recorded: {contents:?}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
