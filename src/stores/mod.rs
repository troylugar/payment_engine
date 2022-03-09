use std::collections::{hash_map::Iter, HashMap, HashSet};

use rust_decimal::Decimal;

use crate::models::{AccountData, Transaction};

#[derive(Debug)]
pub struct TransactionStore {
    // maps tx_id to tx_amount
    transactions: HashMap<u32, Decimal>,
    // holds ids of disputed txs
    disputed_transactions: HashSet<u32>,
}

impl TransactionStore {
    pub fn new() -> Self {
        Self {
            transactions: HashMap::new(),
            disputed_transactions: HashSet::new(),
        }
    }

    pub fn find_by_id(&self, id: &u32) -> Option<Transaction> {
        match self.transactions.contains_key(id) {
            true => Some(Transaction {
                amount: self.transactions[id],
                disputed: self.disputed_transactions.contains(id),
            }),
            false => None,
        }
    }

    pub fn insert_tx(&mut self, id: u32, amount: Decimal) -> Result<(), DataError> {
        match self.transactions.contains_key(&id) {
            true => Err(DataError::AlreadyExists),
            false => {
                self.transactions.insert(id, amount);
                log::info!("inserted tx (id: {}, amount: {})", id, amount);
                Ok(())
            }
        }
    }

    pub fn dispute_transaction(&mut self, id: u32) {
        self.disputed_transactions.insert(id);
        log::info!("disputed tx_id {}", id);
    }

    pub fn resolve_transaction(&mut self, id: &u32) {
        if self.disputed_transactions.contains(id) {
            self.disputed_transactions.remove(id);
            log::info!("resolved tx_id {}", id)
        }
    }
}

#[derive(Debug)]
pub struct AccountStore {
    // maps client_id to account data
    accounts: HashMap<u16, AccountData>,
}

impl AccountStore {
    pub fn new() -> Self {
        Self {
            accounts: HashMap::new(),
        }
    }

    pub fn find_by_id(&mut self, id: &u16) -> Option<AccountData> {
        self.accounts.get(id).map(|x| *x)
    }

    pub fn add_or_update_account(&mut self, id: &u16, data: &AccountData) {
        self.accounts.insert(*id, *data);
        log::info!("saved account (id: {}, data: {:?})", id, data);
    }

    pub fn find_all(&self) -> Iter<u16, AccountData> {
        self.accounts.iter()
    }
}

#[derive(Debug)]
pub struct LockedAccountStore {
    locked_accounts: HashSet<u16>,
}

impl LockedAccountStore {
    pub fn new() -> Self {
        Self {
            locked_accounts: HashSet::new(),
        }
    }

    pub fn lock_account(&mut self, id: u16) {
        self.locked_accounts.insert(id);
        log::info!("locked account {}", id);
    }

    pub fn is_account_locked(&self, id: &u16) -> bool {
        self.locked_accounts.contains(id)
    }
}

pub enum DataError {
    AlreadyExists,
}
