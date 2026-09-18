## Overview

Clipboard Sync follows a decentralized peer-to-peer architecture in which every device runs the same application and is capable of discovering, communicating with, and synchronizing clipboard data with other trusted devices on the same local area network.
There is no central server or cloud infrastructure. Every device acts as both a client and a server, allowing clipboard updates to originate from any connected peer.
The architecture is designed to be modular, separating platform-specific functionality from synchronization logic and networking. This separation allows the core synchronization engine to remain reusable across different operating systems while platform adapters handle clipboard access and user interface responsibilities.

---
# High-Level Architecture

```
                    ┌───────────────────────────┐
                    │     Clipboard Monitor     │
                    └──────────────┬────────────┘
                                   │
                          ClipboardChanged
                                   │
                    ┌──────────────▼────────────┐
                    │       Sync Engine         │
                    └───────┬─────────┬─────────┘
                            │         │
                    Update History    │
                            │         │
                    ┌───────▼──────┐  │
                    │ History Mgr  │  │
                    └──────────────┘  │
                                      │
                            Broadcast Updates
                                      │
                    ┌─────────────────▼─────────────────┐
                    │          Network Layer            │
                    │                                  │
                    │  Discovery     Messaging         │
                    └──────────┬───────────┬───────────┘
                               │           │
                          UDP Broadcast  WebSockets
                               │           │
                         Connected Peer Devices
```

---
# Architectural Principles
The system is designed around the following principles:
- Peer-to-peer communication.
- Event-driven synchronization.
- Modular component design.
- Platform-independent core logic.
- Local-first operation.
- Extensibility through well-defined interfaces.

---
# System Components
## Clipboard Monitor
Responsible for interacting with the operating system clipboard.
Responsibilities include:
- Monitoring clipboard changes.
- Reading clipboard contents.    
- Notifying the Sync Engine of local updates.
- Updating the clipboard when instructed by the Sync Engine.
The Clipboard Monitor does not perform networking or history management.

---
## Sync Engine
The Sync Engine is the core component of the application.
Responsibilities include:
- Processing clipboard events.
- Determining whether clipboard content should be synchronized.    
- Preventing synchronization loops.
- Detecting duplicate clipboard entries.
- Coordinating communication between components.
- Managing synchronization state.
All clipboard synchronization decisions originate from this component.

---
## History Manager
Responsible for maintaining the local clipboard history.
Responsibilities include:
- Storing clipboard entries.
- Enforcing the configured history size limit.
- Removing expired or excess entries.
- Restoring previous clipboard entries.
- Providing history data to the user interface.
History remains local to each device during the MVP.

---
## Discovery Service
Responsible for locating compatible devices on the local network.
Responsibilities include:
- Broadcasting discovery requests.
- Listening for discovery responses.
- Maintaining a list of available peers.
- Detecting newly available devices.
- Removing unreachable peers.
The discovery mechanism operates independently of clipboard synchronization.

---
## Messaging Service
Responsible for communication between paired devices.
Responsibilities include:
- Establishing peer connections.
- Sending protocol messages.
- Receiving protocol messages.
- Managing connection lifecycle. 
- Performing heartbeat monitoring.
- Reporting network events to the Sync Engine.

The Messaging Service abstracts all network communication from the rest of the application.

---
## User Interface
Responsible for user interaction.
Responsibilities include:
- Displaying discovered devices.
- Managing trusted devices.
- Viewing clipboard history.
- Restoring clipboard entries.
- Displaying synchronization status.
- Displaying application notifications.
The user interface should contain minimal business logic.

---
# Communication Architecture
The application separates discovery from communication.
## Discovery
Discovery uses UDP broadcasts to identify compatible devices on the local network.
Its only responsibility is locating peers.
Discovery does not establish trust or synchronize clipboard data.

---
## Communication
Once devices have been paired, clipboard synchronization occurs over persistent WebSocket connections.
Persistent connections reduce connection overhead while allowing bidirectional communication between peers.

---
# Connection Lifecycle

```
Application Starts
        │
        ▼
Start Discovery Service
        │
        ▼
Discover Peer
        │
        ▼
Pair / Verify Trust
        │
        ▼
Establish WebSocket Connection
        │
        ▼
Exchange HELLO Messages
        │
        ▼
Connection Established
        │
        ▼
Clipboard Synchronization
        │
        ▼
Heartbeat Monitoring
        │
        ▼
Disconnect / Reconnect
```

---
# Data Flow
### Local Clipboard Update
```
Clipboard Changed
        │
        ▼
Clipboard Monitor
        │
        ▼
Sync Engine
        │
        ├── Duplicate Check
        ├── Loop Prevention
        ├── Update History
        └── Broadcast Update
                │
                ▼
       Messaging Service
                │
                ▼
        Connected Peers
```

---
### Remote Clipboard Update

```
Incoming Message
        │
        ▼
Messaging Service
        │
        ▼
Sync Engine
        │
        ├── Validate Message
        ├── Duplicate Check
        ├── Update History
        └── Update Clipboard
                │
                ▼
        Clipboard Monitor
```

---
# Storage Architecture
The MVP stores all application data locally.
Persistent data includes:
- Device configuration.
- Device identity.
- Trusted peer information.
- Clipboard history.
- Application settings.
SQLite serves as the local persistence layer.

---
# Concurrency Model
The application is event-driven and asynchronous.
Independent tasks include:
- Clipboard monitoring.
- UDP discovery.
- WebSocket communication.
- Synchronization processing.
- History management.
- User interface events.
The Tokio runtime coordinates asynchronous execution while allowing each component to operate independently.

---
# Platform Abstraction
Platform-specific functionality is isolated behind adapters.
Platform-dependent responsibilities include:
- Clipboard access.
- System notifications.
- Application startup integration.
- User interface implementation.
The synchronization engine, networking, protocol handling, and storage remain platform-independent.

---
# Scalability Considerations
Although the primary use case involves two devices, the architecture supports multiple connected peers.
The Sync Engine treats every connected device equally and propagates clipboard updates to all eligible peers (trusted, connected, not paused, and not the origin of the content) while preventing synchronization loops.
No architectural changes are required to increase the number of connected devices.

---
# Architectural Decisions
The following decisions guide the MVP implementation:

| Decision                  | Rationale                                                                        |
| ------------------------- | -------------------------------------------------------------------------------- |
| Peer-to-peer architecture | Eliminates dependency on cloud services and central servers.                     |
| UDP broadcast discovery   | Provides automatic peer discovery on local networks.                             |
| WebSocket communication   | Supports persistent, bidirectional messaging.                                    |
| SQLite storage            | Lightweight, embedded persistence without additional infrastructure.             |
| Rust                      | Strong safety guarantees, excellent concurrency support, and native performance. |
| Event-driven design       | Separates responsibilities between components and simplifies extensibility.      |

---
# Future Architectural Enhancements
Future versions may introduce:
- mDNS/Zeroconf discovery.
- Transport Layer Security (TLS).
- End-to-end encrypted communication.
- Synchronization of additional clipboard formats.
- Plugin support for new clipboard content types.
- Synchronization across different networks.    
- Platform-specific UI enhancements while preserving a shared synchronization core.