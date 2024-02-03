use std::sync::mpsc::Receiver;
use std::thread;

use bitcoin::{Block, BlockHash};

use crate::util::{spawn_thread, SyncChannel};
use crate::{daemon, errors::*};
use crate::{daemon::Daemon, util::block::HeaderEntry};

#[derive(Debug)]
pub enum FetchFrom {
    Bitcoind,
    BlkFiles,
}

pub struct BlockEntry {
    pub block: Block,
    pub entry: HeaderEntry,
    pub size: u32,
}

pub fn start_fetcher(
    from: FetchFrom,
    daemon: &Daemon,
    new_headers: Vec<HeaderEntry>,
) -> Result<Fetcher<Vec<BlockEntry>>> {
    let fetcher = match from {
        FetchFrom::Bitcoind => bitcoind_fetcher,
        FetchFrom::BlkFiles => blkfiles_fetcher,
    };
    fetcher(daemon, new_headers)
}

pub struct Fetcher<T> {
    receiver: Receiver<T>,
    thread: thread::JoinHandle<()>,
}

impl<T> Fetcher<T> {
    pub fn from(receiver: Receiver<T>, thread: thread::JoinHandle<()>) -> Self {
        Self { receiver, thread }
    }

    pub fn each<F>(self, mut func: F)
    where
        F: FnMut(T),
    {
        for item in self.receiver {
            func(item);
        }

        self.thread.join().expect("fetcher thread panicked")
    }
}

fn bitcoind_fetcher(
    daemon: &Daemon,
    new_headers: Vec<HeaderEntry>,
) -> Result<Fetcher<Vec<BlockEntry>>> {
    if let Some(tip) = new_headers.last() {
        debug!("{:?} ({} left to index", tip, new_headers.len());
    }

    let daemon = daemon.reconnect()?;
    let chan = SyncChannel::new(1);
    let sender = chan.sender();

    Ok(Fetcher::from(
        chan.into_receiver(),
        spawn_thread("bitcoind_fetcher", move || {
            for entries in new_headers.chunks(100) {
                let blockhashes: Vec<BlockHash> = entries.iter().map(|he| *he.hash()).collect();
                let blocks = daemon
                    .getblocks(&blockhashes)
                    .expect("failed to get blocks from bitcoind");
                assert_eq!(blocks.len(), entries.len());

                let block_entries: Vec<BlockEntry> = blocks
                    .into_iter()
                    .zip(entries)
                    .map(|(block, entry)| BlockEntry {
                        entry: entry.clone(),
                        size: block.size() as u32,
                        block,
                    })
                    .collect();
                assert_eq!(block_entries.len(), entries.len());

                sender
                    .send(block_entries)
                    .expect("failed to send fetched blocks");
            }
        }),
    ))
}
