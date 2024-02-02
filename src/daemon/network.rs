use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct NetworkInfo {
    pub(super) version: u64,
    pub(super) subversion: String,
    relayfee: f64, // in BTC/kB
}

#[derive(Debug, Deserialize, Serialize)]
pub struct BlockchainInfo {
    pub chain: String,
    pub blocks: u32,
    pub headers: u32,
    pub bestblockhash: String,
    pub pruned: bool,
    pub verificationprogress: f32,
    pub initialblockdownload: Option<bool>,
}
