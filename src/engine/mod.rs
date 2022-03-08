use std::{collections::hash_map::Iter, fmt};

use crate::{
    models::{AccountData, TransactionType, TxRow},
    stores::{AccountStore, DataError, LockedAccountStore, TransactionStore},
};

#[derive(Debug)]
pub struct Engine {
    account_store: AccountStore,
    tx_store: TransactionStore,
    locked_accounts_store: LockedAccountStore,
}

impl Engine {
    pub fn new() -> Self {
        Self {
            account_store: AccountStore::new(),
            tx_store: TransactionStore::new(),
            locked_accounts_store: LockedAccountStore::new(),
        }
    }

    pub fn process_row(&mut self, row: &TxRow) -> Result<(), ProcessingError> {
        if self.locked_accounts_store.is_account_locked(&row.client_id) {
            return Err(ProcessingError::AccountLocked(row.client_id));
        }
        match (row.tx_type, row.amount) {
            (TransactionType::Deposit, None) => Err(ProcessingError::AmountNotSpecified(row.tx_id)),
            (TransactionType::Withdrawal, None) => {
                Err(ProcessingError::AmountNotSpecified(row.tx_id))
            }
            (TransactionType::Deposit, Some(amount)) => {
                self.process_deposit(row.tx_id, row.client_id, amount)
            }
            (TransactionType::Withdrawal, Some(amount)) => {
                self.process_withdrawal(row.tx_id, row.client_id, amount)
            }
            (TransactionType::Resolve, _) => self.process_resolve(row.tx_id, row.client_id),
            (TransactionType::Dispute, _) => self.process_dispute(row.tx_id, row.client_id),
            (TransactionType::Chargeback, _) => self.process_chargeback(row.tx_id, row.client_id),
        }
    }

    fn process_deposit(
        &mut self,
        tx_id: u32,
        client_id: u16,
        amount: f64,
    ) -> Result<(), ProcessingError> {
        self.tx_store
            .insert_tx(tx_id, amount)
            .map_err(|e| match e {
                DataError::AlreadyExists => ProcessingError::DuplicateTx(tx_id),
            })
            .and_then(|_| {
                let mut account =
                    self.account_store
                        .find_by_id(&client_id)
                        .unwrap_or(AccountData {
                            available: 0.0,
                            held: 0.0,
                        });
                account.available += amount;
                self.account_store
                    .add_or_update_account(&client_id, &account);
                Ok(())
            })
    }

    fn process_withdrawal(
        &mut self,
        tx_id: u32,
        client_id: u16,
        amount: f64,
    ) -> Result<(), ProcessingError> {
        self.tx_store
            .insert_tx(tx_id, amount)
            .map_err(|e| match e {
                DataError::AlreadyExists => ProcessingError::DuplicateTx(tx_id),
            })
            .and_then(|_| match self.account_store.find_by_id(&client_id) {
                None => Err(ProcessingError::AccountNotFound(client_id)),
                Some(mut account) => {
                    if account.available < amount {
                        Err(ProcessingError::InsufficientFunds(client_id))
                    } else {
                        account.available -= amount;
                        self.account_store
                            .add_or_update_account(&client_id, &account);
                        Ok(())
                    }
                }
            })
    }

    fn process_resolve(&mut self, tx_id: u32, client_id: u16) -> Result<(), ProcessingError> {
        match self.tx_store.find_by_id(&tx_id) {
            None => Err(ProcessingError::TxNotDisputed(tx_id)),
            Some(tx) => match tx.disputed {
                false => Err(ProcessingError::TxNotDisputed(tx_id)),
                true => match self.account_store.find_by_id(&client_id) {
                    None => Err(ProcessingError::AccountNotFound(client_id)),
                    Some(mut data) => {
                        data.held -= tx.amount;
                        data.available += tx.amount;
                        self.account_store.add_or_update_account(&client_id, &data);
                        self.tx_store.resolve_transaction(&tx_id);
                        Ok(())
                    }
                },
            },
        }
    }

    fn process_dispute(&mut self, tx_id: u32, client_id: u16) -> Result<(), ProcessingError> {
        match self.tx_store.find_by_id(&tx_id) {
            None => Err(ProcessingError::TxNotFound(tx_id)),
            Some(tx) => match tx.disputed {
                true => Err(ProcessingError::TxAlreadyDisputed(tx_id)),
                false => {
                    match self.account_store.find_by_id(&client_id) {
                        None => Err(ProcessingError::AccountNotFound(client_id)),
                        Some(mut data) => {
                            data.held += tx.amount;
                            data.available -= tx.amount;
                            self.account_store.add_or_update_account(&client_id, &data);
                            self.tx_store.dispute_transaction(tx_id);
                            Ok(())
                        }
                    }
                }
            },
        }
    }

    fn process_chargeback(&mut self, tx_id: u32, client_id: u16) -> Result<(), ProcessingError> {
        match self.tx_store.find_by_id(&tx_id) {
            None => Err(ProcessingError::TxNotFound(tx_id)),
            Some(tx) => match tx.disputed {
                false => Err(ProcessingError::TxNotDisputed(tx_id)),
                true => {
                    match self.account_store.find_by_id(&client_id) {
                        None => Err(ProcessingError::AccountNotFound(client_id)),
                        Some(mut data) => {
                            data.held -= tx.amount;
                            self.account_store.add_or_update_account(&client_id, &data);
                            self.locked_accounts_store.lock_account(client_id);
                            Ok(())
                        }
                    }
                }
            },
        }
    }

    pub fn get_account_iter(&self) -> Iter<u16, AccountData> {
        self.account_store.find_all()
    }

    pub fn is_account_locked(&self, id: u16) -> bool {
        self.locked_accounts_store.is_account_locked(&id)
    }
}

#[derive(Debug)]
pub enum ProcessingError {
    // Unknown,
    AccountNotFound(u16),
    AccountLocked(u16),
    InsufficientFunds(u16),
    DuplicateTx(u32),
    TxAlreadyDisputed(u32),
    TxNotFound(u32),
    TxNotDisputed(u32),
    AmountNotSpecified(u32),
}

impl fmt::Display for ProcessingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}
