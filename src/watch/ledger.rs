//! The watch ledger: which items were already started, so a restart (or a crash) never starts
//! them again. `<state>/twig/watch.json`, timestamps in unix seconds.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

const VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    /// Recorded before `twig new` runs: still this after a crash, and then it is not retried.
    Pending,
    Ok,
    Fail,
    /// The card was already there (or parked): nothing was started.
    Exists,
}

impl Status {
    pub fn name(self) -> &'static str {
        match self {
            Status::Pending => "pending",
            Status::Ok => "ok",
            Status::Fail => "fail",
            Status::Exists => "exists",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub scope: String,
    pub tag: String,
    pub template: String,
    pub result: Status,
    pub at: u64,
    pub exit: Option<i32>,
    pub log: Option<String>,
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Ledger {
    pub v: u32,
    pub entries: BTreeMap<String, Entry>,
}

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// `<state>/twig`: where the ledger, the lock and the logs live (`data_dir` where there is no
/// state dir: macOS, Windows).
pub fn state_dir() -> Result<PathBuf> {
    dirs::state_dir()
        .or_else(dirs::data_dir)
        .map(|d| d.join("twig"))
        .context("user state folder not found")
}

pub fn path() -> Result<PathBuf> {
    Ok(state_dir()?.join("watch.json"))
}

impl Ledger {
    /// The ledger at `file`; empty when it does not exist. A corrupt file is an error, never an
    /// empty ledger: that would start every tagged item again.
    pub fn load(file: &Path) -> Result<Self> {
        let text = match fs::read_to_string(file) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(e).with_context(|| format!("could not read {}", file.display())),
        };
        serde_json::from_str(&text).with_context(|| {
            format!(
                "{} is corrupt (fix or delete it; deleting starts every tagged item again)",
                file.display()
            )
        })
    }

    /// Write through a temporary file, so a crash never leaves half a ledger.
    pub fn save(&mut self, file: &Path) -> Result<()> {
        self.v = VERSION;
        if let Some(dir) = file.parent() {
            fs::create_dir_all(dir)?;
        }
        let tmp = file.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_string_pretty(self)? + "\n")
            .with_context(|| format!("could not write {}", tmp.display()))?;
        fs::rename(&tmp, file).with_context(|| format!("could not write {}", file.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(result: Status) -> Entry {
        Entry {
            scope: "P".into(),
            tag: "agent".into(),
            template: "t".into(),
            result,
            at: 5,
            exit: None,
            log: None,
        }
    }

    #[test]
    fn roundtrips_and_a_missing_file_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("sub/watch.json");
        assert_eq!(Ledger::load(&f).unwrap(), Ledger::default());
        let mut l = Ledger::default();
        l.entries.insert("7".into(), entry(Status::Pending));
        l.save(&f).unwrap();
        let back = Ledger::load(&f).unwrap();
        assert_eq!(back.entries["7"].result, Status::Pending);
        assert_eq!(back.v, VERSION);
    }

    #[test]
    fn a_corrupt_file_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("watch.json");
        fs::write(&f, "{nope").unwrap();
        assert!(Ledger::load(&f)
            .unwrap_err()
            .to_string()
            .contains("corrupt"));
    }
}
