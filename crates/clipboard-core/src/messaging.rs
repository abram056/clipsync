use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use clipboard_proto::error::ErrorCode;
use clipboard_proto::event::{
    ConnectionType, DeviceConnectedPayload, DeviceConnectionFailedPayload,
    DeviceDisconnectedPayload, DisconnectReason, Event, EventSource, EventType,
};
use clipboard_proto::message::{
    current_platform, Envelope, HelloAckPayload, HelloPayload, MessageType, Payload,
    MAX_CLIPBOARD_SIZE, PROTOCOL_VERSION,
};
use clipboard_proto::types::PeerInfo;
use futures_util::{SinkExt, StreamExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{broadcast, Mutex};
use tokio::time;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::{
    accept_async_with_config, connect_async_with_config, MaybeTlsStream, WebSocketStream,
};
use uuid::Uuid;

use crate::channels::{InboundEnvelopeTx, MessagingCommand, MessagingCommandRx};
use crate::config::AppConfig;
use crate::storage::Storage;

const MAX_FRAME_SIZE: usize = MAX_CLIPBOARD_SIZE + 64 * 1024;

type WsSink = futures_util::stream::SplitSink<
    WebSocketStream<MaybeTlsStream<TcpStream>>,
    tokio_tungstenite::tungstenite::Message,
>;
type WsStream = futures_util::stream::SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>;

#[derive(Debug, Clone, PartialEq, Eq)]
enum SessionState {
    Pairing,
    Trusted,
}

struct ConnectionHandle {
    #[allow(dead_code)]
    device_id: Uuid,
    sink: Arc<Mutex<WsSink>>,
    session_state: Arc<Mutex<SessionState>>,
}

type OutboxMap = Arc<Mutex<HashMap<Uuid, Vec<Envelope>>>>;

fn ws_config() -> WebSocketConfig {
    #[allow(deprecated)]
    WebSocketConfig {
        max_send_queue: None,
        write_buffer_size: 128 * 1024,
        max_write_buffer_size: usize::MAX,
        max_message_size: Some(MAX_FRAME_SIZE),
        max_frame_size: Some(MAX_FRAME_SIZE),
        accept_unmasked_frames: false,
    }
}

pub struct MessagingService {
    self_device_id: Uuid,
    self_device_name: String,
    listen_port: u16,
    platform: clipboard_proto::types::Platform,
    max_peers: usize,
    event_tx: broadcast::Sender<Event>,
    inbound_tx: InboundEnvelopeTx,
    trusted_devices: Arc<Mutex<HashMap<Uuid, PeerInfo>>>,
    active_connections: Arc<Mutex<HashMap<Uuid, ConnectionHandle>>>,
    outbox: OutboxMap,
}

impl MessagingService {
    #[allow(clippy::too_many_arguments)]
    pub fn spawn(
        config: &AppConfig,
        self_device_id: Uuid,
        self_device_name: String,
        platform: clipboard_proto::types::Platform,
        storage: Arc<Storage>,
        event_tx: broadcast::Sender<Event>,
        inbound_tx: InboundEnvelopeTx,
        msg_rx: MessagingCommandRx,
    ) -> tokio::task::JoinHandle<()> {
        let max_peers = config.sync.max_peers;
        let listen_port = config.network.listen_port;

        let svc = Self {
            self_device_id,
            self_device_name,
            listen_port,
            platform,
            max_peers,
            event_tx,
            inbound_tx,
            trusted_devices: Arc::new(Mutex::new(HashMap::new())),
            active_connections: Arc::new(Mutex::new(HashMap::new())),
            outbox: Arc::new(Mutex::new(HashMap::new())),
        };

        tokio::spawn(async move {
            svc.run(storage, msg_rx).await;
        })
    }

    async fn run(self, storage: Arc<Storage>, msg_rx: MessagingCommandRx) {
        {
            let mut trusted = self.trusted_devices.lock().await;
            match storage.trusted_peers() {
                Ok(peers) => {
                    for p in peers {
                        trusted.insert(
                            p.device_id,
                            PeerInfo {
                                device_id: p.device_id,
                                device_name: p.device_name,
                                platform: p.platform.unwrap_or_else(current_platform),
                                address: "0.0.0.0:0".parse().unwrap(),
                            },
                        );
                    }
                }
                Err(error) => tracing::error!("messaging: failed to load trusted peers: {}", error),
            }
        }

        let listener = match TcpListener::bind(format!("0.0.0.0:{}", self.listen_port)).await {
            Ok(l) => l,
            Err(e) => {
                tracing::error!("messaging: failed to bind TCP {}: {}", self.listen_port, e);
                return;
            }
        };

        tracing::info!(
            "messaging: WS server listening on port {}",
            self.listen_port
        );

        let server_handle = {
            let inbound_tx = self.inbound_tx.clone();
            let event_tx = self.event_tx.clone();
            let trusted = self.trusted_devices.clone();
            let active = self.active_connections.clone();
            let self_id = self.self_device_id;
            let self_name = self.self_device_name.clone();
            let platform = self.platform;
            let max_peers = self.max_peers;

            tokio::spawn(async move {
                Self::server_loop(
                    listener, self_id, self_name, platform, inbound_tx, event_tx, trusted, active,
                    max_peers,
                )
                .await;
            })
        };

        let cmd_handle = {
            let inbound_tx = self.inbound_tx.clone();
            let event_tx = self.event_tx.clone();
            let trusted = self.trusted_devices.clone();
            let active = self.active_connections.clone();
            let outbox = self.outbox.clone();
            let self_id = self.self_device_id;
            let self_name = self.self_device_name.clone();
            let platform = self.platform;
            let storage = storage.clone();

            tokio::spawn(async move {
                Self::command_loop(
                    msg_rx, self_id, self_name, platform, inbound_tx, event_tx, trusted, active,
                    outbox, storage,
                )
                .await;
            })
        };

        tokio::select! {
            _ = server_handle => {},
            _ = cmd_handle => {},
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn server_loop(
        listener: TcpListener,
        self_device_id: Uuid,
        self_device_name: String,
        platform: clipboard_proto::types::Platform,
        inbound_tx: InboundEnvelopeTx,
        event_tx: broadcast::Sender<Event>,
        trusted: Arc<Mutex<HashMap<Uuid, PeerInfo>>>,
        active: Arc<Mutex<HashMap<Uuid, ConnectionHandle>>>,
        max_peers: usize,
    ) {
        loop {
            let (stream, addr) = match listener.accept().await {
                Ok(v) => v,
                Err(e) => {
                    tracing::warn!("messaging: accept error: {}", e);
                    continue;
                }
            };

            let conn_count = active.lock().await.len();
            if conn_count >= max_peers {
                tracing::warn!(
                    "messaging: max peers reached, rejecting connection from {}",
                    addr
                );
                drop(stream);
                continue;
            }

            let inbound_tx = inbound_tx.clone();
            let event_tx = event_tx.clone();
            let trusted = trusted.clone();
            let active = active.clone();
            let self_id = self_device_id;
            let self_name = self_device_name.clone();

            tokio::spawn(async move {
                Self::handle_server_connection(
                    stream, addr, self_id, self_name, platform, inbound_tx, event_tx, trusted,
                    active,
                )
                .await;
            });
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn handle_server_connection(
        stream: TcpStream,
        addr: SocketAddr,
        self_device_id: Uuid,
        self_device_name: String,
        _platform: clipboard_proto::types::Platform,
        inbound_tx: InboundEnvelopeTx,
        event_tx: broadcast::Sender<Event>,
        trusted: Arc<Mutex<HashMap<Uuid, PeerInfo>>>,
        active: Arc<Mutex<HashMap<Uuid, ConnectionHandle>>>,
    ) {
        let plain_stream = MaybeTlsStream::Plain(stream);
        let ws_stream = match accept_async_with_config(plain_stream, Some(ws_config())).await {
            Ok(ws) => ws,
            Err(e) => {
                tracing::warn!("messaging: WS handshake failed with {}: {}", addr, e);
                return;
            }
        };

        let (mut ws_sink, mut ws_stream) = ws_stream.split();

        let hello_env = match read_envelope(&mut ws_stream).await {
            Some(env) if env.message_type == MessageType::Hello => env,
            _ => {
                tracing::warn!("messaging: no HELLO from {}", addr);
                return;
            }
        };

        let peer_device_id = hello_env.device_id;
        let peer_device_name = hello_env.device_name.clone();

        let accepted = match &hello_env.payload {
            Payload::Hello(hp) => hp.protocol_version == PROTOCOL_VERSION,
            _ => false,
        };

        if !accepted {
            let ack_env = Envelope::build(
                MessageType::HelloAck,
                self_device_id,
                self_device_name.clone(),
                Payload::HelloAck(HelloAckPayload {
                    accepted: false,
                    reason: Some(ErrorCode::ProtocolVersionUnsupported),
                }),
            );
            let _ = send_envelope(&mut ws_sink, &ack_env).await;
            return;
        }

        let is_trusted = trusted.lock().await.contains_key(&peer_device_id);

        let ack_env = Envelope::build(
            MessageType::HelloAck,
            self_device_id,
            self_device_name.clone(),
            Payload::HelloAck(HelloAckPayload {
                accepted: true,
                reason: None,
            }),
        );
        let _ = send_envelope(&mut ws_sink, &ack_env).await;

        let session_state = if is_trusted {
            SessionState::Trusted
        } else {
            SessionState::Pairing
        };

        let _ = event_tx.send(Event::new(
            EventSource::MessagingService,
            EventType::DeviceConnected(DeviceConnectedPayload {
                device_id: peer_device_id,
                device_name: peer_device_name.clone(),
                connection_type: ConnectionType::Inbound,
            }),
        ));

        tracing::info!(
            "messaging: connected with {} ({:?})",
            peer_device_name,
            session_state
        );

        let sink = Arc::new(Mutex::new(ws_sink));
        let session_state_handle = Arc::new(Mutex::new(session_state.clone()));
        {
            let mut conns = active.lock().await;
            conns.insert(
                peer_device_id,
                ConnectionHandle {
                    device_id: peer_device_id,
                    sink: sink.clone(),
                    session_state: session_state_handle.clone(),
                },
            );
        }

        let heartbeat_interval = Duration::from_secs(15);
        let peer_timeout = Duration::from_secs(45);

        let write_handle = {
            let sink = sink.clone();
            let peer_id = peer_device_id;
            tokio::spawn(async move {
                let mut ticker = time::interval(heartbeat_interval);
                loop {
                    ticker.tick().await;
                    let ping_env = Envelope::build(
                        MessageType::Ping,
                        Uuid::new_v4(),
                        "system".to_string(),
                        Payload::Ping,
                    );
                    let mut s = sink.lock().await;
                    if send_envelope(&mut s, &ping_env).await.is_err() {
                        tracing::warn!("messaging: failed to send PING to {}", peer_id);
                        break;
                    }
                }
            })
        };

        let mut _last_pong = tokio::time::Instant::now();
        loop {
            let current_state = session_state_handle.lock().await.clone();
            tokio::select! {
                msg = ws_stream.next() => {
                    match msg {
                        Some(Ok(tokio_tungstenite::tungstenite::Message::Text(text))) => {
                            let env = match Envelope::from_bytes(text.as_bytes()) {
                                Ok(e) => e,
                                Err(e) => {
                                    tracing::warn!("messaging: malformed message from {}: {}", addr, e);
                                    continue;
                                }
                            };
                            _last_pong = tokio::time::Instant::now();
                            Self::handle_message(
                                env, &current_state, peer_device_id, &inbound_tx,
                            ).await;
                        }
                        Some(Ok(tokio_tungstenite::tungstenite::Message::Binary(data))) => {
                            let env = match Envelope::from_bytes(&data) {
                                Ok(e) => e,
                                Err(e) => {
                                    tracing::warn!("messaging: malformed binary from {}: {}", addr, e);
                                    continue;
                                }
                            };
                            _last_pong = tokio::time::Instant::now();
                            Self::handle_message(
                                env, &current_state, peer_device_id, &inbound_tx,
                            ).await;
                        }
                        Some(Ok(tokio_tungstenite::tungstenite::Message::Ping(_))) => {
                            let pong_env = Envelope::build(
                                MessageType::Pong,
                                self_device_id,
                                self_device_name.clone(),
                                Payload::Pong,
                            );
                            let _ = send_envelope(&mut *sink.lock().await, &pong_env).await;
                        }
                        Some(Ok(tokio_tungstenite::tungstenite::Message::Pong(_))) => {
                            _last_pong = tokio::time::Instant::now();
                        }
                        Some(Ok(tokio_tungstenite::tungstenite::Message::Close(_))) => break,
                        Some(Err(_)) => break,
                        None => break,
                        _ => {}
                    }
                }
                _ = tokio::time::sleep(peer_timeout) => {
                    tracing::warn!("messaging: peer {} timed out", peer_device_id);
                    break;
                }
            }
        }

        write_handle.abort();

        let _ = event_tx.send(Event::new(
            EventSource::MessagingService,
            EventType::DeviceDisconnected(DeviceDisconnectedPayload {
                device_id: peer_device_id,
                reason: DisconnectReason::Timeout,
            }),
        ));

        {
            let mut conns = active.lock().await;
            conns.remove(&peer_device_id);
        }
    }

    async fn handle_message(
        env: Envelope,
        session_state: &SessionState,
        peer_device_id: Uuid,
        inbound_tx: &InboundEnvelopeTx,
    ) {
        match &env.message_type {
            MessageType::Ping | MessageType::Pong => {}
            MessageType::HelloAck => {}
            MessageType::Goodbye => {
                tracing::info!("messaging: peer {} sent GOODBYE", peer_device_id);
            }
            MessageType::Error => {
                if let Payload::Error(ep) = &env.payload {
                    tracing::warn!(
                        "messaging: error from {}: {} - {}",
                        peer_device_id,
                        ep.code,
                        ep.message
                    );
                }
            }
            _ => {
                if *session_state == SessionState::Pairing {
                    match &env.message_type {
                        MessageType::PairingRequest
                        | MessageType::PairingAccept
                        | MessageType::PairingReject => {}
                        _ => {
                            tracing::warn!(
                                "messaging: rejected non-pairing message from untrusted {}",
                                peer_device_id
                            );
                            return;
                        }
                    }
                }
                let _ = inbound_tx.send(crate::channels::InboundEnvelope {
                    device_id: peer_device_id,
                    envelope: env,
                });
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn command_loop(
        mut msg_rx: MessagingCommandRx,
        self_device_id: Uuid,
        self_device_name: String,
        platform: clipboard_proto::types::Platform,
        inbound_tx: InboundEnvelopeTx,
        event_tx: broadcast::Sender<Event>,
        trusted: Arc<Mutex<HashMap<Uuid, PeerInfo>>>,
        active: Arc<Mutex<HashMap<Uuid, ConnectionHandle>>>,
        outbox: OutboxMap,
        storage: Arc<Storage>,
    ) {
        while let Some(cmd) = msg_rx.recv().await {
            match cmd {
                MessagingCommand::ConnectTo { peer } => {
                    let peer_id = peer.device_id;
                    let peer_addr = peer.address;

                    if active.lock().await.contains_key(&peer_id) {
                        continue;
                    }

                    let inbound_tx = inbound_tx.clone();
                    let event_tx = event_tx.clone();
                    let trusted = trusted.clone();
                    let active = active.clone();
                    let outbox = outbox.clone();
                    let self_id = self_device_id;
                    let self_name = self_device_name.clone();

                    tokio::spawn(async move {
                        Self::connect_to_peer(
                            peer_addr, peer_id, self_id, self_name, platform, inbound_tx, event_tx,
                            trusted, active, outbox,
                        )
                        .await;
                    });
                }
                MessagingCommand::Send {
                    device_id,
                    envelope,
                } => {
                    let conns = active.lock().await;
                    if let Some(conn) = conns.get(&device_id) {
                        let mut sink = conn.sink.lock().await;
                        let _ = send_envelope(&mut sink, &envelope).await;
                    } else {
                        drop(conns);
                        tracing::info!(
                            "messaging: no connection to {}, queuing envelope",
                            device_id
                        );
                        let mut ob = outbox.lock().await;
                        ob.entry(device_id).or_default().push(envelope);
                    }
                }
                MessagingCommand::Disconnect { device_id } => {
                    // Remove first: a WebSocket close can await I/O, so it must not
                    // hold the active-connections registry lock while doing so.
                    let conn = active.lock().await.remove(&device_id);
                    if let Some(conn) = conn {
                        let mut sink = conn.sink.lock().await;
                        let _ = sink
                            .send(tokio_tungstenite::tungstenite::Message::Close(None))
                            .await;
                    }
                    trusted.lock().await.remove(&device_id);
                    // Forget is terminal for queued outbound data to this peer.
                    outbox.lock().await.remove(&device_id);
                    let _ = event_tx.send(Event::new(
                        EventSource::MessagingService,
                        EventType::DeviceDisconnected(DeviceDisconnectedPayload {
                            device_id,
                            reason: DisconnectReason::Graceful,
                        }),
                    ));
                    tracing::info!("messaging: disconnected from {}", device_id);
                }
                MessagingCommand::ReloadTrusted => {
                    let peers = match storage.trusted_peers() {
                        Ok(peers) => peers,
                        Err(error) => {
                            tracing::error!("messaging: failed to reload trusted peers: {}", error);
                            continue;
                        }
                    };
                    let mut map = trusted.lock().await;
                    map.clear();
                    for p in peers {
                        map.insert(
                            p.device_id,
                            PeerInfo {
                                device_id: p.device_id,
                                device_name: p.device_name,
                                platform: p.platform.unwrap_or_else(current_platform),
                                address: "0.0.0.0:0".parse().unwrap(),
                            },
                        );
                    }
                    tracing::debug!("messaging: reloaded trusted peers ({} entries)", map.len());
                    drop(map);

                    let trusted_ids: std::collections::HashSet<Uuid> =
                        trusted.lock().await.keys().copied().collect();
                    let conns = active.lock().await;
                    for (id, conn) in conns.iter() {
                        if trusted_ids.contains(id) {
                            let mut state = conn.session_state.lock().await;
                            if *state == SessionState::Pairing {
                                *state = SessionState::Trusted;
                                tracing::info!("messaging: promoted {} to Trusted", id);
                            }
                        }
                    }
                }
                MessagingCommand::Stop => {
                    break;
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn connect_to_peer(
        addr: SocketAddr,
        peer_id: Uuid,
        self_device_id: Uuid,
        self_device_name: String,
        _platform: clipboard_proto::types::Platform,
        inbound_tx: InboundEnvelopeTx,
        event_tx: broadcast::Sender<Event>,
        trusted: Arc<Mutex<HashMap<Uuid, PeerInfo>>>,
        active: Arc<Mutex<HashMap<Uuid, ConnectionHandle>>>,
        outbox: OutboxMap,
    ) {
        tracing::info!("messaging: connecting to {} at {}", peer_id, addr);

        let url = format!("ws://{}", addr);

        let ws_stream = match connect_async_with_config(&url, Some(ws_config()), false).await {
            Ok(ws) => ws.0,
            Err(e) => {
                tracing::warn!("messaging: WS handshake failed with {}: {}", peer_id, e);
                let _ = event_tx.send(Event::new(
                    EventSource::MessagingService,
                    EventType::DeviceConnectionFailed(DeviceConnectionFailedPayload {
                        device_id: peer_id,
                        error: e.to_string(),
                    }),
                ));
                return;
            }
        };

        let (mut ws_sink, mut ws_stream) = ws_stream.split();

        let hello_env = Envelope::build(
            MessageType::Hello,
            self_device_id,
            self_device_name.clone(),
            Payload::Hello(HelloPayload {
                protocol_version: PROTOCOL_VERSION,
                supported_capabilities: vec!["text/plain".to_string()],
            }),
        );
        if send_envelope(&mut ws_sink, &hello_env).await.is_err() {
            tracing::warn!("messaging: failed to send HELLO to {}", peer_id);
            return;
        }

        let ack_env =
            match tokio::time::timeout(Duration::from_secs(5), read_envelope(&mut ws_stream)).await
            {
                Ok(Some(env)) if env.message_type == MessageType::HelloAck => env,
                _ => {
                    tracing::warn!("messaging: no HELLO_ACK from {}", peer_id);
                    return;
                }
            };

        match &ack_env.payload {
            Payload::HelloAck(ha) => {
                if !ha.accepted {
                    tracing::info!(
                        "messaging: connection rejected by {}: {:?}",
                        peer_id,
                        ha.reason
                    );
                    return;
                }
            }
            _ => {
                tracing::warn!("messaging: unexpected response from {}", peer_id);
                return;
            }
        }

        let is_trusted = trusted.lock().await.contains_key(&peer_id);
        let session_state = if is_trusted {
            SessionState::Trusted
        } else {
            SessionState::Pairing
        };

        let _ = event_tx.send(Event::new(
            EventSource::MessagingService,
            EventType::DeviceConnected(DeviceConnectedPayload {
                device_id: peer_id,
                device_name: "remote".to_string(),
                connection_type: ConnectionType::Outbound,
            }),
        ));

        tracing::info!("messaging: connected to {} ({:?})", peer_id, session_state);

        let sink = Arc::new(Mutex::new(ws_sink));
        let session_state_handle = Arc::new(Mutex::new(session_state.clone()));
        {
            let mut conns = active.lock().await;
            conns.insert(
                peer_id,
                ConnectionHandle {
                    device_id: peer_id,
                    sink: sink.clone(),
                    session_state: session_state_handle.clone(),
                },
            );
        }

        // Flush any queued outbox messages for this peer
        {
            let mut ob = outbox.lock().await;
            if let Some(queued) = ob.remove(&peer_id) {
                let count = queued.len();
                let mut s = sink.lock().await;
                for env in queued {
                    let _ = send_envelope(&mut s, &env).await;
                }
                tracing::info!(
                    "messaging: flushed {} queued messages to {}",
                    count,
                    peer_id
                );
            }
        }

        let heartbeat_interval = Duration::from_secs(15);
        let write_handle = {
            let sink = sink.clone();
            tokio::spawn(async move {
                let mut ticker = time::interval(heartbeat_interval);
                loop {
                    ticker.tick().await;
                    let ping_env = Envelope::build(
                        MessageType::Ping,
                        Uuid::new_v4(),
                        "system".to_string(),
                        Payload::Ping,
                    );
                    let mut s = sink.lock().await;
                    if send_envelope(&mut s, &ping_env).await.is_err() {
                        tracing::warn!("messaging: failed to send PING to {}", peer_id);
                        break;
                    }
                }
            })
        };

        let peer_timeout = Duration::from_secs(45);
        let mut _last_pong = tokio::time::Instant::now();
        loop {
            let current_state = session_state_handle.lock().await.clone();
            tokio::select! {
                msg = ws_stream.next() => {
                    match msg {
                        Some(Ok(tokio_tungstenite::tungstenite::Message::Text(text))) => {
                            let env = match Envelope::from_bytes(text.as_bytes()) {
                                Ok(e) => e,
                                Err(e) => {
                                    tracing::warn!("messaging: malformed from {}: {}", peer_id, e);
                                    continue;
                                }
                            };
                            _last_pong = tokio::time::Instant::now();
                            Self::handle_message(
                                env, &current_state, peer_id, &inbound_tx,
                            ).await;
                        }
                        Some(Ok(tokio_tungstenite::tungstenite::Message::Binary(data))) => {
                            let env = match Envelope::from_bytes(&data) {
                                Ok(e) => e,
                                Err(e) => {
                                    tracing::warn!("messaging: malformed binary from {}: {}", peer_id, e);
                                    continue;
                                }
                            };
                            _last_pong = tokio::time::Instant::now();
                            Self::handle_message(
                                env, &current_state, peer_id, &inbound_tx,
                            ).await;
                        }
                        Some(Ok(tokio_tungstenite::tungstenite::Message::Ping(_))) => {
                            let pong_env = Envelope::build(
                                MessageType::Pong,
                                self_device_id,
                                self_device_name.clone(),
                                Payload::Pong,
                            );
                            let _ = send_envelope(&mut *sink.lock().await, &pong_env).await;
                        }
                        Some(Ok(tokio_tungstenite::tungstenite::Message::Pong(_))) => {
                            _last_pong = tokio::time::Instant::now();
                        }
                        Some(Ok(tokio_tungstenite::tungstenite::Message::Close(_))) => break,
                        Some(Err(_)) => break,
                        None => break,
                        _ => {}
                    }
                }
                _ = tokio::time::sleep(peer_timeout) => {
                    tracing::warn!("messaging: peer {} timed out", peer_id);
                    break;
                }
            }
        }

        write_handle.abort();

        let _ = event_tx.send(Event::new(
            EventSource::MessagingService,
            EventType::DeviceDisconnected(DeviceDisconnectedPayload {
                device_id: peer_id,
                reason: DisconnectReason::Timeout,
            }),
        ));

        {
            let mut conns = active.lock().await;
            conns.remove(&peer_id);
        }
    }
}

async fn read_envelope(stream: &mut WsStream) -> Option<Envelope> {
    loop {
        match stream.next().await {
            Some(Ok(tokio_tungstenite::tungstenite::Message::Text(text))) => {
                return Envelope::from_bytes(text.as_bytes()).ok();
            }
            Some(Ok(tokio_tungstenite::tungstenite::Message::Binary(data))) => {
                return Envelope::from_bytes(&data).ok();
            }
            Some(Ok(tokio_tungstenite::tungstenite::Message::Close(_))) => return None,
            Some(Err(_)) => return None,
            None => return None,
            _ => {}
        }
    }
}

async fn send_envelope(sink: &mut WsSink, env: &Envelope) -> Result<(), ()> {
    let data = env.to_bytes().map_err(|_| ())?;
    sink.send(tokio_tungstenite::tungstenite::Message::Text(
        String::from_utf8(data).map_err(|_| ())?,
    ))
    .await
    .map_err(|_| ())
}
