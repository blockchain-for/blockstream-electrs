use std::sync::Arc;

use crate::{store::Store, util::block::BlockId};

pub struct ChainQuery {
    pub store: Arc<Store>,
}

impl ChainQuery {
    pub fn blockid_by_height(&self, height: usize) -> Option<BlockId> {
        self.store
            .indexed_headers
            .read()
            .unwrap()
            .header_by_height(height)
            .map(BlockId::from)
    }
}
