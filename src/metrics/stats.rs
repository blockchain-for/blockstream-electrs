use std::fs;

use crate::errors::*;

pub struct Stats {
    pub utime: f64,
    pub rss: u64,
    pub fds: usize,
}

pub fn parse_stats() -> Result<Stats> {
    if cfg!(target_os = "macos") {
        return Ok(Stats {
            utime: 0f64,
            rss: 0u64,
            fds: 0usize,
        });
    }
    let value = fs::read_to_string("/proc/self/stat").chain_err(|| "failed to read stats")?;
    let parts: Vec<&str> = value.split_whitespace().collect();
    let page_size = page_size::get() as u64;
    let ticks_per_second = sysconf::raw::sysconf(sysconf::raw::SysconfVariable::ScClkTck)
        .expect("failed to get _SC_CLK_TCK") as f64;

    let parse_part = |index: usize, name: &str| -> Result<u64> {
        Ok(parts
            .get(index)
            .chain_err(|| format!("missing {}: {:?}", name, parts))?
            .parse::<u64>()
            .chain_err(|| format!("invalid {}: {:?}", name, parts))?)
    };

    // For details, see '/proc/[pid]/stat' section at `man 5 proc`:
    let utime = parse_part(13, "utime")? as f64 / ticks_per_second;
    let rss = parse_part(23, "rss")? * page_size;
    let fds = fs::read_dir("/proc/self/fd")
        .chain_err(|| "failed to read fd directory")?
        .count();
    Ok(Stats { utime, rss, fds })
}
