# Clipboard Sync

## Project Overview

Clipboard Sync is a peer-to-peer clipboard synchronization application designed to simplify the transfer of clipboard data between personal devices on the same local network. The application automatically synchronizes clipboard contents between connected devices, allowing users to copy content on one device and immediately paste it on another without relying on messaging applications, cloud storage, or manual retyping.

The project is designed to support synchronization between any two or more compatible devices. While the initial use case focuses on synchronizing a laptop and a mobile phone, the architecture is intended to support additional device types and multiple connected peers in future versions.

---
# Problem Statement
When working across multiple devices, users frequently encounter situations where they need to transfer small pieces of information such as text snippets, URLs, commands, or code fragments from one device to another. Existing methods—including messaging applications, email, note-taking services, or manually typing the content—introduce unnecessary interruptions for information that is only intended to be copied once.

Clipboard Sync addresses this problem by providing automatic clipboard synchronization between trusted devices connected to the same local network.

---
# Project Objectives
The project aims to:
- Automatically synchronize clipboard contents between connected devices.
- Support bidirectional synchronization where every device can both send and receive clipboard updates.
- Eliminate the need for intermediary applications when transferring clipboard content.
- Maintain a bounded clipboard history for quick recovery of recently copied content.
- Operate entirely within a local area network without requiring internet connectivity or cloud infrastructure.
- Provide a lightweight, responsive, and extensible synchronization service suitable for personal use.

---
# Scope
## In Scope
The MVP includes:
- Automatic synchronisation of plain text clipboard content.
- Bidirectional synchronization between connected devices.
- Local clipboard history with a storage limit of approximately 2 MB.
- Peer-to-peer communication over a local network.
- Automatic device discovery.
- Device pairing and trusted-device management.
- Clipboard history browsing and restoration.
- Support for multiple connected peers.
## Out of Scope (MVP)
The following features are intentionally excluded from the initial release:
- Internet-based synchronization.
- Cloud storage or user accounts
- Image clipboard synchronization. 
- File synchronization.
- Rich text and HTML clipboard formats.
- End-to-end encryption.
- Cross-user collaboration.
- Long-term clipboard history synchronization.
These features remain potential enhancements for future versions.

---
# Functional Requirements
The application shall:
- Detect changes to the local clipboard.
- Synchronize clipboard updates with trusted devices.
- Receive and apply clipboard updates from connected peers.
- Prevent synchronization loops and duplicate updates.
- Maintain a clipboard history with automatic size management.
- Allow users to restore previous clipboard entries.
- Discover compatible devices on the same local network.
- Establish trusted peer connections through a pairing process.
- Synchronize automatically whenever paired devices become available.
---
# Non-Functional Requirements
The application should:
- Operate with minimal latency under normal local network conditions.
- Consume minimal system resources while running in the background.
- Be reliable during temporary network interruptions.
- Be extensible to support additional clipboard formats in future versions.
- Be portable across multiple operating systems.
- Maintain a modular architecture that separates networking, synchronization, storage, and platform-specific functionality

---
# Technology Stack (MVP)

| Component            | Technology                                              |
| -------------------- | ------------------------------------------------------- |
| Programming Language | Rust                                                    |
| Networking           | UDP Broadcast (Discovery), WebSockets (Synchronization) |
| Serialization        | JSON                                                    |
| Local Storage        | SQLite                                                  |
| Async Runtime        | Tokio                                                   |
| Clipboard Access     | arboard                                                 |
| Logging              | tracing                                                 |

Additional libraries may be introduced as implementation progresses.

---
# MVP Features
- Plain text clipboard synchronization.
- Automatic clipboard monitoring.
- Bidirectional synchronization.
- Automatic local network device discovery.
- Persistent peer connections.
- Clipboard history (approximately 2 MB maximum).
- Duplicate detection.
- Synchronization loop prevention.
- Manual restoration of clipboard history.
- Device identification and friendly device names.

---
# Future Enhancements
Potential future releases may introduce:
- Image clipboard synchronization.
- File transfer support.
- Rich text and HTML clipboard synchronisation.
- Automatic mDNS service discovery.
- End-to-end encrypted communication.
- Public-key based device authentication.
- Synchronisation across the internet.
- Device groups and shared workspaces.
- Clipboard search and filtering.
- Clipboard history synchronisation across newly paired devices.
- Cross-platform graphical user interfaces.

---
# Development Roadmap
## Phase 1
Project planning, architecture design, protocol specification, and development environment setup.
## Phase 2
Implementation of the core synchronisation engine, clipboard monitoring, local history management, and peer discovery.
## Phase 3
Implementation of peer communication, synchronisation protocol, and device pairing.
## Phase 4
Development of user interface components, testing, optimisation, and cross-platform support.
## Phase 5
Future enhancements including security improvements, richer clipboard formats, and advanced synchronisation capabilities.

---
# Success Criteria
The MVP will be considered successful if it can:
- Automatically discover compatible devices on the same local network.
- Establish trusted peer connections.
- Synchronize plain text clipboard content between paired devices with minimal latency.
- Maintain reliable clipboard history without synchronization loops or duplicate entries.
- Operate continuously with minimal resource consumption while providing a seamless clipboard sharing experience. For the desktop node this is stated measurably: an idle node with no clipboard traffic must average under 1% CPU (measured over one minute on a release build) and hold resident memory below 40 MB. Both figures and the measurement method are recorded in doc 11.