use std::{process, sync::Arc};

use electrs::{config::Config, errors::*};
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
    Ok(())
}
