# Configuration & Constants Specification

## Overview

This document defines all configurable settings, their defaults, the concrete protocol/network constants, and the error code registry.

## Configuration Storage

| Platform | Mechanism |
| --- | --- |
| Linux desktop | TOML file at `~/.config/clipboard-sync/config.toml` (path resolved via the `dirs` crate). Overlays defaults; values present in SQLite `app_settings` take precedence. |
| Android | No config file. All settings are stored in SQLite via the core settings API and edited through the app UI. |

Precedence (highest to lowest):

1. SQLite `app_settings`
2. TOML file (desktop only)
3. Built-in defaults

## Config File Format (desktop)

```toml
[network]
listen_port = 48272
discovery_port = 48271
discovery_interval_secs = 5
heartbeat_interval_secs = 15
peer_timeout_secs = 45
reconnect_backoff_initial_ms = 1000
reconnect_backoff_max_ms = 60000

[history]
max_size_bytes = 2097152

[sync]
max_clipboard_bytes = 2097152
max_peers = 16
replay_cache_capacity = 1000
replay_cache_ttl_secs = 3600
pairing_timeout_secs = 60
enabled = true

[platform]
clipboard_poll_interval_ms = 250

[storage]
path = "~/.local/share/clipboard-sync/clipboard.db"
```

Only keys the user sets need to appear in the file; unset keys fall back to defaults.

## Settings API (Android)

Exposed through UniFFI: `get_setting(key)`, `set_setting(key, value)`. All settings above are addressable by key (e.g. `history.max_size_bytes`). Persisted in `app_settings`.

## Constants

### Network

| Constant | Default | Notes |
| --- | --- | --- |
| Discovery UDP port | 48271 | DISCOVER / DISCOVER_RESPONSE. |
| Discovery broadcast address | `255.255.255.255` | TTL = 1; unicast response to requester. |
| Discovery interval | 5 s | Periodic DISCOVER broadcast. |
| WebSocket listen port | 48272 | Configurable; advertised in DISCOVER_RESPONSE. |
| Heartbeat interval | 15 s | PING sent every 15 s. |
| Peer timeout | 45 s | Peer marked stale/disconnected without PONG. |
| Reconnect backoff | 1 s, 5 s, 15 s, 60 s cap | Exponential for trusted peers only. |
| Max WebSocket frame | 2,097,152 bytes | Reject larger frames (ErrorCode 7). |

### Limits

| Constant | Default | Notes |
| --- | --- | --- |
| Max clipboard/message payload | 2,097,152 bytes | 2 MB. |
| History cap | 2,097,152 bytes | FIFO eviction (doc 07). |
| Max concurrent peers | 16 | Excess connections rejected (ErrorCode 12). |
| Replay cache capacity | 1,000 message IDs | LRU eviction at capacity. |
| Replay cache TTL | 3,600 s (1 h) | Entries older than TTL pruned. |
| Pairing timeout | 60 s | Pending request expires (ErrorCode 11). |
| Accepted timestamp skew | 300 s (5 min) | Messages outside ± window rejected (ErrorCode 9). Applies to the **envelope timestamp**. |
| Clipboard poll interval | 250 ms | Desktop fallback polling rate (arboard has no OS-notification API on all platforms). |

### Platform

| Item | Values |
| --- | --- |
| Platform enum | `linux`, `android`, `windows`, `macos` |
| clipboard_poll_interval_ms | 250 (desktop only; Android uses AccessibilityService) |

### Identifiers & Hashing

| Item | Scheme |
| --- | --- |
| Device UUID | UUIDv4 |
| Device Name | Human-readable, 1–32 chars |
| Clipboard ID | UUIDv4 |
| Message ID | UUIDv4 |
| Content Hash | SHA-256 hex (64 chars, lowercase) |
| Pairing Request ID | UUIDv4 |

## Error Code Registry

`ErrorCode` enum values match protocol codes (doc 03).

| Code | Name | Description |
| --- | --- | --- |
| 0 | UNKNOWN_ERROR | Unclassified failure. |
| 1 | PROTOCOL_VERSION_UNSUPPORTED | Peer uses an unsupported protocol version. |
| 2 | UNKNOWN_DEVICE | Sender is not a trusted device. |
| 3 | AUTHENTICATION_FAILED | HELLO handshake validation failed. |
| 4 | MALFORMED_MESSAGE | Message failed structural validation. |
| 5 | MISSING_FIELD | A required field is absent. |
| 6 | UNSUPPORTED_CONTENT_TYPE | Unsupported clipboard content type. |
| 7 | PAYLOAD_TOO_LARGE | Payload exceeds the maximum clipboard size. |
| 8 | DUPLICATE_MESSAGE | Message ID already processed (replay). |
| 9 | STALE_MESSAGE | Timestamp outside the acceptable window. |
| 10 | PAIRING_DENIED | Pairing request rejected by the user. |
| 11 | PAIRING_TIMEOUT | Pairing request not answered in time. |
| 12 | CONNECTION_LIMIT | Peer connection limit exceeded. |
| 13 | INTERNAL_ERROR | Unexpected internal failure. |

## Supported Clipboard Types

- `text/plain` only (MVP). Others rejected with ErrorCode 6.
- Clipboard polling interval on desktop: 250 ms (configurable in `[platform]`).

## Protocol Version

- Current: 1. Negotiated in HELLO; mismatches rejected with ErrorCode 1.