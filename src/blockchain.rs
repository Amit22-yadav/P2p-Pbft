use crate::account::{AccountManager, SharedAccountManager};
use crate::chain::{ChainConfig, ChainManager, SharedChainManager};
use crate::crypto::KeyPair;
use crate::execution::{ExecutionEngine, SharedExecutionEngine};
use crate::mempool::{Mempool, MempoolConfig, SharedMempool};
use crate::message::{Hash, NodeId};
use crate::pbft::{OutgoingMessage, PbftConfig, PbftConsensus, PbftState};
use crate::storage::{SharedStorage, Storage};
use crate::types::{
    address_to_hex, public_key_to_address, Address, Block, GenesisConfig, Transaction,
};
use std::path::PathBuf;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::mpsc;
use tokio::time::Duration;
use tracing::{debug, info};

#[derive(Error, Debug)]
pub enum BlockchainError {
    #[error("Storage error: {0}")]
    Storage(#[from] crate::storage::StorageError),
    #[error("Chain error: {0}")]
    Chain(#[from] crate::chain::ChainError),
    #[error("Mempool error: {0}")]
    Mempool(#[from] crate::mempool::MempoolError),
    #[error("Execution error: {0}")]
    Execution(String),
    #[error("Consensus error: {0}")]
    Consensus(String),
    #[error("Network error: {0}")]
    Network(String),
    #[error("Not initialized")]
    NotInitialized,
    #[error("Already running")]
    AlreadyRunning,
}

/// Blockchain configuration
#[derive(Debug, Clone)]
pub struct BlockchainConfig {
    /// Data directory for storage
    pub data_dir: PathBuf,
    /// Block production interval
    pub block_interval: Duration,
    /// Maximum transactions per block
    pub max_block_txs: usize,
    /// Chain ID
    pub chain_id: String,
    /// Initial validators
    pub validators: Vec<NodeId>,
    /// Initial account balances
    pub initial_accounts: Vec<(Address, u64)>,
}

impl Default for BlockchainConfig {
    fn default() -> Self {
        Self {
            data_dir: PathBuf::from("./data"),
            block_interval: Duration::from_secs(5),
            max_block_txs: 1000,
            chain_id: "pbft-chain".to_string(),
            validators: Vec::new(),
            initial_accounts: Vec::new(),
        }
    }
}

impl BlockchainConfig {
    pub fn new(data_dir: PathBuf, chain_id: String) -> Self {
        Self {
            data_dir,
            chain_id,
            ..Default::default()
        }
    }

    pub fn with_validators(mut self, validators: Vec<NodeId>) -> Self {
        self.validators = validators;
        self
    }

    pub fn with_initial_accounts(mut self, accounts: Vec<(Address, u64)>) -> Self {
        self.initial_accounts = accounts;
        self
    }

    pub fn with_block_interval(mut self, interval: Duration) -> Self {
        self.block_interval = interval;
        self
    }
}

/// Main blockchain node
pub struct Blockchain {
    /// Node identity
    node_id: NodeId,
    keypair: KeyPair,
    address: Address,

    /// Core components
    storage: SharedStorage,
    chain: SharedChainManager,
    mempool: SharedMempool,
    account_manager: SharedAccountManager,
    execution_engine: SharedExecutionEngine,

    /// Consensus
    consensus: Option<Arc<PbftConsensus>>,

    /// Configuration
    config: BlockchainConfig,

    /// Channel for incoming transactions
    tx_receiver: Option<mpsc::Receiver<Transaction>>,
    tx_sender: mpsc::Sender<Transaction>,

    /// Channel for committed blocks from consensus
    block_receiver: Option<mpsc::Receiver<Block>>,
}

impl Blockchain {
    /// Create a new blockchain node
    pub fn new(config: BlockchainConfig, keypair: KeyPair) -> Result<Self, BlockchainError> {
        let node_id = keypair.public_key_hex();
        let address = public_key_to_address(&keypair.public_key_bytes());

        info!(
            "Initializing blockchain node: id={}, address={}",
            &node_id[..16],
            address_to_hex(&address)
        );

        // Create data directory if needed
        std::fs::create_dir_all(&config.data_dir).map_err(|e| {
            BlockchainError::Network(format!("Failed to create data dir: {}", e))
        })?;

        // Initialize storage
        let storage_path = config.data_dir.join("chain");
        let storage = Arc::new(Storage::open(&storage_path)?);

        // Create genesis config
        let genesis_config = GenesisConfig::new(config.chain_id.clone(), config.validators.clone())
            .with_accounts(config.initial_accounts.clone());

        // Initialize chain manager
        let chain_config = ChainConfig {
            max_block_txs: config.max_block_txs,
            ..Default::default()
        };
        let chain = Arc::new(ChainManager::new(
            storage.clone(),
            genesis_config,
            chain_config,
        )?);

        // Initialize other components
        let mempool = Arc::new(Mempool::new(MempoolConfig::default()));
        let account_manager = Arc::new(AccountManager::new(storage.clone()));
        let execution_engine = Arc::new(ExecutionEngine::new(storage.clone()));

        // Create transaction channel
        let (tx_sender, tx_receiver) = mpsc::channel(1000);

        Ok(Self {
            node_id,
            keypair,
            address,
            storage,
            chain,
            mempool,
            account_manager,
            execution_engine,
            consensus: None,
            config,
            tx_receiver: Some(tx_receiver),
            tx_sender,
            block_receiver: None,
        })
    }

    /// Initialize consensus with validators
    pub fn init_consensus(
        &mut self,
        replicas: Vec<NodeId>,
        outgoing_tx: mpsc::Sender<OutgoingMessage>,
        committed_tx: mpsc::Sender<crate::message::Request>,
    ) {
        let n = replicas.len();
        let pbft_config = PbftConfig::new(n);

        let state = PbftState::new(
            self.node_id.clone(),
            self.keypair.clone(),
            replicas,
            pbft_config,
        );

        // Create block commit channel
        let (block_tx, block_rx) = mpsc::channel(100);

        let consensus = PbftConsensus::with_block_channel(state, outgoing_tx, committed_tx, block_tx);

        self.consensus = Some(Arc::new(consensus));
        self.block_receiver = Some(block_rx);

        info!("Consensus initialized with {} replicas", n);
    }

    /// Get consensus reference
    pub fn consensus(&self) -> Option<Arc<PbftConsensus>> {
        self.consensus.clone()
    }

    /// Initialize genesis block if needed
    pub fn init_genesis(&self) -> Result<Option<Block>, BlockchainError> {
        let account_manager = AccountManager::new(self.storage.clone());
        Ok(self.chain.init_genesis(&account_manager)?)
    }

    /// Get node ID
    pub fn node_id(&self) -> &NodeId {
        &self.node_id
    }

    /// Get node address
    pub fn address(&self) -> &Address {
        &self.address
    }

    /// Get chain height
    pub fn height(&self) -> u64 {
        self.chain.height()
    }

    /// Get block by height
    pub fn get_block(&self, height: u64) -> Result<Option<Block>, BlockchainError> {
        Ok(self.chain.get_block(height)?)
    }

    /// Get latest block
    pub fn latest_block(&self) -> Result<Block, BlockchainError> {
        Ok(self.chain.latest_block()?)
    }

    /// Get account balance
    pub fn get_balance(&self, address: &Address) -> Result<u64, BlockchainError> {
        Ok(self
            .account_manager
            .get_balance(address)
            .map_err(|e| BlockchainError::Execution(e.to_string()))?)
    }

    /// Get account nonce
    pub fn get_nonce(&self, address: &Address) -> Result<u64, BlockchainError> {
        let account_nonce = self
            .account_manager
            .get_nonce(address)
            .map_err(|e| BlockchainError::Execution(e.to_string()))?;

        // Include pending transactions in nonce
        Ok(self.mempool.get_pending_nonce(address, account_nonce))
    }

    /// Get state value
    pub fn get_state(&self, address: &Address, key: &[u8]) -> Result<Option<Vec<u8>>, BlockchainError> {
        Ok(self.storage.get_state(address, key)?)
    }

    /// Get transaction by hash
    pub fn get_transaction(
        &self,
        hash: &Hash,
    ) -> Result<Option<(Transaction, u64, usize)>, BlockchainError> {
        Ok(self.storage.get_transaction(hash)?)
    }

    /// Submit a transaction
    pub async fn submit_transaction(&self, tx: Transaction) -> Result<Hash, BlockchainError> {
        let tx_hash = tx.hash;

        // Validate transaction
        let account_manager = AccountManager::new(self.storage.clone());
        self.mempool
            .add_with_validation(tx.clone(), &self.execution_engine, &account_manager)?;

        info!(
            "Transaction submitted: hash={}, from={}",
            hex::encode(&tx_hash[..8]),
            address_to_hex(&tx.from)
        );

        // Broadcast transaction to network
        if let Some(ref _consensus) = self.consensus {
            // We'll broadcast the transaction but actual handling is in the main loop
        }

        Ok(tx_hash)
    }

    /// Get mempool statistics
    pub fn mempool_stats(&self) -> crate::mempool::MempoolStats {
        self.mempool.stats()
    }

    /// Get chain statistics
    pub fn chain_stats(&self) -> crate::chain::ChainStats {
        self.chain.stats()
    }

    /// Check if we are the primary
    pub async fn is_primary(&self) -> bool {
        match &self.consensus {
            Some(c) => c.is_primary().await,
            None => false,
        }
    }

    /// Get the sender for submitting transactions
    pub fn transaction_sender(&self) -> mpsc::Sender<Transaction> {
        self.tx_sender.clone()
    }

    /// Take the transaction receiver (can only be called once)
    pub fn take_transaction_receiver(&mut self) -> Option<mpsc::Receiver<Transaction>> {
        self.tx_receiver.take()
    }

    /// Take the block receiver (can only be called once)
    pub fn take_block_receiver(&mut self) -> Option<mpsc::Receiver<Block>> {
        self.block_receiver.take()
    }

    /// Process a committed block
    pub async fn process_committed_block(&self, block: Block) -> Result<(), BlockchainError> {
        info!(
            "Processing committed block: height={}, txs={}, hash={}",
            block.height(),
            block.transactions.len(),
            hex::encode(&block.hash()[..8])
        );

        // Create fresh account manager for this block
        let account_manager = AccountManager::new(self.storage.clone());

        // Validate and execute block
        let execution_result = self.chain.validate_and_execute_block(
            &block,
            &self.execution_engine,
            &account_manager,
        )?;

        // Commit block
        self.chain.commit_block(block.clone(), execution_result)?;

        // Remove committed transactions from mempool
        let tx_hashes: Vec<Hash> = block.transactions.iter().map(|tx| tx.hash).collect();
        self.mempool.remove_committed(&tx_hashes);

        info!(
            "Block committed: height={}, new_chain_height={}",
            block.height(),
            self.chain.height()
        );

        Ok(())
    }

    /// Create and propose a new block (primary only)
    pub async fn propose_block(&self) -> Result<Option<Block>, BlockchainError> {
        let consensus = match &self.consensus {
            Some(c) => c,
            None => return Err(BlockchainError::NotInitialized),
        };

        if !consensus.is_primary().await {
            return Ok(None);
        }

        // Get pending transactions
        let account_manager = AccountManager::new(self.storage.clone());
        let transactions = self
            .mempool
            .get_pending_ordered(self.config.max_block_txs, &account_manager);

        if transactions.is_empty() {
            debug!("No transactions to include in block");
            return Ok(None);
        }

        info!("Proposing block with {} transactions", transactions.len());

        // Create block proposal
        let view = consensus.current_view().await;
        let sequence = consensus.current_sequence().await + 1;

        let block = self.chain.propose_block(
            transactions,
            self.node_id.clone(),
            view,
            sequence,
            &account_manager,
            &self.execution_engine,
        )?;

        // Submit to consensus
        consensus
            .handle_block_proposal(block.clone())
            .await
            .map_err(|e| BlockchainError::Consensus(e.to_string()))?;

        Ok(Some(block))
    }

    /// Get storage reference
    pub fn storage(&self) -> &SharedStorage {
        &self.storage
    }

    /// Get mempool reference
    pub fn mempool(&self) -> &SharedMempool {
        &self.mempool
    }

    /// Get execution engine reference
    pub fn execution_engine(&self) -> &SharedExecutionEngine {
        &self.execution_engine
    }

    /// Get keypair clone for signing
    pub fn keypair(&self) -> KeyPair {
        self.keypair.clone()
    }
}

/// Builder for Blockchain
pub struct BlockchainBuilder {
    config: BlockchainConfig,
    keypair: Option<KeyPair>,
}

impl BlockchainBuilder {
    pub fn new() -> Self {
        Self {
            config: BlockchainConfig::default(),
            keypair: None,
        }
    }

    pub fn data_dir(mut self, path: PathBuf) -> Self {
        self.config.data_dir = path;
        self
    }

    pub fn chain_id(mut self, id: String) -> Self {
        self.config.chain_id = id;
        self
    }

    pub fn validators(mut self, validators: Vec<NodeId>) -> Self {
        self.config.validators = validators;
        self
    }

    pub fn initial_accounts(mut self, accounts: Vec<(Address, u64)>) -> Self {
        self.config.initial_accounts = accounts;
        self
    }

    pub fn block_interval(mut self, interval: Duration) -> Self {
        self.config.block_interval = interval;
        self
    }

    pub fn max_block_txs(mut self, max: usize) -> Self {
        self.config.max_block_txs = max;
        self
    }

    pub fn keypair(mut self, keypair: KeyPair) -> Self {
        self.keypair = Some(keypair);
        self
    }

    pub fn build(self) -> Result<Blockchain, BlockchainError> {
        let keypair = self.keypair.unwrap_or_else(KeyPair::generate);
        Blockchain::new(self.config, keypair)
    }
}

impl Default for BlockchainBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_blockchain_creation() {
        let temp_dir = tempfile::tempdir().unwrap();
        let keypair = KeyPair::generate();

        let config = BlockchainConfig::new(
            temp_dir.path().to_path_buf(),
            "test-chain".to_string(),
        );

        let blockchain = Blockchain::new(config, keypair).unwrap();

        assert_eq!(blockchain.height(), 0);
    }

    #[tokio::test]
    async fn test_genesis_initialization() {
        let temp_dir = tempfile::tempdir().unwrap();
        let keypair = KeyPair::generate();
        let address = public_key_to_address(&keypair.public_key_bytes());

        let config = BlockchainConfig::new(
            temp_dir.path().to_path_buf(),
            "test-chain".to_string(),
        )
        .with_initial_accounts(vec![(address, 1000)]);

        let blockchain = Blockchain::new(config, keypair).unwrap();

        // Initialize genesis
        let genesis = blockchain.init_genesis().unwrap();
        assert!(genesis.is_some());

        // Check balance
        let balance = blockchain.get_balance(&address).unwrap();
        assert_eq!(balance, 1000);
    }

    #[tokio::test]
    async fn test_transaction_submission() {
        let temp_dir = tempfile::tempdir().unwrap();
        let keypair = KeyPair::generate();
        let address = public_key_to_address(&keypair.public_key_bytes());

        let config = BlockchainConfig::new(
            temp_dir.path().to_path_buf(),
            "test-chain".to_string(),
        )
        .with_initial_accounts(vec![(address, 1000)]);

        let blockchain = Blockchain::new(config, keypair.clone()).unwrap();
        blockchain.init_genesis().unwrap();

        // Create and submit transaction
        let to = [2u8; 20];
        let tx = Transaction::transfer(&keypair, to, 100, 0);
        let tx_hash = blockchain.submit_transaction(tx).await.unwrap();

        // Check mempool
        assert_eq!(blockchain.mempool_stats().total_transactions, 1);

        // Check pending nonce
        let nonce = blockchain.get_nonce(&address).unwrap();
        assert_eq!(nonce, 1); // Includes pending tx
    }

    #[test]
    fn test_blockchain_builder() {
        let temp_dir = tempfile::tempdir().unwrap();

        let blockchain = BlockchainBuilder::new()
            .data_dir(temp_dir.path().to_path_buf())
            .chain_id("test-chain".to_string())
            .block_interval(Duration::from_secs(10))
            .max_block_txs(500)
            .build()
            .unwrap();

        assert_eq!(blockchain.height(), 0);
    }
}
