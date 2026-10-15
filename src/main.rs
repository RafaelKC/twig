mod card;
mod claude_md;
mod commands;
mod config;
mod config_edit;
mod doctor;
mod herdr;
mod provider;
mod selfmgmt;
mod sys;
mod ui;
mod workspace;

use std::process::ExitCode;

use anyhow::Result;
use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::Shell;

use config::Config;

/// Per-card environment: git worktrees of several repos under <worktrees>/<id>/,
/// a herdr workspace with agents and, if enabled, a VS Code workspace.
///
/// Config: ~/.config/twig/config.toml (or TWIG_CONFIG).
#[derive(Parser)]
#[command(name = "twig", version)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Create the card: worktrees, herdr workspace and, if enabled, .code-workspace.
    ///
    /// Branch, per repo: --branch; else a branch on origin with <id> in its name
    /// (asks if there are several); else creates <prefix>/<id>-<slug> from origin/<base>.
    New {
        /// Card/issue id in the provider (accepts "123-slug").
        id: String,
        /// Only used when the branch must be CREATED; defaults to the card title.
        slug: Option<String>,
        /// Card repos; defaults to the repos with `default = true`.
        #[arg(long, value_delimiter = ',')]
        repos: Option<Vec<String>>,
        /// Force this branch in every repo.
        #[arg(long)]
        branch: Option<String>,
        /// Base for the new branch (default: git.base_branch).
        #[arg(long)]
        base: Option<String>,
        /// New branch with the fix/ prefix.
        #[arg(long)]
        fix: bool,
        /// Run each repo's `install` in the worktree.
        #[arg(long)]
        install: bool,
        /// Open herdr at the end.
        #[arg(long)]
        herdr: bool,
        /// Open VS Code at the end (error if not enabled/installed).
        #[arg(long)]
        code: bool,
        /// Don't query the provider (offline).
        #[arg(long)]
        no_ado: bool,
    },
    /// Add more repos to an existing card (safe to re-run).
    AddRepo {
        id: String,
        /// Repos to add (comma- or space-separated).
        #[arg(required = true, value_delimiter = ',')]
        repos: Vec<String>,
        /// Force this branch in the new repos.
        #[arg(long)]
        branch: Option<String>,
        /// Base for the new branch.
        #[arg(long)]
        base: Option<String>,
        /// New branch with the fix/ prefix.
        #[arg(long)]
        fix: bool,
        /// Open herdr at the end.
        #[arg(long)]
        herdr: bool,
        /// Open VS Code at the end (error if not enabled/installed).
        #[arg(long)]
        code: bool,
    },
    /// Open the card's herdr workspace (creates whatever is missing).
    Herdr { id: String },
    /// Open the card in VS Code (generates the .code-workspace if missing).
    ///
    /// Without <ID>, the card is inferred from the herdr plugin context
    /// (HERDR_PLUGIN_CONTEXT_JSON) or the current directory; outside a card, opens that folder.
    Code { id: Option<String> },
    /// List active cards and each repo's branch (* = uncommitted changes).
    #[command(alias = "list")]
    Ls,
    /// Regenerate .code-workspace and CLAUDE.md, keeping the "Contexto do card" section.
    Refresh {
        id: String,
        /// Drop the cache and fetch the card data from the provider again.
        #[arg(long)]
        refetch: bool,
    },
    /// Remove the card's worktrees and folder (refuses with uncommitted changes).
    #[command(alias = "remove")]
    Rm {
        id: String,
        /// Also delete the local branches.
        #[arg(long)]
        branches: bool,
    },
    /// Clean up orphaned worktree records and empty card folders.
    Prune {
        /// Only show what would be done.
        #[arg(short = 'n', long)]
        dry_run: bool,
    },
    /// Check dependencies, login and config without changing anything (exits 1 on error).
    Doctor,
    /// Manage the config file.
    Config {
        #[command(subcommand)]
        cmd: ConfigCmd,
    },
    /// Update twig (and its herdr plugin and skill) to the latest release.
    Update {
        /// Only report whether a new version exists.
        #[arg(long)]
        check: bool,
        /// Install this version instead of the latest (e.g. v0.2.0).
        #[arg(long)]
        version: Option<String>,
    },
    /// Remove twig, its herdr plugin, skill and completions; asks about config and az/gh/herdr.
    Uninstall {
        /// Don't ask: remove twig's pieces, keep config and dependencies.
        #[arg(short, long)]
        yes: bool,
    },
    /// Generate the shell completion script (e.g. twig completions bash > ~/.local/share/bash-completion/completions/twig).
    Completions { shell: Shell },
}

#[derive(Subcommand)]
enum ConfigCmd {
    /// Open the config in $VISUAL/$EDITOR (creates it from the template if missing) and validate on save.
    Edit,
}

fn run(cli: Cli) -> Result<()> {
    // these work without a valid config
    match cli.cmd {
        Cmd::Completions { shell } => {
            clap_complete::generate(shell, &mut Cli::command(), "twig", &mut std::io::stdout());
            return Ok(());
        }
        Cmd::Doctor => return doctor::run(),
        Cmd::Update { check, version } => return selfmgmt::update(check, version),
        Cmd::Uninstall { yes } => return selfmgmt::uninstall(yes),
        Cmd::Config { cmd: ConfigCmd::Edit } => return config_edit::edit(),
        _ => {}
    }

    let mut cfg = Config::load()?;
    match cli.cmd {
        Cmd::New { id, slug, repos, branch, base, fix, install, herdr, code, no_ado } => commands::new(
            &mut cfg,
            commands::NewArgs { id, slug, repos, branch, base, fix, install, herdr, code, no_ado },
        ),
        Cmd::AddRepo { id, repos, branch, base, fix, herdr, code } => commands::add_repo(
            &mut cfg,
            commands::AddRepoArgs { id, repos, branch, base, fix, herdr, code },
        ),
        Cmd::Herdr { id } => commands::herdr_cmd(&cfg, &id),
        Cmd::Code { id } => commands::code(&cfg, id.as_deref()),
        Cmd::Ls => commands::ls(&cfg),
        Cmd::Refresh { id, refetch } => commands::refresh(&cfg, &id, refetch),
        Cmd::Rm { id, branches } => commands::rm(&cfg, &id, branches),
        Cmd::Prune { dry_run } => commands::prune(&cfg, dry_run),
        Cmd::Completions { .. } | Cmd::Doctor | Cmd::Config { .. } | Cmd::Update { .. } | Cmd::Uninstall { .. } => {
            unreachable!()
        }
    }
}

fn main() -> ExitCode {
    // `twig ls | head` closes the pipe early; without this println! panics
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("\x1b[31m✖ {e:#}\x1b[0m");
            ExitCode::FAILURE
        }
    }
}
