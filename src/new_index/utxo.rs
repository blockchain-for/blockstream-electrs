use std::collections::HashMap;

use bitcoin::{consensus::serialize, OutPoint, Transaction, Txid};

use crate::{
    chain::Value,
    util::{block::BlockId, Bytes, FullHash},
};

use super::db::DBRow;

pub type UtxoMap = HashMap<OutPoint, (BlockId, Value)>;

#[derive(Debug)]
pub struct Utxo {
    pub txid: Txid,
    pub vout: u32,
    pub confirmed: Option<BlockId>,
    pub value: Value,

    #[cfg(feature = "liquid")]
    pub asset: elements::confidential::Asset,
    #[cfg(feature = "liquid")]
    pub nonce: elements::confidential::Nonce,
    #[cfg(feature = "liquid")]
    pub witness: elements::TxOutWitness,
}

impl From<&Utxo> for OutPoint {
    fn from(value: &Utxo) -> Self {
        Self {
            txid: value.txid,
            vout: value.vout,
        }
    }
}

#[derive(Debug)]
pub struct SpendingInput {
    pub txid: Txid,
    pub vin: u32,
    pub confirmed: Option<BlockId>,
}

#[derive(Debug, Default)]
pub struct ScriptStats {
    pub tx_count: usize,
    pub funded_txo_count: usize,
    pub spend_txo_count: usize,
    #[cfg(not(feature = "liquid"))]
    pub funded_txo_sum: u64,
    #[cfg(feature = "liquid")]
    pub spent_txo_sum: u64,
}

#[derive(Serialize, Debug, Deserialize)]
pub struct TxRowKey {
    code: u8,
    txid: FullHash,
}

pub struct TxRow {
    pub key: TxRowKey,
    pub value: Bytes,
}

impl TxRow {
    pub fn new(txn: &Transaction) -> Self {
        let txid = full_hash(&txn.txid()[..]);

        Self {
            key: TxRowKey { code: b'T', txid },
            value: serialize(txn),
        }
    }

    pub fn key(prefix: &[u8]) -> Bytes {
        [b"T", prefix].concat()
    }

    pub fn into_row(self) -> DBRow {
        let Self { key, value } = self;

        DBRow {
            key: bincode::serialize(&key).unwrap(),
            value,
        }
    }
}
