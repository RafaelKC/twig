//! `twig pr`: one pull request per repo of the card, through the provider, linked to the card
//! and to each other.

use std::path::Path;

use anyhow::{bail, Result};

use crate::card::CardRef;
use crate::config::Config;
use crate::provider::{self, CardInfo, NewPr, Pr, PrState, Provider};
use crate::{card, sys, ui};

/// The base branch of the card: its template's `base`, else `git.base_branch`.
pub fn card_base(cfg: &Config) -> String {
    cfg.template
        .as_ref()
        .and_then(|t| t.base.clone())
        .unwrap_or_else(|| cfg.git.base_branch.clone())
}

/// The PR description: the card link and whatever the provider adds (e.g. `Closes`).
fn body(p: &dyn Provider, cref: &CardRef, info: Option<&CardInfo>) -> String {
    let url = info
        .map(|i| i.url.clone())
        .filter(|u| !u.is_empty())
        .unwrap_or_else(|| p.fallback_url(cref));
    let mut b = match url.is_empty() {
        true => format!("Card {}", cref.display()),
        false => format!("Card {}: {url}", cref.display()),
    };
    if let Some(link) = p.pr_card_link(cref, info) {
        b += &format!("\n\n{link}");
    }
    b
}

/// The description plus the list of the card's other PRs.
fn with_siblings(body: &str, repo: &str, all: &[(String, Pr)]) -> String {
    let others: Vec<String> = all
        .iter()
        .filter(|(r, _)| r != repo)
        .map(|(r, p)| format!("- {r}: {}", p.url))
        .collect();
    format!("{body}\n\nOther PRs of this card:\n{}", others.join("\n"))
}

fn count(wt: &Path, range: &str) -> Option<u32> {
    sys::output(sys::git(wt).args(["rev-list", "--count", range]))
        .ok()
        .and_then(|s| s.trim().parse().ok())
}

pub fn run(cfg: &Config, id: &str, draft: bool, dry_run: bool) -> Result<()> {
    let dir = card::require_card(cfg, id)?;
    let p = provider::current(cfg);
    let cli = p.cli();
    if sys::which(cli).is_none() {
        bail!("`{cli}` is not on PATH (provider.default = \"{cli}\")");
    }
    let repos = card::repos_ordered(cfg, &dir);
    let cref = CardRef::of(cfg, id);
    let info = provider::load(
        cfg,
        &cref,
        repos.first().and_then(|r| cfg.repo_path(r)).as_deref(),
    );
    let base = card_base(cfg);
    let body = body(p.as_ref(), &cref, info.as_ref());

    let mut prs: Vec<(String, Pr)> = vec![];
    let mut created: Vec<String> = vec![];
    let mut failed = false;
    for repo in &repos {
        let wt = dir.join(repo);
        let Some(branch) = sys::git_current_branch(&wt).filter(|b| b != "HEAD") else {
            ui::warn(format!("[{repo}] detached HEAD, skipped"));
            continue;
        };
        if let Some(pr) = p.find_pr(&wt, &branch, PrState::Open) {
            ui::info(format!("[{repo}] PR already open: {}", pr.url));
            prs.push((repo.clone(), pr));
            continue;
        }
        let _ = sys::output(sys::git(&wt).args(["fetch", "origin", &base, "--quiet"]));
        let ahead = count(&wt, &format!("origin/{base}..HEAD")).unwrap_or(0);
        if ahead == 0 {
            ui::info(format!(
                "[{repo}] {branch} has no commits ahead of origin/{base}, skipped"
            ));
            continue;
        }
        if sys::git_is_dirty(&wt) {
            ui::warn(format!(
                "[{repo}] has uncommitted changes; they will not be in the PR"
            ));
        }
        let title = info
            .as_ref()
            .map(|i| i.title.clone())
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| branch.clone());
        if dry_run {
            ui::info(format!(
                "[{repo}] would push {branch} ({ahead} commit(s)) and open a{} PR into {base}: {title}",
                if draft { " draft" } else { "" }
            ));
            continue;
        }
        if let Err(e) = sys::output(sys::git(&wt).args(["push", "-u", "origin", &branch])) {
            ui::warn(format!("[{repo}] push failed: {e:#}"));
            failed = true;
            continue;
        }
        let new = NewPr {
            branch: &branch,
            base: &base,
            title: &title,
            body: &body,
            card: &cref,
            draft,
        };
        match p.create_pr(&wt, &new) {
            Ok(pr) => {
                ui::ok(format!("[{repo}] PR opened: {}", pr.url));
                prs.push((repo.clone(), pr));
                created.push(repo.clone());
            }
            Err(e) => {
                ui::warn(format!("[{repo}] could not open the PR: {e:#}"));
                failed = true;
            }
        }
    }

    // cross-link only the PRs opened now: existing descriptions may have been edited by hand
    if prs.len() > 1 {
        for (repo, pr) in prs.iter().filter(|(r, _)| created.contains(r)) {
            let text = with_siblings(&body, repo, &prs);
            if let Err(e) = p.update_pr_body(&dir.join(repo), pr, &text) {
                ui::warn(format!("[{repo}] could not link the other PRs: {e:#}"));
            }
        }
    }

    if dry_run {
        ui::info("(dry-run: nothing pushed or opened)");
    }
    if !prs.is_empty() {
        println!();
        for (repo, pr) in &prs {
            println!("      {repo:<18} {}", pr.url);
        }
    } else if !dry_run {
        ui::info(format!("card {id} has no PR to open"));
    }
    if failed {
        bail!("some PRs could not be opened (see above)");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn siblings_list_the_other_repos_only() {
        let pr = |u: &str| Pr {
            number: "1".into(),
            url: u.into(),
        };
        let all = vec![("api".to_string(), pr("A")), ("ui".to_string(), pr("U"))];
        assert_eq!(
            with_siblings("Card #1", "api", &all),
            "Card #1\n\nOther PRs of this card:\n- ui: U"
        );
    }
}
