//! `twig status`: per-card state (branch, dirty files, ahead/behind, herdr). The data
//! (`CardStatus`) is serializable and built apart from the printing, so `--json` can reuse it.

use std::path::Path;

use anyhow::Result;
use serde::Serialize;

use crate::config::Config;
use crate::{card, herdr, ports, sys, ui};

/// Commits ahead/behind of a reference.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct AheadBehind {
    pub ahead: u32,
    pub behind: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct RepoStatus {
    pub repo: String,
    pub branch: Option<String>,
    /// Number of files with uncommitted changes (including untracked).
    pub dirty: usize,
    /// The base ref compared against (`origin/<base>` if it exists, else `<base>`), if any.
    pub base_ref: Option<String>,
    pub vs_base: Option<AheadBehind>,
    /// The remote branch compared against (upstream, else `origin/<branch>`), if any.
    pub origin_ref: Option<String>,
    pub vs_origin: Option<AheadBehind>,
    /// The card's dev server port for this repo (`port` in the config + the card's offset).
    pub port: Option<u16>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HerdrStatus {
    pub workspace: bool,
    /// Panes whose agent is `working` or `blocked`.
    pub agents_running: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct CardStatus {
    pub id: String,
    pub repos: Vec<RepoStatus>,
    pub herdr: HerdrStatus,
}

fn ref_exists(wt: &Path, reference: &str) -> bool {
    sys::quiet(sys::git(wt).args([
        "rev-parse",
        "--verify",
        "--quiet",
        &format!("{reference}^{{commit}}"),
    ]))
}

fn ahead_behind(wt: &Path, reference: &str) -> Option<AheadBehind> {
    let out = sys::output(sys::git(wt).args([
        "rev-list",
        "--left-right",
        "--count",
        &format!("HEAD...{reference}"),
    ]))
    .ok()?;
    let mut it = out.split_whitespace().map(|n| n.parse::<u32>().ok());
    Some(AheadBehind {
        ahead: it.next()??,
        behind: it.next()??,
    })
}

fn repo_status(cfg: &Config, dir: &Path, repo: &str, port: Option<u16>) -> RepoStatus {
    let wt = dir.join(repo);
    let branch = sys::git_current_branch(&wt).filter(|b| b != "HEAD");
    let dirty = sys::output(sys::git(&wt).args(["status", "--porcelain"]))
        .map(|s| s.lines().filter(|l| !l.trim().is_empty()).count())
        .unwrap_or(0);

    let base = &cfg.git.base_branch;
    let base_ref = [format!("origin/{base}"), base.clone()]
        .into_iter()
        .find(|r| ref_exists(&wt, r));
    let vs_base = base_ref.as_deref().and_then(|r| ahead_behind(&wt, r));

    let origin_ref = branch.as_ref().and_then(|b| {
        sys::output(sys::git(&wt).args([
            "rev-parse",
            "--abbrev-ref",
            "--symbolic-full-name",
            "@{u}",
        ]))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| Some(format!("origin/{b}")).filter(|r| ref_exists(&wt, r)))
    });
    let vs_origin = origin_ref.as_deref().and_then(|r| ahead_behind(&wt, r));

    RepoStatus {
        repo: repo.to_string(),
        branch,
        dirty,
        base_ref,
        vs_base,
        origin_ref,
        vs_origin,
        port,
    }
}

pub fn collect(cfg: &Config, id: &str) -> CardStatus {
    let dir = cfg.card_dir(id);
    let ports = ports::of(cfg, id);
    let repos = card::repos_ordered(cfg, &dir)
        .iter()
        .map(|r| {
            let port = ports.iter().find(|(p, _)| p == r).map(|(_, n)| *n);
            repo_status(cfg, &dir, r, port)
        })
        .collect();
    let (workspace, agents_running) = herdr::card_presence(id);
    CardStatus {
        id: id.to_string(),
        repos,
        herdr: HerdrStatus {
            workspace,
            agents_running,
        },
    }
}

fn fmt_ab(ab: Option<AheadBehind>) -> String {
    match ab {
        Some(a) => format!("↑{} ↓{}", a.ahead, a.behind),
        None => "-".into(),
    }
}

fn herdr_line(h: &HerdrStatus) -> String {
    match (h.workspace, h.agents_running) {
        (false, _) => "no workspace".into(),
        (true, 0) => "workspace, no agent running".into(),
        (true, n) => format!("workspace, {n} agent(s) running"),
    }
}

fn print_card(s: &CardStatus, detailed: bool) {
    println!(
        "\x1b[1m  #{}\x1b[0m  \x1b[2mherdr: {}\x1b[0m",
        s.id,
        herdr_line(&s.herdr)
    );
    for r in &s.repos {
        let branch = r.branch.as_deref().unwrap_or("?");
        let dirty = if r.dirty > 0 {
            format!(" \x1b[33m*{}\x1b[0m", r.dirty)
        } else {
            String::new()
        };
        let port = r.port.map(|p| format!("  :{p}")).unwrap_or_default();
        println!(
            "      {:<18} {branch}{dirty}  base {}  origin {}{port}",
            r.repo,
            fmt_ab(r.vs_base),
            fmt_ab(r.vs_origin)
        );
        if detailed {
            println!(
                "      {:<18} \x1b[2mbase ref: {}, origin ref: {}, uncommitted files: {}\x1b[0m",
                "",
                r.base_ref.as_deref().unwrap_or("none"),
                r.origin_ref.as_deref().unwrap_or("none"),
                r.dirty
            );
        }
    }
}

/// The config with this card's overrides (ports may differ per card); `None` if they are broken.
fn card_config(id: &str) -> Option<Config> {
    let mut c = Config::load().ok()?;
    c.apply_card(id).ok()?;
    Some(c)
}

/// Top-level `--json` shape: always `{"cards": [...]}`, even for a single card.
#[derive(Debug, Serialize)]
struct StatusOutput {
    cards: Vec<CardStatus>,
}

pub fn run(cfg: &Config, id: Option<&str>, json: bool) -> Result<()> {
    let (cards, detailed) = match id {
        Some(raw) => {
            let (id, _) = card::split_id(raw);
            card::require_card(cfg, &id)?;
            (vec![collect(cfg, &id)], true)
        }
        None => (
            card::list_cards(cfg)
                .iter()
                .map(|id| match card_config(id) {
                    Some(c) => collect(&c, id),
                    None => collect(cfg, id),
                })
                .collect(),
            false,
        ),
    };
    if json {
        println!("{}", serde_json::to_string_pretty(&StatusOutput { cards })?);
        return Ok(());
    }
    if cards.is_empty() {
        ui::info("no active cards");
    }
    for c in &cards {
        print_card(c, detailed);
    }
    Ok(())
}
