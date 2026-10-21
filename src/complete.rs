//! Shell completions, answered by the binary itself (`COMPLETE=<shell> twig`): the shell
//! script is a few lines that call back into twig, so it never gets out of date after an
//! update, and values (card ids, repos, templates) come from the config and the cards on disk.

use std::io::Write;

use anyhow::Result;
use clap_complete::engine::CompletionCandidate;
use clap_complete::env::{Bash, Elvish, EnvCompleter, Fish, Powershell, Zsh};
use clap_complete::Shell;

use crate::config::Config;
use crate::{card, provider, templates};

/// Card ids, with the cached title as help. Never queries the provider (it runs on every TAB).
pub fn cards() -> Vec<CompletionCandidate> {
    let Ok(cfg) = Config::load() else {
        return vec![];
    };
    let p = provider::current(&cfg);
    card::list_cards(&cfg)
        .into_iter()
        .map(|id| {
            let title = std::fs::read_to_string(provider::cache_file(&cfg, &id))
                .ok()
                .and_then(|raw| p.parse_card(&raw))
                .map(|i| i.title);
            let c = CompletionCandidate::new(&id);
            match title {
                Some(t) => c.help(Some(t.into())),
                None => c,
            }
        })
        .collect()
}

/// `twig open <TAB>`: a lone target is accepted where the id goes (`twig open code`).
pub fn cards_or_targets() -> Vec<CompletionCandidate> {
    let mut c = cards();
    c.extend(["default", "herdr", "code"].map(CompletionCandidate::new));
    c
}

pub fn repos() -> Vec<CompletionCandidate> {
    Config::load()
        .map(|cfg| cfg.repos.keys().map(CompletionCandidate::new).collect())
        .unwrap_or_default()
}

pub fn templates() -> Vec<CompletionCandidate> {
    templates::load()
        .ok()
        .flatten()
        .map(|t| t.keys().map(CompletionCandidate::new).collect())
        .unwrap_or_default()
}

/// `twig completions <shell>`: the registration script, the same one `COMPLETE=<shell> twig`
/// prints. It calls `twig` from the PATH.
pub fn registration(shell: Shell) -> Result<()> {
    let completer: &dyn EnvCompleter = match shell {
        Shell::Bash => &Bash,
        Shell::Zsh => &Zsh,
        Shell::Fish => &Fish,
        Shell::PowerShell => &Powershell,
        Shell::Elvish => &Elvish,
        _ => anyhow::bail!("unsupported shell: {shell}"),
    };
    let mut buf = Vec::new();
    completer.write_registration("COMPLETE", "twig", "twig", "twig", &mut buf)?;
    std::io::stdout().write_all(&buf)?;
    Ok(())
}
