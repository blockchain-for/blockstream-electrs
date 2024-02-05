use std::collections::HashMap;

use bincode::Options;
use bitcoin::{
    consensus::{deserialize, serialize},
    BlockHash, OutPoint, Script, Transaction, TxOut, Txid,
};

use crate::{
    chain::Value,
    store::{compute_script_hash, DBRow},
    util::{
        block::{BlockId, BlockMeta},
        full_hash, Bytes, FullHash,
    },
};

use super::BlockEntry;

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

#[derive(Debug, Default, Deserialize, Serialize)]
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

#[derive(Deserialize, Serialize, Debug)]
pub struct TxConfKey {
    pub code: u8,
    pub txid: FullHash,
    pub blockhash: FullHash,
}

pub struct TxConfRow {
    pub key: TxConfKey,
}

impl TxConfRow {
    pub fn new(txn: &Transaction, blockhash: FullHash) -> TxConfRow {
        let txid = full_hash(&txn.txid()[..]);
        TxConfRow {
            key: TxConfKey {
                code: b'C',
                txid,
                blockhash,
            },
        }
    }

    pub fn filter(prefix: &[u8]) -> Bytes {
        [b"C", prefix].concat()
    }

    pub fn into_row(self) -> DBRow {
        DBRow {
            key: bincode::serialize(&self.key).unwrap(),
            value: vec![],
        }
    }

    pub fn from_row(row: DBRow) -> Self {
        TxConfRow {
            key: bincode::deserialize(&row.key).expect("failed to parse TxConfKey"),
        }
    }
}

#[derive(Deserialize, Serialize)]
pub struct TxOutKey {
    pub code: u8,
    pub txid: FullHash,
    pub vout: u16,
}

pub struct TxOutRow {
    pub key: TxOutKey,
    pub value: Bytes, // serialized output
}

impl TxOutRow {
    pub fn new(txid: &FullHash, vout: usize, txout: &TxOut) -> TxOutRow {
        TxOutRow {
            key: TxOutKey {
                code: b'O',
                txid: *txid,
                vout: vout as u16,
            },
            value: serialize(txout),
        }
    }

    pub fn key(outpoint: &OutPoint) -> Bytes {
        bincode::serialize(&TxOutKey {
            code: b'O',
            txid: full_hash(&outpoint.txid[..]),
            vout: outpoint.vout as u16,
        })
        .unwrap()
    }

    pub fn into_row(self) -> DBRow {
        DBRow {
            key: bincode::serialize(&self.key).unwrap(),
            value: self.value,
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct BlockKey {
    pub code: u8,
    pub hash: FullHash,
}

pub struct BlockRow {
    pub key: BlockKey,
    pub value: Bytes, // serialized output
}

impl BlockRow {
    pub fn new_header(block_entry: &BlockEntry) -> BlockRow {
        BlockRow {
            key: BlockKey {
                code: b'B',
                hash: full_hash(&block_entry.entry.hash()[..]),
            },
            value: serialize(&block_entry.block.header),
        }
    }

    pub fn new_txids(hash: FullHash, txids: &[Txid]) -> BlockRow {
        BlockRow {
            key: BlockKey { code: b'X', hash },
            value: bincode::serialize(txids).unwrap(),
        }
    }

    pub fn new_meta(hash: FullHash, meta: &BlockMeta) -> BlockRow {
        BlockRow {
            key: BlockKey { code: b'M', hash },
            value: bincode::serialize(meta).unwrap(),
        }
    }

    pub fn new_done(hash: FullHash) -> BlockRow {
        BlockRow {
            key: BlockKey { code: b'D', hash },
            value: vec![],
        }
    }

    pub fn header_filter() -> Bytes {
        b"B".to_vec()
    }

    pub fn txids_key(hash: FullHash) -> Bytes {
        [b"X", &hash[..]].concat()
    }

    pub fn meta_key(hash: FullHash) -> Bytes {
        [b"M", &hash[..]].concat()
    }

    pub fn done_filter() -> Bytes {
        b"D".to_vec()
    }

    pub fn into_row(self) -> DBRow {
        DBRow {
            key: bincode::serialize(&self.key).unwrap(),
            value: self.value,
        }
    }

    pub fn from_row(row: DBRow) -> Self {
        BlockRow {
            key: bincode::deserialize(&row.key).unwrap(),
            value: row.value,
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct FundingInfo {
    pub txid: FullHash,
    pub vout: u16,
    pub value: Value,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct SpendingInfo {
    pub txid: FullHash, // spending transaction
    pub vin: u16,
    pub prev_txid: FullHash, // funding transaction
    pub prev_vout: u16,
    pub value: Value,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum TxHistoryInfo {
    Funding(FundingInfo),
    Spending(SpendingInfo),

    #[cfg(feature = "liquid")]
    Issuing(asset::IssuingInfo),
    #[cfg(feature = "liquid")]
    Burning(asset::BurningInfo),
    #[cfg(feature = "liquid")]
    Pegin(peg::PeginInfo),
    #[cfg(feature = "liquid")]
    Pegout(peg::PegoutInfo),
}

impl TxHistoryInfo {
    pub fn get_txid(&self) -> Txid {
        match self {
            TxHistoryInfo::Funding(FundingInfo { txid, .. })
            | TxHistoryInfo::Spending(SpendingInfo { txid, .. }) => deserialize(txid),

            #[cfg(feature = "liquid")]
            TxHistoryInfo::Issuing(asset::IssuingInfo { txid, .. })
            | TxHistoryInfo::Burning(asset::BurningInfo { txid, .. })
            | TxHistoryInfo::Pegin(peg::PeginInfo { txid, .. })
            | TxHistoryInfo::Pegout(peg::PegoutInfo { txid, .. }) => deserialize(txid),
        }
        .expect("cannot parse Txid")
    }
}

#[derive(Serialize, Deserialize)]
pub struct TxHistoryKey {
    pub code: u8,              // H for script history or I for asset history (elements only)
    pub hash: FullHash, // either a scripthash (always on bitcoin) or an asset id (elements only)
    pub confirmed_height: u32, // MUST be serialized as big-endian (for correct scans).
    pub txinfo: TxHistoryInfo,
}

pub struct TxHistoryRow {
    pub key: TxHistoryKey,
}

impl TxHistoryRow {
    pub fn new(script: &Script, confirmed_height: u32, txinfo: TxHistoryInfo) -> Self {
        let key = TxHistoryKey {
            code: b'H',
            hash: compute_script_hash(&script),
            confirmed_height,
            txinfo,
        };
        TxHistoryRow { key }
    }

    fn filter(code: u8, hash_prefix: &[u8]) -> Bytes {
        [&[code], hash_prefix].concat()
    }

    fn prefix_end(code: u8, hash: &[u8]) -> Bytes {
        bincode::serialize(&(code, full_hash(&hash[..]), std::u32::MAX)).unwrap()
    }

    fn prefix_height(code: u8, hash: &[u8], height: u32) -> Bytes {
        bincode::options()
            .with_big_endian()
            .serialize(&(code, full_hash(&hash[..]), height))
            .unwrap()
    }

    pub fn into_row(self) -> DBRow {
        DBRow {
            key: bincode::options()
                .with_big_endian()
                .serialize(&self.key)
                .unwrap(),
            value: vec![],
        }
    }

    pub fn from_row(row: DBRow) -> Self {
        let key = bincode::options()
            .with_big_endian()
            .deserialize(&row.key)
            .expect("failed to deserialize TxHistoryKey");
        TxHistoryRow { key }
    }

    pub fn get_txid(&self) -> Txid {
        self.key.txinfo.get_txid()
    }
    fn get_funded_outpoint(&self) -> OutPoint {
        self.key.txinfo.get_funded_outpoint()
    }
}

impl TxHistoryInfo {
    // for funding rows, returns the funded output.
    // for spending rows, returns the spent previous output.
    pub fn get_funded_outpoint(&self) -> OutPoint {
        match self {
            TxHistoryInfo::Funding(ref info) => OutPoint {
                txid: deserialize(&info.txid).unwrap(),
                vout: info.vout as u32,
            },
            TxHistoryInfo::Spending(ref info) => OutPoint {
                txid: deserialize(&info.prev_txid).unwrap(),
                vout: info.prev_vout as u32,
            },
            #[cfg(feature = "liquid")]
            TxHistoryInfo::Issuing(_)
            | TxHistoryInfo::Burning(_)
            | TxHistoryInfo::Pegin(_)
            | TxHistoryInfo::Pegout(_) => unreachable!(),
        }
    }
}

#[derive(Serialize, Deserialize)]
struct TxEdgeKey {
    code: u8,
    funding_txid: FullHash,
    funding_vout: u16,
    spending_txid: FullHash,
    spending_vin: u16,
}

pub struct TxEdgeRow {
    key: TxEdgeKey,
}

impl TxEdgeRow {
    pub fn new(
        funding_txid: FullHash,
        funding_vout: u16,
        spending_txid: FullHash,
        spending_vin: u16,
    ) -> Self {
        let key = TxEdgeKey {
            code: b'S',
            funding_txid,
            funding_vout,
            spending_txid,
            spending_vin,
        };
        TxEdgeRow { key }
    }

    pub fn filter(outpoint: &OutPoint) -> Bytes {
        // TODO build key without using bincode? [ b"S", &outpoint.txid[..], outpoint.vout?? ].concat()
        bincode::serialize(&(b'S', full_hash(&outpoint.txid[..]), outpoint.vout as u16)).unwrap()
    }

    pub fn into_row(self) -> DBRow {
        DBRow {
            key: bincode::serialize(&self.key).unwrap(),
            value: vec![],
        }
    }

    pub fn from_row(row: DBRow) -> Self {
        TxEdgeRow {
            key: bincode::deserialize(&row.key).expect("failed to deserialize TxEdgeKey"),
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct ScriptCacheKey {
    code: u8,
    scripthash: FullHash,
}

pub struct StatsCacheRow {
    key: ScriptCacheKey,
    value: Bytes,
}

impl StatsCacheRow {
    pub fn new(scripthash: &[u8], stats: &ScriptStats, blockhash: &BlockHash) -> Self {
        StatsCacheRow {
            key: ScriptCacheKey {
                code: b'A',
                scripthash: full_hash(scripthash),
            },
            value: bincode::serialize(&(stats, blockhash)).unwrap(),
        }
    }

    pub fn key(scripthash: &[u8]) -> Bytes {
        [b"A", scripthash].concat()
    }

    fn into_row(self) -> DBRow {
        DBRow {
            key: bincode::serialize(&self.key).unwrap(),
            value: self.value,
        }
    }
}

pub type CachedUtxoMap = HashMap<(Txid, u32), (u32, Value)>; // (txid,vout) => (block_height,output_value)

pub struct UtxoCacheRow {
    key: ScriptCacheKey,
    value: Bytes,
}

impl UtxoCacheRow {
    pub fn new(scripthash: &[u8], utxos: &UtxoMap, blockhash: &BlockHash) -> Self {
        let utxos_cache = make_utxo_cache(utxos);

        UtxoCacheRow {
            key: ScriptCacheKey {
                code: b'U',
                scripthash: full_hash(scripthash),
            },
            value: bincode::serialize(&(utxos_cache, blockhash)).unwrap(),
        }
    }

    pub fn key(scripthash: &[u8]) -> Bytes {
        [b"U", scripthash].concat()
    }

    fn into_row(self) -> DBRow {
        DBRow {
            key: bincode::serialize(&self.key).unwrap(),
            value: self.value,
        }
    }
}

// keep utxo cache with just the block height (the hash/timestamp are read later from the headers to reconstruct BlockId)
// and use a (txid,vout) tuple instead of OutPoints (they don't play nicely with bincode serialization)
pub fn make_utxo_cache(utxos: &UtxoMap) -> CachedUtxoMap {
    utxos
        .iter()
        .map(|(outpoint, (blockid, value))| {
            (
                (outpoint.txid, outpoint.vout),
                (blockid.height as u32, *value),
            )
        })
        .collect()
}
