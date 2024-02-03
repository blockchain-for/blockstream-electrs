use std::sync::Arc;

use prometheus::{Gauge, HistogramVec};

use crate::{chain::Network, config::Config};

use super::{db::DBFlush, fetch::FetchFrom, Store};

pub struct Indexer {
    pub store: Arc<Store>,
    pub flush: DBFlush,
    pub from: FetchFrom,
    pub iconfig: IndexerConfig,
    pub duration: HistogramVec,
    pub tip_metric: Gauge,
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
