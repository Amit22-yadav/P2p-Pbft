use crate::message::Hash;
use crate::types::{Account, Address, Block, Transaction, TransactionReceipt};
use rocksdb::{ColumnFamily, ColumnFamilyDescriptor, Options, WriteBatch, DB};
use std::path::Path;
use std::sync::Arc;
use thiserror::Error;

/// Column family names for different data types
const CF_BLOCKS: &str = "blocks";
const CF_BLOCK_HASH: &str = "block_hash";
const CF_ACCOUNTS: &str = "accounts";
const CF_STATE: &str = "state";
const CF_TRANSACTIONS: &str = "transactions";
const CF_RECEIPTS: &str = "receipts";
const CF_METADATA: &str = "metadata";

/// Metadata keys
const META_CHAIN_HEIGHT: &[u8] = b"chain_height";
const META_GENESIS_HASH: &[u8] = b"genesis_hash";

#[derive(Error, Debug)]
pub enum StorageError {
    #[error("RocksDB error: {0}")]
    RocksDb(#[from] rocksdb::Error),
    #[error("Serialization error: {0}")]
    Serialization(#[from] bincode::Error),
    #[error("Block not found: height={0}")]
    BlockNotFound(u64),
    #[error("Account not found: {0}")]
    AccountNotFound(String),
    #[error("Invalid data format")]
    InvalidFormat,
    #[error("Genesis already exists")]
    GenesisExists,
}

/// Persistent storage layer using RocksDB
pub struct Storage {
    db: DB,
}

impl Storage {
    /// Open storage at the given path
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, StorageError> {
        let mut opts = Options::default();
        opts.create_if_missing(true);
        opts.create_missing_column_families(true);

        let cf_descriptors = vec![
            ColumnFamilyDescriptor::new(CF_BLOCKS, Options::default()),
            ColumnFamilyDescriptor::new(CF_BLOCK_HASH, Options::default()),
            ColumnFamilyDescriptor::new(CF_ACCOUNTS, Options::default()),
            ColumnFamilyDescriptor::new(CF_STATE, Options::default()),
            ColumnFamilyDescriptor::new(CF_TRANSACTIONS, Options::default()),
            ColumnFamilyDescriptor::new(CF_RECEIPTS, Options::default()),
            ColumnFamilyDescriptor::new(CF_METADATA, Options::default()),
        ];

        let db = DB::open_cf_descriptors(&opts, path, cf_descriptors)?;

        Ok(Self { db })
    }

    /// Open storage with default test path
    #[cfg(test)]
    pub fn open_temp() -> Result<Self, StorageError> {
        let temp_dir = tempfile::tempdir().unwrap();
        Self::open(temp_dir.path())
    }

    // ==================== Block Operations ====================

    /// Store a block
    pub fn put_block(&self, block: &Block) -> Result<(), StorageError> {
        let cf_blocks = self.cf_handle(CF_BLOCKS)?;
        let cf_hash = self.cf_handle(CF_BLOCK_HASH)?;
        let cf_meta = self.cf_handle(CF_METADATA)?;

        let height_key = block.height().to_be_bytes();
        let block_data = bincode::serialize(block)?;
        let block_hash = block.hash();

        let mut batch = WriteBatch::default();

        // Store block by height
        batch.put_cf(&cf_blocks, &height_key, &block_data);

        // Index block by hash
        batch.put_cf(&cf_hash, &block_hash, &height_key);

        // Update chain height
        batch.put_cf(&cf_meta, META_CHAIN_HEIGHT, &height_key);

        // Store genesis hash if this is genesis
        if block.height() == 0 {
            batch.put_cf(&cf_meta, META_GENESIS_HASH, &block_hash);
        }

        self.db.write(batch)?;
        Ok(())
    }

    /// Get block by height
    pub fn get_block_by_height(&self, height: u64) -> Result<Option<Block>, StorageError> {
        let cf = self.cf_handle(CF_BLOCKS)?;
        let key = height.to_be_bytes();

        match self.db.get_cf(&cf, &key)? {
            Some(data) => Ok(Some(bincode::deserialize(&data)?)),
            None => Ok(None),
        }
    }

    /// Get block by hash
    pub fn get_block_by_hash(&self, hash: &Hash) -> Result<Option<Block>, StorageError> {
        let cf_hash = self.cf_handle(CF_BLOCK_HASH)?;

        match self.db.get_cf(&cf_hash, hash)? {
            Some(height_bytes) => {
                let height = u64::from_be_bytes(
                    height_bytes
                        .try_into()
                        .map_err(|_| StorageError::InvalidFormat)?,
                );
                self.get_block_by_height(height)
            }
            None => Ok(None),
        }
    }

    /// Get the latest block
    pub fn get_latest_block(&self) -> Result<Option<Block>, StorageError> {
        let height = self.get_chain_height()?;
        if height == 0 {
            // Check if genesis exists
            self.get_block_by_height(0)
        } else {
            self.get_block_by_height(height)
        }
    }

    /// Get current chain height
    pub fn get_chain_height(&self) -> Result<u64, StorageError> {
        let cf = self.cf_handle(CF_METADATA)?;

        match self.db.get_cf(&cf, META_CHAIN_HEIGHT)? {
            Some(bytes) => {
                let height =
                    u64::from_be_bytes(bytes.try_into().map_err(|_| StorageError::InvalidFormat)?);
                Ok(height)
            }
            None => Ok(0),
        }
    }

    /// Check if genesis block exists
    pub fn has_genesis(&self) -> Result<bool, StorageError> {
        let cf = self.cf_handle(CF_METADATA)?;
        Ok(self.db.get_cf(&cf, META_GENESIS_HASH)?.is_some())
    }

    /// Get genesis block hash
    pub fn get_genesis_hash(&self) -> Result<Option<Hash>, StorageError> {
        let cf = self.cf_handle(CF_METADATA)?;

        match self.db.get_cf(&cf, META_GENESIS_HASH)? {
            Some(bytes) => {
                let hash: Hash = bytes.try_into().map_err(|_| StorageError::InvalidFormat)?;
                Ok(Some(hash))
            }
            None => Ok(None),
        }
    }

    // ==================== Account Operations ====================

    /// Get account by address
    pub fn get_account(&self, address: &Address) -> Result<Option<Account>, StorageError> {
        let cf = self.cf_handle(CF_ACCOUNTS)?;

        match self.db.get_cf(&cf, address)? {
            Some(data) => Ok(Some(bincode::deserialize(&data)?)),
            None => Ok(None),
        }
    }

    /// Store account
    pub fn put_account(&self, account: &Account) -> Result<(), StorageError> {
        let cf = self.cf_handle(CF_ACCOUNTS)?;
        let data = bincode::serialize(account)?;
        self.db.put_cf(&cf, &account.address, &data)?;
        Ok(())
    }

    /// Get or create account
    pub fn get_or_create_account(&self, address: &Address) -> Result<Account, StorageError> {
        match self.get_account(address)? {
            Some(account) => Ok(account),
            None => Ok(Account::new(*address)),
        }
    }

    // ==================== State Operations ====================

    /// Get state value by key (prefixed with account address)
    pub fn get_state(&self, address: &Address, key: &[u8]) -> Result<Option<Vec<u8>>, StorageError> {
        let cf = self.cf_handle(CF_STATE)?;
        let full_key = make_state_key(address, key);

        Ok(self.db.get_cf(&cf, &full_key)?)
    }

    /// Set state value
    pub fn put_state(
        &self,
        address: &Address,
        key: &[u8],
        value: &[u8],
    ) -> Result<(), StorageError> {
        let cf = self.cf_handle(CF_STATE)?;
        let full_key = make_state_key(address, key);
        self.db.put_cf(&cf, &full_key, value)?;
        Ok(())
    }

    /// Delete state value
    pub fn delete_state(&self, address: &Address, key: &[u8]) -> Result<(), StorageError> {
        let cf = self.cf_handle(CF_STATE)?;
        let full_key = make_state_key(address, key);
        self.db.delete_cf(&cf, &full_key)?;
        Ok(())
    }

    // ==================== Transaction Operations ====================

    /// Store transaction with its block location
    pub fn put_transaction(
        &self,
        tx: &Transaction,
        block_height: u64,
        tx_index: usize,
    ) -> Result<(), StorageError> {
        let cf = self.cf_handle(CF_TRANSACTIONS)?;
        let location = TxLocation {
            block_height,
            tx_index,
        };
        let data = bincode::serialize(&(tx, location))?;
        self.db.put_cf(&cf, &tx.hash, &data)?;
        Ok(())
    }

    /// Get transaction by hash
    pub fn get_transaction(&self, hash: &Hash) -> Result<Option<(Transaction, u64, usize)>, StorageError> {
        let cf = self.cf_handle(CF_TRANSACTIONS)?;

        match self.db.get_cf(&cf, hash)? {
            Some(data) => {
                let (tx, loc): (Transaction, TxLocation) = bincode::deserialize(&data)?;
                Ok(Some((tx, loc.block_height, loc.tx_index)))
            }
            None => Ok(None),
        }
    }

    // ==================== Receipt Operations ====================

    /// Store transaction receipt
    pub fn put_receipt(&self, receipt: &TransactionReceipt) -> Result<(), StorageError> {
        let cf = self.cf_handle(CF_RECEIPTS)?;
        let data = bincode::serialize(receipt)?;
        self.db.put_cf(&cf, &receipt.tx_hash, &data)?;
        Ok(())
    }

    /// Get transaction receipt
    pub fn get_receipt(&self, tx_hash: &Hash) -> Result<Option<TransactionReceipt>, StorageError> {
        let cf = self.cf_handle(CF_RECEIPTS)?;

        match self.db.get_cf(&cf, tx_hash)? {
            Some(data) => Ok(Some(bincode::deserialize(&data)?)),
            None => Ok(None),
        }
    }

    // ==================== Batch Operations ====================

    /// Create a new write batch
    pub fn new_batch(&self) -> StorageWriteBatch {
        StorageWriteBatch {
            batch: WriteBatch::default(),
        }
    }

    /// Execute a batch of operations atomically
    pub fn write_batch(&self, batch: StorageWriteBatch) -> Result<(), StorageError> {
        self.db.write(batch.batch)?;
        Ok(())
    }

    /// Execute a block commit atomically (block + transactions + accounts + receipts)
    pub fn commit_block(
        &self,
        block: &Block,
        accounts: &[Account],
        receipts: &[TransactionReceipt],
    ) -> Result<(), StorageError> {
        let cf_blocks = self.cf_handle(CF_BLOCKS)?;
        let cf_hash = self.cf_handle(CF_BLOCK_HASH)?;
        let cf_accounts = self.cf_handle(CF_ACCOUNTS)?;
        let cf_txs = self.cf_handle(CF_TRANSACTIONS)?;
        let cf_receipts = self.cf_handle(CF_RECEIPTS)?;
        let cf_meta = self.cf_handle(CF_METADATA)?;

        let mut batch = WriteBatch::default();

        // Store block
        let height_key = block.height().to_be_bytes();
        let block_data = bincode::serialize(block)?;
        let block_hash = block.hash();
        batch.put_cf(&cf_blocks, &height_key, &block_data);
        batch.put_cf(&cf_hash, &block_hash, &height_key);

        // Update chain height
        batch.put_cf(&cf_meta, META_CHAIN_HEIGHT, &height_key);

        // Store genesis hash if this is genesis
        if block.height() == 0 {
            batch.put_cf(&cf_meta, META_GENESIS_HASH, &block_hash);
        }

        // Store transactions
        for (tx_index, tx) in block.transactions.iter().enumerate() {
            let location = TxLocation {
                block_height: block.height(),
                tx_index,
            };
            let data = bincode::serialize(&(tx, location))?;
            batch.put_cf(&cf_txs, &tx.hash, &data);
        }

        // Store updated accounts
        for account in accounts {
            let data = bincode::serialize(account)?;
            batch.put_cf(&cf_accounts, &account.address, &data);
        }

        // Store receipts
        for receipt in receipts {
            let data = bincode::serialize(receipt)?;
            batch.put_cf(&cf_receipts, &receipt.tx_hash, &data);
        }

        self.db.write(batch)?;
        Ok(())
    }

    // ==================== Helpers ====================

    fn cf_handle(&self, name: &str) -> Result<&ColumnFamily, StorageError> {
        self.db
            .cf_handle(name)
            .ok_or_else(|| StorageError::InvalidFormat)
    }
}

/// Transaction location in block
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct TxLocation {
    block_height: u64,
    tx_index: usize,
}

/// Create a state key by prefixing with address
fn make_state_key(address: &Address, key: &[u8]) -> Vec<u8> {
    let mut full_key = Vec::with_capacity(20 + key.len());
    full_key.extend_from_slice(address);
    full_key.extend_from_slice(key);
    full_key
}

/// Batch of storage operations for atomic execution
pub struct StorageWriteBatch {
    batch: WriteBatch,
}

/// Thread-safe wrapper for storage
pub type SharedStorage = Arc<Storage>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::KeyPair;
    use crate::types::{public_key_to_address, Transaction, TransactionPayload};

    fn temp_storage() -> Storage {
        let temp_dir = tempfile::tempdir().unwrap();
        Storage::open(temp_dir.path()).unwrap()
    }

    #[test]
    fn test_account_operations() {
        let storage = temp_storage();
        let address = [1u8; 20];

        // Initially no account
        assert!(storage.get_account(&address).unwrap().is_none());

        // Create and store account
        let account = Account::with_balance(address, 1000);
        storage.put_account(&account).unwrap();

        // Retrieve account
        let retrieved = storage.get_account(&address).unwrap().unwrap();
        assert_eq!(retrieved.balance, 1000);
        assert_eq!(retrieved.address, address);
    }

    #[test]
    fn test_state_operations() {
        let storage = temp_storage();
        let address = [1u8; 20];

        // Initially no state
        assert!(storage.get_state(&address, b"key1").unwrap().is_none());

        // Set state
        storage.put_state(&address, b"key1", b"value1").unwrap();

        // Get state
        let value = storage.get_state(&address, b"key1").unwrap().unwrap();
        assert_eq!(value, b"value1");

        // Delete state
        storage.delete_state(&address, b"key1").unwrap();
        assert!(storage.get_state(&address, b"key1").unwrap().is_none());
    }

    #[test]
    fn test_block_operations() {
        let storage = temp_storage();

        // Create a block
        let block = Block::new(
            0,
            [0u8; 32],
            vec![],
            [0u8; 32],
            [0u8; 32],
            "node1".to_string(),
            0,
            0,
        );

        // Store block
        storage.put_block(&block).unwrap();

        // Retrieve by height
        let retrieved = storage.get_block_by_height(0).unwrap().unwrap();
        assert_eq!(retrieved.height(), 0);

        // Retrieve by hash
        let by_hash = storage.get_block_by_hash(&block.hash()).unwrap().unwrap();
        assert_eq!(by_hash.height(), 0);

        // Check chain height
        assert_eq!(storage.get_chain_height().unwrap(), 0);

        // Check genesis
        assert!(storage.has_genesis().unwrap());
    }

    #[test]
    fn test_transaction_operations() {
        let storage = temp_storage();
        let keypair = KeyPair::generate();
        let to = [2u8; 20];

        let tx = Transaction::transfer(&keypair, to, 100, 0);
        let tx_hash = tx.hash;

        // Store transaction
        storage.put_transaction(&tx, 1, 0).unwrap();

        // Retrieve transaction
        let (retrieved, height, index) = storage.get_transaction(&tx_hash).unwrap().unwrap();
        assert_eq!(retrieved.hash, tx_hash);
        assert_eq!(height, 1);
        assert_eq!(index, 0);
    }

    #[test]
    fn test_commit_block_atomic() {
        let storage = temp_storage();

        // Create a block with a transaction
        let keypair = KeyPair::generate();
        let from = public_key_to_address(&keypair.public_key_bytes());
        let to = [2u8; 20];

        let tx = Transaction::transfer(&keypair, to, 100, 0);
        let block = Block::new(
            1,
            [0u8; 32],
            vec![tx.clone()],
            [1u8; 32],
            [2u8; 32],
            "node1".to_string(),
            0,
            1,
        );

        // Prepare accounts
        let sender = Account {
            address: from,
            balance: 900,
            nonce: 1,
        };
        let receiver = Account {
            address: to,
            balance: 100,
            nonce: 0,
        };

        // Prepare receipt
        let receipt = TransactionReceipt {
            tx_hash: tx.hash,
            success: true,
            block_height: 1,
            tx_index: 0,
            error: None,
        };

        // Commit everything atomically
        storage
            .commit_block(&block, &[sender.clone(), receiver.clone()], &[receipt.clone()])
            .unwrap();

        // Verify block
        let stored_block = storage.get_block_by_height(1).unwrap().unwrap();
        assert_eq!(stored_block.height(), 1);

        // Verify accounts
        let stored_sender = storage.get_account(&from).unwrap().unwrap();
        assert_eq!(stored_sender.balance, 900);

        let stored_receiver = storage.get_account(&to).unwrap().unwrap();
        assert_eq!(stored_receiver.balance, 100);

        // Verify transaction
        let (stored_tx, _, _) = storage.get_transaction(&tx.hash).unwrap().unwrap();
        assert_eq!(stored_tx.hash, tx.hash);

        // Verify receipt
        let stored_receipt = storage.get_receipt(&tx.hash).unwrap().unwrap();
        assert!(stored_receipt.success);
    }
}
