mod errors;
mod types;

pub use errors::ClipSyncError;
pub use types::{
    event_to_bridge, lag_event, BridgeConfig, BridgeDevice, BridgeEvent, BridgeEventKind,
    BridgeHistoryEntry, BridgePairingRequest, ClipboardEventPayload, DeviceEventPayload,
    DeviceIdentity, SyncEventPayload,
};

use std::sync::{Arc, Mutex};

use crate::runtime::{start, AppHandle};

/// Listener for curated core events crossing the Android boundary.
///
/// Implemented on the Kotlin side via UniFFI. `Send + Sync` so the bridge can
/// invoke it from the Tokio runtime that owns event forwarding.
pub trait ClipSyncEventListener: Send + Sync {
    fn on_event(&self, event: BridgeEvent);
}

struct Subscription {
    cancel: tokio::sync::oneshot::Sender<()>,
}

pub struct ClipSyncCore {
    handle: Mutex<Option<Arc<AppHandle>>>,
    subscription: Mutex<Option<Subscription>>,
}

impl ClipSyncCore {
    pub fn start(config: BridgeConfig) -> Result<ClipSyncCore, ClipSyncError> {
        let app_config = config.into_app_config()?;
        let handle = start(app_config).map_err(ClipSyncError::from)?;
        Ok(ClipSyncCore {
            handle: Mutex::new(Some(Arc::new(handle))),
            subscription: Mutex::new(None),
        })
    }

    fn with_handle<F, T>(&self, f: F) -> Result<T, ClipSyncError>
    where
        F: FnOnce(&AppHandle) -> Result<T, ClipSyncError>,
    {
        let handle = {
            let guard = self
                .handle
                .lock()
                .map_err(|_| ClipSyncError::InternalError)?;
            guard.as_ref().cloned().ok_or(ClipSyncError::CoreStopped)?
        };
        f(&handle)
    }

    fn uuid_arg(value: &str) -> Result<uuid::Uuid, ClipSyncError> {
        uuid::Uuid::parse_str(value).map_err(|_| ClipSyncError::InvalidDeviceId)
    }

    pub fn identity(&self) -> Result<DeviceIdentity, ClipSyncError> {
        self.with_handle(|handle| {
            let (device_id, device_name) = handle.identity();
            Ok(DeviceIdentity {
                device_id: device_id.to_string(),
                device_name,
            })
        })
    }

    pub fn stop(&self) {
        let handle = {
            if let Ok(mut guard) = self.handle.lock() {
                guard.take()
            } else {
                None
            }
        };
        if let Ok(mut guard) = self.subscription.lock() {
            if let Some(sub) = guard.take() {
                let _ = sub.cancel.send(());
            }
        }
        if let Some(handle) = handle {
            handle.stop();
        }
    }

    // --- Devices ---

    pub fn trusted_peers(&self) -> Result<Vec<BridgeDevice>, ClipSyncError> {
        self.with_handle(|handle| {
            let connected = handle
                .connected_device_ids()
                .map_err(|_| ClipSyncError::OperationFailed)?;
            let connected: std::collections::HashSet<uuid::Uuid> = connected.into_iter().collect();
            let peers = handle.trusted_peers().map_err(map_core_err)?;
            Ok(peers
                .iter()
                .map(|peer| BridgeDevice::from_trusted(peer, connected.contains(&peer.device_id)))
                .collect())
        })
    }

    pub fn discovered_peers(&self) -> Result<Vec<BridgeDevice>, ClipSyncError> {
        self.with_handle(|handle| {
            let peers = handle.discovered_peers().map_err(map_core_err)?;
            Ok(peers.iter().map(BridgeDevice::from_discovered).collect())
        })
    }

    pub fn forget_device(&self, device_id: String) -> Result<(), ClipSyncError> {
        let id = Self::uuid_arg(&device_id)?;
        self.with_handle(|handle| handle.forget_device(id).map_err(map_core_err))
    }

    // --- Pairing ---

    pub fn pending_pairing_requests(&self) -> Result<Vec<BridgePairingRequest>, ClipSyncError> {
        self.with_handle(|handle| {
            let requests = handle.pending_pairing_requests().map_err(map_core_err)?;
            Ok(requests.iter().map(BridgePairingRequest::from).collect())
        })
    }

    pub fn request_pairing(&self, device_id: String) -> Result<(), ClipSyncError> {
        let id = Self::uuid_arg(&device_id)?;
        self.with_handle(|handle| {
            handle.request_pairing(id);
            Ok(())
        })
    }

    pub fn approve_pairing(&self, device_id: String) -> Result<(), ClipSyncError> {
        let id = Self::uuid_arg(&device_id)?;
        self.with_handle(|handle| {
            handle.approve_pairing(id);
            Ok(())
        })
    }

    pub fn reject_pairing(&self, device_id: String, reason: String) -> Result<(), ClipSyncError> {
        let id = Self::uuid_arg(&device_id)?;
        self.with_handle(|handle| {
            handle.reject_pairing(id, &reason);
            Ok(())
        })
    }

    // --- Clipboard ---

    pub fn on_clipboard_changed(&self, content: String) {
        if let Ok(guard) = self.handle.lock() {
            if let Some(handle) = guard.as_ref() {
                handle.on_clipboard_changed(&content);
            }
        }
    }

    pub fn restore_history_entry(&self, clipboard_id: String) -> Result<(), ClipSyncError> {
        let id =
            uuid::Uuid::parse_str(&clipboard_id).map_err(|_| ClipSyncError::InvalidArgument)?;
        self.with_handle(|handle| {
            handle.restore_history_entry(id).map_err(|e| {
                if e.contains("not found") {
                    ClipSyncError::NotFound
                } else {
                    ClipSyncError::OperationFailed
                }
            })
        })
    }

    // --- History ---

    pub fn history(&self, limit: u32) -> Result<Vec<BridgeHistoryEntry>, ClipSyncError> {
        self.with_handle(|handle| {
            let entries = handle.history(limit as usize).map_err(map_core_err)?;
            Ok(entries.iter().map(BridgeHistoryEntry::from).collect())
        })
    }

    pub fn clear_history(&self) -> Result<(), ClipSyncError> {
        self.with_handle(|handle| handle.clear_history().map_err(map_core_err))
    }

    pub fn history_size_bytes(&self) -> Result<i64, ClipSyncError> {
        self.with_handle(|handle| {
            let size = handle.history_size_bytes().map_err(map_core_err)?;
            Ok(size as i64)
        })
    }

    // --- Synchronization ---

    pub fn is_paused(&self) -> Result<bool, ClipSyncError> {
        self.with_handle(|handle| handle.is_paused().map_err(map_core_err))
    }

    pub fn set_paused(&self, paused: bool) -> Result<(), ClipSyncError> {
        self.with_handle(|handle| {
            handle.set_paused(paused);
            Ok(())
        })
    }

    // --- Events ---

    pub fn subscribe(&self, listener: Box<dyn ClipSyncEventListener>) {
        let handle = match self.handle.lock() {
            Ok(guard) => guard.as_ref().cloned(),
            Err(_) => None,
        };
        let Some(handle) = handle else { return };

        if let Ok(mut guard) = self.subscription.lock() {
            if let Some(old) = guard.take() {
                let _ = old.cancel.send(());
            }
        }

        let (cancel_tx, mut cancel_rx) = tokio::sync::oneshot::channel::<()>();
        let mut rx = handle.event_tx().subscribe();

        handle.spawn_task(async move {
            loop {
                tokio::select! {
                    _ = &mut cancel_rx => break,
                    msg = rx.recv() => {
                        match msg {
                            Ok(event) => {
                                if let Some(bridge_event) = event_to_bridge(&event) {
                                    listener.on_event(bridge_event);
                                }
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                                listener.on_event(lag_event(n));
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                        }
                    }
                }
            }
        });

        if let Ok(mut guard) = self.subscription.lock() {
            *guard = Some(Subscription { cancel: cancel_tx });
        }
    }

    pub fn unsubscribe(&self) {
        if let Ok(mut guard) = self.subscription.lock() {
            if let Some(sub) = guard.take() {
                let _ = sub.cancel.send(());
            }
        }
    }
}

fn map_core_err(e: String) -> ClipSyncError {
    if e.contains("not found") {
        ClipSyncError::NotFound
    } else {
        ClipSyncError::OperationFailed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_port(port: u16) -> u16 {
        port + (std::process::id() % 1_000) as u16
    }

    fn temp_storage_path() -> String {
        std::env::temp_dir()
            .join(format!(
                "clipsync_bridge_test_{}_{}",
                std::process::id(),
                uuid::Uuid::new_v4()
            ))
            .display()
            .to_string()
    }

    fn valid_config() -> BridgeConfig {
        BridgeConfig {
            storage_path: temp_storage_path(),
            discovery_port: test_port(58271),
            listen_port: test_port(58272),
            discovery_targets: Vec::new(),
            sync_enabled: true,
        }
    }

    #[test]
    fn empty_storage_path_is_rejected() {
        let config = BridgeConfig {
            storage_path: "   ".to_string(),
            ..valid_config()
        };
        assert!(matches!(
            ClipSyncCore::start(config),
            Err(ClipSyncError::InvalidArgument)
        ));
    }

    #[test]
    fn invalid_discovery_target_is_rejected() {
        let config = BridgeConfig {
            discovery_targets: vec!["not-an-address".to_string()],
            ..valid_config()
        };
        assert!(matches!(
            ClipSyncCore::start(config),
            Err(ClipSyncError::InvalidArgument)
        ));
    }

    #[test]
    fn zero_port_is_rejected() {
        let config = BridgeConfig {
            listen_port: 0,
            ..valid_config()
        };
        assert!(matches!(
            ClipSyncCore::start(config),
            Err(ClipSyncError::InvalidArgument)
        ));
    }

    #[test]
    fn start_identity_stop_roundtrip() {
        let core = ClipSyncCore::start(valid_config()).expect("core must start");

        let identity = core.identity().expect("identity must be available");
        assert!(uuid::Uuid::parse_str(&identity.device_id).is_ok());
        assert!(!identity.device_name.is_empty());

        core.stop();
        assert!(matches!(core.identity(), Err(ClipSyncError::CoreStopped)));

        core.stop();
    }

    #[test]
    fn query_methods_return_empty_on_fresh_start() {
        let core = ClipSyncCore::start(valid_config()).expect("core must start");

        assert!(core.trusted_peers().unwrap().is_empty());
        assert!(core.discovered_peers().unwrap().is_empty());
        assert!(core.pending_pairing_requests().unwrap().is_empty());
        assert!(core.history(10).unwrap().is_empty());
        assert_eq!(core.history_size_bytes().unwrap(), 0);
        assert!(!core.is_paused().unwrap());

        core.stop();
    }

    #[test]
    fn restore_history_entry_missing_is_not_found() {
        let core = ClipSyncCore::start(valid_config()).expect("core must start");

        let missing = uuid::Uuid::new_v4().to_string();
        assert!(matches!(
            core.restore_history_entry(missing),
            Err(ClipSyncError::NotFound)
        ));

        core.stop();
    }

    #[test]
    fn restore_history_entry_invalid_uuid_is_invalid_argument() {
        let core = ClipSyncCore::start(valid_config()).expect("core must start");

        assert!(matches!(
            core.restore_history_entry("not-a-uuid".to_string()),
            Err(ClipSyncError::InvalidArgument)
        ));

        core.stop();
    }

    #[test]
    fn forget_device_invalid_uuid_is_invalid_device_id() {
        let core = ClipSyncCore::start(valid_config()).expect("core must start");

        assert!(matches!(
            core.forget_device("nope".to_string()),
            Err(ClipSyncError::InvalidDeviceId)
        ));

        core.stop();
    }

    struct TestListener {
        tx: std::sync::mpsc::Sender<BridgeEvent>,
    }

    impl ClipSyncEventListener for TestListener {
        fn on_event(&self, event: BridgeEvent) {
            let _ = self.tx.send(event);
        }
    }

    #[test]
    fn subscription_receives_clipboard_history_updated() {
        let core = ClipSyncCore::start(valid_config()).expect("core must start");

        let (tx, rx) = std::sync::mpsc::channel();
        core.subscribe(Box::new(TestListener { tx }));

        core.on_clipboard_changed("hello bridge".to_string());

        let event = rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("should receive an event");
        assert_eq!(event.kind, BridgeEventKind::ClipboardHistoryUpdated);
        assert!(!event.event_id.is_empty());

        core.unsubscribe();
        core.stop();
    }

    #[test]
    fn unsubscribe_stops_delivery() {
        let core = ClipSyncCore::start(valid_config()).expect("core must start");

        let (tx, rx) = std::sync::mpsc::channel();
        core.subscribe(Box::new(TestListener { tx }));
        core.unsubscribe();

        core.on_clipboard_changed("should not arrive".to_string());

        // Give the forwarder time to (not) deliver, then confirm the channel is quiet.
        std::thread::sleep(std::time::Duration::from_millis(300));
        assert!(rx.try_recv().is_err());

        core.stop();
    }

    fn make_event(event_type: clipboard_proto::event::EventType) -> clipboard_proto::event::Event {
        clipboard_proto::event::Event {
            event_id: uuid::Uuid::new_v4(),
            timestamp: chrono::Utc::now(),
            source: clipboard_proto::event::EventSource::UserInterface,
            event_type,
        }
    }

    #[test]
    fn event_to_bridge_maps_device_discovered() {
        let payload = clipboard_proto::event::DeviceDiscoveredPayload {
            device_id: uuid::Uuid::new_v4(),
            device_name: "peer".to_string(),
            platform: clipboard_proto::types::Platform::Linux,
            ip_address: "127.0.0.1".to_string(),
            port: 48272,
        };
        let event = make_event(clipboard_proto::event::EventType::DeviceDiscovered(payload));

        let bridge = event_to_bridge(&event).expect("device discovered is forwarded");
        assert_eq!(bridge.kind, BridgeEventKind::DeviceDiscovered);
        let device = bridge.device.expect("device payload present");
        assert_eq!(device.device_name, "peer");
        assert_eq!(device.platform, "linux");
    }

    #[test]
    fn event_to_bridge_skips_internal_events() {
        let event = make_event(clipboard_proto::event::EventType::ClipboardChanged(
            clipboard_proto::event::ClipboardChangedPayload {
                content: "x".to_string(),
                content_type: "text/plain".to_string(),
            },
        ));
        assert!(event_to_bridge(&event).is_none());
    }

    #[test]
    fn lag_event_reports_dropped_count() {
        let event = lag_event(42);
        assert_eq!(event.kind, BridgeEventKind::EventStreamLagged);
        assert_eq!(event.dropped_events, 42);
    }
}
