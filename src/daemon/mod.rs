mod connection;
mod counter;
mod daemon;
mod network;

use connection::*;
pub use counter::*;
pub use daemon::*;
pub use network::*;

use crate::errors::*;

pub trait CookieGetter: Send + Sync {
    fn get(&self) -> Result<Vec<u8>>;
}
