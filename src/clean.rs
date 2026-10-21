//! `twig clean`: find finished cards (every branch merged, or the work item closed) and remove
//! them with the same rules as `twig rm`.

use std::path::Path;

use anyhow::{bail, Result};

use crate::card::{self, CardRef};
use crate::config::Config;
use crate::pr;
use crate::provider::{self, PrState};
use crate::{commands, sys, templates, ui};

/// Why a card can go.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Reason {
    Closed(String),
    Merged,
}

impl Reason {
    fn text(&self) -> String {
        match self {
            Reason::Closed(state) => format!("work item {state}"),
            Reason::Merged => "every branch merged".into(),
        }
    }
}

/// The commit the branch was created at: the oldest entry of its reflog.
fn created_at(wt: &Path, branch: &str) -> Option<String> {
    let out = sys::output(sys::git(wt).args([
        "reflog",
        "show",
        "--format=%H",
        &format!("refs/heads/{branch}"),
    ]))
    .ok()?;
    out.lines().last().map(|l| l.trim().to_string())
}

fn rev(wt: &Path, r: &str) -> Option<String> {
    sys::output(sys::git(wt).args(["rev-parse", "--verify", "--quiet", r]))
        .ok()
        .map(|s| s.trim().to_string())
}

/// The branch got into the base: a merged PR (catches squash merges), or own commits that are
/// all in `origin/<base>`. A branch with no commits of its own is never "merged", so a card
/// that was just created doesn't qualify.
fn repo_merged(cfg: &Config, wt: &Path, base: &str) -> bool {
    let Some(branch) = sys::git_current_branch(wt).filter(|b| b != "HEAD") else {
        return false;
    };
    if provider::current(cfg)
        .find_pr(wt, &branch, PrState::Merged)
        .is_some()
    {
        return true;
    }
    let _ = sys::output(sys::git(wt).args(["fetch", "origin", base, "--quiet"]));
    let (Some(head), Some(_)) = (rev(wt, "HEAD"), rev(wt, &format!("origin/{base}"))) else {
        return false;
    };
    if created_at(wt, &branch).is_some_and(|c| c == head) {
        return false;
    }
    sys::quiet(sys::git(wt).args([
        "merge-base",
        "--is-ancestor",
        "HEAD",
        &format!("origin/{base}"),
    ]))
}

fn reason(cfg: &Config, id: &str) -> Option<Reason> {
    let dir = cfg.card_dir(id);
    let repos = card::repos_ordered(cfg, &dir);
    let cref = CardRef::of(cfg, id);
    let first = repos.first().and_then(|r| cfg.repo_path(r));
    // the cached state is from when the card was created: ask the provider again
    if let Some(info) = provider::refetch(cfg, &cref, first.as_deref()) {
        if provider::current(cfg).is_finished(&info.state) {
            return Some(Reason::Closed(info.state));
        }
    }
    let base = pr::card_base(cfg);
    (!repos.is_empty() && repos.iter().all(|r| repo_merged(cfg, &dir.join(r), &base)))
        .then_some(Reason::Merged)
}

/// The config as `twig rm <id>` would see it (with the card's template active).
fn card_config(id: &str) -> Result<Config> {
    let mut cfg = Config::load()?;
    templates::activate(&mut cfg, id);
    Ok(cfg)
}

pub fn run(cfg: &Config, dry_run: bool, yes: bool, branches: bool, force: bool) -> Result<()> {
    ui::info("looking for finished cards...");
    let mut found = vec![];
    for id in card::list_cards(cfg) {
        let c = card_config(&id)?;
        if let Some(r) = reason(&c, &id) {
            found.push((id, r));
        }
    }
    if found.is_empty() {
        ui::ok("no finished cards");
        return Ok(());
    }
    for (id, r) in &found {
        println!("      #{id:<16} {}", r.text());
    }
    if dry_run {
        ui::info("(dry-run: nothing removed)");
        return Ok(());
    }

    let mut failed = vec![];
    for (id, r) in &found {
        if !yes && !ui::confirm(&format!("   remove card {id} ({})? [y/N]: ", r.text())) {
            ui::info(format!("card {id} kept"));
            continue;
        }
        if let Err(e) = commands::rm(&card_config(id)?, id, branches, force) {
            ui::warn(format!("card {id}: {e:#}"));
            failed.push(id.clone());
        }
    }
    if !failed.is_empty() {
        bail!("not removed: {}", failed.join(", "));
    }
    Ok(())
}
