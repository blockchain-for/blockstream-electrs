use crate::util::{Bytes, FullHash};
use serde::{Deserialize, Serialize};

use super::db::DBRow;

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
    pub fn from_row(row: DBRow) -> Self {
        BlockRow {
            key: bincode::deserialize(&row.key).unwrap(),
            value: row.value,
        }
    }

    pub fn header_filter() -> Bytes {
        b"B".to_vec()
    }

    pub fn done_filter() -> Bytes {
        b"D".to_vec()
    }
}
