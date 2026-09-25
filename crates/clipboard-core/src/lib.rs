pub mod channels;
pub mod config;
pub mod coordinator;
pub mod discovery;
pub mod messaging;
pub mod pairing;
pub mod replay_cache;
pub mod runtime;
pub mod storage;

pub use config::AppConfig;
pub use runtime::{start, AppHandle};
pub use storage::Storage;

use std::collections::HashSet;
use std::io::BufRead;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::config::{HistoryConfig, NetworkConfig, PlatformConfig, StorageConfig, SyncConfig};

pub struct HarnessConfig {
    pub id: String,
    pub listen_port: u16,
    pub discovery_port: u16,
    pub peer_addr: Option<SocketAddr>,
    pub db_path: PathBuf,
    pub auto_approve: bool,
    pub auto_pair: bool,
    pub clipboard: Vec<String>,
    pub read_stdin: bool,
}

impl HarnessConfig {
    pub fn parse_args() -> Self {
        let args: Vec<String> = std::env::args().collect();
        let mut id = "a".to_string();
        let mut listen_port = 48272u16;
        let mut discovery_port = 48271u16;
        let mut peer_addr: Option<SocketAddr> = None;
        let mut db_path = PathBuf::from("/tmp/clipboard-sync-test");
        let mut auto_approve = false;
        let mut auto_pair = false;
        let mut clipboard: Vec<String> = Vec::new();
        let mut read_stdin = false;

        let mut i = 1;
        while i < args.len() {
            match args[i].as_str() {
                "--id" => {
                    i += 1;
                    id = args[i].clone();
                }
                "--listen-port" => {
                    i += 1;
                    listen_port = args[i].parse().unwrap_or(48272);
                }
                "--discovery-port" => {
                    i += 1;
                    discovery_port = args[i].parse().unwrap_or(48271);
                }
                "--peer" => {
                    i += 1;
                    peer_addr = args[i].parse().ok();
                }
                "--db" => {
                    i += 1;
                    db_path = PathBuf::from(&args[i]);
                }
                "--auto-approve" => {
                    auto_approve = true;
                }
                "--auto-pair" => {
                    auto_pair = true;
                }
                "--clipboard" => {
                    i += 1;
                    clipboard.push(args[i].clone());
                }
                "--stdin" => {
                    read_stdin = true;
                }
                _ => {}
            }
            i += 1;
        }

        db_path.push(format!("node_{}.db", id));

        Self {
            id,
            listen_port,
            discovery_port,
            peer_addr,
            db_path,
            auto_approve,
            auto_pair,
            clipboard,
            read_stdin,
        }
    }

    pub fn to_app_config(&self) -> AppConfig {
        let mut discovery_targets = Vec::new();
        if let Some(peer) = self.peer_addr {
            discovery_targets.push(peer);
        }

        AppConfig {
            network: NetworkConfig {
                discovery_port: self.discovery_port,
                listen_port: self.listen_port,
                discovery_interval_secs: 1,
                heartbeat_interval_secs: 5,
                peer_timeout_secs: 15,
                reconnect_backoff_initial_ms: 1000,
                reconnect_backoff_max_ms: 60000,
                discovery_targets,
            },
            history: HistoryConfig {
                max_size_bytes: 2_097_152,
            },
            sync: SyncConfig {
                max_clipboard_bytes: 2_097_152,
                max_peers: 16,
                replay_cache_capacity: 1000,
                replay_cache_ttl_secs: 3600,
                pairing_timeout_secs: 30,
                enabled: true,
            },
            storage: StorageConfig {
                path: self.db_path.clone(),
            },
            platform: PlatformConfig {
                clipboard_poll_interval_ms: 250,
            },
        }
    }
}

/// A single-line rendering of clipboard content for log output.
fn preview(content: &str) -> String {
    const MAX: usize = 120;
    if content.chars().count() <= MAX {
        return content.to_string();
    }
    let truncated: String = content.chars().take(MAX).collect();
    format!("{}...", truncated)
}

/// Hand queued clipboard text to the core once a trusted peer is connected.
///
/// The core records a content hash when it observes a local update, even when
/// no peer is eligible to receive it. A premature send would therefore store
/// the text without delivering it and every later attempt would be
/// deduplicated as a duplicate, so the queue is only drained when a connected
/// peer is trusted.
fn flush_clipboard(
    pending: &mut Vec<String>,
    connected: &HashSet<uuid::Uuid>,
    trusted: &HashSet<uuid::Uuid>,
    app_cmd_tx: &crate::channels::AppCommandTx,
) {
    if pending.is_empty() || !connected.iter().any(|id| trusted.contains(id)) {
        return;
    }
    for content in pending.drain(..) {
        tracing::info!("sending clipboard: {}", preview(&content));
        let _ = app_cmd_tx.send(crate::channels::AppCommand::ClipboardChanged {
            content,
            content_type: "text/plain".to_string(),
        });
    }
}

pub fn run_harness(config: HarnessConfig) {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("info".parse().unwrap()),
        )
        .init();

    let app_config = config.to_app_config();
    let node_id_for_log = config.id.clone();
    let listen_port = config.listen_port;
    let discovery_port = config.discovery_port;
    let auto_approve = config.auto_approve;
    let auto_pair = config.auto_pair;
    let handle = start(app_config).expect("failed to start");

    let (self_device_id, self_device_name) = handle.identity();
    tracing::info!(
        "Node {} started: device {} ({}) platform {} listen {} discovery {} db {}",
        node_id_for_log,
        self_device_id,
        self_device_name,
        clipboard_proto::message::current_platform(),
        listen_port,
        discovery_port,
        config.db_path.display()
    );

    let event_tx = handle.event_tx();
    let mut event_rx = event_tx.subscribe();
    let node_id = node_id_for_log.clone();
    let app_cmd_tx = handle.app_cmd_tx.clone();

    // Clipboard text is only handed to the core once a trusted peer is
    // connected: the core records a content hash on the local update itself,
    // so an attempt made before pairing would be stored without any eligible
    // target and every later attempt would then be deduplicated.
    let pending_clipboard: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(config.clipboard.clone()));
    if !config.clipboard.is_empty() {
        tracing::info!(
            "[{}] queued {} clipboard payload(s), sent once a trusted peer connects",
            node_id,
            config.clipboard.len()
        );
    }

    let mut trusted_ids: HashSet<uuid::Uuid> = handle
        .trusted_peers()
        .unwrap_or_default()
        .into_iter()
        .map(|peer| peer.device_id)
        .collect();

    if config.read_stdin {
        tracing::info!("[{}] reading clipboard lines from stdin", node_id);
        let pending = pending_clipboard.clone();
        std::thread::spawn(move || {
            let stdin = std::io::stdin();
            for line in stdin.lock().lines() {
                match line {
                    Ok(text) if !text.is_empty() => pending.lock().unwrap().push(text),
                    Ok(_) => {}
                    Err(error) => {
                        tracing::warn!("stdin closed: {}", error);
                        break;
                    }
                }
            }
        });
    }

    let mut connected: HashSet<uuid::Uuid> = HashSet::new();
    let thread_pending = pending_clipboard.clone();
    std::thread::spawn(move || {
        let mut last_flush_check = std::time::Instant::now();
        loop {
            match event_rx.try_recv() {
                Ok(event) => match &event.event_type {
                    clipboard_proto::event::EventType::DeviceDiscovered(p) => {
                        tracing::info!(
                            "[{}] Discovered: {} ({}) at {}",
                            node_id,
                            p.device_name,
                            p.device_id,
                            p.ip_address
                        );
                        if auto_pair && !trusted_ids.contains(&p.device_id) {
                            tracing::info!("[{}] Auto-pairing with {}", node_id, p.device_id);
                            let _ = app_cmd_tx.send(crate::channels::AppCommand::RequestPairing {
                                device_id: p.device_id,
                            });
                        }
                    }
                    clipboard_proto::event::EventType::PairingRequested(p) => {
                        tracing::info!(
                            "[{}] Pairing request from {} ({})",
                            node_id,
                            p.device_name,
                            p.device_id
                        );
                        if auto_approve {
                            tracing::info!("[{}] Auto-approving {}", node_id, p.device_id);
                            let _ = app_cmd_tx.send(crate::channels::AppCommand::ApprovePairing {
                                device_id: p.device_id,
                            });
                        }
                    }
                    clipboard_proto::event::EventType::PairingAccepted(p) => {
                        tracing::info!("[{}] Paired with {}", node_id, p.device_id);
                        trusted_ids.insert(p.device_id);
                    }
                    clipboard_proto::event::EventType::PairingRejected(p) => {
                        tracing::info!(
                            "[{}] Pairing rejected by {} ({:?})",
                            node_id,
                            p.device_id,
                            p.reason
                        );
                    }
                    clipboard_proto::event::EventType::DeviceConnected(p) => {
                        tracing::info!(
                            "[{}] Connected: {} ({:?})",
                            node_id,
                            p.device_name,
                            p.connection_type
                        );
                        connected.insert(p.device_id);
                    }
                    clipboard_proto::event::EventType::DeviceDisconnected(p) => {
                        tracing::info!(
                            "[{}] Disconnected: {} ({:?})",
                            node_id,
                            p.device_id,
                            p.reason
                        );
                        connected.remove(&p.device_id);
                    }
                    clipboard_proto::event::EventType::DeviceConnectionFailed(p) => {
                        tracing::warn!(
                            "[{}] Connection failed: {} - {}",
                            node_id,
                            p.device_id,
                            p.error
                        );
                    }
                    clipboard_proto::event::EventType::ClipboardUpdatedFromRemote(p) => {
                        tracing::info!(
                            "[{}] Received clipboard from {}: {}",
                            node_id,
                            p.origin_device_id,
                            preview(&p.content)
                        );
                    }
                    clipboard_proto::event::EventType::SyncCompleted(p) => {
                        tracing::info!(
                            "[{}] Clipboard {} delivered to {} device(s)",
                            node_id,
                            p.clipboard_id,
                            p.successful_devices.len()
                        );
                    }
                    _ => {}
                },
                Err(tokio::sync::broadcast::error::TryRecvError::Empty) => {
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
                Err(_) => break,
            }

            if last_flush_check.elapsed() >= std::time::Duration::from_secs(1) {
                last_flush_check = std::time::Instant::now();
                if let Ok(mut pending) = thread_pending.lock() {
                    flush_clipboard(&mut pending, &connected, &trusted_ids, &app_cmd_tx);
                }
            }
        }
    });

    tracing::info!("[{}] Press Ctrl+C to stop", node_id_for_log);

    let (tx, rx) = std::sync::mpsc::channel();
    ctrlc::set_handler(move || {
        let _ = tx.send(());
    })
    .expect("Error setting Ctrl-C handler");
    let _ = rx.recv();

    tracing::info!("[{}] Stopping...", node_id_for_log);
    handle.stop();
    tracing::info!("[{}] Stopped", node_id_for_log);
}
