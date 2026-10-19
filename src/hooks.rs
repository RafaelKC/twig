//! Lifecycle hooks: user-defined shell commands run around card operations.
//!
//! Configured card-wide in `[hooks]` and per repo in `[repos.<name>.hooks]`. Only commands the
//! user wrote in the config run; nothing is injected from a provider or plugin.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{bail, Result};

use crate::config::{Config, Hooks};
use crate::{card, sys, ui};

static DISABLED: AtomicBool = AtomicBool::new(false);

/// `--no-hooks`: skip every hook for this invocation.
pub fn disable() {
    DISABLED.store(true, Ordering::Relaxed);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    PostCreate,
    PreRm,
    PostRm,
    PostOpen,
    PostRefresh,
}

const ALL: [Event; 5] = [
    Event::PostCreate,
    Event::PreRm,
    Event::PostRm,
    Event::PostOpen,
    Event::PostRefresh,
];

impl Event {
    pub fn name(self) -> &'static str {
        match self {
            Self::PostCreate => "post_create",
            Self::PreRm => "pre_rm",
            Self::PostRm => "post_rm",
            Self::PostOpen => "post_open",
            Self::PostRefresh => "post_refresh",
        }
    }

    fn is_pre(self) -> bool {
        self == Self::PreRm
    }
}

/// One repo of the card as the hooks see it.
pub struct RepoCtx {
    pub name: String,
    /// Worktree path (`TWIG_REPO_DIR`).
    pub dir: PathBuf,
    /// Where repo-level hooks run (the worktree, unless it no longer exists).
    pub cwd: PathBuf,
    pub branch: String,
}

/// Repos of the card with their current branch.
pub fn repo_ctxs(cfg: &Config, id: &str, names: &[String]) -> Vec<RepoCtx> {
    let dir = cfg.card_dir(id);
    names
        .iter()
        .map(|n| {
            let wt = dir.join(n);
            RepoCtx {
                name: n.clone(),
                branch: sys::git_current_branch(&wt).unwrap_or_default(),
                cwd: wt.clone(),
                dir: wt,
            }
        })
        .collect()
}

/// Every repo currently in the card.
pub fn card_repos(cfg: &Config, id: &str) -> Vec<RepoCtx> {
    let names = card::repos_ordered(cfg, &cfg.card_dir(id));
    repo_ctxs(cfg, id, &names)
}

fn list(h: &Hooks, ev: Event) -> &[String] {
    match ev {
        Event::PostCreate => &h.post_create,
        Event::PreRm => &h.pre_rm,
        Event::PostRm => &h.post_rm,
        Event::PostOpen => &h.post_open,
        Event::PostRefresh => &h.post_refresh,
    }
}

/// Run the hooks of `ev`. A failing `pre_*` hook returns an error (the caller aborts);
/// `post_*` failures only warn. `card_cwd` is where card-level hooks run (the card folder, or a
/// fallback when it is gone). Order: card-level first for `pre_*`, last for `post_*`.
pub fn run(cfg: &Config, ev: Event, id: &str, card_cwd: &Path, repos: &[RepoCtx]) -> Result<()> {
    if DISABLED.load(Ordering::Relaxed) {
        return Ok(());
    }
    let card_dir = cfg.card_dir(id);
    let card_level = || {
        run_list(
            list(&cfg.hooks, ev),
            ev,
            "card",
            id,
            &card_dir,
            card_cwd,
            None,
        )?;
        // hooks of the template the card was created from run after the config's
        match &cfg.template {
            Some(t) => run_list(
                list(&t.hooks, ev),
                ev,
                "template",
                id,
                &card_dir,
                card_cwd,
                None,
            ),
            None => Ok(()),
        }
    };
    if ev.is_pre() {
        card_level()?;
    }
    for r in repos {
        let Some(repo) = cfg.repo(&r.name) else {
            continue;
        };
        run_list(
            list(&repo.hooks, ev),
            ev,
            &r.name,
            id,
            &card_dir,
            &r.cwd,
            Some(r),
        )?;
    }
    if !ev.is_pre() {
        card_level()?;
    }
    Ok(())
}

fn run_list(
    cmds: &[String],
    ev: Event,
    scope: &str,
    id: &str,
    card_dir: &Path,
    cwd: &Path,
    repo: Option<&RepoCtx>,
) -> Result<()> {
    for c in cmds {
        ui::info(format!("[{scope}] {} hook: {c}", ev.name()));
        let mut cmd = sys::shell(c, cwd);
        cmd.env("TWIG_CARD", id)
            .env("TWIG_CARD_ID", id)
            .env("TWIG_CARD_DIR", card_dir)
            .env("TWIG_EVENT", ev.name())
            .env("TWIG_REPO", repo.map(|r| r.name.as_str()).unwrap_or(""))
            .env(
                "TWIG_REPO_DIR",
                repo.map(|r| r.dir.as_path()).unwrap_or(Path::new("")),
            )
            .env("TWIG_BRANCH", repo.map(|r| r.branch.as_str()).unwrap_or(""));
        if let Err(e) = sys::run(&mut cmd) {
            if ev.is_pre() {
                bail!(
                    "[{scope}] {} hook `{c}` failed: {e:#} (--no-hooks skips hooks)",
                    ev.name()
                );
            }
            ui::warn(format!("[{scope}] {} hook `{c}` failed: {e:#}", ev.name()));
            break;
        }
    }
    Ok(())
}

/// Non-empty hook lists of a `Hooks` table, by event name (for display).
pub fn entries(h: &Hooks) -> Vec<(&'static str, &[String])> {
    ALL.iter()
        .map(|ev| (ev.name(), list(h, *ev)))
        .filter(|(_, c)| !c.is_empty())
        .collect()
}

/// Number of commands in one hooks table and its problems.
pub fn validate_hooks(scope: &str, h: &Hooks) -> (usize, Vec<String>) {
    let mut count = 0;
    let mut errors = vec![];
    for ev in ALL {
        for (i, c) in list(h, ev).iter().enumerate() {
            count += 1;
            if c.trim().is_empty() {
                errors.push(format!("{scope}.{}[{i}] is an empty command", ev.name()));
            }
        }
    }
    (count, errors)
}

/// Number of configured hook commands and the problems found in them (for `twig doctor`).
pub fn validate(cfg: &Config) -> (usize, Vec<String>) {
    let mut count = 0;
    let mut errors = vec![];
    let mut check = |scope: &str, h: &Hooks| {
        let (c, e) = validate_hooks(scope, h);
        count += c;
        errors.extend(e);
    };
    check("hooks", &cfg.hooks);
    for (name, r) in &cfg.repos {
        check(&format!("repos.{name}.hooks"), &r.hooks);
    }
    (count, errors)
}
