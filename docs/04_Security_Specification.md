## Overview

Clipboard Sync is designed to synchronize clipboard data between trusted personal devices operating on the same local network.

The MVP prioritizes secure communication between trusted devices while maintaining a lightweight, decentralized architecture. Security mechanisms are designed to evolve without requiring significant architectural changes.

---

# Security Objectives

The application should:

- Prevent unauthorized devices from synchronizing clipboard data.
    
- Protect clipboard contents during transmission within the trusted local network (unencrypted ws:// is acceptable for the LAN-trust MVP; TLS is a future enhancement).
    
- Prevent protocol misuse and replay attacks.
    
- Validate all incoming protocol messages.
    
- Protect locally stored clipboard history.
    
- Minimize accidental exposure of sensitive clipboard contents.
    

---

# Threat Model

The MVP assumes:

- Devices belong to the same user.
    
- Devices operate on the same local network.
    
- Users control the participating devices.
    

Potential threats include:

- Unauthorized devices on the LAN.
    
- Message replay attacks.
    
- Malformed protocol messages.
    
- Oversized payload attacks.
    
- Synchronization loops.
    
- Accidental disclosure through logging.
    

Out of scope:

- Malware running on trusted devices.
    
- Physical device compromise.
    
- Advanced network attacks.
    

---

# Device Identity

Each installation generates a unique device identity during initial setup.

Each identity includes:

- Device UUID
    
- Human-readable device name
    

Future versions may extend the identity with cryptographic keys.

---

# Device Discovery

Discovery identifies available devices only.

Discovery does **not** imply trust.

Discovered devices must complete the pairing process before clipboard synchronization is permitted.

---

# Device Pairing

The MVP establishes trust through explicit user confirmation.

Typical workflow:

1. Device discovered.
    
2. Connection requested.
    
3. User approves pairing.
    
4. Device added to trusted device list.
    
5. Future connections occur automatically.
    

Trusted devices remain stored locally.

---

# Authentication

Authentication occurs during connection establishment.

The HELLO handshake verifies:

- Supported protocol version.
    
- Device identity.
    

Unknown devices may connect and enter **Pairing** state, where only PAIRING_* and PING/PONG messages are permitted. Clipboard messages from untrusted devices are rejected (ErrorCode 2). Trust is established through the pairing process (see doc 03).

---

# Message Validation

Every incoming message must be validated before processing.

Validation includes:

- Valid message format.
    
- Supported protocol version.
    
- Required fields present.
    
- Correct payload structure.
    
- Supported content type.
    
- Clipboard size within configured limits.
    

Invalid messages are rejected.

---

# Replay Protection

Each protocol message includes:

- Message ID
    
- Timestamp
    

Previously processed message IDs should be cached.

Duplicate messages are ignored.

Messages outside an acceptable time window may also be rejected.

---

# Clipboard Validation

The MVP accepts only:

- text/plain
    

Maximum clipboard size:

Approximately 2 MB.

Unsupported content types are rejected.

---

# Synchronization Protection

Synchronization loops are prevented through:

- Clipboard identifiers.
    
- Duplicate detection.
    
- Origin device tracking.
    

Previously synchronized clipboard entries must not be redistributed.

---

# Connection Management

Only trusted devices may receive or send synchronization messages.

The Messaging Service should:

- Reject clipboard messages from untrusted connections (pairing-mode sessions only allow pairing and heartbeat messages).
    
- Close inactive connections.
    
- Detect unexpected disconnects.
    
- Reconnect only to trusted peers.
    

---

# Local Data Protection

Sensitive application data includes:

- Clipboard history.
    
- Trusted device information.
    
- Application settings.
    

Data should be stored in an application-specific directory with appropriate file permissions.

Future versions may encrypt local storage.

---

# Logging Policy

Application logs must never record clipboard contents.

Recommended logging information includes:

- Connection status.
    
- Discovery events.
    
- Synchronization status.
    
- Errors.
    
- Device identifiers.
    

Sensitive clipboard content should remain excluded from logs unless explicitly enabled for debugging.

---

# Security Roadmap

Future releases may introduce:

- TLS-encrypted WebSocket connections.
    
- Public/private key identities.
    
- Mutual authentication.
    
- End-to-end encrypted clipboard synchronization.
    
- Digital signatures for protocol messages.
    
- Pairing verification codes.
    
- Trusted device management UI.
    
- Secure encrypted local history storage.
    

---

# Security Principles

Clipboard Sync follows these principles:

- Discovery is not trust.
    
- Trust is established through pairing.
    
- Validate every message.
    
- Never trust network input.
    
- Minimize sensitive data exposure.
    
- Fail securely when validation fails.
    
- Design for future cryptographic enhancements.