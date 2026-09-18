# Module Specification

## Overview

This document specifies the modules that implement Clipboard Sync. It covers responsibilities, public interfaces, events emitted and consumed, dependencies, and threading models.

Modules are grouped by crate. Type names use Rust syntax; UniFFI-exported types are additionally surfaced to Kotlin for the Android app.

## Conventions

- **Internal events** are defined in `clipboard-proto` (doc 03). Components communicate through a channel rather than direct calls.
- **Protocol messages** are defined in `clipboard-proto` (doc 03).
- Each module states the internal events it **emits** and **subscribes to**.
- Threading notes describe how the module fits into the Tokio runtime and the UI thread.

---

# clipboard-proto

Leaf crate: no internal dependencies.

## message

Defines the wire envelope and all protocol message variants.

Public types:

- `Envelope { message_id: Uuid, message_type: MessageType, device_id: Uuid, device_name: String, timestamp: DateTime<Utc>, payload: Payload }`
- `enum Payload` — one variant per protocol message (DISCOVER, DISCOVER_RESPONSE, HELLO, HELLO_ACK, GOODBYE, PAIRING_REQUEST, PAIRING_ACCEPT, PAIRING_REJECT, CLIPBOARD_UPDATE, CLIPBOARD_ACK, PING, PONG, ERROR).
- `struct ErrorPayload { code: ErrorCode, message: String, related_message_id: Option<Uuid> }`

Functions:

- `Payload::from_bytes / to_bytes` — JSON serialization.
- `Envelope::build(message_type, device_id, device_name, payload) -> Envelope` — assigns a UUIDv4 Message ID and timestamp.

Threading: none; pure data types.

## event

Defines the internal event model.

Public types:

- `struct Event { event_id: Uuid, timestamp: DateTime<Utc>, source: EventSource, event_type: EventType, payload: EventPayload }`
- `enum EventSource` — ClipboardMonitor, SyncEngine, DiscoveryService, MessagingService, HistoryManager, PairingManager, UserInterface.
- `enum EventType` / `enum EventPayload` — one variant per internal event from doc 03 (ApplicationStarted, ClipboardChanged, DeviceDiscovered, SyncRequested, NetworkError, etc.).

Threading: none; pure data types.

## error

Public types:

- `enum ErrorCode` — the registry from doc 03 (0–13).
- `type Result<T> = std::result::Result<T, Error>`
- `enum Error` — wraps `ErrorCode` plus context.

Threading: none.

## shared types

Types shared across crates and the UniFFI boundary:

- `struct DeviceInfo { device_id: Uuid, device_name: String, platform: Platform, is_trusted: bool, is_connected: bool }`
- `struct PairingRequest { request_id: Uuid, device_id: Uuid, device_name: String, platform: Platform, received_at: DateTime<Utc> }`
- `enum SyncStatus { Active, Paused, Syncing { peer_count: usize, history_bytes: u64 } }`
- `struct HistoryEntry { clipboard_id: Uuid, content: String, content_type: String, content_hash: String, origin_device_id: Uuid, created_at: DateTime<Utc>, size_bytes: u64 }`
- `struct PeerInfo { device_id: Uuid, device_name: String, platform: Platform, address: SocketAddr }`

---

# clipboard-core

Depends on `clipboard-proto`, `tokio`, `rusqlite`, `tokio-tungstenite`, `serde`, `uuid`, `sha2`, `chrono`, `tracing`, `uniffi`.

## runtime

Owns the Tokio runtime and coordinates module startup and shutdown.

Public API (UniFFI-exported):

- `fn start(config: AppConfig) -> Result<AppHandle>`
- `fn stop(handle: &AppHandle)`

Responsibilities:

- Build the multi-threaded Tokio runtime.
- Instantiate storage, discovery, messaging, pairing, sync engine, and history manager.
- Wire event channels between modules.
- Convert platform clipboard-monitor callbacks into `ClipboardChanged` events.

Events: subscribes to nothing; it is the composition root.

Threading: main app thread starts/stops the runtime; Tokio worker threads run the modules.

## storage

SQLite-backed persistence.

Public API:

- `fn open(config: &StorageConfig) -> Result<Storage>`
- `fn migrate(&self) -> Result<()>`
- `fn get_device_identity(&self) -> Result<DeviceIdentity>` — creates and persists one on first call.
- `fn set_device_name(&self, name: &str) -> Result<()>`
- `fn trusted_peers(&self) -> Result<Vec<TrustedPeer>>`
- `fn is_trusted(&self, device_id: &Uuid) -> Result<bool>`
- `fn add_trusted_peer(&self, peer: &TrustedPeer) -> Result<()>`
- `fn remove_trusted_peer(&self, device_id: &Uuid) -> Result<()>`
- `fn append_history(&self, entry: &HistoryEntry) -> Result<()>`
- `fn list_history(&self, limit: usize) -> Result<Vec<HistoryEntry>>`
- `fn history_size_bytes(&self) -> Result<u64>`
- `fn evict_history(&self, max_bytes: u64) -> Result<()>`
- `fn get_setting(&self, key: &str) -> Result<Option<String>>`
- `fn set_setting(&self, key: &str, value: &str) -> Result<()>`

Schema and eviction policy: doc 07.

Events: none. It is a library used by other modules.

Threading: owned by the Sync Engine task. Uses a synchronous rusqlite connection guarded by a Mutex; queries are short and non-blocking relative to the workload.

## config

Loads and validates configuration.

Public API:

- `fn load() -> Result<AppConfig>` — reads `~/.config/clipboard-sync/config.toml` (desktop), overlaying defaults; on Android returns defaults only.
- `fn save(&self, path: &Path) -> Result<()>` (desktop only).

Types: `AppConfig { network: NetworkConfig, history: HistoryConfig, sync: SyncConfig, storage: StorageConfig }` plus the per-group sub-structs (doc 08).

Events: none.

Threading: called once at startup.

## discovery

UDP broadcast peer discovery.

Public API:

- `fn spawn(runtime: &Runtime, tx: EventSender) -> JoinHandle<()>`

Responsibilities:

- Periodically broadcast `DISCOVER` (envelope) to `255.255.255.255:48271`, TTL=1.
- Listen on UDP port 48271; respond with `DISCOVER_RESPONSE` (protocol version, listening port, platform).
- Maintain the peer roster; mark peers stale after 45 s without a response.

Events:

- Emits: `DeviceDiscovered`, `PeerListUpdated`.
- Subscribes to: none.

Threading: dedicated Tokio task using `tokio::net::UdpSocket`.

## messaging

WebSocket transport for synchronization sessions.

Public API:

- `fn spawn(runtime: &Runtime, config: &NetworkConfig, tx: EventSender) -> JoinHandle<()>`
- `fn connect_to(addr: SocketAddr, peer: &PeerInfo)` (called by Sync Engine via channel)

Responsibilities:

- Run a WebSocket server on port 48272.
- Connect as a WebSocket client to discovered peers.
- Perform HELLO / HELLO_ACK handshake; reject unsupported versions (ErrorCode 1).
- Enforce connection-state model: unknown devices enter **Pairing** state (only PAIRING_* + PING/PONG allowed); clipboard messages rejected with ErrorCode 2 pre-trust.
- Send/receive envelopes; deliver inbound messages to the Sync Engine.
- Heartbeat PING every 15 s; mark a peer stale after 45 s without PONG.
- Reconnect to trusted peers with backoff (1 s, 5 s, 15 s, 60 s cap).
- Enforce the 2 MB maximum frame size.

Events:

- Emits: `DeviceConnected`, `DeviceDisconnected`, `DeviceConnectionFailed`, `NetworkError`.
- Subscribes to: `SyncRequested` (outbound CLIPBOARD_UPDATE delivery).

Threading: one Tokio task per active connection plus a listener task.

## pairing

Trust establishment.

Public API:

- `fn request_pairing(&self, device_id: &Uuid) -> Result<()>` — sends PAIRING_REQUEST to an untrusted device (user-initiated).
- `fn approve(&self, device_id: &Uuid) -> Result<()>`
- `fn reject(&self, device_id: &Uuid, reason: &str) -> Result<()>`
- `fn pending_requests(&self) -> Vec<PairingRequest>`

Responsibilities:

- Outbound: `request_pairing` opens a connection (if not already connected) and sends PAIRING_REQUEST.
- Inbound: when a connection arrives from an untrusted device, surface the PAIRING_REQUEST to the UI via events.
- Surface pending requests to the UI; apply the user's approve/reject decision.
- Persist the trusted peer on accept; time out requests after 60 s (ErrorCode 11).

Events:

- Emits: `PairingRequested`, `PairingAccepted`, `PairingRejected`, `PairingFailed`.
- Subscribes to: UI approval/rejection.

Threading: state owned by the Sync Engine task; UI approval arrives over the event channel.

## replay_cache

Replay and duplicate-message protection.

Public API:

- `fn check_and_record(message_id: &Uuid, content_hash: Option<&str>) -> Result<bool>` — returns true if already seen (reject), else records and returns false.
- `fn prune(now: DateTime<Utc>)` — drops entries older than 1 h.

Precedence: message-ID replay check first (ErrorCode 8: DUPLICATE_MESSAGE) → content-hash dedup (DuplicateClipboardIgnored). Timestamp skew (±300 s) applies to the envelope timestamp.

Configuration: capacity 1,000 message IDs; 1 h TTL.

Threading: owned by the Sync Engine task; guarded by a small Mutex if shared.

## sync_engine

Core orchestration and synchronization decisions.

Public API (UniFFI-exported):

- `fn on_clipboard_changed(&self, content: String) -> Result<()>` — called by platform clipboard monitors. The engine assigns Clipboard ID and content hash.
- `fn restore_history_entry(&self, clipboard_id: &Uuid) -> Result<()>` — copies the entry back to the local clipboard.
- `fn set_paused(&self, paused: bool) -> Result<()>`
- `fn is_paused(&self) -> bool`
- `fn sync_status(&self) -> SyncStatus`

Responsibilities:

- Process inbound envelopes: validate, replay-check (ErrorCode 8), deduplicate by content hash (DuplicateClipboardIgnored), persist, apply to clipboard, send CLIPBOARD_ACK, and never redistribute remotes.
- Process local `ClipboardChanged` events: compute hash, assign Clipboard ID, deduplicate, persist, broadcast to all eligible peers via `SyncRequested`.
- **Sole writer to clipboard history** — calls `storage.append_history` / `evict_history` directly.
- Prevent loops via origin-device tracking (`origin_device_id` on CLIPBOARD_UPDATE) + content-hash dedup.
- Coordinate pairing and connection lifecycle through the modules above.
- Enforce pause: when paused, ignore inbound clipboard messages and suppress outbound broadcasts.

Events:

- Emits: `ClipboardUpdatedFromRemote`, `ClipboardHistoryUpdated`, `SyncCompleted`, `DuplicateClipboardIgnored`, `ProtocolError`.
- Subscribes to: `ClipboardChanged` (from monitor), `DeviceDisconnected`, `NetworkError`, `ProtocolError`, inbound protocol messages from Messaging.

Threading: single owner task in the Tokio runtime; all state mutations happen on this task to avoid locking.

## history_manager

Local clipboard history.

Public API (UniFFI-exported):

- `fn list(limit: usize) -> Vec<HistoryEntry>`
- `fn clear() -> Result<()>`

Responsibilities:

- Provide listing and restoration queries for the UI.
- History persistence (append, eviction) is owned by the Sync Engine, which calls `storage.append_history` / `evict_history` directly.

Events:

- Subscribes to: none (queries only).

Threading: runs on the Sync Engine task; synchronous SQLite calls.

## bridge (UniFFI)

The interface exported to Kotlin for the Android app.

Public API (UDL-defined, implemented by `runtime`, `sync_engine`, `history_manager`, `pairing`, `storage`):

- `start(config) / stop()`
- `set_event_callback(callback)` — pushes internal events to Kotlin.
- `device_list() -> List<DeviceInfo>`
- `request_pairing(device_id)` — initiate pairing with a discovered device.
- `approve_pairing(device_id) / reject_pairing(device_id)`
- `set_paused(paused: bool) / is_paused() -> bool`
- `history(limit) -> List<HistoryEntry>`
- `restore_history_entry(clipboard_id)`
- `get_setting(key) / set_setting(key, value)`

Threading: UniFFI callbacks are invoked on the core event loop; the Kotlin side marshals them to the main/UI thread.

---

# clipboard-tui

Desktop front-end binary.

## clipboard_monitor

arboard-based clipboard access.

Public API:

- `fn spawn(tx: EventSender) -> JoinHandle<()>` — polls the OS clipboard every 250 ms; emits `ClipboardChanged` on external changes. (arboard has no OS-notification API on all platforms; polling is the fallback.)
- `fn set_clipboard(content: &str) -> Result<()>` — applies remote/restored content.

Events:

- Emits: `ClipboardChanged`.
- Subscribes to: instructions from the Sync Engine to set clipboard content.

Threading: its own Tokio task; arboard calls are short and on a dedicated thread to avoid blocking the runtime.

## tui

ratatui user interface.

Public API:

- `fn run(app: AppHandle) -> Result<()>` — event loop driving render + input.

Responsibilities: render device list, history, status bar; handle keybindings (doc 09); call core APIs for pairing approval and restoration.

Events:

- Subscribes to: `DeviceDiscovered`, `DeviceConnected`, `DeviceDisconnected`, `PeerListUpdated`, `ClipboardHistoryUpdated`, `SyncCompleted`, pairing events.
- Emits: pairing approve/reject, restore requests (via core API calls rather than events).

Threading: runs on the main thread; renders in a loop; reads events from the core channel.

---

# Android app (Kotlin)

## clipboard_capture_service (AccessibilityService)

Global clipboard capture.

Responsibilities:

- Declared in the manifest with `canRetrieveWindowContent`; user enables it manually in Android settings.
- On accessibility events, read clipboard text and forward it to the core via `on_clipboard_changed`.
- Handle the Android 13+ clipboard-read toast interaction.

Threading: Android accessibility callback thread; forwards to the core over the UniFFI boundary.

## foreground_service

Keepalive service.

Responsibilities:

- Run a foreground service of type `dataSync` with a persistent notification.
- Keep the WebSocket connection alive; tolerate Doze via partial wakelocks only while syncing.

## clipboard_writer (Kotlin)

Receives remote clipboard updates and writes them to the Android clipboard.

Responsibilities:

- Subscribes to `ClipboardUpdatedFromRemote` events from the core.
- Writes the content to `ClipboardManager` on the main thread.
- Emits a local `ClipboardChanged` after writing (this is treated as a local update; the engine deduplicates it and does not re-broadcast).

Threading: event callback runs on the core event loop; clipboard write dispatched to the Android main thread via `Handler(Looper.getMainLooper())`.

## ui (Compose screens)

Screens and flows specified in doc 09:

- Device list (discovered + trusted)
- Pairing approval
- History list + restore
- Settings
- Status / notification

Events: subscribes to the same core events as the TUI; marshals them onto the UI thread.