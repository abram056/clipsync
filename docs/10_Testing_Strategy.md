# Testing Strategy

## Overview

This document defines how Clipboard Sync is verified at each level: unit, integration, cross-device end-to-end, and platform-specific Android testing. It also defines the CI gates.

## Principles

- The platform-independent core must be tested without a real UI or clipboard.
- Network tests use loopback and the two-node harness; real network behavior is validated in cross-device E2E.
- Every test that touches sync correctness verifies **both directions** and the **absence of loops/duplicates**.

## Test Levels

### 1. Unit Tests

Located beside the code (`crates/*/src/**`), run with `cargo test`.

Coverage targets:

| Module | Cases |
| --- | --- |
| clipboard-proto | Envelope round-trip (serialize → deserialize), message field validation, error code mapping. |
| storage | Schema migration idempotency, CRUD, trusted-peer lifecycle, eviction math, hash uniqueness. |
| config | Defaults, TOML overlay, precedence (SQLite > file > defaults), invalid-value rejection. |
| replay_cache | Capacity eviction, TTL pruning, duplicate detection, message-ID precedence over content-hash. |
| discovery | Roster add/stale/timeout behavior (pure logic extracted for testing). |
| sync_engine | Dedup by content hash, loop prevention (remote entries never redistributed), origin tracking via `origin_device_id`. |
| history_manager | Append, cap enforcement, FIFO eviction by insertion order (`id ASC`), retention of newest entry. |
| bridge | UniFFI type mapping; event callback payloads. |

### 2. Integration Tests — Two-Node Harness

Located in `crates/clipboard-core/tests/`. The harness runs two `clipboard-core` instances on loopback with programmatic clipboard mocks (inject `on_clipboard_changed`; capture applied clipboard writes).

Required scenarios:

| Scenario | Verifies |
| --- | --- |
| Discovery | Node B is discovered by node A; roster populated; stale peer removed after 45 s. |
| Handshake & pairing-mode | HELLO/HELLO_ACK; untrusted connection enters pairing state; clipboard messages rejected pre-trust (ErrorCode 2). |
| Pairing (requester-initiated) | User calls `request_pairing`; PAIRING_REQUEST sent; approve → trusted; reject → connection closed. |
| Bidirectional sync | A copies → B receives; B copies → A receives. |
| No loop | A copies → B receives → A must NOT receive its own content back. |
| No duplicate | Same content copied twice locally produces one entry, one broadcast. |
| Duplicate suppression | Replaying a captured CLIPBOARD_UPDATE is ignored (ErrorCode 8, replay cache). |
| Content-hash dedup | Same content hash from same origin produces DuplicateClipboardIgnored. |
| Oversized payload | 2 MB + 1 byte payload is rejected with ErrorCode 7. |
| Version mismatch | Peer announcing protocol v2 is rejected (ErrorCode 1). |
| Heartbeat | PING/PONG keep-alive; peer marked stale after 45 s silence. |
| Reconnect | Peer killed and restarted; automatic reconnection with backoff. |
| History | Cap enforced (2 MB FIFO by insertion order); restore copies entry back. |
| Pause | Paused node neither broadcasts nor applies inbound updates; resume restores. |
| Timestamp skew | Message outside ±300 s envelope timestamp rejected (ErrorCode 9). |

### 3. Cross-Device End-to-End (Manual)

Performed against real hardware on a LAN:

- Laptop (TUI) ↔ Android phone.
- Copy on laptop → paste on phone; copy on phone → paste on laptop.
- Pairing flow from both initiators.
- Wake phone from Doze → sync resumes.
- Forget a device → no further sync until re-paired.
- App killed and restarted → reconnects automatically.
- Pause/resume toggle verified on both platforms.

### 4. Android Instrumentation Tests

In `android/app/src/androidTest/`:

- **AccessibilityService contract**: with the service enabled, injecting a clipboard event results in `on_clipboard_changed` being forwarded to the core (verified via a test double).
- **Clipboard writer**: `ClipboardUpdatedFromRemote` event results in content written to `ClipboardManager`.
- **Settings screen**: permission-status detection renders the correct guidance; `sync.enabled` toggle persists.
- **Restore**: tapping a history entry writes the value to the Android clipboard.
- Foreground service lifecycle: service starts with a notification; survives configuration changes.

## Verification of Success Criteria

Mapping to doc 01:

| Criterion | Covered by |
| --- | --- |
| Automatic discovery on the LAN | Harness discovery + E2E. |
| Trusted peer connections | Harness pairing scenarios (both inbound and requester-initiated). |
| Low-latency plain-text sync both ways | Harness bidirectional + E2E. |
| History without loops/duplicates | Harness loop/duplicate scenarios + origin-field verification. |
| Low resource consumption while idle | Phase F resource pass: measure CPU/RAM with no active traffic. |

## CI Gates

For the Rust workspace, on every push/PR:

- `cargo fmt --check`
- `cargo clippy --all-targets -- -D warnings`
- `cargo test --workspace`

Android:

- `./gradlew test` (unit) and `./gradlew connectedDebugAndroidTest` (instrumented, on a device/emulator in CI when available).

## Tooling Notes

- The two-node harness uses the real discovery/WS transport on loopback with ephemeral high ports overridable in `NetworkConfig`.
- Timing-sensitive assertions (heartbeat, stale detection) use configurable intervals shrunk to milliseconds in tests.
- The harness binaries are `clipboard-core/src/bin/node_a.rs` and `clipboard-core/src/bin/node_b.rs` (dev-only, not shipped).
