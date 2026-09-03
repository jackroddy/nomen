//! Weights persisted between runs.
//!
//! Plain `key value` lines, the same shape as the corpus files — tuning
//! weights is the point of the panel, and a tuned set should survive a restart
//! without pulling in a config-format dependency to do it.

use std::fs;
use std::path::PathBuf;

use nomen::Weights;

fn path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("nomen").join("weights"))
}

pub fn load() -> Option<Weights> {
    let text = fs::read_to_string(path()?).ok()?;
    let mut w = Weights::default();
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        let (Some(key), Some(value)) = (parts.next(), parts.next()) else {
            continue;
        };
        let Ok(v) = value.parse::<f32>() else { continue };
        match key {
            "name" => w.name = v,
            "coverage" => w.coverage = v,
            "usage" => w.usage = v,
            "relation" => w.relation = v,
            "niceness" => w.niceness = v,
            _ => {}
        }
    }
    Some(w)
}

/// Store the weights. Failing to is not worth interrupting anyone over.
pub fn save(w: &Weights) {
    let Some(path) = path() else { return };
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    // two decimals: the panel steps by 0.05, and accumulated float error
    // would otherwise write 2.8999996 into a file meant to be readable
    let body = format!(
        "name {:.2}\ncoverage {:.2}\nusage {:.2}\nrelation {:.2}\nniceness {:.2}\n",
        w.name, w.coverage, w.usage, w.relation, w.niceness
    );
    let _ = fs::write(path, body);
}
