# P2P PBFT - Complete Technical Documentation

This document provides an in-depth technical explanation of the P2P network with PBFT consensus implementation. It covers the theoretical foundations, architectural decisions, and detailed code explanations.

## Table of Contents

1. [Introduction](#1-introduction)
2. [Theoretical Background](#2-theoretical-background)
   - [Byzantine Fault Tolerance](#21-byzantine-fault-tolerance)
   - [PBFT Algorithm](#22-pbft-algorithm)
   - [P2P Networking Concepts](#23-p2p-networking-concepts)
3. [Architecture Deep Dive](#3-architecture-deep-dive)
4. [Code Walkthrough](#4-code-walkthrough)
   - [Cryptography Module](#41-cryptography-module-cryptors)
   - [Message Module](#42-message-module-messagers)
   - [Network Module](#43-network-module-networkrs)
   - [PBFT Module](#44-pbft-module-pbftrs)
   - [Node Module](#45-node-module-noders)
   - [Main Module](#46-main-module-mainrs)
5. [Data Flow](#5-data-flow)
6. [Security Considerations](#6-security-considerations)
7. [Performance Optimization](#7-performance-optimization)
8. [Comparison with libp2p](#8-comparison-with-libp2p)
9. [Future Improvements](#9-future-improvements)

---

## 1. Introduction

This project implements a distributed consensus system using the Practical Byzantine Fault Tolerance (PBFT) algorithm over a custom peer-to-peer network. The implementation is written in Rust, leveraging its strong type system and memory safety guarantees to build a robust distributed system.

### Why PBFT?

PBFT was chosen because:
- It provides **deterministic finality** (once committed, transactions cannot be reversed)
- It tolerates **Byzantine failures** (malicious nodes)
- It has **low latency** compared to proof-of-work systems
- It's well-suited for **permissioned networks** with known participants

### Why Custom P2P Instead of libp2p?

While libp2p is an excellent library, we built a custom P2P layer to:
- Demonstrate core networking concepts clearly
- Reduce dependencies and complexity
- Have full control over the protocol
- Keep the codebase educational and easy to understand

---

## 2. Theoretical Background

### 2.1 Byzantine Fault Tolerance

#### The Byzantine Generals Problem

The Byzantine Generals Problem, introduced by Lamport, Shostak, and Pease in 1982, describes the challenge of achieving consensus in a distributed system where some participants may be faulty or malicious.

**Scenario**: Several Byzantine generals surround a city. They must agree on a common battle plan (attack or retreat). Some generals may be traitors who send conflicting messages.

**Key Insight**: With `n` generals and `f` traitors, consensus is possible if and only if `n >= 3f + 1`.

#### Why 3f + 1?

Consider a network with `n` nodes and `f` Byzantine (faulty) nodes:

```
Total nodes:     n = 3f + 1
Byzantine nodes: f (can send arbitrary messages)
Honest nodes:    n - f = 2f + 1

Quorum needed:   2f + 1 (majority of honest nodes)
```

**Proof intuition**:
- We need `2f + 1` nodes to agree (quorum)
- Even if `f` Byzantine nodes vote maliciously, `f + 1` honest nodes in the quorum guarantee correctness
- Two quorums of size `2f + 1` must overlap by at least one honest node

```
Example with n=4, f=1:

Nodes: [A, B, C, D] where D is Byzantine

Quorum = 2(1) + 1 = 3

If A, B, C agree → consensus achieved (even if D disagrees)
Byzantine node D cannot forge enough votes to create a conflicting consensus
```

### 2.2 PBFT Algorithm

#### Overview

PBFT (Practical Byzantine Fault Tolerance) was introduced by Castro and Liskov in 1999. It provides state machine replication that tolerates Byzantine faults.

#### System Model

```
┌─────────────────────────────────────────────────────────────┐
│                         CLIENTS                              │
│                    (Request originators)                     │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                         REPLICAS                             │
│  ┌─────────┐  ┌─────────┐  ┌─────────┐  ┌─────────┐        │
│  │ Primary │  │Replica 1│  │Replica 2│  │Replica 3│        │
│  │(Leader) │  │(Backup) │  │(Backup) │  │(Backup) │        │
│  └─────────┘  └─────────┘  └─────────┘  └─────────┘        │
└─────────────────────────────────────────────────────────────┘
```

**Roles**:
- **Primary (Leader)**: Orders client requests, initiates consensus
- **Backups**: Validate and participate in consensus
- **Clients**: Send requests, wait for responses

#### The Three Phases

PBFT consensus proceeds in three phases:

```
                    NORMAL CASE OPERATION

Client ──Request──► Primary
                      │
        ┌─────────────┼─────────────┐
        ▼             ▼             ▼
    ┌───────┐    ┌───────┐    ┌───────┐
    │Phase 1│    │Phase 2│    │Phase 3│
    │Pre-   │───►│Prepare│───►│Commit │───► Execute
    │prepare│    │       │    │       │
    └───────┘    └───────┘    └───────┘
```

##### Phase 1: Pre-Prepare

The primary assigns a sequence number to the request and broadcasts a PRE-PREPARE message.

```rust
PrePrepare {
    view: u64,           // Current view number
    sequence: u64,       // Sequence number assigned
    digest: [u8; 32],    // Hash of the request
    request: Request,    // The actual request
    primary_id: String,  // Primary's identifier
    signature: Vec<u8>,  // Primary's signature
}
```

**Validation by backups**:
1. The message is in the current view
2. The signature is valid
3. The sequence number is within the valid range (water marks)
4. No conflicting pre-prepare exists for this sequence

##### Phase 2: Prepare

Each backup that accepts the pre-prepare broadcasts a PREPARE message.

```rust
Prepare {
    view: u64,
    sequence: u64,
    digest: [u8; 32],
    replica_id: String,
    signature: Vec<u8>,
}
```

A replica has **prepared** when it has:
- A valid pre-prepare for `(view, sequence, digest)`
- `2f` prepares from different replicas matching the pre-prepare

##### Phase 3: Commit

Once prepared, a replica broadcasts a COMMIT message.

```rust
Commit {
    view: u64,
    sequence: u64,
    digest: [u8; 32],
    replica_id: String,
    signature: Vec<u8>,
}
```

A replica has **committed-local** when it has:
- Prepared the request
- `2f + 1` commits from different replicas (including itself)

##### Execution

After committed-local, the request is executed in sequence number order.

#### Message Flow Diagram

```
    Client     Primary    Replica1    Replica2    Replica3
       │          │          │           │           │
       │─Request─►│          │           │           │
       │          │          │           │           │
       │          │──────PrePrepare─────────────────►│
       │          │          │           │           │
       │          │◄─────────────Prepare─────────────│
       │          │          │◄──────────────────────│
       │          │◄─────────│           │           │
       │          │          │───────────────────────►
       │          │          │           │           │
       │          │◄──────────────Commit─────────────│
       │          │          │◄──────────────────────│
       │          │◄─────────│           │           │
       │          │          │───────────────────────►
       │          │          │           │           │
       │◄──Reply──│◄─────────│◄──────────│◄──────────│
       │          │          │           │           │
```

#### View Change Protocol

When the primary fails or behaves maliciously, the system performs a view change:

```
                    VIEW CHANGE PROTOCOL

Timeout detected → Broadcast ViewChange → New Primary sends NewView
                                              │
     ┌────────────────────────────────────────┘
     ▼
Resume normal operation with view = view + 1
```

**ViewChange Message**:
```rust
ViewChange {
    new_view: u64,          // The new view number
    replica_id: String,     // Sender's ID
    last_sequence: u64,     // Last committed sequence
    signature: Vec<u8>,
}
```

**NewView Message** (from new primary):
```rust
NewView {
    new_view: u64,
    view_changes: Vec<ViewChange>,  // 2f+1 ViewChange messages
    primary_id: String,
    signature: Vec<u8>,
}
```

**Primary Selection**:
```
primary_index = view_number % number_of_replicas
```

#### Checkpointing

Checkpoints prevent unbounded log growth and enable garbage collection:

```rust
Checkpoint {
    sequence: u64,       // Checkpoint sequence number
    digest: [u8; 32],    // State hash at this point
    replica_id: String,
    signature: Vec<u8>,
}
```

**Stable Checkpoint**: When `2f + 1` replicas agree on a checkpoint, it becomes stable.

**Water Marks**:
- `low_watermark`: Sequence of last stable checkpoint
- `high_watermark`: `low_watermark + checkpoint_interval * 2`

### 2.3 P2P Networking Concepts

#### Peer-to-Peer Architecture

Unlike client-server models, P2P networks have no central authority:

```
        Client-Server                    Peer-to-Peer

           Server                      ┌───┐     ┌───┐
             │                         │ P │◄───►│ P │
     ┌───────┼───────┐                 └───┘     └───┘
     │       │       │                   ▲  ╲   ╱  ▲
   Client  Client  Client                │   ╲ ╱   │
                                         │    ╳    │
                                         │   ╱ ╲   │
                                         ▼  ╱   ╲  ▼
                                       ┌───┐     ┌───┐
                                       │ P │◄───►│ P │
                                       └───┘     └───┘
```

#### Key P2P Concepts

**1. Peer Discovery**
How nodes find each other:
- **Bootstrap nodes**: Known entry points to the network
- **Peer exchange**: Nodes share their peer lists
- **DHT (Distributed Hash Table)**: Decentralized peer lookup

Our implementation uses bootstrap nodes and peer exchange:
```
1. New node connects to bootstrap peer
2. Bootstrap sends known peer list
3. New node connects to discovered peers
4. Network forms mesh topology
```

**2. Message Routing**
How messages reach their destinations:
- **Direct messaging**: Point-to-point TCP connections
- **Broadcast**: Send to all connected peers
- **Gossip**: Probabilistic message propagation

Our implementation uses direct TCP connections with broadcast for consensus messages.

**3. Connection Management**
```rust
// Each peer connection has:
struct PeerHandle {
    info: PeerInfo,              // Node ID, address
    sender: mpsc::Sender<Msg>,   // Channel to send messages
}

// Peer info tracks:
struct PeerInfo {
    node_id: String,
    address: String,
    connected: bool,
}
```

#### TCP vs UDP for Consensus

We chose TCP because:
- **Reliability**: Guaranteed delivery (important for consensus)
- **Ordering**: Messages arrive in order
- **Connection state**: Know when peers disconnect

UDP would be faster but:
- No delivery guarantees
- Would need to implement reliability ourselves
- Connection tracking more complex

---

## 3. Architecture Deep Dive

### Layer Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                        APPLICATION LAYER                         │
│                         (main.rs, CLI)                          │
│  - Command parsing                                               │
│  - User interaction                                              │
│  - Node lifecycle management                                     │
├─────────────────────────────────────────────────────────────────┤
│                         NODE LAYER                               │
│                          (node.rs)                               │
│  - Combines network + consensus                                  │
│  - Message routing                                               │
│  - Event loop orchestration                                      │
├─────────────────────────────────────────────────────────────────┤
│              CONSENSUS LAYER              │    NETWORK LAYER     │
│                 (pbft.rs)                 │    (network.rs)      │
│  - PBFT state machine                    │  - TCP connections   │
│  - Message processing                    │  - Peer management   │
│  - View changes                          │  - Message I/O       │
│  - Checkpointing                         │  - Discovery         │
├──────────────────────────────────────────┴──────────────────────┤
│                      MESSAGE LAYER                               │
│                       (message.rs)                               │
│  - Message type definitions                                      │
│  - Serialization (bincode)                                       │
│  - Hash computation                                              │
├─────────────────────────────────────────────────────────────────┤
│                     CRYPTOGRAPHY LAYER                           │
│                        (crypto.rs)                               │
│  - Ed25519 key generation                                        │
│  - Signing                                                       │
│  - Verification                                                  │
└─────────────────────────────────────────────────────────────────┘
```

### Component Interaction

```
                    ┌──────────────────────┐
                    │        main.rs       │
                    │   (CLI & Startup)    │
                    └──────────┬───────────┘
                               │
                               ▼
                    ┌──────────────────────┐
                    │       Node           │
                    │  (Orchestrator)      │
                    └──────────┬───────────┘
                               │
            ┌──────────────────┼──────────────────┐
            │                  │                  │
            ▼                  ▼                  ▼
    ┌───────────────┐  ┌───────────────┐  ┌───────────────┐
    │   Network     │  │PbftConsensus  │  │   Channels    │
    │  (P2P Layer)  │  │(State Machine)│  │ (Async Comm)  │
    └───────┬───────┘  └───────┬───────┘  └───────────────┘
            │                  │
            │    ┌─────────────┘
            │    │
            ▼    ▼
    ┌───────────────┐
    │   Messages    │
    │(Serialization)│
    └───────┬───────┘
            │
            ▼
    ┌───────────────┐
    │    Crypto     │
    │  (Ed25519)    │
    └───────────────┘
```

### Async Architecture

The system uses Tokio for async I/O with multiple concurrent tasks:

```
┌─────────────────────────────────────────────────────────────────┐
│                      TOKIO RUNTIME                               │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  ┌─────────────────┐   ┌─────────────────┐                     │
│  │  TCP Listener   │   │ Message Handler │                     │
│  │   (Incoming)    │   │  (Main Loop)    │                     │
│  └────────┬────────┘   └────────┬────────┘                     │
│           │                     │                               │
│           ▼                     ▼                               │
│  ┌─────────────────┐   ┌─────────────────┐                     │
│  │  Per-Peer       │   │  Outgoing Msg   │                     │
│  │  Reader/Writer  │   │  Broadcaster    │                     │
│  └─────────────────┘   └─────────────────┘                     │
│                                                                  │
│  ┌─────────────────┐   ┌─────────────────┐                     │
│  │   CLI Input     │   │ Timeout Checker │                     │
│  │   Handler       │   │  (View Change)  │                     │
│  └─────────────────┘   └─────────────────┘                     │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

---

## 4. Code Walkthrough

### 4.1 Cryptography Module (crypto.rs)

This module provides cryptographic primitives using Ed25519.

#### Why Ed25519?

- **Fast**: ~70,000 signatures/second
- **Small keys**: 32-byte public keys
- **Small signatures**: 64 bytes
- **Secure**: 128-bit security level
- **Deterministic**: Same message + key = same signature

#### Key Structures

```rust
/// Keypair for signing and verification
pub struct KeyPair {
    signing_key: SigningKey,      // Private key (32 bytes)
    verifying_key: VerifyingKey,  // Public key (32 bytes)
}
```

#### Key Generation

```rust
impl KeyPair {
    /// Generate a new random keypair
    pub fn generate() -> Self {
        let mut csprng = OsRng;  // Cryptographically secure RNG
        let signing_key = SigningKey::generate(&mut csprng);
        let verifying_key = signing_key.verifying_key();
        Self { signing_key, verifying_key }
    }

    /// Create keypair from seed (deterministic)
    pub fn from_seed(seed: &[u8; 32]) -> Self {
        let signing_key = SigningKey::from_bytes(seed);
        let verifying_key = signing_key.verifying_key();
        Self { signing_key, verifying_key }
    }
}
```

**Usage of deterministic keys**: When running tests, we use `from_seed` with the node index to generate predictable keys. This ensures the same node always has the same identity.

#### Signing and Verification

```rust
impl KeyPair {
    /// Sign a message
    pub fn sign(&self, message: &[u8]) -> Vec<u8> {
        let signature = self.signing_key.sign(message);
        signature.to_bytes().to_vec()
    }

    /// Verify a signature
    pub fn verify(&self, message: &[u8], signature: &[u8]) -> Result<(), CryptoError> {
        // Convert signature bytes to Signature struct
        let sig_bytes: [u8; 64] = signature
            .try_into()
            .map_err(|_| CryptoError::InvalidKeyFormat)?;
        let signature = ed25519_dalek::Signature::from_bytes(&sig_bytes);

        // Verify using public key
        self.verifying_key
            .verify(message, &signature)
            .map_err(|_| CryptoError::SignatureVerificationFailed)
    }
}
```

#### Node Identity

```rust
/// Get the public key as hex string (used as node ID)
pub fn public_key_hex(&self) -> String {
    hex::encode(self.public_key_bytes())
}
```

The node ID is the hex-encoded public key (64 characters). This provides:
- Unique identification
- Built-in authentication (messages can be verified)
- No central authority for ID assignment

### 4.2 Message Module (message.rs)

This module defines all message types used in the protocol.

#### Type Aliases

```rust
/// Unique identifier for a node in the network
pub type NodeId = String;

/// View number in PBFT (incremented on view change)
pub type ViewNumber = u64;

/// Sequence number for ordering requests
pub type SequenceNumber = u64;

/// Hash of a block or message
pub type Hash = [u8; 32];
```

#### Client Request

```rust
/// A client request/transaction to be processed
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct Request {
    pub client_id: String,    // Who sent this request
    pub timestamp: u64,       // When it was created (ms since epoch)
    pub operation: Vec<u8>,   // The actual data/command
}

impl Request {
    pub fn new(client_id: String, operation: Vec<u8>) -> Self {
        Self {
            client_id,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64,
            operation,
        }
    }

    /// Compute SHA256 digest of the request
    pub fn digest(&self) -> Hash {
        let mut hasher = Sha256::new();
        hasher.update(&self.client_id.as_bytes());
        hasher.update(&self.timestamp.to_le_bytes());
        hasher.update(&self.operation);
        hasher.finalize().into()
    }
}
```

The digest serves as a unique identifier for the request and is used throughout consensus to refer to it efficiently.

#### PBFT Messages

```rust
/// Pre-prepare message from primary
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrePrepare {
    pub view: ViewNumber,
    pub sequence: SequenceNumber,
    pub digest: Hash,
    pub request: Request,      // Full request included
    pub primary_id: NodeId,
    pub signature: Vec<u8>,
}

/// Prepare message from replicas
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Prepare {
    pub view: ViewNumber,
    pub sequence: SequenceNumber,
    pub digest: Hash,          // Only digest, not full request
    pub replica_id: NodeId,
    pub signature: Vec<u8>,
}

/// Commit message from replicas
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Commit {
    pub view: ViewNumber,
    pub sequence: SequenceNumber,
    pub digest: Hash,
    pub replica_id: NodeId,
    pub signature: Vec<u8>,
}
```

**Design choice**: Only PrePrepare contains the full request. Prepare and Commit only contain the digest to reduce bandwidth.

#### Network Messages

```rust
/// Network messages for P2P communication
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NetworkMessage {
    /// Handshake for peer discovery
    Handshake {
        node_id: NodeId,
        listen_port: u16,
    },
    /// Acknowledge handshake
    HandshakeAck {
        node_id: NodeId,
        known_peers: Vec<(NodeId, String)>,  // Peer exchange
    },
    /// Heartbeat/ping
    Ping { node_id: NodeId, timestamp: u64 },
    /// Heartbeat response
    Pong { node_id: NodeId, timestamp: u64 },
    /// PBFT protocol messages
    Consensus(ConsensusMessage),
}

/// PBFT consensus messages
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConsensusMessage {
    Request(Request),
    PrePrepare(PrePrepare),
    Prepare(Prepare),
    Commit(Commit),
    Reply(Reply),
    ViewChange(ViewChange),
    NewView(NewView),
    Checkpoint(Checkpoint),
}
```

#### Serialization

```rust
impl NetworkMessage {
    pub fn serialize(&self) -> Result<Vec<u8>, bincode::Error> {
        bincode::serialize(self)
    }

    pub fn deserialize(data: &[u8]) -> Result<Self, bincode::Error> {
        bincode::deserialize(data)
    }
}
```

We use **bincode** for serialization because:
- Very fast (zero-copy deserialization possible)
- Compact binary format
- Works well with Serde derive macros
- No schema negotiation needed

### 4.3 Network Module (network.rs)

This module implements the P2P networking layer.

#### Core Structures

```rust
/// Information about a connected peer
#[derive(Debug, Clone)]
pub struct PeerInfo {
    pub node_id: NodeId,
    pub address: String,
    pub connected: bool,
}

/// Handle for sending messages to a peer
#[derive(Clone)]
pub struct PeerHandle {
    pub info: PeerInfo,
    sender: mpsc::Sender<NetworkMessage>,  // Async channel
}

/// P2P Network manager
pub struct Network {
    pub node_id: NodeId,
    pub listen_port: u16,
    pub peers: Arc<RwLock<HashMap<NodeId, PeerHandle>>>,
    message_tx: mpsc::Sender<(NodeId, NetworkMessage)>,
    message_rx: Option<mpsc::Receiver<(NodeId, NetworkMessage)>>,
}
```

**Why `Arc<RwLock<...>>`?**
- `Arc`: Shared ownership across async tasks
- `RwLock`: Multiple readers, single writer (from parking_lot for sync access)
- Allows concurrent peer lookups while supporting modifications

#### Starting the Listener

```rust
pub async fn start_listener(&self) -> Result<(), NetworkError> {
    let addr = format!("0.0.0.0:{}", self.listen_port);
    let listener = TcpListener::bind(&addr).await?;
    info!("Listening on {}", addr);

    let peers = self.peers.clone();
    let message_tx = self.message_tx.clone();
    let node_id = self.node_id.clone();
    let listen_port = self.listen_port;

    // Spawn listener task
    tokio::spawn(async move {
        loop {
            match listener.accept().await {
                Ok((stream, addr)) => {
                    info!("Incoming connection from {}", addr);
                    // Spawn handler for this connection
                    tokio::spawn(handle_incoming_connection(
                        stream, addr, peers.clone(),
                        message_tx.clone(), node_id.clone(), listen_port
                    ));
                }
                Err(e) => error!("Failed to accept: {}", e),
            }
        }
    });

    Ok(())
}
```

**Key pattern**: Each connection gets its own task. This allows handling many connections concurrently.

#### Connection Handshake

```rust
async fn handle_incoming_connection(
    stream: TcpStream,
    address: String,
    peers: Arc<RwLock<HashMap<NodeId, PeerHandle>>>,
    message_tx: mpsc::Sender<(NodeId, NetworkMessage)>,
    our_node_id: NodeId,
    our_listen_port: u16,
) -> Result<(), NetworkError> {
    let (read_half, mut write_half) = stream.into_split();
    let mut read_half = tokio::io::BufReader::new(read_half);

    // 1. Receive handshake from connecting peer
    let msg = receive_message(&mut read_half).await?;
    let (peer_id, peer_port) = match msg {
        NetworkMessage::Handshake { node_id, listen_port } => (node_id, listen_port),
        _ => return Err(NetworkError::ConnectionFailed("Expected handshake".into())),
    };

    // 2. Send back our info + known peers (peer exchange)
    let known_peers: Vec<(NodeId, String)> = peers.read()
        .values()
        .map(|p| (p.info.node_id.clone(), p.info.address.clone()))
        .collect();

    let ack = NetworkMessage::HandshakeAck {
        node_id: our_node_id,
        known_peers,
    };
    send_message(&mut write_half, &ack).await?;

    // 3. Create peer handle and store
    let (tx, rx) = mpsc::channel(100);
    let peer_handle = PeerHandle {
        info: PeerInfo {
            node_id: peer_id.clone(),
            address: format!("{}:{}", address.split(':').next().unwrap(), peer_port),
            connected: true,
        },
        sender: tx,
    };
    peers.write().insert(peer_id.clone(), peer_handle);

    // 4. Spawn reader and writer tasks
    tokio::spawn(peer_writer(write_half, rx));
    peer_reader(read_half, peer_id.clone(), message_tx).await?;

    // Cleanup on disconnect
    peers.write().remove(&peer_id);
    Ok(())
}
```

#### Message Framing

TCP is a stream protocol, so we need framing to know where messages begin and end:

```rust
/// Send a message with length prefix
async fn send_message<W: AsyncWriteExt + Unpin>(
    writer: &mut W,
    msg: &NetworkMessage,
) -> Result<(), NetworkError> {
    let data = msg.serialize()?;
    let len = data.len() as u32;

    // Write 4-byte length prefix (little-endian)
    writer.write_all(&len.to_le_bytes()).await?;
    // Write message data
    writer.write_all(&data).await?;
    writer.flush().await?;
    Ok(())
}

/// Receive a message with length prefix
async fn receive_message<R: AsyncReadExt + Unpin>(
    reader: &mut R,
) -> Result<NetworkMessage, NetworkError> {
    // Read 4-byte length prefix
    let mut len_buf = [0u8; 4];
    reader.read_exact(&mut len_buf).await?;
    let len = u32::from_le_bytes(len_buf) as usize;

    // Read message data
    let mut data = vec![0u8; len];
    reader.read_exact(&mut data).await?;

    let msg = NetworkMessage::deserialize(&data)?;
    Ok(msg)
}
```

**Wire format**:
```
┌────────────┬─────────────────────────────┐
│ Length (4B)│      Message (N bytes)      │
│ Little-end │     bincode serialized      │
└────────────┴─────────────────────────────┘
```

#### Broadcasting

```rust
/// Broadcast a message to all peers
pub async fn broadcast(&self, msg: NetworkMessage) {
    let peers: Vec<PeerHandle> = self.peers.read().values().cloned().collect();
    for peer in peers {
        if let Err(e) = peer.send(msg.clone()).await {
            warn!("Failed to send to peer {}: {}", peer.info.node_id, e);
        }
    }
}
```

**Note**: We clone the peer list to avoid holding the lock while sending. This prevents deadlocks and allows concurrent sends.

### 4.4 PBFT Module (pbft.rs)

This is the heart of the consensus algorithm.

#### Consensus Instance

Each request being processed has its own state:

```rust
/// State for a single consensus instance
#[derive(Debug, Clone)]
pub struct ConsensusInstance {
    pub sequence: SequenceNumber,
    pub view: ViewNumber,
    pub request: Option<Request>,
    pub digest: Option<Hash>,
    pub phase: Phase,
    pub pre_prepare: Option<PrePrepare>,
    pub prepares: HashMap<NodeId, Prepare>,
    pub commits: HashMap<NodeId, Commit>,
    pub started_at: Instant,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Phase {
    Idle,
    PrePrepared,
    Prepared,
    Committed,
}
```

#### PBFT State

```rust
pub struct PbftState {
    pub node_id: NodeId,
    pub keypair: KeyPair,
    pub view: ViewNumber,
    pub sequence: SequenceNumber,        // Next sequence to assign (primary)
    pub low_watermark: SequenceNumber,   // Last stable checkpoint
    pub high_watermark: SequenceNumber,  // low + 2*checkpoint_interval
    pub config: PbftConfig,
    pub replicas: Vec<NodeId>,           // Ordered list of all replicas
    pub log: HashMap<SequenceNumber, ConsensusInstance>,
    pub executed: HashSet<Hash>,         // Already executed requests
    pub checkpoints: HashMap<SequenceNumber, HashMap<NodeId, Checkpoint>>,
    pub stable_checkpoint: SequenceNumber,
    pub view_changes: HashMap<ViewNumber, HashMap<NodeId, ViewChange>>,
    pub view_changing: bool,
    pub last_primary_activity: Instant,
}
```

#### Configuration

```rust
#[derive(Debug, Clone)]
pub struct PbftConfig {
    pub n: usize,                    // Total nodes
    pub f: usize,                    // Max faulty (n-1)/3
    pub view_change_timeout: Duration,
    pub checkpoint_interval: u64,
}

impl PbftConfig {
    pub fn new(n: usize) -> Self {
        let f = (n - 1) / 3;
        Self {
            n,
            f,
            view_change_timeout: Duration::from_secs(10),
            checkpoint_interval: 100,
        }
    }

    /// Quorum size (2f + 1)
    pub fn quorum(&self) -> usize {
        2 * self.f + 1
    }
}
```

#### Primary Selection

```rust
/// Get the primary for the current view
pub fn primary(&self) -> &NodeId {
    let primary_idx = (self.view as usize) % self.replicas.len();
    &self.replicas[primary_idx]
}

/// Check if we are the primary
pub fn is_primary(&self) -> bool {
    self.primary() == &self.node_id
}
```

The primary rotates through replicas as views change. This ensures liveness even if one node is slow.

#### Handling Requests (Primary)

```rust
pub async fn handle_request(&self, request: Request) -> Result<(), PbftError> {
    let mut state = self.state.write();

    // If not primary, forward to primary
    if !state.is_primary() {
        let msg = NetworkMessage::Consensus(ConsensusMessage::Request(request));
        let _ = self.outgoing_tx.send(msg).await;
        return Ok(());
    }

    // Check if already executed (deduplication)
    let digest = request.digest();
    if state.executed.contains(&digest) {
        return Ok(());
    }

    // Assign sequence number
    state.sequence += 1;
    let sequence = state.sequence;
    let view = state.view;

    // Check water marks (prevent log overflow)
    if sequence > state.high_watermark {
        warn!("Sequence {} exceeds high watermark", sequence);
        return Err(PbftError::InvalidSequence);
    }

    // Create signed pre-prepare
    let sign_data = format!("{}-{}-{:?}", view, sequence, digest);
    let signature = state.sign(sign_data.as_bytes());

    let pre_prepare = PrePrepare {
        view, sequence, digest,
        request: request.clone(),
        primary_id: state.node_id.clone(),
        signature,
    };

    // Update local state
    let instance = state.get_or_create_instance(sequence);
    instance.request = Some(request);
    instance.digest = Some(digest);
    instance.pre_prepare = Some(pre_prepare.clone());
    instance.phase = Phase::PrePrepared;

    // Broadcast to all replicas
    drop(state);  // Release lock before async operation
    let msg = NetworkMessage::Consensus(ConsensusMessage::PrePrepare(pre_prepare));
    let _ = self.outgoing_tx.send(msg).await;

    Ok(())
}
```

**Important patterns**:
1. Check if we're the primary first
2. Deduplicate using `executed` set
3. Drop the lock before async operations to avoid blocking
4. Sign messages for authentication

#### Handling Pre-Prepare (Backups)

```rust
pub async fn handle_pre_prepare(&self, pre_prepare: PrePrepare) -> Result<(), PbftError> {
    let prepare = {
        let mut state = self.state.write();

        // Validation checks
        if pre_prepare.view != state.view {
            return Err(PbftError::InvalidView);
        }
        if &pre_prepare.primary_id != state.primary() {
            return Err(PbftError::NotPrimary);
        }
        if pre_prepare.sequence <= state.low_watermark
            || pre_prepare.sequence > state.high_watermark {
            return Err(PbftError::InvalidSequence);
        }

        // Check for conflicting pre-prepare
        if let Some(instance) = state.log.get(&pre_prepare.sequence) {
            if instance.pre_prepare.is_some() {
                if instance.digest != Some(pre_prepare.digest) {
                    return Err(PbftError::InvalidMessage);  // Conflicting!
                }
                return Ok(());  // Already have this
            }
        }

        // Verify request digest
        let computed_digest = pre_prepare.request.digest();
        if computed_digest != pre_prepare.digest {
            return Err(PbftError::InvalidMessage);
        }

        // Extract values needed for prepare message
        let sequence = pre_prepare.sequence;
        let view = pre_prepare.view;
        let digest = pre_prepare.digest;
        let node_id = state.node_id.clone();

        // Create prepare message
        let sign_data = format!("{}-{}-{:?}", view, sequence, digest);
        let signature = state.sign(sign_data.as_bytes());

        let prepare = Prepare {
            view, sequence, digest,
            replica_id: node_id.clone(),
            signature,
        };

        // Update instance state
        let instance = state.get_or_create_instance(sequence);
        instance.request = Some(pre_prepare.request.clone());
        instance.digest = Some(digest);
        instance.pre_prepare = Some(pre_prepare);
        instance.phase = Phase::PrePrepared;
        instance.prepares.insert(node_id, prepare.clone());

        state.last_primary_activity = Instant::now();

        prepare
    };  // Lock released here

    // Broadcast prepare
    let msg = NetworkMessage::Consensus(ConsensusMessage::Prepare(prepare.clone()));
    let _ = self.outgoing_tx.send(msg).await;

    // Check if we can advance
    self.try_advance_to_prepared(prepare.sequence).await?;

    Ok(())
}
```

#### Advancing to Prepared State

```rust
async fn try_advance_to_prepared(&self, sequence: SequenceNumber) -> Result<(), PbftError> {
    let commit = {
        let mut state = self.state.write();
        let quorum = state.config.quorum();
        let node_id = state.node_id.clone();

        let instance = match state.log.get(&sequence) {
            Some(i) => i,
            None => return Ok(()),
        };

        // Check conditions for prepared state
        if instance.pre_prepare.is_none() { return Ok(()); }
        if instance.phase != Phase::PrePrepared { return Ok(()); }
        if instance.prepares.len() < quorum { return Ok(()); }

        info!("Prepared for sequence {}", sequence);

        // Extract needed values
        let view = instance.view;
        let digest = instance.digest.unwrap();

        // Create commit message
        let sign_data = format!("{}-{}-{:?}-commit", view, sequence, digest);
        let signature = state.sign(sign_data.as_bytes());

        let commit = Commit {
            view, sequence, digest,
            replica_id: node_id.clone(),
            signature,
        };

        // Update state
        let instance = state.log.get_mut(&sequence).unwrap();
        instance.phase = Phase::Prepared;
        instance.commits.insert(node_id, commit.clone());

        commit
    };

    // Broadcast commit
    let msg = NetworkMessage::Consensus(ConsensusMessage::Commit(commit));
    let _ = self.outgoing_tx.send(msg).await;

    self.try_advance_to_committed(sequence).await?;

    Ok(())
}
```

**Key insight**: The prepared state guarantees that any other correct replica that prepares will prepare with the same request. This is crucial for safety.

#### Advancing to Committed State

```rust
async fn try_advance_to_committed(&self, sequence: SequenceNumber) -> Result<(), PbftError> {
    let request_to_commit = {
        let mut state = self.state.write();
        let quorum = state.config.quorum();

        let instance = match state.log.get(&sequence) {
            Some(i) => i,
            None => return Ok(()),
        };

        // Check conditions
        if instance.phase != Phase::Prepared { return Ok(()); }
        if instance.commits.len() < quorum { return Ok(()); }

        info!("Committed for sequence {}", sequence);

        // Determine if we should execute
        let request_to_commit = if let Some(ref request) = instance.request {
            let digest = request.digest();
            if !state.executed.contains(&digest) {
                Some((request.clone(), digest))
            } else {
                None
            }
        } else {
            None
        };

        // Update phase
        let instance = state.log.get_mut(&sequence).unwrap();
        instance.phase = Phase::Committed;

        // Mark as executed
        if let Some((_, digest)) = &request_to_commit {
            state.executed.insert(*digest);
        }

        request_to_commit
    };

    // Execute request (outside lock)
    if let Some((request, _)) = request_to_commit {
        let _ = self.committed_tx.send(request).await;
        self.try_checkpoint(sequence).await?;
    }

    Ok(())
}
```

#### View Change

```rust
pub async fn start_view_change(&self) {
    let view_change = {
        let mut state = self.state.write();

        if state.view_changing {
            return;  // Already in progress
        }

        let new_view = state.view + 1;
        info!("Starting view change to view {}", new_view);
        state.view_changing = true;

        // Create view change message
        let sign_data = format!("{}-view-change", new_view);
        let signature = state.sign(sign_data.as_bytes());
        let node_id = state.node_id.clone();

        let view_change = ViewChange {
            new_view,
            replica_id: node_id.clone(),
            last_sequence: state.sequence,
            signature,
        };

        // Add our own view change
        state.view_changes
            .entry(new_view)
            .or_insert_with(HashMap::new)
            .insert(node_id, view_change.clone());

        view_change
    };

    let msg = NetworkMessage::Consensus(ConsensusMessage::ViewChange(view_change));
    let _ = self.outgoing_tx.send(msg).await;
}

pub async fn handle_view_change(&self, view_change: ViewChange) -> Result<(), PbftError> {
    let new_view_msg = {
        let mut state = self.state.write();

        if view_change.new_view <= state.view {
            return Ok(());  // Old view change, ignore
        }

        let new_view = view_change.new_view;

        // Store view change
        state.view_changes
            .entry(new_view)
            .or_insert_with(HashMap::new)
            .insert(view_change.replica_id.clone(), view_change);

        // Check if we have enough
        let quorum = state.config.quorum();
        let count = state.view_changes.get(&new_view).map(|e| e.len()).unwrap_or(0);

        if count >= quorum {
            let replicas_len = state.replicas.len();
            let primary_idx = (new_view as usize) % replicas_len;
            let is_new_primary = state.replicas[primary_idx] == state.node_id;

            if is_new_primary {
                info!("We are new primary for view {}", new_view);

                // Gather view changes for proof
                let view_changes: Vec<ViewChange> = state.view_changes
                    .get(&new_view)
                    .map(|e| e.values().cloned().collect())
                    .unwrap_or_default();

                let sign_data = format!("{}-new-view", new_view);
                let signature = state.sign(sign_data.as_bytes());

                Some(NewView {
                    new_view,
                    view_changes,
                    primary_id: state.node_id.clone(),
                    signature,
                })
            } else {
                None
            }
        } else {
            None
        }
    };

    if let Some(new_view_msg) = new_view_msg {
        let msg = NetworkMessage::Consensus(ConsensusMessage::NewView(new_view_msg));
        let _ = self.outgoing_tx.send(msg).await;
    }

    Ok(())
}
```

### 4.5 Node Module (node.rs)

This module provides a high-level abstraction combining network and consensus.

```rust
pub struct Node {
    pub node_id: NodeId,
    network: Network,
    consensus: Option<PbftConsensus>,
    outgoing_rx: mpsc::Receiver<NetworkMessage>,
    committed_rx: mpsc::Receiver<Request>,
}

impl Node {
    pub fn new(listen_port: u16, seed: Option<[u8; 32]>) -> Self {
        // Generate keypair (random or deterministic)
        let keypair = match seed {
            Some(s) => KeyPair::from_seed(&s),
            None => KeyPair::generate(),
        };

        let node_id = keypair.public_key_hex();
        let network = Network::new(node_id.clone(), listen_port);

        // Create channels for message flow
        let (outgoing_tx, outgoing_rx) = mpsc::channel(1000);
        let (committed_tx, committed_rx) = mpsc::channel(1000);

        // Initialize consensus
        let config = PbftConfig::new(1);
        let state = PbftState::new(node_id.clone(), keypair, vec![node_id.clone()], config);
        let consensus = PbftConsensus::new(state, outgoing_tx, committed_tx);

        Self {
            node_id, network,
            consensus: Some(consensus),
            outgoing_rx, committed_rx,
        }
    }
}
```

#### NodeBuilder Pattern

```rust
pub struct NodeBuilder {
    listen_port: u16,
    seed: Option<[u8; 32]>,
    bootstrap_peers: Vec<String>,
}

impl NodeBuilder {
    pub fn new(listen_port: u16) -> Self {
        Self {
            listen_port,
            seed: None,
            bootstrap_peers: Vec::new(),
        }
    }

    pub fn with_seed(mut self, seed: [u8; 32]) -> Self {
        self.seed = Some(seed);
        self
    }

    pub fn with_bootstrap_peer(mut self, address: String) -> Self {
        self.bootstrap_peers.push(address);
        self
    }

    pub fn build(self) -> Node {
        Node::new(self.listen_port, self.seed)
    }
}
```

This provides a clean API for node configuration.

### 4.6 Main Module (main.rs)

The entry point handling CLI and orchestration.

#### CLI Definition

```rust
#[derive(Parser)]
#[command(name = "p2p-pbft")]
#[command(about = "P2P Network with PBFT Consensus")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Start {
        #[arg(short, long, default_value = "8000")]
        port: u16,
        #[arg(short, long)]
        seed: Option<String>,
        #[arg(short, long)]
        bootstrap: Vec<String>,
        #[arg(short, long, default_value = "4")]
        nodes: usize,
        #[arg(short, long)]
        index: Option<usize>,
    },
    Keygen {
        #[arg(short, long)]
        seed: Option<String>,
    },
}
```

#### Main Event Loop

```rust
async fn start_node(...) -> Result<(), Box<dyn std::error::Error>> {
    // Setup...

    // Main message processing loop
    while let Some((peer_id, msg)) = network_rx.recv().await {
        match msg {
            NetworkMessage::Consensus(consensus_msg) => {
                if let Err(e) = consensus.process_message(consensus_msg).await {
                    warn!("Error processing message: {}", e);
                }
            }
            NetworkMessage::Ping { timestamp, .. } => {
                // Respond with pong
                let pong = NetworkMessage::Pong {
                    node_id: node_id.clone(),
                    timestamp,
                };
                network.send_to_peer(&peer_id, pong).await?;
            }
            _ => {}
        }
    }

    Ok(())
}
```

---

## 5. Data Flow

### Request Lifecycle

```
┌────────────────────────────────────────────────────────────────┐
│                        REQUEST LIFECYCLE                        │
└────────────────────────────────────────────────────────────────┘

1. CLIENT SUBMISSION
   User → CLI → Request::new() → handle_request()

2. PRIMARY PROCESSING
   handle_request() → assign sequence → create PrePrepare → broadcast

3. BACKUP PROCESSING
   receive PrePrepare → validate → create Prepare → broadcast

4. PREPARE COLLECTION
   collect Prepares → check quorum (2f+1) → advance to Prepared

5. COMMIT BROADCAST
   create Commit → broadcast

6. COMMIT COLLECTION
   collect Commits → check quorum (2f+1) → advance to Committed

7. EXECUTION
   execute request → send to application → create checkpoint if needed

```

### Message Flow Example (4 nodes)

```
Time    Node0(Primary)   Node1           Node2           Node3
────────────────────────────────────────────────────────────────
  │
  │     Request arrives
  │         │
  │     PrePrepare ────────────────────────────────────────────►
  │         │              │               │               │
  │     Prepare      Prepare         Prepare         Prepare
  │         │◄─────────────│               │               │
  │         │◄─────────────────────────────│               │
  │         │◄─────────────────────────────────────────────│
  │         │──────────────►               │               │
  │         │──────────────────────────────►               │
  │         │──────────────────────────────────────────────►
  │                   (All have 4 prepares - quorum met)
  │
  │     Commit       Commit          Commit          Commit
  │         │◄─────────────│               │               │
  │         │◄─────────────────────────────│               │
  │         │◄─────────────────────────────────────────────│
  │         │──────────────►               │               │
  │         │──────────────────────────────►               │
  │         │──────────────────────────────────────────────►
  │                   (All have 4 commits - quorum met)
  │
  │     Execute      Execute         Execute         Execute
  │
  ▼
```

---

## 6. Security Considerations

### Message Authentication

Every PBFT message is signed:

```rust
// Signing
let sign_data = format!("{}-{}-{:?}", view, sequence, digest);
let signature = state.sign(sign_data.as_bytes());

// The signature binds:
// - View number (prevents replay in different views)
// - Sequence number (prevents reordering)
// - Digest (ensures message integrity)
```

### Replay Attack Prevention

1. **View numbers**: Messages from old views are rejected
2. **Sequence numbers**: Only processes sequences in valid range
3. **Executed set**: Prevents re-execution of same request

### Byzantine Fault Handling

| Attack | Defense |
|--------|---------|
| Forged messages | Ed25519 signatures |
| Message replay | View/sequence validation |
| Primary equivocation | Digest checking in prepares |
| Primary silence | View change timeout |
| Network partition | Quorum requirement |

### Potential Vulnerabilities

1. **No signature verification**: Current implementation signs but doesn't verify (simplified for clarity)
2. **No TLS**: Communications are unencrypted
3. **DoS**: No rate limiting on incoming connections

---

## 7. Performance Optimization

### Current Optimizations

1. **Async I/O**: Non-blocking operations via Tokio
2. **Lock minimization**: Drop locks before async operations
3. **Efficient serialization**: Bincode for fast binary encoding
4. **Channel-based communication**: Decouples components

### Potential Improvements

```rust
// 1. Batching requests
struct Batch {
    requests: Vec<Request>,
    digest: Hash,
}

// 2. Parallel signature verification
async fn verify_batch(prepares: Vec<Prepare>) {
    futures::future::join_all(
        prepares.iter().map(|p| verify_signature(p))
    ).await;
}

// 3. Connection pooling
struct ConnectionPool {
    connections: HashMap<NodeId, Vec<TcpStream>>,
}
```

---

## 8. Comparison with libp2p

### What is libp2p?

libp2p is a modular networking stack used by many blockchain projects (IPFS, Ethereum 2.0, Polkadot).

### Feature Comparison

| Feature | Our Implementation | libp2p |
|---------|-------------------|--------|
| Transport | TCP only | TCP, QUIC, WebSocket, etc. |
| Discovery | Bootstrap + peer exchange | mDNS, Kademlia DHT, etc. |
| Multiplexing | One connection per peer | mplex, yamux |
| Security | Ed25519 signatures | Noise, TLS 1.3 |
| Pub/Sub | Broadcast to all | GossipSub, FloodSub |
| NAT Traversal | None | AutoNAT, Circuit Relay |
| Complexity | ~1500 LOC | Massive ecosystem |

### When to Use Each

**Use our implementation when**:
- Learning distributed systems concepts
- Building a simple permissioned network
- Wanting full control over the protocol
- Minimizing dependencies

**Use libp2p when**:
- Building production systems
- Need NAT traversal
- Want interoperability with other libp2p systems
- Need advanced features (DHT, pub/sub)

### libp2p Equivalent Code

```rust
// Our approach
let network = Network::new(node_id, port);
network.connect_to_peer("127.0.0.1:8001").await?;

// libp2p equivalent
let transport = tcp::TcpConfig::new();
let mut swarm = SwarmBuilder::with_existing_identity(keypair)
    .with_tokio()
    .with_tcp(tcp::Config::default(), noise::Config::new, yamux::Config::default)?
    .with_behaviour(|_| MyBehaviour::default())?
    .build();
swarm.dial("/ip4/127.0.0.1/tcp/8001".parse()?)?;
```

---

## 9. Future Improvements

### Short Term

1. **Signature verification**: Verify all incoming signatures
2. **TLS encryption**: Encrypt peer connections
3. **Request batching**: Process multiple requests per consensus round
4. **Persistent storage**: Save state to disk

### Medium Term

1. **State synchronization**: Allow nodes to catch up after being offline
2. **Dynamic membership**: Add/remove nodes without restart
3. **Client replies**: Send execution results back to clients
4. **Metrics and monitoring**: Prometheus/Grafana integration

### Long Term

1. **BFT-SMaRt integration**: More optimized consensus
2. **Sharding**: Parallel consensus for scalability
3. **libp2p migration**: For production deployment
4. **Formal verification**: Prove correctness properties

---

## Glossary

| Term | Definition |
|------|------------|
| **Byzantine fault** | A failure where a node behaves arbitrarily (possibly maliciously) |
| **Consensus** | Agreement among distributed nodes on a single value |
| **Digest** | Cryptographic hash of a message |
| **Primary** | The leader node that orders requests |
| **Quorum** | Minimum number of nodes needed for agreement (2f+1) |
| **Replica** | A node participating in consensus |
| **Sequence number** | Order assigned to a request |
| **View** | A configuration with a specific primary |
| **View change** | Process of electing a new primary |
| **Water marks** | Bounds on acceptable sequence numbers |

---

## References

1. Castro, M., & Liskov, B. (1999). *Practical Byzantine Fault Tolerance*. OSDI.
2. Lamport, L., Shostak, R., & Pease, M. (1982). *The Byzantine Generals Problem*. ACM TOPLAS.
3. [Tokio Documentation](https://tokio.rs/)
4. [Ed25519 Paper](https://ed25519.cr.yp.to/ed25519-20110926.pdf)
5. [libp2p Specification](https://github.com/libp2p/specs)
