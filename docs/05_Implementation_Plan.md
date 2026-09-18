# Implementation Plan

## Overview

This document defines the build order, milestones, task breakdown, and verification strategy for Clipboard Sync (MVP).

The project is a Rust workspace containing a platform-independent core library, a desktop TUI front-end, and an Android GUI front-end that consumes the core through UniFFI-generated bindings.

## Repository Layout

```
shared-clipboard/
├── Cargo.toml                  # workspace manifest
├── crates/
│   ├── clipboard-core/         # sync engine, networking, storage, events (platform-independent)
│   │   ├── src/
│   │   ├── src/bin/            # two-node integration harness binaries (dev only)
│   │   ├── Cargo.toml
│   │   └── tests/              # integration tests
│   ├── clipboard-tui/          # desktop front-end binary (ratatui + arboard)
│   │   ├── src/
│   │   └── Cargo.toml
│   └── clipboard-proto/        # protocol + event type definitions (leaf crate)
│       ├── src/
│       └── Cargo.toml
├── android/                    # Android app (Kotlin/Compose)
│   ├── app/
│   ├── core/                   # generated UniFFI bindings live here
│   └── gradle/...
├── docs/                       # specification documents
└── scripts/                    # dev tooling (codegen, test harness helpers)
```

Dependency direction: `clipboard-proto` is a leaf crate; `clipboard-core` depends on it; `clipboard-tui` and the Android app depend on `clipboard-core`. Nothing depends on a front-end.

## Build Phases

### Phase A — Foundation

Goal: a compiling workspace with the protocol types, core skeleton, and storage layer.

Tasks:

1. Initialize the Cargo workspace and the three crates.
2. Define all protocol message types, the message envelope, and internal event types in `clipboard-proto`.
3. Implement serialization (serde/serde_json) for every message type with unit tests.
4. Implement the error type and the error code registry mapping (see doc 08).
5. Implement the SQLite storage layer: schema creation, migrations, and CRUD for device config, trusted peers, clipboard history, and settings (see doc 07).
6. Implement device identity creation and persistence (UUIDv4).
7. Implement configuration loading with defaults (see doc 08).

Milestone: `cargo test` passes; workspace builds cleanly.

### Phase B — Networking

Goal: devices can discover each other, connect, and establish trust.

Tasks:

1. Implement the Discovery Service over UDP broadcast on port 48271:
   - Broadcast DISCOVER every 5 seconds (TTL=1).
   - Listen for DISCOVER and reply with DISCOVER_RESPONSE.
   - Maintain a peer roster with staleness detection (45 s without a response).
2. Implement the Messaging Service:
   - WebSocket server listening on port 48272.
   - WebSocket client connection to discovered peers.
   - HELLO / HELLO_ACK handshake with protocol version validation.
   - PING / PONG heartbeat every 15 s.
   - Reconnect with backoff (1 s, 5 s, 15 s, 60 s cap) to trusted peers.
3. Implement the Pairing Manager:
   - Handle inbound PAIRING_REQUEST; surface pending requests to the UI via events.
   - Implement `request_pairing(device_id)` for outbound pairing initiation (user taps "Pair").
   - Approve or reject pairing; persist trusted peers after PAIRING_ACCEPT.
   - Enforce connection-state model: pairing-mode sessions allow only PAIRING_* + PING/PONG; clipboard messages rejected pre-trust (ErrorCode 2).
   - Time out pending requests after 60 s (ErrorCode 11).
4. Implement inbound message validation and the replay cache (1,000 message IDs, 1 h TTL).

Milestone: two local processes discover each other, handshake, pair, and maintain heartbeats.

### Phase C — Synchronization

Goal: clipboard content synchronizes correctly and safely.

Tasks:

1. Implement the Sync Engine event loop:
   - Accept ClipboardChanged events from platform clipboard monitors.
   - Deduplicate by content hash; assign Clipboard ID; prevent echoes to origin devices.
   - Update history and broadcast CLIPBOARD_UPDATE to eligible peers.
   - Handle inbound CLIPBOARD_UPDATE: validate, dedup, persist, apply to local clipboard, send CLIPBOARD_ACK.
2. Implement the History Manager:
   - Append entries, enforce the 2 MB total cap with FIFO eviction.
   - Provide listing and restoration APIs.
3. Wire loop-prevention: entries received from a remote must never be redistributed to any peer (origin tracking + content-hash dedup).
4. Emit internal events (SyncCompleted, DuplicateClipboardIgnored, etc.) as specified in doc 03.
5. Implement pause/resume: `set_paused(bool)` persisted in `sync.enabled` setting; paused node neither broadcasts nor applies inbound updates.

Milestone: two-node harness on loopback synchronizes content both ways with no loops or duplicates; pause/resume toggle verified.

### Phase D — Desktop TUI

Goal: the desktop front-end is usable end-to-end.

Tasks:

1. Implement the arboard-based clipboard monitor: detect changes, read text, set text.
2. Implement the ratatui interface with device list, history view, status bar, and keybindings (see doc 09).
3. Implement pairing approval and history restoration flows in the TUI.
4. Add tracing logging to a local file and stderr; ensure clipboard content is never logged.

Milestone: two desktop instances (or one desktop + one harness node) synchronize end-to-end.

### Phase E — Android App

Goal: the Android app is usable end-to-end.

Tasks:

1. Define the UniFFI interface (UDL) at `crates/clipboard-core/src/clipsync.udl` covering:
   - start / stop the core runtime.
   - Event subscription (callback into Kotlin).
   - device list, pairing request/approve/reject, history list, restore, settings get/set.
   - `set_paused(bool)` / `is_paused()`.
2. Generate UniFFI bindings; add the `uniffi-bindgen` step to the build.
3. Implement the AccessibilityService for global clipboard capture:
   - Manifest declaration and user-facing setup instructions.
   - Listen for `TYPE_WINDOW_CONTENT_CHANGED` and focus events with debounce (250 ms).
   - Read clipboard text on qualifying events; feed into the core via `on_clipboard_changed`.
4. Implement the foreground service (type `dataSync`) for keepalive with a persistent notification.
   - Declare `FOREGROUND_SERVICE_DATA_SYNC` permission (required on Android 14+).
5. Implement the **clipboard writer** Kotlin component: subscribes to `ClipboardUpdatedFromRemote` events; writes content to `ClipboardManager`.
6. Implement the Jetpack Compose UI screens (see doc 09).
7. Implement settings persistence through the core settings API (no config file on Android).

Milestone: laptop ↔ phone synchronization works in both directions.

### Phase F — Hardening and Validation

Tasks:

1. Execute the full test strategy from doc 10.
2. Cross-device E2E testing (laptop ↔ Android).
3. Resource-usage pass: verify low CPU/RAM while idle.
4. Loop-prevention, replay, and oversized-payload test verification.
5. Documentation cross-check against the specifications.

Milestone: all success criteria from doc 01 are satisfied.

## Task Dependencies

```
clipboard-proto
      │
      ▼
clipboard-core ───────► clipboard-tui
 (storage → discovery   (depends on core)
  → messaging → sync)
      │
      ▼
 android app (via UniFFI, depends on core)
```

- Phase B depends on A. Phase C depends on B. Phase D depends on C.
- Phase E depends on C; it can run in parallel with D.
- Phase F depends on D and E.

## Testing Approach

- Unit tests live beside each module.
- Integration tests live in `clipboard-core/tests/` and drive the two-node harness over loopback.
- The Android app uses instrumented tests for the accessibility service contract and manual E2E for cross-device behavior.
- Full detail in doc 10.

## Tooling

- `cargo fmt`, `cargo clippy -- -D warnings`, `cargo test` are CI gates for the workspace.
- `uniffi-bindgen` generates Kotlin bindings during the Android build.
- Gradle builds the Android app; the Rust core is compiled as a static library for the Android targets (`aarch64-linux-android`, `armv7-linux-androideabi`, `x86_64-linux-android`).