//! P2P Network with PBFT Consensus
//!
//! This library provides a complete implementation of a peer-to-peer network
//! using the Practical Byzantine Fault Tolerance (PBFT) consensus algorithm.
//!
//! ## Features
//!
//! - **P2P Networking**: TCP-based peer-to-peer communication with automatic peer discovery
//! - **PBFT Consensus**: Full implementation of PBFT including normal case operation,
//!   view changes, and checkpointing
//! - **Cryptographic Signatures**: Ed25519 signatures for message authentication
//!
//! ## Example
//!
//! ```rust,no_run
//! use p2p_pbft::node::NodeBuilder;
//!
//! #[tokio::main]
//! async fn main() {
//!     let mut node = NodeBuilder::new(8000)
//!         .with_bootstrap_peer("127.0.0.1:8001".to_string())
//!         .build();
//!
//!     node.start().await.unwrap();
//! }
//! ```

pub mod crypto;
pub mod message;
pub mod network;
pub mod node;
pub mod pbft;

pub use crypto::KeyPair;
pub use message::{ConsensusMessage, NetworkMessage, Request};
pub use network::{Network, NetworkError, PeerInfo};
pub use node::{Node, NodeBuilder};
pub use pbft::{PbftConfig, PbftConsensus, PbftError, PbftState};
