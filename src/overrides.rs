//! Config layers on top of the user config, in increasing precedence:
//!
//! 1. `.twig.toml` versioned in a repo: that repo's settings (`run`, `install`, `port`...);
//! 2. `<card>/.twig.toml`: `[repos.<name>]` settings for one card;
//! 3. `TWIG__<SECTION>__<KEY>` environment variables (any key, e.g. `TWIG__GIT__BASE_BRANCH`).
//!
//! CLI flags still win over all of them. The layers are applied to the TOML document before it
//! is deserialized, so they go through the same validation as the config file.

use std::fs;
use std::path::Path;

use anyhow::{bail, Context, Result};
use toml_edit::{DocumentMut, Item, Table};

use crate::config_kv;

/// Name of the repo and card override files.
pub const FILE: &str = ".twig.toml";
pub const ENV_PREFIX: &str = "TWIG__";

/// Repo settings a `.twig.toml` may change. Not `path`/`default` (they are about your machine
/// and your cards, not the repo) nor `hooks` (personal workflow, kept in your own config).
pub const REPO_KEYS: [&str; 11] = [
    "label",
    "short",
    "run",
    "autorun",
    "port",
    "ports",
    "env",
    "install",
    "post_create",
    "local_files",
    "debug",
];

/// `TWIG__GIT__BASE_BRANCH=develop`: the variable, its key segments and the raw value.
#[derive(Debug, Clone)]
pub struct EnvOverride {
    pub var: String,
    segments: Vec<String>,
    raw: String,
}

impl EnvOverride {
    fn is_repo(&self) -> bool {
        self.segments.first().is_some_and(|s| s == "repos")
    }
}

/// The `TWIG__*` variables of the environment, sorted (so the result doesn't depend on order).
pub fn from_env() -> Vec<EnvOverride> {
    let mut v: Vec<EnvOverride> = std::env::vars()
        .filter_map(|(var, raw)| {
            let rest = var.strip_prefix(ENV_PREFIX)?;
            let segments: Vec<String> = rest.split("__").map(str::to_lowercase).collect();
            if segments.iter().any(String::is_empty) {
                return None;
            }
            Some(EnvOverride {
                var: var.clone(),
                segments,
                raw,
            })
        })
        .collect();
    v.sort_by(|a, b| a.var.cmp(&b.var));
    v
}

/// Env var names can't have `-` or lower case: a segment matches an existing key ignoring case
/// and treating `_` and `-` alike (`MY_API` → `my-api`).
fn same_key(segment: &str, key: &str) -> bool {
    key.to_lowercase().replace('-', "_") == segment
}

fn resolve(doc: &DocumentMut, segments: &[String]) -> Vec<String> {
    let mut item = Some(doc.as_item());
    segments
        .iter()
        .map(|seg| {
            let found = item
                .and_then(Item::as_table_like)
                .and_then(|t| t.iter().map(|(k, _)| k).find(|k| same_key(seg, k)))
                .map(str::to_string);
            let key = found.unwrap_or_else(|| seg.clone());
            item = item.and_then(|i| i.get(&key));
            key
        })
        .collect()
}

/// Apply the env overrides (only the `repos.*` ones with `repos_only`). Each value is read the
/// same way `twig config set` reads it: the first reading that `valid` accepts wins.
pub fn apply_env(
    doc: &mut DocumentMut,
    overrides: &[EnvOverride],
    repos_only: bool,
    valid: impl Fn(&DocumentMut) -> bool,
) -> Result<()> {
    for o in overrides.iter().filter(|o| !repos_only || o.is_repo()) {
        let path = resolve(doc, &o.segments);
        let parts: Vec<&str> = path.iter().map(String::as_str).collect();
        let mut chosen = None;
        for value in config_kv::candidates(&o.raw) {
            let mut next = doc.clone();
            config_kv::set_in(next.as_item_mut(), &parts, value, "")
                .with_context(|| format!("{}: can't set `{}`", o.var, path.join(".")))?;
            let ok = valid(&next);
            if chosen.is_none() || ok {
                chosen = Some(next);
            }
            if ok {
                break;
            }
        }
        if let Some(next) = chosen {
            *doc = next;
        }
    }
    Ok(())
}

fn read(file: &Path) -> Result<Option<DocumentMut>> {
    let Ok(text) = fs::read_to_string(file) else {
        return Ok(None);
    };
    text.parse::<DocumentMut>()
        .map(Some)
        .with_context(|| format!("invalid TOML: {}", file.display()))
}

fn check_repo_keys(t: &Table, file: &Path, scope: &str) -> Result<()> {
    for (k, _) in t.iter() {
        if !REPO_KEYS.contains(&k) {
            bail!(
                "{}: `{scope}{k}` can't be overridden here (allowed: {})",
                file.display(),
                REPO_KEYS.join(", ")
            );
        }
    }
    Ok(())
}

fn merge_into(doc: &mut DocumentMut, repo: &str, t: &Table) {
    if doc.get("repos").is_none() {
        let mut repos = Table::new();
        repos.set_implicit(true);
        doc["repos"] = Item::Table(repos);
    }
    if doc["repos"].get(repo).is_none() {
        doc["repos"][repo] = Item::Table(Table::new());
    }
    for (k, v) in t.iter() {
        doc["repos"][repo][k] = v.clone();
    }
}

/// A repo's `.twig.toml` (keys at the root, see [`REPO_KEYS`]) into `repos.<repo>`.
pub fn merge_repo_file(doc: &mut DocumentMut, repo: &str, file: &Path) -> Result<()> {
    let Some(over) = read(file)? else {
        return Ok(());
    };
    check_repo_keys(over.as_table(), file, "")?;
    merge_into(doc, repo, over.as_table());
    Ok(())
}

/// A card's `.twig.toml`: only `[repos.<name>]` tables.
pub fn merge_card_file(doc: &mut DocumentMut, file: &Path) -> Result<()> {
    let Some(over) = read(file)? else {
        return Ok(());
    };
    for (k, v) in over.as_table().iter() {
        // `[herdr.layout]` is read by `layout::read_card_file`
        if k == "herdr" {
            continue;
        }
        let Some(repos) = (k == "repos").then(|| v.as_table()).flatten() else {
            bail!(
                "{}: only [repos.<name>] tables and [herdr.layout] are allowed in a card's {FILE} (found `{k}`)",
                file.display()
            );
        };
        for (repo, t) in repos.iter() {
            let t = t
                .as_table()
                .with_context(|| format!("{}: `repos.{repo}` must be a table", file.display()))?;
            check_repo_keys(t, file, &format!("repos.{repo}."))?;
            merge_into(doc, repo, t);
        }
    }
    Ok(())
}

/// Validate a repo's `.twig.toml` on its own (for `twig doctor`).
pub fn check_repo_file(file: &Path) -> Result<bool> {
    let Some(over) = read(file)? else {
        return Ok(false);
    };
    check_repo_keys(over.as_table(), file, "")?;
    let mut doc = DocumentMut::new();
    merge_into(&mut doc, "r", over.as_table());
    #[derive(serde::Deserialize)]
    struct Repos {
        #[allow(dead_code)]
        repos: indexmap::IndexMap<String, crate::config::Repo>,
    }
    toml::from_str::<Repos>(&doc.to_string())
        .with_context(|| format!("invalid {}", file.display()))?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(var: &str, raw: &str) -> EnvOverride {
        EnvOverride {
            var: var.into(),
            segments: var[ENV_PREFIX.len()..]
                .split("__")
                .map(str::to_lowercase)
                .collect(),
            raw: raw.into(),
        }
    }

    #[test]
    fn env_segments_match_existing_keys_with_dashes_and_case() {
        let mut doc: DocumentMut =
            "[repos.my-api]\nrun = \"a\"\n[git.branch_prefix]\nBug = \"fix\"\n"
                .parse()
                .unwrap();
        let o = [
            env("TWIG__REPOS__MY_API__RUN", "b"),
            env("TWIG__GIT__BRANCH_PREFIX__BUG", "bugfix"),
            env("TWIG__REPOS__MY_API__PORT", "4200"),
        ];
        apply_env(&mut doc, &o, false, |_| true).unwrap();
        assert_eq!(doc["repos"]["my-api"]["run"].as_str(), Some("b"));
        assert_eq!(doc["repos"]["my-api"]["port"].as_integer(), Some(4200));
        assert_eq!(doc["git"]["branch_prefix"]["Bug"].as_str(), Some("bugfix"));
    }

    #[test]
    fn repos_only_skips_other_sections() {
        let mut doc = DocumentMut::new();
        let o = [
            env("TWIG__GIT__BASE_BRANCH", "dev"),
            env("TWIG__REPOS__X__RUN", "r"),
        ];
        apply_env(&mut doc, &o, true, |_| true).unwrap();
        assert!(doc.get("git").is_none());
        assert_eq!(doc["repos"]["x"]["run"].as_str(), Some("r"));
    }

    #[test]
    fn repo_file_rejects_keys_that_are_not_repo_settings() {
        let tmp = tempfile::tempdir().unwrap();
        let f = tmp.path().join(FILE);
        fs::write(&f, "run = \"x\"\npath = \"/etc\"\n").unwrap();
        let err = merge_repo_file(&mut DocumentMut::new(), "r", &f)
            .unwrap_err()
            .to_string();
        assert!(err.contains("`path` can't be overridden"), "{err}");

        fs::write(&f, "[hooks]\npre_rm = [\"x\"]\n").unwrap();
        assert!(merge_repo_file(&mut DocumentMut::new(), "r", &f).is_err());
    }

    #[test]
    fn the_example_repo_file_is_valid() {
        let f = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/repo.twig.toml");
        assert!(check_repo_file(&f).unwrap());
    }

    #[test]
    fn card_file_only_takes_repo_tables() {
        let tmp = tempfile::tempdir().unwrap();
        let f = tmp.path().join(FILE);
        fs::write(&f, "[repos.api]\nport = 6000\n").unwrap();
        let mut doc = DocumentMut::new();
        merge_card_file(&mut doc, &f).unwrap();
        assert_eq!(doc["repos"]["api"]["port"].as_integer(), Some(6000));

        fs::write(&f, "[git]\nbase_branch = \"x\"\n").unwrap();
        let err = merge_card_file(&mut doc, &f).unwrap_err().to_string();
        assert!(err.contains("only [repos.<name>]"), "{err}");

        fs::write(&f, "[herdr.layout]\nagent = \"columns\"\n").unwrap();
        merge_card_file(&mut doc, &f).unwrap();
    }
}
