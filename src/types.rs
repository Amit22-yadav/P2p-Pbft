use crate::crypto::KeyPair;
use crate::message::{compute_hash, Hash, NodeId, SequenceNumber, ViewNumber};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::time::{SystemTime, UNIX_EPOCH};

/// 20-byte address derived from public key hash
pub type Address = [u8; 20];

/// Convert public key to address (first 20 bytes of SHA256 hash)
pub fn public_key_to_address(public_key: &[u8; 32]) -> Address {
    let hash = compute_hash(public_key);
    let mut addr = [0u8; 20];
    addr.copy_from_slice(&hash[..20]);
    addr
}

/// Account state (Ethereum-style)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Account {
    pub address: Address,
    pub balance: u64,
    pub nonce: u64,
}

impl Account {
    pub fn new(address: Address) -> Self {
        Self {
            address,
            balance: 0,
            nonce: 0,
        }
    }

    pub fn with_balance(address: Address, balance: u64) -> Self {
        Self {
            address,
            balance,
            nonce: 0,
        }
    }
}

/// Transaction payload types
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TransactionPayload {
    /// Transfer value from sender to recipient
    Transfer { to: Address, value: u64 },
    /// Set a key-value pair in sender's storage
    SetState { key: Vec<u8>, value: Vec<u8> },
    /// Delete a key from sender's storage
    DeleteState { key: Vec<u8> },
}

/// A blockchain transaction
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Transaction {
    /// Transaction hash (computed from content)
    pub hash: Hash,
    /// Sender address
    pub from: Address,
    /// Transaction nonce (for replay protection)
    pub nonce: u64,
    /// Transaction payload
    pub payload: TransactionPayload,
    /// Signature over transaction content
    pub signature: Vec<u8>,
    /// Timestamp when transaction was created
    pub timestamp: u64,
}

impl Transaction {
    /// Create a new unsigned transaction
    pub fn new(from: Address, nonce: u64, payload: TransactionPayload) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        let mut tx = Self {
            hash: [0u8; 32],
            from,
            nonce,
            payload,
            signature: Vec::new(),
            timestamp,
        };
        tx.hash = tx.compute_hash();
        tx
    }

    /// Compute the hash of the transaction (excluding signature)
    pub fn compute_hash(&self) -> Hash {
        let mut hasher = Sha256::new();
        hasher.update(&self.from);
        hasher.update(&self.nonce.to_le_bytes());
        hasher.update(&self.timestamp.to_le_bytes());
        hasher.update(&bincode::serialize(&self.payload).unwrap_or_default());
        hasher.finalize().into()
    }

    /// Get the signing message (hash of transaction content)
    pub fn signing_message(&self) -> Hash {
        self.compute_hash()
    }

    /// Sign the transaction
    pub fn sign(&mut self, keypair: &KeyPair) {
        let message = self.signing_message();
        self.signature = keypair.sign(&message);
    }

    /// Create and sign a transfer transaction
    pub fn transfer(keypair: &KeyPair, to: Address, value: u64, nonce: u64) -> Self {
        let from = public_key_to_address(&keypair.public_key_bytes());
        let mut tx = Self::new(from, nonce, TransactionPayload::Transfer { to, value });
        tx.sign(keypair);
        tx
    }

    /// Create and sign a set state transaction
    pub fn set_state(keypair: &KeyPair, key: Vec<u8>, value: Vec<u8>, nonce: u64) -> Self {
        let from = public_key_to_address(&keypair.public_key_bytes());
        let mut tx = Self::new(from, nonce, TransactionPayload::SetState { key, value });
        tx.sign(keypair);
        tx
    }

    /// Create and sign a delete state transaction
    pub fn delete_state(keypair: &KeyPair, key: Vec<u8>, nonce: u64) -> Self {
        let from = public_key_to_address(&keypair.public_key_bytes());
        let mut tx = Self::new(from, nonce, TransactionPayload::DeleteState { key });
        tx.sign(keypair);
        tx
    }

    /// Get the value being transferred (if transfer transaction)
    pub fn value(&self) -> u64 {
        match &self.payload {
            TransactionPayload::Transfer { value, .. } => *value,
            _ => 0,
        }
    }

    /// Get the recipient address (if transfer transaction)
    pub fn to(&self) -> Option<Address> {
        match &self.payload {
            TransactionPayload::Transfer { to, .. } => Some(*to),
            _ => None,
        }
    }
}

/// Block header
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BlockHeader {
    /// Block version
    pub version: u32,
    /// Block height (0 for genesis)
    pub height: u64,
    /// Block timestamp
    pub timestamp: u64,
    /// Hash of previous block
    pub prev_hash: Hash,
    /// Merkle root of transactions
    pub tx_root: Hash,
    /// State root after executing this block
    pub state_root: Hash,
    /// Proposer node ID
    pub proposer: NodeId,
    /// PBFT view number
    pub view: ViewNumber,
    /// PBFT sequence number
    pub sequence: SequenceNumber,
}

impl BlockHeader {
    /// Compute the hash of the block header
    pub fn hash(&self) -> Hash {
        let mut hasher = Sha256::new();
        hasher.update(&self.version.to_le_bytes());
        hasher.update(&self.height.to_le_bytes());
        hasher.update(&self.timestamp.to_le_bytes());
        hasher.update(&self.prev_hash);
        hasher.update(&self.tx_root);
        hasher.update(&self.state_root);
        hasher.update(self.proposer.as_bytes());
        hasher.update(&self.view.to_le_bytes());
        hasher.update(&self.sequence.to_le_bytes());
        hasher.finalize().into()
    }
}

/// A complete block
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Block {
    /// Block header
    pub header: BlockHeader,
    /// Transactions in this block
    pub transactions: Vec<Transaction>,
    /// Commit signatures from replicas (collected during PBFT commit phase)
    pub commit_signatures: Vec<BlockSignature>,
}

impl Block {
    /// Create a new block
    pub fn new(
        height: u64,
        prev_hash: Hash,
        transactions: Vec<Transaction>,
        tx_root: Hash,
        state_root: Hash,
        proposer: NodeId,
        view: ViewNumber,
        sequence: SequenceNumber,
    ) -> Self {
        let header = BlockHeader {
            version: 1,
            height,
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64,
            prev_hash,
            tx_root,
            state_root,
            proposer,
            view,
            sequence,
        };

        Self {
            header,
            transactions,
            commit_signatures: Vec::new(),
        }
    }

    /// Get block hash
    pub fn hash(&self) -> Hash {
        self.header.hash()
    }

    /// Get block height
    pub fn height(&self) -> u64 {
        self.header.height
    }

    /// Check if this is the genesis block
    pub fn is_genesis(&self) -> bool {
        self.header.height == 0
    }

    /// Add a commit signature
    pub fn add_signature(&mut self, sig: BlockSignature) {
        if !self
            .commit_signatures
            .iter()
            .any(|s| s.replica_id == sig.replica_id)
        {
            self.commit_signatures.push(sig);
        }
    }

    /// Get number of signatures
    pub fn signature_count(&self) -> usize {
        self.commit_signatures.len()
    }
}

/// Block signature from a replica
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BlockSignature {
    pub replica_id: NodeId,
    pub signature: Vec<u8>,
}

/// Transaction receipt after execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionReceipt {
    /// Transaction hash
    pub tx_hash: Hash,
    /// Whether execution succeeded
    pub success: bool,
    /// Block height where transaction was included
    pub block_height: u64,
    /// Transaction index within the block
    pub tx_index: usize,
    /// Error message if failed
    pub error: Option<String>,
}

/// Genesis configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenesisConfig {
    /// Chain identifier
    pub chain_id: String,
    /// Genesis timestamp
    pub timestamp: u64,
    /// Initial validators (node IDs)
    pub validators: Vec<NodeId>,
    /// Initial account balances
    pub accounts: Vec<(Address, u64)>,
}

impl GenesisConfig {
    pub fn new(chain_id: String, validators: Vec<NodeId>) -> Self {
        Self {
            chain_id,
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64,
            validators,
            accounts: Vec::new(),
        }
    }

    pub fn with_accounts(mut self, accounts: Vec<(Address, u64)>) -> Self {
        self.accounts = accounts;
        self
    }

    /// Create genesis block from config
    pub fn create_genesis_block(&self, state_root: Hash) -> Block {
        let header = BlockHeader {
            version: 1,
            height: 0,
            timestamp: self.timestamp,
            prev_hash: [0u8; 32],
            tx_root: [0u8; 32], // Empty merkle root for genesis
            state_root,
            proposer: "genesis".to_string(),
            view: 0,
            sequence: 0,
        };

        Block {
            header,
            transactions: Vec::new(),
            commit_signatures: Vec::new(),
        }
    }
}

/// Utility to format address as hex string
pub fn address_to_hex(addr: &Address) -> String {
    format!("0x{}", hex::encode(addr))
}

/// Utility to parse address from hex string
pub fn address_from_hex(s: &str) -> Result<Address, hex::FromHexError> {
    let s = s.strip_prefix("0x").unwrap_or(s);
    let bytes = hex::decode(s)?;
    if bytes.len() != 20 {
        return Err(hex::FromHexError::InvalidStringLength);
    }
    let mut addr = [0u8; 20];
    addr.copy_from_slice(&bytes);
    Ok(addr)
}

/// Utility to format hash as hex string
pub fn hash_to_hex(hash: &Hash) -> String {
    format!("0x{}", hex::encode(hash))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_account_creation() {
        let addr = [1u8; 20];
        let account = Account::with_balance(addr, 1000);
        assert_eq!(account.balance, 1000);
        assert_eq!(account.nonce, 0);
    }

    #[test]
    fn test_transaction_creation() {
        let keypair = KeyPair::generate();
        let to = [2u8; 20];
        let tx = Transaction::transfer(&keypair, to, 100, 0);

        assert_eq!(tx.nonce, 0);
        assert_eq!(tx.value(), 100);
        assert_eq!(tx.to(), Some(to));
        assert!(!tx.signature.is_empty());
    }

    #[test]
    fn test_block_creation() {
        let block = Block::new(
            1,
            [0u8; 32],
            vec![],
            [0u8; 32],
            [0u8; 32],
            "node1".to_string(),
            0,
            1,
        );

        assert_eq!(block.height(), 1);
        assert!(!block.is_genesis());
        assert_eq!(block.signature_count(), 0);
    }

    #[test]
    fn test_address_conversion() {
        let addr = [0xab; 20];
        let hex = address_to_hex(&addr);
        assert!(hex.starts_with("0x"));

        let parsed = address_from_hex(&hex).unwrap();
        assert_eq!(parsed, addr);
    }
}
