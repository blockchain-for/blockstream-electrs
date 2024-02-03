use bitcoin::Block;

use crate::util::block::HeaderEntry;

pub enum FetchFrom {
    Bitcoind,
    BlkFiles,
}

pub struct BlockEntry {
    pub block: Block,
    pub entry: HeaderEntry,
    pub size: u32,
}
