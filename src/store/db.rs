use std::path::Path;

use rocksdb;

use crate::{config::Config, util::Bytes};

static DB_VERSION: u32 = 1;

pub struct DBRow {
    pub key: Vec<u8>,
    pub value: Vec<u8>,
}

#[derive(Copy, Clone, Debug)]
pub enum DBFlush {
    Disable,
    Enable,
}

pub struct ScanIterator<'a> {
    prefix: Vec<u8>,
    iter: rocksdb::DBIterator<'a>,
    done: bool,
}

impl<'a> Iterator for ScanIterator<'a> {
    type Item = DBRow;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }

        let (key, value) = self.iter.next()?;
        if !key.starts_with(&self.prefix) {
            self.done = true;
            return None;
        }

        Some(DBRow {
            key: key.to_vec(),
            value: value.to_vec(),
        })
    }
}

pub struct ReverseScanIterator<'a> {
    prefix: Vec<u8>,
    iter: rocksdb::DBRawIterator<'a>,
    done: bool,
}

impl<'a> Iterator for ReverseScanIterator<'a> {
    type Item = DBRow;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done || !self.iter.valid() {
            return None;
        }

        let key = self.iter.key().unwrap();
        if !key.starts_with(&self.prefix) {
            self.done = true;
            return None;
        }

        let row = DBRow {
            key: key.into(),
            value: self.iter.value().unwrap().into(),
        };

        self.iter.prev();

        Some(row)
    }
}

#[derive(Debug)]
pub struct DB {
    db: rocksdb::DB,
}

impl DB {
    pub fn open(path: &Path, config: &Config) -> Self {
        debug!("opening DB at {:?}", path);

        let mut db_opts = rocksdb::Options::default();
        db_opts.create_if_missing(true);
        db_opts.set_max_open_files(100_000); // make sure to `limit -n` this process correctly
        db_opts.set_compaction_style(rocksdb::DBCompactionStyle::Level);
        db_opts.set_compression_type(rocksdb::DBCompressionType::Snappy);
        db_opts.set_target_file_size_base(1_073_741_824);
        db_opts.set_write_buffer_size(256 << 20);
        db_opts.set_disable_auto_compactions(true);

        // db_opts.set_advise_random_on_open(???);
        db_opts.set_compaction_readahead_size(1 << 20);
        db_opts.increase_parallelism(2);

        let db = Self {
            db: rocksdb::DB::open(&db_opts, path).expect("failed to open RocksDB"),
        };
        db.verify_compatibility(config);

        db
    }

    pub fn get(&self, key: &[u8]) -> Option<Bytes> {
        self.db.get(key).unwrap().map(|v| v.to_vec())
    }

    pub fn write(&self, mut rows: Vec<DBRow>, flush: DBFlush) {
        debug!(
            "writing {} rows to {:?}, flush={:?}",
            rows.len(),
            self.db,
            flush
        );
        rows.sort_unstable_by(|a, b| a.key.cmp(&b.key));
        let mut batch = rocksdb::WriteBatch::default();
        for row in rows {
            #[cfg(not(feature = "oldcpu"))]
            batch.put(&row.key, &row.value);
            #[cfg(feature = "oldcpu")]
            batch.put(&row.key, &row.value).unwrap();
        }
        let do_flush = match flush {
            DBFlush::Enable => true,
            DBFlush::Disable => false,
        };
        let mut opts = rocksdb::WriteOptions::new();
        opts.set_sync(do_flush);
        opts.disable_wal(!do_flush);
        self.db.write_opt(batch, &opts).unwrap();
    }

    pub fn flush(&self) {
        self.db.flush().unwrap();
    }

    pub fn put(&self, key: &[u8], value: &[u8]) {
        self.db.put(key, value).unwrap()
    }

    pub fn iter_scan(&self, prefix: &[u8]) -> ScanIterator {
        ScanIterator {
            prefix: prefix.to_vec(),
            iter: self.db.prefix_iterator(prefix),
            done: false,
        }
    }

    pub fn iter_scan_from(&self, prefix: &[u8], start_at: &[u8]) -> ScanIterator {
        let iter = self.db.iterator(rocksdb::IteratorMode::From(
            start_at,
            rocksdb::Direction::Forward,
        ));

        ScanIterator {
            prefix: prefix.to_vec(),
            iter,
            done: false,
        }
    }

    fn verify_compatibility(&self, config: &Config) {
        let mut compatibility_bytes = bincode::serialize(&DB_VERSION).unwrap();

        if config.light_mode {
            // append a byte to indicate light_mode is enabled.
            // we're not letting bincode serialize this so that the compatibility bytes won't change
            // (and require a reindex) when light_mode is disabled.
            // this should be changed the next time we bump DB_VERSION and require a re-index anyway
            compatibility_bytes.push(1)
        }

        match self.get(b"V") {
            None => self.put(b"V", &compatibility_bytes),
            Some(ref x) if x != &compatibility_bytes => {
                panic!("Incompatible database found. Please reindex");
            }
            Some(_) => (),
        }
    }
}
