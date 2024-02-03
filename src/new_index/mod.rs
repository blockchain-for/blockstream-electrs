pub mod db;
pub mod fetch;
pub mod indexer;
pub mod schema;
pub mod utxo;

use std::{
    collections::{HashMap, HashSet},
    path::Path,
    sync::RwLock,
};

use bitcoin::{consensus::deserialize, BlockHash, BlockHeader};

use crate::{config::Config, util::block::HeaderList};

use self::{db::DB, schema::BlockRow};

const MIN_HISTORY_ITEMS_TO_CACHE: usize = 100;

pub struct Store {
    // TODO: should be column families
    pub txstore: DB,
    pub history: DB,
    pub cache: DB,
    pub added_blockhashes: RwLock<HashSet<BlockHash>>,
    pub indexed_blockhashes: RwLock<HashSet<BlockHash>>,
    pub indexed_headers: RwLock<HeaderList>,
}

impl Store {
    pub fn open(path: &Path, config: &Config) -> Self {
        let txstore = DB::open(&path.join("txstore"), config);
        let added_blockhashes = load_blockhashes(&txstore, &BlockRow::done_filter());
        debug!("{} blocks were added", added_blockhashes.len());

        let history = DB::open(&path.join("history"), config);
        let indexed_blockhashes = load_blockhashes(&history, &BlockRow::done_filter());
        debug!("{} blocks were indexed", indexed_blockhashes.len());

        let cache = DB::open(&path.join("cache"), config);

        let headers = if let Some(tip_hash) = txstore.get(b"t") {
            let tip_hash = deserialize(&tip_hash).expect("invalid chain tip in `t`");
            let headers_map = load_blockheaders(&txstore);

            debug!(
                "{} headers were loaded, tip at {:?}",
                headers_map.len(),
                tip_hash
            );

            HeaderList::new(headers_map, tip_hash)
        } else {
            HeaderList::default()
        };

        Self {
            txstore,
            history,
            cache,
            added_blockhashes: RwLock::new(added_blockhashes),
            indexed_blockhashes: RwLock::new(indexed_blockhashes),
            indexed_headers: RwLock::new(headers),
        }
    }

    pub fn txstore(&self) -> &DB {
        &self.txstore
    }

    pub fn history(&self) -> &DB {
        &self.history
    }

    pub fn cache(&self) -> &DB {
        &self.cache
    }

    pub fn done_inital_sync(&self) -> bool {
        self.txstore.get(b"t").is_some()
    }
}

fn load_blockhashes(db: &DB, prefix: &[u8]) -> HashSet<BlockHash> {
    db.iter_scan(prefix)
        .map(BlockRow::from_row)
        .map(|r| deserialize(&r.key.hash).expect("failed to parse BlockHash"))
        .collect()
}

fn load_blockheaders(db: &DB) -> HashMap<BlockHash, BlockHeader> {
    db.iter_scan(&BlockRow::header_filter())
        .map(BlockRow::from_row)
        .map(|r| {
            let key: BlockHash = deserialize(&r.key.hash).expect("failed to parse BlockHash");
            let value = deserialize(&r.value).expect("failed to parse BlockHeader");
            (key, value)
        })
        .collect()
}
