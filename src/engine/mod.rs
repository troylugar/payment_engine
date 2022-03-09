use std::{collections::hash_map::Iter, fmt};

use rust_decimal::Decimal;

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
        amount: Decimal,
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
                            available: Decimal::ZERO,
                            held: Decimal::ZERO,
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
        amount: Decimal,
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

    fn process_dispute(&mut self, tx_id: u32, client_id: u16) -> Result<(), ProcessingError> {
        match self.tx_store.find_by_id(&tx_id) {
            None => Err(ProcessingError::TxNotFound(tx_id)),
            Some(tx) => match tx.disputed {
                true => Err(ProcessingError::TxAlreadyDisputed(tx_id)),
                false => match self.account_store.find_by_id(&client_id) {
                    None => Err(ProcessingError::AccountNotFound(client_id)),
                    Some(mut data) => {
                        data.held += tx.amount;
                        data.available -= tx.amount;
                        self.account_store.add_or_update_account(&client_id, &data);
                        self.tx_store.dispute_transaction(tx_id);
                        Ok(())
                    }
                },
            },
        }
    }

    fn process_resolve(&mut self, tx_id: u32, client_id: u16) -> Result<(), ProcessingError> {
        match self.tx_store.find_by_id(&tx_id) {
            None => Err(ProcessingError::TxNotFound(tx_id)),
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

    fn process_chargeback(&mut self, tx_id: u32, client_id: u16) -> Result<(), ProcessingError> {
        match self.tx_store.find_by_id(&tx_id) {
            None => Err(ProcessingError::TxNotFound(tx_id)),
            Some(tx) => match tx.disputed {
                false => Err(ProcessingError::TxNotDisputed(tx_id)),
                true => match self.account_store.find_by_id(&client_id) {
                    None => Err(ProcessingError::AccountNotFound(client_id)),
                    Some(mut data) => {
                        data.held -= tx.amount;
                        self.account_store.add_or_update_account(&client_id, &data);
                        self.locked_accounts_store.lock_account(client_id);
                        Ok(())
                    }
                },
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

#[derive(Debug, PartialEq, Eq)]
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

#[cfg(test)]
mod tests {
    use super::Engine;

    mod deposits {

        use rust_decimal_macros::dec;

        use crate::{
            engine::ProcessingError,
            models::{TransactionType, TxRow},
        };

        use super::Engine;

        #[test]
        fn should_process_deposit() {
            let row = TxRow {
                tx_type: TransactionType::Deposit,
                tx_id: 1,
                client_id: 2,
                amount: Some(dec!(123.45)),
            };
            let mut engine = Engine::new();
            engine.process_row(&row).unwrap();
            if let Some((acc_id, acc)) = engine.get_account_iter().next() {
                assert_eq!(*acc_id, 2u16);
                assert_eq!(acc.available, dec!(123.45));
            } else {
                panic!("account not found");
            }
        }

        #[test]
        fn should_not_process_deposit_without_amount() {
            let row = TxRow {
                tx_type: TransactionType::Deposit,
                tx_id: 1,
                client_id: 2,
                amount: None,
            };
            let mut engine = Engine::new();
            let err = engine.process_row(&row).unwrap_err();
            assert_eq!(err, ProcessingError::AmountNotSpecified(1u32));
        }

        #[test]
        fn should_not_process_duplicate_deposit() {
            let row = TxRow {
                tx_type: TransactionType::Deposit,
                tx_id: 1,
                client_id: 2,
                amount: Some(dec!(123.45)),
            };
            let row2 = row.clone();
            let mut engine = Engine::new();
            engine.process_row(&row).unwrap();
            let err = engine.process_row(&row2).unwrap_err();
            assert_eq!(err, ProcessingError::DuplicateTx(1u32));
        }
    }

    mod withdrawals {

        use rust_decimal_macros::dec;

        use crate::{
            engine::ProcessingError,
            models::{TransactionType, TxRow},
        };

        use super::Engine;

        #[test]
        fn should_process_withdrawal() {
            let deposit = TxRow {
                tx_type: TransactionType::Deposit,
                tx_id: 1,
                client_id: 2,
                amount: Some(dec!(123.45)),
            };
            let withdrawal = TxRow {
                tx_type: TransactionType::Withdrawal,
                tx_id: 2,
                client_id: deposit.client_id,
                amount: Some(dec!(120.00)),
            };
            let mut engine = Engine::new();
            engine.process_row(&deposit).unwrap();
            engine.process_row(&withdrawal).unwrap();
            let (acc_id, acc) = engine.get_account_iter().next().unwrap();
            assert_eq!(*acc_id, 2u16);
            assert_eq!(
                acc.available,
                deposit.amount.unwrap() - withdrawal.amount.unwrap()
            );
        }

        #[test]
        fn should_not_process_overdraft_withdrawal() {
            let deposit = TxRow {
                tx_type: TransactionType::Deposit,
                tx_id: 1,
                client_id: 2,
                amount: Some(dec!(123.45)),
            };
            let withdrawal = TxRow {
                tx_type: TransactionType::Withdrawal,
                tx_id: 2,
                client_id: deposit.client_id,
                amount: Some(dec!(125.00)),
            };
            let mut engine = Engine::new();
            engine.process_row(&deposit).unwrap();
            let err = engine.process_row(&withdrawal).unwrap_err();
            assert_eq!(err, ProcessingError::InsufficientFunds(2u16));
            let (acc_id, acc) = engine.get_account_iter().next().unwrap();
            assert_eq!(*acc_id, 2u16);
            assert_eq!(acc.available, deposit.amount.unwrap());
        }

        #[test]
        fn should_not_process_withdrawal_when_account_not_found() {
            let withdrawal = TxRow {
                tx_type: TransactionType::Withdrawal,
                tx_id: 1,
                client_id: 2,
                amount: Some(dec!(125.00)),
            };
            let mut engine = Engine::new();
            let err = engine.process_row(&withdrawal).unwrap_err();
            assert_eq!(err, ProcessingError::AccountNotFound(2u16));
            assert!(
                engine.get_account_iter().next().is_none(),
                "account should not exist"
            );
        }

        #[test]
        fn should_not_process_duplicate_withdrawal() {
            let deposit = TxRow {
                tx_type: TransactionType::Deposit,
                tx_id: 1,
                client_id: 2,
                amount: Some(dec!(123.45)),
            };
            let withdrawal1 = TxRow {
                tx_type: TransactionType::Withdrawal,
                tx_id: 2,
                client_id: deposit.client_id,
                amount: Some(dec!(123.45)),
            };
            let withdrawal2 = withdrawal1.clone();
            let mut engine = Engine::new();
            engine.process_row(&deposit).unwrap();
            engine.process_row(&withdrawal1).unwrap();
            let err = engine.process_row(&withdrawal2).unwrap_err();
            assert_eq!(err, ProcessingError::DuplicateTx(2u32));
        }
    }

    mod disputes {
        use rust_decimal_macros::dec;

        use crate::{
            engine::ProcessingError,
            models::{TransactionType, TxRow},
        };

        use super::Engine;

        #[test]
        fn should_process_dispute() {
            let deposit1 = TxRow {
                tx_type: TransactionType::Deposit,
                tx_id: 1,
                client_id: 2,
                amount: Some(dec!(123.45)),
            };
            let deposit2 = TxRow {
                tx_type: TransactionType::Deposit,
                tx_id: 2,
                client_id: deposit1.client_id,
                amount: Some(dec!(100.00)),
            };
            let dispute = TxRow {
                tx_type: TransactionType::Dispute,
                tx_id: deposit2.tx_id,
                client_id: deposit2.client_id,
                amount: None,
            };
            let mut engine = Engine::new();
            engine.process_row(&deposit1).unwrap();
            engine.process_row(&deposit2).unwrap();
            engine.process_row(&dispute).unwrap();
            if let Some((acc_id, acc)) = engine.get_account_iter().next() {
                assert_eq!(*acc_id, 2u16);
                assert_eq!(acc.available, deposit1.amount.unwrap());
                assert_eq!(acc.held, deposit2.amount.unwrap());
            } else {
                panic!("account not found");
            }
        }

        #[test]
        fn should_not_process_duplicate_dispute() {
            let deposit1 = TxRow {
                tx_type: TransactionType::Deposit,
                tx_id: 1,
                client_id: 2,
                amount: Some(dec!(123.45)),
            };
            let deposit2 = TxRow {
                tx_type: TransactionType::Deposit,
                tx_id: 2,
                client_id: deposit1.client_id,
                amount: Some(dec!(100.00)),
            };
            let dispute1 = TxRow {
                tx_type: TransactionType::Dispute,
                tx_id: deposit2.tx_id,
                client_id: deposit2.client_id,
                amount: None,
            };
            let dispute2 = dispute1.clone();
            let mut engine = Engine::new();
            engine.process_row(&deposit1).unwrap();
            engine.process_row(&deposit2).unwrap();
            engine.process_row(&dispute1).unwrap();
            let err = engine.process_row(&dispute2).unwrap_err();
            assert_eq!(err, ProcessingError::TxAlreadyDisputed(dispute2.tx_id));
        }

        #[test]
        fn should_not_process_dispute_when_tx_not_found() {
            let deposit1 = TxRow {
                tx_type: TransactionType::Deposit,
                tx_id: 1,
                client_id: 2,
                amount: Some(dec!(123.45)),
            };
            let deposit2 = TxRow {
                tx_type: TransactionType::Deposit,
                tx_id: 2,
                client_id: deposit1.client_id,
                amount: Some(dec!(100.00)),
            };
            let dispute = TxRow {
                tx_type: TransactionType::Dispute,
                tx_id: 3,
                client_id: deposit2.client_id,
                amount: None,
            };
            let mut engine = Engine::new();
            engine.process_row(&deposit1).unwrap();
            engine.process_row(&deposit2).unwrap();
            let err = engine.process_row(&dispute).unwrap_err();
            assert_eq!(err, ProcessingError::TxNotFound(dispute.tx_id));
        }

        #[test]
        fn should_not_process_dispute_when_account_not_found() {
            let deposit1 = TxRow {
                tx_type: TransactionType::Deposit,
                tx_id: 1,
                client_id: 2,
                amount: Some(dec!(123.45)),
            };
            let deposit2 = TxRow {
                tx_type: TransactionType::Deposit,
                tx_id: 2,
                client_id: deposit1.client_id,
                amount: Some(dec!(100.00)),
            };
            let dispute = TxRow {
                tx_type: TransactionType::Dispute,
                tx_id: 2,
                client_id: 1,
                amount: None,
            };
            let mut engine = Engine::new();
            engine.process_row(&deposit1).unwrap();
            engine.process_row(&deposit2).unwrap();
            let err = engine.process_row(&dispute).unwrap_err();
            assert_eq!(err, ProcessingError::AccountNotFound(dispute.client_id));
        }
    }

    mod resolutions {
        use rust_decimal::Decimal;
        use rust_decimal_macros::dec;

        use crate::{
            engine::ProcessingError,
            models::{TransactionType, TxRow},
        };

        use super::Engine;

        #[test]
        fn should_process_resolve() {
            let deposit1 = TxRow {
                tx_type: TransactionType::Deposit,
                tx_id: 1,
                client_id: 2,
                amount: Some(dec!(123.45)),
            };
            let deposit2 = TxRow {
                tx_type: TransactionType::Deposit,
                tx_id: 2,
                client_id: deposit1.client_id,
                amount: Some(dec!(100.00)),
            };
            let dispute = TxRow {
                tx_type: TransactionType::Dispute,
                tx_id: deposit2.tx_id,
                client_id: deposit2.client_id,
                amount: None,
            };
            let resolve = TxRow {
                tx_type: TransactionType::Resolve,
                tx_id: dispute.tx_id,
                client_id: dispute.client_id,
                amount: None,
            };
            let mut engine = Engine::new();
            engine.process_row(&deposit1).unwrap();
            engine.process_row(&deposit2).unwrap();
            engine.process_row(&dispute).unwrap();
            engine.process_row(&resolve).unwrap();
            let (acc_id2, acc2) = engine.get_account_iter().next().unwrap();
            assert_eq!(*acc_id2, dispute.client_id);
            assert_eq!(
                acc2.available,
                deposit1.amount.unwrap() + deposit2.amount.unwrap()
            );
            assert_eq!(acc2.held, Decimal::ZERO);
        }

        #[test]
        fn should_not_process_resolution_for_undisputed_txs() {
            let deposit = TxRow {
                tx_type: TransactionType::Deposit,
                tx_id: 1,
                client_id: 2,
                amount: Some(dec!(123.45)),
            };
            let resolve = TxRow {
                tx_type: TransactionType::Resolve,
                tx_id: deposit.tx_id,
                client_id: deposit.client_id,
                amount: None,
            };
            let mut engine = Engine::new();
            engine.process_row(&deposit).unwrap();
            let err = engine.process_row(&resolve).unwrap_err();
            assert_eq!(err, ProcessingError::TxNotDisputed(resolve.tx_id));
        }

        #[test]
        fn should_not_process_resolution_for_non_existing_txs() {
            let deposit = TxRow {
                tx_type: TransactionType::Deposit,
                tx_id: 1,
                client_id: 2,
                amount: Some(dec!(123.45)),
            };
            let resolve = TxRow {
                tx_type: TransactionType::Resolve,
                tx_id: 2,
                client_id: deposit.client_id,
                amount: None,
            };
            let mut engine = Engine::new();
            engine.process_row(&deposit).unwrap();
            let err = engine.process_row(&resolve).unwrap_err();
            assert_eq!(err, ProcessingError::TxNotFound(resolve.tx_id));
        }
    }

    mod chargebacks {
        use rust_decimal::Decimal;
        use rust_decimal_macros::dec;

        use crate::{
            engine::{Engine, ProcessingError},
            models::{TransactionType, TxRow},
        };

        #[test]
        fn should_process_chargeback() {
            let deposit1 = TxRow {
                tx_type: TransactionType::Deposit,
                tx_id: 1,
                client_id: 2,
                amount: Some(dec!(100.00)),
            };
            let deposit2 = TxRow {
                tx_type: TransactionType::Deposit,
                tx_id: 2,
                client_id: 2,
                amount: Some(dec!(50.00)),
            };
            let dispute = TxRow {
                tx_type: TransactionType::Dispute,
                tx_id: deposit2.tx_id,
                client_id: deposit2.client_id,
                amount: None,
            };
            let chargeback = TxRow {
                tx_type: TransactionType::Chargeback,
                tx_id: dispute.tx_id,
                client_id: dispute.client_id,
                amount: None,
            };
            let mut engine = Engine::new();
            engine.process_row(&deposit1).unwrap();
            engine.process_row(&deposit2).unwrap();
            engine.process_row(&dispute).unwrap();
            engine.process_row(&chargeback).unwrap();
            let (acc_id, acc) = engine.get_account_iter().next().unwrap();
            assert_eq!(*acc_id, dispute.client_id);
            assert_eq!(acc.available, deposit1.amount.unwrap());
            assert_eq!(acc.held, Decimal::ZERO);
            assert!(engine.is_account_locked(*acc_id))
        }

        #[test]
        fn should_not_process_chargeback_for_non_existing_tx() {
            let chargeback = TxRow {
                tx_type: TransactionType::Chargeback,
                tx_id: 1,
                client_id: 2,
                amount: None,
            };
            let mut engine = Engine::new();
            let err = engine.process_row(&chargeback).unwrap_err();
            assert_eq!(err, ProcessingError::TxNotFound(chargeback.tx_id));
        }

        #[test]
        fn should_not_process_chargeback_for_undisputed_tx() {
            let deposit = TxRow {
                tx_type: TransactionType::Deposit,
                tx_id: 1,
                client_id: 2,
                amount: Some(dec!(100.00)),
            };
            let chargeback = TxRow {
                tx_type: TransactionType::Chargeback,
                tx_id: deposit.tx_id,
                client_id: deposit.client_id,
                amount: None,
            };
            let mut engine = Engine::new();
            engine.process_row(&deposit).unwrap();
            let err = engine.process_row(&chargeback).unwrap_err();
            assert_eq!(err, ProcessingError::TxNotDisputed(chargeback.tx_id));
        }
    }
}
