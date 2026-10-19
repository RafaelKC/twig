mod card;
mod claude_md;
mod commands;
mod config;
mod config_edit;
mod doctor;
mod herdr;
mod memo;
mod provider;
mod selfmgmt;
mod status;
mod sync;
mod sys;
mod ui;
mod updatecheck;
mod workspace;

use std::process::ExitCode;

use anyhow::Result;
use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::Shell;

use config::{Config, OpenTarget};

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
        /// Run each repo's `install` and `post_create` (default with `worktree.auto_install`).
        #[arg(long)]
        install: bool,
        /// Skip `install` and `post_create`, even with `worktree.auto_install`.
        #[arg(long, conflicts_with = "install")]
        no_setup: bool,
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
        /// Run `install` and `post_create` on the new repos (default with `worktree.auto_install`).
        #[arg(long)]
        install: bool,
        /// Skip `install` and `post_create`, even with `worktree.auto_install`.
        #[arg(long, conflicts_with = "install")]
        no_setup: bool,
        /// Open herdr at the end.
        #[arg(long)]
        herdr: bool,
        /// Open VS Code at the end (error if not enabled/installed).
        #[arg(long)]
        code: bool,
    },
    /// Open a card in herdr or VS Code (target defaults to `[open] default` in the config).
    ///
    /// Without <ID>, the card is inferred like `twig code` does; a lone target (`twig open code`)
    /// is read as the target. `twig herdr` and `twig code`
    /// are aliases of `twig open <id> herdr` / `twig open [id] code`.
    Open {
        id: Option<String>,
        /// Where to open it: `default` uses `[open] default` from the config.
        #[arg(value_enum)]
        target: Option<Target>,
    },
    /// Open the card's page (work item) in the default browser (Azure DevOps only for now).
    ///
    /// Without <ID>, the card is inferred from the current directory; outside a card, it is a
    /// usage error. With `provider = "gh"` this is not supported yet.
    #[command(name = "open-card", alias = "opcd")]
    OpenCard { id: Option<String> },
    /// Open the card's herdr workspace (creates whatever is missing).
    Herdr { id: String },
    /// Open the card in VS Code (generates the .code-workspace if missing).
    ///
    /// Without <ID>, the card is inferred from the herdr plugin context
    /// (HERDR_PLUGIN_CONTEXT_JSON) or the current directory; outside a card, opens that folder.
    Code { id: Option<String> },
    /// List active cards and each repo's branch (* = uncommitted changes).
    #[command(alias = "list")]
    Ls {
        /// Machine-readable output (JSON on stdout, see docs/COMMANDS.md).
        #[arg(long)]
        json: bool,
    },
    /// Per repo: branch, uncommitted files, ahead/behind vs base and origin; herdr workspace and agent.
    ///
    /// Without <ID>, every card (compact); with an id, the details of that card.
    Status {
        id: Option<String>,
        /// Machine-readable output (JSON on stdout, see docs/COMMANDS.md).
        #[arg(long)]
        json: bool,
    },
    /// Regenerate .code-workspace and CLAUDE.md, keeping the "Card context" section.
    Refresh {
        id: String,
        /// Drop the cache and fetch the card data from the provider again.
        #[arg(long)]
        refetch: bool,
    },
    /// (in development) Append a line to the card's memory, or show it when no text is given.
    ///
    /// The memory is a MEMORY.md per card, kept in `<worktrees>/memos/<id>-<provider>/` and
    /// linked from the card folder, so it survives `twig rm`. Each entry is one line,
    /// `YYYY-MM-DD HH:MM: TEXT` (local time). The last 10 entries are copied into the context
    /// file on `new`, `add-repo` and `refresh`. The format may change before it is stable.
    ///
    /// Without --card, the card is inferred from the current directory.
    Memo {
        /// Entry to append (newlines become spaces). Without it, prints the memo path and content.
        text: Vec<String>,
        /// Card to use instead of the one inferred from the current directory.
        #[arg(short, long)]
        card: Option<String>,
    },
    /// Fetch the base branch and rebase/merge it into every repo of the card.
    ///
    /// Repos with uncommitted changes are skipped and reported; a conflict stops the run
    /// and names the repo and files. Default strategy: `[git] sync` (rebase).
    Sync {
        id: String,
        /// Merge the base branch instead of rebasing.
        #[arg(long, conflicts_with = "rebase")]
        merge: bool,
        /// Rebase onto the base branch (overrides `[git] sync = "merge"`).
        #[arg(long)]
        rebase: bool,
        /// Fetch and show what would happen, without changing any worktree.
        #[arg(short = 'n', long)]
        dry_run: bool,
    },
    /// Remove the card's worktrees, folder and herdr workspace (refuses with uncommitted changes or a running agent).
    #[command(alias = "remove")]
    Rm {
        id: String,
        /// Also delete the local branches.
        #[arg(long)]
        branches: bool,
        /// Remove even with uncommitted changes or a running agent.
        #[arg(short, long)]
        force: bool,
    },
    /// Clean up orphaned worktree records and empty card folders.
    Prune {
        /// Only show what would be done.
        #[arg(short = 'n', long)]
        dry_run: bool,
    },
    /// Check dependencies, login and config without changing anything (exits 1 on error).
    Doctor {
        /// Machine-readable output (JSON on stdout, see docs/COMMANDS.md).
        #[arg(long)]
        json: bool,
    },
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

/// Target of `twig open`.
#[derive(Clone, Copy, clap::ValueEnum)]
enum Target {
    /// The `[open] default` from the config (herdr if unset).
    Default,
    Herdr,
    Code,
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
        Cmd::Doctor { json } => return doctor::run(json),
        Cmd::Update { check, version } => return selfmgmt::update(check, version),
        Cmd::Uninstall { yes } => return selfmgmt::uninstall(yes),
        Cmd::Config {
            cmd: ConfigCmd::Edit,
        } => return config_edit::edit(),
        _ => {}
    }

    let mut cfg = Config::load()?;
    let update = cfg.update.clone();
    let json = matches!(
        cli.cmd,
        Cmd::Ls { json: true } | Cmd::Status { json: true, .. }
    );
    let result = match cli.cmd {
        Cmd::New {
            id,
            slug,
            repos,
            branch,
            base,
            fix,
            install,
            no_setup,
            herdr,
            code,
            no_ado,
        } => commands::new(
            &mut cfg,
            commands::NewArgs {
                id,
                slug,
                repos,
                branch,
                base,
                fix,
                install,
                no_setup,
                herdr,
                code,
                no_ado,
            },
        ),
        Cmd::AddRepo {
            id,
            repos,
            branch,
            base,
            fix,
            install,
            no_setup,
            herdr,
            code,
        } => commands::add_repo(
            &mut cfg,
            commands::AddRepoArgs {
                id,
                repos,
                branch,
                base,
                fix,
                install,
                no_setup,
                herdr,
                code,
            },
        ),
        Cmd::Open { id, target } => {
            // `twig open code`: a lone target name is the target, not a card id
            let (id, target) = match (id.as_deref(), target) {
                (Some(t), None) if <Target as clap::ValueEnum>::from_str(t, true).is_ok() => {
                    (None, <Target as clap::ValueEnum>::from_str(t, true).ok())
                }
                _ => (id, target),
            };
            let target = match target {
                None | Some(Target::Default) => None,
                Some(Target::Herdr) => Some(OpenTarget::Herdr),
                Some(Target::Code) => Some(OpenTarget::Code),
            };
            commands::open(&cfg, id.as_deref(), target)
        }
        Cmd::OpenCard { id } => commands::open_card(&cfg, id.as_deref()),
        Cmd::Herdr { id } => commands::herdr_cmd(&cfg, &id),
        Cmd::Code { id } => commands::code(&cfg, id.as_deref()),
        Cmd::Ls { json } => commands::ls(&cfg, json),
        Cmd::Status { id, json } => status::run(&cfg, id.as_deref(), json),
        Cmd::Refresh { id, refetch } => commands::refresh(&cfg, &id, refetch),
        Cmd::Memo { text, card } => memo::run(&cfg, &text, card.as_deref()),
        Cmd::Rm {
            id,
            branches,
            force,
        } => commands::rm(&cfg, &id, branches, force),
        Cmd::Sync {
            id,
            merge,
            rebase,
            dry_run,
        } => {
            let mode = match (merge, rebase) {
                (true, _) => Some(config::SyncMode::Merge),
                (_, true) => Some(config::SyncMode::Rebase),
                _ => None,
            };
            sync::run(&cfg, &id, mode, dry_run)
        }
        Cmd::Prune { dry_run } => commands::prune(&cfg, dry_run),
        Cmd::Completions { .. }
        | Cmd::Doctor { .. }
        | Cmd::Config { .. }
        | Cmd::Update { .. }
        | Cmd::Uninstall { .. } => {
            unreachable!()
        }
    };
    // stderr-only hint after the command's own output; silent unless a TTY and a newer
    // release, and never with --json
    if !json {
        updatecheck::maybe_hint(&update);
    }
    result
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
