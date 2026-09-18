# Data Model & Storage Specification

## Overview

All persistent application data is stored locally in a single SQLite database using rusqlite with the `bundled` feature. The database lives in the platform-specific application data directory:

| Platform | Database path |
| --- | --- |
| Linux desktop | `~/.local/share/clipboard-sync/clipboard.db` |
| Android | App-internal storage (`Context.getDatabasePath`), accessed through the core |

SQLite is accessed synchronously from a single task (the Sync Engine task) via a Mutex-guarded connection. Foreign keys and `WAL` mode are enabled.

## Device Identity

The identity is created once at first launch and persisted in `device_config`.

| Field | Type | Notes |
| --- | --- | --- |
| device_id | TEXT (UUIDv4) | Primary key; fixed for the lifetime of the installation. |
| device_name | TEXT | Defaults to the OS hostname; user-editable, 1–32 chars. |
| created_at | TEXT (ISO 8601) | First-launch timestamp. |

## Tables

### device_config

Single-row table holding the local device identity. Enforced by the storage layer (rowid fixed at 1; `INSERT OR IGNORE` with a CHECK constraint).

```sql
CREATE TABLE device_config (
    id          INTEGER PRIMARY KEY CHECK (id = 1),
    device_id   TEXT NOT NULL,
    device_name TEXT NOT NULL,
    created_at  TEXT NOT NULL
);
```

### trusted_peers

Devices the user has approved for synchronization.

```sql
CREATE TABLE trusted_peers (
    device_id   TEXT PRIMARY KEY NOT NULL,
    device_name TEXT NOT NULL,
    platform    TEXT,
    paired_at   TEXT NOT NULL,
    last_seen   TEXT
);
```

Semantics:

- A peer becomes trusted only after the user approves a `PAIRING_REQUEST`.
- `last_seen` is updated on each successful connection or PONG.
- Removing a peer deletes the row; the device must be paired again to reconnect.

### clipboard_history

Every synchronized clipboard entry (local origin or received from a remote).

```sql
CREATE TABLE clipboard_history (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    clipboard_id     TEXT NOT NULL UNIQUE,      -- UUIDv4
    content          TEXT NOT NULL,
    content_type     TEXT NOT NULL DEFAULT 'text/plain',
    content_hash     TEXT NOT NULL,             -- SHA-256 hex
    origin_device_id TEXT NOT NULL,
    created_at       TEXT NOT NULL,             -- ISO 8601 UTC
    size_bytes       INTEGER NOT NULL
);

CREATE INDEX idx_history_created ON clipboard_history (created_at DESC);
CREATE INDEX idx_history_hash ON clipboard_history (content_hash);
```

Semantics:

- `content_hash` is the SHA-256 hex digest of `content`; used for duplicate detection.
- `origin_device_id` is the device that first generated the entry; used for loop prevention.
- `clipboard_id` is unique and stable; restoration targets it.
- Size accounting uses `size_bytes` = UTF-8 byte length of `content`.

### app_settings

Key-value settings store. This is the single source of settings truth; on desktop, the TOML file overlays only values not present here (doc 08).

```sql
CREATE TABLE app_settings (
    key   TEXT PRIMARY KEY NOT NULL,
    value TEXT NOT NULL
);
```

## History Retention & Eviction

The history cap is 2,097,152 bytes (2 MB) of stored content.

- On insert, `size_bytes` is summed across `clipboard_history`.
- If the total exceeds the cap, entries are evicted oldest-first by **insertion order** (`id ASC` — not `created_at`, which is the content-creation timestamp from the origin device and subject to clock skew) until the total is at or below the cap.
- The newest entry is never evicted.
- Since the maximum clipboard payload equals the history cap (2 MB), a single entry can never exceed the cap.
- The current history size is reported to the UI via `ClipboardHistoryUpdated`.

## Pending Pairing Requests

Pending pairing requests are process-state only (not persisted to SQLite). On restart, pending requests are lost; the pairing process must be re-initiated by the user.

## Schema Migration

- Schema version is tracked in `PRAGMA user_version`.
- Migrations are a sequential list of `CREATE TABLE`/`ALTER` scripts applied within a transaction when `user_version < target`.
- MVP ships version 1; future changes increment the version and add a migration step.

## Data Protection

- The database directory is created with user-only permissions on desktop.
- Clipboard content is stored in plaintext during the MVP; encryption of local storage is a future enhancement (doc 04).
- No clipboard content is ever written to logs (doc 04).