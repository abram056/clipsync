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
        "clipboard_sync_test_d_{}_{}_{}",
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

fn setup_paired_manual(
    handle_a: &AppHandle,
    handle_b: &AppHandle,
) -> tokio::sync::broadcast::Receiver<clipboard_proto::event::Event> {
    let mut rx_a = handle_a.event_tx().subscribe();
    let mut rx_b = handle_b.event_tx().subscribe();

    let a_device_id = wait_for_discovered(&mut rx_b, Duration::from_secs(5))
        .expect("B should discover A via unicast")
        .device_id;

    let cmd_a = handle_a.app_cmd_tx();
    let ev_a = handle_a.event_tx();
    let approver_handle = std::thread::spawn(move || {
        let mut rx = ev_a.subscribe();
        loop {
            match rx.try_recv() {
                Ok(event) => {
                    if let EventType::PairingRequested(p) = &event.event_type {
                        let _ = cmd_a.send(AppCommand::ApprovePairing {
                            device_id: p.device_id,
                        });
                        break;
                    }
                }
                Err(_) => std::thread::sleep(Duration::from_millis(50)),
            }
        }
    });

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

    let _ = approver_handle.join();

    std::thread::sleep(Duration::from_secs(1));

    handle_a.event_tx().subscribe()
}

#[test]
fn trusted_peers_list_after_pairing() {
    let config_a = make_config(1, 61191, 61192, None);
    let config_b = make_config(2, 61192, 61193, Some("127.0.0.1:61191".parse().unwrap()));

    let handle_a = start(config_a).expect("start A");
    let handle_b = start(config_b).expect("start B");

    setup_paired_manual(&handle_a, &handle_b);

    let trusted_a = handle_a.trusted_peers();
    assert_eq!(trusted_a.len(), 1, "A should have 1 trusted peer");
    assert!(
        !trusted_a[0].device_name.is_empty(),
        "trusted peer name should not be empty"
    );

    let trusted_b = handle_b.trusted_peers();
    assert_eq!(trusted_b.len(), 1, "B should have 1 trusted peer");

    handle_a.stop();
    handle_b.stop();
}

#[test]
fn history_size_bytes_after_sync() {
    let config = make_config(3, 61201, 61202, None);
    let handle = start(config).expect("start");

    handle.on_clipboard_changed("first");
    std::thread::sleep(Duration::from_millis(300));

    handle.on_clipboard_changed("second");
    std::thread::sleep(Duration::from_millis(300));

    let size = handle.history_size_bytes();
    assert!(size > 0, "history size should be > 0, got {}", size);

    handle.stop();
}

#[test]
fn pending_pairing_requests_query() {
    let config = make_config(4, 61211, 61212, None);
    let handle = start(config).expect("start");

    let pending = handle.pending_pairing_requests();
    assert!(
        pending.is_empty(),
        "should have no pending pairing requests at start"
    );

    handle.stop();
}

#[test]
fn forget_stops_sync_and_reconnects() {
    let config_a = make_config(5, 61221, 61222, None);
    let config_b = make_config(6, 61222, 61223, Some("127.0.0.1:61221".parse().unwrap()));

    let handle_a = start(config_a).expect("start A");
    let handle_b = start(config_b).expect("start B");

    let mut rx_b = handle_b.event_tx().subscribe();

    let a_id = wait_for_discovered(&mut rx_b, Duration::from_secs(5))
        .expect("B should discover A")
        .device_id;

    // B requests pairing, A approves inline via its command channel
    let cmd_a = handle_a.app_cmd_tx();
    let ev_a = handle_a.event_tx();
    std::thread::spawn(move || {
        let mut rx = ev_a.subscribe();
        loop {
            if let Ok(event) = rx.try_recv() {
                if let EventType::PairingRequested(p) = &event.event_type {
                    let _ = cmd_a.send(AppCommand::ApprovePairing {
                        device_id: p.device_id,
                    });
                    break;
                }
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    });

    std::thread::sleep(Duration::from_secs(2));
    handle_b.send_command(AppCommand::RequestPairing { device_id: a_id });

    let mut rx_a = handle_a.event_tx().subscribe();
    wait_for_event(&mut rx_a, Duration::from_secs(10), |et| {
        matches!(et, EventType::PairingAccepted(_))
    });
    wait_for_event(&mut rx_b, Duration::from_secs(5), |et| {
        matches!(et, EventType::PairingAccepted(_))
    });

    std::thread::sleep(Duration::from_secs(1));

    // Confirm pairing
    assert_eq!(
        handle_a.trusted_peers().len(),
        1,
        "A should have 1 trusted peer after pairing"
    );

    // A forgets B
    let b_id = handle_a.trusted_peers().first().unwrap().device_id;
    handle_a.forget_device(b_id).unwrap();
    std::thread::sleep(Duration::from_secs(1));

    // A should have no trusted peers
    assert!(
        handle_a.trusted_peers().is_empty(),
        "A should have 0 trusted peers after forget"
    );

    // A copies, B should NOT receive it
    handle_a.on_clipboard_changed("after forget");
    std::thread::sleep(Duration::from_secs(3));

    let received = wait_for_event(&mut rx_b, Duration::from_millis(500), |et| {
        matches!(et, EventType::ClipboardUpdatedFromRemote(_))
    });
    assert!(
        !received,
        "B should NOT receive clipboard after being forgotten by A"
    );

    handle_a.stop();
    handle_b.stop();
}
