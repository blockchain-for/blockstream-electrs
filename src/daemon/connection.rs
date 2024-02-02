use std::{
    io::{BufRead, BufReader, Lines},
    net::SocketAddr,
    net::TcpStream,
    sync::Arc,
    time::Duration,
};

use crate::errors::*;
use crate::signal::Waiter;

use super::CookieGetter;

pub(super) struct Connection {
    tx: TcpStream,
    rx: Lines<BufReader<TcpStream>>,
    cookie_getter: Arc<dyn CookieGetter>,
    addr: SocketAddr,
    signal: Waiter,
}

impl Connection {
    pub fn new(
        addr: SocketAddr,
        cookie_getter: Arc<dyn CookieGetter>,
        signal: Waiter,
    ) -> Result<Self> {
        let conn = tcp_connect(addr, &signal)?;
        let reader = BufReader::new(
            conn.try_clone()
                .chain_err(|| format!("failed to clone: {:?}", conn))?,
        );

        Ok(Self {
            tx: conn,
            rx: reader.lines(),
            cookie_getter,
            addr,
            signal,
        })
    }
}
pub fn tcp_connect(addr: SocketAddr, signal: &Waiter) -> Result<TcpStream> {
    loop {
        match TcpStream::connect(addr) {
            Ok(conn) => return Ok(conn),
            Err(e) => {
                warn!("failed to connect daemon at {}: {}", addr, e);
                signal.wait(Duration::from_secs(3), false)?;
                continue;
            }
        }
    }
}
