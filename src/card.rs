//! Card folder, worktrees and branch resolution.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use regex::Regex;

use crate::config::{expand, Config};
use crate::{provider, sys, ui};

/// Accept "50306-dispositivos" as "50306" + slug "dispositivos".
pub fn split_id(raw: &str) -> (String, Option<String>) {
    let re = Regex::new(r"^([0-9]+)-(.+)$").unwrap();
    match re.captures(raw) {
        Some(c) => (c[1].to_string(), Some(c[2].to_string())),
        None => (raw.to_string(), None),
    }
}

/// Marker in the card folder with the GitHub issue the card was created from (`owner/repo#123`).
const ISSUE_MARKER: &str = ".twig-issue";

/// A card as the provider sees it. `key` names the card folder and the herdr workspace;
/// `number` is the work item / issue number; `repo` is the GitHub `owner/repo` that owns the
/// issue, when the card was created from a repo-scoped id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardRef {
    pub key: String,
    pub number: String,
    pub repo: Option<String>,
}

/// `owner/repo#123` as written by [`CardRef::record`].
fn parse_marker(key: &str, raw: &str) -> Option<CardRef> {
    let (repo, number) = raw.rsplit_once('#')?;
    (!repo.is_empty() && !number.is_empty()).then(|| CardRef {
        key: key.to_string(),
        number: number.to_string(),
        repo: Some(repo.to_string()),
    })
}

impl CardRef {
    /// The card stored in `<worktrees>/<key>/` (its marker, else the key is the number).
    pub fn of(cfg: &Config, key: &str) -> Self {
        fs::read_to_string(cfg.card_dir(key).join(ISSUE_MARKER))
            .ok()
            .and_then(|raw| parse_marker(key, raw.trim()))
            .unwrap_or_else(|| Self {
                key: key.to_string(),
                number: key.to_string(),
                repo: None,
            })
    }

    /// `owner/repo#123` for repo-scoped cards, else `#123`.
    pub fn display(&self) -> String {
        match &self.repo {
            Some(r) => format!("{r}#{}", self.number),
            None => format!("#{}", self.number),
        }
    }

    pub fn record(&self, cfg: &Config) {
        if let Some(r) = &self.repo {
            let _ = fs::write(
                cfg.card_dir(&self.key).join(ISSUE_MARKER),
                format!("{r}#{}\n", self.number),
            );
        }
    }
}

/// Resolve what the user typed: `123`, `123-slug`, or a provider-specific id (GitHub:
/// `owner/repo#123`, `repo#123` with `provider.gh.owner`, an issue URL). Returns the card and
/// the slug in it.
pub fn parse_ref(cfg: &Config, raw: &str) -> Result<(CardRef, Option<String>)> {
    let p = provider::current(cfg);
    if let Some(r) = p.parse_id(raw) {
        return r.map(|r| (r, None));
    }
    if raw.contains('#') || raw.contains("://") {
        bail!(
            "{raw:?} is not a card id for provider \"{}\" (owner/repo#123 ids are only for provider \"gh\")",
            p.cli()
        );
    }
    let (id, slug) = split_id(raw);
    Ok((
        CardRef {
            key: id.clone(),
            number: id,
            repo: None,
        },
        slug,
    ))
}

/// The card folder name for an id typed on an existing card (`owner/repo#4` → `repo-4`).
pub fn key(cfg: &Config, raw: &str) -> Result<String> {
    match provider::current(cfg).parse_id(raw) {
        Some(r) => r.map(|r| r.key),
        None => Ok(raw.to_string()),
    }
}

/// Repos actually present in the card folder (independent of the config), sorted alphabetically.
pub fn repos_in(dir: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(dir) else {
        return vec![];
    };
    let mut repos: Vec<String> = entries
        .flatten()
        .filter(|e| e.path().join(".git").exists())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    repos.sort();
    repos
}

/// Card repos in config order (registered ones first, in file order), then the rest.
pub fn repos_ordered(cfg: &Config, dir: &Path) -> Vec<String> {
    let present = repos_in(dir);
    let mut ordered: Vec<String> = cfg
        .repos
        .keys()
        .filter(|r| present.contains(r))
        .cloned()
        .collect();
    ordered.extend(present.into_iter().filter(|r| !cfg.repos.contains_key(r)));
    ordered
}

/// Folder under `<worktrees>` that holds the cards' memos; never a card.
pub const MEMOS_DIR: &str = "memos";

/// Folder names in `<worktrees>` that are never cards (hidden dirs and the memos dir).
pub fn is_reserved(name: &str) -> bool {
    name.starts_with('.') || name == MEMOS_DIR
}

pub fn list_cards(cfg: &Config) -> Vec<String> {
    let Ok(entries) = fs::read_dir(cfg.worktrees()) else {
        return vec![];
    };
    let mut ids: Vec<String> = entries
        .flatten()
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| !is_reserved(n))
        .collect();
    ids.sort();
    ids
}

pub fn require_card(cfg: &Config, id: &str) -> Result<PathBuf> {
    let dir = cfg.card_dir(id);
    if is_reserved(id) || !dir.is_dir() {
        bail!("card {id} not found in {}", cfg.worktrees().display());
    }
    Ok(dir)
}

pub enum Current {
    Card(String),
    Dir(PathBuf),
}

/// Card the user is "in": the herdr workspace label (plugin context), else a cwd
/// inside `<worktrees>/<id>/`. Not a card → the first existing directory.
pub fn infer(cfg: &Config) -> Current {
    let ctx: serde_json::Value = std::env::var("HERDR_PLUGIN_CONTEXT_JSON")
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();

    if let Some(label) = ctx["workspace_label"].as_str().filter(|l| !l.is_empty()) {
        if !is_reserved(label) && cfg.card_dir(label).is_dir() {
            return Current::Card(label.to_string());
        }
    }

    let mut dirs: Vec<PathBuf> = ["focused_pane_cwd", "workspace_cwd"]
        .iter()
        .filter_map(|k| ctx[*k].as_str())
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .collect();
    dirs.extend(std::env::current_dir().ok());

    // compare resolved paths: the cwd is canonical (macOS: /private/var) but the config may not be
    let wt = cfg.worktrees();
    let wt_real = wt.canonicalize().unwrap_or_else(|_| wt.clone());
    for d in &dirs {
        if let Some(rest) = rel_to_worktrees(d, &wt, &wt_real) {
            if let Some(id) = rest.components().next() {
                let id = id.as_os_str().to_string_lossy().into_owned();
                if !is_reserved(&id) && cfg.card_dir(&id).is_dir() {
                    return Current::Card(id);
                }
            }
        }
    }
    Current::Dir(
        dirs.into_iter()
            .find(|d| d.is_dir())
            .unwrap_or_else(|| PathBuf::from(".")),
    )
}

fn rel_to_worktrees(d: &Path, wt: &Path, wt_real: &Path) -> Option<PathBuf> {
    let real = d.canonicalize().unwrap_or_else(|_| d.to_path_buf());
    [d, real.as_path()]
        .iter()
        .find_map(|p| p.strip_prefix(wt).or_else(|_| p.strip_prefix(wt_real)).ok())
        .map(Path::to_path_buf)
}

/// Make sure the repo exists; a repo missing from the config gets registered (asks for the path if not found).
pub fn ensure_repo(cfg: &mut Config, name: &str) -> Result<PathBuf> {
    if let Some(p) = cfg.repo_path(name) {
        if !cfg.repos.contains_key(name) {
            cfg.register_repo(name, &p)?;
            ui::ok(format!(
                "repo {name} registered in the config ({})",
                p.display()
            ));
        }
        return Ok(p);
    }
    if let Some(r) = cfg.repo(name) {
        if !r.path.is_empty() {
            bail!("repo {name}: config path is not a git repo: {}", r.path);
        }
    }

    ui::warn(format!("repo {name} not found. Looked in:"));
    for root in cfg.search_roots() {
        eprintln!("     {}", root.join(name).display());
    }
    let answer = ui::ask(&format!("   path to the {name} clone (empty cancels): "));
    if answer.is_empty() {
        bail!("git repo not found: {name}");
    }
    let p = expand(&answer);
    if !p.join(".git").exists() {
        bail!("{} is not a git repo", p.display());
    }
    cfg.register_repo(name, &p)?;
    ui::ok(format!(
        "repo {name} registered in the config ({})",
        p.display()
    ));
    Ok(p)
}

pub fn branch_name(prefix: &str, id: &str, slug: &str) -> String {
    if slug.is_empty() {
        format!("{prefix}/{id}")
    } else {
        format!("{prefix}/{id}-{slug}")
    }
}

/// Origin branches whose name contains the id as a whole number (12345 doesn't match 123456).
fn remote_branches_with_id(repo: &Path, id: &str) -> Vec<String> {
    // a prefix, not 'refs/remotes/origin/*': the glob doesn't cross slashes
    let out = sys::output(sys::git(repo).args([
        "for-each-ref",
        "--format=%(refname:short)",
        "refs/remotes/origin/",
    ]))
    .unwrap_or_default();
    let re = Regex::new(&format!(r"(^|[^0-9]){}([^0-9]|$)", regex::escape(id))).unwrap();
    out.lines()
        .filter_map(|l| l.strip_prefix("origin/"))
        .filter(|b| *b != "HEAD" && re.is_match(b))
        .map(String::from)
        .collect()
}

pub struct BranchOpts<'a> {
    pub id: &'a str,
    pub slug: &'a str,
    pub forced: Option<&'a str>,
    pub base: &'a str,
    pub prefix: &'a str,
}

enum Origin {
    Remote,
    New,
}

fn resolve_branch(repo_name: &str, repo: &Path, o: &BranchOpts) -> (String, Origin) {
    if let Some(f) = o.forced {
        let origin = if sys::git_ref_exists(repo, &format!("refs/remotes/origin/{f}")) {
            Origin::Remote
        } else {
            Origin::New
        };
        return (f.to_string(), origin);
    }

    let matches = remote_branches_with_id(repo, o.id);
    match matches.len() {
        0 => (branch_name(o.prefix, o.id, o.slug), Origin::New),
        1 => (matches[0].clone(), Origin::Remote),
        n => {
            ui::warn(format!(
                "[{repo_name}] {n} branches on origin with '{}':",
                o.id
            ));
            for (i, b) in matches.iter().enumerate() {
                eprintln!("   \x1b[1m{})\x1b[0m {b}", i + 1);
            }
            eprintln!(
                "   \x1b[1m0)\x1b[0m create a new branch from origin/{}",
                o.base
            );
            let choice = ui::ask("   choice [1]: ");
            match choice.as_str() {
                "0" => (branch_name(o.prefix, o.id, o.slug), Origin::New),
                c => {
                    let idx = c.parse::<usize>().unwrap_or(1).clamp(1, n);
                    (matches[idx - 1].clone(), Origin::Remote)
                }
            }
        }
    }
}

/// Create a repo's worktree inside the card. Returns the branch.
pub fn make_worktree(repo_name: &str, repo: &Path, dest: &Path, o: &BranchOpts) -> Result<String> {
    ui::info(format!("[{repo_name}] fetch..."));
    sys::run(sys::git(repo).args(["fetch", "origin", "--prune", "--quiet"]))
        .with_context(|| format!("[{repo_name}] git fetch failed"))?;
    // without this an interrupted `new` leaves the path registered and blocks the next one
    sys::quiet(sys::git(repo).args(["worktree", "prune"]));

    let (branch, origin) = resolve_branch(repo_name, repo, o);
    let dest_s = dest.to_string_lossy();

    if sys::git_ref_exists(repo, &format!("refs/heads/{branch}")) {
        sys::run(sys::git(repo).args(["worktree", "add", &dest_s, &branch]))?;
        ui::info(format!("[{repo_name}] local branch already existed"));
    } else if matches!(origin, Origin::Remote) {
        sys::run(sys::git(repo).args([
            "worktree",
            "add",
            "--track",
            "-b",
            &branch,
            &dest_s,
            &format!("origin/{branch}"),
        ]))?;
        ui::info(format!("[{repo_name}] came from origin (upstream set)"));
    } else {
        // --no-track: branching from origin/<base> would make git adopt the base as upstream,
        // and a bare push/pull would target it
        sys::run(sys::git(repo).args([
            "worktree",
            "add",
            "--no-track",
            "-b",
            &branch,
            &dest_s,
            &format!("origin/{}", o.base),
        ]))?;
        sys::quiet(sys::git(repo).args(["config", &format!("branch.{branch}.remote"), "origin"]));
        sys::quiet(sys::git(repo).args([
            "config",
            &format!("branch.{branch}.merge"),
            &format!("refs/heads/{branch}"),
        ]));
        ui::warn(format!(
            "[{repo_name}] NEW branch from origin/{} (upstream: origin/{branch}, created on first push)",
            o.base
        ));
    }
    ui::ok(format!("[{repo_name}] {branch}"));
    Ok(branch)
}

/// Undo a `new` that failed midway: remove the created worktrees and the card folder.
pub fn rollback(cfg: &Config, dir: &Path, done: &[String]) {
    ui::warn("undoing what had already been created...");
    for repo in done {
        if let Some(main) = cfg.repo_path(repo) {
            let wt = dir.join(repo);
            sys::quiet(sys::git(&main).args([
                "worktree",
                "remove",
                "--force",
                &wt.to_string_lossy(),
            ]));
            sys::quiet(sys::git(&main).args(["worktree", "prune"]));
            ui::info(format!("[{repo}] worktree undone"));
        }
    }
    let _ = fs::remove_dir_all(dir);
    ui::info(format!("folder {} removed", dir.display()));
    ui::info("if the error above mentions 'already registered', run: twig prune");
}

/// Copy gitignored files from the main clone into the card's worktrees.
/// Tracked files are skipped: the worktree already has its branch's version.
pub fn copy_local_files(cfg: &Config, dir: &Path) {
    let mut n = 0;
    for repo in repos_in(dir) {
        let Some(main) = cfg.repo_path(&repo) else {
            continue;
        };
        let wt = dir.join(&repo);
        let mut patterns = cfg.worktree.local_files.clone();
        if let Some(r) = cfg.repo(&repo) {
            patterns.extend(r.local_files.iter().cloned());
        }
        for pattern in patterns {
            let full = main.join(&pattern);
            let Ok(paths) = glob::glob(&full.to_string_lossy()) else {
                ui::warn(format!(
                    "[{repo}] invalid pattern in local_files: {pattern}"
                ));
                continue;
            };
            for src in paths.flatten().filter(|p| p.is_file()) {
                let Ok(rel) = src.strip_prefix(&main) else {
                    continue;
                };
                let rel_s = rel.to_string_lossy().replace('\\', "/");
                let dst = wt.join(rel);
                if dst.exists() || sys::git_is_tracked(&wt, &rel_s) {
                    continue;
                }
                if let Some(parent) = dst.parent() {
                    let _ = fs::create_dir_all(parent);
                }
                if fs::copy(&src, &dst).is_ok() {
                    ui::info(format!("copied {repo}/{rel_s}"));
                    n += 1;
                }
            }
        }
    }
    if n > 0 {
        ui::ok(format!("{n} local file(s) copied"));
    }
}

/// Deterministic per-card color (same id = always the same color).
pub fn color(id: &str) -> &'static str {
    const PALETTE: [&str; 10] = [
        "#7d3c98", "#b03a2e", "#1f618d", "#117864", "#af601a", "#2c3e50", "#196f3d", "#6c3483",
        "#884ea0", "#a04000",
    ];
    let digits: String = id.chars().filter(char::is_ascii_digit).collect();
    let tail = &digits[digits.len().saturating_sub(18)..];
    let num: u64 = tail.parse().unwrap_or(0);
    PALETTE[(num % PALETTE.len() as u64) as usize]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_id_and_slug() {
        assert_eq!(
            split_id("50306-dispositivos"),
            ("50306".into(), Some("dispositivos".into()))
        );
        assert_eq!(split_id("50306"), ("50306".into(), None));
    }

    fn cfg(provider: &str) -> (tempfile::TempDir, Config) {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("config.toml");
        fs::write(
            &file,
            format!(
                "version = 1\n{provider}\n[paths]\nworktrees = \"{}\"\n",
                tmp.path().join("wt").display()
            ),
        )
        .unwrap();
        let cfg = Config::load_from(&file).unwrap();
        (tmp, cfg)
    }

    fn gh_ref(key: &str, number: &str, repo: &str) -> CardRef {
        CardRef {
            key: key.into(),
            number: number.into(),
            repo: Some(repo.into()),
        }
    }

    #[test]
    fn gh_ids_are_repo_scoped() {
        let (_t, c) = cfg("[provider]\ndefault = \"gh\"\ngh.owner = \"acme\"\n");
        let want = gh_ref("web-12", "12", "acme/web");
        assert_eq!(parse_ref(&c, "acme/web#12").unwrap().0, want);
        assert_eq!(parse_ref(&c, "web#12").unwrap().0, want);
        assert_eq!(
            parse_ref(&c, "https://github.com/acme/web/issues/12")
                .unwrap()
                .0,
            want
        );
        assert_eq!(
            parse_ref(&c, "other/web#12").unwrap().0,
            gh_ref("web-12", "12", "other/web")
        );
        // plain numbers keep working
        let (r, slug) = parse_ref(&c, "12-fix-it").unwrap();
        assert_eq!(
            (r.key.as_str(), r.repo, slug.as_deref()),
            ("12", None, Some("fix-it"))
        );
        assert_eq!(key(&c, "acme/web#12").unwrap(), "web-12");
        assert_eq!(key(&c, "web-12").unwrap(), "web-12");
    }

    #[test]
    fn short_gh_id_needs_an_owner_and_az_rejects_gh_ids() {
        let (_t, c) = cfg("[provider]\ndefault = \"gh\"\n");
        let err = parse_ref(&c, "web#12").unwrap_err().to_string();
        assert!(err.contains("provider.gh.owner"), "{err}");
        assert!(parse_ref(&c, "a/b#x").is_err());
        let (_t, c) = cfg("");
        let err = parse_ref(&c, "acme/web#12").unwrap_err().to_string();
        assert!(err.contains("provider \"gh\""), "{err}");
    }

    #[test]
    fn card_ref_round_trips_through_the_marker() {
        let (_t, c) = cfg("[provider]\ndefault = \"gh\"\n");
        let r = gh_ref("web-12", "12", "acme/web");
        fs::create_dir_all(c.card_dir("web-12")).unwrap();
        r.record(&c);
        assert_eq!(CardRef::of(&c, "web-12"), r);
        assert_eq!(r.display(), "acme/web#12");
        let plain = CardRef::of(&c, "7");
        assert_eq!(
            (plain.number.as_str(), plain.display()),
            ("7", "#7".to_string())
        );
    }

    #[test]
    fn color_is_deterministic_per_id() {
        // 52644 % 10 = 4
        assert_eq!(color("52644"), "#af601a");
    }
}
