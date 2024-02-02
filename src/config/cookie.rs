use std::{fs, path::PathBuf};

use crate::daemon::CookieGetter;

use crate::errors::*;

pub struct StaticCookie {
    pub value: Vec<u8>,
}

impl CookieGetter for StaticCookie {
    fn get(&self) -> crate::errors::Result<Vec<u8>> {
        Ok(self.value.clone())
    }
}

pub struct CookieFile {
    pub daemon_dir: PathBuf,
}

impl CookieGetter for CookieFile {
    fn get(&self) -> crate::errors::Result<Vec<u8>> {
        let path = self.daemon_dir.join(".cookie");
        let contents = fs::read(&path).chain_err(|| {
            ErrorKind::Connection(format!("failed to read cookie from {:?}", path))
        })?;

        Ok(contents)
    }
}
