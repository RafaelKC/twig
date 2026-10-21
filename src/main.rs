mod card;
mod claude_md;
mod clean;
mod commands;
mod config;
mod config_edit;
mod config_kv;
mod doctor;
mod herdr;
mod hooks;
mod memo;
mod overrides;
mod ports;
mod pr;
mod procenv;
mod provider;
mod selfmgmt;
mod status;
mod sync;
mod sys;
mod templates;
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
    /// Skip every lifecycle hook (`[hooks]`, `[repos.<name>.hooks]`) for this invocation.
    #[arg(long, global = true)]
    no_hooks: bool,

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
        /// Card/issue id in the provider (accepts "123-slug"; with provider gh also
        /// "owner/repo#123", "repo#123" with provider.gh.owner, or the issue URL).
        id: String,
        /// Only used when the branch must be CREATED; defaults to the card title.
        slug: Option<String>,
        /// Preset from templates.toml (repos, agent, install, hooks); default: `[templates] default`.
        #[arg(long)]
        template: Option<String>,
        /// Card repos; defaults to the template's, else the repos with `default = true`.
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
    /// Open the card's page (work item or issue) in the default browser.
    ///
    /// Without <ID>, the card is inferred from the current directory; outside a card, it is a
    /// usage error.
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
    /// Show the card's dev server ports (`port` of each repo + the card's offset).
    ///
    /// Without <ID>, the card of the current directory. `--env` prints `export` lines, for a
    /// terminal outside herdr: `eval "$(twig ports --env)"`.
    Ports {
        id: Option<String>,
        /// Print `export TWIG_PORT_<REPO>=<port>` lines.
        #[arg(long)]
        env: bool,
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
    /// Push each repo's branch and open one PR per repo that has commits ahead of the base.
    ///
    /// Title from the card, description linking the card (and `Closes owner/repo#N` on GitHub;
    /// the work item is linked on Azure DevOps), the sibling PRs linked to each other. A branch
    /// that already has an open PR just prints it. Without <ID>, the card of the current directory.
    Pr {
        id: Option<String>,
        /// Open the PRs as drafts.
        #[arg(long)]
        draft: bool,
        /// Show what would be pushed and opened, without doing it.
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
    /// Remove finished cards: every branch merged into the base, or the work item closed.
    ///
    /// Lists the candidates and asks per card (or --yes). Removal is `twig rm`: it refuses
    /// uncommitted changes or a running agent unless --force.
    Clean {
        /// Only list the finished cards.
        #[arg(short = 'n', long)]
        dry_run: bool,
        /// Don't ask: remove every finished card.
        #[arg(short, long)]
        yes: bool,
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
    /// Manage card templates (templates.toml).
    Template {
        #[command(subcommand)]
        cmd: TemplateCmd,
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
    /// Print one value by dotted key (e.g. git.base_branch, repos.my-back.run); tables and arrays print as TOML.
    Get {
        /// Dotted key, e.g. `open.default` or `repos.my-back.run`.
        key: String,
    },
    /// Set one value by dotted key, validated, keeping comments and formatting (bool/int/string inferred; arrays as `a,b` or `[a,b]`).
    Set {
        /// Dotted key, e.g. `git.base_branch` or `repos.my-back.run`.
        key: String,
        /// New value.
        value: String,
    },
}

#[derive(Subcommand)]
enum TemplateCmd {
    /// List the templates and their repos.
    Ls,
    /// Show one template.
    Show { name: String },
    /// Open templates.toml in $VISUAL/$EDITOR (creates it if missing) and validate on save.
    Edit,
}

fn run(cli: Cli) -> Result<()> {
    if cli.no_hooks {
        hooks::disable();
    }
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
        Cmd::Config {
            cmd: ConfigCmd::Get { key },
        } => return config_kv::get(&key),
        Cmd::Config {
            cmd: ConfigCmd::Set { key, value },
        } => return config_kv::set(&key, &value),
        Cmd::Template { cmd } => {
            return match cmd {
                TemplateCmd::Ls => templates::ls(),
                TemplateCmd::Show { name } => templates::show(&name),
                TemplateCmd::Edit => config_edit::edit_file(
                    &templates::templates_path()?,
                    templates::EDIT_TEMPLATE,
                    "templates",
                    templates::check_file,
                ),
            }
        }
        _ => {}
    }

    let mut cfg = Config::load()?;
    let update = cfg.update.clone();
    let mut cli = cli;
    normalize_ids(&cfg, &mut cli.cmd)?;
    // existing cards keep the agent and hooks of the template they were created from
    // an existing card brings its template (agent, hooks) and its config overrides
    let card_id = match &cli.cmd {
        Cmd::AddRepo { id, .. }
        | Cmd::Herdr { id }
        | Cmd::Refresh { id, .. }
        | Cmd::Sync { id, .. }
        | Cmd::Rm { id, .. }
        | Cmd::Pr { id: Some(id), .. }
        | Cmd::Ports { id: Some(id), .. }
        | Cmd::Status { id: Some(id), .. }
        | Cmd::OpenCard { id: Some(id) }
        | Cmd::Memo { card: Some(id), .. }
        | Cmd::Open { id: Some(id), .. }
        | Cmd::Code { id: Some(id) } => Some(id.clone()),
        Cmd::Open { id: None, .. }
        | Cmd::Code { id: None }
        | Cmd::Pr { id: None, .. }
        | Cmd::Ports { id: None, .. }
        | Cmd::OpenCard { id: None }
        | Cmd::Memo { card: None, .. } => match card::infer(&cfg) {
            card::Current::Card(id) => Some(id),
            card::Current::Dir(_) => None,
        },
        _ => None,
    };
    if let Some(id) = &card_id {
        templates::activate(&mut cfg, id);
        cfg.apply_card(id)?;
    }
    let json = matches!(
        cli.cmd,
        Cmd::Ls { json: true } | Cmd::Status { json: true, .. }
    );
    let result = match cli.cmd {
        Cmd::New {
            id,
            slug,
            template,
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
                template,
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
        Cmd::Ports { id, env } => match id {
            Some(id) => ports::show(&cfg, &id, env),
            None => match card::infer(&cfg) {
                card::Current::Card(id) => ports::show(&cfg, &id, env),
                card::Current::Dir(dir) => Err(anyhow::anyhow!(
                    "{} is not inside a card; usage: twig ports <id>",
                    dir.display()
                )),
            },
        },
        Cmd::Clean {
            dry_run,
            yes,
            branches,
            force,
        } => clean::run(&cfg, dry_run, yes, branches, force),
        Cmd::Pr { id, draft, dry_run } => match id {
            Some(id) => pr::run(&cfg, &id, draft, dry_run),
            None => match card::infer(&cfg) {
                card::Current::Card(id) => pr::run(&cfg, &id, draft, dry_run),
                card::Current::Dir(dir) => Err(anyhow::anyhow!(
                    "{} is not inside a card; usage: twig pr <id>",
                    dir.display()
                )),
            },
        },
        Cmd::Completions { .. }
        | Cmd::Doctor { .. }
        | Cmd::Config { .. }
        | Cmd::Template { .. }
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

/// Ids of existing cards may be typed as `owner/repo#123`: turn them into the card folder name.
fn normalize_ids(cfg: &Config, cmd: &mut Cmd) -> Result<()> {
    let id = match cmd {
        Cmd::AddRepo { id, .. }
        | Cmd::Herdr { id }
        | Cmd::Refresh { id, .. }
        | Cmd::Sync { id, .. }
        | Cmd::Rm { id, .. } => Some(id),
        Cmd::Open { id: Some(id), .. }
        | Cmd::OpenCard { id: Some(id) }
        | Cmd::Code { id: Some(id) }
        | Cmd::Status { id: Some(id), .. }
        | Cmd::Pr { id: Some(id), .. }
        | Cmd::Ports { id: Some(id), .. }
        | Cmd::Memo { card: Some(id), .. } => Some(id),
        _ => None,
    };
    if let Some(id) = id {
        *id = card::key(cfg, id)?;
    }
    Ok(())
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
