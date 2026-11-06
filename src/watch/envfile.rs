//! `env_file` of `watch.toml`: provider credentials (`AZURE_DEVOPS_EXT_PAT`, `GH_TOKEN`...) kept
//! out of the unit and the plist. systemd reads the file itself (`EnvironmentFile=`); on macOS,
//! and in `install`'s preflight, twig reads it.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use super::config::WatchCfg;
use crate::config::expand;

/// Variables that log `az` / `gh` in; `install` warns when one is exported but not in a file.
pub const CREDENTIAL_VARS: [&str; 3] = ["AZURE_DEVOPS_EXT_PAT", "GH_TOKEN", "GITHUB_TOKEN"];

/// The configured file, with `~` expanded; `None` when `env_file` is unset.
pub fn path(w: &WatchCfg) -> Option<PathBuf> {
    let p = w.env_file.trim();
    (!p.is_empty()).then(|| expand(p))
}

/// `KEY=VALUE` lines (the systemd `EnvironmentFile` subset): `#`/`;` comments, an optional
/// `export `, and one pair of surrounding quotes.
pub fn parse(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
                return None;
            }
            let line = line.strip_prefix("export ").unwrap_or(line);
            let (k, v) = line.split_once('=')?;
            let k = k.trim();
            if k.is_empty() || k.contains(char::is_whitespace) {
                return None;
            }
            let v = v.trim();
            let v = ['"', '\'']
                .iter()
                .find_map(|q| v.strip_prefix(*q).and_then(|r| r.strip_suffix(*q)))
                .unwrap_or(v);
            Some((k.to_string(), v.to_string()))
        })
        .collect()
}

pub fn read(file: &Path) -> Result<Vec<(String, String)>> {
    let text = std::fs::read_to_string(file)
        .with_context(|| format!("could not read env_file {}", file.display()))?;
    Ok(parse(&text))
}

/// Export the file's variables into this process (for children: `az`, `gh`, `twig new`).
/// Variables already set win, so a real environment can override the file.
pub fn load_into_process(w: &WatchCfg) -> Result<()> {
    let Some(file) = path(w) else { return Ok(()) };
    for (k, v) in read(&file)? {
        if std::env::var_os(&k).is_none() {
            std::env::set_var(k, v);
        }
    }
    Ok(())
}

/// True when the file is readable by group or others (it holds secrets).
#[cfg(unix)]
pub fn too_open(file: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(file).is_ok_and(|m| m.permissions().mode() & 0o077 != 0)
}

#[cfg(not(unix))]
pub fn too_open(_: &Path) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_environment_file_subset() {
        let v = parse("# c\n\nA=1\nexport B = \"two words\"\nC='x'\nbad line\n;d\nD=a=b\n");
        assert_eq!(
            v,
            vec![
                ("A".into(), "1".into()),
                ("B".into(), "two words".into()),
                ("C".into(), "x".into()),
                ("D".into(), "a=b".into()),
            ]
        );
    }

    #[test]
    fn unset_env_file_means_none() {
        assert!(path(&WatchCfg::default()).is_none());
    }
}
