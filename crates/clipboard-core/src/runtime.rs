use std::sync::Arc;
use std::time::Duration;

use clipboard_proto::event::Event;
use clipboard_proto::types::HistoryEntry;
use tokio::sync::{broadcast, oneshot};

use crate::channels::{AppCommand, AppCommandTx, Channels};
use crate::config::AppConfig;
use crate::coordinator::Coordinator;
use crate::discovery::DiscoveryService;
use crate::messaging::MessagingService;
use crate::storage::Storage;

pub struct AppHandle {
    runtime: tokio::runtime::Runtime,
    pub(crate) app_cmd_tx: AppCommandTx,
    event_tx: broadcast::Sender<Event>,
}

impl AppHandle {
    pub fn next_event(&self) -> Option<Event> {
        let mut rx = self.event_tx.subscribe();
        rx.try_recv().ok()
    }

    pub fn next_event_blocking(&self, timeout: Duration) -> Option<Event> {
        let mut rx = self.event_tx.subscribe();
        let rt = tokio::runtime::Handle::current();
        match rt
            .block_on(async { tokio::time::timeout(timeout, async { rx.recv().await.ok() }).await })
        {
            Ok(Some(event)) => Some(event),
            _ => None,
        }
    }

    pub fn request_pairing(&self, device_id: uuid::Uuid) {
        let _ = self
            .app_cmd_tx
            .send(AppCommand::RequestPairing { device_id });
    }

    pub fn approve_pairing(&self, device_id: uuid::Uuid) {
        let _ = self
            .app_cmd_tx
            .send(AppCommand::ApprovePairing { device_id });
    }

    pub fn reject_pairing(&self, device_id: uuid::Uuid, reason: &str) {
        let _ = self.app_cmd_tx.send(AppCommand::RejectPairing {
            device_id,
            reason: reason.to_string(),
        });
    }

    pub fn on_clipboard_changed(&self, content: &str) {
        let _ = self.app_cmd_tx.send(AppCommand::ClipboardChanged {
            content: content.to_string(),
            content_type: "text/plain".to_string(),
        });
    }

    pub fn restore_history_entry(&self, clipboard_id: uuid::Uuid) {
        let _ = self
            .app_cmd_tx
            .send(AppCommand::RestoreHistoryEntry { clipboard_id });
    }

    pub fn history(&self, limit: usize) -> Vec<HistoryEntry> {
        let (tx, rx) = oneshot::channel();
        let _ = self.app_cmd_tx.send(AppCommand::ListHistory {
            limit,
            response: tx,
        });
        self.runtime
            .handle()
            .block_on(async { rx.await.unwrap_or_default() })
    }

    pub fn clear_history(&self) -> Result<(), String> {
        let (tx, rx) = oneshot::channel();
        let _ = self
            .app_cmd_tx
            .send(AppCommand::ClearHistory { response: tx });
        self.runtime
            .handle()
            .block_on(async { rx.await.unwrap_or(Ok(())) })
    }

    pub fn is_paused(&self) -> bool {
        let (tx, rx) = oneshot::channel();
        let _ = self.app_cmd_tx.send(AppCommand::IsPaused { response: tx });
        self.runtime
            .handle()
            .block_on(async { rx.await.unwrap_or(false) })
    }

    pub fn set_paused(&self, paused: bool) {
        let _ = self.app_cmd_tx.send(AppCommand::SetPaused { paused });
    }

    pub fn stop(&self) {
        let (tx, rx) = oneshot::channel();
        let _ = self.app_cmd_tx.send(AppCommand::Stop { response: tx });
        let _ = self.runtime.handle().block_on(rx);
    }

    pub fn event_tx(&self) -> broadcast::Sender<Event> {
        self.event_tx.clone()
    }

    pub fn app_cmd_tx(&self) -> AppCommandTx {
        self.app_cmd_tx.clone()
    }

    /// Send a raw command (for integration testing).
    #[doc(hidden)]
    pub fn send_command(&self, cmd: AppCommand) {
        let _ = self.app_cmd_tx.send(cmd);
    }
}

pub fn start(config: AppConfig) -> Result<AppHandle, clipboard_proto::error::Error> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("clipboard-sync")
        .build()
        .map_err(clipboard_proto::error::Error::Io)?;

    let (device_id, device_name, _created_at) = {
        let storage = Storage::open(&config.storage)?;
        storage.get_device_identity()?
    };

    let storage = Arc::new(Storage::open_with_history_cap(
        &config.storage,
        config.history.max_size_bytes,
    )?);

    let channels = Channels::new();

    let platform = clipboard_proto::message::current_platform();

    let _guard = runtime.enter();

    let _discovery_handle = DiscoveryService::spawn(
        &config,
        device_id,
        device_name.clone(),
        platform,
        channels.event_tx.clone(),
    );

    let _messaging_handle = MessagingService::spawn(
        &config,
        device_id,
        device_name.clone(),
        platform,
        storage.clone(),
        channels.event_tx.clone(),
        channels.inbound_tx.clone(),
        channels.msg_cmd_rx,
    );

    let _coordinator_handle = Coordinator::spawn(
        &config,
        device_id,
        device_name.clone(),
        storage.clone(),
        channels.msg_cmd_tx.clone(),
        channels.event_tx.clone(),
        channels.app_cmd_rx,
        channels.inbound_rx,
    );

    let app_handle = AppHandle {
        runtime,
        app_cmd_tx: channels.app_cmd_tx,
        event_tx: channels.event_tx,
    };

    Ok(app_handle)
}
