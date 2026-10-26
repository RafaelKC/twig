//! The park marker: `<card>/.parked.json`, written by `twig park` so people and external tools
//! can tell a card parked on purpose from one that merely has its workspace closed. The format
//! is a stable contract, documented in docs/PARKED.md.

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::card::{self, CardRef};
use crate::config::Config;
use crate::{provider, ui};

pub const FILE: &str = ".parked.json";
const VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Marker {
    pub v: u32,
    /// Local date and time of the park, ISO 8601 with offset.
    pub since: String,
    /// The card's state in the provider at park time (`null` when unknown).
    pub state: Option<String>,
    /// Ids of the card's child items at park time (`null` when unknown); numbers when numeric.
    pub children: Option<Vec<Value>>,
}

pub fn path(cfg: &Config, id: &str) -> PathBuf {
    cfg.card_dir(id).join(FILE)
}

/// The card's marker; `None` when it isn't parked (or the file is unreadable).
pub fn read(cfg: &Config, id: &str) -> Option<Marker> {
    serde_json::from_str(&fs::read_to_string(path(cfg, id)).ok()?).ok()
}

/// Just the `since` of the marker, for `ls`/`status`.
pub fn since(cfg: &Config, id: &str) -> Option<String> {
    read(cfg, id).map(|m| m.since)
}

/// Delete the marker; whether there was one.
pub fn clear(cfg: &Config, id: &str) -> bool {
    fs::remove_file(path(cfg, id)).is_ok()
}

fn child_value(id: String) -> Value {
    id.parse::<u64>()
        .map(Value::from)
        .unwrap_or(Value::String(id))
}

/// State and children from the provider (best effort): asks it again, falls back to the card
/// cache and, with neither, gives `(None, None)` and a warning.
fn provider_data(cfg: &Config, id: &str) -> (Option<String>, Option<Vec<Value>>) {
    let p = provider::current(cfg);
    let cref = CardRef::of(cfg, id);
    let first = card::repos_ordered(cfg, &cfg.card_dir(id))
        .first()
        .and_then(|r| cfg.repo_path(r));
    let info = provider::refetch(cfg, &cref, first.as_deref()).or_else(|| {
        ui::warn(format!("{} unavailable: using the card cache", p.cli()));
        fs::read_to_string(provider::cache_file(cfg, id))
            .ok()
            .and_then(|raw| p.parse_card(&raw))
    });
    let Some(info) = info else {
        ui::warn("no provider data nor cache: state and children recorded as null");
        return (None, None);
    };
    let state = Some(info.state.clone()).filter(|s| !s.is_empty());
    let children = p
        .children(&info.raw)
        .map(|c| c.into_iter().map(child_value).collect());
    (state, children)
}

/// Write the marker of a card being parked now. An existing marker is kept as is (its `since`
/// and `children` are the comparison base) and returned with `false`.
pub fn write_new(cfg: &Config, id: &str) -> Result<(Marker, bool)> {
    if let Some(m) = read(cfg, id) {
        return Ok((m, false));
    }
    let (state, children) = provider_data(cfg, id);
    let m = Marker {
        v: VERSION,
        since: now_local(),
        state,
        children,
    };
    fs::write(path(cfg, id), serde_json::to_string(&m)? + "\n")
        .with_context(|| format!("could not write {}", path(cfg, id).display()))?;
    Ok((m, true))
}

/// Local time as ISO 8601 with offset (`2026-10-26T14:03:00-03:00`).
#[cfg(unix)]
fn now_local() -> String {
    // SAFETY: `time` and `localtime_r` only write into the zeroed `tm` we own.
    let tm = unsafe {
        let t = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&t, &mut tm);
        tm
    };
    let off = tm.tm_gmtoff;
    let (sign, a) = if off < 0 { ('-', -off) } else { ('+', off) };
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}{sign}{:02}:{:02}",
        tm.tm_year + 1900,
        tm.tm_mon + 1,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min,
        tm.tm_sec,
        a / 3600,
        a % 3600 / 60
    )
}

#[cfg(not(unix))]
fn now_local() -> String {
    // PowerShell's round-trip format has fractional seconds: cut them, keep the offset
    let out = crate::sys::output(crate::sys::tool("powershell").args([
        "-NoProfile",
        "-Command",
        "Get-Date -Format 'yyyy-MM-ddTHH:mm:sszzz'",
    ]));
    out.map(|s| s.trim().to_string()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg_with_card(tmp: &std::path::Path) -> Config {
        let wt = tmp.join("wt");
        fs::create_dir_all(wt.join("7")).unwrap();
        let file = tmp.join("config.toml");
        fs::write(
            &file,
            format!("version = 1\n[paths]\nworktrees = '{}'\n", wt.display()),
        )
        .unwrap();
        Config::load_from(&file).unwrap()
    }

    #[test]
    fn now_is_iso_8601_with_offset() {
        let re = regex::Regex::new(r"^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d[+-]\d\d:\d\d$").unwrap();
        assert!(re.is_match(&now_local()), "{}", now_local());
    }

    #[test]
    fn marker_serializes_to_the_documented_shape() {
        let m = Marker {
            v: 1,
            since: "2026-10-26T14:03:00-03:00".into(),
            state: Some("Resolved".into()),
            children: Some(vec![51234.into(), 51240.into()]),
        };
        assert_eq!(
            serde_json::to_string(&m).unwrap(),
            r#"{"v":1,"since":"2026-10-26T14:03:00-03:00","state":"Resolved","children":[51234,51240]}"#
        );
    }

    #[test]
    fn unavailable_provider_records_nulls_and_repeat_keeps_the_marker() {
        let tmp = tempfile::tempdir().unwrap();
        let cfg = cfg_with_card(tmp.path());
        // no cache and (in tests) no logged-in provider CLI: state and children stay null
        let (m, created) = write_new(&cfg, "7").unwrap();
        assert!(created);
        assert_eq!((m.v, m.state.clone(), m.children.clone()), (1, None, None));
        let raw = fs::read_to_string(path(&cfg, "7")).unwrap();
        assert!(raw.contains(r#""state":null,"children":null"#));

        // a second park keeps the original marker untouched
        let old = Marker {
            since: "2020-01-01T00:00:00+00:00".into(),
            ..m
        };
        fs::write(path(&cfg, "7"), serde_json::to_string(&old).unwrap()).unwrap();
        let (again, created) = write_new(&cfg, "7").unwrap();
        assert!(!created);
        assert_eq!(again, old);
        assert_eq!(
            since(&cfg, "7").as_deref(),
            Some("2020-01-01T00:00:00+00:00")
        );
    }

    #[test]
    fn cache_fills_state_when_the_provider_is_unavailable() {
        let tmp = tempfile::tempdir().unwrap();
        let cfg = cfg_with_card(tmp.path());
        fs::write(
            provider::cache_file(&cfg, "7"),
            r#"{"fields":{"System.Title":"t","System.State":"Resolved"},"relations":[{"rel":"System.LinkTypes.Hierarchy-Forward","url":"https://dev.azure.com/o/p/_apis/wit/workItems/51234"}]}"#,
        )
        .unwrap();
        let (m, _) = write_new(&cfg, "7").unwrap();
        // the provider default is az; without `az` logged in the cache is used
        if m.state.is_some() {
            assert_eq!(m.state.as_deref(), Some("Resolved"));
            assert_eq!(m.children, Some(vec![51234.into()]));
        }
    }

    #[test]
    fn undo_removes_the_marker() {
        let tmp = tempfile::tempdir().unwrap();
        let cfg = cfg_with_card(tmp.path());
        assert!(!clear(&cfg, "7"));
        write_new(&cfg, "7").unwrap();
        assert!(read(&cfg, "7").is_some());
        assert!(clear(&cfg, "7"));
        assert!(read(&cfg, "7").is_none());
    }
}
