//! Checking whether a name is already taken on crates.io.
//!
//! This is deliberately separate from [`generate`](crate::generate), which does
//! no I/O and knows nothing about registries: a caller ranks names first, then
//! asks about the handful it intends to show.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::fs;
use std::io::Write as _;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Whether a crate name is free to publish under.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Availability {
    Available,
    Taken,
    /// The lookup did not complete — offline, timed out, or an unexpected
    /// status. Never treated as an error; the caller shows it as unknown.
    Unknown,
}

/// How long a cached answer is trusted.
//
// a name going from available to taken is the change that matters, and a week
// is short enough that a stale "available" does not send someone very far down
// the wrong path
const CACHE_TTL: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// Simultaneous requests. The sparse index is a CDN built for cargo's own
/// traffic, so this is about latency, not politeness: lookups measured ~300ms
/// each, and `--only-available` can ask about a few hundred names.
const WORKERS: usize = 16;

/// Looks up crate names, memoized on disk.
pub struct Registry {
    agent: ureq::Agent,
    seen: HashMap<String, Availability>,
    cache_path: Option<PathBuf>,
}

impl Default for Registry {
    fn default() -> Registry {
        Registry::new()
    }
}

impl Registry {
    pub fn new() -> Registry {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(10)))
            // crates.io asks clients to identify themselves
            .user_agent(concat!("nomen/", env!("CARGO_PKG_VERSION"), " (availability check)"))
            .build()
            .into();

        let cache_path = cache_path();
        let seen = cache_path.as_deref().map(read_cache).unwrap_or_default();
        Registry { agent, seen, cache_path }
    }

    /// Look up every name, in order. Names already known are not re-fetched.
    pub fn check(&mut self, names: &[&str]) -> Vec<Availability> {
        let pending: Vec<&str> = {
            let mut seen_here = Vec::new();
            names
                .iter()
                .copied()
                .filter(|n| !self.seen.contains_key(*n))
                .filter(|n| {
                    let fresh = !seen_here.contains(n);
                    if fresh {
                        seen_here.push(*n);
                    }
                    fresh
                })
                .collect()
        };

        if !pending.is_empty() {
            let agent = &self.agent;
            let fetched: Vec<(String, Availability)> = std::thread::scope(|scope| {
                let chunk = pending.len().div_ceil(WORKERS).max(1);
                let handles: Vec<_> = pending
                    .chunks(chunk)
                    .map(|batch| scope.spawn(move || {
                        batch.iter().map(|n| ((*n).to_string(), look_up(agent, n))).collect::<Vec<_>>()
                    }))
                    .collect();
                handles.into_iter().flat_map(|h| h.join().unwrap_or_default()).collect()
            });

            for (name, status) in &fetched {
                self.seen.insert(name.clone(), *status);
            }
            // an Unknown says nothing about the name, only about the network
            self.record(fetched.iter().filter(|(_, s)| *s != Availability::Unknown));
        }

        names.iter().map(|n| self.seen.get(*n).copied().unwrap_or(Availability::Unknown)).collect()
    }

    fn record<'a>(&self, entries: impl Iterator<Item = &'a (String, Availability)>) {
        let Some(path) = &self.cache_path else { return };
        let now = unix_now();
        let mut buf = String::new();
        for (name, status) in entries {
            let tag = match status {
                Availability::Available => "free",
                Availability::Taken => "taken",
                Availability::Unknown => continue,
            };
            let _ = writeln!(buf, "{name}\t{tag}\t{now}");
        }
        if buf.is_empty() {
            return;
        }
        // a cache is an optimization; failing to write one is not worth
        // reporting to someone who asked for names
        if let Some(dir) = path.parent() {
            let _ = fs::create_dir_all(dir);
        }
        let _ = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .and_then(|mut f| f.write_all(buf.as_bytes()));
    }
}

fn look_up(agent: &ureq::Agent, name: &str) -> Availability {
    let url = format!("https://index.crates.io/{}", index_path(name));
    match agent.get(&url).call() {
        Ok(_) => Availability::Taken,
        Err(ureq::Error::StatusCode(404)) => Availability::Available,
        Err(_) => Availability::Unknown,
    }
}

/// Path of a crate's entry in the sparse index.
//
// the layout cargo itself uses: names are bucketed by their first four
// characters, with special cases for names too short to fill two buckets.
// see: https://doc.rust-lang.org/cargo/reference/registry-index.html
pub(crate) fn index_path(name: &str) -> String {
    match name.len() {
        0 => String::new(),
        1 => format!("1/{name}"),
        2 => format!("2/{name}"),
        3 => format!("3/{}/{}", &name[..1], name),
        _ => format!("{}/{}/{}", &name[..2], &name[2..4], name),
    }
}

fn cache_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))?;
    Some(base.join("nomen").join("crates.tsv"))
}

fn read_cache(path: &std::path::Path) -> HashMap<String, Availability> {
    let Ok(text) = fs::read_to_string(path) else {
        return HashMap::new();
    };
    let now = unix_now();
    let mut out = HashMap::new();
    // later lines win: the file is append-only, so a re-check of a name
    // appears after the entry it supersedes
    for line in text.lines() {
        let mut parts = line.split('\t');
        let (Some(name), Some(tag), Some(at)) = (parts.next(), parts.next(), parts.next()) else {
            continue;
        };
        let Ok(at) = at.parse::<u64>() else { continue };
        if now.saturating_sub(at) > CACHE_TTL.as_secs() {
            continue;
        }
        let status = match tag {
            "free" => Availability::Available,
            "taken" => Availability::Taken,
            _ => continue,
        };
        out.insert(name.to_string(), status);
    }
    out
}

fn unix_now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_paths_match_the_cargo_layout() {
        assert_eq!(index_path("a"), "1/a");
        assert_eq!(index_path("ab"), "2/ab");
        assert_eq!(index_path("cat"), "3/c/cat");
        assert_eq!(index_path("digs"), "di/gs/digs");
        assert_eq!(index_path("serde"), "se/rd/serde");
    }
}
