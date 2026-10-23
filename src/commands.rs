use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::{bail, Result};

use crate::config::{Config, OpenTarget};
use crate::hooks::{self, Event};
use crate::provider::{self, CardInfo};
use crate::{card, claude_md, herdr, memo, ports, procenv, sys, templates, ui, workspace};

pub struct NewArgs {
    pub id: String,
    pub slug: Option<String>,
    pub template: Option<String>,
    pub repos: Option<Vec<String>>,
    pub branch: Option<String>,
    pub base: Option<String>,
    pub fix: bool,
    pub install: bool,
    pub no_setup: bool,
    pub herdr: bool,
    pub code: bool,
    pub no_ado: bool,
    pub prompt: Option<String>,
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
    if sys::find_code().is_none() {
        bail!("`code` is not on PATH");
    }
    Ok(())
}

/// Open VS Code and/or herdr at the end of `new` / `add-repo`.
fn open_after(
    cfg: &Config,
    id: &str,
    ws: Option<&str>,
    code: bool,
    herdr_flag: bool,
) -> Result<()> {
    if code || (cfg.vscode.enabled && cfg.vscode.open_on_new) {
        if sys::find_code().is_some() {
            open_code(cfg, id)?;
        } else {
            ui::warn("vscode.open_on_new is on, but `code` is not on PATH");
        }
    }
    if herdr_flag || cfg.herdr.open_on_new {
        match ws {
            Some(ws) => {
                herdr::open(ws)?;
                fire(cfg, Event::PostOpen, id, &hooks::card_repos(cfg, id));
            }
            None => ui::warn("herdr workspace was not created; try `twig herdr <id>`"),
        }
    }
    Ok(())
}

/// Fire the card/repo hooks of a `post_*` event (failures only warn).
fn fire(cfg: &Config, ev: Event, id: &str, repos: &[hooks::RepoCtx]) {
    fire_in(cfg, ev, id, &cfg.card_dir(id), repos);
}

fn fire_in(cfg: &Config, ev: Event, id: &str, cwd: &Path, repos: &[hooks::RepoCtx]) {
    if let Err(e) = hooks::run(cfg, ev, id, cwd, repos) {
        ui::warn(format!("{e:#}"));
    }
}

fn sync_herdr(cfg: &Config, id: &str, prompt: Option<&str>) -> Option<String> {
    match herdr::sync(cfg, id, prompt) {
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

    let (cref, slug_from_id) = card::parse_ref(cfg, &a.id)?;
    let id = cref.key.clone();
    if card::is_reserved(&id) {
        bail!("{id:?} cannot be a card id (reserved folder name)");
    }
    let mut slug = a.slug.or(slug_from_id).unwrap_or_default();

    // the template is a preset: explicit flags override it
    let tpl = templates::resolve(cfg, a.template.as_deref())?;
    let tpl_name = tpl.as_ref().map(|(n, _)| n.clone());
    let (mut t_repos, mut t_install, mut t_prefix, mut t_base) = (vec![], None, None, None);
    if let Some((name, t)) = tpl {
        ui::info(format!("template: {name}"));
        t_repos = t.repos.clone();
        t_install = t.install;
        t_prefix = t.branch_prefix.clone();
        t_base = t.base.clone();
        templates::apply(cfg, t);
    }
    let install = a.install || (t_install == Some(true) && !a.no_setup);
    let no_setup = a.no_setup || (t_install == Some(false) && !a.install);

    let repos = a
        .repos
        .or_else(|| Some(t_repos).filter(|r| !r.is_empty()))
        .unwrap_or_else(|| cfg.default_repos());
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
        ui::warn(format!(
            "{} existed with no worktrees inside, reusing it",
            dir.display()
        ));
    }

    let info: Option<CardInfo> = if a.no_ado {
        None
    } else {
        provider::load(cfg, &cref, first_repo_path(cfg, &repos).as_deref())
    };
    if let Some(i) = &info {
        ui::info(format!("card: {}", i.title));
        if slug.is_empty() {
            slug = i.slug();
            ui::info(format!("slug derived from the title: {slug}"));
        }
    }

    fs::create_dir_all(&dir)?;
    cref.record(cfg);
    if let Some(i) = &info {
        provider::save_cache(cfg, &id, i);
    }
    if let Some(name) = &tpl_name {
        templates::record(cfg, &id, name);
    }

    let prefix = if a.fix {
        "fix".to_string()
    } else {
        t_prefix.unwrap_or_else(|| {
            cfg.branch_prefix(info.as_ref().map(|i| i.kind.as_str()).unwrap_or(""))
        })
    };
    let base = a
        .base
        .or(t_base)
        .unwrap_or_else(|| cfg.git.base_branch.clone());
    let opts = card::BranchOpts {
        id: &cref.number,
        slug: &slug,
        forced: a.branch.as_deref(),
        base: &base,
        prefix: &prefix,
    };

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

    // the worktrees exist now: their `.twig.toml` (and the card's) apply from here on
    apply_overrides(cfg, &id);
    card::copy_local_files(cfg, &dir);
    ports::write(cfg, &id)?;
    if cfg.vscode.enabled {
        workspace::write(cfg, &id, &title)?;
    }
    memo::ensure_link(cfg, &id);
    claude_md::write(cfg, &id, &slug, info.as_ref())?;

    if wants_setup(cfg, install, no_setup) {
        setup(cfg, &id, &repos);
    }
    fire(
        cfg,
        Event::PostCreate,
        &id,
        &hooks::repo_ctxs(cfg, &id, &repos),
    );

    // the template's prompt is a preset too: --prompt replaces it ("" = none)
    let prompt = a
        .prompt
        .or_else(|| cfg.template.as_ref().and_then(|t| t.agent_prompt.clone()));
    let ws = sync_herdr(cfg, &id, prompt.as_deref());
    open_after(cfg, &id, ws.as_deref(), a.code, a.herdr)
}

/// A broken `.twig.toml` only warns here (the card is already created); the next command on
/// the card reports it as an error.
fn apply_overrides(cfg: &mut Config, id: &str) {
    if let Err(e) = cfg.apply_card(id) {
        ui::warn(format!("{e:#}"));
    }
}

fn wants_setup(cfg: &Config, install: bool, no_setup: bool) -> bool {
    install || (cfg.worktree.auto_install && !no_setup)
}

/// Per repo: `install`, then each `post_create` command, in the worktree. Failures only warn.
fn setup(cfg: &Config, id: &str, repos: &[String]) {
    let dir = cfg.card_dir(id);
    for repo in repos {
        let Some(r) = cfg.repo(repo) else { continue };
        let cmds = Some(&r.install)
            .filter(|c| !c.is_empty())
            .into_iter()
            .chain(&r.post_create);
        let mut any = false;
        for c in cmds {
            any = true;
            ui::info(format!("[{repo}] {c}"));
            let mut cmd = sys::shell(c, &dir.join(repo));
            cmd.env("TWIG_CARD", id)
                .env("TWIG_REPO", repo)
                .envs(procenv::of(cfg, id, Some(repo)));
            if let Err(e) = sys::run(&mut cmd) {
                ui::warn(format!("[{repo}] `{c}` failed: {e:#}"));
                break;
            }
        }
        if !any {
            ui::info(format!(
                "[{repo}] no `install` or `post_create` in the config, skipping"
            ));
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
    let cref = card::CardRef::of(cfg, &a.id);
    let info = provider::load(
        cfg,
        &cref,
        existing.first().and_then(|r| cfg.repo_path(r)).as_deref(),
    );
    let slug = info.as_ref().map(CardInfo::slug).unwrap_or_default();
    let t = cfg.template.as_ref();
    let t_install = t.and_then(|t| t.install);
    let install = a.install || (t_install == Some(true) && !a.no_setup);
    let no_setup = a.no_setup || (t_install == Some(false) && !a.install);
    let prefix = if a.fix {
        "fix".to_string()
    } else {
        t.and_then(|t| t.branch_prefix.clone()).unwrap_or_else(|| {
            cfg.branch_prefix(info.as_ref().map(|i| i.kind.as_str()).unwrap_or(""))
        })
    };
    let base = a
        .base
        .or_else(|| t.and_then(|t| t.base.clone()))
        .unwrap_or_else(|| cfg.git.base_branch.clone());
    let opts = card::BranchOpts {
        id: &cref.number,
        slug: &slug,
        forced: a.branch.as_deref(),
        base: &base,
        prefix: &prefix,
    };

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

    apply_overrides(cfg, &a.id);
    card::copy_local_files(cfg, &dir);
    if wants_setup(cfg, install, no_setup) {
        setup(cfg, &a.id, &created);
    }
    regenerate(cfg, &a.id, info.as_ref())?;
    if !created.is_empty() {
        fire(
            cfg,
            Event::PostCreate,
            &a.id,
            &hooks::repo_ctxs(cfg, &a.id, &created),
        );
    }
    let ws = sync_herdr(cfg, &a.id, None);
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
    ports::write(cfg, id)?;
    if cfg.vscode.enabled {
        workspace::write(cfg, id, &title)?;
    }
    memo::ensure_link(cfg, id);
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

/// `twig open-card [id]`: open the card's provider URL (cached, else derived) in the browser.
pub fn open_card(cfg: &Config, id: Option<&str>) -> Result<()> {
    let id = match id {
        Some(id) => id.to_string(),
        None => match card::infer(cfg) {
            card::Current::Card(id) => id,
            card::Current::Dir(dir) => {
                bail!(
                    "{} is not inside a card; usage: twig open-card <id>",
                    dir.display()
                )
            }
        },
    };
    let repo = card::repos_ordered(cfg, &cfg.card_dir(&id))
        .first()
        .and_then(|r| cfg.repo_path(r));
    let cref = card::CardRef::of(cfg, &id);
    let url = provider::load(cfg, &cref, repo.as_deref())
        .map(|i| i.url)
        .filter(|u| !u.is_empty())
        .or_else(|| Some(provider::current(cfg).fallback_url(&cref)).filter(|u| !u.is_empty()))
        .ok_or_else(|| {
            let cli = provider::current(cfg).cli();
            anyhow::anyhow!("no URL for card {id}: not cached and the provider returned nothing (is `{cli}` installed and logged in?)")
        })?;
    if sys::open_url(&url) {
        ui::ok(format!("opening #{id}: {url}"));
    } else {
        ui::warn("could not launch a browser; open this URL manually");
        println!("{url}");
    }
    Ok(())
}

pub fn herdr_cmd(cfg: &Config, id: &str) -> Result<()> {
    let ws = herdr::sync(cfg, id, None)?;
    herdr::open(&ws)?;
    fire(cfg, Event::PostOpen, id, &hooks::card_repos(cfg, id));
    Ok(())
}

pub fn open_code(cfg: &Config, id: &str) -> Result<()> {
    check_code_flag(cfg, true)?;
    card::require_card(cfg, id)?;
    let file = cfg.workspace_file(id);
    if !file.exists() {
        let dir = cfg.card_dir(id);
        let info = provider::load(
            cfg,
            &card::CardRef::of(cfg, id),
            card::repos_ordered(cfg, &dir)
                .first()
                .and_then(|r| cfg.repo_path(r))
                .as_deref(),
        );
        let title = card::repos_ordered(cfg, &dir)
            .first()
            .and_then(|r| sys::git_current_branch(&dir.join(r)))
            .or_else(|| info.map(|i| i.title))
            .unwrap_or_default();
        workspace::write(cfg, id, &title)?;
    }
    launch_code(&file)?;
    ui::ok(format!("opening #{id} in VS Code"));
    fire(cfg, Event::PostOpen, id, &hooks::card_repos(cfg, id));
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
    let mut cmd = match sys::find_code() {
        Some(code) => Command::new(code),
        None => sys::tool("code"),
    };
    cmd.arg(target);
    // without this VS Code's integrated terminal inherits HERDR_* and refuses to run `herdr` (nested)
    for (k, _) in std::env::vars().filter(|(k, _)| k.starts_with("HERDR_")) {
        cmd.env_remove(k);
    }
    sys::run(&mut cmd)
}

/// One repo of a card in `twig ls`.
#[derive(Debug, Clone, serde::Serialize)]
pub struct LsRepo {
    pub repo: String,
    pub branch: Option<String>,
    /// Uncommitted changes (including untracked files).
    pub dirty: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct LsCard {
    pub id: String,
    pub repos: Vec<LsRepo>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct LsOutput {
    pub cards: Vec<LsCard>,
}

pub fn ls_collect(cfg: &Config) -> LsOutput {
    // the git calls dominate the time, so every repo is probed on its own thread (order is kept)
    let cards = std::thread::scope(|s| {
        let pending: Vec<_> = card::list_cards(cfg)
            .into_iter()
            .map(|id| {
                let dir = cfg.card_dir(&id);
                let repos: Vec<_> = card::repos_ordered(cfg, &dir)
                    .into_iter()
                    .map(|repo| {
                        let wt = dir.join(&repo);
                        s.spawn(move || LsRepo {
                            branch: sys::git_current_branch(&wt),
                            dirty: sys::git_is_dirty(&wt),
                            repo,
                        })
                    })
                    .collect();
                (id, repos)
            })
            .collect();
        pending
            .into_iter()
            .map(|(id, repos)| LsCard {
                id,
                repos: repos
                    .into_iter()
                    .map(|h| h.join().expect("ls worker panicked"))
                    .collect(),
            })
            .collect()
    });
    LsOutput { cards }
}

pub fn ls(cfg: &Config, json: bool) -> Result<()> {
    let out = ls_collect(cfg);
    if json {
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(());
    }
    if out.cards.is_empty() {
        ui::info("no active cards");
        return Ok(());
    }
    for c in out.cards {
        println!("\x1b[1m  #{}\x1b[0m", c.id);
        for r in c.repos {
            let branch = r.branch.as_deref().unwrap_or("?");
            let dirty = if r.dirty { " \x1b[33m*\x1b[0m" } else { "" };
            println!("      {:<18} {branch}{dirty}", r.repo);
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
    let info = provider::load(
        cfg,
        &card::CardRef::of(cfg, id),
        card::repos_ordered(cfg, &dir)
            .first()
            .and_then(|r| cfg.repo_path(r))
            .as_deref(),
    );
    regenerate(cfg, id, info.as_ref())?;
    fire(cfg, Event::PostRefresh, id, &hooks::card_repos(cfg, id));
    Ok(())
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

    let mut ctxs = hooks::repo_ctxs(cfg, id, &repos);
    if !force {
        hooks::run(cfg, Event::PreRm, id, &dir, &ctxs)?;
    }

    herdr::close_workspace(id);

    for repo in &repos {
        let wt = dir.join(repo);
        let branch = sys::git_current_branch(&wt);
        let Some(main) = cfg.repo_path(repo) else {
            ui::warn(format!(
                "[{repo}] main clone not found, removing only the folder"
            ));
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
    if memo::entries(cfg, id) > 0 {
        ui::info(format!(
            "memo kept at {} (`twig new {id}` links it back)",
            cfg.memo_file(id).display()
        ));
    }
    // the worktrees are gone: repo-level hooks run in the main clone, card-level ones in the
    // worktrees folder
    for c in &mut ctxs {
        if let Some(main) = cfg.repo_path(&c.name) {
            c.cwd = main;
        }
    }
    ctxs.retain(|c| c.cwd.is_dir());
    fire_in(cfg, Event::PostRm, id, &cfg.worktrees(), &ctxs);
    Ok(())
}

pub fn prune(cfg: &Config, dry_run: bool) -> Result<()> {
    ui::info("looking for orphaned worktree records...");
    let mut found = false;
    for repo in &cfg.known_repos() {
        let Some(main) = cfg.repo_path(repo) else {
            continue;
        };
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
