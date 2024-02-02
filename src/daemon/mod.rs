mod connection;
mod counter;
mod network;

use bitcoin::consensus::serialize;
use bitcoin::hashes::hex::ToHex;
use bitcoin::{consensus::deserialize, hashes::hex::FromHex, Block, BlockHeader, Transaction};
use bitcoin::{BlockHash, Txid};
use connection::*;
pub use counter::*;
pub use network::*;

use itertools::Itertools;
use prometheus::{HistogramOpts, HistogramVec};
use serde_json::{from_str, from_value, Value};
use std::collections::HashSet;
use std::io::{BufRead, BufReader, Lines, Write};
use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

use crate::util::block::HeaderList;
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

    // Get estimated feerates for the provided confirmation targets using a batch RPC request
    // Missing estimates are logged but do not cause a failure, whatever is available is returned
    #[allow(clippy::float_cmp)]
    pub fn estimatesmartfee_batch(&self, conf_targets: &[u16]) -> Result<HashMap<u16, f64>> {
        let params_list: Vec<Value> = conf_targets.iter().map(|t| json!([t])).collect();

        Ok(self
            .requests("estimatesmartfee", &params_list)?
            .iter()
            .zip(conf_targets)
            .filter_map(|(reply, target)| {
                if !reply["errors"].is_null() {
                    warn!(
                        "failed estimating fee for target {}: {:?}",
                        target, reply["errors"]
                    );
                    return None;
                }

                let feerate = reply["feerate"]
                    .as_f64()
                    .unwrap_or_else(|| panic!("invalid estimatesmartfee response: {:?}", reply));

                if feerate == -1f64 {
                    warn!("not enough data to estimate fee for target {}", target);
                    return None;
                }

                // from BTC/kB to sat/b
                Some((*target, feerate * 100_000f64))
            })
            .collect())
    }

    fn get_all_headers(&self, tip: &BlockHash) -> Result<Vec<BlockHeader>> {
        let info: Value = self.request("getblockheader", json!([tip.to_hex()]))?;
        let tip_height = info
            .get("height")
            .expect("missing height")
            .as_u64()
            .expect("non-numeric height") as usize;
        let all_heights: Vec<usize> = (0..=tip_height).collect();
        let chunk_size = 100_000;
        let mut result = vec![];
        for heights in all_heights.chunks(chunk_size) {
            trace!("downloading {} block headers", heights.len());
            let mut headers = self.getblockheaders(&heights)?;
            assert!(headers.len() == heights.len());
            result.append(&mut headers);
        }

        let mut blockhash = BlockHash::default();
        for header in &result {
            assert_eq!(header.prev_blockhash, blockhash);
            blockhash = header.block_hash();
        }
        assert_eq!(blockhash, *tip);
        Ok(result)
    }

    // Returns a list of BlockHeaders in ascending height (i.e. the tip is last).
    pub fn get_new_headers(
        &self,
        indexed_headers: &HeaderList,
        bestblockhash: &BlockHash,
    ) -> Result<Vec<BlockHeader>> {
        // Iterate back over headers until known blockash is found:
        if indexed_headers.is_empty() {
            debug!("downloading all block headers up to {}", bestblockhash);
            return self.get_all_headers(bestblockhash);
        }
        debug!(
            "downloading new block headers ({} already indexed) from {}",
            indexed_headers.len(),
            bestblockhash,
        );
        let mut new_headers = vec![];
        let null_hash = BlockHash::default();
        let mut blockhash = *bestblockhash;
        while blockhash != null_hash {
            if indexed_headers.header_by_blockhash(&blockhash).is_some() {
                break;
            }
            let header = self
                .getblockheader(&blockhash)
                .chain_err(|| format!("failed to get {} header", blockhash))?;
            blockhash = header.prev_blockhash;
            new_headers.push(header);
        }
        trace!("downloaded {} block headers", new_headers.len());
        new_headers.reverse(); // so the tip is the last vector entry
        Ok(new_headers)
    }

    pub fn get_relayfee(&self) -> Result<f64> {
        let relayfee = self.getnetworkinfo()?.relayfee;

        // from BTC/kB to sat/b
        Ok(relayfee * 100_000f64)
    }

    fn request(&self, method: &str, params: Value) -> Result<Value> {
        let mut values = self.retry_request_batch(method, &[params])?;
        assert_eq!(values.len(), 1);
        Ok(values.remove(0))
    }

    fn requests(&self, method: &str, params: &[Value]) -> Result<Vec<Value>> {
        self.retry_request_batch(method, params)
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

/// For Bitcoind api
impl Daemon {
    fn getnetworkinfo(&self) -> Result<NetworkInfo> {
        let info = self.request("getnetworkinfo", json!([]))?;
        from_value(info).chain_err(|| "invalid network info")
    }

    fn getblockchaininfo(&self) -> Result<BlockchainInfo> {
        let info: Value = self.request("getblockchaininfo", json!([]))?;
        from_value(info).chain_err(|| "invalid blockchain info")
    }

    pub fn getbestblockhash(&self) -> Result<BlockHash> {
        parse_hash(&self.request("getbestblockhash", json!([]))?)
    }

    pub fn getblockheader(&self, blockhash: &BlockHash) -> Result<BlockHeader> {
        header_from_value(self.request("getblockheader", json!([blockhash.to_hex(), false]))?)
    }

    pub fn getblockheaders(&self, heights: &[usize]) -> Result<Vec<BlockHeader>> {
        let heights: Vec<Value> = heights.iter().map(|height| json!([height])).collect();
        let params_list: Vec<Value> = self
            .requests("getblockhash", &heights)?
            .into_iter()
            .map(|hash| json!([hash, /*verbose=*/ false]))
            .collect();
        let mut result = vec![];
        for h in self.requests("getblockheader", &params_list)? {
            result.push(header_from_value(h)?);
        }

        Ok(result)
    }

    pub fn getblock(&self, blockhash: &BlockHash) -> Result<Block> {
        let block = block_from_value(
            self.request("getblock", json!([blockhash.to_hex(), /*verbose=*/ false]))?,
        )?;
        assert_eq!(block.block_hash(), *blockhash);

        Ok(block)
    }

    pub fn getblock_raw(&self, blockhash: &BlockHash, verbose: u32) -> Result<Value> {
        self.request("getblock", json!([blockhash.to_hex(), verbose]))
    }

    pub fn getblocks(&self, blockhashes: &[BlockHash]) -> Result<Vec<Block>> {
        let params_list: Vec<Value> = blockhashes
            .iter()
            .map(|hash| json!([hash.to_hex(), /*verbose=*/ false]))
            .collect();
        let values = self.requests("getblock", &params_list)?;
        let mut blocks = vec![];
        for value in values {
            blocks.push(block_from_value(value)?);
        }
        Ok(blocks)
    }

    pub fn gettransactions(&self, txhashes: &[&Txid]) -> Result<Vec<Transaction>> {
        let params_list: Vec<Value> = txhashes
            .iter()
            .map(|txhash| json!([txhash.to_hex(), /*verbose=*/ false]))
            .collect();

        let values = self.requests("getrawtransaction", &params_list)?;
        let mut txs = vec![];
        for value in values {
            txs.push(tx_from_value(value)?);
        }
        assert_eq!(txhashes.len(), txs.len());
        Ok(txs)
    }

    pub fn gettransaction_raw(
        &self,
        txid: &Txid,
        blockhash: &BlockHash,
        verbose: bool,
    ) -> Result<Value> {
        self.request(
            "getrawtransaction",
            json!([txid.to_hex(), verbose, blockhash]),
        )
    }

    pub fn getmempooltx(&self, txhash: &Txid) -> Result<Transaction> {
        let value = self.request(
            "getrawtransaction",
            json!([txhash.to_hex(), /*verbose=*/ false]),
        )?;
        tx_from_value(value)
    }

    pub fn getmempooltxids(&self) -> Result<HashSet<Txid>> {
        let res = self.request("getrawmempool", json!([/*verbose=*/ false]))?;
        serde_json::from_value(res).chain_err(|| "invalid getrawmempool reply")
    }

    pub fn broadcast(&self, tx: &Transaction) -> Result<Txid> {
        self.broadcast_raw(&hex::encode(serialize(tx)))
    }

    pub fn broadcast_raw(&self, txhex: &str) -> Result<Txid> {
        let txid = self.request("sendrawtransaction", json!([txhex]))?;

        Txid::from_hex(txid.as_str().chain_err(|| "non-string txid")?)
            .chain_err(|| "failed to parse txid")
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
