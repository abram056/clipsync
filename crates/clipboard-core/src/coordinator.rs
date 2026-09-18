use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::Utc;
use clipboard_proto::error::ErrorCode;
use clipboard_proto::event::{Event, EventSource, EventType};
use clipboard_proto::message::{
    ClipboardAckPayload, ClipboardAckStatus, ClipboardUpdatePayload, Envelope, MessageType, Payload,
};
use clipboard_proto::types::PeerInfo;
use sha2::Digest;
use tokio::sync::broadcast;
use tokio::time;
use uuid::Uuid;

use crate::channels::{AppCommandRx, InboundEnvelopeRx, MessagingCommandTx};
use crate::config::AppConfig;
use crate::pairing::PairingManager;
use crate::replay_cache::ReplayCache;
use crate::storage::Storage;

const TIMESTAMP_SKEW_SECS: i64 = 300;

const RECONNECT_INITIAL: Duration = Duration::from_secs(1);
const RECONNECT_STEP_1: Duration = Duration::from_secs(5);
const RECONNECT_STEP_2: Duration = Duration::from_secs(15);
const RECONNECT_CAP: Duration = Duration::from_secs(60);

#[derive(Debug, Clone)]
struct ReconnectEntry {
    next_attempt: Instant,
    current_delay: Duration,
}

pub struct Coordinator {
    self_device_id: Uuid,
    self_device_name: String,
    storage: Arc<Storage>,
    msg_tx: MessagingCommandTx,
    event_tx: broadcast::Sender<Event>,
    pairing: PairingManager,
    replay_cache: ReplayCache,
    discovered_peers: std::collections::HashMap<Uuid, PeerInfo>,
    reconnect_queue: std::collections::HashMap<Uuid, ReconnectEntry>,
    connected_peers: HashSet<Uuid>,
    max_clipboard_bytes: usize,
    paused: bool,
}

impl Coordinator {
    #[allow(clippy::too_many_arguments)]
    pub fn spawn(
        config: &AppConfig,
        self_device_id: Uuid,
        self_device_name: String,
        storage: Arc<Storage>,
        msg_tx: MessagingCommandTx,
        event_tx: broadcast::Sender<Event>,
        mut app_cmd_rx: AppCommandRx,
        mut inbound_rx: InboundEnvelopeRx,
    ) -> tokio::task::JoinHandle<()> {
        let pairing = PairingManager::new(
            self_device_id,
            self_device_name.clone(),
            clipboard_proto::types::Platform::Linux,
            storage.clone(),
            msg_tx.clone(),
            event_tx.clone(),
            config.sync.pairing_timeout_secs,
        );

        let replay_cache = ReplayCache::new(
            config.sync.replay_cache_capacity,
            config.sync.replay_cache_ttl_secs,
        );

        let paused = storage
            .get_setting("sync.enabled")
            .ok()
            .flatten()
            .map(|v| v == "false")
            .unwrap_or(!config.sync.enabled);

        let mut coordinator = Self {
            self_device_id,
            self_device_name,
            storage,
            msg_tx,
            event_tx: event_tx.clone(),
            pairing,
            replay_cache,
            discovered_peers: std::collections::HashMap::new(),
            reconnect_queue: std::collections::HashMap::new(),
            connected_peers: HashSet::new(),
            max_clipboard_bytes: config.sync.max_clipboard_bytes,
            paused,
        };

        let mut event_rx = event_tx.subscribe();

        tokio::spawn(async move {
            let mut tick_interval = time::interval(Duration::from_secs(1));
            let mut prune_interval = time::interval(Duration::from_secs(60));

            loop {
                tokio::select! {
                    cmd = app_cmd_rx.recv() => {
                        match cmd {
                        Some(crate::channels::AppCommand::RequestPairing { device_id }) => {
                            if let Some(peer) = coordinator.discovered_peers.get(&device_id).cloned() {
                                coordinator.pairing.request_pairing(&device_id, &peer);
                            } else {
                                tracing::warn!("coordinator: unknown device {}", device_id);
                            }
                        }
                            Some(crate::channels::AppCommand::ApprovePairing { device_id }) => {
                                coordinator.pairing.approve(&device_id);
                                let _ = coordinator.msg_tx.send(crate::channels::MessagingCommand::ReloadTrusted);
                            }
                            Some(crate::channels::AppCommand::RejectPairing { device_id, reason }) => {
                                coordinator.pairing.reject(&device_id, &reason);
                            }
                            Some(crate::channels::AppCommand::ClipboardChanged { content, content_type }) => {
                                coordinator.on_local_clipboard(content, content_type);
                            }
                            Some(crate::channels::AppCommand::RestoreHistoryEntry { clipboard_id }) => {
                                coordinator.on_restore(clipboard_id);
                            }
                            Some(crate::channels::AppCommand::ListHistory { limit, response }) => {
                                let result = coordinator.storage.list_history(limit).unwrap_or_default();
                                let _ = response.send(result);
                            }
                            Some(crate::channels::AppCommand::ClearHistory { response }) => {
                                let result = coordinator.storage.clear_history().map_err(|e| e.to_string());
                                let _ = response.send(result);
                            }
                            Some(crate::channels::AppCommand::IsPaused { response }) => {
                                let _ = response.send(coordinator.paused);
                            }
                            Some(crate::channels::AppCommand::SetPaused { paused }) => {
                                coordinator.paused = paused;
                                let val = if paused { "false" } else { "true" };
                                let _ = coordinator.storage.set_setting("sync.enabled", val);
                                tracing::info!("coordinator: sync {}", if paused { "paused" } else { "resumed" });
                            }
                            Some(crate::channels::AppCommand::Stop { response }) => {
                                tracing::info!("coordinator: shutting down");
                                let _ = response.send(());
                                break;
                            }
                            None => break,
                        }
                    }
                inbound = inbound_rx.recv() => {
                    match inbound {
                        Some(msg) => {
                            coordinator.handle_inbound(msg).await;
                        }
                            None => break,
                        }
                    }
                event = event_rx.recv() => {
                    if let Ok(event) = event {
                        coordinator.handle_event(event).await;
                    }
                }
                    _ = tick_interval.tick() => {
                        coordinator.pairing.tick();
                        coordinator.tick_reconnect();
                    }
                    _ = prune_interval.tick() => {
                        coordinator.replay_cache.prune();
                    }
                }
            }
        })
    }

    fn on_local_clipboard(&mut self, content: String, content_type: String) {
        if self.paused {
            return;
        }

        if content.len() > self.max_clipboard_bytes {
            tracing::warn!("coordinator: local clipboard exceeds max size, ignoring");
            return;
        }

        let content_hash = compute_content_hash(&content);

        if self.replay_cache.check_and_record_hash(&content_hash) {
            tracing::debug!("coordinator: duplicate local clipboard content, ignoring");
            self.emit_event(EventType::DuplicateClipboardIgnored(
                clipboard_proto::event::DuplicateClipboardIgnoredPayload {
                    clipboard_id: Uuid::nil(),
                    reason: clipboard_proto::event::DuplicateReason::DuplicateContent,
                },
            ));
            return;
        }

        let clipboard_id = Uuid::new_v4();
        let now = Utc::now();
        let entry = clipboard_proto::types::HistoryEntry {
            clipboard_id,
            content: content.clone(),
            content_type: content_type.clone(),
            content_hash: content_hash.clone(),
            origin_device_id: self.self_device_id,
            created_at: now,
            size_bytes: content.len() as u64,
        };

        if let Err(e) = self.storage.append_history(&entry) {
            tracing::error!("coordinator: failed to persist clipboard: {}", e);
            return;
        }

        let history_size = self.storage.history_size_bytes().unwrap_or(0);
        self.emit_event(EventType::ClipboardHistoryUpdated(
            clipboard_proto::event::ClipboardHistoryUpdatedPayload {
                clipboard_id,
                current_history_size: history_size,
            },
        ));

        let target_peers: Vec<Uuid> = self
            .connected_peers
            .iter()
            .filter(|id| self.storage.is_trusted(id).unwrap_or(false))
            .copied()
            .collect();

        if target_peers.is_empty() {
            self.emit_event(EventType::SyncCompleted(
                clipboard_proto::event::SyncCompletedPayload {
                    clipboard_id,
                    successful_devices: vec![],
                    failed_devices: vec![],
                },
            ));
            return;
        }

        self.emit_event(EventType::SyncRequested(
            clipboard_proto::event::SyncRequestedPayload {
                clipboard_id,
                target_devices: target_peers.clone(),
            },
        ));

        let update_env = Envelope::build(
            MessageType::ClipboardUpdate,
            self.self_device_id,
            self.self_device_name.clone(),
            Payload::ClipboardUpdate(ClipboardUpdatePayload {
                clipboard_id,
                origin_device_id: self.self_device_id,
                content_type,
                content,
                content_hash,
                creation_timestamp: now,
            }),
        );

        let mut successful = Vec::new();
        let failed = Vec::new();

        for peer_id in &target_peers {
            let _ = self.msg_tx.send(crate::channels::MessagingCommand::Send {
                device_id: *peer_id,
                envelope: update_env.clone(),
            });
            successful.push(*peer_id);
        }

        tracing::info!(
            "coordinator: broadcast clipboard {} to {} peers",
            clipboard_id,
            successful.len()
        );

        self.emit_event(EventType::SyncCompleted(
            clipboard_proto::event::SyncCompletedPayload {
                clipboard_id,
                successful_devices: successful,
                failed_devices: failed,
            },
        ));
    }

    fn on_restore(&mut self, clipboard_id: Uuid) {
        if let Ok(Some(entry)) = self.storage.get_history_entry(&clipboard_id) {
            self.emit_event(EventType::ClipboardRestored(
                clipboard_proto::event::ClipboardRestoredPayload { clipboard_id },
            ));
            tracing::info!("coordinator: restored clipboard {}", entry.clipboard_id);
        } else {
            tracing::warn!("coordinator: history entry {} not found", clipboard_id);
        }
    }

    async fn handle_inbound(&mut self, msg: crate::channels::InboundEnvelope) {
        let env = msg.envelope;
        let device_id = msg.device_id;

        // Timestamp skew check
        let now = Utc::now();
        let diff = now.signed_duration_since(env.timestamp);
        if diff.num_seconds().abs() > TIMESTAMP_SKEW_SECS {
            tracing::warn!(
                "coordinator: message from {} outside time window (skew {}s)",
                device_id,
                diff.num_seconds()
            );
            let err_env = Envelope::build(
                MessageType::Error,
                self.self_device_id,
                self.self_device_name.clone(),
                Payload::Error(clipboard_proto::message::ErrorPayload {
                    code: ErrorCode::StaleMessage,
                    message: format!("Timestamp skew {}s exceeds limit", diff.num_seconds()),
                    related_message_id: Some(env.message_id),
                }),
            );
            let _ = self.msg_tx.send(crate::channels::MessagingCommand::Send {
                device_id,
                envelope: err_env,
            });
            return;
        }

        // Replay check
        if self.replay_cache.check_and_record(&env.message_id) {
            tracing::warn!(
                "coordinator: duplicate message {} from {}",
                env.message_id,
                device_id
            );
            let err_env = Envelope::build(
                MessageType::Error,
                self.self_device_id,
                self.self_device_name.clone(),
                Payload::Error(clipboard_proto::message::ErrorPayload {
                    code: ErrorCode::DuplicateMessage,
                    message: "Message already processed".to_string(),
                    related_message_id: Some(env.message_id),
                }),
            );
            let _ = self.msg_tx.send(crate::channels::MessagingCommand::Send {
                device_id,
                envelope: err_env,
            });
            return;
        }

        match env.message_type {
            MessageType::PairingRequest => {
                self.pairing.on_pairing_request(env);
            }
            MessageType::PairingAccept => {
                self.pairing.on_pairing_accept(env);
            }
            MessageType::PairingReject => {
                self.pairing.on_pairing_reject(env);
            }
            MessageType::ClipboardUpdate => {
                self.handle_clipboard_update(env, device_id).await;
            }
            MessageType::ClipboardAck => {
                if let Payload::ClipboardAck(ca) = env.payload {
                    match ca.status {
                        ClipboardAckStatus::Accepted => {
                            tracing::debug!(
                                "coordinator: clipboard {} accepted by {}",
                                ca.clipboard_id,
                                device_id
                            );
                        }
                        ClipboardAckStatus::Rejected { code } => {
                            tracing::warn!(
                                "coordinator: clipboard {} rejected by {}: {:?}",
                                ca.clipboard_id,
                                device_id,
                                code
                            );
                        }
                    }
                }
            }
            _ => {
                tracing::debug!(
                    "coordinator: unhandled message type {:?} from {}",
                    env.message_type,
                    device_id
                );
            }
        }
    }

    async fn handle_clipboard_update(&mut self, env: Envelope, device_id: Uuid) {
        if self.paused {
            tracing::debug!("coordinator: ignoring inbound clipboard (paused)");
            return;
        }

        let cu = match env.payload {
            Payload::ClipboardUpdate(cu) => cu,
            _ => return,
        };

        // Validate content type
        if cu.content_type != "text/plain" {
            tracing::warn!(
                "coordinator: unsupported content type '{}' from {}",
                cu.content_type,
                device_id
            );
            let ack_env = Envelope::build(
                MessageType::ClipboardAck,
                self.self_device_id,
                self.self_device_name.clone(),
                Payload::ClipboardAck(ClipboardAckPayload {
                    clipboard_id: cu.clipboard_id,
                    status: ClipboardAckStatus::Rejected {
                        code: ErrorCode::UnsupportedContentType,
                    },
                }),
            );
            let _ = self.msg_tx.send(crate::channels::MessagingCommand::Send {
                device_id,
                envelope: ack_env,
            });
            return;
        }

        // Validate size
        if cu.content.len() > self.max_clipboard_bytes {
            tracing::warn!(
                "coordinator: oversized clipboard from {} ({} bytes)",
                device_id,
                cu.content.len()
            );
            let ack_env = Envelope::build(
                MessageType::ClipboardAck,
                self.self_device_id,
                self.self_device_name.clone(),
                Payload::ClipboardAck(ClipboardAckPayload {
                    clipboard_id: cu.clipboard_id,
                    status: ClipboardAckStatus::Rejected {
                        code: ErrorCode::PayloadTooLarge,
                    },
                }),
            );
            let _ = self.msg_tx.send(crate::channels::MessagingCommand::Send {
                device_id,
                envelope: ack_env,
            });
            return;
        }

        // Content-hash dedup
        if self.replay_cache.check_and_record_hash(&cu.content_hash) {
            tracing::debug!(
                "coordinator: duplicate content hash from {} for {}",
                device_id,
                cu.clipboard_id
            );
            let reason = if cu.origin_device_id == self.self_device_id {
                clipboard_proto::event::DuplicateReason::SameOrigin
            } else {
                clipboard_proto::event::DuplicateReason::DuplicateContent
            };
            self.emit_event(EventType::DuplicateClipboardIgnored(
                clipboard_proto::event::DuplicateClipboardIgnoredPayload {
                    clipboard_id: cu.clipboard_id,
                    reason,
                },
            ));
            return;
        }

        // Persist to history
        let entry = clipboard_proto::types::HistoryEntry {
            clipboard_id: cu.clipboard_id,
            content: cu.content.clone(),
            content_type: cu.content_type.clone(),
            content_hash: cu.content_hash.clone(),
            origin_device_id: cu.origin_device_id,
            created_at: cu.creation_timestamp,
            size_bytes: cu.content.len() as u64,
        };

        if let Err(e) = self.storage.append_history(&entry) {
            tracing::error!("coordinator: failed to persist inbound clipboard: {}", e);
            let ack_env = Envelope::build(
                MessageType::ClipboardAck,
                self.self_device_id,
                self.self_device_name.clone(),
                Payload::ClipboardAck(ClipboardAckPayload {
                    clipboard_id: cu.clipboard_id,
                    status: ClipboardAckStatus::Rejected {
                        code: ErrorCode::InternalError,
                    },
                }),
            );
            let _ = self.msg_tx.send(crate::channels::MessagingCommand::Send {
                device_id,
                envelope: ack_env,
            });
            return;
        }

        // Send ACK
        let ack_env = Envelope::build(
            MessageType::ClipboardAck,
            self.self_device_id,
            self.self_device_name.clone(),
            Payload::ClipboardAck(ClipboardAckPayload {
                clipboard_id: cu.clipboard_id,
                status: ClipboardAckStatus::Accepted,
            }),
        );
        let _ = self.msg_tx.send(crate::channels::MessagingCommand::Send {
            device_id,
            envelope: ack_env,
        });

        // Emit events
        self.emit_event(EventType::ClipboardUpdatedFromRemote(
            clipboard_proto::event::ClipboardUpdatedFromRemotePayload {
                clipboard_id: cu.clipboard_id,
                origin_device_id: cu.origin_device_id,
                content: cu.content,
                content_type: cu.content_type,
                content_hash: cu.content_hash,
            },
        ));

        let history_size = self.storage.history_size_bytes().unwrap_or(0);
        self.emit_event(EventType::ClipboardHistoryUpdated(
            clipboard_proto::event::ClipboardHistoryUpdatedPayload {
                clipboard_id: cu.clipboard_id,
                current_history_size: history_size,
            },
        ));

        tracing::info!(
            "coordinator: applied clipboard {} from {}",
            cu.clipboard_id,
            device_id
        );
    }

    async fn handle_event(&mut self, event: Event) {
        match event.event_type {
            EventType::DeviceDiscovered(p) => {
                let peer = PeerInfo {
                    device_id: p.device_id,
                    device_name: p.device_name,
                    platform: clipboard_proto::types::Platform::Linux,
                    address: format!("{}:{}", p.ip_address, p.port)
                        .parse()
                        .unwrap_or_else(|_| "127.0.0.1:48272".parse().unwrap()),
                };
                self.discovered_peers.insert(p.device_id, peer);

                if self.storage.is_trusted(&p.device_id).unwrap_or(false) {
                    let peer = self.discovered_peers.get(&p.device_id).cloned().unwrap();
                    tracing::info!(
                        "coordinator: auto-connecting to trusted peer {}",
                        p.device_id
                    );
                    let _ = self
                        .msg_tx
                        .send(crate::channels::MessagingCommand::ConnectTo { peer });
                }
            }
            EventType::DeviceConnected(p) => {
                self.connected_peers.insert(p.device_id);
                self.reconnect_queue.remove(&p.device_id);
            }
            EventType::DeviceDisconnected(p) => {
                self.connected_peers.remove(&p.device_id);
                if self.storage.is_trusted(&p.device_id).unwrap_or(false)
                    && !self.reconnect_queue.contains_key(&p.device_id)
                {
                    tracing::info!(
                        "coordinator: scheduling reconnect for trusted peer {}",
                        p.device_id
                    );
                    self.reconnect_queue.insert(
                        p.device_id,
                        ReconnectEntry {
                            next_attempt: Instant::now() + RECONNECT_INITIAL,
                            current_delay: RECONNECT_INITIAL,
                        },
                    );
                }
            }
            EventType::PairingAccepted(p) => {
                tracing::info!(
                    "coordinator: pairing accepted with {}, reloading trusted peers",
                    p.device_id
                );
                let _ = self
                    .msg_tx
                    .send(crate::channels::MessagingCommand::ReloadTrusted);
            }
            _ => {}
        }
    }

    fn emit_event(&self, event_type: EventType) {
        let event = Event::new(EventSource::SyncEngine, event_type);
        let _ = self.event_tx.send(event);
    }

    fn tick_reconnect(&mut self) {
        let now = Instant::now();
        let peers_to_reconnect: Vec<(Uuid, PeerInfo)> = self
            .reconnect_queue
            .iter()
            .filter(|(_, entry)| now >= entry.next_attempt)
            .filter_map(|(device_id, _)| {
                self.discovered_peers
                    .get(device_id)
                    .cloned()
                    .map(|p| (*device_id, p))
            })
            .collect();

        for (device_id, peer) in peers_to_reconnect {
            tracing::info!(
                "coordinator: reconnecting to {} ({})",
                peer.device_name,
                device_id
            );
            let _ = self
                .msg_tx
                .send(crate::channels::MessagingCommand::ConnectTo { peer });

            if let Some(entry) = self.reconnect_queue.get_mut(&device_id) {
                entry.next_attempt = Instant::now() + entry.current_delay;
                entry.current_delay = match entry.current_delay {
                    d if d < RECONNECT_STEP_1 => RECONNECT_STEP_1,
                    d if d < RECONNECT_STEP_2 => RECONNECT_STEP_2,
                    _ => RECONNECT_CAP,
                };
            }
        }
    }
}

fn compute_content_hash(content: &str) -> String {
    let hash = sha2::Sha256::digest(content.as_bytes());
    hash.iter().map(|b| format!("{:02x}", b)).collect()
}
