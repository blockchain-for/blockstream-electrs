use std::sync::Arc;

use bitcoin::BlockHash;

use crate::{
    chain::Network,
    config::Config,
    daemon::Daemon,
    store::{DBFlush, Store, DB},
    util::block::HeaderEntry,
};

use crate::metrics::{Gauge, HistogramOpts, HistogramTimer, HistogramVec, MetricOpts, Metrics};

use crate::errors::*;

use self::fetch::{BlockEntry, FetchFrom};

pub mod fetch;
pub mod query;
pub mod schema;
pub mod utxo;

pub struct Indexer {
    pub store: Arc<Store>,
    pub flush: DBFlush,
    pub from: FetchFrom,
    pub iconfig: IndexerConfig,
    pub duration: HistogramVec,
    pub tip_metric: Gauge,
}

impl Indexer {
    pub fn open(store: Arc<Store>, from: FetchFrom, config: &Config, metrics: &Metrics) -> Self {
        Self {
            store,
            flush: DBFlush::Disable,
            from,
            iconfig: IndexerConfig::from(config),
            duration: metrics.histogram_vec(
                HistogramOpts::new("index_duration", "Index update duration (in seconds)"),
                &["step"],
            ),
            tip_metric: metrics.gauge(MetricOpts::new("tip_height", "Current chain tip height")),
        }
    }

    pub fn update(&mut self, daemon: &Daemon) -> Result<BlockHash> {
        let daemon = daemon.reconnect()?;
        let tip = daemon.getbestblockhash()?;
        let new_headers = self.get_new_headers(&daemon, &tip)?;

        let to_add = self.headers_to_add(&new_headers);

        debug!(
            "adding transactions from {} blocks using {:?}",
            to_add.len(),
            self.from
        );

        start_fetcher(self.from, &daemon, to_add)?.map(|blocks| self.add(&blocks));

        self.start_auto_compactions(&self.store.txstore);

        let to_index = self.headers_to_index(&new_headers);
        debug!(
            "indexing history from {} blocks using {:?}",
            to_index.len(),
            self.from
        );
        start_fetcher(self.from, &daemon, to_index)?.map(|blocks| self.index(&blocks));
        self.start_auto_compactions(&self.store.history);

        todo!()
    }

    fn get_new_headers(&self, daemon: &Daemon, tip: &BlockHash) -> Result<Vec<HeaderEntry>> {
        let headers = self.store.indexed_headers.read().unwrap();
        let new_headers = daemon.get_new_headers(&headers, tip)?;

        let res = headers.order(new_headers);

        if let Some(tip) = res.last() {
            info!("{:#?} ({} left to index", tip, res.len());
        }

        Ok(res)
    }

    fn headers_to_add(&self, new_headers: &[HeaderEntry]) -> Vec<HeaderEntry> {
        let added_blockhashes = self.store.added_blockhashes.read().unwrap();
        new_headers
            .iter()
            .filter(|he| !added_blockhashes.contains(he.hash()))
            .cloned()
            .collect()
    }

    fn headers_to_index(&self, new_headers: &[HeaderEntry]) -> Vec<HeaderEntry> {
        let indexed_blockhashes = self.store.indexed_blockhashes.read().unwrap();
        new_headers
            .iter()
            .filter(|e| !indexed_blockhashes.contains(e.hash()))
            .cloned()
            .collect()
    }

    fn add(&self, blocks: &[BlockEntry]) {
        // TODO: skip orphaned blocks?
        let rows = {
            let _timer = self.start_timer("add_process");
            add_blocks(blocks, &self.iconfig)
        };
        {
            let _timer = self.start_timer("add_write");
            self.store.txstore.write(rows, self.flush);
        }

        self.store
            .added_blockhashes
            .write()
            .unwrap()
            .extend(blocks.iter().map(|b| b.entry.hash()));
    }

    fn index(&self, blocks: &[BlockEntry]) {
        let previous_txos_map = {
            let _timer = self.start_timer("index_lookup");
            lookup_txos(&self.store.txstore, &get_previous_txos(blocks), false)
        };
        let rows = {
            let _timer = self.start_timer("index_process");
            let added_blockhashes = self.store.added_blockhashes.read().unwrap();
            for b in blocks {
                let blockhash = b.entry.hash();
                // TODO: replace by lookup into txstore_db?
                if !added_blockhashes.contains(blockhash) {
                    panic!("cannot index block {} (missing from store)", blockhash);
                }
            }
            index_blocks(blocks, &previous_txos_map, &self.iconfig)
        };
        self.store.history.write(rows, self.flush);
    }

    fn start_auto_compactions(&self, store: &DB) {
        todo!()
    }

    fn start_timer(&self, name: &str) -> HistogramTimer {
        self.duration.with_label_values(&[name]).start_timer()
    }
}
pub struct IndexerConfig {
    pub light_mode: bool,
    pub address_search: bool,
    pub index_unspendables: bool,
    pub network: Network,
    #[cfg(feature = "liquid")]
    pub parent_network: crate::chain::BNetwork,
}

impl From<&Config> for IndexerConfig {
    fn from(config: &Config) -> Self {
        IndexerConfig {
            light_mode: config.light_mode,
            address_search: config.address_search,
            index_unspendables: config.index_unspendables,
            network: config.network_type,
            #[cfg(feature = "liquid")]
            parent_network: config.parent_network,
        }
    }
}

fn start_fetcher(
    from: FetchFrom,
    daemon: &Daemon,
    new_headers: Vec<HeaderEntry>,
) -> Result<Fetcher<Vec<HeaderEntry>>> {
    todo!()
}
