# UI/UX Specification

## Overview

Clipboard Sync ships with two front-ends:

- **Android app** (primary): a minimal Jetpack Compose GUI, plus an AccessibilityService and a foreground service for reliable background operation.
- **Desktop TUI**: a ratatui terminal interface for Linux/desktop.

Both front-ends render the same core state and interact with the core through its public API (doc 06).

---

# Android App

## App Identity

| Item | Value |
| --- | --- |
| App name | Clipboard Sync |
| Package | `com.abram056.clipsync` |
| minSdk | 26 (Android 8.0) |
| UI | Jetpack Compose |

## Screens

### 1. Home / Device List

Purpose: show discovered and trusted devices and connection state.

Content:

- Section "Discovered" — devices answering discovery broadcasts. Each row shows device name, platform, and a **Pair** action (calls `request_pairing`).
- Section "Trusted" — paired devices with connection status (Connected / Stale / Offline) and a **Forget** action.
- Empty states with guidance when no devices are found (check Wi-Fi / same network).
- A persistent status line: sync service running or stopped.

Actions: Pair (navigates to pairing flow), Forget (removes from trusted peers).

### 2. Pairing Approval

Purpose: decide whether to trust a requesting device.

Shown when a `PairingRequested` event is received from the core.

Content:

- Device name, platform, and a request prompt ("Allow <device> to sync your clipboard?").
- **Approve** and **Reject** buttons.
- The dialog auto-dismisses after 60 s (pairing timeout) and is treated as a rejection.

### 3. History

Purpose: browse and restore recent clipboard entries.

Content:

- Reverse-chronological list of clipboard entries (first line of text + timestamp + origin device name; "Unknown device" if origin is not in the trusted peers list).
- Tap an entry to restore it to the local clipboard (copies it back).
- **Clear history** action.
- History size indicator (bytes used vs 2 MB cap).

### 4. Settings

Purpose: configure the app and its required permissions.

Content:

- **Clipboard capture**: status of the AccessibilityService with a **Open settings** button (jumps to the system accessibility settings page for the app). Explains why it is required and what data it reads.
- **Synchronization**: toggle to pause/resume syncing (persisted as `sync.enabled` in core settings).
- **Device name**: edit the local device name.
- **Network**: listening port (advanced, default 48272).
- **Privacy note**: text explaining clipboard data is sent only to trusted devices on the local network and is never logged.

### 5. Foreground-Service Notification

Purpose: keep the service alive and surface status.

Content:

- Persistent notification: "Clipboard Sync active" with connection count.
- Tapping it opens the app.
- A **Stop** action is intentionally omitted; stopping is done in the app to avoid accidentally breaking sync.

## Android Platform Constraints

### Clipboard capture via AccessibilityService

- Android 10+ restricts clipboard reads to the focused app or default IME; a background service cannot read it. To capture clipboard content globally, the app uses an `AccessibilityService` declared with `canRetrieveWindowContent`.
- The user must enable it manually in **Settings → Accessibility → Clipboard Sync**; the app cannot request it programmatically. The Settings screen detects whether it is enabled (`AccessibilityManager.isEnabled` + service id check) and guides the user.
- The service listens for `TYPE_WINDOW_CONTENT_CHANGED` and focus events with a 250 ms debounce to avoid excessive reads.
- Play Store policy requires accessibility services to serve a genuine accessibility purpose; if the app is ever distributed via Play, this must be reviewed. For personal/sideloaded use this is acceptable.

### Android 13+ clipboard read toast

- Reading the clipboard may show a system toast ("Clipboard Sync pasted from..."). This is platform behavior, not an app defect; it is documented in Settings.

### Foreground service & battery

- A foreground service of type `dataSync` with a persistent notification keeps the WebSocket alive.
- Declare `FOREGROUND_SERVICE_DATA_SYNC` permission (required on Android 14+).
- Under Doze, the connection may be suspended; the app reconnects on resume using the reconnect backoff policy (doc 08).

### Feed path

- **Inbound**: the AccessibilityService reads clipboard text on qualifying events (with 250 ms debounce) and calls `on_clipboard_changed(content)` on the core.
- **Outbound**: the `clipboard_writer` Kotlin component subscribes to `ClipboardUpdatedFromRemote` events from the core and writes the content to `ClipboardManager` on the main thread. This produces a local clipboard write; the engine deduplicates it and does not re-broadcast.

---

# Desktop TUI

## Layout

Three-pane terminal layout:

```
┌───────────────┬────────────────────┬──────────────┐
│ Devices       │ History            │ Status       │
│               │                    │              │
│  • Laptop   ● │  12:04  text...    │  Sync: on    │
│  • Phone    ● │  12:01  text...    │  Peers: 2    │
│  • Kitchen  ○ │  ...               │  History: 0.4│
│               │                    │  MB / 2 MB   │
└───────────────┴────────────────────┴──────────────┘
```

- **Devices**: discovered (○) and connected (●) peers; paired peers marked; **P** calls `request_pairing` on a discovered device, **F** forgets a trusted one.
- **History**: reverse-chronological entries with timestamp and origin; **Enter** restores the highlighted entry.
- **Status**: sync on/off, connected peer count, history size, last error (if any).

## Keybindings

| Key | Action |
| --- | --- |
| Tab | Cycle panes |
| ↑ / ↓ | Move selection |
| Enter | Restore selected history entry / confirm |
| P | Pair selected discovered device |
| F | Forget selected trusted device |
| A / R | Approve / Reject a pending pairing prompt |
| S | Pause / resume syncing |
| Q | Quit |

## Interaction Model

- Clipboard changes are captured by the arboard monitor and pushed to the core automatically (no UI involvement).
- Remote clipboard updates appear in History and flash a brief status message ("Received from <device>").
- Pairing prompts appear as a modal overlay on the Devices pane with A/R keys.
- Logs go to stderr and a local file; clipboard content is never logged (doc 04).

---

# Common Behavior

- **Restore**: on both platforms, restoring an entry copies it to the local clipboard. This produces a local `ClipboardChanged`; the Sync Engine deduplicates by content hash so it is not treated as new content (doc 03/06).
- **Notifications**: transient status messages (sync errors, pairing outcomes) are shown in the TUI status pane and as Android toasts only for user-relevant events (pairing, errors), never for clipboard content.
- **Pause**: while paused, the app still monitors locally but does not broadcast to peers and ignores inbound updates.