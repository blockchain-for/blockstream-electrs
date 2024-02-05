mod network;

pub use network::*;

#[cfg(not(feature = "liquid"))]
pub type Value = u64;
#[cfg(feature = "liquid")]
pub use confidential::Value;

#[cfg(not(feature = "liquid"))] // use regular Bitcoin data structures
pub use bitcoin::{
    blockdata::script, consensus::deserialize, util::address, Block, BlockHash, BlockHeader,
    OutPoint, Script, Transaction, TxIn, TxOut, Txid,
};

#[cfg(feature = "liquid")]
pub use {
    crate::elements::asset,
    elements::{
        address, confidential, encode::deserialize, script, Address, AssetId, Block, BlockHash,
        BlockHeader, OutPoint, Script, Transaction, TxIn, TxOut, Txid,
    },
};

use bitcoin::blockdata::constants::genesis_block;
