//! The set of crate names already taken on crates.io.
//!
//! crates.io publishes a full database dump. Downloading it whole would be
//! 1815 MB, but `data/crates.csv` and `data/reserved_crate_names.csv` are the
//! 10th and 14th members of the tar, both ahead of the multi-gigabyte version
//! tables — so streaming and stopping there costs about 356 MB and yields
//! every name at once.
//!
//! That is worth the download: one snapshot answers for all ~77k candidates a
//! search can return, where per-name lookups could only cover the handful on
//! screen. It also gets reserved names right, which a per-name 404 check
//! cannot — an unpublished reserved name is not available.

use std::collections::HashSet;
use std::fs;
use std::io::{self, BufRead, BufReader, Write};
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::registry::Availability;

/// crates.io rebuilds the dump daily, but a refresh costs 356 MB, and a name
/// taken since the last one still shows free only until the caller asks for a
/// live re-check. A week trades staleness for bandwidth in the right direction.
pub const REFRESH_AFTER: Duration = Duration::from_secs(7 * 24 * 60 * 60);

const DUMP_URL: &str = "https://static.crates.io/db-dump.tar.gz";

/// Every crate name on crates.io, as of [`Snapshot::fetched_at`].
pub struct Snapshot {
    taken: HashSet<String>,
    fetched_at: SystemTime,
}

impl Snapshot {
    /// Read the stored snapshot, if there is one.
    pub fn load() -> Option<Snapshot> {
        let path = path()?;
        let file = BufReader::new(fs::File::open(path).ok()?);
        let mut lines = file.lines();
        let stamp: u64 = lines.next()?.ok()?.trim().parse().ok()?;
        let taken = lines.map_while(Result::ok).filter(|l| !l.is_empty()).collect();
        Some(Snapshot { taken, fetched_at: UNIX_EPOCH + Duration::from_secs(stamp) })
    }

    /// Download a fresh snapshot. Does not store it; call [`Snapshot::save`].
    pub fn fetch() -> io::Result<Snapshot> {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(600)))
            .user_agent(concat!("nomen/", env!("CARGO_PKG_VERSION"), " (availability snapshot)"))
            .build()
            .into();
        let resp = agent
            .get(DUMP_URL)
            .call()
            .map_err(|e| io::Error::other(format!("fetching the crates.io dump: {e}")))?;

        let gz = flate2::read::GzDecoder::new(resp.into_body().into_reader());
        let mut archive = tar::Archive::new(gz);

        let mut taken = HashSet::new();
        let mut found = 0;
        for entry in archive.entries()? {
            let entry = entry?;
            let path = entry.path()?.to_path_buf();
            let base = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
            if !matches!(base, "crates.csv" | "reserved_crate_names.csv") {
                continue;
            }
            for name in name_column(BufReader::with_capacity(1 << 20, entry))? {
                taken.insert(normalize(&name));
            }
            found += 1;
            // stop as soon as both are in hand, leaving the rest of the
            // archive — the great majority of it — undownloaded
            if found == 2 {
                break;
            }
        }
        if taken.is_empty() {
            return Err(io::Error::other("the dump contained no crate names"));
        }
        Ok(Snapshot { taken, fetched_at: SystemTime::now() })
    }

    /// Store the snapshot for later runs.
    pub fn save(&self) -> io::Result<()> {
        let Some(path) = path() else {
            return Ok(());
        };
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        // write beside the target and rename, so an interrupted save cannot
        // truncate a good snapshot
        let tmp = path.with_extension("tmp");
        {
            let mut out = io::BufWriter::new(fs::File::create(&tmp)?);
            writeln!(out, "{}", self.fetched_at.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs())?;
            for name in &self.taken {
                out.write_all(name.as_bytes())?;
                out.write_all(b"\n")?;
            }
            out.flush()?;
        }
        fs::rename(&tmp, &path)
    }

    pub fn status(&self, name: &str) -> Availability {
        if self.taken.contains(&normalize(name)) {
            Availability::Taken
        } else {
            Availability::Available
        }
    }

    pub fn age(&self) -> Duration {
        SystemTime::now().duration_since(self.fetched_at).unwrap_or_default()
    }

    pub fn is_stale(&self) -> bool {
        self.age() > REFRESH_AFTER
    }

    pub fn len(&self) -> usize {
        self.taken.len()
    }

    pub fn is_empty(&self) -> bool {
        self.taken.is_empty()
    }
}

/// Fold a crate name the way crates.io compares them.
//
// registry names are case-insensitive and treat `-` and `_` as the same
// character, so `Foo-Bar` and `foo_bar` are one name. Our candidates are
// always plain lowercase letters, but the taken side is not.
pub fn normalize(name: &str) -> String {
    name.trim().to_ascii_lowercase().replace('-', "_")
}

fn path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))?;
    Some(base.join("nomen").join("taken.txt"))
}

/// Path the snapshot is stored at, for reporting.
pub fn stored_at() -> Option<PathBuf> {
    path()
}

// ---

/// Read the `name` column out of an RFC 4180 CSV stream.
//
// **note: a real parser is required here, not a split on commas. The dump's
// description column carries embedded commas, quotes and newlines -- which is
// why crates.csv is 26 million lines long while holding 327k crates.
fn name_column(reader: impl BufRead) -> io::Result<Vec<String>> {
    let mut csv = Csv { reader, pending: None };
    let Some(header) = csv.record()? else {
        return Ok(Vec::new());
    };
    // reserved_crate_names.csv has a single column, also called `name`
    let column = header.iter().position(|h| h == "name").unwrap_or(0);

    let mut names = Vec::new();
    while let Some(record) = csv.record()? {
        if let Some(name) = record.get(column)
            && !name.is_empty()
        {
            names.push(name.clone());
        }
    }
    Ok(names)
}

struct Csv<R> {
    reader: R,
    pending: Option<u8>,
}

impl<R: BufRead> Csv<R> {
    fn byte(&mut self) -> io::Result<Option<u8>> {
        if let Some(b) = self.pending.take() {
            return Ok(Some(b));
        }
        let buf = self.reader.fill_buf()?;
        if buf.is_empty() {
            return Ok(None);
        }
        let b = buf[0];
        self.reader.consume(1);
        Ok(Some(b))
    }

    /// The next record, or `None` at end of input.
    fn record(&mut self) -> io::Result<Option<Vec<String>>> {
        let mut fields: Vec<String> = Vec::new();
        let mut field: Vec<u8> = Vec::new();
        let mut quoted = false;
        let mut any = false;

        loop {
            let Some(b) = self.byte()? else {
                if !any {
                    return Ok(None);
                }
                fields.push(String::from_utf8_lossy(&field).into_owned());
                return Ok(Some(fields));
            };
            any = true;

            match b {
                b'"' if quoted => match self.byte()? {
                    // a doubled quote inside a quoted field is one quote
                    Some(b'"') => field.push(b'"'),
                    other => {
                        quoted = false;
                        self.pending = other;
                    }
                },
                b'"' if field.is_empty() => quoted = true,
                b',' if !quoted => {
                    fields.push(String::from_utf8_lossy(&field).into_owned());
                    field.clear();
                }
                b'\n' if !quoted => {
                    fields.push(String::from_utf8_lossy(&field).into_owned());
                    return Ok(Some(fields));
                }
                b'\r' if !quoted => {}
                _ => field.push(b),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_survive_quoted_fields_with_commas_and_newlines() {
        // the shape that makes a naive comma split wrong
        let csv = "id,name,description\n\
                   1,serde,\"a framework, for serializing\"\n\
                   2,tokio,\"an async runtime\nspanning lines\"\n\
                   3,ratatui,\"quote \"\"inside\"\" the text\"\n";
        let names = name_column(io::Cursor::new(csv)).unwrap();
        assert_eq!(names, ["serde", "tokio", "ratatui"]);
    }

    #[test]
    fn a_single_column_file_still_parses() {
        let names = name_column(io::Cursor::new("name\nalpha\nbeta\n")).unwrap();
        assert_eq!(names, ["alpha", "beta"]);
    }

    #[test]
    fn normalization_matches_how_the_registry_compares_names() {
        assert_eq!(normalize("Foo-Bar"), "foo_bar");
        assert_eq!(normalize("foo_bar"), "foo_bar");
        assert_eq!(normalize(" SERDE "), "serde");
    }

    #[test]
    fn a_dashed_crate_blocks_its_underscored_spelling() {
        let snap = Snapshot {
            taken: ["foo_bar".to_string()].into_iter().collect(),
            fetched_at: SystemTime::now(),
        };
        assert_eq!(snap.status("Foo-Bar"), Availability::Taken);
        assert_eq!(snap.status("foo_bar"), Availability::Taken);
        assert_eq!(snap.status("foobar"), Availability::Available);
    }

    #[test]
    fn staleness_follows_the_refresh_window() {
        let fresh = Snapshot { taken: HashSet::new(), fetched_at: SystemTime::now() };
        assert!(!fresh.is_stale());
        let old = Snapshot {
            taken: HashSet::new(),
            fetched_at: SystemTime::now() - REFRESH_AFTER - Duration::from_secs(60),
        };
        assert!(old.is_stale());
    }
}
