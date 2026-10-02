# Clipboard Sync

Peer-to-peer clipboard synchronization for your own devices on the same local
network. Copy a URL, a command, or a snippet on your laptop and paste it on
your phone — no cloud, no accounts, no messaging app in the middle.

Devices discover each other over UDP broadcast, pair once through an explicit
approval step, and then keep a trusted WebSocket connection open. Every
clipboard update is hashed so loops and duplicates are dropped, and a bounded
local history (2 MB by default) keeps recent entries recoverable. Everything
runs on the LAN; there is no server component.

## Features

- Automatic LAN discovery (UDP broadcast) with per-device status
- Pairing with approval, a configurable timeout, and a persistent trust list
- Bidirectional plain-text sync over WebSockets, with reconnect backoff
- Duplicate/loop prevention via content hashing
- Clipboard history with restore, size-capped
- Pause/resume, multi-peer support, friendly device names
- Desktop TUI (ratatui); Android front-end planned (Phase E, not yet built)

Out of scope for the MVP: images, files, rich text, encryption, and anything
crossing the internet. See [docs/01](docs/01_Project_Description.md).

## Workspace

| Crate | Purpose |
| --- | --- |
| `clipboard-proto` | Wire protocol: messages, envelopes, events, error codes |
| `clipboard-core` | Sync engine: discovery, pairing, storage, coordinator, plus two test-harness binaries |
| `clipboard-tui` | Desktop front-end: terminal UI, clipboard monitor, logging |

## Quick start

Requires Rust (stable) and a desktop with X11 or Wayland for clipboard access.

```sh
cargo build --workspace --locked
cargo test  --workspace --locked
```

Run a single node:

```sh
cargo run -p clipboard-tui
```

Run two nodes on one machine (separate databases and ports). On a single
host, broadcasts don't cross differing discovery ports, so point each node at
the other with `--peer`:

```sh
cargo run -p clipboard-tui -- --db /tmp/node-a \
  --peer 127.0.0.1:48281
cargo run -p clipboard-tui -- --db /tmp/node-b \
  --listen-port 48282 --discovery-port 48281 \
  --peer 127.0.0.1:48271
```

Pair them from the Devices pane (**P**), approve the prompt on the other side
(**A**/**R** to reject), and copy something. Full keybindings are in
[docs/09](docs/09_UI_Specification.md).

For scripted end-to-end runs there is a headless harness (same one-machine
`--peer` caveat; databases default to `node_a.db`/`node_b.db`):

```sh
# terminal 1
cargo run -p clipboard-core --bin node_a -- --id a --auto-approve \
  --peer 127.0.0.1:48281
# terminal 2
cargo run -p clipboard-core --bin node_b -- --id b --auto-pair \
  --listen-port 48282 --discovery-port 48281 \
  --peer 127.0.0.1:48271
```

## Configuration and data

| What | Where |
| --- | --- |
| Config (optional TOML) | `~/.config/clipboard-sync/config.toml` |
| Database | `~/.local/share/clipboard-sync/clipboard.db` |
| Log file | `~/.local/share/clipboard-sync/clipboard-sync.log` |

Only the keys you set need to appear in the config — unset keys fall back to
the documented defaults (ports `48271`/`48272`, 60 s pairing timeout, 2 MB
history cap, 1/5/15/60 s reconnect backoff). All keys and constants:
[docs/08](docs/08_Configuration_And_Constants.md).

Logs go to stderr and to the log file; clipboard content itself is never
logged.

## Development

CI runs the same gates locally:

```sh
cargo fmt  --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test  --workspace --locked
```

## Documentation

The specification lives in [`docs/`](docs/):

| Doc | Contents |
| --- | --- |
| [01 Project Description](docs/01_Project_Description.md) | Goals, scope, requirements |
| [02 Architecture](docs/02_Architecture.md) | Component structure |
| [03 Protocol](docs/03_Events_And_Protocol_Specification.md) | Messages, events, flows |
| [04 Security](docs/04_Security_Specification.md) | Trust model, logging policy |
| [05 Implementation Plan](docs/05_Implementation_Plan.md) | Phases and milestones |
| [06 Module Specification](docs/06_Module_Specification.md) | Per-module contracts |
| [07 Data Model](docs/07_Data_Model_And_Storage.md) | Schema and storage |
| [08 Configuration](docs/08_Configuration_And_Constants.md) | Config keys and constants |
| [09 UI Specification](docs/09_UI_Specification.md) | TUI and Android UI |
| [10 Testing Strategy](docs/10_Testing_Strategy.md) | Test plan and coverage |
