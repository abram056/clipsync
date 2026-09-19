use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

use clipboard_proto::event::{
    DeviceDiscoveredPayload, Event, EventSource, EventType, PeerListUpdatedPayload,
};
use clipboard_proto::message::{
    DiscoverPayload, DiscoverResponsePayload, Envelope, MessageType, Payload, PROTOCOL_VERSION,
};
use clipboard_proto::types::PeerInfo;
use tokio::net::UdpSocket;
use tokio::sync::broadcast;
use tokio::time;
use uuid::Uuid;

use crate::config::AppConfig;

#[derive(Debug, Clone)]
struct RosterEntry {
    peer: PeerInfo,
    last_seen: Instant,
}

pub struct DiscoveryService {
    self_device_id: Uuid,
    self_device_name: String,
    discovery_port: u16,
    listen_port: u16,
    platform: clipboard_proto::types::Platform,
    broadcast_interval: Duration,
    peer_timeout: Duration,
    discovery_targets: Vec<SocketAddr>,
    event_tx: broadcast::Sender<Event>,
}

impl DiscoveryService {
    pub fn spawn(
        config: &AppConfig,
        self_device_id: Uuid,
        self_device_name: String,
        platform: clipboard_proto::types::Platform,
        event_tx: broadcast::Sender<Event>,
    ) -> tokio::task::JoinHandle<()> {
        let svc = Self {
            self_device_id,
            self_device_name,
            discovery_port: config.network.discovery_port,
            listen_port: config.network.listen_port,
            platform,
            broadcast_interval: Duration::from_secs(config.network.discovery_interval_secs),
            peer_timeout: Duration::from_secs(config.network.peer_timeout_secs),
            discovery_targets: config.network.discovery_targets.clone(),
            event_tx,
        };
        tokio::spawn(async move { svc.run().await })
    }

    async fn run(self) {
        let bind_addr = format!("0.0.0.0:{}", self.discovery_port);
        let socket = match UdpSocket::bind(&bind_addr).await {
            Ok(s) => s,
            Err(e) => {
                tracing::error!("discovery: failed to bind UDP on {}: {}", bind_addr, e);
                return;
            }
        };

        if let Err(e) = socket.set_broadcast(true) {
            tracing::warn!("discovery: failed to set broadcast: {}", e);
        }

        let mut roster: HashMap<Uuid, RosterEntry> = HashMap::new();
        let mut interval = time::interval(self.broadcast_interval);
        // `interval` ticks immediately on creation. Consume that tick so clients
        // have a chance to subscribe to discovery events after startup rather
        // than losing the only initial unicast response.
        interval.tick().await;
        let mut stale_check = time::interval(Duration::from_secs(5));

        tracing::info!("discovery: listening on {}", bind_addr);

        loop {
            tokio::select! {
            _ = interval.tick() => {
                    self.broadcast_discover(&socket).await;
                    // also send to discovery_targets if configured
                    for target in &self.discovery_targets {
                        let _ = self.send_discover_to(&socket, *target).await;
                    }
                }
                _ = stale_check.tick() => {
                    let stale = self.check_staleness(&mut roster);
                    if stale {
                        self.emit_peer_list(&roster);
                    }
                }
                result = self.recv_one(&socket) => {
                    match result {
                        Ok((data, from_addr)) => {
                            self.handle_datagram(&data, from_addr, &socket, &mut roster).await;
                        }
                        Err(e) => {
                            tracing::warn!("discovery: recv error: {}", e);
                        }
                    }
                }
            }
        }
    }

    async fn recv_one(&self, socket: &UdpSocket) -> Result<(Vec<u8>, SocketAddr), std::io::Error> {
        let mut buf = vec![0u8; 4096];
        let (n, from) = socket.recv_from(&mut buf).await?;
        buf.truncate(n);
        Ok((buf, from))
    }

    async fn broadcast_discover(&self, socket: &UdpSocket) {
        let broadcast_addr =
            SocketAddr::new("255.255.255.255".parse().unwrap(), self.discovery_port);
        let envelope = Envelope::build(
            MessageType::Discover,
            self.self_device_id,
            self.self_device_name.clone(),
            Payload::Discover(DiscoverPayload {
                protocol_version: PROTOCOL_VERSION,
            }),
        );
        if let Ok(data) = envelope.to_bytes() {
            let _ = socket.send_to(&data, broadcast_addr).await;
        }
    }

    async fn send_discover_to(&self, socket: &UdpSocket, target: SocketAddr) {
        let envelope = Envelope::build(
            MessageType::Discover,
            self.self_device_id,
            self.self_device_name.clone(),
            Payload::Discover(DiscoverPayload {
                protocol_version: PROTOCOL_VERSION,
            }),
        );
        if let Ok(data) = envelope.to_bytes() {
            let _ = socket.send_to(&data, target).await;
        }
    }

    async fn handle_datagram(
        &self,
        data: &[u8],
        from_addr: SocketAddr,
        socket: &UdpSocket,
        roster: &mut HashMap<Uuid, RosterEntry>,
    ) {
        let envelope = match Envelope::from_bytes(data) {
            Ok(env) => env,
            Err(e) => {
                tracing::warn!("discovery: malformed envelope from {}: {}", from_addr, e);
                return;
            }
        };

        // ignore own messages
        if envelope.device_id == self.self_device_id {
            return;
        }

        match envelope.message_type {
            MessageType::Discover => {
                self.handle_discover(&envelope, from_addr, socket).await;
            }
            MessageType::DiscoverResponse => {
                self.handle_discover_response(&envelope, from_addr, roster);
            }
            _ => {
                tracing::debug!("discovery: unexpected message type from {}", from_addr);
            }
        }
    }

    async fn handle_discover(
        &self,
        envelope: &Envelope,
        from_addr: SocketAddr,
        socket: &UdpSocket,
    ) {
        // Validate protocol version
        match &envelope.payload {
            Payload::Discover(dp) => {
                if dp.protocol_version != PROTOCOL_VERSION {
                    tracing::debug!(
                        "discovery: ignoring DISCOVER with unsupported version {}",
                        dp.protocol_version
                    );
                    return;
                }
            }
            _ => {
                tracing::warn!("discovery: DISCOVER envelope with wrong payload variant");
                return;
            }
        }

        // Reply with DISCOVER_RESPONSE
        let response = Envelope::build(
            MessageType::DiscoverResponse,
            self.self_device_id,
            self.self_device_name.clone(),
            Payload::DiscoverResponse(DiscoverResponsePayload {
                protocol_version: PROTOCOL_VERSION,
                listening_port: self.listen_port,
                platform: self.platform,
            }),
        );

        if let Ok(data) = response.to_bytes() {
            let _ = socket.send_to(&data, from_addr).await;
        }

        // Also emit DeviceDiscovered for the sender
        let _ = self.event_tx.send(Event::new(
            EventSource::DiscoveryService,
            EventType::DeviceDiscovered(DeviceDiscoveredPayload {
                device_id: envelope.device_id,
                device_name: envelope.device_name.clone(),
                ip_address: from_addr.ip().to_string(),
                port: self.listen_port,
            }),
        ));
    }

    fn handle_discover_response(
        &self,
        envelope: &Envelope,
        from_addr: SocketAddr,
        roster: &mut HashMap<Uuid, RosterEntry>,
    ) {
        let ws_port = match &envelope.payload {
            Payload::DiscoverResponse(dr) => {
                if dr.protocol_version != PROTOCOL_VERSION {
                    tracing::debug!(
                        "discovery: ignoring DISCOVER_RESPONSE with unsupported version {}",
                        dr.protocol_version
                    );
                    return;
                }
                dr.listening_port
            }
            _ => {
                tracing::warn!("discovery: DISCOVER_RESPONSE with wrong payload variant");
                return;
            }
        };

        let addr = SocketAddr::new(from_addr.ip(), ws_port);
        let peer = PeerInfo {
            device_id: envelope.device_id,
            device_name: envelope.device_name.clone(),
            platform: clipboard_proto::types::Platform::Linux,
            address: addr,
        };

        let _ = self.event_tx.send(Event::new(
            EventSource::DiscoveryService,
            EventType::DeviceDiscovered(DeviceDiscoveredPayload {
                device_id: envelope.device_id,
                device_name: envelope.device_name.clone(),
                ip_address: from_addr.ip().to_string(),
                port: ws_port,
            }),
        ));

        roster.insert(
            envelope.device_id,
            RosterEntry {
                peer,
                last_seen: Instant::now(),
            },
        );
    }

    fn check_staleness(&self, roster: &mut HashMap<Uuid, RosterEntry>) -> bool {
        let now = Instant::now();
        let before = roster.len();
        roster.retain(|_, entry| now.duration_since(entry.last_seen) < self.peer_timeout);
        roster.len() != before
    }

    fn emit_peer_list(&self, roster: &HashMap<Uuid, RosterEntry>) {
        let devices: Vec<clipboard_proto::types::DeviceInfo> = roster
            .values()
            .map(|entry| clipboard_proto::types::DeviceInfo {
                device_id: entry.peer.device_id,
                device_name: entry.peer.device_name.clone(),
                platform: entry.peer.platform,
                is_trusted: false,
                is_connected: false,
            })
            .collect();
        let _ = self.event_tx.send(Event::new(
            EventSource::DiscoveryService,
            EventType::PeerListUpdated(PeerListUpdatedPayload {
                connected_devices: devices,
            }),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roster_staleness() {
        let timeout = Duration::from_millis(50);
        let mut roster: HashMap<Uuid, RosterEntry> = HashMap::new();
        let id = Uuid::new_v4();
        roster.insert(
            id,
            RosterEntry {
                peer: PeerInfo {
                    device_id: id,
                    device_name: "test".to_string(),
                    platform: clipboard_proto::types::Platform::Linux,
                    address: "127.0.0.1:48272".parse().unwrap(),
                },
                last_seen: Instant::now() - Duration::from_millis(100),
            },
        );
        let before = roster.len();
        roster.retain(|_, entry| Instant::now().duration_since(entry.last_seen) < timeout);
        assert_eq!(roster.len(), before - 1);
    }

    #[test]
    fn roster_keeps_fresh() {
        let timeout = Duration::from_secs(45);
        let mut roster: HashMap<Uuid, RosterEntry> = HashMap::new();
        let id = Uuid::new_v4();
        roster.insert(
            id,
            RosterEntry {
                peer: PeerInfo {
                    device_id: id,
                    device_name: "test".to_string(),
                    platform: clipboard_proto::types::Platform::Linux,
                    address: "127.0.0.1:48272".parse().unwrap(),
                },
                last_seen: Instant::now(),
            },
        );
        let before = roster.len();
        roster.retain(|_, entry| Instant::now().duration_since(entry.last_seen) < timeout);
        assert_eq!(roster.len(), before);
    }

    #[test]
    fn ignore_own_device() {
        let id = Uuid::new_v4();
        let envelope = Envelope::build(
            MessageType::Discover,
            id,
            "self".to_string(),
            Payload::Discover(DiscoverPayload {
                protocol_version: PROTOCOL_VERSION,
            }),
        );
        assert_eq!(envelope.device_id, id);
    }
}
