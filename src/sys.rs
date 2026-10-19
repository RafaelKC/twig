use std::env;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{bail, Context, Result};

/// Look up an executable on PATH.
pub fn which(bin: &str) -> Option<PathBuf> {
    let exts: Vec<String> = if cfg!(windows) {
        env::var("PATHEXT")
            .unwrap_or_else(|_| ".EXE;.CMD;.BAT".into())
            .split(';')
            .map(|e| e.to_string())
            .collect()
    } else {
        vec![String::new()]
    };
    env::split_paths(&env::var_os("PATH")?).find_map(|dir| {
        exts.iter()
            .map(|e| dir.join(format!("{bin}{e}")))
            .find(|p| p.is_file())
    })
}

/// Run and return stdout; error with stderr if the command fails.
pub fn output(cmd: &mut Command) -> Result<String> {
    let out = cmd
        .stdin(Stdio::null())
        .output()
        .with_context(|| format!("could not run {:?}", cmd.get_program()))?;
    if !out.status.success() {
        bail!(
            "{:?} failed: {}",
            cmd.get_program(),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Run inheriting the terminal (output visible to the user).
pub fn run(cmd: &mut Command) -> Result<()> {
    let status = cmd
        .status()
        .with_context(|| format!("could not run {:?}", cmd.get_program()))?;
    if !status.success() {
        bail!("{:?} exited with {status}", cmd.get_program());
    }
    Ok(())
}

/// Run silently; only success matters.
pub fn quiet(cmd: &mut Command) -> bool {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Shell command defined in the config (`run`, `install`).
pub fn shell(script: &str, cwd: &Path) -> Command {
    let mut cmd = if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.args(["/C", script]);
        c
    } else {
        let mut c = Command::new("sh");
        c.args(["-c", script]);
        c
    };
    cmd.current_dir(cwd);
    cmd
}

pub fn git(repo: &Path) -> Command {
    let mut cmd = Command::new("git");
    cmd.arg("-C").arg(repo);
    cmd
}

pub fn git_current_branch(repo: &Path) -> Option<String> {
    output(git(repo).args(["rev-parse", "--abbrev-ref", "HEAD"]))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

pub fn git_is_dirty(repo: &Path) -> bool {
    output(git(repo).args(["status", "--porcelain"]))
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false)
}

pub fn git_ref_exists(repo: &Path, refname: &str) -> bool {
    quiet(git(repo).args(["show-ref", "--verify", "--quiet", refname]))
}

pub fn git_is_tracked(repo: &Path, rel: &str) -> bool {
    quiet(git(repo).args(["ls-files", "--error-unmatch", rel]))
}

/// A repo's orphaned worktree records (folder deleted by hand), as "path — reason".
pub fn git_orphan_worktrees(repo: &Path) -> Vec<String> {
    let out = output(git(repo).args(["worktree", "list", "--porcelain"])).unwrap_or_default();
    let mut current = "";
    let mut orphans = vec![];
    for line in out.lines() {
        if let Some(p) = line.strip_prefix("worktree ") {
            current = p;
        } else if let Some(reason) = line.strip_prefix("prunable ") {
            orphans.push(format!("{current} — {reason}"));
        }
    }
    orphans
}
