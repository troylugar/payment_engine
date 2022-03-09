use rust_decimal::Decimal;
use serde::Deserialize;

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TransactionType {
    Deposit,
    Withdrawal,
    Resolve,
    Dispute,
    Chargeback,
}

#[derive(Clone, Copy, Debug, Deserialize)]
pub struct TxRow {
    #[serde(rename(deserialize = "type"))]
    pub tx_type: TransactionType,
    #[serde(rename(deserialize = "client"))]
    pub client_id: u16,
    #[serde(rename(deserialize = "tx"))]
    pub tx_id: u32,
    pub amount: Option<Decimal>,
}

#[derive(Clone, Copy, Debug)]
pub struct AccountData {
    pub available: Decimal,
    pub held: Decimal,
}

#[derive(Clone, Copy, Debug)]
pub struct Transaction {
    pub amount: Decimal,
    pub disputed: bool,
}
