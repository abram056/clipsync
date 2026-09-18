## Overview

This document defines the communication contracts used within Clipboard Sync.

Communication occurs at two levels:

- **Internal Events** — exchanged between application components.
    
- **Network Protocol Messages** — exchanged between devices over the network.
    

Keeping these contracts independent allows the internal architecture to evolve without affecting the network protocol.

---

# Internal Event Model

All application components communicate through strongly typed events.

Each event contains:

- Event ID
    
- Timestamp
    
- Event Source
    
- Event Type
    
- Event Payload
    

---

# Event Sources

Events may originate from the following components:

- Clipboard Monitor
    
- Sync Engine
    
- Discovery Service
    
- Messaging Service
    
- History Manager
    
- Pairing Manager
    
- User Interface
    

---

# Internal Events

## Application Lifecycle

### ApplicationStarted

Purpose

Application initialization has completed.

Payload

- Version
    
- Device ID
    
- Device Name
    

---

### ApplicationStopping

Purpose

Application is shutting down.

Payload

- Shutdown Reason (free text — `graceful`, `user`, or `error`)
    

---

## Clipboard Events

### ClipboardChanged

Purpose

Generated whenever the operating system clipboard changes.

Payload

- Content
    
- Content Type
    

---

### ClipboardUpdatedFromRemote

Purpose

Generated after receiving clipboard data from another device.

Payload

- Clipboard ID
    
- Origin Device
    
- Content
    
- Content Type
    
- Content Hash
    

---

### ClipboardHistoryUpdated

Purpose

Clipboard history has been modified.

Payload

- Clipboard ID
    
- Current History Size
    

---

### ClipboardRestoreRequested

Purpose

A previously stored clipboard entry should become the active clipboard.

Payload

- Clipboard ID
    

---

### ClipboardRestored

Purpose

Clipboard restoration completed successfully.

Payload

- Clipboard ID
    

---

## Discovery Events

### DeviceDiscovered

Purpose

A compatible device responded to discovery.

Payload

- Device ID
    
- Device Name
    
- IP Address
    
- Port
    

---

### DeviceConnected

Purpose

A peer connection has been established.

Payload

- Device ID
    
- Device Name
    
- Connection Type (`inbound` | `outbound`)
    

---

### DeviceDisconnected

Purpose

A peer disconnected.

Payload

- Device ID

- Disconnect Reason (`graceful` | `error` | `timeout`)
    

---

### DeviceConnectionFailed

Purpose

A connection attempt failed.

Payload

- Device ID
    
- Error (free text — system error message)
    

---

### PeerListUpdated

Purpose

Available peers changed.

Payload

- Connected Devices (`Vec<DeviceInfo>`)
    

---

## Synchronization Events

### SyncRequested

Purpose

The Sync Engine should distribute clipboard updates.

Payload

- Clipboard ID
    
- Target Devices
    

---

### SyncCompleted

Purpose

Synchronization finished.

Payload

- Clipboard ID
    
- Successful Devices
    
- Failed Devices
    

---

### DuplicateClipboardIgnored

Purpose

Clipboard content has already been processed.

Payload

- Clipboard ID
    
- Reason (`duplicate_content` | `same_origin`)
    

---

## Pairing Events

### PairingRequested

Purpose

An untrusted device has requested to pair with this device.

Payload

- Request ID

- Device UUID

- Device Name

- Platform

---

### PairingAccepted

Purpose

The local user approved the pairing request.

Payload

- Request ID

- Device UUID

---

### PairingRejected

Purpose

The local user declined the pairing request, or the request timed out.

Payload

- Request ID

- Device UUID

- Reason

---

### PairingFailed

Purpose

A pairing operation failed after acceptance (e.g. trust persistence error).

Payload

- Request ID

- Error

---

## Error Events

### NetworkError

Payload

- Device ID
    
- Operation (free text — the operation that failed, e.g. `connect`, `send`)

- Error (free text — system error message)
    

---

### ProtocolError

Payload

- Device ID
    
- Message ID
    
- Error Description (ErrorCode + free text)
    

---

# Network Protocol

## Message Envelope

Every network message uses the following structure:

|Field|Description|
|---|---|
|Message ID|Unique message identifier|
|Message Type|Type of protocol message|
|Device ID|Sender identifier|
|Device Name|Human-readable device name|
|Timestamp|Message creation time|
|Payload|Message-specific data|

---

# Protocol Messages

## Discovery

### DISCOVER

Purpose

Locate compatible devices.

Payload

- Protocol Version
    

---

### DISCOVER_RESPONSE

Purpose

Respond to a discovery request.

Payload

- Protocol Version
    
- Listening Port
    
- Platform
    

---

## Connection

### HELLO

Purpose

Initiate a synchronization session.

Payload

- Protocol Version

- Supported Capabilities (`Vec<String>` — MVP: `["text/plain"]`)
    

---

### HELLO_ACK

Purpose

Accept or reject a connection.

Payload

- Accepted (bool)

- Reason (ErrorCode, optional — present when Accepted is false)
    

---

### GOODBYE

Purpose

Gracefully terminate a session.

Payload

- Disconnect Reason (`graceful` | `error` | `timeout`)
    

---

## Device Pairing

Pairing establishes trust between devices. It is initiated over an existing, unauthenticated connection and requires explicit user approval on the receiving device.

### PAIRING_REQUEST

Purpose

Request trust with the recipient device.

Payload

- Device UUID

- Device Name

- Platform

- Request ID

---

### PAIRING_ACCEPT

Purpose

Confirm that the user approved the pairing request.

Payload

- Request ID

- Device UUID

- Device Name

- Paired At

---

### PAIRING_REJECT

Purpose

Notify the requester that the pairing request was declined or timed out.

Payload

- Request ID

- Reason

---

## Clipboard Synchronization

### CLIPBOARD_UPDATE

Purpose

Transmit clipboard content.

Payload

- Clipboard ID
    
- Origin Device ID
    
- Content Type
    
- Clipboard Content
    
- Content Hash
    
- Creation Timestamp
    

---

### CLIPBOARD_ACK

Purpose

Confirm receipt of clipboard content.

Payload

- Clipboard ID
    
- Status (`Accepted` | `Rejected(ErrorCode)`)
    

---

## Connection Health

### PING

Purpose

Verify peer availability.

Payload

None.

---

### PONG

Purpose

Respond to heartbeat requests.

Payload

None.

---

## Error Handling

### ERROR

Purpose

Report protocol errors.

Payload

- Error Code
    
- Error Message
    
- Related Message ID
    

---

# Connection State Model

Every WebSocket connection transitions through states:

```
         HELLO (unknown device)
                │
                ▼
          ┌──────────┐
          │ Pairing  │  Only PAIRING_* + PING/PONG allowed.
          └────┬─────┘  Clipboard messages rejected (ErrorCode 2).
               │
          PAIRING_ACCEPT
               │
               ▼
          ┌──────────┐
          │ Trusted  │  HELLO_ACK accepted.
          └────┬─────┘
               │
       CLIPBOARD_UPDATE etc.
               │
               ▼
          ┌──────────┐
          │ Syncing  │  Full synchronization.
          └──────────┘
```

An untrusted device may connect and exchange pairing messages; clipboard data flows only after trust is established. Rejected pairings trigger GOODBYE.

---

# Connection Sequence

The normal communication sequence is:

1. Discovery broadcast.

2. Discovery response.

3. WebSocket connection established.

4. HELLO exchanged; HELLO_ACK received. Untrusted connections enter **Pairing** state.

5. If not yet trusted, a PAIRING_REQUEST is sent and the user approves (PAIRING_ACCEPT) or declines (PAIRING_REJECT). Rejecting triggers GOODBYE.

6. Once trusted, clipboard synchronization begins.

7. Periodic heartbeat (PING/PONG).

8. GOODBYE during graceful shutdown.
    

---

# Protocol Constants

## Platform

Enum of supported platform values:

- `linux`

- `android`

- `windows`

- `macos`

---

## Protocol Version

Current Version

- Version 1
    

---

## Discovery

Transport

- UDP Broadcast on port 48271 (TTL=1)
    

---

## Synchronization

Transport

- WebSockets on TCP port 48272 (configurable)
    

---

## Supported Clipboard Types (MVP)

- text/plain
    

---

## Maximum Clipboard Size

2,097,152 bytes (2 MB)

---

# Identifiers

| Identifier | Format | Notes |
| --- | --- | --- |
| Device UUID | UUIDv4 | Generated at first launch; persisted. |
| Device Name | Human-readable string | User-configurable, 1–32 characters. |
| Clipboard ID | UUIDv4 | Assigned per clipboard update; unique across all devices. |
| Message ID | UUIDv4 | Unique per protocol message; used for replay protection. |
| Content Hash | SHA-256 hex | 64-character lowercase hex; used for duplicate detection. |
| Request ID | UUIDv4 | Correlates a PAIRING_REQUEST with its ACCEPT/REJECT. |

---

# Error Code Registry

Every ERROR message must include one of the following codes.

| Code | Name | Description |
| --- | --- | --- |
| 0 | UNKNOWN_ERROR | Unclassified failure. |
| 1 | PROTOCOL_VERSION_UNSUPPORTED | Peer uses an unsupported protocol version. |
| 2 | UNKNOWN_DEVICE | Sender is not a trusted device. |
| 3 | AUTHENTICATION_FAILED | HELLO handshake validation failed. |
| 4 | MALFORMED_MESSAGE | Message failed structural validation. |
| 5 | MISSING_FIELD | A required field is absent. |
| 6 | UNSUPPORTED_CONTENT_TYPE | Payload uses an unsupported clipboard content type. |
| 7 | PAYLOAD_TOO_LARGE | Payload exceeds the maximum clipboard size. |
| 8 | DUPLICATE_MESSAGE | Message ID was already processed (replay). |
| 9 | STALE_MESSAGE | Message timestamp is outside the acceptable window. |
| 10 | PAIRING_DENIED | Pairing request was rejected by the user. |
| 11 | PAIRING_TIMEOUT | Pairing request was not answered in time. |
| 12 | CONNECTION_LIMIT | Peer connection limit was exceeded. |
| 13 | INTERNAL_ERROR | Unexpected internal failure. |

---

# Versioning Strategy

Future protocol versions should preserve backward compatibility where practical.

Breaking protocol changes should increment the protocol version.

Unsupported versions must be rejected during the HELLO handshake.

---

# Extensibility

Future protocol messages may include:

- IMAGE_CLIPBOARD_UPDATE
    
- FILE_TRANSFER_REQUEST
    
- FILE_TRANSFER_CHUNK
    
- FILE_TRANSFER_COMPLETE
    
- HISTORY_REQUEST
    
- HISTORY_RESPONSE
    
- DEVICE_RENAME
    
- DEVICE_CAPABILITIES
    
- ENCRYPTION_HANDSHAKE
    
- SETTINGS_SYNC
    

These messages are reserved for future protocol revisions.