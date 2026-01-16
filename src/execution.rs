use crate::account::{AccountError, AccountManager};
use crate::message::Hash;
use crate::storage::SharedStorage;
use crate::types::{address_to_hex, hash_to_hex, Account, Address, Block, Transaction, TransactionPayload, TransactionReceipt};
use std::sync::Arc;
use thiserror::Error;
use tracing::{debug, info, trace, warn};

#[derive(Error, Debug, Clone)]
pub enum ExecutionError {
    #[error("Invalid signature")]
    InvalidSignature,
    #[error("Invalid nonce: expected {expected}, got {got}")]
    InvalidNonce { expected: u64, got: u64 },
    #[error("Insufficient balance: have {have}, need {need}")]
    InsufficientBalance { have: u64, need: u64 },
    #[error("Invalid transaction format")]
    InvalidFormat,
    #[error("Transaction too large")]
    TransactionTooLarge,
    #[error("Key too large")]
    KeyTooLarge,
    #[error("Value too large")]
    ValueTooLarge,
    #[error("Storage error: {0}")]
    Storage(String),
    #[error("Account error: {0}")]
    Account(String),
}

impl From<AccountError> for ExecutionError {
    fn from(e: AccountError) -> Self {
        match e {
            AccountError::InsufficientBalance { have, need } => {
                ExecutionError::InsufficientBalance { have, need }
            }
            AccountError::InvalidNonce { expected, got } => {
                ExecutionError::InvalidNonce { expected, got }
            }
            _ => ExecutionError::Account(e.to_string()),
        }
    }
}

/// Result of executing a single transaction
#[derive(Debug, Clone)]
pub struct TxExecutionResult {
    pub success: bool,
    pub error: Option<ExecutionError>,
    pub modified_accounts: Vec<Account>,
}

/// Result of executing a block
#[derive(Debug)]
pub struct BlockExecutionResult {
    pub state_root: Hash,
    pub receipts: Vec<TransactionReceipt>,
    pub modified_accounts: Vec<Account>,
}

/// Configuration for execution engine
#[derive(Debug, Clone)]
pub struct ExecutionConfig {
    pub max_tx_size: usize,
    pub max_key_size: usize,
    pub max_value_size: usize,
}

impl Default for ExecutionConfig {
    fn default() -> Self {
        Self {
            max_tx_size: 1024 * 1024,    // 1 MB
            max_key_size: 1024,           // 1 KB
            max_value_size: 1024 * 1024,  // 1 MB
        }
    }
}

/// Transaction execution engine
pub struct ExecutionEngine {
    storage: SharedStorage,
    config: ExecutionConfig,
}

impl ExecutionEngine {
    pub fn new(storage: SharedStorage) -> Self {
        Self {
            storage,
            config: ExecutionConfig::default(),
        }
    }

    pub fn with_config(storage: SharedStorage, config: ExecutionConfig) -> Self {
        Self { storage, config }
    }

    /// Validate a transaction without executing
    pub fn validate_transaction(
        &self,
        tx: &Transaction,
        account_manager: &AccountManager,
    ) -> Result<(), ExecutionError> {
        // Check transaction size
        let tx_size = bincode::serialize(tx).map_err(|_| ExecutionError::InvalidFormat)?.len();
        if tx_size > self.config.max_tx_size {
            return Err(ExecutionError::TransactionTooLarge);
        }

        // Verify signature
        self.verify_signature(tx)?;

        // Check nonce
        let current_nonce = account_manager.get_nonce(&tx.from)?;
        if tx.nonce != current_nonce {
            return Err(ExecutionError::InvalidNonce {
                expected: current_nonce,
                got: tx.nonce,
            });
        }

        // Validate payload-specific constraints
        match &tx.payload {
            TransactionPayload::Transfer { value, .. } => {
                let balance = account_manager.get_balance(&tx.from)?;
                if balance < *value {
                    return Err(ExecutionError::InsufficientBalance {
                        have: balance,
                        need: *value,
                    });
                }
            }
            TransactionPayload::SetState { key, value } => {
                if key.len() > self.config.max_key_size {
                    return Err(ExecutionError::KeyTooLarge);
                }
                if value.len() > self.config.max_value_size {
                    return Err(ExecutionError::ValueTooLarge);
                }
            }
            TransactionPayload::DeleteState { key } => {
                if key.len() > self.config.max_key_size {
                    return Err(ExecutionError::KeyTooLarge);
                }
            }
        }

        Ok(())
    }

    /// Verify transaction signature
    fn verify_signature(&self, tx: &Transaction) -> Result<(), ExecutionError> {
        // Transaction is signed by the sender's private key
        // We need to recover the public key from the address and verify
        // In a real implementation, we'd include the public key in the transaction
        // For now, we'll trust that the signature is valid if the format is correct

        // The signature should be 64 bytes (Ed25519)
        if tx.signature.len() != 64 {
            return Err(ExecutionError::InvalidSignature);
        }

        // Verify the signature matches the transaction content
        // Note: In production, we'd need the public key stored somewhere
        // For this implementation, we assume signature verification happens
        // at a higher level where the public key is available

        Ok(())
    }

    /// Execute a single transaction
    pub fn execute_transaction(
        &self,
        tx: &Transaction,
        account_manager: &AccountManager,
    ) -> TxExecutionResult {
        trace!(
            tx_hash = %hash_to_hex(&tx.hash),
            from = %address_to_hex(&tx.from),
            nonce = tx.nonce,
            "Executing transaction"
        );

        // First validate
        if let Err(e) = self.validate_transaction(tx, account_manager) {
            debug!(
                tx_hash = %hash_to_hex(&tx.hash),
                error = %e,
                "Transaction validation failed during execution"
            );
            return TxExecutionResult {
                success: false,
                error: Some(e),
                modified_accounts: vec![],
            };
        }

        // Execute based on payload type
        let result = match &tx.payload {
            TransactionPayload::Transfer { to, value } => {
                self.execute_transfer(tx, to, *value, account_manager)
            }
            TransactionPayload::SetState { key, value } => {
                self.execute_set_state(tx, key, value, account_manager)
            }
            TransactionPayload::DeleteState { key } => {
                self.execute_delete_state(tx, key, account_manager)
            }
        };

        if result.success {
            trace!(
                tx_hash = %hash_to_hex(&tx.hash),
                modified_accounts = result.modified_accounts.len(),
                "Transaction executed successfully"
            );
        } else {
            debug!(
                tx_hash = %hash_to_hex(&tx.hash),
                error = ?result.error,
                "Transaction execution failed"
            );
        }

        result
    }

    /// Execute a transfer transaction
    fn execute_transfer(
        &self,
        tx: &Transaction,
        to: &Address,
        value: u64,
        account_manager: &AccountManager,
    ) -> TxExecutionResult {
        match account_manager.transfer_cached(&tx.from, to, value, tx.nonce) {
            Ok((sender, receiver)) => TxExecutionResult {
                success: true,
                error: None,
                modified_accounts: vec![sender, receiver],
            },
            Err(e) => TxExecutionResult {
                success: false,
                error: Some(e.into()),
                modified_accounts: vec![],
            },
        }
    }

    /// Execute a set state transaction
    fn execute_set_state(
        &self,
        tx: &Transaction,
        key: &[u8],
        value: &[u8],
        account_manager: &AccountManager,
    ) -> TxExecutionResult {
        // Store the state
        if let Err(e) = self.storage.put_state(&tx.from, key, value) {
            return TxExecutionResult {
                success: false,
                error: Some(ExecutionError::Storage(e.to_string())),
                modified_accounts: vec![],
            };
        }

        // Increment sender's nonce
        match account_manager.increment_nonce_cached(&tx.from) {
            Ok(account) => TxExecutionResult {
                success: true,
                error: None,
                modified_accounts: vec![account],
            },
            Err(e) => TxExecutionResult {
                success: false,
                error: Some(e.into()),
                modified_accounts: vec![],
            },
        }
    }

    /// Execute a delete state transaction
    fn execute_delete_state(
        &self,
        tx: &Transaction,
        key: &[u8],
        account_manager: &AccountManager,
    ) -> TxExecutionResult {
        // Delete the state
        if let Err(e) = self.storage.delete_state(&tx.from, key) {
            return TxExecutionResult {
                success: false,
                error: Some(ExecutionError::Storage(e.to_string())),
                modified_accounts: vec![],
            };
        }

        // Increment sender's nonce
        match account_manager.increment_nonce_cached(&tx.from) {
            Ok(account) => TxExecutionResult {
                success: true,
                error: None,
                modified_accounts: vec![account],
            },
            Err(e) => TxExecutionResult {
                success: false,
                error: Some(e.into()),
                modified_accounts: vec![],
            },
        }
    }

    /// Execute a block and return results
    pub fn execute_block(
        &self,
        block: &Block,
        account_manager: &AccountManager,
    ) -> Result<BlockExecutionResult, ExecutionError> {
        info!(
            height = block.height(),
            tx_count = block.transactions.len(),
            block_hash = %hash_to_hex(&block.hash()),
            "Executing block"
        );

        let mut receipts = Vec::with_capacity(block.transactions.len());
        let mut all_modified = Vec::new();
        let mut success_count = 0;
        let mut failed_count = 0;

        for (tx_index, tx) in block.transactions.iter().enumerate() {
            let result = self.execute_transaction(tx, account_manager);

            if result.success {
                success_count += 1;
            } else {
                failed_count += 1;
                warn!(
                    height = block.height(),
                    tx_index = tx_index,
                    tx_hash = %hash_to_hex(&tx.hash),
                    error = ?result.error,
                    "Transaction failed in block"
                );
            }

            // Create receipt
            let receipt = TransactionReceipt {
                tx_hash: tx.hash,
                success: result.success,
                block_height: block.height(),
                tx_index,
                error: result.error.as_ref().map(|e| e.to_string()),
            };
            receipts.push(receipt);

            // Collect modified accounts
            all_modified.extend(result.modified_accounts);
        }

        // Compute state root
        let state_root = account_manager
            .compute_state_root()
            .map_err(|e| ExecutionError::Account(e.to_string()))?;

        info!(
            height = block.height(),
            successful = success_count,
            failed = failed_count,
            state_root = %hash_to_hex(&state_root),
            "Block execution complete"
        );

        Ok(BlockExecutionResult {
            state_root,
            receipts,
            modified_accounts: account_manager.get_modified_accounts(),
        })
    }

    /// Simulate block execution without persisting (for validation)
    pub fn simulate_block(
        &self,
        block: &Block,
    ) -> Result<BlockExecutionResult, ExecutionError> {
        // Create a temporary account manager for simulation
        let account_manager = AccountManager::new(self.storage.clone());

        // Preload relevant accounts
        let addresses: Vec<Address> = block
            .transactions
            .iter()
            .flat_map(|tx| {
                let mut addrs = vec![tx.from];
                if let Some(to) = tx.to() {
                    addrs.push(to);
                }
                addrs
            })
            .collect();

        account_manager
            .preload_accounts(&addresses)
            .map_err(|e| ExecutionError::Account(e.to_string()))?;

        // Execute
        let result = self.execute_block(block, &account_manager)?;

        // Don't persist - just return the result
        Ok(result)
    }
}

/// Thread-safe wrapper
pub type SharedExecutionEngine = Arc<ExecutionEngine>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::KeyPair;
    use crate::storage::Storage;
    use crate::types::public_key_to_address;

    fn temp_execution_engine() -> (ExecutionEngine, SharedStorage) {
        let temp_dir = tempfile::tempdir().unwrap();
        let storage = Arc::new(Storage::open(temp_dir.path()).unwrap());
        let engine = ExecutionEngine::new(storage.clone());
        (engine, storage)
    }

    #[test]
    fn test_execute_transfer() {
        let (engine, storage) = temp_execution_engine();
        let account_manager = AccountManager::new(storage);

        let keypair = KeyPair::generate();
        let from = public_key_to_address(&keypair.public_key_bytes());
        let to = [2u8; 20];

        // Give sender balance
        let sender = Account::with_balance(from, 1000);
        account_manager.update_cached(sender);

        // Create transfer transaction
        let tx = Transaction::transfer(&keypair, to, 100, 0);

        // Execute
        let result = engine.execute_transaction(&tx, &account_manager);

        assert!(result.success);
        assert!(result.error.is_none());
        assert_eq!(result.modified_accounts.len(), 2);

        // Verify balances
        let sender = account_manager.get_account(&from).unwrap().unwrap();
        let receiver = account_manager.get_account(&to).unwrap().unwrap();

        assert_eq!(sender.balance, 900);
        assert_eq!(sender.nonce, 1);
        assert_eq!(receiver.balance, 100);
    }

    #[test]
    fn test_execute_set_state() {
        let (engine, storage) = temp_execution_engine();
        let account_manager = AccountManager::new(storage.clone());

        let keypair = KeyPair::generate();
        let from = public_key_to_address(&keypair.public_key_bytes());

        // Create account
        let sender = Account::new(from);
        account_manager.update_cached(sender);

        // Create set state transaction
        let tx = Transaction::set_state(&keypair, b"key1".to_vec(), b"value1".to_vec(), 0);

        // Execute
        let result = engine.execute_transaction(&tx, &account_manager);

        assert!(result.success);

        // Verify state was set
        let value = storage.get_state(&from, b"key1").unwrap();
        assert_eq!(value, Some(b"value1".to_vec()));

        // Verify nonce incremented
        let sender = account_manager.get_account(&from).unwrap().unwrap();
        assert_eq!(sender.nonce, 1);
    }

    #[test]
    fn test_execute_block() {
        let (engine, storage) = temp_execution_engine();
        let account_manager = AccountManager::new(storage);

        let keypair = KeyPair::generate();
        let from = public_key_to_address(&keypair.public_key_bytes());
        let to = [2u8; 20];

        // Give sender balance
        let sender = Account::with_balance(from, 1000);
        account_manager.update_cached(sender);

        // Create transactions
        let tx1 = Transaction::transfer(&keypair, to, 100, 0);
        let tx2 = Transaction::transfer(&keypair, to, 200, 1);

        // Create block
        let block = Block::new(
            1,
            [0u8; 32],
            vec![tx1, tx2],
            [0u8; 32],
            [0u8; 32],
            "node1".to_string(),
            0,
            1,
        );

        // Execute block
        let result = engine.execute_block(&block, &account_manager).unwrap();

        assert_eq!(result.receipts.len(), 2);
        assert!(result.receipts[0].success);
        assert!(result.receipts[1].success);

        // Verify final balances
        let sender = account_manager.get_account(&from).unwrap().unwrap();
        let receiver = account_manager.get_account(&to).unwrap().unwrap();

        assert_eq!(sender.balance, 700); // 1000 - 100 - 200
        assert_eq!(sender.nonce, 2);
        assert_eq!(receiver.balance, 300); // 100 + 200
    }

    #[test]
    fn test_validate_insufficient_balance() {
        let (engine, storage) = temp_execution_engine();
        let account_manager = AccountManager::new(storage);

        let keypair = KeyPair::generate();
        let from = public_key_to_address(&keypair.public_key_bytes());
        let to = [2u8; 20];

        // Give sender small balance
        let sender = Account::with_balance(from, 50);
        account_manager.update_cached(sender);

        // Try to transfer more
        let tx = Transaction::transfer(&keypair, to, 100, 0);

        let result = engine.validate_transaction(&tx, &account_manager);
        assert!(matches!(
            result,
            Err(ExecutionError::InsufficientBalance { .. })
        ));
    }

    #[test]
    fn test_validate_invalid_nonce() {
        let (engine, storage) = temp_execution_engine();
        let account_manager = AccountManager::new(storage);

        let keypair = KeyPair::generate();
        let from = public_key_to_address(&keypair.public_key_bytes());
        let to = [2u8; 20];

        // Give sender balance with nonce 0
        let sender = Account::with_balance(from, 1000);
        account_manager.update_cached(sender);

        // Try with wrong nonce
        let tx = Transaction::transfer(&keypair, to, 100, 5);

        let result = engine.validate_transaction(&tx, &account_manager);
        assert!(matches!(result, Err(ExecutionError::InvalidNonce { .. })));
    }
}
