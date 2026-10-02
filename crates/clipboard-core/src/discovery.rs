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

        // No DeviceDiscovered here: a DISCOVER carries neither the requester's
        // listen port nor its platform, so the only thing this side could
        // report is its own values. The requester learns about us from the
        // response above; we learn about the requester when they answer one of
        // our DISCOVER broadcasts.
        tracing::debug!("discovery: answered DISCOVER from {}", from_addr);
    }

    /// Build the peer record described by a `DISCOVER_RESPONSE`.
    ///
    /// The responder is the only party that knows its own WebSocket listen
    /// port, so this is the authoritative source for a peer's address and
    /// platform.
    fn peer_from_response(
        &self,
        envelope: &Envelope,
        from_addr: SocketAddr,
    ) -> Option<(PeerInfo, DeviceDiscoveredPayload)> {
        let response = match &envelope.payload {
            Payload::DiscoverResponse(response) => response,
            _ => {
                tracing::warn!("discovery: DISCOVER_RESPONSE with wrong payload variant");
                return None;
            }
        };

        if response.protocol_version != PROTOCOL_VERSION {
            tracing::debug!(
                "discovery: ignoring DISCOVER_RESPONSE with unsupported version {}",
                response.protocol_version
            );
            return None;
        }

        let address = SocketAddr::new(from_addr.ip(), response.listening_port);

        let peer = PeerInfo {
            device_id: envelope.device_id,
            device_name: envelope.device_name.clone(),
            platform: response.platform,
            address,
        };

        let payload = DeviceDiscoveredPayload {
            device_id: envelope.device_id,
            device_name: envelope.device_name.clone(),
            platform: response.platform,
            ip_address: from_addr.ip().to_string(),
            port: response.listening_port,
        };

        Some((peer, payload))
    }

    fn handle_discover_response(
        &self,
        envelope: &Envelope,
        from_addr: SocketAddr,
        roster: &mut HashMap<Uuid, RosterEntry>,
    ) {
        let Some((peer, payload)) = self.peer_from_response(envelope, from_addr) else {
            return;
        };

        let _ = self.event_tx.send(Event::new(
            EventSource::DiscoveryService,
            EventType::DeviceDiscovered(payload),
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

    /// Drives the real staleness check: an entry older than the configured
    /// peer timeout must be dropped from the roster.
    #[test]
    fn roster_staleness() {
        let mut service = unspawned_service();
        service.peer_timeout = Duration::from_millis(50);
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

        assert!(
            service.check_staleness(&mut roster),
            "removing an entry must be reported"
        );
        assert!(
            roster.is_empty(),
            "the stale entry must be dropped from the roster"
        );
    }

    /// An entry seen within the timeout must survive the check.
    #[test]
    fn roster_keeps_fresh() {
        let service = unspawned_service();
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

        assert!(
            !service.check_staleness(&mut roster),
            "a fresh entry must not be reported as removed"
        );
        assert_eq!(roster.len(), 1, "the fresh entry must be kept");
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

    /// A service that is never spawned, so the helpers under test can be
    /// exercised without binding a UDP port.
    fn unspawned_service() -> DiscoveryService {
        let (event_tx, _event_rx) = broadcast::channel(8);
        DiscoveryService {
            self_device_id: Uuid::new_v4(),
            self_device_name: "self".to_string(),
            discovery_port: 48271,
            listen_port: 48272,
            platform: clipboard_proto::types::Platform::Linux,
            broadcast_interval: Duration::from_secs(3600),
            peer_timeout: Duration::from_secs(45),
            discovery_targets: Vec::new(),
            event_tx,
        }
    }

    fn discover_response(
        platform: clipboard_proto::types::Platform,
        protocol_version: u32,
        listening_port: u16,
    ) -> Envelope {
        Envelope::build(
            MessageType::DiscoverResponse,
            Uuid::new_v4(),
            "peer".to_string(),
            Payload::DiscoverResponse(DiscoverResponsePayload {
                protocol_version,
                listening_port,
                platform,
            }),
        )
    }

    #[test]
    fn payload_from_response_uses_peer_port_and_platform() {
        let svc = unspawned_service();
        let from_addr: SocketAddr = "192.168.1.50:48271".parse().unwrap();

        for platform in [
            clipboard_proto::types::Platform::Linux,
            clipboard_proto::types::Platform::Android,
            clipboard_proto::types::Platform::Windows,
            clipboard_proto::types::Platform::MacOS,
        ] {
            let envelope = discover_response(platform, PROTOCOL_VERSION, 49001);
            let (peer, payload) = svc
                .peer_from_response(&envelope, from_addr)
                .expect("supported DISCOVER_RESPONSE should describe a peer");

            assert_eq!(payload.port, 49001, "port must come from the response");
            assert_eq!(payload.ip_address, "192.168.1.50");
            assert_eq!(payload.platform, platform);
            assert_eq!(payload.device_id, envelope.device_id);
            assert_eq!(payload.device_name, "peer");
            assert_eq!(
                peer.address,
                "192.168.1.50:49001".parse::<SocketAddr>().unwrap(),
                "peer address must combine the source ip with the advertised port"
            );
            assert_eq!(peer.platform, platform);
        }
    }

    #[test]
    fn response_with_unsupported_version_is_ignored() {
        let svc = unspawned_service();
        let from_addr: SocketAddr = "192.168.1.50:48271".parse().unwrap();
        let envelope = discover_response(
            clipboard_proto::types::Platform::Linux,
            PROTOCOL_VERSION + 1,
            49001,
        );
        assert!(svc.peer_from_response(&envelope, from_addr).is_none());
    }

    #[test]
    fn response_with_wrong_payload_variant_is_ignored() {
        let svc = unspawned_service();
        let from_addr: SocketAddr = "192.168.1.50:48271".parse().unwrap();
        let envelope = Envelope::build(
            MessageType::DiscoverResponse,
            Uuid::new_v4(),
            "peer".to_string(),
            Payload::Discover(DiscoverPayload {
                protocol_version: PROTOCOL_VERSION,
            }),
        );
        assert!(svc.peer_from_response(&envelope, from_addr).is_none());
    }

    /// An ephemeral port outside the range the integration tests reserve for
    /// themselves (phase_c 60191+, phase_d 61191+, protocol_validation
    /// 62300+, each plus `pid % 1000`), so a concurrently running test
    /// binary cannot collide.
    fn free_udp_port() -> u16 {
        for _ in 0..20 {
            let socket = std::net::UdpSocket::bind("127.0.0.1:0").expect("bind probe socket");
            let port = socket.local_addr().expect("probe address").port();
            if !(60000..=64000).contains(&port) {
                return port;
            }
        }
        panic!("could not find a free discovery port");
    }

    #[tokio::test]
    async fn discover_datagram_is_answered_without_emitting_device_discovered() {
        let discovery_port = free_udp_port();

        let mut config = AppConfig::default();
        config.network.discovery_port = discovery_port;
        config.network.discovery_interval_secs = 3600;

        let self_device_id = Uuid::new_v4();
        let (event_tx, mut event_rx) = broadcast::channel(64);
        let handle = DiscoveryService::spawn(
            &config,
            self_device_id,
            "self".to_string(),
            clipboard_proto::types::Platform::Linux,
            event_tx,
        );

        let socket = UdpSocket::bind("127.0.0.1:0")
            .await
            .expect("bind test socket");
        let discover = Envelope::build(
            MessageType::Discover,
            Uuid::new_v4(),
            "peer".to_string(),
            Payload::Discover(DiscoverPayload {
                protocol_version: PROTOCOL_VERSION,
            }),
        );
        let data = discover.to_bytes().expect("serialize DISCOVER");

        // The service binds asynchronously, so keep offering DISCOVER until it
        // answers rather than sending once into a race.
        let mut buf = vec![0u8; 4096];
        let mut response: Option<Envelope> = None;
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        while response.is_none() {
            let _ = socket.send_to(&data, ("127.0.0.1", discovery_port)).await;
            match tokio::time::timeout(Duration::from_millis(300), socket.recv_from(&mut buf)).await
            {
                Ok(Ok((n, _))) => response = Envelope::from_bytes(&buf[..n]).ok(),
                _ => assert!(
                    std::time::Instant::now() < deadline,
                    "DISCOVER went unanswered on 127.0.0.1:{}",
                    discovery_port
                ),
            }
        }

        let response = response.expect("malformed DISCOVER_RESPONSE");
        assert_eq!(response.message_type, MessageType::DiscoverResponse);
        assert_eq!(
            response.device_id, self_device_id,
            "response must be attributed to the answering service"
        );
        match response.payload {
            Payload::DiscoverResponse(dr) => {
                assert_eq!(dr.protocol_version, PROTOCOL_VERSION);
                assert_eq!(dr.listening_port, config.network.listen_port);
                assert_eq!(dr.platform, clipboard_proto::types::Platform::Linux);
            }
            other => panic!("expected DISCOVER_RESPONSE, got {:?}", other),
        }

        // Give the service a chance to emit anything it was going to emit.
        tokio::time::sleep(Duration::from_millis(200)).await;
        while let Ok(event) = event_rx.try_recv() {
            assert!(
                !matches!(event.event_type, EventType::DeviceDiscovered(_)),
                "answering a DISCOVER must not report a peer"
            );
        }

        handle.abort();
    }
}
