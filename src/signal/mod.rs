use core::panic;
use std::{
    thread,
    time::{Duration, Instant},
};

use channel::RecvTimeoutError;
use crossbeam_channel as channel;

use signal_hook::consts::{SIGINT, SIGTERM, SIGUSR1};

use crate::errors::*;

#[derive(Debug, Clone)]
pub struct Waiter {
    receiver: channel::Receiver<i32>,
}

fn notify(signals: &[i32]) -> channel::Receiver<i32> {
    let (sender, receiver) = channel::bounded(1);
    let mut signals =
        signal_hook::iterator::Signals::new(signals).expect("failed to register signal hook");

    thread::spawn(move || {
        for signal in signals.forever() {
            sender
                .send(signal)
                .unwrap_or_else(|_| panic!("failed to send signal {}", signal));
        }
    });

    receiver
}
impl Waiter {
    pub fn start() -> Self {
        Self {
            receiver: notify(&[
                SIGINT, SIGTERM,
                SIGUSR1, // allow external triggering (e.g. via bitcoind `blocknotify`)
            ]),
        }
    }

    pub fn wait(&self, duration: Duration, accept_sigusr: bool) -> Result<()> {
        self.wait_deadline(Instant::now() + duration, accept_sigusr)
    }

    pub fn wait_deadline(&self, deadline: Instant, accept_sigusr: bool) -> Result<()> {
        match self.receiver.recv_deadline(deadline) {
            Ok(sig) if sig == SIGUSR1 => {
                trace!("notified via SIGUSR1");
                if accept_sigusr {
                    Ok(())
                } else {
                    self.wait_deadline(deadline, accept_sigusr)
                }
            }
            Ok(sig) => bail!(ErrorKind::Interrupt(sig)),
            Err(RecvTimeoutError::Timeout) => Ok(()),
            Err(RecvTimeoutError::Disconnected) => bail!("signal hook channel disconnected"),
        }
    }
}
