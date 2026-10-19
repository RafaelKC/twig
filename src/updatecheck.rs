//! Passive "new version available" hint, printed to stderr after a command.
//!
//! Never slows or breaks a command: the latest release is cached (refreshed at most every
//! `update.check_interval_hours`, with a short timeout) and every failure is silent.

use std::fs;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::config::{home, Update};
use crate::selfmgmt;

const TIMEOUT_SECS: u32 = 2;
const HINT_EVERY_SECS: u64 = 24 * 3600;

/// What is remembered between runs (`~/.cache/twig/latest-version`, `key=value` lines).
#[derive(Debug, Default, Clone, PartialEq)]
pub struct State {
    /// Unix time of the last lookup attempt (successful or not).
    pub checked_at: u64,
    /// Latest release tag seen, e.g. `v0.2.0`.
    pub latest: String,
    /// Version the hint was last shown for, and when.
    pub hinted: String,
    pub hinted_at: u64,
}

impl State {
    pub fn parse(text: &str) -> Self {
        let mut s = State::default();
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            let v = v.trim();
            match k.trim() {
                "checked_at" => s.checked_at = v.parse().unwrap_or(0),
                "latest" => s.latest = v.into(),
                "hinted" => s.hinted = v.into(),
                "hinted_at" => s.hinted_at = v.parse().unwrap_or(0),
                _ => {}
            }
        }
        s
    }

    pub fn render(&self) -> String {
        format!(
            "checked_at={}\nlatest={}\nhinted={}\nhinted_at={}\n",
            self.checked_at, self.latest, self.hinted, self.hinted_at
        )
    }
}

/// True when `TWIG_NO_UPDATE_CHECK` is set to something other than empty/0/false.
pub fn env_disabled() -> bool {
    match std::env::var("TWIG_NO_UPDATE_CHECK") {
        Ok(v) => !matches!(v.trim(), "" | "0" | "false"),
        Err(_) => false,
    }
}

fn cache_file() -> Option<PathBuf> {
    let base = match std::env::var("XDG_CACHE_HOME") {
        Ok(d) if !d.is_empty() => PathBuf::from(d),
        _ => home().ok()?.join(".cache"),
    };
    Some(base.join("twig/latest-version"))
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Decides what to do. Returns the hint (if one should be printed) and the state to persist.
/// `fetch` is only called when the cache is stale.
pub fn evaluate(
    mut state: State,
    now: u64,
    current: &str,
    interval_hours: u64,
    fetch: impl FnOnce() -> Option<String>,
) -> (Option<String>, State) {
    if now.saturating_sub(state.checked_at) >= interval_hours.saturating_mul(3600) {
        state.checked_at = now; // also on failure, so an offline machine doesn't retry on every command
        if let Some(tag) = fetch() {
            state.latest = tag;
        }
    }
    let newer = match (
        selfmgmt::parse_version(&state.latest),
        selfmgmt::parse_version(current),
    ) {
        (Some(l), Some(c)) => l > c,
        _ => false,
    };
    if !newer {
        return (None, state);
    }
    if state.hinted == state.latest && now.saturating_sub(state.hinted_at) < HINT_EVERY_SECS {
        return (None, state);
    }
    state.hinted = state.latest.clone();
    state.hinted_at = now;
    let hint = format!(
        "A new version of twig is available (v{} → {}). Run: twig update",
        current.trim_start_matches('v'),
        state.latest
    );
    (Some(hint), state)
}

/// Full flow with the real cache file and the real lookup. Silent on any failure.
pub fn maybe_hint(cfg: &Update) {
    let tty = std::io::stdout().is_terminal() && std::io::stderr().is_terminal();
    if !cfg.check || env_disabled() || !tty {
        return;
    }
    let Some(file) = cache_file() else {
        return;
    };
    if let Some(hint) = run_with(&file, cfg.check_interval_hours, || {
        selfmgmt::latest_tag_within(TIMEOUT_SECS).ok()
    }) {
        eprintln!("\x1b[33m{hint}\x1b[0m");
    }
}

/// Reads/writes the cache around [`evaluate`]; split out so tests can use a fake lookup.
fn run_with(
    file: &Path,
    interval_hours: u64,
    fetch: impl FnOnce() -> Option<String>,
) -> Option<String> {
    let state = fs::read_to_string(file)
        .map(|t| State::parse(&t))
        .unwrap_or_default();
    let (hint, new) = evaluate(
        state.clone(),
        now(),
        env!("CARGO_PKG_VERSION"),
        interval_hours,
        fetch,
    );
    if new != state {
        if let Some(dir) = file.parent() {
            let _ = fs::create_dir_all(dir);
        }
        let _ = fs::write(file, new.render());
    }
    hint
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_000_000;

    fn cached(latest: &str, age: u64) -> State {
        State {
            checked_at: NOW - age,
            latest: latest.into(),
            ..Default::default()
        }
    }

    fn no_fetch() -> Option<String> {
        panic!("must not hit the network when the cache is fresh")
    }

    #[test]
    fn cached_newer_version_gives_hint() {
        let (hint, st) = evaluate(cached("v9.0.0", 60), NOW, "0.2.0", 24, no_fetch);
        assert_eq!(
            hint.unwrap(),
            "A new version of twig is available (v0.2.0 → v9.0.0). Run: twig update"
        );
        assert_eq!(st.hinted, "v9.0.0");
    }

    #[test]
    fn same_or_older_version_gives_nothing() {
        assert!(evaluate(cached("v0.2.0", 60), NOW, "0.2.0", 24, no_fetch)
            .0
            .is_none());
        assert!(evaluate(cached("v0.1.0", 60), NOW, "0.2.0", 24, no_fetch)
            .0
            .is_none());
    }

    #[test]
    fn offline_gives_nothing_and_is_not_retried_every_command() {
        let (hint, st) = evaluate(State::default(), NOW, "0.2.0", 24, || None);
        assert!(hint.is_none());
        assert_eq!(st.checked_at, NOW);
        // a second run right after does not call fetch again
        let (hint, _) = evaluate(st, NOW + 5, "0.2.0", 24, no_fetch);
        assert!(hint.is_none());
    }

    #[test]
    fn stale_cache_is_refreshed() {
        let old = cached("v0.2.0", 25 * 3600);
        let (hint, st) = evaluate(old, NOW, "0.2.0", 24, || Some("v0.3.0".into()));
        assert!(hint.unwrap().contains("v0.3.0"));
        assert_eq!(st.checked_at, NOW);
    }

    #[test]
    fn failed_refresh_keeps_previous_latest() {
        let old = cached("v0.3.0", 25 * 3600);
        let (hint, _) = evaluate(old, NOW, "0.2.0", 24, || None);
        assert!(hint.is_some());
    }

    #[test]
    fn hint_shown_once_per_version_per_day() {
        let (_, st) = evaluate(cached("v0.3.0", 60), NOW, "0.2.0", 24, no_fetch);
        assert!(evaluate(st.clone(), NOW + 3600, "0.2.0", 24, no_fetch)
            .0
            .is_none());
        assert!(evaluate(st.clone(), NOW + 25 * 3600, "0.2.0", 48, no_fetch)
            .0
            .is_some());
        // a newer release hints again immediately
        let mut st2 = st;
        st2.latest = "v0.4.0".into();
        assert!(evaluate(st2, NOW + 60, "0.2.0", 24, no_fetch).0.is_some());
    }

    #[test]
    fn state_round_trips_and_ignores_garbage() {
        let s = State {
            checked_at: 5,
            latest: "v1.0.0".into(),
            hinted: "v1.0.0".into(),
            hinted_at: 7,
        };
        assert_eq!(State::parse(&s.render()), s);
        assert_eq!(State::parse("nonsense\n=\nchecked_at=x"), State::default());
    }

    #[test]
    fn cache_file_persists_between_runs() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("twig/latest-version");
        let hint = run_with(&file, 24, || Some("v99.0.0".into()));
        assert!(hint.is_some());
        assert!(file.exists());
        // second run: fresh cache, already hinted, no lookup
        assert!(run_with(&file, 24, no_fetch).is_none());
    }

    #[test]
    fn disabled_and_non_tty_skip_everything() {
        // under `cargo test` stdout/stderr are captured (not a TTY), so this must be a no-op
        // and must not touch the cache or network.
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("XDG_CACHE_HOME", dir.path());
        maybe_hint(&Update::default());
        assert!(!dir.path().join("twig/latest-version").exists());
        maybe_hint(&Update {
            check: false,
            check_interval_hours: 24,
        });
        assert!(!dir.path().join("twig/latest-version").exists());
    }
}
