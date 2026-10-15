use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::{bail, Result};

use crate::config::Config;
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
    pub herdr: bool,
    pub code: bool,
}

/// VS Code explicitly requested (`--code` or `twig code`): fail before creating anything.
fn check_code_flag(cfg: &Config, code: bool) -> Result<()> {
    if !code {
        return Ok(());
    }
    if !cfg.vscode.enabled {
        bail!("VS Code desativado no config (vscode.enabled = false)");
    }
    if sys::which("code").is_none() {
        bail!("o `code` não está no PATH");
    }
    Ok(())
}

/// Open VS Code and/or herdr at the end of `new` / `add-repo`.
fn open_after(cfg: &Config, id: &str, ws: Option<&str>, code: bool, herdr_flag: bool) -> Result<()> {
    if code || (cfg.vscode.enabled && cfg.vscode.open_on_new) {
        if sys::which("code").is_some() {
            open_code(cfg, id)?;
        } else {
            ui::warn("vscode.open_on_new ligado, mas o `code` não está no PATH");
        }
    }
    if herdr_flag || cfg.herdr.open_on_new {
        match ws {
            Some(ws) => herdr::open(ws)?,
            None => ui::warn("workspace do herdr não foi criado; tente `twig herdr <id>`"),
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
        bail!("nenhum repo: passe --repos ou marque `default = true` em algum repo do config");
    }
    // validate (and register) the repos before creating anything
    let mut paths = vec![];
    for r in &repos {
        paths.push(card::ensure_repo(cfg, r)?);
    }

    let dir = cfg.card_dir(&id);
    if dir.is_dir() {
        if !card::repos_in(&dir).is_empty() {
            bail!("card {id} já existe: use `twig code {id}`, `twig herdr {id}` ou `twig rm {id}`");
        }
        ui::warn(format!("{} existia sem worktrees dentro, reaproveitando", dir.display()));
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
            ui::info(format!("slug derivado do título: {slug}"));
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
                bail!("[{repo}] falhou ao criar a worktree");
            }
        }
    }

    card::copy_local_files(cfg, &dir);
    if cfg.vscode.enabled {
        workspace::write(cfg, &id, &title)?;
    }
    claude_md::write(cfg, &id, &slug, info.as_ref())?;

    if a.install {
        install(cfg, &id, &repos)?;
    }

    let ws = sync_herdr(cfg, &id);
    open_after(cfg, &id, ws.as_deref(), a.code, a.herdr)
}

fn install(cfg: &Config, id: &str, repos: &[String]) -> Result<()> {
    let dir = cfg.card_dir(id);
    for repo in repos {
        match cfg.repo(repo).filter(|r| !r.install.is_empty()) {
            Some(r) => {
                ui::info(format!("[{repo}] {}...", r.install));
                if let Err(e) = sys::run(&mut sys::shell(&r.install, &dir.join(repo))) {
                    ui::warn(format!("[{repo}] install falhou: {e:#}"));
                }
            }
            None => ui::info(format!("[{repo}] sem `install` no config, pulando")),
        }
    }
    Ok(())
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
    for (repo, path) in a.repos.iter().zip(&paths) {
        if dir.join(repo).join(".git").exists() {
            ui::info(format!("[{repo}] já está no card"));
            continue;
        }
        card::make_worktree(repo, path, &dir.join(repo), &opts)?;
        added = true;
    }

    card::copy_local_files(cfg, &dir);
    regenerate(cfg, &a.id, info.as_ref())?;
    let ws = sync_herdr(cfg, &a.id);
    if added && cfg.vscode.enabled {
        ui::info("se o VS Code do card estiver aberto, aceite o reload do workspace");
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
    ui::ok(format!("abrindo #{id} no VS Code"));
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
            ui::ok(format!("não é um card; abrindo {} no VS Code", dir.display()));
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
        ui::info("nenhum card ativo");
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
        ui::info("cache do card descartado");
    }
    let info = provider::load(cfg, id, card::repos_ordered(cfg, &dir).first().and_then(|r| cfg.repo_path(r)).as_deref());
    regenerate(cfg, id, info.as_ref())
}

pub fn rm(cfg: &Config, id: &str, branches: bool) -> Result<()> {
    let dir = card::require_card(cfg, id)?;
    let repos = card::repos_in(&dir);

    for repo in &repos {
        if sys::git_is_dirty(&dir.join(repo)) {
            bail!("[{repo}] tem alteração não commitada, resolva antes");
        }
    }

    for repo in &repos {
        let wt = dir.join(repo);
        let branch = sys::git_current_branch(&wt);
        let Some(main) = cfg.repo_path(repo) else {
            ui::warn(format!("[{repo}] clone principal não encontrado, removendo só a pasta"));
            continue;
        };
        sys::run(sys::git(&main).args(["worktree", "remove", "--force", &wt.to_string_lossy()]))?;
        sys::quiet(sys::git(&main).args(["worktree", "prune"]));
        if branches {
            if let Some(b) = branch.filter(|b| b != "HEAD") {
                if sys::quiet(sys::git(&main).args(["branch", "-D", &b])) {
                    ui::info(format!("[{repo}] branch {b} apagada"));
                }
            }
        }
        ui::ok(format!("[{repo}] worktree removida"));
    }

    fs::remove_dir_all(&dir)?;
    ui::ok(format!("card {id} removido"));
    Ok(())
}

pub fn prune(cfg: &Config, dry_run: bool) -> Result<()> {
    ui::info("procurando registros de worktree órfãos...");
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
            ui::ok(format!("[{repo}] registros limpos"));
        }
    }
    if !found {
        ui::ok("nenhum registro órfão");
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
    ui::warn("pastas de card sem worktree dentro:");
    for d in &leftovers {
        println!("      {}", d.display());
    }
    if dry_run {
        ui::info("(dry-run: nada removido)");
        return Ok(());
    }
    // they may hold a CLAUDE.md with notes, so ask first
    if ui::confirm("   remover essas pastas? [y/N]: ") {
        for d in &leftovers {
            fs::remove_dir_all(d)?;
            ui::ok(format!("removida {}", d.display()));
        }
    } else {
        ui::info("mantidas");
    }
    Ok(())
}
