//! `twig exists <id>`: a scriptable answer about one card (exists? parked? in herdr? provider
//! state). Exit 0 when the card exists, 1 when it doesn't, so `twig exists 123 && ...` works.

use std::fs;

use anyhow::Result;
use serde::Serialize;

use crate::card::{self, CardRef};
use crate::config::Config;
use crate::status::{HerdrStatus, RepoStatus};
use crate::{herdr, parked, provider, status};

#[derive(Debug, Serialize)]
pub struct ParkedInfo {
    pub since: String,
}

#[derive(Debug, Serialize)]
pub struct Existence {
    pub id: String,
    /// Whether `<worktrees>/<id>/` is a card folder.
    pub exists: bool,
    pub parked: Option<ParkedInfo>,
    pub herdr: HerdrStatus,
    /// The work item state in the provider; `null` when unknown (offline, not logged in, `--local`).
    pub state: Option<String>,
    /// Local view, empty when the card doesn't exist.
    pub repos: Vec<RepoStatus>,
}

/// Provider state (best effort, silent): a fresh query, else the card cache.
fn provider_state(cfg: &Config, id: &str, exists: bool) -> Option<String> {
    let p = provider::current(cfg);
    let cref = CardRef::of(cfg, id);
    let first = card::repos_ordered(cfg, &cfg.card_dir(id))
        .first()
        .and_then(|r| cfg.repo_path(r));
    let raw = p.fetch_card(&cref, first.as_deref()).or_else(|| {
        exists
            .then(|| fs::read_to_string(provider::cache_file(cfg, id)).ok())
            .flatten()
    })?;
    Some(p.parse_card(&raw)?.state).filter(|s| !s.is_empty())
}

pub fn collect(cfg: &Config, id: &str, local: bool) -> Existence {
    let exists = !card::is_reserved(id) && cfg.card_dir(id).is_dir();
    let (repos, herdr) = if exists {
        let s = status::collect(cfg, id);
        (s.repos, s.herdr)
    } else {
        let (workspace, agents_running) = herdr::card_presence(id);
        (
            vec![],
            HerdrStatus {
                workspace,
                agents_running,
            },
        )
    };
    Existence {
        id: id.to_string(),
        exists,
        parked: parked::since(cfg, id).map(|since| ParkedInfo { since }),
        herdr,
        state: if local {
            None
        } else {
            provider_state(cfg, id, exists)
        },
        repos,
    }
}

fn print(e: &Existence) {
    if !e.exists {
        println!("\x1b[31m✖ card {} does not exist\x1b[0m", e.id);
    } else {
        println!("\x1b[32m✔ card {} exists\x1b[0m", e.id);
    }
    if let Some(s) = &e.state {
        println!("  state:  {s}");
    }
    if let Some(p) = &e.parked {
        println!("  parked: since {}", p.since);
    }
    let h = &e.herdr;
    let ws = match (h.workspace, h.agents_running) {
        (false, _) => "no workspace".to_string(),
        (true, 0) => "workspace, no agent running".to_string(),
        (true, n) => format!("workspace, {n} agent(s) running"),
    };
    println!("  herdr:  {ws}");
    for r in &e.repos {
        let dirty = if r.dirty > 0 {
            format!(" *{}", r.dirty)
        } else {
            String::new()
        };
        println!(
            "  {:<18} {}{dirty}",
            r.repo,
            r.branch.as_deref().unwrap_or("?")
        );
    }
}

/// Prints the answer and exits 1 when the card doesn't exist (errors exit 1 too, with a message).
pub fn run(cfg: &Config, raw: &str, json: bool, local: bool) -> Result<()> {
    let (id, _) = card::split_id(raw);
    let e = collect(cfg, &id, local);
    if json {
        println!("{}", serde_json::to_string_pretty(&e)?);
    } else {
        print(&e);
    }
    if !e.exists {
        std::process::exit(1);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reserved_and_missing_folders_do_not_exist() {
        let tmp = tempfile::tempdir().unwrap();
        let (cfg, id) = crate::testutil::mixed_bases_card(tmp.path());
        assert!(collect(&cfg, &id, true).exists);
        assert!(!collect(&cfg, "999", true).exists);
        assert!(!collect(&cfg, ".hidden", true).exists);
    }

    #[test]
    fn local_skips_the_provider() {
        let tmp = tempfile::tempdir().unwrap();
        let (cfg, id) = crate::testutil::mixed_bases_card(tmp.path());
        let e = collect(&cfg, &id, true);
        assert!(e.state.is_none());
        assert_eq!(e.repos.len(), 2);
    }
}
