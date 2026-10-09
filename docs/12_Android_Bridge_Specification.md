
# Android Bridge Specification

## 1. Overview

This document specifies the boundary between the platform-independent ClipSync Rust core and the Android application.

The Android application is a platform frontend. It is responsible for Android-specific concerns such as:

* clipboard access
* foreground/background lifecycle
* Android permissions and services
* user interface
* Android notifications
* presentation of synchronization state

The Rust core remains responsible for:

* peer discovery
* device identity
* pairing and trust
* network connections
* synchronization
* deduplication
* loop prevention
* clipboard history
* persistence
* synchronization state

The bridge exists to expose a small, stable API between these two layers.

The Android application must **not** depend directly on Tokio, `broadcast`, `AppCommand`, `Storage`, internal coordinator types, or other Rust implementation details.

---

# 2. Architectural Position

The Android application follows this structure:

```text
┌─────────────────────────────────────────────┐
│                 Android UI                  │
│              Jetpack Compose               │
└──────────────────────┬──────────────────────┘
                       │
                       ▼
┌─────────────────────────────────────────────┐
│                 ViewModel                   │
│        UI state + user interactions         │
└──────────────────────┬──────────────────────┘
                       │
                       ▼
┌─────────────────────────────────────────────┐
│                Repository                   │
│       Android-facing application API        │
└──────────────────────┬──────────────────────┘
                       │
                       ▼
┌─────────────────────────────────────────────┐
│              ClipSync Bridge                │
│       UniFFI-generated Kotlin API           │
└──────────────────────┬──────────────────────┘
                       │
═══════════════════════╪═══════════════════════
                       │ FFI
═══════════════════════╪═══════════════════════
                       │
                       ▼
┌─────────────────────────────────────────────┐
│              ClipSync Rust Core             │
│                                             │
│ Coordinator │ Discovery │ Messaging │ Store │
└─────────────────────────────────────────────┘
```

Platform-specific clipboard access remains outside the bridge:

```text
Android Clipboard
       ↕
Clipboard Service
       ↕
Android Repository
       ↕
ClipSync Bridge
       ↕
Rust Core
```

The bridge is therefore an **application boundary**, not a replacement for the core architecture.

---

# 3. Design Goals

The bridge must satisfy the following goals.

### 3.1 Minimal API surface

Expose only operations required by the Android application.

Internal Rust implementation details must not become part of the Android API.

### 3.2 Stable data contract

Android-facing objects should remain stable even if internal Rust structures change.

For example, changing the internal `TrustedPeer` representation must not require exposing the new Rust representation to Kotlin.

### 3.3 Asynchronous event delivery

Rust events must be delivered continuously to Android without requiring Kotlin to poll the core.

### 3.4 Explicit lifecycle

Starting and stopping the core must have deterministic behavior.

### 3.5 Thread safety

The bridge must be safe to call from Android background threads and must never require Kotlin UI code to understand Rust runtime/threading behavior.

### 3.6 Platform independence

The Rust core must remain usable by the desktop TUI without requiring Android or UniFFI.

---

# 4. Technology

The Android bridge will use **UniFFI**, driven by an explicit UDL file at `crates/clipboard-core/src/clipsync.udl` with a `uniffi-bindgen` generation step, as defined in doc 05 Phase E. The exact UniFFI version and generated-binding workflow must be pinned in the Rust dependency configuration before implementation.

The bridge should live in `clipboard-core` rather than in `clipboard-proto`.

However, Android ships a shared library (`.so`), so Phase E additionally requires a `cdylib` build target — either a dedicated FFI crate (for example `crates/clipboard-ffi`) depending on `clipboard-core`, or an additional crate type on an existing crate. The bridge *module* remains in `clipboard-core`; the `cdylib` target is a packaging detail consistent with the UDL placement defined in doc 05 Phase E.

Reason:

* `clipboard-proto` remains a protocol/data-model crate.
* `clipboard-core` owns application lifecycle and orchestration.
* The bridge exposes the core to frontends.
* The Android application depends on the core through the bridge.

Conceptually:

```text
clipboard-proto
       ▲
       │
clipboard-core
       │
       ├──────── clipboard-tui
       │
       └──────── Android / UniFFI
```

---

# 5. Bridge API

The Android-facing API is divided into the following areas:

```text
ClipSyncCore
│
├── Lifecycle
│   ├── start
│   └── stop
│
├── Identity
│   └── identity
│
├── Devices
│   ├── trustedPeers
│   ├── discoveredPeers
│   └── forgetDevice
│
├── Pairing
│   ├── pendingPairingRequests
│   ├── requestPairing
│   ├── approvePairing
│   └── rejectPairing
│
├── Clipboard
│   ├── onClipboardChanged
│   └── restoreHistoryEntry
│
├── History
│   ├── history
│   ├── clearHistory
│   └── historySizeBytes
│
├── Synchronization
│   ├── isPaused
│   └── setPaused
│
└── Events
    └── subscribe
```

The exact UniFFI syntax is an implementation detail and must not be considered part of this specification.

---

# 6. Lifecycle

## 6.1 Start

The bridge must provide a way to create and start a core instance.

Conceptually:

```text
ClipSyncCore.start(config)
```

The Android application must not construct the internal `AppConfig` directly.

Instead, the bridge should accept a bridge-specific configuration object containing only fields appropriate for Android.

For example:

```text
BridgeConfig
├── storagePath
├── discoveryPort
├── listenPort
├── discoveryTargets
└── syncEnabled
```

The bridge converts this into the internal `AppConfig`.

`deviceName` is intentionally **not** part of `BridgeConfig`. The internal `AppConfig` has no device-name field; device identity and name are seeded by Rust storage on first run (a generated UUID plus the host name). On Android that host-name default will be poor (typically `localhost`), which raises the priority of the deferred `setDeviceName()` operation described in Section 25. Until that core API exists, Android displays the name returned by `identity()` and cannot change it.

This prevents internal configuration structures from becoming FFI contracts.

---

## 6.2 Stop

The bridge must provide:

```text
core.stop()
```

Stopping must:

1. request a graceful shutdown from the Rust core;
2. stop network services;
3. terminate background tasks owned by the core;
4. emit the appropriate stopping event;
5. release the core runtime;
6. terminate event delivery.

The Android application must be able to safely stop the core when its foreground service is terminated.

---

## 6.3 Start/stop ownership

The Android application must have a single owner of the running core instance.

Recommended model:

```text
ClipSyncApplication
        │
        ▼
CoreManager
        │
        └── ClipSyncCore
```

UI components must not independently create core instances.

This prevents multiple Rust runtimes from accidentally operating against the same database and network ports.

---

# 7. Identity API

The bridge exposes the current device identity.

Conceptually:

```text
identity() -> DeviceIdentity
```

The Android representation should contain:

```text
DeviceIdentity
├── deviceId: String
└── deviceName: String
```

UUIDs cross the FFI boundary as strings.

The Android application must not need to understand Rust's `Uuid` type.

The bridge is responsible for parsing and validating UUIDs when Android passes device IDs back into Rust.

---

# 8. Device API

The Android UI needs information about trusted devices.

Conceptually:

```text
trustedPeers() -> List<BridgeDevice>
discoveredPeers() -> List<BridgeDevice>
forgetDevice(deviceId)
```

`discoveredPeers()` returns the current discovery roster (devices seen on the network that are not necessarily trusted). It is served by the `ListDiscoveredPeers` `AppCommand`, which answers with the coordinator's cached discovery roster (each entry carries `is_trusted` from storage and `is_connected` from the coordinator's live connected set). This provides the snapshot the Android "Nearby devices" UI needs for the initial-state requirements in Section 30.

The bridge-facing peer representation should contain only UI-relevant information.

Example:

```text
BridgeDevice
├── deviceId: String
├── deviceName: String
├── platform: Platform
├── isTrusted: Boolean
├── isConnected: Boolean
└── lastSeen: Long?
```

Timestamps use Unix milliseconds.

The bridge should not expose:

* `DateTime<Utc>`
* `SocketAddr`
* internal storage structures
* WebSocket state
* coordinator state

---

# 9. Connected State

The current Rust `TrustedPeer` model describes persisted trust but does not itself represent the complete live connection state.

Therefore `isConnected` must not be fabricated from persisted trusted-peer data.

The core exposes live connection state through the `ListConnectedDeviceIds` `AppCommand` (the coordinator's authoritative connected set). The bridge seeds `isConnected` from this snapshot and then maintains it from the event stream:

```text
DeviceConnected      → set deviceId connected
DeviceDisconnected   → set deviceId disconnected
```

The snapshot avoids a cold-start window in which every device is reported disconnected; the events keep the state current between snapshots. The source is always live coordinator state — never persisted trust data.

`PeerListUpdated` must **not** be used as a connection-state source. It is emitted by discovery with the *discovered* roster, and its payload currently hard-codes `is_connected: false` and `is_trusted: false`. Its proper role is populating `discoveredPeers()`, not `isConnected`.

The Android-facing representation should therefore distinguish:

```text
Trusted
Connected
Discovered
```

These are different concepts.

A device may be:

```text
Discovered + Not Trusted + Not Connected
```

or:

```text
Trusted + Connected
```

or:

```text
Trusted + Not Connected
```

The Android UI must not infer trust solely from connection state.

---

# 10. Pairing API

The bridge exposes:

```text
requestPairing(deviceId)
approvePairing(deviceId)
rejectPairing(deviceId, reason)
pendingPairingRequests()
```

Pairing request representation:

```text
BridgePairingRequest
├── requestId: String
├── deviceId: String
├── deviceName: String
├── platform: Platform
└── receivedAt: Long
```

The request ID and device ID are strings at the Android boundary.

The bridge converts them to UUIDs internally.

---

# 11. Clipboard Input

Android sends local clipboard changes into the core through:

```text
onClipboardChanged(content)
```

The bridge currently maps this to the existing core API:

```text
AppHandle::on_clipboard_changed()
```

The current core treats these inputs as:

```text
content_type = "text/plain"
```

The MVP therefore exposes text clipboard input only.

The bridge must not allow arbitrary content types until the core explicitly supports additional clipboard formats.

---

# 12. Remote Clipboard Events

Remote clipboard updates are delivered through the event stream.

The Android event representation should contain:

```text
ClipboardUpdatedFromRemote
├── clipboardId: String
├── originDeviceId: String
├── content: String
├── contentType: String
└── contentHash: String
```

The Android clipboard writer consumes this event and writes the content to Android's `ClipboardManager`.

The writer must not call the network/core directly.

```text
Rust Core
    │
    │ ClipboardUpdatedFromRemote
    ▼
Core Event Collector
    │
    ▼
Repository
    │
    ▼
Clipboard Writer
    │
    ▼
Android ClipboardManager
```

The resulting local clipboard change is expected to pass through the normal Android clipboard-monitoring path.

The core's existing deduplication and origin tracking must prevent the remote write from being rebroadcast.

---

# 13. History API

The bridge exposes:

```text
history(limit)
clearHistory()
restoreHistoryEntry(clipboardId)
historySizeBytes()
```

Android representation:

```text
BridgeHistoryEntry
├── clipboardId: String
├── content: String
├── contentType: String
├── contentHash: String
├── originDeviceId: String
├── createdAt: Long
└── sizeBytes: Long
```

The bridge converts:

```text
Uuid       → String
DateTime   → Unix milliseconds
u64        → Long
```

### Restore mechanism

`restoreHistoryEntry(clipboardId)` does **not** deliver clipboard content. The core's `ClipboardRestored` event carries only the clipboard ID, and the core does not write any platform clipboard — it records the entry's hash in the replay cache (so the resulting local write will be deduplicated) and emits the event.

Therefore the Android restore procedure is:

```text
UI invokes restoreHistoryEntry(clipboardId)
        │
        ▼
Rust core records replay hash, emits ClipboardRestored { clipboardId }
        │
        ▼
Android writes the entry content to the clipboard itself,
using the history entry already loaded in its repository state,
through the ClipboardWriter
        │
        ▼
Clipboard monitor observes the write
        │
        ▼
Rust core recognizes the recorded hash and does not rebroadcast
```

This mirrors the desktop TUI, which calls `restore_history_entry()` and writes the content to the clipboard itself from its loaded history entry.

A missing history entry surfaces as a `NotFound` error (Section 20): `restoreHistoryEntry` now returns a result, and the core reports missing entries rather than logging a silent warning.

The Android UI must not depend on SQLite or the Rust `Storage` type.

---

# 14. Synchronization State

The bridge exposes:

```text
isPaused()
setPaused(paused)
```

The Android application uses this for the Settings screen and synchronization status.

The MVP does not require Android to manipulate internal synchronization state directly.

A future bridge API may expose a richer status object:

```text
SyncState
├── paused
├── connectedPeerCount
├── pendingSyncCount
└── lastSyncAt
```

This should only be added when the UI requires it.

---

# 15. Event Architecture

## 15.1 Requirement

The Rust core already exposes an internal:

```text
broadcast::Sender<Event>
```

This is appropriate for Rust consumers, but it must not be exposed directly to Kotlin.

The Android bridge must create a dedicated long-lived subscription mechanism.

The desired flow is:

```text
Rust Event Bus
      │
      ▼
Bridge Event Subscription
      │
      ▼
UniFFI Boundary
      │
      ▼
Kotlin Event Collector
      │
      ▼
Repository StateFlow
      │
      ▼
ViewModel
      │
      ▼
Compose
```

---

# 16. Event Subscription Lifetime

The current `AppHandle::next_event()` and `next_event_blocking()` APIs create a new broadcast receiver for each call.

They are therefore not suitable as the Android event mechanism.

The bridge should instead own a persistent receiver:

```text
AppHandle.event_tx()
        │
        ▼
bridge-owned receiver
        │
        ▼
long-lived forwarding task
```

The receiver remains alive for the lifetime of the bridge subscription.

When the subscription is cancelled:

1. the forwarding task terminates;
2. the receiver is dropped;
3. no further events are delivered to Kotlin.

The Android application must not need to know about `tokio::broadcast`.

---

# 17. Event Representation

Not every internal Rust event needs to cross the Android boundary.

The bridge should expose a curated event model.

Initial Android events:

```text
ApplicationStarted
ApplicationStopping

DeviceDiscovered
DeviceConnected
DeviceDisconnected
DeviceConnectionFailed

PairingRequested
PairingAccepted
PairingRejected
PairingFailed

ClipboardUpdatedFromRemote
ClipboardHistoryUpdated
ClipboardRestored

SyncCompleted
DuplicateClipboardIgnored

NetworkError
ProtocolError
```

Events that exist internally but provide no useful Android behavior should remain internal.

For example:

```text
SyncRequested
ClipboardChanged
ClipboardRestoreRequested
```

may remain internal unless a concrete Android use case requires them.

---

# 18. Android Event DTO

The Android event should be represented by a bridge-owned event model rather than exposing the Rust `Event` and `EventType` hierarchy directly.

Conceptually:

```text
BridgeEvent
├── eventId: String
├── timestamp: Long
└── type: BridgeEventType
```

`BridgeEventType` contains the Android-safe payload for each event.

The exact generated Kotlin representation is an implementation detail.

The important requirement is that Android does not depend on:

```text
clipboard_proto::event::Event
clipboard_proto::event::EventType
```

directly.

---

# 19. Event-to-State Conversion

Events should not normally be consumed directly by every Compose screen.

Instead:

```text
Bridge Event
      │
      ▼
CoreEventCollector
      │
      ▼
Repository
      │
      ├── devices StateFlow
      ├── pairingRequests StateFlow
      ├── history StateFlow
      ├── syncState StateFlow
      └── errors/events
             │
             ▼
          ViewModels
```

This prevents UI components from becoming coupled to the Rust event model.

---

# 20. Error Handling

Bridge methods must use structured errors where possible.

The Android layer should distinguish at least:

```text
InvalidArgument
CoreNotRunning
CoreStopped
StorageError
OperationFailed
InvalidDeviceId
NotFound
InternalError
```

Errors should not expose Rust implementation details such as:

```text
oneshot channel closed
broadcast receiver lagged
tokio runtime failure
SQLite connection internals
```

Those details belong in Rust logs.

The Android-facing error should provide a stable message/code suitable for UI handling.

---

# 21. Threading Rules

The bridge must assume Android calls may originate from arbitrary threads.

Rules:

1. UI code must never block on Rust operations.
2. Potentially blocking bridge operations must be called from Kotlin background coroutines.
3. Rust bridge methods must not hold global locks while waiting for coordinator responses.
4. Event forwarding must run independently of the Android main thread.
5. Clipboard writes must occur through the Android-side clipboard component according to Android's threading requirements.
6. The Rust Tokio runtime remains an internal implementation detail.

Example:

```text
Compose
   │
   ▼
ViewModel
   │
   ▼
Dispatchers.IO
   │
   ▼
Bridge
   │
   ▼
Rust Core
```

---

# 22. Runtime Ownership

The Rust core currently owns a Tokio runtime inside `AppHandle`.

The bridge must preserve this ownership model.

Android must not create or manage a Tokio runtime.

The desired ownership is:

```text
ClipSyncCore
    │
    └── AppHandle
          │
          └── Tokio Runtime
```

The Android process owns the `ClipSyncCore` object.

The Rust core owns its runtime.

This keeps runtime lifecycle deterministic and prevents Android lifecycle code from becoming coupled to Tokio.

---

# 23. Storage Configuration

Android should use an Android-specific storage location.

The bridge must not rely on Rust's desktop `dirs` behavior to determine the application's storage path.

The Android application should determine an appropriate application-private directory and provide it to the bridge.

Conceptually:

```text
Android Context
      │
      ▼
applicationContext.filesDir
      │
      ▼
BridgeConfig.storagePath
      │
      ▼
Rust AppConfig.storage.path
```

This is particularly important because the Termux environment and normal Android application sandbox have different filesystem layouts.

---

# 24. Configuration Boundary

The internal `AppConfig` remains a Rust type.

The bridge should not expose:

```text
AppConfig
NetworkConfig
HistoryConfig
SyncConfig
StorageConfig
PlatformConfig
```

directly.

Instead, define a bridge configuration DTO containing only values Android is allowed to configure.

The bridge converts:

```text
BridgeConfig
       ↓
AppConfig
```

This allows Rust configuration internals to evolve without breaking Kotlin bindings.

---

# 25. Device Name

The Android UI must be able to display and eventually modify the local device name.

However, the existing `AppHandle` API currently exposes identity retrieval but not a device-name mutation operation.

Therefore changing the device name is **not part of the first bridge implementation**.

The first bridge milestone exposes:

```text
identity()
```

A future core API may add:

```text
setDeviceName(name)
```

Only after the underlying storage and networking behavior has been defined should this be added to the bridge.

---

# 26. Lifecycle With Android Services

The Android architecture will eventually contain:

```text
ClipSyncApplication
        │
        ├── CoreManager
        │      └── ClipSyncCore
        │
        └── Repository

SyncForegroundService
        │
        └── keeps application sync operation alive

ClipboardAccessibilityService
        │
        └── captures clipboard changes
```

The services should not each create their own core instance.

Instead:

```text
                CoreManager
                    │
              ClipSyncCore
              /          \
             /            \
Clipboard Service      Foreground Service
```

The exact Android lifecycle coordination will be specified separately in the Android service implementation document.

---

# 27. Initial Kotlin-facing API

The intended API should feel approximately like:

```kotlin
val core = ClipSyncCore.start(config)

core.identity()

core.trustedPeers()

core.discoveredPeers()

core.pendingPairingRequests()

core.requestPairing(deviceId)

core.approvePairing(deviceId)

core.rejectPairing(deviceId, reason)

core.onClipboardChanged(content)

core.history(limit)

core.restoreHistoryEntry(clipboardId)

core.clearHistory()

core.historySizeBytes()

core.isPaused()

core.setPaused(true)

core.subscribe(listener)

core.stop()
```

This is an API contract, not final Kotlin syntax.

Generated UniFFI types and lifecycle wrappers may alter the exact shape.

---

# 28. Repository API

The Android repository should hide the generated UniFFI API from the rest of the application.

Conceptually:

```kotlin
interface ClipSyncRepository {

    val devices: StateFlow<List<Device>>

    val discoveredDevices: StateFlow<List<Device>>

    val pairingRequests: StateFlow<List<PairingRequest>>

    val history: StateFlow<List<HistoryItem>>

    val syncState: StateFlow<SyncState>

    suspend fun start()

    suspend fun stop()

    suspend fun requestPairing(deviceId: String)

    suspend fun approvePairing(deviceId: String)

    suspend fun rejectPairing(deviceId: String, reason: String)

    suspend fun forgetDevice(deviceId: String)

    suspend fun restoreHistory(id: String)

    suspend fun clearHistory()

    suspend fun setPaused(paused: Boolean)

    suspend fun onClipboardChanged(content: String)
}
```

The repository becomes the boundary between generated bindings and application code.

---

# 29. Android Event Collector

A dedicated collector translates bridge events into repository state.

Conceptually:

```text
Bridge
  │
  ▼
CoreEventCollector
  │
  ├── Device events
  │       ↓
  │   Device state
  │
  ├── Pairing events
  │       ↓
  │   Pairing state
  │
  ├── Clipboard events
  │       ↓
  │   Clipboard writer/history
  │
  ├── Sync events
  │       ↓
  │   Sync status
  │
  └── Error events
          ↓
      UI notifications
```

The collector should be long-lived while the core is running.

---

# 30. Events and Initial State

Events alone are insufficient to reconstruct all application state after Android starts.

Therefore the repository must establish initial state by querying the core:

```text
start core
   │
   ├── identity()
   ├── trustedPeers()
   ├── discoveredPeers()
   ├── pendingPairingRequests()
   ├── history()
   └── isPaused()
```

Connected state has no snapshot source: it starts empty (all devices disconnected) and is maintained exclusively by `DeviceConnected`/`DeviceDisconnected` events after subscription, as specified in Section 9.

After the initial snapshot is loaded, events maintain the state.

This produces:

```text
Initial Snapshot
      +
Event Stream
      =
Current Android State
```

---

# 31. Event Ordering

The bridge should preserve event ordering as observed by its single subscription.

The Android collector should process events sequentially.

Multiple independent collectors must not subscribe directly to the same Rust event stream if doing so can produce inconsistent state.

Preferred model:

```text
             Rust Event Stream
                    │
                    ▼
          Single CoreEventCollector
                    │
                    ▼
              Repository
             /    |     \
            /     |      \
       Devices  History  SyncState
```

This provides one authoritative event-processing path.

---

# 32. Event Loss

The Rust event system currently uses Tokio broadcast semantics.

A slow receiver may eventually encounter a lagged receiver condition.

The Android bridge must not silently treat this as normal operation.

If the bridge detects event loss:

1. report an event-stream error;
2. refresh repository state from the core;
3. resume event collection where possible.

The recovery model is:

```text
Event loss detected
        │
        ▼
Refresh snapshot
        │
        ▼
Resume event stream
```

The implementation should avoid silently presenting stale device/history state.

---

# 33. Security Boundary

The bridge does not create a new trust mechanism.

Device trust remains entirely controlled by the Rust core.

Android must never directly manipulate:

* trusted-peer database records;
* authentication tokens;
* pairing secrets;
* network connections;
* protocol messages.

Android can only invoke the defined pairing operations.

```text
Android
   │
   │ requestPairing()
   ▼
Rust Core
   │
   ├── Pairing Manager
   ├── Trust validation
   └── Persistence
```

---

# 34. Clipboard Privacy

The bridge must not log clipboard content.

This applies to:

* bridge calls;
* event conversion;
* Kotlin logging;
* error messages;
* debugging output.

Logging may include:

```text
clipboard ID
content size
origin device ID
event type
timestamp
```

but must never include clipboard text.

This follows the privacy requirements in the security and UI specifications.

---

# 35. Testing Requirements

The bridge implementation must include tests for:

### Lifecycle

* start succeeds with valid configuration;
* start fails cleanly with invalid configuration;
* stop succeeds;
* repeated stop does not panic;
* events stop after shutdown.

### Identity

* identity is returned correctly;
* UUID conversion is valid.

### Device API

* trusted peers are converted correctly;
* discovered peers are converted correctly;
* platform mapping is correct;
* timestamps convert correctly;
* connected state is derived correctly from connection events only.

### Pairing

* valid device IDs are accepted;
* invalid device IDs return bridge errors;
* pairing commands reach the core.

### Clipboard

* local clipboard input reaches the core;
* text content type is correctly assigned;
* clipboard content is never logged.

### History

* history conversion is correct;
* timestamps and sizes survive conversion;
* UUIDs survive round-trip conversion;
* restoring a missing entry returns a `NotFound` bridge error.

### Events

* internal events map to bridge events correctly;
* irrelevant internal events are filtered;
* event ordering is preserved;
* subscription cancellation stops delivery;
* event loss triggers state recovery.

---

# 36. Android Integration Test

The first Android bridge integration test should prove:

```text
Android/Kotlin
      │
      │ start
      ▼
Rust Core
      │
      │ identity
      ▼
Android/Kotlin
```

Then:

```text
Android
   │
   │ onClipboardChanged("hello")
   ▼
Rust Core
```

And finally:

```text
Rust Core
   │
   │ ClipboardUpdatedFromRemote
   ▼
Android Event Collector
```

No Compose UI is required for this test.

The bridge must first work independently of the UI.

---

# 37. Bridge Milestone Definition

Doc 15 is normative for milestone numbering and sequencing. This section defines the bridge's own completion criteria, which correspond to Milestone 2 (UniFFI Bridge Foundation) and the bridge portions of Milestone 3 (Event Pipeline) in that plan.

The bridge milestone is complete when all of the following are true:

* UniFFI is integrated.
* Android can create a core instance.
* Android can start and stop the core.
* Android can retrieve device identity.
* Android can retrieve trusted peers.
* Android can retrieve discovered peers.
* Android can retrieve history.
* Android can send clipboard text to the core.
* Android can invoke pairing operations.
* Android can pause/resume synchronization.
* Android receives a long-lived event stream.
* Events are represented using bridge-owned DTOs.
* No Tokio or internal Rust synchronization types cross the boundary.
* Bridge errors are represented using Android-safe types.
* Bridge unit tests pass.
* A minimal Android integration harness can start the core and receive events.

The Android UI is **not** part of this milestone.

---

# 38. Android Platform Integration

After the bridge is stable, implementation proceeds to Android-specific functionality. Doc 15 is normative for the ordering; in its numbering the sequence is:

```text
Milestone 2
UniFFI bridge foundation
       │
       ▼
Milestone 3
Event pipeline
       │
       ▼
Milestone 4
CoreManager / application lifecycle
       │
       ▼
Milestone 5
Foreground service
       │
       ▼
Milestone 6
Clipboard capture / writer
       │
       ▼
Milestones 7–10
Repository, pairing, history, Compose UI
       │
       ▼
Milestone 12
Cross-device E2E
```

The clipboard capture mechanism and foreground service must therefore not be implemented simultaneously with the initial bridge.

Keeping these concerns separate makes bridge failures easier to diagnose.

---

# 39. Non-Goals

The first bridge implementation will not:

* expose raw Rust structs;
* expose Tokio;
* expose `broadcast::Receiver`;
* expose `AppCommand`;
* expose SQLite;
* expose WebSocket connections;
* expose protocol messages;
* expose internal coordinator state;
* implement Android clipboard access;
* implement Compose screens;
* implement Android notifications;
* implement device-name mutation;
* support non-text clipboard formats.

These remain outside the first bridge milestone.

---

# 40. Final Architecture

The completed architecture should look like:

```text
                         ANDROID
┌─────────────────────────────────────────────────────────┐
│                                                         │
│  Compose UI                                             │
│      │                                                  │
│      ▼                                                  │
│  ViewModels                                             │
│      │                                                  │
│      ▼                                                  │
│  Repository                                             │
│      │                                                  │
│      ▼                                                  │
│  UniFFI Bridge                                          │
│      │                                                  │
│      │ FFI                                              │
└──────┼──────────────────────────────────────────────────┘
       │
       ▼
┌─────────────────────────────────────────────────────────┐
│                    RUST CORE                            │
│                                                         │
│  AppHandle / Bridge Adapter                             │
│          │                                              │
│          ▼                                              │
│     Coordinator                                         │
│       │      │                                          │
│       ▼      ▼                                          │
│  Discovery Messaging                                    │
│       │      │                                          │
│       └──┬───┘                                          │
│          ▼                                              │
│     Sync / Pairing                                      │
│          │                                              │
│          ▼                                              │
│       Storage                                           │
│                                                         │
└─────────────────────────────────────────────────────────┘
       │
       ▼
Android platform services
       │
       ├── Clipboard capture
       ├── Clipboard writer
       └── Foreground service
```

The fundamental rule is:

> **Android owns Android concerns. Rust owns synchronization concerns. The bridge owns the boundary between them.**
