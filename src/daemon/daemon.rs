use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

use prometheus::{HistogramOpts, HistogramVec};
use serde_json::{from_value, Value};

use crate::{chain::Network, metrics::Metrics, signal::Waiter};

use super::{BlockchainInfo, Connection, CookieGetter, Counter, NetworkInfo};

use crate::errors::*;

pub struct Daemon {
    daemon_dir: PathBuf,
    blocks_dir: PathBuf,
    network: Network,
    conn: Mutex<Connection>,
    message_id: Counter, // for monotonic JSONRPC 'id'
    signal: Waiter,

    // For monitoring
    latency: HistogramVec,
    size: HistogramVec,
}

impl Daemon {
    pub fn new(
        daemon_dir: &Path,
        blocks_dir: &Path,
        daemon_rpc_addr: SocketAddr,
        cookie_getter: Arc<dyn CookieGetter>,
        network: Network,
        signal: Waiter,
        metrics: &Metrics,
    ) -> Result<Self> {
        let daemon = Self {
            daemon_dir: daemon_dir.to_path_buf(),
            blocks_dir: blocks_dir.to_path_buf(),
            network,
            conn: Mutex::new(Connection::new(
                daemon_rpc_addr,
                cookie_getter,
                signal.clone(),
            )?),
            message_id: Counter::default(),
            signal: signal.clone(),
            latency: metrics.histogram_vec(
                HistogramOpts::new("daemon_rpc", "Bitcoind RPC latency (in seconds)"),
                &["method"],
            ),
            size: metrics.histogram_vec(
                HistogramOpts::new("daemon_bytes", "Bitcoind RPC size (in bytes)"),
                &["method", "dir"],
            ),
        };

        let network_info = daemon.getnetworkinfo()?;
        info!("{:#?}", network_info);

        if network_info.version < 160_000 {
            bail!(
                "{} is not supported - Please use bitcoind 0.16+",
                network_info.subversion
            );
        }

        let blockchain_info = daemon.getblockchaininfo()?;
        info!("{:#?}", blockchain_info);

        if blockchain_info.pruned {
            bail!("pruned node is not supported (use '-prune=0' bitcoind flag");
        }

        loop {
            let info = daemon.getblockchaininfo()?;

            if !info.initialblockdownload.unwrap_or(false) && info.blocks == info.headers {
                break;
            }

            warn!(
                "Waiting for bitcoind sync to finish: {}/{} blocks, verification progress: {:.3}%",
                info.blocks,
                info.headers,
                info.verificationprogress * 100.0
            );

            signal.wait(Duration::from_secs(5), false)?;
        }

        Ok(daemon)
    }

    fn getnetworkinfo(&self) -> Result<NetworkInfo> {
        let info = self.request("getnetworkinfo", json!([]))?;
        from_value(info).chain_err(|| "invalid network info")
    }

    fn getblockchaininfo(&self) -> Result<BlockchainInfo> {
        todo!()
    }

    fn request(&self, method: &str, params: Value) -> Result<Value> {
        let mut values = self.retry_request_batch(method, &[params])?;
        assert_eq!(values.len(), 1);
        Ok(values.remove(0))
    }

    fn retry_request_batch(&self, method: &str, params: &[Value]) -> Result<Vec<Value>> {
        todo!()
    }
}
