mod connection;
mod counter;
mod network;

use bitcoin::{consensus::deserialize, hashes::hex::FromHex, Block, BlockHeader, Transaction};
use connection::*;
pub use counter::*;
pub use network::*;

use itertools::Itertools;
use prometheus::{HistogramOpts, HistogramVec};
use serde_json::{from_str, from_value, Value};
use std::io::{BufRead, BufReader, Lines, Write};
use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

use crate::{chain::Network, errors::*, metrics::Metrics, signal::Waiter};

pub trait CookieGetter: Send + Sync {
    fn get(&self) -> Result<Vec<u8>>;
}

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

    pub fn reconnect(&self) -> Result<Self> {
        Ok(Self {
            daemon_dir: self.daemon_dir.clone(),
            blocks_dir: self.blocks_dir.clone(),
            network: self.network,
            conn: Mutex::new(self.conn.lock().unwrap().reconnect()?),
            message_id: Counter::default(),
            signal: self.signal.clone(),
            latency: self.latency.clone(),
            size: self.size.clone(),
        })
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
        loop {
            match self.handle_request_batch(method, params) {
                Err(Error(ErrorKind::Connection(msg), _)) => {
                    warn!("reconnecting to bitcoind: {}", msg);
                    self.signal.wait(Duration::from_secs(3), false)?;

                    let mut conn = self.conn.lock().unwrap();
                    *conn = conn.reconnect()?;

                    continue;
                }
                result => return result,
            }
        }
    }

    fn handle_request_batch(&self, method: &str, params: &[Value]) -> Result<Vec<Value>> {
        let id = self.message_id.next();
        let chunks = params
            .iter()
            .map(|p| json!({"method": method, "params": p, "id": id}))
            .chunks(50_000);

        let mut results = vec![];

        for chunk in &chunks {
            let req = chunk.collect();
            let mut replies = self.call_jsonrpc(method, &req)?;

            if let Some(replies_vec) = replies.as_array_mut() {
                for reply in replies_vec {
                    results.push(parse_jsonrpc_reply(reply.take(), method, id)?);
                }
            } else {
                bail!("non-array replies: {:?}", replies);
            }
        }

        Ok(results)
    }

    fn call_jsonrpc(&self, method: &str, request: &Value) -> Result<Value> {
        let mut conn = self.conn.lock().unwrap();
        let timer = self.latency.with_label_values(&[method]).start_timer();
        let request = request.to_string();

        conn.send(&request)?;

        self.size
            .with_label_values(&[method, "send"])
            .observe(request.len() as f64);

        let response = conn.recv()?;

        let result: Value = from_str(&response).chain_err(|| "invalid JSON")?;

        timer.observe_duration();

        self.size
            .with_label_values(&[method, "recv"])
            .observe(response.len() as f64);

        Ok(result)
    }
}

fn parse_jsonrpc_reply(mut reply: Value, method: &str, expected_id: u64) -> Result<Value> {
    if let Some(reply_obj) = reply.as_object_mut() {
        if let Some(err) = reply_obj.get("error") {
            if !err.is_null() {
                if let Some(code) = parse_error_code(err) {
                    match code {
                        // RPC_IN_WARMUP -> retry by later reconnection
                        -28 => bail!(ErrorKind::Connection(err.to_string())),
                        _ => bail!("{} RPC error: {}", method, err),
                    }
                }
            }
        }
        let id = reply_obj
            .get("id")
            .chain_err(|| format!("no id in reply: {:?}", reply_obj))?
            .clone();
        if id != expected_id {
            bail!(
                "wrong {} response id {}, expected {}",
                method,
                id,
                expected_id
            );
        }
        if let Some(result) = reply_obj.get_mut("result") {
            return Ok(result.take());
        }
        bail!("no result in reply: {:?}", reply_obj);
    }
    bail!("non-object reply: {:?}", reply);
}

/// Parse JSONRPC error code, if exists.
fn parse_error_code(err: &Value) -> Option<i64> {
    err.as_object()?.get("code")?.as_i64()
}

fn parse_hash<T>(value: &Value) -> Result<T>
where
    T: FromHex,
{
    T::from_hex(
        value
            .as_str()
            .chain_err(|| format!("non-string value: {}", value))?,
    )
    .chain_err(|| format!("non-hex value:: {}", value))
}

fn header_from_value(value: Value) -> Result<BlockHeader> {
    let header_hex = value
        .as_str()
        .chain_err(|| format!("non-string header: {}", value))?;
    let header_bytes = hex::decode(header_hex).chain_err(|| "non-hex header")?;

    deserialize(&header_bytes).chain_err(|| format!("failed to parse header {}", header_hex))
}

fn block_from_value(value: Value) -> Result<Block> {
    let block_hex = value.as_str().chain_err(|| "non-string block")?;
    let block_bytes = hex::decode(block_hex).chain_err(|| "non-hex block")?;
    deserialize(&block_bytes).chain_err(|| format!("failed to parse block {}", block_hex))
}

fn tx_from_value(value: Value) -> Result<Transaction> {
    let tx_hex = value.as_str().chain_err(|| "non-string tx")?;
    let tx_bytes = hex::decode(tx_hex).chain_err(|| "non-hex tx")?;
    deserialize(&tx_bytes).chain_err(|| format!("failed to parse tx {}", tx_hex))
}
