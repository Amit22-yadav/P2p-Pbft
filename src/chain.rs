use crate::account::AccountManager;
use crate::execution::{BlockExecutionResult, ExecutionEngine};
use crate::mempool::Mempool;
use crate::merkle::compute_merkle_root;
use crate::message::{Hash, NodeId, SequenceNumber, ViewNumber};
use crate::storage::{SharedStorage, StorageError};
use crate::types::{Account, Block, GenesisConfig, Transaction};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use thiserror::Error;
use tracing::{debug, info};

#[derive(Error, Debug)]
pub enum ChainError {
    #[error("Invalid previous hash: expected {expected}, got {got}")]
    InvalidPrevHash { expected: String, got: String },
    #[error("Invalid block height: expected {expected}, got {got}")]
    InvalidHeight { expected: u64, got: u64 },
    #[error("Invalid merkle root")]
    InvalidMerkleRoot,
    #[error("Invalid state root")]
    InvalidStateRoot,
    #[error("Genesis block already exists")]
    GenesisExists,
    #[error("Genesis block not found")]
    GenesisNotFound,
    #[error("Block not found at height {0}")]
    BlockNotFound(u64),
    #[error("Storage error: {0}")]
    Storage(#[from] StorageError),
    #[error("Execution error: {0}")]
    Execution(String),
}

/// Chain configuration
#[derive(Debug, Clone)]
pub struct ChainConfig {
    /// Maximum transactions per block
    pub max_block_txs: usize,
    /// Maximum block size in bytes
    pub max_block_size: usize,
}

impl Default for ChainConfig {
    fn default() -> Self {
        Self {
            max_block_txs: 1000,
            max_block_size: 10 * 1024 * 1024, // 10 MB
        }
    }
}

/// Chain manager handles block lifecycle
pub struct ChainManager {
    storage: SharedStorage,
    genesis_config: GenesisConfig,
    current_height: AtomicU64,
    config: ChainConfig,
}

impl ChainManager {
    /// Create a new chain manager
    pub fn new(
        storage: SharedStorage,
        genesis_config: GenesisConfig,
        config: ChainConfig,
    ) -> Result<Self, ChainError> {
        let height = storage.get_chain_height()?;

        Ok(Self {
            storage,
            genesis_config,
            current_height: AtomicU64::new(height),
            config,
        })
    }

    /// Initialize the genesis block if it doesn't exist
    pub fn init_genesis(
        &self,
        account_manager: &AccountManager,
    ) -> Result<Option<Block>, ChainError> {
        // Check if genesis already exists
        if self.storage.has_genesis()? {
            info!("Genesis block already exists");
            return Ok(None);
        }

        info!("Initializing genesis block");

        // Set up initial accounts
        for (address, balance) in &self.genesis_config.accounts {
            let account = Account::with_balance(*address, *balance);
            account_manager.update_cached(account);
        }

        // Compute initial state root
        let state_root = account_manager
            .compute_state_root()
            .map_err(|e| ChainError::Execution(e.to_string()))?;

        // Create genesis block
        let genesis = self.genesis_config.create_genesis_block(state_root);

        // Persist genesis block and accounts
        let accounts = account_manager.get_modified_accounts();
        self.storage.commit_block(&genesis, &accounts, &[])?;

        // Clear account cache
        account_manager.clear_cache();

        info!(
            "Genesis block created: height=0, hash={}",
            hex::encode(&genesis.hash()[..8])
        );

        Ok(Some(genesis))
    }

    /// Get current chain height
    pub fn height(&self) -> u64 {
        self.current_height.load(Ordering::SeqCst)
    }

    /// Get the latest block
    pub fn latest_block(&self) -> Result<Block, ChainError> {
        let height = self.height();
        self.storage
            .get_block_by_height(height)?
            .ok_or(ChainError::BlockNotFound(height))
    }

    /// Get block by height
    pub fn get_block(&self, height: u64) -> Result<Option<Block>, ChainError> {
        Ok(self.storage.get_block_by_height(height)?)
    }

    /// Get block by hash
    pub fn get_block_by_hash(&self, hash: &Hash) -> Result<Option<Block>, ChainError> {
        Ok(self.storage.get_block_by_hash(hash)?)
    }

    /// Get the genesis block
    pub fn genesis_block(&self) -> Result<Block, ChainError> {
        self.storage
            .get_block_by_height(0)?
            .ok_or(ChainError::GenesisNotFound)
    }

    /// Validate a proposed block
    pub fn validate_block(&self, block: &Block) -> Result<(), ChainError> {
        let current_height = self.height();
        let expected_height = current_height + 1;

        // Check height
        if block.height() != expected_height {
            return Err(ChainError::InvalidHeight {
                expected: expected_height,
                got: block.height(),
            });
        }

        // Get previous block and check hash
        let prev_block = self.latest_block()?;
        let prev_hash = prev_block.hash();

        if block.header.prev_hash != prev_hash {
            return Err(ChainError::InvalidPrevHash {
                expected: hex::encode(&prev_hash[..8]),
                got: hex::encode(&block.header.prev_hash[..8]),
            });
        }

        // Verify merkle root
        let tx_hashes: Vec<Hash> = block.transactions.iter().map(|tx| tx.hash).collect();
        let computed_root = compute_merkle_root(&tx_hashes);

        if block.header.tx_root != computed_root {
            return Err(ChainError::InvalidMerkleRoot);
        }

        // Check block size
        let block_size = bincode::serialize(block)
            .map_err(|e| ChainError::Execution(e.to_string()))?
            .len();
        if block_size > self.config.max_block_size {
            return Err(ChainError::Execution(format!(
                "Block too large: {} > {}",
                block_size, self.config.max_block_size
            )));
        }

        // Check transaction count
        if block.transactions.len() > self.config.max_block_txs {
            return Err(ChainError::Execution(format!(
                "Too many transactions: {} > {}",
                block.transactions.len(),
                self.config.max_block_txs
            )));
        }

        Ok(())
    }

    /// Validate and execute a block, returning execution result
    pub fn validate_and_execute_block(
        &self,
        block: &Block,
        execution_engine: &ExecutionEngine,
        account_manager: &AccountManager,
    ) -> Result<BlockExecutionResult, ChainError> {
        // First validate structure
        self.validate_block(block)?;

        // Execute block
        let result = execution_engine
            .execute_block(block, account_manager)
            .map_err(|e| ChainError::Execution(e.to_string()))?;

        // Verify state root matches
        if block.header.state_root != result.state_root {
            return Err(ChainError::InvalidStateRoot);
        }

        Ok(result)
    }

    /// Commit a finalized block (after PBFT consensus)
    pub fn commit_block(
        &self,
        block: Block,
        execution_result: BlockExecutionResult,
    ) -> Result<(), ChainError> {
        let height = block.height();

        debug!(
            "Committing block: height={}, txs={}, hash={}",
            height,
            block.transactions.len(),
            hex::encode(&block.hash()[..8])
        );

        // Persist block with accounts and receipts
        self.storage.commit_block(
            &block,
            &execution_result.modified_accounts,
            &execution_result.receipts,
        )?;

        // Update current height
        self.current_height.store(height, Ordering::SeqCst);

        info!(
            "Block committed: height={}, txs={}, hash={}",
            height,
            block.transactions.len(),
            hex::encode(&block.hash()[..8])
        );

        Ok(())
    }

    /// Create a new block proposal (for primary node)
    pub fn propose_block(
        &self,
        transactions: Vec<Transaction>,
        proposer: NodeId,
        view: ViewNumber,
        sequence: SequenceNumber,
        account_manager: &AccountManager,
        execution_engine: &ExecutionEngine,
    ) -> Result<Block, ChainError> {
        let current_height = self.height();
        let new_height = current_height + 1;

        // Get previous block hash
        let prev_hash = self.latest_block()?.hash();

        // Compute merkle root
        let tx_hashes: Vec<Hash> = transactions.iter().map(|tx| tx.hash).collect();
        let tx_root = compute_merkle_root(&tx_hashes);

        // Execute transactions to get state root
        let temp_block = Block::new(
            new_height,
            prev_hash,
            transactions.clone(),
            tx_root,
            [0u8; 32], // Placeholder state root
            proposer.clone(),
            view,
            sequence,
        );

        let execution_result = execution_engine
            .execute_block(&temp_block, account_manager)
            .map_err(|e| ChainError::Execution(e.to_string()))?;

        // Create final block with correct state root
        let block = Block::new(
            new_height,
            prev_hash,
            transactions,
            tx_root,
            execution_result.state_root,
            proposer,
            view,
            sequence,
        );

        debug!(
            "Block proposed: height={}, txs={}, hash={}",
            new_height,
            block.transactions.len(),
            hex::encode(&block.hash()[..8])
        );

        Ok(block)
    }

    /// Get pending transactions from mempool and create block proposal
    pub fn create_block_proposal(
        &self,
        mempool: &Mempool,
        proposer: NodeId,
        view: ViewNumber,
        sequence: SequenceNumber,
        account_manager: &AccountManager,
        execution_engine: &ExecutionEngine,
    ) -> Result<Block, ChainError> {
        // Get transactions from mempool
        let transactions = mempool.get_pending_ordered(self.config.max_block_txs, account_manager);

        debug!(
            "Creating block proposal with {} transactions",
            transactions.len()
        );

        self.propose_block(
            transactions,
            proposer,
            view,
            sequence,
            account_manager,
            execution_engine,
        )
    }

    /// Get chain statistics
    pub fn stats(&self) -> ChainStats {
        ChainStats {
            height: self.height(),
            genesis_hash: self
                .storage
                .get_genesis_hash()
                .ok()
                .flatten()
                .unwrap_or([0u8; 32]),
        }
    }
}

/// Chain statistics
#[derive(Debug, Clone)]
pub struct ChainStats {
    pub height: u64,
    pub genesis_hash: Hash,
}

/// Thread-safe wrapper
pub type SharedChainManager = Arc<ChainManager>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::KeyPair;
    use crate::storage::Storage;
    use crate::types::public_key_to_address;

    fn temp_chain_manager() -> (ChainManager, SharedStorage, AccountManager, ExecutionEngine) {
        let temp_dir = tempfile::tempdir().unwrap();
        let storage = Arc::new(Storage::open(temp_dir.path()).unwrap());

        let genesis_config = GenesisConfig::new("test-chain".to_string(), vec![]);
        let chain_manager =
            ChainManager::new(storage.clone(), genesis_config, ChainConfig::default()).unwrap();

        let account_manager = AccountManager::new(storage.clone());
        let execution_engine = ExecutionEngine::new(storage.clone());

        (chain_manager, storage, account_manager, execution_engine)
    }

    #[test]
    fn test_init_genesis() {
        let (chain_manager, storage, account_manager, _) = temp_chain_manager();

        // Initialize genesis
        let genesis = chain_manager.init_genesis(&account_manager).unwrap();
        assert!(genesis.is_some());

        let genesis = genesis.unwrap();
        assert_eq!(genesis.height(), 0);
        assert!(genesis.is_genesis());

        // Check it's stored
        assert!(storage.has_genesis().unwrap());
        assert_eq!(chain_manager.height(), 0);

        // Second init should return None
        let genesis2 = chain_manager.init_genesis(&account_manager).unwrap();
        assert!(genesis2.is_none());
    }

    #[test]
    fn test_genesis_with_accounts() {
        let temp_dir = tempfile::tempdir().unwrap();
        let storage = Arc::new(Storage::open(temp_dir.path()).unwrap());

        let addr1 = [1u8; 20];
        let addr2 = [2u8; 20];

        let genesis_config = GenesisConfig::new("test-chain".to_string(), vec![])
            .with_accounts(vec![(addr1, 1000), (addr2, 2000)]);

        let chain_manager =
            ChainManager::new(storage.clone(), genesis_config, ChainConfig::default()).unwrap();

        let account_manager = AccountManager::new(storage.clone());

        chain_manager.init_genesis(&account_manager).unwrap();

        // Check accounts are initialized
        let acc1 = storage.get_account(&addr1).unwrap().unwrap();
        let acc2 = storage.get_account(&addr2).unwrap().unwrap();

        assert_eq!(acc1.balance, 1000);
        assert_eq!(acc2.balance, 2000);
    }

    #[test]
    fn test_propose_and_commit_block() {
        let (chain_manager, storage, account_manager, execution_engine) = temp_chain_manager();

        // Initialize genesis
        chain_manager.init_genesis(&account_manager).unwrap();

        // Create keypair and fund account
        let keypair = KeyPair::generate();
        let from = public_key_to_address(&keypair.public_key_bytes());
        let to = [2u8; 20];

        let sender = Account::with_balance(from, 1000);
        storage.put_account(&sender).unwrap();

        // Create transaction
        let tx = Transaction::transfer(&keypair, to, 100, 0);

        // Propose block
        let block = chain_manager
            .propose_block(
                vec![tx],
                "node1".to_string(),
                0,
                1,
                &account_manager,
                &execution_engine,
            )
            .unwrap();

        assert_eq!(block.height(), 1);
        assert_eq!(block.transactions.len(), 1);

        // Validate block
        chain_manager.validate_block(&block).unwrap();

        // Execute and commit
        let account_manager2 = AccountManager::new(storage.clone());
        let result = chain_manager
            .validate_and_execute_block(&block, &execution_engine, &account_manager2)
            .unwrap();

        chain_manager.commit_block(block, result).unwrap();

        // Check height updated
        assert_eq!(chain_manager.height(), 1);

        // Check balances
        let sender = storage.get_account(&from).unwrap().unwrap();
        let receiver = storage.get_account(&to).unwrap().unwrap();

        assert_eq!(sender.balance, 900);
        assert_eq!(receiver.balance, 100);
    }

    #[test]
    fn test_validate_invalid_height() {
        let (chain_manager, _, account_manager, _) = temp_chain_manager();

        chain_manager.init_genesis(&account_manager).unwrap();

        // Create block with wrong height
        let block = Block::new(
            5, // Wrong height
            [0u8; 32],
            vec![],
            [0u8; 32],
            [0u8; 32],
            "node1".to_string(),
            0,
            1,
        );

        let result = chain_manager.validate_block(&block);
        assert!(matches!(result, Err(ChainError::InvalidHeight { .. })));
    }

    #[test]
    fn test_validate_invalid_prev_hash() {
        let (chain_manager, _, account_manager, _) = temp_chain_manager();

        chain_manager.init_genesis(&account_manager).unwrap();

        // Create block with wrong prev_hash
        let block = Block::new(
            1,
            [0xff; 32], // Wrong prev_hash
            vec![],
            [0u8; 32],
            [0u8; 32],
            "node1".to_string(),
            0,
            1,
        );

        let result = chain_manager.validate_block(&block);
        assert!(matches!(result, Err(ChainError::InvalidPrevHash { .. })));
    }

    #[test]
    fn test_chain_stats() {
        let (chain_manager, _, account_manager, _) = temp_chain_manager();

        chain_manager.init_genesis(&account_manager).unwrap();

        let stats = chain_manager.stats();
        assert_eq!(stats.height, 0);
        assert_ne!(stats.genesis_hash, [0u8; 32]);
    }
}
