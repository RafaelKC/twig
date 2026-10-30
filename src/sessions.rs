//! Agent sessions archive (experimental): `twig rm --keep-sessions` (or `[agent] keep_sessions`)
//! moves the Claude Code transcripts of a card's repos next to the memo, and `twig new` /
//! `twig add-repo` move them back.
//!
//! Claude Code keeps them in `~/.claude/projects/<cwd with every non-alphanumeric as '-'>/`.
//! The archive is `<worktrees>/memos/<id>-<provider>/sessions/<repo>/`. The format is internal
//! to Claude Code and may change; every failure here is a warning, never an error.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::config::{home, AgentKind, Config};
use crate::ui;

/// Folder name Claude Code uses for a working directory.
pub fn encode_cwd(cwd: &Path) -> String {
    cwd.to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

/// `~/.claude/projects` (or under `CLAUDE_CONFIG_DIR`).
fn projects_dir() -> Option<PathBuf> {
    let base = match std::env::var_os("CLAUDE_CONFIG_DIR") {
        Some(d) if !d.is_empty() => PathBuf::from(d),
        _ => home().ok()?.join(".claude"),
    };
    Some(base.join("projects"))
}

/// Whether the feature applies: the agent is Claude Code.
fn active(cfg: &Config) -> bool {
    cfg.ai.provider == AgentKind::Claude
}

/// Whether there is a conversation to resume in `wt`. Only Claude Code is checked (`--continue`
/// fails without one); for the other agents this is assumed true.
pub fn has_session(cfg: &Config, wt: &Path) -> bool {
    if cfg.ai.provider != AgentKind::Claude {
        return true;
    }
    let Some(projects) = projects_dir() else {
        return false;
    };
    let found = |p: &Path| count_sessions(&projects.join(encode_cwd(p))) > 0;
    found(wt) || wt.canonicalize().is_ok_and(|real| found(&real))
}

/// Number of sessions (`*.jsonl` transcripts) directly in `dir`.
fn count_sessions(dir: &Path) -> usize {
    fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .filter(|e| e.path().extension().is_some_and(|x| x == "jsonl"))
                .count()
        })
        .unwrap_or(0)
}

/// Move `src` into `dst`, merging with what is already there (existing files are replaced).
fn move_dir(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir_all(dst).with_context(|| format!("creating {}", dst.display()))?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            move_dir(&from, &to)?;
        } else {
            if fs::rename(&from, &to).is_err() {
                // another filesystem: copy, then remove
                fs::copy(&from, &to).with_context(|| format!("copying {}", from.display()))?;
                fs::remove_file(&from)?;
            }
        }
    }
    fs::remove_dir(src).ok();
    Ok(())
}

/// Move `<projects>/<encoded wt>` into `<archive>/<repo>`; returns how many sessions moved.
fn archive_one(projects: &Path, wt: &Path, archive: &Path, repo: &str) -> Result<usize> {
    let mut src = projects.join(encode_cwd(wt));
    if !src.is_dir() {
        // Claude records the resolved path
        match wt.canonicalize() {
            Ok(real) if projects.join(encode_cwd(&real)).is_dir() => {
                src = projects.join(encode_cwd(&real))
            }
            _ => return Ok(0),
        }
    }
    let n = count_sessions(&src);
    move_dir(&src, &archive.join(repo))?;
    Ok(n)
}

/// Move the archived `<archive>/<repo>` back into `<projects>/<encoded wt>`.
fn restore_one(projects: &Path, wt: &Path, archive: &Path, repo: &str) -> Result<usize> {
    let src = archive.join(repo);
    if !src.is_dir() {
        return Ok(0);
    }
    let n = count_sessions(&src);
    move_dir(&src, &projects.join(encode_cwd(wt)))?;
    Ok(n)
}

fn summary(done: &[(String, usize)]) -> String {
    done.iter()
        .map(|(r, n)| format!("{r} ({n})"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// `twig rm`: archive the sessions of the card's repos; call before the folder is removed.
/// `keep` is the `--keep-sessions` / `--no-keep-sessions` flag, which wins over the config.
pub fn archive(cfg: &Config, id: &str, repos: &[String], keep: Option<bool>) {
    if !active(cfg) || !keep.unwrap_or(cfg.ai.keep_sessions) {
        return;
    }
    let Some(projects) = projects_dir() else {
        return;
    };
    let archive = cfg.sessions_dir(id);
    let dir = cfg.card_dir(id);
    let mut done = vec![];
    for repo in repos {
        match archive_one(&projects, &dir.join(repo), &archive, repo) {
            Ok(0) => {}
            Ok(n) => done.push((repo.clone(), n)),
            Err(e) => ui::warn(format!(
                "[{repo}] could not archive the agent sessions: {e:#}"
            )),
        }
    }
    if !done.is_empty() {
        ui::info(format!(
            "sessions kept at {}: {} (`twig new {id}` restores them)",
            archive.display(),
            summary(&done)
        ));
    }
}

/// `twig new` / `twig add-repo`: restore the archived sessions of `repos` into the card.
pub fn restore(cfg: &Config, id: &str, repos: &[String]) {
    if !active(cfg) {
        return;
    }
    let Some(projects) = projects_dir() else {
        return;
    };
    let archive = cfg.sessions_dir(id);
    let dir = cfg.card_dir(id);
    let mut done = vec![];
    for repo in repos {
        match restore_one(&projects, &dir.join(repo), &archive, repo) {
            Ok(0) => {}
            Ok(n) => done.push((repo.clone(), n)),
            Err(e) => ui::warn(format!(
                "[{repo}] could not restore the agent sessions: {e:#}"
            )),
        }
    }
    if !done.is_empty() {
        ui::info(format!("sessions restored: {}", summary(&done)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_every_non_alphanumeric_as_dash() {
        assert_eq!(
            encode_cwd(Path::new("/home/u/.wt/50222/frota-api")),
            "-home-u--wt-50222-frota-api"
        );
    }

    #[test]
    fn archive_then_restore_round_trips() {
        let tmp = tempfile::tempdir().unwrap();
        let projects = tmp.path().join("projects");
        let wt = tmp.path().join("wt/1/api");
        let proj = projects.join(encode_cwd(&wt));
        fs::create_dir_all(proj.join("abc")).unwrap();
        fs::write(proj.join("abc.jsonl"), "{}").unwrap();
        fs::write(proj.join("abc/tool.txt"), "x").unwrap();
        let archive = tmp.path().join("memos/1-az/sessions");

        assert_eq!(archive_one(&projects, &wt, &archive, "api").unwrap(), 1);
        assert!(!proj.exists());
        assert!(archive.join("api/abc.jsonl").is_file());
        assert!(archive.join("api/abc/tool.txt").is_file());

        assert_eq!(restore_one(&projects, &wt, &archive, "api").unwrap(), 1);
        assert!(proj.join("abc.jsonl").is_file());
        assert!(proj.join("abc/tool.txt").is_file());
        assert!(!archive.join("api").exists());
    }

    #[test]
    fn nothing_to_move_is_zero() {
        let tmp = tempfile::tempdir().unwrap();
        let wt = tmp.path().join("wt/1/api");
        let archive = tmp.path().join("a");
        assert_eq!(archive_one(tmp.path(), &wt, &archive, "api").unwrap(), 0);
        assert_eq!(restore_one(tmp.path(), &wt, &archive, "api").unwrap(), 0);
    }
}
