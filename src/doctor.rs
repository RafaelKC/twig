//! `twig doctor`: check dependencies, login and config without changing anything.

use std::path::Path;
use std::process::Command;

use anyhow::{bail, Result};

use crate::config::{config_path, Config, ProviderKind};
use crate::sys;

#[derive(Default)]
struct Report {
    errors: usize,
    warnings: usize,
}

impl Report {
    fn section(&self, title: &str) {
        println!("\n\x1b[1m{title}\x1b[0m");
    }

    fn ok(&self, msg: impl AsRef<str>) {
        println!("  \x1b[32m✔\x1b[0m {}", msg.as_ref());
    }

    fn info(&self, msg: impl AsRef<str>) {
        println!("  \x1b[36m·\x1b[0m {}", msg.as_ref());
    }

    fn warn(&mut self, msg: impl AsRef<str>) {
        self.warnings += 1;
        println!("  \x1b[33m!\x1b[0m {}", msg.as_ref());
    }

    fn error(&mut self, msg: impl AsRef<str>) {
        self.errors += 1;
        println!("  \x1b[31m✖\x1b[0m {}", msg.as_ref());
    }

    /// Binary on PATH; error if missing. Returns whether it was found.
    fn require_bin(&mut self, bin: &str, why: &str) -> bool {
        match sys::which(bin) {
            Some(p) => {
                self.ok(format!("{bin} ({})", p.display()));
                true
            }
            None => {
                self.error(format!("{bin} not found on PATH ({why})"));
                false
            }
        }
    }
}

fn git_global(key: &str) -> String {
    sys::output(Command::new("git").args(["config", "--global", key]))
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

fn succeeds(bin: &str, args: &[&str]) -> bool {
    sys::quiet(Command::new(bin).args(args))
}

/// First existing folder walking up from `p`.
fn nearest_existing(p: &Path) -> Option<&Path> {
    p.ancestors().find(|a| a.exists())
}

pub fn run() -> Result<()> {
    let mut r = Report::default();

    r.section("config");
    let path = config_path()?;
    let cfg = match Config::load_from(&path) {
        Ok(cfg) => {
            r.ok(format!("{} valid", path.display()));
            Some(cfg)
        }
        Err(e) => {
            r.error(format!("{e:#}"));
            r.info("fix it with `twig config edit`; checks that depend on the config were skipped");
            None
        }
    };

    r.section("git");
    if r.require_bin("git", "required") {
        for key in ["user.name", "user.email"] {
            match git_global(key) {
                v if v.is_empty() => r.warn(format!("{key} not set (git config --global {key} ...)")),
                v => r.ok(format!("{key} = {v}")),
            }
        }
    }

    r.section("herdr");
    if r.require_bin("herdr", "required") {
        if succeeds("herdr", &["workspace", "list"]) {
            r.ok("herdr server responding");
        } else {
            r.info("herdr server stopped (twig starts it when needed)");
        }
    }

    let Some(cfg) = cfg else {
        return finish(&r);
    };

    r.section("VS Code");
    if cfg.vscode.enabled {
        r.require_bin("code", "vscode.enabled = true");
    } else {
        r.info("disabled in the config (vscode.enabled = false)");
    }

    r.section("provider");
    match cfg.provider.default {
        ProviderKind::Az => {
            if r.require_bin("az", "provider.default = \"az\"") {
                if succeeds("az", &["extension", "show", "--name", "azure-devops", "--output", "none"]) {
                    r.ok("azure-devops extension installed");
                    // `az account show` isn't enough: ADO access may come from `az devops login` (PAT)
                    let mut args = vec!["devops", "project", "list", "--top", "1", "--output", "none"];
                    let org = &cfg.provider.az.organization;
                    if !org.is_empty() {
                        args.extend(["--organization", org.as_str()]);
                    }
                    if succeeds("az", &args) {
                        r.ok("Azure DevOps access ok");
                    } else {
                        r.warn("az has no access to Azure DevOps (az login or az devops login; check provider.az.organization)");
                    }
                } else {
                    r.warn("azure-devops extension missing (az extension add --name azure-devops)");
                }
            }
        }
        ProviderKind::Gh => {
            if r.require_bin("gh", "provider.default = \"gh\"") {
                if succeeds("gh", &["auth", "status"]) {
                    r.ok("gh logged in");
                } else {
                    r.warn("gh is not logged in (gh auth login)");
                }
            }
        }
    }

    r.section("AI");
    let ai_bin = cfg.ai.command.split_whitespace().next().unwrap_or_default();
    if ai_bin.is_empty() {
        r.error("ai.command is empty");
    } else {
        r.require_bin(ai_bin, "ai.command");
    }

    r.section("folders");
    let wt = cfg.worktrees();
    if wt.is_dir() {
        r.ok(format!("worktrees: {}", wt.display()));
    } else if nearest_existing(&wt).is_some_and(Path::is_dir) {
        r.info(format!("worktrees: {} does not exist yet (created on the first `twig new`)", wt.display()));
    } else {
        r.error(format!("worktrees: {} cannot be created", wt.display()));
    }

    r.section("repos");
    if cfg.repos.is_empty() {
        r.warn("no repos registered (see docs/CONFIG.md → Configuring repos)");
    } else if cfg.default_repos().is_empty() {
        r.warn("no repo with `default = true`: `twig new` will require --repos");
    }
    for name in cfg.repos.keys() {
        match cfg.repo_path(name) {
            Some(p) => r.ok(format!("{name} ({})", p.display())),
            None => r.error(format!("{name}: clone not found or not a git repo")),
        }
    }

    r.section("orphaned worktrees");
    let mut any = false;
    for repo in cfg.known_repos() {
        let Some(main) = cfg.repo_path(&repo) else { continue };
        for o in sys::git_orphan_worktrees(&main) {
            r.warn(format!("{repo}: {o}"));
            any = true;
        }
    }
    if any {
        r.info("clean up with `twig prune`");
    } else {
        r.ok("none");
    }

    finish(&r)
}

fn finish(r: &Report) -> Result<()> {
    println!();
    if r.errors > 0 {
        bail!("{} error(s), {} warning(s)", r.errors, r.warnings);
    }
    if r.warnings > 0 {
        println!("\x1b[33m! all working, with {} warning(s)\x1b[0m", r.warnings);
    } else {
        println!("\x1b[32m✔ all good\x1b[0m");
    }
    Ok(())
}
