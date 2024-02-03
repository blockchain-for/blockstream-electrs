pub mod block;

use std::{
    sync::mpsc::{channel, sync_channel, Receiver, Sender, SyncSender},
    thread,
};

pub type Bytes = Vec<u8>;
// TODO: replace by a separate opaque type (similar to Sha256dHash, but without the "double")
pub type FullHash = [u8; 32]; // serialized SHA256 result

// TODO: consolidate serialization/deserialize code for bincode/bitcoin.
const HASH_LEN: usize = 32;

pub fn spawn_thread<F, T>(name: &str, f: F) -> thread::JoinHandle<T>
where
    F: FnOnce() -> T,
    F: Send + 'static,
    T: Send + 'static,
{
    thread::Builder::new()
        .name(name.to_string())
        .spawn(f)
        .unwrap()
}

pub fn full_hash(hash: &[u8]) -> FullHash {
    *array_ref![hash, 0, HASH_LEN]
}

pub struct SyncChannel<T> {
    tx: SyncSender<T>,
    rx: Receiver<T>,
}

impl<T> SyncChannel<T> {
    pub fn new(size: usize) -> Self {
        let (tx, rx) = sync_channel(size);
        Self { tx, rx }
    }

    pub fn sender(&self) -> SyncSender<T> {
        self.sender().clone()
    }

    pub fn receiver(&self) -> &Receiver<T> {
        &self.rx
    }

    pub fn into_receiver(self) -> Receiver<T> {
        self.rx
    }
}

pub struct Channel<T> {
    tx: Sender<T>,
    rx: Receiver<T>,
}

impl<T> Channel<T> {
    pub fn unbounded() -> Self {
        let (tx, rx) = channel();
        Channel { tx, rx }
    }

    pub fn sender(&self) -> Sender<T> {
        self.tx.clone()
    }

    pub fn receiver(&self) -> &Receiver<T> {
        &self.rx
    }

    pub fn into_receiver(self) -> Receiver<T> {
        self.rx
    }
}
