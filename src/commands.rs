use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::{bail, Result};

use crate::config::{Config, OpenTarget};
use crate::provider::{self, CardInfo};
use crate::{card, claude_md, herdr, sys, ui, workspace};

pub struct NewArgs {
    pub id: String,
    pub slug: Option<String>,
    pub repos: Option<Vec<String>>,
    pub branch: Option<String>,
    pub base: Option<String>,
    pub fix: bool,
    pub install: bool,
    pub no_setup: bool,
    pub herdr: bool,
    pub code: bool,
    pub no_ado: bool,
}

pub struct AddRepoArgs {
    pub id: String,
    pub repos: Vec<String>,
    pub branch: Option<String>,
    pub base: Option<String>,
    pub fix: bool,
    pub install: bool,
    pub no_setup: bool,
    pub herdr: bool,
    pub code: bool,
}

/// VS Code explicitly requested (`--code` or `twig code`): fail before creating anything.
fn check_code_flag(cfg: &Config, code: bool) -> Result<()> {
    if !code {
        return Ok(());
    }
    if !cfg.vscode.enabled {
        bail!("VS Code disabled in the config (vscode.enabled = false)");
    }
    if sys::which("code").is_none() {
        bail!("`code` is not on PATH");
    }
    Ok(())
}

/// Open VS Code and/or herdr at the end of `new` / `add-repo`.
fn open_after(cfg: &Config, id: &str, ws: Option<&str>, code: bool, herdr_flag: bool) -> Result<()> {
    if code || (cfg.vscode.enabled && cfg.vscode.open_on_new) {
        if sys::which("code").is_some() {
            open_code(cfg, id)?;
        } else {
            ui::warn("vscode.open_on_new is on, but `code` is not on PATH");
        }
    }
    if herdr_flag || cfg.herdr.open_on_new {
        match ws {
            Some(ws) => herdr::open(ws)?,
            None => ui::warn("herdr workspace was not created; try `twig herdr <id>`"),
        }
    }
    Ok(())
}

fn sync_herdr(cfg: &Config, id: &str) -> Option<String> {
    match herdr::sync(cfg, id) {
        Ok(ws) => Some(ws),
        Err(e) => {
            ui::warn(format!("herdr: {e:#}"));
            None
        }
    }
}

fn first_repo_path(cfg: &Config, repos: &[String]) -> Option<std::path::PathBuf> {
    repos.first().and_then(|r| cfg.repo_path(r))
}

pub fn new(cfg: &mut Config, a: NewArgs) -> Result<()> {
    check_code_flag(cfg, a.code)?;

    let (id, slug_from_id) = card::split_id(&a.id);
    let mut slug = a.slug.or(slug_from_id).unwrap_or_default();

    let repos = a.repos.unwrap_or_else(|| cfg.default_repos());
    if repos.is_empty() {
        bail!("no repos: pass --repos or set `default = true` on some repo in the config");
    }
    // validate (and register) the repos before creating anything
    let mut paths = vec![];
    for r in &repos {
        paths.push(card::ensure_repo(cfg, r)?);
    }

    let dir = cfg.card_dir(&id);
    if dir.is_dir() {
        if !card::repos_in(&dir).is_empty() {
            bail!("card {id} already exists: use `twig code {id}`, `twig herdr {id}` or `twig rm {id}`");
        }
        ui::warn(format!("{} existed with no worktrees inside, reusing it", dir.display()));
    }

    let info: Option<CardInfo> = if a.no_ado {
        None
    } else {
        provider::load(cfg, &id, first_repo_path(cfg, &repos).as_deref())
    };
    if let Some(i) = &info {
        ui::info(format!("card: {}", i.title));
        if slug.is_empty() {
            slug = i.slug();
            ui::info(format!("slug derived from the title: {slug}"));
        }
    }

    fs::create_dir_all(&dir)?;
    if let Some(i) = &info {
        provider::save_cache(cfg, &id, i);
    }

    let prefix = if a.fix {
        "fix".to_string()
    } else {
        cfg.branch_prefix(info.as_ref().map(|i| i.kind.as_str()).unwrap_or(""))
    };
    let base = a.base.unwrap_or_else(|| cfg.git.base_branch.clone());
    let opts = card::BranchOpts { id: &id, slug: &slug, forced: a.branch.as_deref(), base: &base, prefix: &prefix };

    let mut done: Vec<String> = vec![];
    let mut title = String::new();
    for (repo, path) in repos.iter().zip(&paths) {
        match card::make_worktree(repo, path, &dir.join(repo), &opts) {
            Ok(branch) => {
                if title.is_empty() {
                    title = branch;
                }
                done.push(repo.clone());
            }
            Err(e) => {
                ui::warn(format!("{e:#}"));
                card::rollback(cfg, &dir, &done);
                bail!("[{repo}] failed to create the worktree");
            }
        }
    }

    card::copy_local_files(cfg, &dir);
    if cfg.vscode.enabled {
        workspace::write(cfg, &id, &title)?;
    }
    claude_md::write(cfg, &id, &slug, info.as_ref())?;

    if wants_setup(cfg, a.install, a.no_setup) {
        setup(cfg, &id, &repos);
    }

    let ws = sync_herdr(cfg, &id);
    open_after(cfg, &id, ws.as_deref(), a.code, a.herdr)
}

fn wants_setup(cfg: &Config, install: bool, no_setup: bool) -> bool {
    install || (cfg.worktree.auto_install && !no_setup)
}

/// Per repo: `install`, then each `post_create` command, in the worktree. Failures only warn.
fn setup(cfg: &Config, id: &str, repos: &[String]) {
    let dir = cfg.card_dir(id);
    for repo in repos {
        let Some(r) = cfg.repo(repo) else { continue };
        let cmds = Some(&r.install).filter(|c| !c.is_empty()).into_iter().chain(&r.post_create);
        let mut any = false;
        for c in cmds {
            any = true;
            ui::info(format!("[{repo}] {c}"));
            let mut cmd = sys::shell(c, &dir.join(repo));
            cmd.env("TWIG_CARD", id).env("TWIG_REPO", repo);
            if let Err(e) = sys::run(&mut cmd) {
                ui::warn(format!("[{repo}] `{c}` failed: {e:#}"));
                break;
            }
        }
        if !any {
            ui::info(format!("[{repo}] no `install` or `post_create` in the config, skipping"));
        }
    }
}

pub fn add_repo(cfg: &mut Config, a: AddRepoArgs) -> Result<()> {
    check_code_flag(cfg, a.code)?;
    let dir = card::require_card(cfg, &a.id)?;

    let mut paths = vec![];
    for r in &a.repos {
        paths.push(card::ensure_repo(cfg, r)?);
    }

    let existing = card::repos_ordered(cfg, &dir);
    let info = provider::load(cfg, &a.id, existing.first().and_then(|r| cfg.repo_path(r)).as_deref());
    let slug = info.as_ref().map(CardInfo::slug).unwrap_or_default();
    let prefix = if a.fix {
        "fix".to_string()
    } else {
        cfg.branch_prefix(info.as_ref().map(|i| i.kind.as_str()).unwrap_or(""))
    };
    let base = a.base.unwrap_or_else(|| cfg.git.base_branch.clone());
    let opts = card::BranchOpts { id: &a.id, slug: &slug, forced: a.branch.as_deref(), base: &base, prefix: &prefix };

    let mut added = false;
    let mut created: Vec<String> = vec![];
    for (repo, path) in a.repos.iter().zip(&paths) {
        if dir.join(repo).join(".git").exists() {
            ui::info(format!("[{repo}] is already in the card"));
            continue;
        }
        card::make_worktree(repo, path, &dir.join(repo), &opts)?;
        added = true;
        created.push(repo.clone());
    }

    card::copy_local_files(cfg, &dir);
    if wants_setup(cfg, a.install, a.no_setup) {
        setup(cfg, &a.id, &created);
    }
    regenerate(cfg, &a.id, info.as_ref())?;
    let ws = sync_herdr(cfg, &a.id);
    if added && cfg.vscode.enabled {
        ui::info("if the card's VS Code is open, accept the workspace reload");
    }
    open_after(cfg, &a.id, ws.as_deref(), a.code, a.herdr)
}

/// Regenerate `.code-workspace` (if VS Code is enabled) and CLAUDE.md.
fn regenerate(cfg: &Config, id: &str, info: Option<&CardInfo>) -> Result<()> {
    let dir = cfg.card_dir(id);
    let title = card::repos_ordered(cfg, &dir)
        .first()
        .and_then(|r| sys::git_current_branch(&dir.join(r)))
        .unwrap_or_default();
    if cfg.vscode.enabled {
        workspace::write(cfg, id, &title)?;
    }
    claude_md::write(cfg, id, "", info)
}

/// `twig open [id] [target]`; `herdr` and `code` go through the same functions.
/// Without a target, `[open] default` from the config decides.
pub fn open(cfg: &Config, id: Option<&str>, target: Option<OpenTarget>) -> Result<()> {
    match target.unwrap_or(cfg.open.default) {
        OpenTarget::Code => code(cfg, id),
        OpenTarget::Herdr => match id {
            Some(id) => herdr_cmd(cfg, id),
            None => match card::infer(cfg) {
                card::Current::Card(id) => herdr_cmd(cfg, &id),
                card::Current::Dir(dir) => {
                    bail!("{} is not inside a card; pass the card id", dir.display())
                }
            },
        },
    }
}

pub fn herdr_cmd(cfg: &Config, id: &str) -> Result<()> {
    let ws = herdr::sync(cfg, id)?;
    herdr::open(&ws)
}

pub fn open_code(cfg: &Config, id: &str) -> Result<()> {
    check_code_flag(cfg, true)?;
    card::require_card(cfg, id)?;
    let file = cfg.workspace_file(id);
    if !file.exists() {
        let dir = cfg.card_dir(id);
        let info = provider::load(cfg, id, card::repos_ordered(cfg, &dir).first().and_then(|r| cfg.repo_path(r)).as_deref());
        let title = card::repos_ordered(cfg, &dir)
            .first()
            .and_then(|r| sys::git_current_branch(&dir.join(r)))
            .or_else(|| info.map(|i| i.title))
            .unwrap_or_default();
        workspace::write(cfg, id, &title)?;
    }
    launch_code(&file)?;
    ui::ok(format!("opening #{id} in VS Code"));
    Ok(())
}

/// `twig code [id]`: without an id, infer the card from the herdr plugin context or the cwd.
pub fn code(cfg: &Config, id: Option<&str>) -> Result<()> {
    if let Some(id) = id {
        return open_code(cfg, id);
    }
    check_code_flag(cfg, true)?;
    match card::infer(cfg) {
        card::Current::Card(id) => open_code(cfg, &id),
        card::Current::Dir(dir) => {
            launch_code(&dir)?;
            ui::ok(format!("not a card; opening {} in VS Code", dir.display()));
            Ok(())
        }
    }
}

fn launch_code(target: &Path) -> Result<()> {
    let mut cmd = Command::new("code");
    cmd.arg(target);
    // without this VS Code's integrated terminal inherits HERDR_* and refuses to run `herdr` (nested)
    for (k, _) in std::env::vars().filter(|(k, _)| k.starts_with("HERDR_")) {
        cmd.env_remove(k);
    }
    sys::run(&mut cmd)
}

pub fn ls(cfg: &Config) -> Result<()> {
    let ids = card::list_cards(cfg);
    if ids.is_empty() {
        ui::info("no active cards");
        return Ok(());
    }
    for id in ids {
        println!("\x1b[1m  #{id}\x1b[0m");
        let dir = cfg.card_dir(&id);
        for repo in card::repos_ordered(cfg, &dir) {
            let wt = dir.join(&repo);
            let branch = sys::git_current_branch(&wt).unwrap_or_else(|| "?".into());
            let dirty = if sys::git_is_dirty(&wt) { " \x1b[33m*\x1b[0m" } else { "" };
            println!("      {repo:<18} {branch}{dirty}");
        }
    }
    Ok(())
}

pub fn refresh(cfg: &Config, id: &str, refetch: bool) -> Result<()> {
    let dir = card::require_card(cfg, id)?;
    if refetch {
        provider::drop_cache(cfg, id);
        ui::info("card cache dropped");
    }
    let info = provider::load(cfg, id, card::repos_ordered(cfg, &dir).first().and_then(|r| cfg.repo_path(r)).as_deref());
    regenerate(cfg, id, info.as_ref())
}

pub fn rm(cfg: &Config, id: &str, branches: bool, force: bool) -> Result<()> {
    let dir = card::require_card(cfg, id)?;
    let repos = card::repos_in(&dir);

    if !force {
        for repo in &repos {
            if sys::git_is_dirty(&dir.join(repo)) {
                bail!("[{repo}] has uncommitted changes, resolve them first (or use --force)");
            }
        }
        let busy = herdr::running_agents(id);
        if !busy.is_empty() {
            bail!("an agent is running in the herdr workspace #{id} ({}), stop it first (or use --force)", busy.join(", "));
        }
    }

    herdr::close_workspace(id);

    for repo in &repos {
        let wt = dir.join(repo);
        let branch = sys::git_current_branch(&wt);
        let Some(main) = cfg.repo_path(repo) else {
            ui::warn(format!("[{repo}] main clone not found, removing only the folder"));
            continue;
        };
        sys::run(sys::git(&main).args(["worktree", "remove", "--force", &wt.to_string_lossy()]))?;
        sys::quiet(sys::git(&main).args(["worktree", "prune"]));
        if branches {
            if let Some(b) = branch.filter(|b| b != "HEAD") {
                if sys::quiet(sys::git(&main).args(["branch", "-D", &b])) {
                    ui::info(format!("[{repo}] branch {b} deleted"));
                }
            }
        }
        ui::ok(format!("[{repo}] worktree removed"));
    }

    fs::remove_dir_all(&dir)?;
    ui::ok(format!("card {id} removed"));
    Ok(())
}

pub fn prune(cfg: &Config, dry_run: bool) -> Result<()> {
    ui::info("looking for orphaned worktree records...");
    let mut found = false;
    for repo in &cfg.known_repos() {
        let Some(main) = cfg.repo_path(repo) else { continue };
        let orphans = sys::git_orphan_worktrees(&main);
        if orphans.is_empty() {
            continue;
        }
        found = true;
        println!("\x1b[1m  {repo}\x1b[0m ({})", main.display());
        for o in &orphans {
            println!("      {o}");
        }
        if !dry_run {
            sys::quiet(sys::git(&main).args(["worktree", "prune"]));
            ui::ok(format!("[{repo}] records cleaned"));
        }
    }
    if !found {
        ui::ok("no orphaned records");
    }

    let leftovers: Vec<_> = card::list_cards(cfg)
        .into_iter()
        .map(|id| cfg.card_dir(&id))
        .filter(|d| card::repos_in(d).is_empty())
        .collect();
    if leftovers.is_empty() {
        return Ok(());
    }
    println!();
    ui::warn("card folders with no worktree inside:");
    for d in &leftovers {
        println!("      {}", d.display());
    }
    if dry_run {
        ui::info("(dry-run: nothing removed)");
        return Ok(());
    }
    // they may hold a CLAUDE.md with notes, so ask first
    if ui::confirm("   remove these folders? [y/N]: ") {
        for d in &leftovers {
            fs::remove_dir_all(d)?;
            ui::ok(format!("removed {}", d.display()));
        }
    } else {
        ui::info("kept");
    }
    Ok(())
}
