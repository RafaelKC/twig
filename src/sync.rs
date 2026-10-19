//! `twig sync`: bring the base branch into every repo's worktree of a card.

use anyhow::{bail, Result};

use crate::config::{Config, SyncMode};
use crate::{card, sys, ui};

/// Fetch `origin/<base>` and rebase/merge it into each repo of the card. Repos with
/// uncommitted changes are skipped and reported; a conflict stops the whole run.
pub fn run(cfg: &Config, id: &str, mode: Option<SyncMode>, dry_run: bool) -> Result<()> {
    let (id, _) = card::split_id(id);
    let dir = card::require_card(cfg, &id)?;
    let mode = mode.unwrap_or(cfg.git.sync);
    let base = &cfg.git.base_branch;
    let remote_ref = format!("origin/{base}");
    let verb = match mode {
        SyncMode::Rebase => "rebase",
        SyncMode::Merge => "merge",
    };

    let repos = card::repos_ordered(cfg, &dir);
    if repos.is_empty() {
        ui::info(format!("card {id} has no repos"));
        return Ok(());
    }

    let (mut synced, mut current, mut skipped, mut failed) = (0, 0, vec![], vec![]);
    for repo in &repos {
        let wt = dir.join(repo);
        let Some(branch) = sys::git_current_branch(&wt).filter(|b| b != "HEAD") else {
            ui::warn(format!("{repo}: detached HEAD, skipped"));
            skipped.push(repo.clone());
            continue;
        };
        if sys::git_is_dirty(&wt) {
            ui::warn(format!("{repo}: uncommitted changes, skipped"));
            skipped.push(repo.clone());
            continue;
        }
        if let Err(e) = sys::output(sys::git(&wt).args(["fetch", "origin", base])) {
            ui::warn(format!("{repo}: could not fetch {remote_ref}: {e:#}"));
            failed.push(repo.clone());
            continue;
        }
        let behind = sys::output(sys::git(&wt).args([
            "rev-list",
            "--count",
            &format!("HEAD..{remote_ref}"),
        ]))
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok())
        .unwrap_or(0);
        if behind == 0 {
            ui::info(format!("{repo}: {branch} is up to date with {remote_ref}"));
            current += 1;
            continue;
        }
        if dry_run {
            ui::info(format!(
                "{repo}: would {verb} {behind} commit(s) from {remote_ref} into {branch}"
            ));
            synced += 1;
            continue;
        }

        let mut cmd = sys::git(&wt);
        match mode {
            SyncMode::Rebase => cmd.args(["rebase", &remote_ref]),
            SyncMode::Merge => cmd.args(["merge", "--no-edit", &remote_ref]),
        };
        if sys::quiet(&mut cmd) {
            ui::ok(format!(
                "{repo}: {verb}d {behind} commit(s) from {remote_ref}"
            ));
            synced += 1;
        } else {
            let files = sys::output(sys::git(&wt).args(["diff", "--name-only", "--diff-filter=U"]))
                .unwrap_or_default();
            let files: Vec<&str> = files.lines().collect();
            let abort = format!("git -C {} {verb} --abort", wt.display());
            if files.is_empty() {
                bail!("{repo}: {verb} of {remote_ref} failed (abort with `{abort}`)");
            }
            bail!(
                "{repo}: conflict in {verb} of {remote_ref}, stopped. Conflicting files: {}. \
                 Resolve them in {} or abort with `{abort}`",
                files.join(", "),
                wt.display()
            );
        }
    }

    let tail = format!(
        "{synced} {verb}{}, {current} up to date, {} skipped, {} failed",
        if dry_run { " (dry run)" } else { "" },
        skipped.len(),
        failed.len()
    );
    if failed.is_empty() {
        ui::ok(tail);
        Ok(())
    } else {
        bail!("{tail} ({})", failed.join(", "))
    }
}
