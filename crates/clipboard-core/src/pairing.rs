use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::Utc;
use clipboard_proto::error::ErrorCode;
use clipboard_proto::event::{
    Event, EventSource, EventType, PairingAcceptedPayload, PairingFailedPayload,
    PairingRejectedPayload, PairingRequestedPayload,
};
use clipboard_proto::message::{
    Envelope, MessageType, PairingAcceptPayload, PairingRejectPayload, PairingRejectReason,
    PairingRequestPayload, Payload,
};
use clipboard_proto::types::{PeerInfo, TrustedPeer};
use tokio::sync::broadcast;
use uuid::Uuid;

use crate::channels::MessagingCommandTx;
use crate::storage::Storage;

#[derive(Debug, Clone)]
struct PendingRequest {
    request_id: Uuid,
    device_id: Uuid,
    device_name: String,
    received_at: Instant,
}

pub struct PairingManager {
    self_device_id: Uuid,
    self_device_name: String,
    platform: clipboard_proto::types::Platform,
    storage: Arc<Storage>,
    msg_tx: MessagingCommandTx,
    event_tx: broadcast::Sender<Event>,
    pending: HashMap<Uuid, PendingRequest>,
    timeout: Duration,
}

impl PairingManager {
    pub fn new(
        self_device_id: Uuid,
        self_device_name: String,
        platform: clipboard_proto::types::Platform,
        storage: Arc<Storage>,
        msg_tx: MessagingCommandTx,
        event_tx: broadcast::Sender<Event>,
        timeout_secs: u64,
    ) -> Self {
        Self {
            self_device_id,
            self_device_name,
            platform,
            storage,
            msg_tx,
            event_tx,
            pending: HashMap::new(),
            timeout: Duration::from_secs(timeout_secs),
        }
    }

    /// Handle an inbound PAIRING_REQUEST from a remote device.
    pub fn on_pairing_request(&mut self, envelope: Envelope) {
        let (device_id, device_name, request_id) = match &envelope.payload {
            Payload::PairingRequest(pr) => (pr.device_id, pr.device_name.clone(), pr.request_id),
            _ => return,
        };

        // Ignore if already trusted
        if self.storage.is_trusted(&device_id).unwrap_or(false) {
            tracing::info!(
                "pairing: ignoring request from already-trusted {}",
                device_id
            );
            return;
        }

        // Dedupe per device
        if self.pending.contains_key(&device_id) {
            tracing::debug!("pairing: already have pending request from {}", device_id);
            return;
        }

        self.pending.insert(
            device_id,
            PendingRequest {
                request_id,
                device_id,
                device_name: device_name.clone(),
                received_at: Instant::now(),
            },
        );

        let _ = self.event_tx.send(Event::new(
            EventSource::PairingManager,
            EventType::PairingRequested(PairingRequestedPayload {
                request_id,
                device_id,
                device_name,
                platform: self.platform,
            }),
        ));
    }

    /// Handle an inbound PAIRING_ACCEPT from a remote device.
    pub fn on_pairing_accept(&mut self, envelope: Envelope) {
        let (request_id, device_id, device_name) = match &envelope.payload {
            Payload::PairingAccept(pa) => (pa.request_id, pa.device_id, pa.device_name.clone()),
            _ => return,
        };

        self.pending.remove(&device_id);

        let peer = TrustedPeer {
            device_id,
            device_name,
            platform: Some(self.platform),
            paired_at: Utc::now(),
            last_seen: Some(Utc::now()),
        };

        if let Err(e) = self.storage.add_trusted_peer(&peer) {
            tracing::error!(
                "pairing: failed to persist trusted peer {}: {}",
                device_id,
                e
            );
            let _ = self.event_tx.send(Event::new(
                EventSource::PairingManager,
                EventType::PairingFailed(PairingFailedPayload {
                    request_id,
                    error: e.to_string(),
                }),
            ));
            return;
        }

        let _ = self.event_tx.send(Event::new(
            EventSource::PairingManager,
            EventType::PairingAccepted(PairingAcceptedPayload {
                request_id,
                device_id,
            }),
        ));
    }

    /// Handle an inbound PAIRING_REJECT from a remote device.
    pub fn on_pairing_reject(&mut self, envelope: Envelope) {
        let (request_id, device_id) = match &envelope.payload {
            Payload::PairingReject(pr) => {
                let device_id = self
                    .pending
                    .values()
                    .find(|p| p.request_id == pr.request_id)
                    .map(|p| p.device_id);
                (pr.request_id, device_id)
            }
            _ => return,
        };

        if let Some(device_id) = device_id {
            self.pending.remove(&device_id);
        }

        let _ = self.event_tx.send(Event::new(
            EventSource::PairingManager,
            EventType::PairingRejected(PairingRejectedPayload {
                request_id,
                device_id: device_id.unwrap_or_default(),
                reason: clipboard_proto::event::PairingRejectReason::UserDenied,
            }),
        ));
    }

    /// User-initiated pairing request (user taps "Pair").
    pub fn request_pairing(&mut self, device_id: &Uuid, peer: &PeerInfo) {
        let request_id = Uuid::new_v4();

        let env = Envelope::build(
            MessageType::PairingRequest,
            self.self_device_id,
            self.self_device_name.clone(),
            Payload::PairingRequest(PairingRequestPayload {
                device_id: self.self_device_id,
                device_name: self.self_device_name.clone(),
                platform: self.platform,
                request_id,
            }),
        );

        let _ = self
            .msg_tx
            .send(crate::channels::MessagingCommand::ConnectTo { peer: peer.clone() });

        let _ = self.msg_tx.send(crate::channels::MessagingCommand::Send {
            device_id: *device_id,
            envelope: env,
        });

        tracing::info!("pairing: sent pairing request to {}", device_id);
    }

    /// User approves a pending pairing request.
    pub fn approve(&mut self, device_id: &Uuid) {
        let pending = match self.pending.remove(device_id) {
            Some(p) => p,
            None => {
                tracing::warn!("pairing: no pending request for {}", device_id);
                return;
            }
        };

        let env = Envelope::build(
            MessageType::PairingAccept,
            self.self_device_id,
            self.self_device_name.clone(),
            Payload::PairingAccept(PairingAcceptPayload {
                request_id: pending.request_id,
                device_id: self.self_device_id,
                device_name: self.self_device_name.clone(),
                paired_at: Utc::now(),
            }),
        );

        let _ = self.msg_tx.send(crate::channels::MessagingCommand::Send {
            device_id: *device_id,
            envelope: env,
        });

        let peer = TrustedPeer {
            device_id: *device_id,
            device_name: pending.device_name,
            platform: Some(self.platform),
            paired_at: Utc::now(),
            last_seen: Some(Utc::now()),
        };

        if let Err(e) = self.storage.add_trusted_peer(&peer) {
            tracing::error!(
                "pairing: failed to persist trusted peer {}: {}",
                device_id,
                e
            );
            let _ = self.event_tx.send(Event::new(
                EventSource::PairingManager,
                EventType::PairingFailed(PairingFailedPayload {
                    request_id: pending.request_id,
                    error: e.to_string(),
                }),
            ));
            return;
        }

        let _ = self.event_tx.send(Event::new(
            EventSource::PairingManager,
            EventType::PairingAccepted(PairingAcceptedPayload {
                request_id: pending.request_id,
                device_id: *device_id,
            }),
        ));

        tracing::info!("pairing: approved {}", device_id);
    }

    /// User rejects a pending pairing request.
    pub fn reject(&mut self, device_id: &Uuid, reason: &str) {
        let pending = match self.pending.remove(device_id) {
            Some(p) => p,
            None => {
                tracing::warn!("pairing: no pending request for {}", device_id);
                return;
            }
        };

        let env = Envelope::build(
            MessageType::PairingReject,
            self.self_device_id,
            self.self_device_name.clone(),
            Payload::PairingReject(PairingRejectPayload {
                request_id: pending.request_id,
                reason: PairingRejectReason::UserDenied,
            }),
        );

        let _ = self.msg_tx.send(crate::channels::MessagingCommand::Send {
            device_id: *device_id,
            envelope: env,
        });

        let _ = self.event_tx.send(Event::new(
            EventSource::PairingManager,
            EventType::PairingRejected(PairingRejectedPayload {
                request_id: pending.request_id,
                device_id: *device_id,
                reason: clipboard_proto::event::PairingRejectReason::UserDenied,
            }),
        ));

        tracing::info!("pairing: rejected {} ({})", device_id, reason);
    }

    /// Periodic tick to expire pending requests.
    pub fn tick(&mut self) {
        let now = Instant::now();
        let expired: Vec<Uuid> = self
            .pending
            .iter()
            .filter(|(_, req)| now.duration_since(req.received_at) >= self.timeout)
            .map(|(id, _)| *id)
            .collect();

        for device_id in expired {
            if let Some(req) = self.pending.remove(&device_id) {
                tracing::info!("pairing: request from {} timed out", device_id);

                let err_env = Envelope::build(
                    MessageType::Error,
                    self.self_device_id,
                    self.self_device_name.clone(),
                    Payload::Error(clipboard_proto::message::ErrorPayload {
                        code: ErrorCode::PairingTimeout,
                        message: "Pairing request timed out".to_string(),
                        related_message_id: Some(req.request_id),
                    }),
                );

                let _ = self.msg_tx.send(crate::channels::MessagingCommand::Send {
                    device_id,
                    envelope: err_env,
                });

                let _ = self.event_tx.send(Event::new(
                    EventSource::PairingManager,
                    EventType::PairingRejected(PairingRejectedPayload {
                        request_id: req.request_id,
                        device_id,
                        reason: clipboard_proto::event::PairingRejectReason::Timeout,
                    }),
                ));
            }
        }
    }

    pub fn pending_requests(&self) -> Vec<clipboard_proto::types::PairingRequest> {
        self.pending
            .values()
            .map(|req| clipboard_proto::types::PairingRequest {
                request_id: req.request_id,
                device_id: req.device_id,
                device_name: req.device_name.clone(),
                platform: self.platform,
                received_at: Utc::now(),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_pairing() -> PairingManager {
        let storage = Arc::new(
            Storage::open(&crate::config::StorageConfig {
                path: std::path::PathBuf::from(":memory:"),
            })
            .unwrap(),
        );
        let (msg_tx, _) = tokio::sync::mpsc::unbounded_channel();
        let (event_tx, _) = broadcast::channel(64);
        PairingManager::new(
            Uuid::new_v4(),
            "test".to_string(),
            clipboard_proto::types::Platform::Linux,
            storage,
            msg_tx,
            event_tx,
            60,
        )
    }

    #[test]
    fn request_pairing_sends_envelope() {
        let mut pm = make_test_pairing();
        let peer = PeerInfo {
            device_id: Uuid::new_v4(),
            device_name: "peer".to_string(),
            platform: clipboard_proto::types::Platform::Linux,
            address: "127.0.0.1:48272".parse().unwrap(),
        };
        pm.request_pairing(&peer.device_id, &peer);
    }

    #[test]
    fn approve_with_no_pending_does_nothing() {
        let mut pm = make_test_pairing();
        pm.approve(&Uuid::new_v4());
    }

    #[test]
    fn reject_with_no_pending_does_nothing() {
        let mut pm = make_test_pairing();
        pm.reject(&Uuid::new_v4(), "test");
    }
}
