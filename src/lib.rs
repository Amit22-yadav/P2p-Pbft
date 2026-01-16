//! P2P Blockchain with PBFT Consensus
//!
//! This library provides a complete implementation of a blockchain
//! using the Practical Byzantine Fault Tolerance (PBFT) consensus algorithm.
//!
//! ## Features
//!
//! - **P2P Networking**: TCP-based peer-to-peer communication with automatic peer discovery
//! - **PBFT Consensus**: Full implementation of PBFT including normal case operation,
//!   view changes, and checkpointing
//! - **Blockchain**: Block structure, transactions, persistent storage (RocksDB)
//! - **Account Model**: Ethereum-style accounts with balances and nonces
//! - **JSON-RPC API**: Ethereum-compatible JSON-RPC interface
//! - **Cryptographic Signatures**: Ed25519 signatures for message authentication
//!
//! ## Example
//!
//! ```rust,no_run
//! use p2p_pbft::blockchain::{Blockchain, BlockchainConfig};
//! use p2p_pbft::crypto::KeyPair;
//! use std::path::PathBuf;
//!
//! #[tokio::main]
//! async fn main() {
//!     let keypair = KeyPair::generate();
//!     let config = BlockchainConfig::new(
//!         PathBuf::from("./data"),
//!         "my-chain".to_string(),
//!     );
//!
//!     let blockchain = Blockchain::new(config, keypair).unwrap();
//!     blockchain.init_genesis().unwrap();
//! }
//! ```

// Core modules (original)
pub mod crypto;
pub mod message;
pub mod network;
pub mod node;
pub mod pbft;

// Blockchain modules (new)
pub mod account;
pub mod blockchain;
pub mod chain;
pub mod execution;
pub mod mempool;
pub mod merkle;
pub mod rpc;
pub mod storage;
pub mod types;

// Re-exports (original)
pub use crypto::KeyPair;
pub use message::{ConsensusMessage, NetworkMessage, Request};
pub use network::{Network, NetworkError, PeerInfo};
pub use node::{Node, NodeBuilder};
pub use pbft::{OutgoingMessage, PbftConfig, PbftConsensus, PbftError, PbftState};

// Re-exports (blockchain)
pub use blockchain::{Blockchain, BlockchainBuilder, BlockchainConfig};
pub use chain::{ChainConfig, ChainManager};
pub use mempool::{Mempool, MempoolConfig};
pub use storage::Storage;
pub use types::{Account, Address, Block, BlockHeader, Transaction, TransactionPayload};
