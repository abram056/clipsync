# Conformance And Resource Baseline

Phase F deliverables (doc 05 §Phase F): a traceability report mapping doc 01's
requirements to implementation and tests, a log of accepted deviations from the
specifications, and a measured resource baseline for the desktop node.

Line numbers refer to the commit that introduced this document and will drift
with normal maintenance; prefer the symbol name when navigating.

---

# Requirements Traceability

## Functional Requirements

| # | Requirement (doc 01) | Implementation | Coverage |
| --- | --- | --- | --- |
| F1 | Detect changes to the local clipboard. | `clipboard-tui/src/clipboard_monitor.rs` `ClipboardMonitor::spawn` | `phase_c_sync::bidirectional_sync` |
| F2 | Synchronize clipboard updates with trusted devices. | `clipboard-core/src/coordinator.rs` `handle_clipboard_update` | `phase_c_sync::bidirectional_sync` |
| F3 | Receive and apply clipboard updates from connected peers. | `clipboard-core/src/coordinator.rs` `handle_clipboard_update`; applied via `tui.rs` `ClipboardUpdatedFromRemote` | `phase_c_sync::bidirectional_sync` |
| F4 | Prevent synchronization loops and duplicate updates. | `replay_cache.rs` `check_and_record_hash` + `origin_device_id` on `CLIPBOARD_UPDATE` | `phase_c_sync::no_loop`, `phase_c_sync::no_duplicate_local_copy`, `protocol_validation::same_origin_duplicate_hash_ignored` |
| F5 | Maintain a clipboard history with automatic size management. | `storage.rs` `HISTORY_CAP_BYTES` (2 MiB), `evict_history` | `storage.rs` `history_eviction`, `phase_d_core::history_size_bytes_after_sync` |
| F6 | Allow users to restore previous clipboard entries. | `runtime.rs` `restore_history_entry`; TUI history pane `Enter` | `phase_c_sync::history_list_and_restore` |
| F7 | Discover compatible devices on the same local network. | `discovery.rs` `broadcast_discover`, `check_staleness` | `phase_c_sync::mutual_discovery_connects_both_sides`, `discovery.rs` roster tests |
| F8 | Establish trusted peer connections through a pairing process. | `pairing.rs` `request_pairing`, `approve`, `reject` | `phase_d_core::trusted_peers_list_after_pairing`, `protocol_validation::pairing_reject_returns_reject_to_requester` |
| F9 | Synchronize automatically whenever paired devices become available. | `coordinator.rs` reconnect scheduling on discovery and socket loss | `phase_d_core::peer_restart_reconnects_without_repairing`, `discovery.rs` roster tests |

## Non-Functional Requirements

| # | Requirement (doc 01) | Status | Evidence |
| --- | --- | --- | --- |
| N1 | Minimal latency under normal LAN conditions. | Implemented; not separately measured. | Exercised by `phase_c_sync::bidirectional_sync` |
| N2 | Minimal resource use while running in the background. | Measured below. | see "Resource Baseline" |
| N3 | Reliable during temporary network interruptions. | Implemented (backoff, reconnect, heartbeat). | `phase_d_core::peer_restart_reconnects_without_repairing`, `phase_d_core::forget_stops_sync_and_reconnects`, `protocol_validation::configured_heartbeat_and_peer_timeout_are_honoured` |
| N4 | Extensible to additional clipboard formats. | Text only by MVP design; `content_type` exists on history entries. | doc 01 §Out of Scope |
| N5 | Portable across multiple operating systems. | Desktop TUI implemented for Linux/desktop. Android front-end is Phase E, not started. | doc 09 §Overview |
| N6 | Modular architecture separating networking, sync, storage, platform. | Implemented as three crates plus a platform adapter. | workspace layout; doc 02 §Platform Abstraction |

## Success Criteria

| Criterion (doc 01) | Evidence |
| --- | --- |
| Automatic discovery on the LAN. | `phase_c_sync::mutual_discovery_connects_both_sides`, `phase_c_sync::lan_broadcast_discovery_manual` (manual, ignored in CI) |
| Trusted peer connections. | `phase_d_core::trusted_peers_list_after_pairing`, `protocol_validation::pairing_reject_returns_reject_to_requester` |
| Plain-text sync with minimal latency. | `phase_c_sync::bidirectional_sync` |
| History without loops or duplicates. | `phase_c_sync::no_loop`, `phase_c_sync::no_duplicate_local_copy`, `storage.rs` `history_eviction` |
| Seamless operation with minimal resources. | see "Resource Baseline" |

---

# Resource Baseline

Measured on the release build of `clipboard-tui` (`cargo build --release`), on
Linux/Wayland, sampling `utime + stime` deltas from `/proc/<pid>/stat` against
`CLK_TCK`, with `VmRSS` read from `/proc/<pid>/status`. Each sample is 5 s,
after a 15 s warmup.

Four states: **(A)** TUI idle with no peers, **(B)** TUI paired with one
harness peer and no traffic, **(C)** TUI under a `wl-copy` burst (one write
every 2 s for 60 s), **(D)** harness peer with no clipboard monitor, as the
network-and-timers-only baseline.

Attribution: state A is re-run with `platform.clipboard_poll_interval_ms`
raised from 250 to 2000. The difference between the two runs is the cost of
the 250 ms clipboard poll loop.

| State | Idle CPU (avg) | RSS |
| --- | --- | --- |
| A — TUI, no peers | _pending_ | _pending_ |
| A2 — TUI, poll interval 2000 ms | _pending_ | _pending_ |
| B — TUI, one paired peer | _pending_ | _pending_ |
| C — TUI, clipboard traffic | _pending_ | _pending_ |
| D — harness peer, no monitor | _pending_ | _pending_ |

Interpretation: if A − A2 is a large share of A, the clipboard poll interval
is the dominant idle cost and `platform.clipboard_poll_interval_ms` (doc 08)
is the tuning knob; otherwise the idle cost is dominated by the runtime's
timers.

Measured values are recorded here after the rendering fixes land, so the
baseline describes the shipped UI.

---

# Accepted Deviations

Deviations from the specifications, recorded for Phase F. Each entry is
either a known gap or a point where the specification leaves the choice
open and the implementation picked one.

## Gap: GOODBYE is never sent

doc 03 requires GOODBYE when a pairing is rejected (doc 03:653, doc 03:669)
and during graceful shutdown (doc 03:675). `MessageType::Goodbye` is defined
in `clipboard-proto` and inbound GOODBYE is handled (`messaging.rs`), but no
code path sends it: `PairingManager::reject` transmits `PAIRING_REJECT` and
leaves the session to time out or be closed by the peer.

The user-visible behaviour is still correct — the requester learns the
outcome from `PAIRING_REJECT`, which
`protocol_validation::pairing_reject_returns_reject_to_requester` covers —
so only the polite session teardown is missing.

**Deferred**, not fixed here: it needs a new messaging command, an explicit
point in both the reject and shutdown paths, and tests on both sides.

## Decision: ErrorCode 2 leaves the connection open

doc 03:636 states that clipboard messages are rejected with ErrorCode 2
while a connection sits in the Pairing state, but does not say whether the
connection should then be closed. The implementation answers with an `ERROR`
message and keeps the session alive, so the pairing exchange can continue.

Chosen because closing would abort a legitimate pairing attempt whenever a
peer sends a message out of order.

## Deviation: `max_peers` bounds inbound connections only

doc 08:37 defines `max_peers` without specifying a direction. The server
enforces it (`protocol_validation::connection_limit_returns_error_twelve`
returns ErrorCode 12 past the limit), but outbound dialling is not capped,
so a peer could in principle hold more than `max_peers` sockets if it is
also dialling out. **Accepted for MVP**; revisit if connection growth ever
becomes a concern.

## Note: reconnect ladder clamps to the configured bounds

doc 08:68 documents the ladder as 1 s → 5 s → 15 s → 60 s. The
implementation walks exactly those steps under the default configuration and
clamps them to whatever `reconnect_backoff_initial_ms` and
`reconnect_backoff_max_ms` are set to, so a lowered cap shortens the ladder
rather than exceeding it. This is the intended reading of having the bounds
configurable, and `phase_d_core::peer_restart_reconnects_without_repairing`
plus `coordinator`'s backoff unit tests cover it.

## Note: `device_name` is not bounded by the protocol

Nothing in doc 03 or the `clipboard-proto` types caps `device_name`, and no
validation rejects an oversized one. Rather than change the wire contract in
the MVP, the TUI bounds the name where it is drawn (device list, pairing
popup, status messages) so markers and pane geometry survive. A protocol-level
cap would be a doc 03 change with a matching error code, which is out of scope
here.

## Coverage note: LAN broadcast is a manual test

`phase_c_sync::lan_broadcast_discovery_manual` is `#[ignore]`d: it needs a
real broadcast-capable network, which loopback does not provide. It is run by
hand and is excluded from CI, so the LAN half of the discovery success
criterion is verified manually (doc 10 §2).

# Future Work

- Android front-end (doc 05 Phase E) and cross-device E2E (doc 10 §3) remain
  unstarted, so N5 and the cross-device success criteria are not yet satisfied.
