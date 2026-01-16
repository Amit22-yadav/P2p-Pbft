use crate::types::{Block, Transaction};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Unique identifier for a node in the network
pub type NodeId = String;

/// View number in PBFT (incremented on view change)
pub type ViewNumber = u64;

/// Sequence number for ordering requests
pub type SequenceNumber = u64;

/// Hash of a block or message
pub type Hash = [u8; 32];

/// A client request/transaction to be processed
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct Request {
    pub client_id: String,
    pub timestamp: u64,
    pub operation: Vec<u8>,
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

    pub fn digest(&self) -> Hash {
        let mut hasher = Sha256::new();
        hasher.update(&self.client_id.as_bytes());
        hasher.update(&self.timestamp.to_le_bytes());
        hasher.update(&self.operation);
        hasher.finalize().into()
    }
}

/// Pre-prepare message from primary
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrePrepare {
    pub view: ViewNumber,
    pub sequence: SequenceNumber,
    pub digest: Hash,
    pub request: Request,
    pub primary_id: NodeId,
    pub signature: Vec<u8>,
}

/// Prepare message from replicas
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Prepare {
    pub view: ViewNumber,
    pub sequence: SequenceNumber,
    pub digest: Hash,
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

/// Reply to client
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reply {
    pub view: ViewNumber,
    pub timestamp: u64,
    pub client_id: String,
    pub replica_id: NodeId,
    pub result: Vec<u8>,
    pub signature: Vec<u8>,
}

/// View change message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViewChange {
    pub new_view: ViewNumber,
    pub replica_id: NodeId,
    pub last_sequence: SequenceNumber,
    pub signature: Vec<u8>,
}

/// New view message from new primary
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewView {
    pub new_view: ViewNumber,
    pub view_changes: Vec<ViewChange>,
    pub primary_id: NodeId,
    pub signature: Vec<u8>,
}

/// Checkpoint message for garbage collection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    pub sequence: SequenceNumber,
    pub digest: Hash,
    pub replica_id: NodeId,
    pub signature: Vec<u8>,
}

// ==================== Block-Based Consensus Messages ====================

/// Block pre-prepare message from primary (for block-based consensus)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockPrePrepare {
    pub view: ViewNumber,
    pub sequence: SequenceNumber,
    pub block_hash: Hash,
    pub block: Block,
    pub primary_id: NodeId,
    pub signature: Vec<u8>,
}

/// Block prepare message from replicas
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockPrepare {
    pub view: ViewNumber,
    pub sequence: SequenceNumber,
    pub block_hash: Hash,
    pub replica_id: NodeId,
    pub signature: Vec<u8>,
}

/// Block commit message from replicas
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockCommit {
    pub view: ViewNumber,
    pub sequence: SequenceNumber,
    pub block_hash: Hash,
    pub replica_id: NodeId,
    pub signature: Vec<u8>,
}

/// Block committed notification (sent after 2f+1 commits)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockCommitted {
    pub block_hash: Hash,
    pub height: u64,
    pub signatures: Vec<(NodeId, Vec<u8>)>,
}

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
        known_peers: Vec<(NodeId, String)>,
    },
    /// Heartbeat/ping
    Ping {
        node_id: NodeId,
        timestamp: u64,
    },
    /// Heartbeat response
    Pong {
        node_id: NodeId,
        timestamp: u64,
    },
    /// PBFT protocol messages
    Consensus(ConsensusMessage),
}

/// PBFT consensus messages
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConsensusMessage {
    // Legacy request-based messages (kept for compatibility)
    Request(Request),
    PrePrepare(PrePrepare),
    Prepare(Prepare),
    Commit(Commit),
    Reply(Reply),
    ViewChange(ViewChange),
    NewView(NewView),
    Checkpoint(Checkpoint),

    // Block-based consensus messages
    BlockPrePrepare(BlockPrePrepare),
    BlockPrepare(BlockPrepare),
    BlockCommit(BlockCommit),
    BlockCommitted(BlockCommitted),

    // Transaction propagation
    TransactionBroadcast(Transaction),
}

impl NetworkMessage {
    pub fn serialize(&self) -> Result<Vec<u8>, bincode::Error> {
        bincode::serialize(self)
    }

    pub fn deserialize(data: &[u8]) -> Result<Self, bincode::Error> {
        bincode::deserialize(data)
    }
}

/// Compute SHA256 hash
pub fn compute_hash(data: &[u8]) -> Hash {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher.finalize().into()
}
