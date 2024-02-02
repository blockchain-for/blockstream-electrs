use std::{process, sync::Arc};

use electrs::{
    config::Config, daemon::Daemon, errors::*, metrics::Metrics, new_index::Store, signal::Waiter,
};
use error_chain::ChainedError;
use log::error;

fn main() {
    let config = Arc::new(Config::from_args());
    if let Err(e) = run_server(config) {
        error!("server failed: {}", e.display_chain());

        process::exit(1);
    }
}

fn run_server(config: Arc<Config>) -> Result<()> {
    let signal = Waiter::start();
    let metrics = Metrics::new(config.monitoring_addr);
    metrics.start();

    let daemon = Arc::new(Daemon::new(
        config.daemon_dir.as_path(),
        &config.blocks_dir,
        config.daemon_rpc_addr,
        config.cookie_getter(),
        config.network_type,
        signal.clone(),
        &metrics,
    ));

    let store = Arc::new(Store::open(&config.db_path.join("newindex"), &config));

    Ok(())
}
