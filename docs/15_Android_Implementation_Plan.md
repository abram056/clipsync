# ClipSync Android Implementation Plan

**Status:** Planned
**Platform:** Android
**Primary language:** Kotlin + Rust
**Bridge:** UniFFI
**UI:** Jetpack Compose
**Core:** Existing Rust `clipboard-core`

---

## 1. Purpose

This document defines the implementation sequence for bringing ClipSync from its existing Rust core into a native Android application.

It is an execution roadmap, not a replacement for the architecture documents.

The implementation must preserve the following boundary:

```text
┌───────────────────────────────────────┐
│              Android                  │
│                                       │
│ Compose → ViewModel → Repository      │
│                    ↓                  │
│             Android Services          │
│                    ↓                  │
│             UniFFI Bridge             │
└────────────────────┬──────────────────┘
                     │
                     ▼
┌───────────────────────────────────────┐
│             Rust Core                 │
│                                       │
│ Sync • Pairing • Discovery • History │
│ Persistence • Networking • Dedup      │
└───────────────────────────────────────┘
```

The implementation should proceed from the bottom of this stack upward.

---

# 2. Implementation Principles

### 2.1 Rust remains the synchronization authority

Android must not reimplement:

* synchronization rules
* deduplication
* loop prevention
* peer trust logic
* pairing protocol
* history persistence
* network behavior
* retry/reconnection logic

Android reports local clipboard changes and consumes the resulting state/events.

---

### 2.2 Android owns Android concerns

Android is responsible for:

* application lifecycle
* foreground services
* clipboard access
* notifications
* permissions
* Compose UI
* Android-specific configuration
* user interaction

---

### 2.3 The bridge is a boundary, not a convenience wrapper

The UniFFI API must expose a deliberate public contract.

Do not expose:

* `AppCommand`
* Tokio types
* `broadcast::Receiver`
* SQLite connections
* WebSocket types
* internal Rust event structures
* internal configuration structures
* storage implementation details

---

### 2.4 Build in vertical slices

Each milestone should produce something testable.

Avoid building the entire Android application and only testing it at the end.

The progression should be:

```text
Rust Core
   ↓
Bridge
   ↓
Events
   ↓
Android lifecycle
   ↓
Background service
   ↓
Clipboard
   ↓
Repository
   ↓
UI
   ↓
End-to-end hardening
```

---

# 3. Milestone 0 — Repository Baseline

### Goal

Establish a clean starting point before Android-specific changes begin.

### Tasks

* Confirm all recent Rust fixes are pushed.
* Confirm the working tree is clean.
* Run existing Rust tests.
* Build the existing workspace.
* Verify the existing Termux/two-device test still works.
* Record the current Rust toolchain and Android development environment.
* Create an Android feature branch from the current `main`.

Suggested branch:

```text
feat/android-app
```

If remote branch creation is unavailable through tooling, create it locally and push it manually.

### Gate

Do not begin Android bridge implementation until:

* existing Rust code builds
* existing tests pass
* the known two-device sync path still works

The Android work must not accidentally become the source of unrelated Rust regressions.

### Record — 2026-10-07

Baseline verified on:

* rustc/cargo 1.97.0; workspace builds clean; 86 tests pass, 0 fail (1 ignored: manual discovery test)
* two-node harness smoke test passes (discovery → auto-pair → auto-approve → clipboard delivery)
* `main` in sync with `origin/main`
* OpenJDK 25.0.4.1 — Gradle/AGP compatibility to be verified in Session 1
* adb 36.0.0 installed; Android SDK/NDK absent (`ANDROID_HOME` unset)
* rustup targets: `x86_64-unknown-linux-gnu` only — Android targets pending Session 1
* no Android device currently attached (required by Milestones 6 and 12)

---

# 4. Milestone 1 — Android Project Skeleton

### Goal

Create the native Android application without connecting it to Rust yet.

### Tasks

Create:

```text
android/
└── app/
```

Establish:

* Gradle configuration
* Kotlin
* Jetpack Compose
* application class
* launcher activity
* basic theme
* package structure
* debug build
* basic navigation shell

Initial package:

```text
com.abram056.clipsync
```

Create the architectural directories defined in the UI/service specifications.

At this stage, screens may contain placeholders.

### Verification

The application should:

1. compile
2. install
3. launch
4. render Compose UI
5. survive Activity recreation

No Rust integration is required yet.

### Gate

A clean Android build must work independently of the Rust bridge.

### Record — 2026-10-09

* Toolchain deviation (user-approved): Gradle 9.3.1 + AGP 9.1.1 + compileSdk/targetSdk 37 — the original AGP 9.0.1/compileSdk 36 pair cannot resolve current stable AndroidX (compose 1.12.1, core 1.19.1, lifecycle 2.11.0, navigation 2.10.2 require AGP ≥ 9.1 and API 37)
* AGP built-in Kotlin: no `org.jetbrains.kotlin.android` plugin; compose compiler plugin 2.2.10 kept
* System JDK is JRE-only (no `javac`); Temurin JDK 21 at `~/tools/jdk21` wired via machine-level `~/.gradle/gradle.properties`
* `./gradlew :app:assembleDebug` green; debug APK ~18 MB
* Bottom-nav skeleton (Home/History/Devices/Settings), light/dark semantic themes

---

# 5. Milestone 2 — UniFFI Bridge Foundation

### Goal

Expose a minimal, stable Android-facing API over the existing Rust core.

### Tasks

Add UniFFI to the Rust workspace.

Create the bridge layer inside `clipboard-core`.

Conceptually:

```text
clipboard-core/
├── bridge/
│   ├── mod.rs
│   ├── types.rs
│   ├── errors.rs
│   └── ...
```

The exact structure can be adjusted during implementation.

Define:

* bridge configuration
* bridge DTOs
* bridge errors
* `ClipSyncCore`
* lifecycle methods
* identity methods

Initial API should cover:

```text
start
stop
identity
```

Then add:

```text
trustedPeers
discoveredPeers
history
pendingPairingRequests
```

followed by commands such as:

```text
requestPairing
approvePairing
rejectPairing
forgetDevice
onClipboardChanged
restoreHistoryEntry
clearHistory
setPaused
```

### Core support required

Two bridge methods need small core changes that are part of this milestone:

* `discoveredPeers` needs a new `ListDiscoveredPeers`-style `AppCommand` returning the coordinator's cached discovery roster (the bridge cannot snapshot it from the event stream alone).
* `restoreHistoryEntry` should return a result so a missing history entry surfaces as a `NotFound` bridge error instead of a silent Rust-side log.

### Important implementation detail

`AppHandle` already owns the Tokio runtime and application command channel.

The bridge should adapt this existing core rather than creating a second synchronization architecture.

Conceptually:

```text
Kotlin
   ↓
ClipSyncCore
   ↓
AppHandle
   ↓
Existing Rust Core
```

### Configuration

Do not expose `AppConfig` directly.

Create an Android-facing configuration containing only values Android legitimately needs to provide.

The Rust side converts that configuration into the internal configuration.

### Build target

The bridge module lives in `clipboard-core`, but Android needs a shared library. Establish a `cdylib` target in this milestone — a dedicated FFI crate (for example `crates/clipboard-ffi`) or an additional crate type — and wire it to the `clipsync.udl` defined in doc 05 Phase E.

### Gate

Before continuing:

* Kotlin can create the Rust core.
* Rust starts successfully.
* Kotlin can retrieve identity.
* Kotlin can stop the core.
* Invalid configuration produces a useful error.
* The Android process does not block while Rust starts.

### Record — 2026-10-09

* Rust bridge (Phase 1): UDL (`clipsync.udl`) + `bridge/` module + `cdylib` target in `clipboard-core`; workspace tests 90 pass (86 baseline + 4 bridge)
* Android wiring (Phase 3): Gradle Exec tasks `generateBindings` (uniffi-bindgen → Kotlin, into `build/generated/uniffi`) and `buildRustBridge` (cargo-ndk arm64-v8a, `--features uniffi`, into `build/jniLibs/cargo`); generated Kotlin sources must be registered via `sourceSets.main.kotlin.directories` — AGP 9 built-in Kotlin does not compile Kotlin from `java.directories`
* JNA must be consumed as `net.java.dev.jna:jna:<version>@aar` — the plain jar contains no Android `libjnidispatch.so`
* `storage_path` is the SQLite database **file** path, not a directory
* INTERNET permission added to the manifest (discovery binds UDP sockets)
* On-device test on BND-L21 (API 26) via `connectedDebugAndroidTest`: `startRetrieveIdentityAndStop` + `invalidConfigurationProducesUsefulError` — 2/2 pass, 0 failures; core starts off the main thread, identity returns non-blank device id/name, clean stop

---

# 6. Milestone 3 — Event Pipeline

### Goal

Create the persistent Rust → Android event path.

This is one of the most important milestones.

The existing event system uses Tokio broadcast channels. The Android layer must not repeatedly create receivers and poll them.

Instead, introduce a bridge-owned subscription mechanism.

Conceptually:

```text
Rust Event
    ↓
Bridge Event Forwarder
    ↓
UniFFI Event
    ↓
Kotlin CoreEventCollector
    ↓
Repository
    ↓
StateFlow
    ↓
ViewModel
    ↓
Compose
```

### Rust tasks

Implement:

* long-lived event receiver
* event conversion
* Android-facing event DTOs
* subscription lifecycle
* cancellation
* lag handling

The bridge should curate internal events rather than expose every internal event.

### Event categories

Initially support events required for:

* lifecycle
* clipboard
* device discovery
* connection state
* pairing
* synchronization
* errors

### Lag handling

If the underlying broadcast receiver reports lag:

```text
Event stream
     ↓
lag detected
     ↓
refresh authoritative snapshots
     ↓
resume event processing
```

The Android repository must not assume that every event will always arrive.

### Kotlin tasks

Implement:

```text
CoreEventCollector
ClipSyncRepository
```

The repository should:

1. start the event collector
2. load initial snapshots
3. consume events
4. update state
5. expose `StateFlow`s to the UI

### Gate

Demonstrate:

```text
Rust state change
      ↓
Bridge event
      ↓
Kotlin event collector
      ↓
Repository state
```

without requiring the UI.

---

# 7. Milestone 4 — CoreManager and Application Lifecycle

### Goal

Guarantee exactly one active Rust core per Android application process.

Create:

```text
ClipSyncApplication
CoreManager
```

The ownership model becomes:

```text
Application
    │
    └── CoreManager
          │
          └── ClipSyncCore
```

Activities, ViewModels, and services access the core through the application-level manager.

### CoreManager responsibilities

* create core
* start core
* expose active core
* stop core
* prevent duplicate instances
* coordinate event collector lifecycle
* handle initialization failures

### It must not

* implement synchronization
* own Compose state
* implement clipboard monitoring
* contain UI logic

### Lifecycle principle

The Activity lifecycle must not determine whether ClipSync is synchronized.

Closing the UI should not necessarily stop synchronization.

### Gate

Verify:

* opening the Activity does not create multiple cores
* Activity recreation does not create another core
* multiple ViewModels can access the same core
* service and UI share the same core instance

---

# 8. Milestone 5 — Foreground Service

### Goal

Keep ClipSync operating when the application UI is not visible.

Implement:

```text
SyncForegroundService
```

Responsibilities:

* keep synchronization alive
* maintain foreground-service state
* display persistent notification
* interact with `CoreManager`
* react to service lifecycle events

It must not contain:

* networking implementation
* pairing implementation
* synchronization algorithms
* database logic

### Startup flow

```text
User enables Sync
       ↓
SyncForegroundService starts
       ↓
CoreManager ensures core exists
       ↓
Core starts
       ↓
Event collector starts
       ↓
Sync runs
```

### Failure handling

If the Rust core fails:

```text
Core failure
    ↓
report error
    ↓
bounded retry/recovery
    ↓
restore service/core state
```

Avoid infinite rapid restart loops.

### Gate

The user should be able to:

1. start synchronization
2. leave the Activity
3. lock the device
4. return later
5. find synchronization still running

---

# 9. Milestone 6 — Android Clipboard Integration

### Goal

Connect the Android clipboard to the Rust synchronization core.

Create:

```text
ClipboardMonitor
ClipboardWriter
```

The implementation should hide Android clipboard details behind these components.

### Clipboard access mechanism

Android 10 (API 29) blocks clipboard reads for apps without input focus; a foreground service does not grant access. This milestone must implement the hybrid mechanism decided in doc 13, Section 12:

* API ≤ 28: `ClipboardManager.OnPrimaryClipChangedListener`
* API ≥ 29: a minimal `AccessibilityService` observing clipboard text changes only

Both sit behind the `ClipboardMonitor` interface. The user-facing enablement step (accessibility settings) is part of Milestone 10's onboarding/Settings work, but the service itself must exist before this milestone's gate can pass on Android 10+ devices.

---

## 9.1 Local clipboard → Rust

Flow:

```text
Android Clipboard
      ↓
ClipboardMonitor
      ↓
Repository
      ↓
ClipSyncCore.onClipboardChanged()
      ↓
Rust synchronization
```

The monitor may perform lightweight filtering/debounce.

It must not determine synchronization correctness.

---

## 9.2 Rust → Android clipboard

Flow:

```text
Remote clipboard event
       ↓
CoreEventCollector
       ↓
Repository
       ↓
ClipboardWriter
       ↓
Android ClipboardManager
```

Rust remains responsible for preventing synchronization loops.

### Important test

Test:

```text
Phone A clipboard
      ↓
Rust
      ↓
Phone B clipboard
      ↓
Phone B clipboard monitor
      ↓
Rust
```

The change must not bounce indefinitely between devices.

### Gate

Precondition: on every Android 10+ test device, the accessibility capture service is enabled.

Physical-device testing must demonstrate:

* Android → laptop
* laptop → Android
* Android → Android
* repeated identical clipboard values
* rapid clipboard changes
* application UI closed
* background capture while another app is focused (API 29+, service enabled)
* screen locked where Android permits the configured clipboard behavior

---

# 10. Milestone 7 — Repository and Domain State

### Goal

Complete the Kotlin state layer before building the full UI.

Implement:

```text
Device
HistoryItem
PairingRequest
SyncState
```

and the repository operations required by the UI.

The repository becomes the single Android-facing state coordinator.

Example:

```text
Repository
├── syncState
├── devices
├── pairingRequests
├── history
├── recentClipboard
└── errors
```

Expose these through `StateFlow`/appropriate reactive streams.

### Initial state loading

When the core starts:

```text
Core starts
    ↓
Repository requests snapshots
    ↓
Repository initializes StateFlows
    ↓
Event collector maintains state
```

This is important because events alone are not sufficient to reconstruct the entire current state.

### Gate

The UI should be able to consume realistic repository state without directly interacting with Rust.

---

# 11. Milestone 8 — Pairing and Device Management

### Goal

Implement the complete device-management workflow.

Implement repository operations for:

* discovered devices
* pairing requests
* requesting pairing
* approving pairing
* rejecting pairing
* trusted devices
* forgetting devices

Device states:

```text
Discovered
Pairing
Trusted
Connected
Disconnected
```

### Pairing flow

```text
Devices screen
      ↓
Select discovered device
      ↓
Request pairing
      ↓
Remote approval
      ↓
Trusted
      ↓
Connected
```

Discovery must never automatically imply trust.

### Gate

Test pairing between at least two physical devices.

Verify:

* rejection
* approval
* duplicate requests
* timeout/expiry
* already-trusted devices
* forgetting a device
* reconnecting a previously trusted device

---

# 12. Milestone 9 — History

### Goal

Expose clipboard history through the Android repository and UI.

Implement:

* history retrieval
* pagination/limited retrieval where supported
* local search/filtering
* restore
* clear history
* storage-size display if required by the UI

History remains persisted by Rust.

Android does not create a second history database.

### Restore flow

```text
History item
    ↓
Repository.restoreHistoryEntry()
    ↓
Rust Core
    ↓
Android clipboard update
```

### Gate

Verify history survives:

* Activity restart
* service restart
* core restart
* process restart where persistent storage is expected

---

# 13. Milestone 10 — Compose UI

### Goal

Build the user-facing application on top of the now-stable repository.

UI implementation order:

1. Theme
2. Navigation
3. Home
4. History
5. Devices
6. Pairing
7. Settings
8. Error/loading/empty states

---

## 13.1 Theme

Implement:

* light theme
* dark theme
* semantic colors
* typography
* spacing
* reusable components

Avoid screen-specific hardcoded colors where semantic theme values are appropriate.

---

## 13.2 Home

Display:

* synchronization status
* connected device count
* device preview
* recent clipboard activity
* pause/resume control where appropriate

---

## 13.3 History

Implement:

* history list
* clipboard preview
* restore/copy
* search/filter
* clear history

---

## 13.4 Devices

Implement:

* trusted devices
* discovered devices
* connection status
* pairing actions
* forget-device action
* pairing requests

---

## 13.5 Settings

Implement:

* synchronization enable/disable
* pause/resume
* device identity
* history management
* notifications
* about

Device-name editing should remain deferred if the Rust core does not yet provide a stable setter.

### UI rule

Compose must never call UniFFI directly.

```text
Compose
   ↓
ViewModel
   ↓
Repository
   ↓
CoreManager / ClipSyncCore
```

---

# 14. Milestone 11 — Background and Lifecycle Hardening

### Goal

Make the application reliable outside the happy path.

Test:

### Activity lifecycle

* Activity recreation
* rotation/configuration change
* app backgrounding
* app foregrounding

### Service lifecycle

* service start
* service stop
* service restart
* notification interaction
* process death

### Core lifecycle

* startup failure
* shutdown
* repeated start/stop
* network unavailable at startup
* network becoming available later

### Event lifecycle

* collector cancellation
* collector restart
* event lag
* snapshot refresh
* duplicate collector prevention

### Gate

No duplicated:

* core instances
* event collectors
* clipboard monitors
* service work

---

# 15. Milestone 12 — Cross-Device End-to-End Testing

### Goal

Validate the actual product rather than isolated components.

Minimum test matrix:

| Test                | Expected result               |
| ------------------- | ----------------------------- |
| Android → Laptop    | Clipboard arrives             |
| Laptop → Android    | Clipboard arrives             |
| Android → Android   | Clipboard arrives             |
| Background capture  | Clipboard captured while another app is focused (API 29+, service enabled) |
| Duplicate clipboard | No unnecessary sync loop      |
| Rapid changes       | Correct final state           |
| Device discovery    | Device appears                |
| Pairing approval    | Device becomes trusted        |
| Pairing rejection   | Device remains untrusted      |
| Forget device       | Trust is removed              |
| Network loss        | Core remains alive/reconnects |
| Network recovery    | Sync resumes                  |
| UI closed           | Background sync continues     |
| Service restart     | Core recovers                 |
| App restart         | Persistent state restored     |
| History restore     | Clipboard is restored         |
| History clear       | History is removed            |

---

# 16. Milestone 13 — Release Hardening

### Logging

Audit every log statement.

Never log clipboard contents.

Avoid logging:

* clipboard text
* sensitive pairing information
* unnecessary device identifiers
* persistent secrets

Use structured logging with appropriate release verbosity.

---

### Error handling

Every user-visible failure should have a useful Android-facing error.

Avoid leaking internal Rust errors directly into the UI.

Conceptually:

```text
Rust internal error
       ↓
Bridge error
       ↓
Repository/domain error
       ↓
User-facing message
```

---

### Resource management

Verify:

* Tokio runtime shutdown
* event subscription cancellation
* coroutine cancellation
* service cleanup
* clipboard listener cleanup
* no leaked references to Activity contexts

---

### Build

Produce:

* debug build
* release build
* signed release configuration

Confirm that the application works without development tooling attached.

---

# 17. Milestone Dependencies

The implementation order is intentional.

```text
                 ┌───────────────────┐
                 │ Repository Baseline│
                 └─────────┬─────────┘
                           ↓
                 ┌───────────────────┐
                 │ Android Skeleton  │
                 └─────────┬─────────┘
                           ↓
                 ┌───────────────────┐
                 │ UniFFI Bridge     │
                 └─────────┬─────────┘
                           ↓
                 ┌───────────────────┐
                 │ Event Pipeline    │
                 └─────────┬─────────┘
                           ↓
                 ┌───────────────────┐
                 │ CoreManager       │
                 └─────────┬─────────┘
                           ↓
                 ┌───────────────────┐
                 │ Foreground Service│
                 └─────────┬─────────┘
                           ↓
                 ┌───────────────────┐
                 │ Clipboard Layer   │
                 └─────────┬─────────┘
                           ↓
                 ┌───────────────────┐
                 │ Repository State  │
                 └─────────┬─────────┘
                           ↓
                 ┌───────────────────┐
                 │ Pairing / History │
                 └─────────┬─────────┘
                           ↓
                 ┌───────────────────┐
                 │ Compose UI        │
                 └─────────┬─────────┘
                           ↓
                 ┌───────────────────┐
                 │ E2E Hardening     │
                 └─────────┬─────────┘
                           ↓
                 ┌───────────────────┐
                 │ Release           │
                 └───────────────────┘
```

---

# 18. Implementation Gates

Do not proceed merely because code compiles.

### Gate A — Rust/Bridge

Must demonstrate:

```text
Kotlin → Rust → Kotlin
```

with lifecycle and identity.

---

### Gate B — Events

Must demonstrate:

```text
Rust event → Kotlin repository state
```

without UI involvement.

---

### Gate C — Background runtime

Must demonstrate:

```text
UI closed → Rust continues running
```

---

### Gate D — Clipboard

Must demonstrate:

```text
Android ↔ Rust ↔ another device
```

with loop prevention.

---

### Gate E — UI

Only begin full UI work after repository state and bridge behavior are stable.

---

### Gate F — MVP

Android MVP is not complete until:

```text
Two devices
      +
Pairing
      +
Background synchronization
      +
Clipboard propagation
      +
History
      +
Process/service recovery
```

work together.

---

# 19. Testing Strategy

Testing should exist at multiple levels.

## Rust

Continue testing:

* synchronization
* deduplication
* pairing
* persistence
* protocol behavior
* history
* reconnection

---

## Bridge

Test:

* DTO conversion
* invalid arguments
* lifecycle
* error conversion
* UUID conversion
* timestamp conversion
* event conversion
* subscription cancellation
* discovered-peers snapshot conversion
* `restoreHistoryEntry` missing-entry `NotFound` error

---

## Kotlin

Test:

* repository state transitions
* ViewModels
* pairing state
* sync state
* event handling
* event-lag recovery
* service/core lifecycle coordination

---

## Instrumentation

Use real Android devices for:

* clipboard access
* foreground service
* notifications
* background behavior
* process death
* device discovery
* actual network communication

Android emulators may be useful for UI tests but should not be the only validation environment.

---

# 20. What Not to Build Yet

The following should remain outside the MVP unless implementation reveals a genuine requirement:

* rich clipboard formats
* images/files
* clipboard attachments
* cloud synchronization
* accounts
* analytics
* transfer-speed dashboards
* clipboard categories
* favorites/pinning
* per-device sync rules
* advanced search
* cloud history
* multiple simultaneous Android core instances
* elaborate onboarding
* widgets
* Wear OS support

The first Android release should prove the core idea:

> **A phone can participate in ClipSync as a first-class peer while the synchronization logic remains shared Rust code.**

---

# 21. Definition of Android MVP

ClipSync Android MVP is complete when all of the following are true:

* The application starts the existing Rust core.
* Rust remains responsible for synchronization.
* Android can discover peers.
* Android can request and complete pairing.
* Trusted devices are displayed.
* Clipboard changes synchronize between devices.
* Clipboard changes are captured in the background on Android 10+ when the accessibility service is enabled.
* Synchronization continues while the UI is closed.
* Android can write remote clipboard content.
* Synchronization does not endlessly bounce clipboard changes.
* Clipboard history is available.
* History can be restored and cleared.
* Core state survives normal service/UI lifecycle changes.
* Network interruption does not permanently break synchronization.
* Event loss can recover through state refresh.
* The UI reflects actual Rust/core state.
* No synchronization logic is duplicated in Kotlin.
* Clipboard content is not unnecessarily written to logs.
* Two physical devices can complete the end-to-end workflow.

---

# 22. Recommended First Implementation Session

When implementation begins, do **not** start with Compose screens.

The first coding session should be:

```text
1. Verify current main
2. Create Android skeleton
3. Add UniFFI
4. Create bridge module
5. Define bridge DTOs/errors
6. Implement ClipSyncCore lifecycle
7. Generate Kotlin bindings
8. Launch core from a minimal Android test
9. Retrieve device identity
10. Stop core cleanly
```

Then stop and test.

The next session should focus exclusively on the event pipeline.

This keeps the difficult Rust↔Android boundary isolated before Android UI and services add additional moving parts.

---

# 23. Source-of-Truth Documents

The implementation should use these documents together:

```text
docs/
├── 12_Android_Bridge_Specification.md
├── 13_Android_Service_Architecture.md
├── 14_Android_UI_Specification.md
└── 15_Android_Implementation_Plan.md
```

Their responsibilities are different:

| Document                             | Purpose                              |
| ------------------------------------ | ------------------------------------ |
| `12_Android_Bridge_Specification.md` | Rust ↔ Android contract              |
| `13_Android_Service_Architecture.md` | Android ↔ OS/background architecture |
| `14_Android_UI_Specification.md`     | User-facing behavior                 |
| `15_Android_Implementation_Plan.md`  | Implementation order and gates       |

If implementation decisions conflict with these documents, update the relevant specification rather than silently changing the architecture in code.

---

# 24. Final Architecture Target

The completed Android application should converge on:

```text
                    Android App
                         │
              ┌──────────┴──────────┐
              │                     │
          Compose UI           Android OS
              │                     │
         ViewModels          Foreground Service
              │                     │
         Repository        Clipboard Monitor
              │                     │
              └──────────┬──────────┘
                         │
                    CoreManager
                         │
                  UniFFI Bridge
                         │
                  ┌──────┴──────┐
                  │  Rust Core  │
                  │             │
                  │ Sync        │
                  │ Pairing     │
                  │ Discovery   │
                  │ History     │
                  │ Persistence │
                  │ Networking  │
                  └─────────────┘
```

The architectural rule to preserve throughout implementation is:

> **Android owns Android concerns. Rust owns synchronization concerns. The bridge owns the boundary.**

And the practical implementation rule is:

> **Do not move to the next layer until the current layer has a testable, working boundary.**

