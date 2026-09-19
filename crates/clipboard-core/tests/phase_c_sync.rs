use std::net::SocketAddr;
use std::time::Duration;

use clipboard_core::channels::AppCommand;
use clipboard_core::config::{
    AppConfig, HistoryConfig, NetworkConfig, PlatformConfig, StorageConfig, SyncConfig,
};
use clipboard_core::{start, AppHandle};
use clipboard_proto::event::EventType;

fn test_port(port: u16) -> u16 {
    port + (std::process::id() % 1_000) as u16
}

fn make_config(
    id: u16,
    discovery_port: u16,
    listen_port: u16,
    peer: Option<SocketAddr>,
) -> AppConfig {
    let mut discovery_targets = Vec::new();
    if let Some(p) = peer {
        discovery_targets.push(SocketAddr::new(p.ip(), test_port(p.port())));
    }

    let db_path = std::env::temp_dir().join(format!(
        "clipboard_sync_test_{}_{}_{}",
        id,
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(format!("{}-wal", db_path.display()));
    let _ = std::fs::remove_file(format!("{}-shm", db_path.display()));

    AppConfig {
        network: NetworkConfig {
            discovery_port: test_port(discovery_port),
            listen_port: test_port(listen_port),
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
        storage: StorageConfig { path: db_path },
        platform: PlatformConfig {
            clipboard_poll_interval_ms: 250,
        },
    }
}

fn drain_events(
    rx: &mut tokio::sync::broadcast::Receiver<clipboard_proto::event::Event>,
    timeout: Duration,
    predicate: impl Fn(&EventType) -> bool,
) -> Vec<EventType> {
    let deadline = std::time::Instant::now() + timeout;
    let mut found = Vec::new();
    loop {
        match rx.try_recv() {
            Ok(event) => {
                if predicate(&event.event_type) {
                    found.push(event.event_type);
                }
            }
            Err(tokio::sync::broadcast::error::TryRecvError::Empty) => {
                if std::time::Instant::now() >= deadline {
                    break;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(_) => break,
        }
    }
    found
}

fn wait_for_event(
    rx: &mut tokio::sync::broadcast::Receiver<clipboard_proto::event::Event>,
    timeout: Duration,
    predicate: impl Fn(&EventType) -> bool,
) -> bool {
    !drain_events(rx, timeout, predicate).is_empty()
}

fn wait_for_discovered(
    rx: &mut tokio::sync::broadcast::Receiver<clipboard_proto::event::Event>,
    timeout: Duration,
) -> Option<clipboard_proto::event::DeviceDiscoveredPayload> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        match rx.try_recv() {
            Ok(event) => {
                if let EventType::DeviceDiscovered(payload) = event.event_type {
                    return Some(payload);
                }
            }
            Err(tokio::sync::broadcast::error::TryRecvError::Empty) => {
                if std::time::Instant::now() >= deadline {
                    return None;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(_) => return None,
        }
    }
}

fn spawn_auto_approver(handle: &AppHandle) {
    let cmd_tx = handle.app_cmd_tx();
    let event_tx = handle.event_tx();
    std::thread::spawn(move || {
        let mut rx = event_tx.subscribe();
        loop {
            match rx.try_recv() {
                Ok(event) => {
                    if let EventType::PairingRequested(p) = &event.event_type {
                        let _ = cmd_tx.send(AppCommand::ApprovePairing {
                            device_id: p.device_id,
                        });
                    }
                }
                Err(_) => std::thread::sleep(Duration::from_millis(100)),
            }
        }
    });
}

fn setup_paired(
    handle_a: &AppHandle,
    handle_b: &AppHandle,
) -> tokio::sync::broadcast::Receiver<clipboard_proto::event::Event> {
    let mut rx_a = handle_a.event_tx().subscribe();
    let mut rx_b = handle_b.event_tx().subscribe();

    let a_device_id = wait_for_discovered(&mut rx_b, Duration::from_secs(5))
        .expect("B should discover A via unicast")
        .device_id;

    spawn_auto_approver(handle_a);

    std::thread::sleep(Duration::from_secs(2));

    handle_b.send_command(AppCommand::RequestPairing {
        device_id: a_device_id,
    });

    let paired = wait_for_event(&mut rx_a, Duration::from_secs(10), |et| {
        matches!(et, EventType::PairingAccepted(_))
    });
    assert!(paired, "A should accept pairing");

    let paired_b = wait_for_event(&mut rx_b, Duration::from_secs(5), |et| {
        matches!(et, EventType::PairingAccepted(_))
    });
    assert!(paired_b, "B should see pairing accepted");

    std::thread::sleep(Duration::from_secs(1));

    handle_a.event_tx().subscribe()
}

#[test]
fn bidirectional_sync() {
    let config_a = make_config(1, 60191, 60192, None);
    let config_b = make_config(2, 60192, 60193, Some("127.0.0.1:60191".parse().unwrap()));

    let handle_a = start(config_a).expect("start A");
    let handle_b = start(config_b).expect("start B");

    let mut rx_a = setup_paired(&handle_a, &handle_b);

    handle_b.on_clipboard_changed("hello from B");

    let received = wait_for_event(&mut rx_a, Duration::from_secs(5), |et| {
        matches!(et, EventType::ClipboardUpdatedFromRemote(_))
    });
    assert!(received, "A should receive clipboard from B");

    handle_a.on_clipboard_changed("hello from A");

    let mut rx_b = handle_b.event_tx().subscribe();
    let received = wait_for_event(&mut rx_b, Duration::from_secs(5), |et| {
        matches!(et, EventType::ClipboardUpdatedFromRemote(_))
    });
    assert!(received, "B should receive clipboard from A");

    handle_a.stop();
    handle_b.stop();
}

#[test]
fn no_loop() {
    let config_a = make_config(3, 60201, 60202, None);
    let config_b = make_config(4, 60202, 60203, Some("127.0.0.1:60201".parse().unwrap()));

    let handle_a = start(config_a).expect("start A");
    let handle_b = start(config_b).expect("start B");

    let mut rx_a = setup_paired(&handle_a, &handle_b);

    handle_b.on_clipboard_changed("test content");

    let received = wait_for_event(&mut rx_a, Duration::from_secs(5), |et| {
        matches!(et, EventType::ClipboardUpdatedFromRemote(_))
    });
    assert!(received, "A should receive clipboard");

    std::thread::sleep(Duration::from_secs(2));

    let mut rx_b = handle_b.event_tx().subscribe();
    let looped = wait_for_event(&mut rx_b, Duration::from_millis(500), |et| {
        matches!(et, EventType::ClipboardUpdatedFromRemote(_))
    });
    assert!(!looped, "B must NOT receive its own content back (no loop)");

    handle_a.stop();
    handle_b.stop();
}

#[test]
fn no_duplicate_local_copy() {
    let config = make_config(5, 60211, 60212, None);
    let handle = start(config).expect("start");
    let mut rx = handle.event_tx().subscribe();

    handle.on_clipboard_changed("same content");
    std::thread::sleep(Duration::from_millis(300));
    handle.on_clipboard_changed("same content");
    std::thread::sleep(Duration::from_millis(300));

    let dups = drain_events(&mut rx, Duration::from_millis(500), |et| {
        matches!(et, EventType::DuplicateClipboardIgnored(_))
    });
    assert!(
        !dups.is_empty(),
        "should have at least one DuplicateClipboardIgnored"
    );

    handle.stop();
}

#[test]
fn pause_resume() {
    let config_a = make_config(6, 60221, 60222, None);
    let config_b = make_config(7, 60222, 60223, Some("127.0.0.1:60221".parse().unwrap()));

    let handle_a = start(config_a).expect("start A");
    let handle_b = start(config_b).expect("start B");

    let mut rx_a = setup_paired(&handle_a, &handle_b);

    handle_a.set_paused(true);
    assert!(handle_a.is_paused().unwrap());

    handle_b.on_clipboard_changed("while paused");
    std::thread::sleep(Duration::from_secs(2));

    let received = wait_for_event(&mut rx_a, Duration::from_millis(500), |et| {
        matches!(et, EventType::ClipboardUpdatedFromRemote(_))
    });
    assert!(!received, "paused node should not receive clipboard");

    handle_a.set_paused(false);
    assert!(!handle_a.is_paused().unwrap());

    handle_a.stop();
    handle_b.stop();
}

#[test]
fn history_list_and_restore() {
    let config = make_config(8, 60231, 60232, None);
    let handle = start(config).expect("start");
    let mut rx = handle.event_tx().subscribe();

    handle.on_clipboard_changed("first");
    std::thread::sleep(Duration::from_millis(300));
    handle.on_clipboard_changed("second");
    std::thread::sleep(Duration::from_millis(300));

    let history = handle.history(10).unwrap();
    assert!(
        history.len() >= 2,
        "history should have at least 2 entries, got {}",
        history.len()
    );

    let entry = &history[0];
    assert_eq!(entry.content, "second");

    handle.restore_history_entry(entry.clipboard_id);
    std::thread::sleep(Duration::from_millis(200));

    let restored = wait_for_event(&mut rx, Duration::from_secs(1), |et| {
        matches!(et, EventType::ClipboardRestored(_))
    });
    assert!(restored, "should emit ClipboardRestored");

    handle.stop();
}
