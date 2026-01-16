use crate::message::{compute_hash, Hash};
use crate::storage::{SharedStorage, StorageError};
use crate::types::{Account, Address};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum AccountError {
    #[error("Insufficient balance: have {have}, need {need}")]
    InsufficientBalance { have: u64, need: u64 },
    #[error("Invalid nonce: expected {expected}, got {got}")]
    InvalidNonce { expected: u64, got: u64 },
    #[error("Account not found: {0}")]
    NotFound(String),
    #[error("Storage error: {0}")]
    Storage(#[from] StorageError),
}

/// Account state manager with caching
pub struct AccountManager {
    storage: SharedStorage,
    /// In-memory cache of accounts (for pending state during block execution)
    cache: RwLock<HashMap<Address, Account>>,
}

impl AccountManager {
    pub fn new(storage: SharedStorage) -> Self {
        Self {
            storage,
            cache: RwLock::new(HashMap::new()),
        }
    }

    /// Get account, checking cache first, then storage
    pub fn get_account(&self, address: &Address) -> Result<Option<Account>, AccountError> {
        // Check cache first
        {
            let cache = self.cache.read();
            if let Some(account) = cache.get(address) {
                return Ok(Some(account.clone()));
            }
        }

        // Fall back to storage
        Ok(self.storage.get_account(address)?)
    }

    /// Get account or create a new one with zero balance
    pub fn get_or_create_account(&self, address: &Address) -> Result<Account, AccountError> {
        match self.get_account(address)? {
            Some(account) => Ok(account),
            None => Ok(Account::new(*address)),
        }
    }

    /// Get account balance
    pub fn get_balance(&self, address: &Address) -> Result<u64, AccountError> {
        Ok(self.get_or_create_account(address)?.balance)
    }

    /// Get account nonce
    pub fn get_nonce(&self, address: &Address) -> Result<u64, AccountError> {
        Ok(self.get_or_create_account(address)?.nonce)
    }

    /// Validate a transfer operation
    pub fn validate_transfer(
        &self,
        from: &Address,
        value: u64,
        nonce: u64,
    ) -> Result<(), AccountError> {
        let account = self.get_or_create_account(from)?;

        // Check nonce
        if nonce != account.nonce {
            return Err(AccountError::InvalidNonce {
                expected: account.nonce,
                got: nonce,
            });
        }

        // Check balance
        if account.balance < value {
            return Err(AccountError::InsufficientBalance {
                have: account.balance,
                need: value,
            });
        }

        Ok(())
    }

    /// Execute a transfer in cache (doesn't persist)
    pub fn transfer_cached(
        &self,
        from: &Address,
        to: &Address,
        value: u64,
        nonce: u64,
    ) -> Result<(Account, Account), AccountError> {
        // Validate
        self.validate_transfer(from, value, nonce)?;

        // Get accounts
        let mut sender = self.get_or_create_account(from)?;
        let mut receiver = self.get_or_create_account(to)?;

        // Apply transfer
        sender.balance -= value;
        sender.nonce += 1;
        receiver.balance += value;

        // Update cache
        {
            let mut cache = self.cache.write();
            cache.insert(*from, sender.clone());
            cache.insert(*to, receiver.clone());
        }

        Ok((sender, receiver))
    }

    /// Update account in cache
    pub fn update_cached(&self, account: Account) {
        let mut cache = self.cache.write();
        cache.insert(account.address, account);
    }

    /// Increment nonce in cache
    pub fn increment_nonce_cached(&self, address: &Address) -> Result<Account, AccountError> {
        let mut account = self.get_or_create_account(address)?;
        account.nonce += 1;

        let mut cache = self.cache.write();
        cache.insert(*address, account.clone());

        Ok(account)
    }

    /// Get all modified accounts from cache
    pub fn get_modified_accounts(&self) -> Vec<Account> {
        let cache = self.cache.read();
        cache.values().cloned().collect()
    }

    /// Clear the cache (after persisting or rolling back)
    pub fn clear_cache(&self) {
        let mut cache = self.cache.write();
        cache.clear();
    }

    /// Persist all cached accounts to storage
    pub fn persist_cache(&self) -> Result<(), AccountError> {
        let cache = self.cache.read();
        for account in cache.values() {
            self.storage.put_account(account)?;
        }
        Ok(())
    }

    /// Load accounts into cache (for simulating execution)
    pub fn preload_accounts(&self, addresses: &[Address]) -> Result<(), AccountError> {
        let mut cache = self.cache.write();
        for address in addresses {
            if !cache.contains_key(address) {
                if let Some(account) = self.storage.get_account(address)? {
                    cache.insert(*address, account);
                }
            }
        }
        Ok(())
    }

    /// Compute state root from all accounts
    /// This is a simple implementation - a production system would use a Merkle Patricia Trie
    pub fn compute_state_root(&self) -> Result<Hash, AccountError> {
        // Get all accounts from cache
        let cache = self.cache.read();

        if cache.is_empty() {
            return Ok([0u8; 32]);
        }

        // Sort by address for deterministic ordering
        let mut accounts: Vec<_> = cache.values().collect();
        accounts.sort_by_key(|a| a.address);

        // Hash all accounts together
        let mut data = Vec::new();
        for account in accounts {
            data.extend_from_slice(&account.address);
            data.extend_from_slice(&account.balance.to_le_bytes());
            data.extend_from_slice(&account.nonce.to_le_bytes());
        }

        Ok(compute_hash(&data))
    }
}

/// Thread-safe wrapper
pub type SharedAccountManager = Arc<AccountManager>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::Storage;
    use std::sync::Arc;

    fn temp_account_manager() -> AccountManager {
        let temp_dir = tempfile::tempdir().unwrap();
        let storage = Arc::new(Storage::open(temp_dir.path()).unwrap());
        AccountManager::new(storage)
    }

    #[test]
    fn test_get_nonexistent_account() {
        let manager = temp_account_manager();
        let address = [1u8; 20];

        let account = manager.get_or_create_account(&address).unwrap();
        assert_eq!(account.balance, 0);
        assert_eq!(account.nonce, 0);
    }

    #[test]
    fn test_transfer_cached() {
        let manager = temp_account_manager();
        let from = [1u8; 20];
        let to = [2u8; 20];

        // Give sender some balance
        let sender = Account::with_balance(from, 1000);
        manager.update_cached(sender);

        // Execute transfer
        let (new_sender, new_receiver) = manager.transfer_cached(&from, &to, 100, 0).unwrap();

        assert_eq!(new_sender.balance, 900);
        assert_eq!(new_sender.nonce, 1);
        assert_eq!(new_receiver.balance, 100);
    }

    #[test]
    fn test_transfer_insufficient_balance() {
        let manager = temp_account_manager();
        let from = [1u8; 20];
        let to = [2u8; 20];

        // Give sender small balance
        let sender = Account::with_balance(from, 50);
        manager.update_cached(sender);

        // Try to transfer more than available
        let result = manager.transfer_cached(&from, &to, 100, 0);
        assert!(matches!(result, Err(AccountError::InsufficientBalance { .. })));
    }

    #[test]
    fn test_transfer_invalid_nonce() {
        let manager = temp_account_manager();
        let from = [1u8; 20];
        let to = [2u8; 20];

        // Give sender balance with nonce 0
        let sender = Account::with_balance(from, 1000);
        manager.update_cached(sender);

        // Try with wrong nonce
        let result = manager.transfer_cached(&from, &to, 100, 5);
        assert!(matches!(result, Err(AccountError::InvalidNonce { .. })));
    }

    #[test]
    fn test_modified_accounts() {
        let manager = temp_account_manager();

        let acc1 = Account::with_balance([1u8; 20], 100);
        let acc2 = Account::with_balance([2u8; 20], 200);

        manager.update_cached(acc1.clone());
        manager.update_cached(acc2.clone());

        let modified = manager.get_modified_accounts();
        assert_eq!(modified.len(), 2);
    }

    #[test]
    fn test_clear_cache() {
        let manager = temp_account_manager();

        let acc = Account::with_balance([1u8; 20], 100);
        manager.update_cached(acc);

        assert_eq!(manager.get_modified_accounts().len(), 1);

        manager.clear_cache();

        assert_eq!(manager.get_modified_accounts().len(), 0);
    }

    #[test]
    fn test_compute_state_root() {
        let manager = temp_account_manager();

        let acc1 = Account::with_balance([1u8; 20], 100);
        let acc2 = Account::with_balance([2u8; 20], 200);

        manager.update_cached(acc1);
        manager.update_cached(acc2);

        let root1 = manager.compute_state_root().unwrap();

        // Same accounts should produce same root
        let root2 = manager.compute_state_root().unwrap();
        assert_eq!(root1, root2);

        // Different accounts should produce different root
        manager.clear_cache();
        let acc3 = Account::with_balance([3u8; 20], 300);
        manager.update_cached(acc3);

        let root3 = manager.compute_state_root().unwrap();
        assert_ne!(root1, root3);
    }
}
