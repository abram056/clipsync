use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use clipboard_proto::error::ErrorCode;
use clipboard_proto::event::{Event, EventType};
use clipboard_proto::message::{Envelope, MessageType, Payload};
use clipboard_proto::types::PeerInfo;
use tokio::sync::broadcast;
use tokio::time;
use uuid::Uuid;

use crate::channels::{AppCommandRx, InboundEnvelopeRx, MessagingCommandTx};
use crate::config::AppConfig;
use crate::pairing::PairingManager;
use crate::replay_cache::ReplayCache;
use crate::storage::Storage;

const TIMESTAMP_SKEW_SECS: i64 = 300;

pub struct Coordinator {
    self_device_id: Uuid,
    self_device_name: String,
    storage: Arc<Storage>,
    msg_tx: MessagingCommandTx,
    #[allow(dead_code)]
    event_tx: broadcast::Sender<Event>,
    pairing: PairingManager,
    replay_cache: ReplayCache,
    discovered_peers: std::collections::HashMap<Uuid, PeerInfo>,
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

        let mut coordinator = Self {
            self_device_id,
            self_device_name,
            storage,
            msg_tx,
            event_tx: event_tx.clone(),
            pairing,
            replay_cache,
            discovered_peers: std::collections::HashMap::new(),
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
                            }
                            Some(crate::channels::AppCommand::RejectPairing { device_id, reason }) => {
                                coordinator.pairing.reject(&device_id, &reason);
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
                    }
                    _ = prune_interval.tick() => {
                        coordinator.replay_cache.prune();
                    }
                }
            }
        })
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
                tracing::debug!("coordinator: clipboard update from {} (Phase C)", device_id);
            }
            MessageType::ClipboardAck => {
                tracing::debug!("coordinator: clipboard ack from {} (Phase C)", device_id);
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
            EventType::DeviceDisconnected(p) => {
                tracing::debug!("coordinator: device {} disconnected", p.device_id);
            }
            _ => {}
        }
    }
}
