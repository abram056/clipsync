//! Doc-10 integration scenarios for inbound protocol validation.
//!
//! The core's validation paths (rejections with ErrorCodes 1, 7, 8 and 9,
//! plus same-origin duplicate suppression) live in
//! `coordinator::handle_inbound` / `handle_clipboard_update`, so exercising
//! them requires a peer that can send crafted envelopes. These tests drive a
//! raw WebSocket client — the "hostile peer" — through the real HELLO
//! handshake and pairing flow instead of a second clipboard-core instance.
//!
//! The oversized-payload test uses a deliberately small configured limit
//! (1 KB + 1 byte) rather than the documented 2 MB + 1: the boundary logic
//! under test is identical and the test stays fast.

use std::net::SocketAddr;
use std::time::Duration;

use chrono::Utc;
use clipboard_core::channels::AppCommand;
use clipboard_core::config::{
    AppConfig, HistoryConfig, NetworkConfig, PlatformConfig, StorageConfig, SyncConfig,
};
use clipboard_core::{start, AppHandle};
use clipboard_proto::error::ErrorCode;
use clipboard_proto::event::{DisconnectReason, DuplicateReason, Event, EventType};
use clipboard_proto::message::{
    ClipboardAckStatus, ClipboardUpdatePayload, Envelope, HelloPayload, MessageType,
    PairingRejectReason, PairingRequestPayload, Payload, PROTOCOL_VERSION,
};
use clipboard_proto::types::Platform;
use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message as WsMessage;
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};
use uuid::Uuid;

/// Port bases reserved by this file (plus `pid % 1000`, the workspace
/// convention). Chosen above phase_c (60191+, topping out at 61253) and
/// above phase_d (61191+, which tops out at 62263 even with the pid offset).
const VERSION_MISMATCH_BASE: u16 = 62300;
const OVERSIZED_BASE: u16 = 62304;
const REPLAY_BASE: u16 = 62308;
const SKEW_BASE: u16 = 62312;
const DUPLICATE_HASH_BASE: u16 = 62316;
const REJECT_BASE: u16 = 62320;
const PRETRUST_BASE: u16 = 62324;
const CONNECTION_LIMIT_BASE: u16 = 62328;
const HEARTBEAT_BASE: u16 = 62332;

fn test_port(port: u16) -> u16 {
    port + (std::process::id() % 1_000) as u16
}

fn node_addr(listen_port: u16) -> SocketAddr {
    SocketAddr::from(([127, 0, 0, 1], test_port(listen_port)))
}

fn make_config(
    id: u16,
    discovery_port: u16,
    listen_port: u16,
    max_clipboard_bytes: usize,
) -> AppConfig {
    let db_path = std::env::temp_dir().join(format!(
        "clipboard_sync_pv_{}_{}_{}",
        id,
        std::process::id(),
        Uuid::new_v4()
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
            discovery_targets: Vec::new(),
        },
        history: HistoryConfig {
            max_size_bytes: 2_097_152,
        },
        sync: SyncConfig {
            max_clipboard_bytes,
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

/// Wait for the first matching event instead of draining until the deadline,
/// so repeated waits in one test do not each burn the full timeout.
fn wait_for(
    rx: &mut tokio::sync::broadcast::Receiver<Event>,
    timeout: Duration,
    predicate: impl Fn(&EventType) -> bool,
) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        match rx.try_recv() {
            Ok(event) => {
                if predicate(&event.event_type) {
                    return true;
                }
            }
            Err(tokio::sync::broadcast::error::TryRecvError::Empty) => {
                if std::time::Instant::now() >= deadline {
                    return false;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(_) => return false,
        }
    }
}

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;
type WsSink = futures_util::stream::SplitSink<Ws, WsMessage>;
type WsStream = futures_util::stream::SplitStream<Ws>;

/// A raw protocol client that is not backed by a clipboard-core instance.
struct HostilePeer {
    sink: WsSink,
    stream: WsStream,
    device_id: Uuid,
    device_name: String,
}

impl HostilePeer {
    /// Connect and run the HELLO handshake. Returns the peer together with
    /// the HELLO_ACK the server sent (None on timeout or socket close).
    async fn connect(addr: SocketAddr, protocol_version: u32) -> (Self, Option<Envelope>) {
        // start() returns as soon as the runtime is up, while the node's WS
        // listener binds asynchronously; retry briefly instead of racing it.
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        let ws = loop {
            match connect_async(format!("ws://{}", addr)).await {
                Ok((ws, _)) => break ws,
                Err(err) => {
                    if tokio::time::Instant::now() >= deadline {
                        panic!("websocket connect to {}: {}", addr, err);
                    }
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
            }
        };
        let (sink, stream) = ws.split();
        let mut peer = HostilePeer {
            sink,
            stream,
            device_id: Uuid::new_v4(),
            device_name: "hostile-peer".to_string(),
        };

        let hello = Envelope::build(
            MessageType::Hello,
            peer.device_id,
            peer.device_name.clone(),
            Payload::Hello(HelloPayload {
                protocol_version,
                supported_capabilities: Vec::new(),
            }),
        );
        peer.send(hello).await;
        let ack = peer.recv_one(Duration::from_secs(5)).await;
        (peer, ack)
    }

    async fn send(&mut self, env: Envelope) {
        let data = env.to_bytes().expect("serialize envelope");
        self.sink
            .send(WsMessage::Text(String::from_utf8(data).expect("utf8")))
            .await
            .expect("send envelope");
    }

    /// Next protocol envelope, skipping WebSocket control frames. Returns
    /// None on timeout, protocol error or connection close.
    async fn recv_one(&mut self, timeout: Duration) -> Option<Envelope> {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return None;
            }
            match tokio::time::timeout(remaining, self.stream.next()).await {
                Ok(Some(Ok(WsMessage::Text(text)))) => {
                    return Envelope::from_bytes(text.as_bytes()).ok()
                }
                Ok(Some(Ok(WsMessage::Binary(data)))) => return Envelope::from_bytes(&data).ok(),
                Ok(Some(Ok(_))) => continue,
                Ok(Some(Err(_))) | Ok(None) => return None,
                Err(_) => return None,
            }
        }
    }

    /// First envelope matching `predicate`, skipping everything else
    /// (heartbeats, unrelated traffic). None on timeout or close.
    async fn recv_matching(
        &mut self,
        timeout: Duration,
        predicate: impl Fn(&Envelope) -> bool,
    ) -> Option<Envelope> {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return None;
            }
            let env = self.recv_one(remaining).await?;
            if predicate(&env) {
                return Some(env);
            }
        }
    }
}

fn clipboard_update(origin_device_id: Uuid, content_hash: &str, content: String) -> Envelope {
    Envelope::build(
        MessageType::ClipboardUpdate,
        Uuid::new_v4(),
        "hostile-peer".to_string(),
        Payload::ClipboardUpdate(ClipboardUpdatePayload {
            clipboard_id: Uuid::new_v4(),
            origin_device_id,
            content_type: "text/plain".to_string(),
            content,
            content_hash: content_hash.to_string(),
            creation_timestamp: Utc::now(),
        }),
    )
}

/// Drive the raw client through the pairing handshake against `handle`:
/// PAIRING_REQUEST, approval, PAIRING_ACCEPT, and the trust promotion that
/// follows, so subsequent envelopes are judged against a Trusted session.
async fn pair_with(handle: &AppHandle, peer: &mut HostilePeer) {
    let mut rx = handle.event_tx().subscribe();
    let request_id = Uuid::new_v4();

    let request = Envelope::build(
        MessageType::PairingRequest,
        peer.device_id,
        peer.device_name.clone(),
        Payload::PairingRequest(PairingRequestPayload {
            device_id: peer.device_id,
            device_name: peer.device_name.clone(),
            platform: Platform::Linux,
            request_id,
        }),
    );
    peer.send(request).await;

    assert!(
        wait_for(&mut rx, Duration::from_secs(5), |et| matches!(
            et,
            EventType::PairingRequested(p) if p.device_id == peer.device_id
        )),
        "node should surface the pairing request"
    );

    handle.send_command(AppCommand::ApprovePairing {
        device_id: peer.device_id,
    });

    let accept = peer
        .recv_matching(Duration::from_secs(5), |env| {
            env.message_type == MessageType::PairingAccept
        })
        .await
        .expect("PAIRING_ACCEPT after approval");
    match accept.payload {
        Payload::PairingAccept(pa) => assert_eq!(
            pa.request_id, request_id,
            "PAIRING_ACCEPT should carry the original request id"
        ),
        other => panic!("expected PAIRING_ACCEPT, got {:?}", other),
    }

    assert!(
        wait_for(&mut rx, Duration::from_secs(5), |et| matches!(
            et,
            EventType::PairingAccepted(p) if p.device_id == peer.device_id
        )),
        "node should emit PairingAccepted"
    );

    // ReloadTrusted is queued right after approve() returns and promotes the
    // connection's session state; give the messaging command loop a beat so
    // the first post-pairing envelope is not judged against Pairing.
    tokio::time::sleep(Duration::from_millis(500)).await;
}

/// HELLO with an unsupported protocol version must be refused during the
/// handshake with PROTOCOL_VERSION_UNSUPPORTED (ErrorCode 1).
#[test]
fn version_mismatch_rejected_in_hello() {
    let config = make_config(
        1,
        VERSION_MISMATCH_BASE,
        VERSION_MISMATCH_BASE + 1,
        2_097_152,
    );
    let handle = start(config).expect("start node");
    let addr = node_addr(VERSION_MISMATCH_BASE + 1);

    let rt = tokio::runtime::Runtime::new().expect("runtime");
    rt.block_on(async {
        let (_peer, ack) = HostilePeer::connect(addr, PROTOCOL_VERSION + 1).await;
        let ack = ack.expect("HELLO_ACK even for an unsupported version");
        assert_eq!(ack.message_type, MessageType::HelloAck);
        match ack.payload {
            Payload::HelloAck(ha) => {
                assert!(!ha.accepted, "unsupported protocol version must be refused");
                assert_eq!(ha.reason, Some(ErrorCode::ProtocolVersionUnsupported));
            }
            other => panic!("expected HELLO_ACK, got {:?}", other),
        }
    });

    handle.stop();
}

/// An oversized CLIPBOARD_UPDATE from a trusted peer is answered with
/// CLIPBOARD_ACK Rejected(PAYLOAD_TOO_LARGE) (ErrorCode 7) — the payload
/// must not reach history or the sync engine.
#[test]
fn oversized_clipboard_rejected() {
    let config = make_config(2, OVERSIZED_BASE, OVERSIZED_BASE + 1, 1024);
    let handle = start(config).expect("start node");
    let addr = node_addr(OVERSIZED_BASE + 1);

    let rt = tokio::runtime::Runtime::new().expect("runtime");
    rt.block_on(async {
        let (mut peer, ack) = HostilePeer::connect(addr, PROTOCOL_VERSION).await;
        match ack.map(|a| a.payload) {
            Some(Payload::HelloAck(ha)) => assert!(ha.accepted, "handshake should be accepted"),
            other => panic!("expected accepted HELLO_ACK, got {:?}", other),
        }
        pair_with(&handle, &mut peer).await;

        let oversized = clipboard_update(peer.device_id, "hash-oversize", "x".repeat(1025));
        peer.send(oversized).await;

        let ack = peer
            .recv_matching(Duration::from_secs(5), |env| {
                env.message_type == MessageType::ClipboardAck
            })
            .await
            .expect("CLIPBOARD_ACK for the oversized payload");
        match ack.payload {
            Payload::ClipboardAck(ca) => assert!(
                matches!(
                    ca.status,
                    ClipboardAckStatus::Rejected { code } if code == ErrorCode::PayloadTooLarge
                ),
                "expected PAYLOAD_TOO_LARGE rejection, got {:?}",
                ca.status
            ),
            other => panic!("expected CLIPBOARD_ACK, got {:?}", other),
        }
    });

    handle.stop();
}

/// Resending an envelope that reuses a message id must be answered with
/// ERROR(DUPLICATE_MESSAGE) (ErrorCode 8) and not applied a second time.
#[test]
fn replayed_message_id_rejected() {
    let config = make_config(3, REPLAY_BASE, REPLAY_BASE + 1, 2_097_152);
    let handle = start(config).expect("start node");
    let addr = node_addr(REPLAY_BASE + 1);

    let rt = tokio::runtime::Runtime::new().expect("runtime");
    rt.block_on(async {
        let (mut peer, _ack) = HostilePeer::connect(addr, PROTOCOL_VERSION).await;
        pair_with(&handle, &mut peer).await;

        let env = clipboard_update(peer.device_id, "hash-replay", "replay me".to_string());
        peer.send(env.clone()).await;

        let first = peer
            .recv_matching(Duration::from_secs(5), |e| {
                e.message_type == MessageType::ClipboardAck
            })
            .await
            .expect("CLIPBOARD_ACK for the first delivery");
        match first.payload {
            Payload::ClipboardAck(ca) => assert!(
                matches!(ca.status, ClipboardAckStatus::Accepted),
                "first delivery should be accepted, got {:?}",
                ca.status
            ),
            other => panic!("expected CLIPBOARD_ACK, got {:?}", other),
        }

        peer.send(env).await;
        let replay = peer
            .recv_matching(Duration::from_secs(5), |e| {
                e.message_type == MessageType::Error
            })
            .await
            .expect("ERROR for the replayed message");
        match replay.payload {
            Payload::Error(ep) => assert_eq!(
                ep.code,
                ErrorCode::DuplicateMessage,
                "replayed envelope must be refused as a duplicate"
            ),
            other => panic!("expected ERROR, got {:?}", other),
        }
    });

    handle.stop();
}

/// An envelope whose timestamp falls outside the accepted skew window is
/// answered with ERROR(STALE_MESSAGE) (ErrorCode 9).
#[test]
fn timestamp_skew_rejected() {
    let config = make_config(4, SKEW_BASE, SKEW_BASE + 1, 2_097_152);
    let handle = start(config).expect("start node");
    let addr = node_addr(SKEW_BASE + 1);

    let rt = tokio::runtime::Runtime::new().expect("runtime");
    rt.block_on(async {
        let (mut peer, _ack) = HostilePeer::connect(addr, PROTOCOL_VERSION).await;
        pair_with(&handle, &mut peer).await;

        let mut env = clipboard_update(peer.device_id, "hash-skew", "skewed".to_string());
        env.timestamp = Utc::now() - chrono::Duration::seconds(400);
        peer.send(env).await;

        let err = peer
            .recv_matching(Duration::from_secs(5), |e| {
                e.message_type == MessageType::Error
            })
            .await
            .expect("ERROR for the stale envelope");
        match err.payload {
            Payload::Error(ep) => assert_eq!(
                ep.code,
                ErrorCode::StaleMessage,
                "stale envelope must be refused with STALE_MESSAGE"
            ),
            other => panic!("expected ERROR, got {:?}", other),
        }
    });

    handle.stop();
}

/// Inbound content that matches an already-recorded hash must be dropped
/// with DuplicateClipboardIgnored, and when the payload claims this node as
/// its origin the reason must be SameOrigin — the loop-prevention core of
/// "entries received from a remote must never be redistributed".
#[test]
fn same_origin_duplicate_hash_ignored() {
    let config = make_config(5, DUPLICATE_HASH_BASE, DUPLICATE_HASH_BASE + 1, 2_097_152);
    let handle = start(config).expect("start node");
    let addr = node_addr(DUPLICATE_HASH_BASE + 1);
    let mut rx = handle.event_tx().subscribe();

    let rt = tokio::runtime::Runtime::new().expect("runtime");
    rt.block_on(async {
        let (mut peer, _ack) = HostilePeer::connect(addr, PROTOCOL_VERSION).await;
        pair_with(&handle, &mut peer).await;

        let (node_id, _) = handle.identity();
        let first = clipboard_update(node_id, "hash-same-origin", "same bytes".to_string());
        peer.send(first).await;
        let ack = peer
            .recv_matching(Duration::from_secs(5), |e| {
                e.message_type == MessageType::ClipboardAck
            })
            .await
            .expect("CLIPBOARD_ACK for the first delivery");
        match ack.payload {
            Payload::ClipboardAck(ca) => assert!(
                matches!(ca.status, ClipboardAckStatus::Accepted),
                "first delivery should be accepted, got {:?}",
                ca.status
            ),
            other => panic!("expected CLIPBOARD_ACK, got {:?}", other),
        }

        // Fresh message id, same content hash: the hash dedup must catch it.
        let second = clipboard_update(node_id, "hash-same-origin", "same bytes".to_string());
        peer.send(second).await;

        let ignored = wait_for(&mut rx, Duration::from_secs(5), |et| {
            matches!(
                et,
                EventType::DuplicateClipboardIgnored(p)
                    if matches!(p.reason, DuplicateReason::SameOrigin)
            )
        });
        assert!(
            ignored,
            "duplicate content claiming this node as origin must be ignored as SameOrigin"
        );
    });

    handle.stop();
}

/// A rejected pairing request must answer the requester with
/// PAIRING_REJECT(UserDenied) and surface PairingRejected on the node.
#[test]
fn pairing_reject_returns_reject_to_requester() {
    let config = make_config(6, REJECT_BASE, REJECT_BASE + 1, 2_097_152);
    let handle = start(config).expect("start node");
    let addr = node_addr(REJECT_BASE + 1);
    let mut rx = handle.event_tx().subscribe();

    let rt = tokio::runtime::Runtime::new().expect("runtime");
    rt.block_on(async {
        let (mut peer, _ack) = HostilePeer::connect(addr, PROTOCOL_VERSION).await;

        let request_id = Uuid::new_v4();
        let request = Envelope::build(
            MessageType::PairingRequest,
            peer.device_id,
            peer.device_name.clone(),
            Payload::PairingRequest(PairingRequestPayload {
                device_id: peer.device_id,
                device_name: peer.device_name.clone(),
                platform: Platform::Linux,
                request_id,
            }),
        );
        peer.send(request).await;

        assert!(
            wait_for(&mut rx, Duration::from_secs(5), |et| matches!(
                et,
                EventType::PairingRequested(p) if p.device_id == peer.device_id
            )),
            "node should surface the pairing request"
        );

        handle.send_command(AppCommand::RejectPairing {
            device_id: peer.device_id,
            reason: "no thanks".to_string(),
        });

        let reject = peer
            .recv_matching(Duration::from_secs(5), |env| {
                env.message_type == MessageType::PairingReject
            })
            .await
            .expect("PAIRING_REJECT sent back to the requester");
        match reject.payload {
            Payload::PairingReject(pr) => {
                assert_eq!(pr.request_id, request_id);
                assert!(
                    matches!(pr.reason, PairingRejectReason::UserDenied),
                    "reject reason should be UserDenied, got {:?}",
                    pr.reason
                );
            }
            other => panic!("expected PAIRING_REJECT, got {:?}", other),
        }

        assert!(
            wait_for(&mut rx, Duration::from_secs(5), |et| matches!(
                et,
                EventType::PairingRejected(p) if p.device_id == peer.device_id
            )),
            "node should emit PairingRejected"
        );
    });

    handle.stop();
}

/// A clipboard message from a connection still in Pairing must be refused
/// with UNKNOWN_DEVICE (ErrorCode 2) on the wire — and the connection must
/// survive so the peer can still pair (doc 03, connection state model).
#[test]
fn pre_trust_clipboard_update_returns_unknown_device() {
    let config = make_config(7, PRETRUST_BASE, PRETRUST_BASE + 1, 2_097_152);
    let handle = start(config).expect("start node");
    let addr = node_addr(PRETRUST_BASE + 1);
    let mut rx = handle.event_tx().subscribe();

    let rt = tokio::runtime::Runtime::new().expect("runtime");
    rt.block_on(async {
        let (mut peer, _ack) = HostilePeer::connect(addr, PROTOCOL_VERSION).await;

        // Session stays Pairing: no approval, no PAIRING_ACCEPT.
        let refused = clipboard_update(peer.device_id, "hash-pretrust", "early".to_string());
        let refused_id = refused.message_id;
        peer.send(refused).await;

        let err = peer
            .recv_matching(Duration::from_secs(5), |env| {
                env.message_type == MessageType::Error
            })
            .await
            .expect("ERROR for the pre-trust clipboard update");
        match err.payload {
            Payload::Error(ep) => {
                assert_eq!(
                    ep.code,
                    ErrorCode::UnknownDevice,
                    "pre-trust clipboard messages are refused with ErrorCode 2"
                );
                assert_eq!(
                    ep.related_message_id,
                    Some(refused_id),
                    "the ERROR must reference the refused message"
                );
            }
            other => panic!("expected Payload::Error, got {:?}", other),
        }

        // Nothing may be applied: no ACK for it, and history stays empty.
        let ack = peer
            .recv_matching(Duration::from_millis(500), |env| {
                env.message_type == MessageType::ClipboardAck
            })
            .await;
        assert!(
            ack.is_none(),
            "the refused message must not produce a CLIPBOARD_ACK"
        );

        // Send-and-continue: the pairing channel still works after the
        // violation, so the peer can request pairing on the same socket.
        let request_id = Uuid::new_v4();
        let request = Envelope::build(
            MessageType::PairingRequest,
            peer.device_id,
            peer.device_name.clone(),
            Payload::PairingRequest(PairingRequestPayload {
                device_id: peer.device_id,
                device_name: peer.device_name.clone(),
                platform: Platform::Linux,
                request_id,
            }),
        );
        peer.send(request).await;
        assert!(
            wait_for(&mut rx, Duration::from_secs(5), |et| matches!(
                et,
                EventType::PairingRequested(p) if p.device_id == peer.device_id
            )),
            "the connection must survive the refusal so pairing can still proceed"
        );
    });

    // Outside rt.block_on: history_size_bytes drives block_on internally.
    assert_eq!(
        handle.history_size_bytes().unwrap(),
        0,
        "the refused message must not reach history"
    );

    handle.stop();
}

/// Connections past `sync.max_peers` are refused with CONNECTION_LIMIT
/// (ErrorCode 12), and the peers already inside keep their sessions
/// (doc 08, limits table).
#[test]
fn connection_limit_returns_error_twelve() {
    let mut config = make_config(
        8,
        CONNECTION_LIMIT_BASE,
        CONNECTION_LIMIT_BASE + 1,
        2_097_152,
    );
    config.sync.max_peers = 1;
    let handle = start(config).expect("start node");
    let addr = node_addr(CONNECTION_LIMIT_BASE + 1);
    let mut rx = handle.event_tx().subscribe();

    let rt = tokio::runtime::Runtime::new().expect("runtime");
    rt.block_on(async {
        // The first peer fills the single slot.
        let (peer_a, ack_a) = HostilePeer::connect(addr, PROTOCOL_VERSION).await;
        let mut peer_a = peer_a;
        match ack_a.map(|a| a.payload) {
            Some(Payload::HelloAck(ha)) => assert!(ha.accepted, "first peer must be accepted"),
            other => panic!("expected accepted HELLO_ACK, got {:?}", other),
        }
        // The slot is recorded just after HELLO_ACK is written; give the
        // connection loop time to get there before opening the second one.
        tokio::time::sleep(Duration::from_millis(300)).await;

        // The second peer is refused with ErrorCode 12.
        let (_peer_b, first) = HostilePeer::connect(addr, PROTOCOL_VERSION).await;
        let first = first.expect("the over-cap connection must be answered");
        match first.payload {
            Payload::Error(ep) => assert_eq!(
                ep.code,
                ErrorCode::ConnectionLimit,
                "over-cap connections are refused with ErrorCode 12"
            ),
            other => panic!("expected Payload::Error, got {:?}", other),
        }

        // The first peer's session is unaffected: pairing still works.
        let request_id = Uuid::new_v4();
        let request = Envelope::build(
            MessageType::PairingRequest,
            peer_a.device_id,
            peer_a.device_name.clone(),
            Payload::PairingRequest(PairingRequestPayload {
                device_id: peer_a.device_id,
                device_name: peer_a.device_name.clone(),
                platform: Platform::Linux,
                request_id,
            }),
        );
        peer_a.send(request).await;
        assert!(
            wait_for(&mut rx, Duration::from_secs(5), |et| matches!(
                et,
                EventType::PairingRequested(p) if p.device_id == peer_a.device_id
            )),
            "the first peer must keep working after the second is refused"
        );
    });

    handle.stop();
}

/// Heartbeat and peer timeout come from `heartbeat_interval_secs` and
/// `peer_timeout_secs`, not from hardcoded 15 s/45 s (doc 08 network
/// constants, doc 10 heartbeat row). Configured small: 1 s / 3 s.
#[test]
fn configured_heartbeat_and_peer_timeout_are_honoured() {
    let mut config = make_config(9, HEARTBEAT_BASE, HEARTBEAT_BASE + 1, 2_097_152);
    config.network.heartbeat_interval_secs = 1;
    config.network.peer_timeout_secs = 3;
    let handle = start(config).expect("start node");
    let addr = node_addr(HEARTBEAT_BASE + 1);
    let mut rx = handle.event_tx().subscribe();

    let rt = tokio::runtime::Runtime::new().expect("runtime");
    rt.block_on(async {
        let (mut peer, _ack) = HostilePeer::connect(addr, PROTOCOL_VERSION).await;

        // A PING arrives on the configured 1 s interval (the interval's
        // first tick fires as soon as the connection is up).
        let ping = peer
            .recv_matching(Duration::from_secs(3), |env| {
                env.message_type == MessageType::Ping
            })
            .await;
        assert!(
            ping.is_some(),
            "the node must PING within heartbeat_interval_secs"
        );

        // The peer never answers, so the configured 3 s timeout applies.
        assert!(
            wait_for(&mut rx, Duration::from_secs(8), |et| matches!(
                et,
                EventType::DeviceDisconnected(p)
                    if matches!(p.reason, DisconnectReason::Timeout)
            )),
            "a silent peer must be dropped after peer_timeout_secs (3s, not 45s)"
        );
    });

    handle.stop();
}
