# Android UI Specification

## 1. Overview

This document defines the user interface and user-facing behavior for the ClipSync Android application.

The UI is responsible for:

* showing synchronization status;
* showing connected and trusted devices;
* handling pairing;
* displaying clipboard history;
* allowing clipboard history restoration;
* controlling synchronization settings;
* exposing application/device configuration;
* communicating errors and important background states.

The UI does **not** implement synchronization logic.

The UI communicates with the application through the Android repository layer:

```text id="6g3y7p"
Compose
   │
   ▼
ViewModel
   │
   ▼
Repository
   │
   ▼
ClipSync Bridge
   │
   ▼
Rust Core
```

---

# 2. UI Design Principles

The ClipSync UI should follow five principles.

### 2.1 Status first

The user should immediately understand:

> "Is ClipSync currently working?"

The home screen should make synchronization state the most prominent piece of information.

### 2.2 Device-centric

ClipSync exists to synchronize between devices.

Devices and their connection state should therefore be easy to discover.

### 2.3 History is useful, not dominant

Clipboard history is important but should not overwhelm the primary synchronization experience.

### 2.4 Minimal interaction

Most synchronization should happen automatically.

The user should not need to manually trigger synchronization.

### 2.5 Clear failure states

The UI must distinguish between:

* paused;
* disconnected;
* reconnecting;
* permission problems;
* actual application errors.

"Not currently connected" should not look like "ClipSync is broken."

---

# 3. Navigation

The initial application should use a simple bottom navigation structure.

```text id="t6m2vh"
┌───────────────────────────────────────────┐
│                                           │
│                 Screen                    │
│                                           │
│                                           │
├───────────────────────────────────────────┤
│  Home     History     Devices     Settings│
└───────────────────────────────────────────┘
```

Primary destinations:

1. Home
2. History
3. Devices
4. Settings

Pairing is initiated from the Devices screen and may use a dialog/sheet rather than a permanent navigation destination.

---

# 4. Application Screens

The initial screen hierarchy is:

```text id="j10yq4"
App
│
├── Home
│
├── History
│   └── History item actions
│
├── Devices
│   ├── Trusted devices
│   ├── Pair device
│   └── Pairing request
│
└── Settings
    ├── Synchronization
    ├── Device
    ├── Notifications
    └── About
```

The initial version should avoid deep navigation.

---

# 5. Home Screen

The Home screen is the primary ClipSync dashboard.

Conceptually:

```text id="5pn3qv"
┌───────────────────────────────────────────┐
│ ClipSync                              ⚙  │
│                                           │
│ ┌───────────────────────────────────────┐ │
│ │                                       │ │
│ │              ● Syncing                │ │
│ │        Connected to 2 devices         │ │
│ │                                       │ │
│ └───────────────────────────────────────┘ │
│                                           │
│ Devices                                   │
│                                           │
│  💻 Laptop                 Connected      │
│  📱 Phone                  Connected      │
│                                           │
│ Recent clipboard                          │
│                                           │
│ "Example clipboard text..."               │
│ "Another copied item..."                  │
│                                           │
│                 View history →             │
└───────────────────────────────────────────┘
```

The exact visual design remains flexible.

The information hierarchy should remain stable.

---

# 6. Home Screen Sections

The Home screen contains:

### Header

Contains:

* ClipSync title/logo;
* optional settings shortcut.

### Synchronization card

Shows:

* current synchronization state;
* number of connected devices;
* primary action where applicable.

### Devices preview

Shows a small number of relevant devices.

The full device list is available under Devices.

### Recent clipboard

Shows the latest few history entries.

The full history is available under History.

---

# 7. Synchronization Status

The synchronization card is the most important component on Home.

Supported states:

```text id="lh7e1o"
Disabled
Starting
Running
Paused
Reconnecting
No devices
Error
```

### Disabled

```text id="c8k9un"
Synchronization is off
```

Action:

```text
Enable synchronization
```

### Starting

```text id="x6k5rj"
Starting ClipSync...
```

No destructive action should be presented.

### Running

```text id="7aw3jo"
Syncing
2 devices connected
```

### Paused

```text id="q0v6m4"
Synchronization paused
```

Action:

```text
Resume
```

### Reconnecting

```text id="6z2f9e"
Reconnecting...
```

The UI should reassure the user that ClipSync is still active.

### No devices

```text id="6t7q9j"
No connected devices
```

Action:

```text
Pair a device
```

### Error

```text id="w8t1gc"
Something went wrong
```

The UI should provide a useful recovery action where possible.

---

# 8. Home Device Preview

The Home screen should display a compact list of currently relevant devices.

Each device row contains:

```text id="83z8t4"
Device icon
Device name
Connection state
```

Example:

```text
Laptop
Connected

Pixel Phone
Disconnected
```

A maximum of approximately 3–4 devices should be shown on Home.

If more exist:

```text
View all devices →
```

---

# 9. Home Clipboard Preview

The Home screen shows recent clipboard entries.

Each entry should display:

* content preview;
* origin device;
* relative time.

Example:

```text id="9m1k5r"
"git commit -m 'fix sync loop'"
Laptop · 2 min ago
```

Clipboard content should be visually truncated rather than allowed to dominate the screen.

---

# 10. History Screen

The History screen provides access to stored clipboard entries.

Conceptually:

```text id="az0y5e"
┌───────────────────────────────────────────┐
│ ← History                                 │
│                                           │
│ Search clipboard...                       │
│                                           │
│ Today                                     │
│                                           │
│ "git status"                  Laptop      │
│ 2 min ago                                  │
│                                           │
│ "https://example.com"        Phone        │
│ 18 min ago                                 │
│                                           │
│ Yesterday                                 │
│                                           │
│ "some clipboard text..."      Laptop      │
└───────────────────────────────────────────┘
```

---

# 11. History Item

Each history item should display:

```text id="e7m2xj"
Content preview
Origin device
Timestamp
```

Optional metadata:

* content type;
* size.

These should not clutter the default presentation.

---

# 12. History Actions

Selecting a history entry should expose:

```text id="t6x2qk"
Restore
Copy
```

The primary action is:

> Restore

Restore means:

```text id="i2p8kh"
History
   │
   ▼
Repository.restoreHistoryEntry(id)
   │
   ▼
Rust Core records the replay hash
and emits ClipboardRestored { clipboardId }
   │
   ▼
ClipboardWriter writes the entry content
(already loaded in the history list)
   │
   ▼
Android Clipboard
```

The operation should use the existing Rust `restoreHistoryEntry()` path rather than having Android independently construct a clipboard event. Note that `ClipboardRestored` carries only the clipboard ID — the content is written Android-side from the loaded history entry, and the core's replay cache prevents the write from being rebroadcast (see doc 12, Section 13).

---

# 13. History Search

Search should initially be local to the Android presentation layer.

The MVP does not require a Rust-side full-text search API.

The UI may filter the currently loaded history list.

If history becomes too large for efficient client-side filtering, a Rust-side search API can be introduced later.

---

# 14. Empty History

When there is no history:

```text id="u4q4ad"
No clipboard history yet

Copy something on any connected device
and it will appear here.
```

The empty state should not look like an error.

---

# 15. History Storage Limit

The UI may show the current history size in Settings.

Example:

```text id="v3u0gq"
Clipboard history
1.2 MB used
2 MB limit
```

This information comes from the core's `historySizeBytes()`.

The UI must not independently calculate authoritative storage usage.

---

# 16. Devices Screen

The Devices screen manages trusted devices and pairing.

Conceptually:

```text id="7l9f1z"
┌───────────────────────────────────────────┐
│ Devices                         + Pair     │
│                                           │
│ Trusted devices                           │
│                                           │
│ 💻 Laptop                                 │
│    Connected                              │
│                                           │
│ 📱 Phone                                  │
│    Last seen 5 min ago                    │
│                                           │
│                                           │
│ Nearby devices                            │
│                                           │
│ 🖥 Desktop                                │
│    Available to pair                      │
└───────────────────────────────────────────┘
```

---

# 17. Device States

The UI distinguishes:

```text id="v4tqf9"
Discovered
Pairing
Trusted
Connected
Disconnected
```

These states must not be conflated.

For example:

```text
Trusted + Disconnected
```

means:

> This device is trusted, but it is currently unavailable.

It does **not** mean pairing has failed.

---

# 18. Trusted Devices

Trusted devices are the primary device list.

Each row displays:

```text id="9l6k2v"
Device icon
Device name
Platform
Connection state
```

Actions may include:

```text
Forget device
```

Forgetting a device must require confirmation.

---

# 19. Forget Device Confirmation

Example:

```text id="v9j7f4"
Forget "Laptop"?

This device will no longer be trusted
by ClipSync.

Cancel        Forget
```

The UI should not imply that the remote device itself is deleted.

Only the local trust relationship is removed.

---

# 20. Nearby Devices

Nearby/discovered devices may be shown separately from trusted devices.

A discovered device is not automatically trusted.

Example:

```text id="c2m8yq"
Desktop-PC
Available

[Pair]
```

The Pair action starts the pairing workflow.

---

# 21. Pairing Flow

The pairing flow is:

```text id="g7p0u8"
Devices
   │
   ▼
Pair device
   │
   ▼
Discover devices
   │
   ▼
Select device
   │
   ▼
Pairing request
   │
   ▼
Remote approval
   │
   ▼
Trusted device
```

The UI must clearly communicate where the pairing process currently is.

---

# 22. Pairing Request

When another device requests pairing:

```text id="y2q8km"
┌───────────────────────────────────────────┐
│ Pairing request                           │
│                                           │
│ Laptop wants to connect to ClipSync.      │
│                                           │
│            [Device icon]                  │
│                                           │
│ Laptop                                    │
│ Linux                                     │
│                                           │
│ Cancel                  Accept             │
└───────────────────────────────────────────┘
```

The user should not need to navigate away from the current screen to respond.

A dialog, bottom sheet, or system notification may be used depending on the Android lifecycle context.

---

# 23. Pairing Result

Success:

```text id="w7p4c2"
Device paired

Laptop is now trusted.
```

Failure:

```text id="k5v9h3"
Pairing failed

The device could not be paired.
Try again.
```

Timeout:

```text id="x1c4m7"
Pairing request expired.
```

---

# 24. Pairing Security

The UI must not imply that discovering a device means trusting it.

The explicit user action:

```text
Pair
```

or:

```text
Accept
```

is required.

No automatic trust should be granted solely because a device appears on the network.

---

# 25. Settings Screen

Settings contains application configuration rather than synchronization history.

Conceptually:

```text id="u8z5cm"
Settings

Synchronization
  Synchronization        On
  Pause synchronization

Clipboard capture
  Background capture      Enabled / Not granted

Device
  Device name
  Device identity

Notifications
  Sync notifications

Storage
  Clipboard history
  Clear history

About
  Version
  Open source licenses
```

The Background capture row shows whether the Android 10+ capture mechanism (doc 13, Section 12) is granted; when not granted, it links to the accessibility settings so the user can diagnose missing background capture.

Only settings supported by the current core should be implemented.

---

# 26. Synchronization Settings

The first synchronization section should include:

```text id="6f4q2n"
Synchronization       [On/Off]

Pause synchronization [On/Off]
```

These are different concepts.

### Synchronization disabled

The core/service may be stopped.

### Synchronization paused

The core may remain running but synchronization activity is paused.

The UI should explain the distinction.

---

# 27. Device Settings

Device settings should initially show:

```text id="y4q8nr"
Device name
Device ID
Platform
```

The device ID may be presented as a secondary/debug detail.

Example:

```text
This device

Gionee Phone
Android

Device ID
ed4e...7a21
```

The full device ID should not dominate normal UI.

---

# 28. Device Name Editing

Device-name editing should only be enabled after the underlying Rust core supports a stable `setDeviceName()` operation.

Until then:

```text
Device name
My Phone
```

may be displayed as read-only.

Do not implement an Android-only device name that differs from the Rust identity.

The Rust core must remain the source of truth.

---

# 29. Notifications

Notification settings may eventually include:

```text id="8b1z7k"
Sync notifications       On
Pairing requests         On
Errors                   On
```

The MVP should avoid excessive notifications.

Normal successful clipboard synchronization should not generate a notification for every clipboard item.

---

# 30. Storage Settings

The storage section should show:

```text id="x7k4mc"
Clipboard history

1.2 MB used
2 MB maximum

[Clear history]
```

Clearing history requires confirmation.

---

# 31. Clear History Confirmation

Example:

```text id="r2n6vz"
Clear clipboard history?

This removes stored clipboard history
from this device.

Cancel                 Clear
```

The action calls the Rust core's history-clearing API.

Android must not directly delete database files.

---

# 32. About Screen

The About section should contain:

```text id="q9w2pd"
ClipSync

Version 0.x.x

Device-to-device clipboard synchronization
without requiring a central clipboard server.

Open source licenses
```

The About screen should remain simple.

---

# 33. Loading States

Every screen that depends on core data needs a loading state.

Example:

```text id="j6n8ry"
Loading devices...
```

Avoid displaying fake data while the core is initializing.

---

# 34. Error States

Errors should be localized to the affected operation.

For example:

```text id="d7m2pa"
Devices

Unable to load devices.
[Retry]
```

rather than:

```text
Something went wrong with ClipSync.
```

unless the entire core is unavailable.

---

# 35. Transient Errors

Transient operations may use a snackbar/toast-style message.

Examples:

```text id="v8h2qn"
"Clipboard restored"

"Device forgotten"

"Pairing request sent"
```

These messages should be brief.

Persistent errors should appear in the relevant screen/state rather than relying solely on transient messages.

---

# 36. Offline State

If there is no network connection:

```text id="w2j7sx"
Syncing

Waiting for connection...
```

The UI should not suggest that clipboard history has been lost.

History remains available locally.

This distinction is important:

```text
Network unavailable
        ≠
Clipboard unavailable
```

---

# 37. Paused State

When synchronization is paused:

```text id="m5v1ca"
Synchronization paused

Clipboard changes will not be
synchronized until you resume.
```

History behavior should follow the core's defined semantics.

The UI must not invent separate Android-side pause behavior.

---

# 38. Accessibility

The UI must support Android accessibility requirements.

Important considerations:

* meaningful content descriptions;
* sufficient contrast;
* touch targets of appropriate size;
* text scaling;
* screen-reader-friendly device states;
* state changes communicated accessibly.

Icons must not be the only way to communicate state.

For example:

```text
●
```

should be accompanied by:

```text
Connected
```

---

# 39. Dark and Light Themes

The application must support both dark and light system themes.

The UI specification does not hard-code individual colors.

Instead, the implementation should define semantic colors:

```text id="3w6y1q"
Primary
OnPrimary
Background
Surface
OnSurface
Secondary
Success
Warning
Error
Outline
```

The same semantic roles should map appropriately to both themes.

---

# 40. Visual Language

The visual language should be:

* clean;
* technical without feeling intimidating;
* calm;
* information-dense where useful;
* visually distinct between state types.

Avoid turning ClipSync into a dashboard full of graphs.

The primary visual metaphor should remain:

> **devices connected by synchronization.**

---

# 41. UI State Model

The repository should expose UI-friendly state rather than raw bridge events.

Conceptually:

```text id="8y6x2k"
SyncState
├── Disabled
├── Starting
├── Running
├── Paused
├── Reconnecting
├── NoDevices
└── Error
```

Device:

```text id="n1x5az"
DeviceState
├── Discovered
├── Pairing
├── Trusted
├── Connected
└── Disconnected
```

Pairing:

```text id="t4c7pm"
PairingState
├── Idle
├── Requesting
├── WaitingForApproval
├── Accepted
├── Rejected
├── Expired
└── Failed
```

These states are presentation models.

They must not replace the Rust core's authoritative state model.

---

# 42. ViewModel Responsibilities

Each major screen gets its own ViewModel where appropriate.

Suggested structure:

```text id="z8n5c2"
HomeViewModel
HistoryViewModel
DevicesViewModel
SettingsViewModel
```

ViewModels should:

* expose UI state;
* invoke repository operations;
* handle user actions;
* translate repository errors into UI state/messages.

They should not:

* call UniFFI-generated APIs directly;
* access Android ClipboardManager;
* access the Rust database;
* manage foreground services directly.

---

# 43. HomeViewModel

HomeViewModel consumes:

```text id="1r4p8a"
syncState
devices
recentHistory
```

and exposes:

```text id="0x7h4c"
HomeUiState
├── syncState
├── connectedDeviceCount
├── devicesPreview
└── recentHistory
```

---

# 44. HistoryViewModel

HistoryViewModel consumes:

```text id="m4f8q1"
history
```

and exposes:

```text id="k6p3w9"
HistoryUiState
├── loading
├── entries
├── searchQuery
├── filteredEntries
└── error
```

Restore actions go through the repository.

---

# 45. DevicesViewModel

DevicesViewModel manages:

```text id="e7q2m5"
trustedDevices
discoveredDevices
pairingRequests
```

and user actions:

```text
pair
accept
reject
forget
```

It does not implement pairing itself.

---

# 46. SettingsViewModel

SettingsViewModel manages:

```text id="y6n4v2"
sync enabled
paused
notification settings
storage information
```

Only settings actually supported by the underlying application should appear.

---

# 47. Navigation Rules

Navigation should remain shallow.

Preferred:

```text id="j4m8tq"
Home
History
Devices
Settings
```

Avoid:

```text
Home
 └── Device
      └── Device Settings
           └── Connection Settings
                └── Advanced
```

unless a real requirement appears.

ClipSync is primarily a background utility, not a large management application.

---

# 48. First-Run Experience

On first launch, the user should understand what ClipSync does.

A minimal onboarding flow may explain:

```text id="h3q7wm"
1. ClipSync keeps your clipboard synchronized
   between your devices.

2. Enable clipboard capture
   (Android 10+: opens the accessibility
   settings for ClipSync).

3. Pair another device.

4. Leave ClipSync running in the background.
```

Step 2 is required on Android 10+ for background capture (see doc 13, Section 12). If the user skips or denies it, the app must keep working for foreground use, history, and restore, and Settings must show the capture state as not granted.

The onboarding should not become a mandatory multi-page tutorial.

A skip option should be available where appropriate.

---

# 49. First-Run Pairing

After onboarding, the primary action should be:

```text id="q7p2m1"
Pair your first device
```

This should take the user directly to Devices/pairing.

If no device is available:

```text
No devices found.

Make sure both devices are on the
same network and ClipSync is running.
```

The UI should not expose protocol details.

---

# 50. Desktop Relationship

The Android UI should reflect the same conceptual model as the desktop ClipSync application.

Terms such as:

```text
device
trusted
pairing
clipboard history
synchronization
paused
connected
```

should remain consistent across platforms.

The Android UI may adapt layouts for mobile but should not introduce different semantics.

---

# 51. UI and Rust Contract

The UI should depend on the bridge specification rather than internal Rust types.

```text id="h8p4c2"
Compose
  ↓
ViewModel
  ↓
Repository
  ↓
Bridge DTOs
  ↓
Rust Core
```

If the Rust implementation changes internally:

```text
Rust internals
    ↓
Bridge adapter
    ↓
same Android API
```

the UI should not need to change.

---

# 52. MVP UI Scope

The first usable Android UI consists of:

### Home

* synchronization status;
* connected device count;
* device preview;
* recent clipboard preview.

### History

* history list;
* restore;
* clear history.

### Devices

* trusted devices;
* connection state;
* discovered devices;
* pair;
* accept/reject pairing;
* forget device.

### Settings

* enable/disable synchronization;
* pause/resume;
* background capture state and setup (Android 10+);
* device identity;
* history size;
* clear history;
* about.

Everything else is secondary.

---

# 53. Deferred UI Features

The following should not block the MVP:

* advanced clipboard search;
* clipboard categories;
* favorites;
* pinned clipboard entries;
* clipboard editing;
* rich media previews;
* per-device synchronization rules;
* scheduling;
* analytics;
* graphs;
* transfer speeds;
* extensive notification customization.

These can be considered after the basic synchronization experience is reliable.

---

# 54. UI Definition of Done

The UI milestone is complete when:

* Home communicates synchronization state immediately;
* History displays and restores clipboard entries;
* Devices displays trusted/discovered devices;
* pairing can be completed through the UI;
* devices can be forgotten;
* synchronization can be paused/resumed;
* synchronization can be enabled/disabled;
* storage/history information is visible;
* errors have meaningful recovery paths;
* dark and light themes are usable;
* accessibility basics are implemented;
* UI does not directly access Rust internals;
* UI does not contain synchronization logic;
* the complete application remains functional with the Activity closed.

---

# 55. Final UI Architecture

```text id="3h7v1c"
                         Compose UI
                             │
              ┌──────────────┼──────────────┐
              │              │              │
           Home           History        Devices
              │              │              │
              └──────────────┼──────────────┘
                             │
                       ViewModels
                             │
                             ▼
                         Repository
                             │
                  ┌──────────┴──────────┐
                  │                     │
                  ▼                     ▼
             Core Bridge          Android Services
                  │                     │
                  ▼                     ├── Clipboard Monitor
              Rust Core                ├── Clipboard Writer
                  │                     └── Foreground Service
                  │
                  ▼
          Discovery / Pairing /
          Synchronization / DB
```

The central UI rule is:

> **The UI observes and commands ClipSync; it does not become ClipSync.**

