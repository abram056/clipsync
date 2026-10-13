
# Android Service Architecture

## 1. Overview

This document defines the Android-side service architecture for ClipSync.

The Android application has three responsibilities that cannot be delegated to the Rust core:

1. observing Android clipboard changes;
2. writing remote clipboard updates into the Android clipboard;
3. keeping ClipSync's synchronization process alive when the UI is not visible.

These responsibilities are implemented through Android platform components.

The Rust core remains responsible for synchronization, networking, pairing, persistence, deduplication, and peer state.

The resulting architecture is:

```text
┌──────────────────────────────────────────────────────┐
│                    Android App                       │
│                                                      │
│  Compose UI                                          │
│      │                                               │
│      ▼                                               │
│  ViewModel                                            │
│      │                                               │
│      ▼                                               │
│  Repository                                           │
│      │                                               │
│      ▼                                               │
│  ClipSyncCore Bridge                                  │
│      │                                               │
│      ▼                                               │
│  Rust Core                                            │
└──────────────────────────────────────────────────────┘
             ▲                         │
             │                         │
             │                         ▼
     local clipboard            remote clipboard
             │                         │
             ▼                         ▼
┌──────────────────────┐      ┌────────────────────────┐
│ Clipboard Monitoring │      │ Clipboard Writer       │
│ Android component    │      │ Android component      │
└──────────────────────┘      └────────────────────────┘

                ┌──────────────────────────┐
                │ SyncForegroundService    │
                │ keeps core alive         │
                └──────────────────────────┘
```

---

# 2. Architectural Principle

The Android layer must **not become a second synchronization engine**.

The division of responsibility is:

| Responsibility               | Android | Rust |
| ---------------------------- | ------: | ---: |
| Read Android clipboard       |       ✓ |      |
| Write Android clipboard      |       ✓ |      |
| Observe clipboard changes    |       ✓ |      |
| Detect clipboard duplicates  |         |    ✓ |
| Generate clipboard IDs       |         |    ✓ |
| Persist history              |         |    ✓ |
| Peer discovery               |         |    ✓ |
| Pairing                      |         |    ✓ |
| Trust management             |         |    ✓ |
| Network connections          |         |    ✓ |
| Synchronization              |         |    ✓ |
| Loop prevention              |         |    ✓ |
| UI state                     |       ✓ |      |
| Android notifications        |       ✓ |      |
| Foreground-service lifecycle |       ✓ |      |
| Protocol handling            |         |    ✓ |

This separation is critical.

For example, Android should not decide whether a clipboard item should be synchronized.

It should report:

```text
"the local clipboard changed"
```

to Rust.

Rust decides:

```text
whether it is new
whether it should be stored
whether it should be synchronized
which peers should receive it
```

---

# 3. Android Components

The initial Android architecture contains the following components:

```text
android/app/src/main/java/com/abram056/clipsync/

├── core/
│   ├── ClipSyncCore.kt
│   ├── CoreEventCollector.kt
│   └── CoreManager.kt
│
├── data/
│   └── ClipSyncRepository.kt
│
├── service/
│   ├── ClipboardMonitor.kt
│   ├── ClipboardAccessibilityService.kt
│   ├── SyncForegroundService.kt
│   └── ClipboardWriter.kt
│
└── ui/
    └── ...
```

`ClipboardMonitor` is the stable adapter interface (Section 12); `ClipboardAccessibilityService` is the API 29+ capture implementation behind it, with a `ClipboardManager` listener (backed by a primary-clip poll for OEM builds that drop the listener) serving API ≤ 28.

The exact Android component used for clipboard monitoring may depend on the Android API level and the capabilities available to the target devices.

The service boundary defined here should remain stable even if the underlying monitoring mechanism changes.

---

# 4. CoreManager

`CoreManager` owns the single Rust core instance.

Conceptually:

```text
ClipSyncApplication
        │
        ▼
   CoreManager
        │
        ▼
   ClipSyncCore
```

Its responsibilities are:

* create the core;
* start the core;
* expose the running instance;
* prevent duplicate instances;
* stop the core;
* coordinate event collection.

It must not contain synchronization logic.

---

# 5. Single Core Instance

There must be exactly one active ClipSync core instance per Android application process.

The following is prohibited:

```text
Activity ──────► ClipSyncCore #1
Service ───────► ClipSyncCore #2
ViewModel ─────► ClipSyncCore #3
```

This could cause:

* duplicate network listeners;
* duplicate discovery;
* database contention;
* inconsistent peer state;
* duplicated synchronization;
* multiple event streams.

Instead:

```text
                 CoreManager
                     │
                     ▼
                ClipSyncCore
                /     |     \
               /      |      \
          UI Repo   Service   Collector
```

All components access the same logical core instance.

---

# 6. Application Ownership

The core manager should live at application scope rather than Activity scope.

Conceptually:

```kotlin
class ClipSyncApplication : Application() {

    val coreManager: CoreManager by ...

}
```

This prevents Activity recreation from restarting the Rust core.

For example:

```text
Activity created
     │
     ▼
uses existing CoreManager
     │
     ▼
Activity destroyed
     │
     ▼
Core continues running
```

The UI lifecycle must therefore not determine synchronization lifecycle.

---

# 7. SyncForegroundService

The foreground service keeps synchronization alive when ClipSync is operating in background mode.

Its responsibilities are intentionally narrow:

* keep the application process eligible to remain alive;
* ensure the Rust core remains started;
* display the required persistent notification;
* react to service lifecycle events;
* request orderly core shutdown when synchronization is explicitly disabled.

It does **not**:

* perform peer discovery;
* synchronize clipboard contents itself;
* maintain WebSocket connections;
* manipulate the database;
* implement pairing.

Those remain Rust responsibilities.

---

# 8. Foreground Service Lifecycle

The intended lifecycle is:

```text
User enables synchronization
             │
             ▼
Start foreground service
             │
             ▼
Ensure CoreManager is running
             │
             ▼
Start event collector
             │
             ▼
ClipSync operational
```

Stopping synchronization:

```text
User disables synchronization
             │
             ▼
Stop synchronization
             │
             ▼
Stop event collector
             │
             ▼
Stop Rust core
             │
             ▼
Stop foreground service
```

The exact order must ensure that no Android component attempts to call the core after it has been destroyed.

---

# 9. Service Restart

Android may recreate a service after process or service termination depending on the configured service behavior and Android version.

The service must therefore treat `onStartCommand()` as potentially receiving a fresh lifecycle.

On startup it should:

1. obtain the application-level `CoreManager`;
2. check whether the core is already running;
3. start it if required;
4. ensure event collection is active;
5. restore the required service state.

It must not blindly create a second core.

---

# 10. Process Death

Android process death is different from normal service shutdown.

The architecture must assume that the following can disappear without warning:

```text
Activity
ViewModel
Repository instance
Service instance
CoreManager instance
Rust core instance
```

Persistent state must therefore live in durable storage.

The Rust database remains the source of truth for:

* device identity;
* trusted peers;
* clipboard history;
* pairing-related persistent state;
* configuration that is intentionally persisted.

When the application process starts again:

```text
Android Process
      │
      ▼
CoreManager
      │
      ▼
Rust Core
      │
      ▼
Persistent Storage
      │
      ▼
Current State
```

The Android UI reconstructs its state from the core.

---

# 11. Clipboard Monitoring

Clipboard monitoring is an Android-specific responsibility.

The monitor detects changes to the Android clipboard and forwards supported text content to the Rust core.

Conceptually:

```text
Android ClipboardManager
          │
          ▼
Clipboard Monitor
          │
          ▼
Clipboard change detected
          │
          ▼
Repository/Core Bridge
          │
          ▼
ClipSyncCore.onClipboardChanged()
```

The monitor must not directly communicate with peers.

---

# 12. Clipboard Capture Mechanism (Hybrid by API Level)

Android 10 (API 29) restricts clipboard reading to applications that currently have input focus. A regular app in the background — including one running a foreground service — **cannot** read the clipboard on API 29 and above. This constrains the central premise of this architecture (copying in another app while ClipSync runs in the background).

ClipSync therefore commits to a hybrid capture mechanism selected by API level:

| Android version | API level | Mechanism                                                         |
| --------------- | --------- | ----------------------------------------------------------------- |
| 9 and below     | ≤ 28      | `ClipboardManager.OnPrimaryClipChangedListener` + primary-clip poll |
| 10 and above    | ≥ 29      | Minimal `AccessibilityService`                                    |

Note (M6 finding): some OEM builds — observed on Huawei EMUI 8 — never dispatch the platform clip-changed listener at all, so the API ≤ 28 path pairs the listener with a low-frequency primary-clip poll. Both converge on the same filter, so the overlap is idempotent and a pre-existing clip is not a copy event.

Requirements for this decision:

* Both mechanisms sit behind the same internal adapter interface, so the rest of the architecture does not change:

```text
ClipboardMonitor
├── start()
├── stop()
└── onClipboardChanged(content)
```

* The accessibility-service path requires an explicit user action in Android Settings before background capture works. The application must expose this requirement (first-run onboarding and Settings — see doc 14) and must degrade gracefully when it is not granted: local history restore, UI, and foreground sync continue to work; background clipboard *capture* does not.
* The accessibility service must be minimal: it observes clipboard text changes only. It must not inspect, transmit, or log other screen content — this is a privacy requirement, not a preference (Section 44).
* Google Play policy restricts accessibility-service usage to accessibility purposes; this constrains distribution through Play. The mechanism remains valid for sideloaded/internal distribution, and the constraint must be recorded as a known distribution risk.

The exact mechanism behind `ClipboardMonitor` may still evolve per device/vendor behavior, which is why the service boundary — not the mechanism — is the stable contract in this document.

---

# 13. Clipboard Monitoring Responsibilities

The monitor is responsible for:

* detecting clipboard changes;
* extracting supported text content;
* ignoring unsupported clipboard formats for the MVP;
* avoiding obvious self-triggered notification loops where Android APIs make this necessary;
* forwarding local clipboard content to the repository/core.

It is not responsible for:

* hashing;
* assigning clipboard IDs;
* persistence;
* synchronization decisions;
* peer selection;
* deduplication across devices.

---

# 14. Clipboard Change Flow

For a locally copied piece of text:

```text
User copies text
       │
       ▼
Android Clipboard
       │
       ▼
Clipboard Monitor
       │
       ▼
Repository
       │
       ▼
ClipSyncCore
       │
       ▼
Rust Core
       │
       ├── calculate identity/hash
       ├── deduplicate
       ├── store history
       └── synchronize
```

Android should not wait synchronously for synchronization to complete.

The local clipboard operation must remain independent of network state.

---

# 15. Remote Clipboard Flow

For a clipboard update received from another device:

```text
Remote Device
      │
      ▼
Rust Core
      │
      ▼
Bridge Event
      │
      ▼
Repository
      │
      ▼
ClipboardWriter
      │
      ▼
Android ClipboardManager
```

The writer must only write the clipboard.

It must not call:

```text
onClipboardChanged()
```

manually.

The Android clipboard monitoring mechanism may observe the resulting clipboard change naturally.

The Rust core must then use its existing deduplication/origin mechanisms to prevent rebroadcast.

---

# 16. ClipboardWriter

`ClipboardWriter` is an Android-side component responsible for applying remote clipboard updates.

Conceptually:

```kotlin
class ClipboardWriter {

    suspend fun write(content: String)

}
```

It should not know about:

* Rust;
* WebSockets;
* peers;
* pairing;
* history;
* synchronization.

Its only responsibility is:

```text
String → Android Clipboard
```

---

# 17. Clipboard Write Metadata

Where supported and useful, the writer may attach internal metadata to distinguish ClipSync-generated clipboard content.

However, the first implementation should **not depend on Android clipboard metadata** for synchronization correctness.

The Rust core remains authoritative for:

* content identity;
* origin;
* deduplication;
* replay prevention.

Android metadata may be used as an optimization later.

---

# 18. Clipboard Monitoring Loop Prevention

The most important rule is:

> Android-side clipboard monitoring must not become responsible for deciding whether a clipboard update originated locally or remotely.

Example:

```text
Laptop
   │
   │ "hello"
   ▼
Phone Rust Core
   │
   ▼
ClipboardWriter
   │
   ▼
Android Clipboard
   │
   ▼
Clipboard Monitor
   │
   ▼
Rust Core
```

The final step is expected.

The Rust core must recognize that `"hello"` is already known and avoid synchronizing it back to the laptop.

This keeps loop prevention centralized.

---

# 19. Clipboard Debouncing

The Android clipboard monitor may observe multiple callbacks for one logical clipboard operation.

The monitor may therefore perform lightweight debounce/coalescing.

However, debounce must not replace Rust deduplication.

For example:

```text
Android callback
Android callback
Android callback
       │
       ▼
Clipboard Monitor
       │
       ▼
one local clipboard submission
```

Even if the monitor fails to coalesce them, Rust must remain correct.

---

# 20. Clipboard Size

The Android layer should avoid reading arbitrarily large clipboard contents into memory.

The Rust core currently has a configured maximum clipboard size (`SyncConfig.max_clipboard_bytes`, 2 MiB by default). The separate total-history cap is `HistoryConfig.max_size_bytes`.

The Android implementation should enforce an appropriate early limit where practical, but Rust remains the authoritative validation layer.

For oversized content:

```text
Android Clipboard
       │
       ▼
Monitor
       │
       ▼
Core
       │
       ▼
Reject
```

The Android UI may expose an appropriate error/event.

---

# 21. Unsupported Clipboard Content

The MVP supports text.

If the clipboard contains:

* images;
* files;
* rich HTML;
* unsupported MIME types;

the monitor should not attempt to serialize them as text merely to synchronize them.

Instead:

```text
unsupported content
       │
       ▼
ignore / report unsupported type
```

The bridge should remain text-only until the Rust core has an explicit multi-format clipboard model.

---

# 22. Clipboard Service and UI

The clipboard monitor must not depend on an Activity being alive.

This is important because:

```text
Activity lifecycle ≠ synchronization lifecycle
```

The user should be able to:

1. open ClipSync;
2. leave the app;
3. lock the phone;
4. use another application;
5. copy text;
6. have ClipSync synchronize it.

Therefore clipboard monitoring belongs to the background synchronization architecture, not the Compose UI.

---

# 23. Event Collector

`CoreEventCollector` is the Android-side long-lived consumer of bridge events.

Its responsibilities include:

* subscribe to the Rust core;
* convert bridge events into repository state;
* forward remote clipboard events to `ClipboardWriter`;
* update device state;
* update pairing state;
* update synchronization state;
* report errors.

Conceptually:

```text
Rust Event Stream
       │
       ▼
CoreEventCollector
       │
       ├── Device event ──────► Repository
       ├── Pairing event ─────► Repository
       ├── History event ─────► Repository
       ├── Sync event ────────► Repository
       └── Clipboard event ───► ClipboardWriter
```

---

# 24. Collector Lifecycle

The collector should have explicit lifecycle methods:

```text
start()
stop()
```

Starting it twice must not create duplicate subscriptions.

Stopping it must cancel its event collection job.

Conceptually:

```text
CoreManager
     │
     ├── Core
     │
     └── EventCollector
             │
             └── Job
```

The collector job should be owned by an application-level coroutine scope rather than by a Compose screen.

---

# 25. Repository Responsibilities

The repository coordinates Android application state.

It should:

* expose `StateFlow`s;
* invoke bridge operations;
* maintain current UI-relevant state;
* receive events;
* request initial state snapshots;
* coordinate clipboard input/output.

It should not:

* create network connections;
* access SQLite;
* implement synchronization;
* make pairing decisions;
* know Rust implementation details.

---

# 26. Service-to-Repository Relationship

Both services should communicate through application-level components.

Preferred:

```text
Clipboard Monitor
       │
       ▼
Repository
       │
       ▼
Core Bridge
```

and:

```text
Core Event Collector
       │
       ▼
Repository
       │
       ▼
Clipboard Writer
```

Avoid direct service-to-service communication:

```text
Clipboard Service ─────X────► SyncForegroundService
```

This reduces lifecycle coupling.

---

# 27. Service-to-Core Relationship

The foreground service may ensure the core is alive, but it should not expose arbitrary Rust operations.

Avoid:

```text
SyncForegroundService
    ├── core.network()
    ├── core.storage()
    ├── core.discovery()
    └── core.sync()
```

Instead:

```text
SyncForegroundService
        │
        ▼
CoreManager
        │
        ▼
ClipSyncCore
```

The service only controls lifecycle.

---

# 28. Notification

The foreground service requires a persistent Android notification when active.

The notification should communicate:

* ClipSync is running;
* synchronization state;
* optionally connected-device count.

Example conceptual states:

```text
ClipSync
Syncing with 2 devices
```

or:

```text
ClipSync
Paused
```

The notification must not display clipboard contents.

---

# 29. Notification Actions

Potential notification actions include:

```text
Pause
Resume
Open ClipSync
```

The first implementation should keep notification actions minimal.

If `Pause` is implemented, it should call:

```text
Repository
    ↓
ClipSyncCore.setPaused(true)
```

It must not directly stop networking or manipulate the Rust runtime.

---

# 30. Service Start Conditions

The foreground service should only run when background synchronization is enabled.

Conceptually:

```text
Synchronization enabled?
       │
   ┌───┴───┐
   │       │
  yes      no
   │       │
   ▼       ▼
Service   no service
```

The persistent setting controlling this behavior should live outside the service itself.

---

# 31. User-Visible Synchronization State

The UI may distinguish:

```text
Disabled
Starting
Running
Paused
Stopping
Error
```

The Rust core may expose lower-level states.

The Android repository maps those into UI-friendly state.

Example:

```text
Rust lifecycle events
        │
        ▼
Repository mapping
        │
        ▼
SyncState.Running
```

The Compose UI should not interpret raw Rust event types itself.

---

# 32. Startup Sequence

Normal application startup:

```text
Application created
       │
       ▼
Create CoreManager
       │
       ▼
Load persisted Android settings
       │
       ▼
Is synchronization enabled?
       │
   ┌───┴────┐
   │        │
  yes       no
   │        │
   ▼        ▼
Start      UI only
service
   │
   ▼
Start core
   │
   ▼
Start event collector
   │
   ▼
Load initial state
   │
   ▼
Ready
```

---

# 33. Clipboard Monitor Startup

The clipboard monitor should start only after the core is ready to accept clipboard events.

Preferred order:

```text
Start core
   │
   ▼
Start event collector
   │
   ▼
Load repository state
   │
   ▼
Start clipboard monitor
```

This reduces the chance of losing a clipboard event during initialization.

---

# 34. Clipboard Monitor Shutdown

Shutdown should happen in the reverse logical order:

```text
Stop clipboard monitoring
        │
        ▼
Stop accepting new clipboard input
        │
        ▼
Stop event collector
        │
        ▼
Stop Rust core
        │
        ▼
Stop foreground service
```

This prevents new Android clipboard events from arriving after the Rust core has been destroyed.

---

# 35. Race Conditions

The Android implementation must account for races such as:

```text
Clipboard changes
      │
      ├────────► Service stopping
      │
      ▼
Repository submits event
      │
      ▼
Core already stopped
```

The repository must handle `CoreNotRunning`/`CoreStopped` cleanly.

It must not crash the service or application.

Similarly:

```text
Remote event
      │
      ▼
ClipboardWriter
      │
      ├────────► Activity destroyed
      │
      ▼
Android clipboard write
```

Clipboard writing must not depend on an Activity context.

Use application context for components that outlive the UI.

---

# 36. Android Context Rules

Long-lived components must use application-scoped context where possible.

Avoid retaining:

```text
Activity
View
Composable
Fragment
```

inside:

* `CoreManager`;
* `Repository`;
* `ClipboardWriter`;
* foreground service coordination;
* event collectors.

This prevents Activity memory leaks.

---

# 37. Coroutine Ownership

Each long-lived component should have a clearly defined coroutine scope.

Recommended conceptual ownership:

```text
ApplicationScope
│
├── CoreEventCollector
├── Repository state collection
└── background coordination
```

The foreground service may have a service-specific scope for its own lifecycle work.

The clipboard monitor may have a monitor-specific scope if the Android API requires asynchronous processing.

No coroutine should outlive the component that owns it without an explicit reason.

---

# 38. UI Lifecycle

Compose screens observe repository state.

They do not start or stop synchronization simply because they enter or leave composition.

For example:

```text
HomeScreen appears
       │
       ▼
observe StateFlow
```

not:

```text
HomeScreen appears
       │
       ▼
start Rust core
```

Likewise:

```text
HomeScreen disappears
       │
       ▼
stop Rust core
```

must not happen.

---

# 39. Settings Lifecycle

Settings such as:

* synchronization enabled;
* paused;
* notification preferences;
* device display name;

should be managed by application-level state.

Changing synchronization from Settings should result in:

```text
SettingsViewModel
       │
       ▼
Repository
       │
       ▼
CoreManager / Service controller
```

The Settings screen must not directly bind itself to the foreground service implementation.

---

# 40. Failure Recovery

If the Rust core crashes or becomes unavailable:

```text
Core Event Collector
        │
        ▼
Error
        │
        ▼
Repository
        │
        ▼
SyncState.Error
```

The service should attempt recovery according to Android lifecycle constraints.

A recovery attempt should:

1. stop the invalid core instance;
2. release the old event collector;
3. recreate the core;
4. recreate the event collector;
5. reload the initial state.

The implementation should use bounded retry/backoff rather than an infinite tight restart loop.

---

# 41. Network Failure

Network failure is not an Android service failure.

For example:

```text
Wi-Fi unavailable
```

does not mean:

```text
stop ClipSync
```

The Rust core should remain alive and manage reconnection.

Android should present the resulting state:

```text
ClipSync
Waiting for connection
```

The foreground service remains active if background synchronization is enabled.

---

# 42. Device Sleep

Android may suspend or restrict background activity depending on device configuration and power-management policies.

The service architecture should therefore avoid assuming continuous CPU execution.

The Rust core should tolerate:

* temporary network loss;
* delayed timers;
* reconnection;
* process restart.

Android-specific power-management handling should be added only where necessary after testing on real target devices.

---

# 43. Permissions

Android permissions must be handled by the Android application.

Rust must never request Android permissions directly.

The UI should communicate permission requirements to the user.

The service layer should expose clear failure states when a required Android capability is unavailable.

---

# 44. Security and Privacy

The service architecture must follow these rules:

### Clipboard content

Never log clipboard text.

### Notifications

Never display clipboard content.

### Background services

Do not expose clipboard data through intents unless absolutely necessary.

### Inter-component communication

Prefer in-process application objects and structured Kotlin APIs over implicit broadcasts.

### Storage

Use application-private storage for ClipSync data.

### Rust boundary

Do not expose network credentials or internal protocol objects to Android UI code.

---

# 45. Testing Strategy

The service architecture should be tested in layers.

## 45.1 Unit tests

Test:

* repository state updates;
* event mapping;
* clipboard deduplication/coalescing logic;
* lifecycle state transitions.

## 45.2 Bridge integration tests

Test:

```text
Android → Rust
Rust → Android
```

without requiring the complete UI.

## 45.3 Android service tests

Test:

* service starts;
* service does not create duplicate core;
* service stops cleanly;
* process recreation restores state;
* clipboard monitor starts/stops;
* clipboard writer writes remote text.

## 45.4 End-to-end tests

Use two physical devices where possible:

```text
Phone A
   │
   │ clipboard
   ▼
Rust Core
   │
   │ network
   ▼
Phone B
   │
   ▼
Android Clipboard
```

Then reverse the direction.

---

# 46. Minimum End-to-End Scenario

The first complete Android test should be:

```text
0. On every Android 10+ test device, enable the ClipSync
   accessibility capture service in Android Settings.
1. Install ClipSync on Phone A.
2. Install ClipSync on Phone B.
3. Start synchronization on both.
4. Pair the devices.
5. Copy "hello" on Phone A.
6. Confirm Phone B receives "hello".
7. Copy "world" on Phone B.
8. Confirm Phone A receives "world".
9. Close the ClipSync UI on both phones.
10. Repeat the clipboard test.
11. Confirm synchronization continues.
```

This validates the actual purpose of the service architecture.

---

# 47. Background Scenario

The next test (precondition: on Android 10+, the accessibility capture service is enabled):

```text
1. Start ClipSync.
2. Leave the application.
3. Open another application.
4. Copy text.
5. Wait for synchronization.
6. Confirm the other device receives it.
```

This proves that synchronization does not depend on the Compose Activity remaining alive.

---

# 48. Process Restart Scenario

Test:

```text
1. Pair two devices.
2. Synchronize clipboard content.
3. Terminate ClipSync process.
4. Relaunch ClipSync.
5. Confirm trusted devices remain.
6. Confirm history remains.
7. Re-establish synchronization.
```

Persistent state must survive the Android process lifecycle.

---

# 49. Failure Scenario

Test:

```text
1. Start synchronization.
2. Kill or invalidate the Rust core.
3. Observe Android error state.
4. Allow recovery.
5. Confirm core restarts.
6. Confirm event collection resumes.
7. Confirm synchronization resumes.
```

The application must recover without requiring the user to reinstall or manually recreate the database.

---

# 50. Implementation Order

The Android service architecture should be implemented in the following order:

```text
Phase 1
CoreManager                        (doc 15 Milestone 4)
    │
    ▼
Phase 2
Foreground service                 (doc 15 Milestone 5)
    │
    ▼
Phase 3
Event collector                    (doc 15 Milestones 3 + 7)
    │
    ▼
Phase 4
Clipboard writer                   (doc 15 Milestone 6)
    │
    ▼
Phase 5
Clipboard monitor                  (doc 15 Milestone 6)
    │
    ▼
Phase 6
Repository state integration       (doc 15 Milestone 7)
    │
    ▼
Phase 7
Compose UI integration             (doc 15 Milestone 10)
```

Where this sequence differs from doc 15 — notably the event pipeline, which doc 15 schedules at Milestone 3, *before* CoreManager — doc 15 is normative. This section describes the service architecture's internal dependencies, not the project-wide schedule.

Do not implement the clipboard monitor before the Rust bridge and event pipeline are operational.

Otherwise there will be too many moving parts to diagnose simultaneously.

---

# 51. Definition of Done

The Android service architecture is considered implemented when:

* exactly one Rust core instance can be active;
* the core is owned at application scope;
* synchronization can run without an Activity;
* the foreground service keeps synchronization alive;
* the event collector has an explicit lifecycle;
* remote clipboard events reach `ClipboardWriter`;
* local clipboard changes reach the Rust core;
* Android does not implement synchronization logic;
* Android does not access Rust storage directly;
* clipboard content is never logged;
* service restart does not create duplicate cores;
* process restart restores persisted state;
* network failures do not unnecessarily terminate the service;
* core failures can be recovered;
* background clipboard capture follows the hybrid mechanism in Section 12 and its ungranted state is surfaced to the user;
* two physical devices can synchronize while the UI is closed.

---

# 52. Final Architecture

The resulting Android architecture is:

```text
                         Android Process
┌─────────────────────────────────────────────────────────────┐
│                                                             │
│                    ClipSyncApplication                     │
│                           │                                 │
│                           ▼                                 │
│                      CoreManager                            │
│                           │                                 │
│                           ▼                                 │
│                     ClipSyncCore                            │
│                           │                                 │
│                    ┌──────┴──────┐                          │
│                    │             │                          │
│                    ▼             ▼                          │
│              EventCollector   Repository                    │
│                    │             │                          │
│                    │             │                          │
│                    │             ├──────────────┐           │
│                    │             │              │           │
│                    ▼             ▼              ▼           │
│             ClipboardWriter  ViewModels    Clipboard Input  │
│                    │             │              ▲           │
│                    ▼             ▼              │           │
│              Android Clipboard  Compose   Clipboard Monitor │
│                                                             │
│                SyncForegroundService                       │
│                       │                                     │
│                       └────── keeps Core alive              │
│                                                             │
└───────────────────────────┬─────────────────────────────────┘
                            │
                            │ UniFFI
                            ▼
┌─────────────────────────────────────────────────────────────┐
│                       Rust Core                             │
│                                                             │
│  Runtime → Coordinator → Discovery / Sync / Pairing / DB   │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

The architectural rule to preserve throughout implementation is:

> **The foreground service keeps the core alive, the clipboard monitor reports Android clipboard changes, the clipboard writer applies remote changes, and the Rust core remains the sole authority for synchronization.**
