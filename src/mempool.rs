use crate::account::AccountManager;
use crate::execution::ExecutionEngine;
use crate::message::Hash;
use crate::types::{Address, Transaction};
use parking_lot::RwLock;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum MempoolError {
    #[error("Transaction already exists")]
    DuplicateTransaction,
    #[error("Mempool is full")]
    MempoolFull,
    #[error("Transaction validation failed: {0}")]
    ValidationFailed(String),
    #[error("Too many pending transactions from sender")]
    TooManyFromSender,
    #[error("Transaction nonce too low")]
    NonceTooLow,
    #[error("Transaction nonce gap")]
    NonceGap,
}

/// Mempool configuration
#[derive(Debug, Clone)]
pub struct MempoolConfig {
    /// Maximum number of transactions in the pool
    pub max_size: usize,
    /// Maximum transactions per sender
    pub max_per_sender: usize,
    /// Maximum transaction size in bytes
    pub max_tx_size: usize,
}

impl Default for MempoolConfig {
    fn default() -> Self {
        Self {
            max_size: 10000,
            max_per_sender: 100,
            max_tx_size: 1024 * 1024, // 1 MB
        }
    }
}

/// Transaction pool for pending transactions
pub struct Mempool {
    /// Pending transactions by hash
    transactions: RwLock<HashMap<Hash, Transaction>>,
    /// Transaction queue ordered by arrival time
    queue: RwLock<VecDeque<Hash>>,
    /// Transactions grouped by sender
    by_sender: RwLock<HashMap<Address, HashSet<Hash>>>,
    /// Expected next nonce per sender (for nonce tracking)
    pending_nonces: RwLock<HashMap<Address, u64>>,
    /// Configuration
    config: MempoolConfig,
}

impl Mempool {
    pub fn new(config: MempoolConfig) -> Self {
        Self {
            transactions: RwLock::new(HashMap::new()),
            queue: RwLock::new(VecDeque::new()),
            by_sender: RwLock::new(HashMap::new()),
            pending_nonces: RwLock::new(HashMap::new()),
            config,
        }
    }

    /// Add a transaction to the mempool
    pub fn add(&self, tx: Transaction) -> Result<(), MempoolError> {
        let tx_hash = tx.hash;

        // Check if already exists
        {
            let transactions = self.transactions.read();
            if transactions.contains_key(&tx_hash) {
                return Err(MempoolError::DuplicateTransaction);
            }
        }

        // Check pool size
        {
            let transactions = self.transactions.read();
            if transactions.len() >= self.config.max_size {
                return Err(MempoolError::MempoolFull);
            }
        }

        // Check per-sender limit
        {
            let by_sender = self.by_sender.read();
            if let Some(sender_txs) = by_sender.get(&tx.from) {
                if sender_txs.len() >= self.config.max_per_sender {
                    return Err(MempoolError::TooManyFromSender);
                }
            }
        }

        // Add to all data structures
        {
            let mut transactions = self.transactions.write();
            let mut queue = self.queue.write();
            let mut by_sender = self.by_sender.write();
            let mut pending_nonces = self.pending_nonces.write();

            transactions.insert(tx_hash, tx.clone());
            queue.push_back(tx_hash);

            by_sender
                .entry(tx.from)
                .or_insert_with(HashSet::new)
                .insert(tx_hash);

            // Update pending nonce
            let entry = pending_nonces.entry(tx.from).or_insert(tx.nonce);
            if tx.nonce >= *entry {
                *entry = tx.nonce + 1;
            }
        }

        Ok(())
    }

    /// Add a transaction with validation
    pub fn add_with_validation(
        &self,
        tx: Transaction,
        execution_engine: &ExecutionEngine,
        account_manager: &AccountManager,
    ) -> Result<(), MempoolError> {
        // Validate transaction
        if let Err(e) = execution_engine.validate_transaction(&tx, account_manager) {
            return Err(MempoolError::ValidationFailed(e.to_string()));
        }

        self.add(tx)
    }

    /// Remove a transaction by hash
    pub fn remove(&self, hash: &Hash) -> Option<Transaction> {
        let mut transactions = self.transactions.write();
        let mut queue = self.queue.write();
        let mut by_sender = self.by_sender.write();

        if let Some(tx) = transactions.remove(hash) {
            queue.retain(|h| h != hash);

            if let Some(sender_txs) = by_sender.get_mut(&tx.from) {
                sender_txs.remove(hash);
                if sender_txs.is_empty() {
                    by_sender.remove(&tx.from);
                }
            }

            Some(tx)
        } else {
            None
        }
    }

    /// Remove multiple transactions (after block commit)
    pub fn remove_committed(&self, tx_hashes: &[Hash]) {
        let mut transactions = self.transactions.write();
        let mut queue = self.queue.write();
        let mut by_sender = self.by_sender.write();

        let hash_set: HashSet<_> = tx_hashes.iter().collect();

        for hash in tx_hashes {
            if let Some(tx) = transactions.remove(hash) {
                if let Some(sender_txs) = by_sender.get_mut(&tx.from) {
                    sender_txs.remove(hash);
                    if sender_txs.is_empty() {
                        by_sender.remove(&tx.from);
                    }
                }
            }
        }

        queue.retain(|h| !hash_set.contains(h));
    }

    /// Get transactions for block proposal (ordered by arrival)
    pub fn get_pending(&self, max_count: usize) -> Vec<Transaction> {
        let transactions = self.transactions.read();
        let queue = self.queue.read();

        queue
            .iter()
            .take(max_count)
            .filter_map(|hash| transactions.get(hash).cloned())
            .collect()
    }

    /// Get transactions for block proposal, respecting nonce order per sender
    pub fn get_pending_ordered(&self, max_count: usize, account_manager: &AccountManager) -> Vec<Transaction> {
        let transactions = self.transactions.read();
        let by_sender = self.by_sender.read();

        let mut result = Vec::new();
        let mut selected: HashSet<Hash> = HashSet::new();

        // For each sender, select transactions in nonce order
        for (sender, tx_hashes) in by_sender.iter() {
            // Get current nonce for sender
            let current_nonce = account_manager
                .get_nonce(sender)
                .unwrap_or(0);

            // Collect and sort transactions by nonce
            let mut sender_txs: Vec<_> = tx_hashes
                .iter()
                .filter_map(|h| transactions.get(h))
                .collect();
            sender_txs.sort_by_key(|tx| tx.nonce);

            // Select consecutive transactions starting from current nonce
            let mut expected_nonce = current_nonce;
            for tx in sender_txs {
                if tx.nonce == expected_nonce {
                    result.push(tx.clone());
                    selected.insert(tx.hash);
                    expected_nonce += 1;
                } else if tx.nonce > expected_nonce {
                    // Gap in nonces - stop selecting from this sender
                    break;
                }
                // Skip if nonce < expected (already processed)
            }
        }

        // Sort result by original queue order
        let queue = self.queue.read();
        let queue_order: HashMap<_, _> = queue.iter().enumerate().map(|(i, h)| (*h, i)).collect();
        result.sort_by_key(|tx| queue_order.get(&tx.hash).copied().unwrap_or(usize::MAX));

        result.into_iter().take(max_count).collect()
    }

    /// Check if transaction exists in mempool
    pub fn contains(&self, hash: &Hash) -> bool {
        let transactions = self.transactions.read();
        transactions.contains_key(hash)
    }

    /// Get a transaction by hash
    pub fn get(&self, hash: &Hash) -> Option<Transaction> {
        let transactions = self.transactions.read();
        transactions.get(hash).cloned()
    }

    /// Get current mempool size
    pub fn len(&self) -> usize {
        let transactions = self.transactions.read();
        transactions.len()
    }

    /// Check if mempool is empty
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Get transactions from a specific sender
    pub fn get_by_sender(&self, sender: &Address) -> Vec<Transaction> {
        let transactions = self.transactions.read();
        let by_sender = self.by_sender.read();

        if let Some(hashes) = by_sender.get(sender) {
            hashes
                .iter()
                .filter_map(|h| transactions.get(h).cloned())
                .collect()
        } else {
            Vec::new()
        }
    }

    /// Get the pending nonce for a sender (account nonce + pending txs)
    pub fn get_pending_nonce(&self, sender: &Address, current_nonce: u64) -> u64 {
        let pending_nonces = self.pending_nonces.read();
        pending_nonces.get(sender).copied().unwrap_or(current_nonce)
    }

    /// Clear all transactions
    pub fn clear(&self) {
        let mut transactions = self.transactions.write();
        let mut queue = self.queue.write();
        let mut by_sender = self.by_sender.write();
        let mut pending_nonces = self.pending_nonces.write();

        transactions.clear();
        queue.clear();
        by_sender.clear();
        pending_nonces.clear();
    }

    /// Get mempool statistics
    pub fn stats(&self) -> MempoolStats {
        let transactions = self.transactions.read();
        let by_sender = self.by_sender.read();

        MempoolStats {
            total_transactions: transactions.len(),
            unique_senders: by_sender.len(),
            max_size: self.config.max_size,
        }
    }
}

/// Mempool statistics
#[derive(Debug, Clone)]
pub struct MempoolStats {
    pub total_transactions: usize,
    pub unique_senders: usize,
    pub max_size: usize,
}

/// Thread-safe wrapper
pub type SharedMempool = Arc<Mempool>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::KeyPair;
    use crate::storage::Storage;
    use crate::types::public_key_to_address;

    fn temp_mempool() -> Mempool {
        Mempool::new(MempoolConfig::default())
    }

    #[test]
    fn test_add_and_get() {
        let mempool = temp_mempool();
        let keypair = KeyPair::generate();
        let to = [2u8; 20];

        let tx = Transaction::transfer(&keypair, to, 100, 0);
        let hash = tx.hash;

        mempool.add(tx.clone()).unwrap();

        assert!(mempool.contains(&hash));
        assert_eq!(mempool.len(), 1);

        let retrieved = mempool.get(&hash).unwrap();
        assert_eq!(retrieved.hash, hash);
    }

    #[test]
    fn test_duplicate_rejection() {
        let mempool = temp_mempool();
        let keypair = KeyPair::generate();
        let to = [2u8; 20];

        let tx = Transaction::transfer(&keypair, to, 100, 0);

        mempool.add(tx.clone()).unwrap();
        let result = mempool.add(tx);

        assert!(matches!(result, Err(MempoolError::DuplicateTransaction)));
    }

    #[test]
    fn test_remove() {
        let mempool = temp_mempool();
        let keypair = KeyPair::generate();
        let to = [2u8; 20];

        let tx = Transaction::transfer(&keypair, to, 100, 0);
        let hash = tx.hash;

        mempool.add(tx).unwrap();
        assert_eq!(mempool.len(), 1);

        mempool.remove(&hash);
        assert_eq!(mempool.len(), 0);
        assert!(!mempool.contains(&hash));
    }

    #[test]
    fn test_get_pending() {
        let mempool = temp_mempool();
        let keypair = KeyPair::generate();
        let to = [2u8; 20];

        // Add multiple transactions
        for i in 0..5 {
            let tx = Transaction::transfer(&keypair, to, 100, i);
            mempool.add(tx).unwrap();
        }

        // Get subset
        let pending = mempool.get_pending(3);
        assert_eq!(pending.len(), 3);
    }

    #[test]
    fn test_remove_committed() {
        let mempool = temp_mempool();
        let keypair = KeyPair::generate();
        let to = [2u8; 20];

        let tx1 = Transaction::transfer(&keypair, to, 100, 0);
        let tx2 = Transaction::transfer(&keypair, to, 200, 1);
        let tx3 = Transaction::transfer(&keypair, to, 300, 2);

        let hash1 = tx1.hash;
        let hash2 = tx2.hash;
        let hash3 = tx3.hash;

        mempool.add(tx1).unwrap();
        mempool.add(tx2).unwrap();
        mempool.add(tx3).unwrap();

        assert_eq!(mempool.len(), 3);

        mempool.remove_committed(&[hash1, hash2]);

        assert_eq!(mempool.len(), 1);
        assert!(mempool.contains(&hash3));
        assert!(!mempool.contains(&hash1));
        assert!(!mempool.contains(&hash2));
    }

    #[test]
    fn test_by_sender() {
        let mempool = temp_mempool();
        let keypair1 = KeyPair::generate();
        let keypair2 = KeyPair::generate();
        let to = [2u8; 20];

        let from1 = public_key_to_address(&keypair1.public_key_bytes());
        let from2 = public_key_to_address(&keypair2.public_key_bytes());

        // Add transactions from two senders
        mempool
            .add(Transaction::transfer(&keypair1, to, 100, 0))
            .unwrap();
        mempool
            .add(Transaction::transfer(&keypair1, to, 200, 1))
            .unwrap();
        mempool
            .add(Transaction::transfer(&keypair2, to, 300, 0))
            .unwrap();

        let sender1_txs = mempool.get_by_sender(&from1);
        let sender2_txs = mempool.get_by_sender(&from2);

        assert_eq!(sender1_txs.len(), 2);
        assert_eq!(sender2_txs.len(), 1);
    }

    #[test]
    fn test_max_per_sender() {
        let config = MempoolConfig {
            max_per_sender: 2,
            ..Default::default()
        };
        let mempool = Mempool::new(config);
        let keypair = KeyPair::generate();
        let to = [2u8; 20];

        mempool
            .add(Transaction::transfer(&keypair, to, 100, 0))
            .unwrap();
        mempool
            .add(Transaction::transfer(&keypair, to, 200, 1))
            .unwrap();

        let result = mempool.add(Transaction::transfer(&keypair, to, 300, 2));
        assert!(matches!(result, Err(MempoolError::TooManyFromSender)));
    }

    #[test]
    fn test_stats() {
        let mempool = temp_mempool();
        let keypair1 = KeyPair::generate();
        let keypair2 = KeyPair::generate();
        let to = [2u8; 20];

        mempool
            .add(Transaction::transfer(&keypair1, to, 100, 0))
            .unwrap();
        mempool
            .add(Transaction::transfer(&keypair2, to, 200, 0))
            .unwrap();

        let stats = mempool.stats();
        assert_eq!(stats.total_transactions, 2);
        assert_eq!(stats.unique_senders, 2);
    }
}
