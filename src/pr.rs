//! `twig pr`: one pull request per repo of the card, through `gh` or `az repos`, linked to the
//! card and to each other. Also the PR lookup that `twig clean` uses.

use std::path::Path;
use std::process::Command;

use anyhow::{bail, Context, Result};
use regex::Regex;
use serde_json::Value;

use crate::card::CardRef;
use crate::config::{Config, ProviderKind};
use crate::provider::{self, CardInfo};
use crate::{card, sys, ui};

/// A pull request of one repo of the card.
#[derive(Debug, Clone)]
pub struct Pr {
    /// `gh`: the PR number; `az`: the pull request id.
    pub number: String,
    pub url: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrState {
    Open,
    Merged,
}

/// The base branch of the card: its template's `base`, else `git.base_branch`.
pub fn card_base(cfg: &Config) -> String {
    cfg.template
        .as_ref()
        .and_then(|t| t.base.clone())
        .unwrap_or_else(|| cfg.git.base_branch.clone())
}

fn az(cfg: &Config, wt: &Path) -> Command {
    let mut cmd = Command::new("az");
    cmd.current_dir(wt);
    if !cfg.provider.az.organization.is_empty() {
        cmd.args(["--organization", &cfg.provider.az.organization]);
    }
    cmd
}

fn gh(wt: &Path) -> Command {
    let mut cmd = Command::new("gh");
    cmd.current_dir(wt);
    cmd
}

/// Web URL of an `az repos pr` result (the `url` field is the REST one).
fn az_pr(v: &Value) -> Option<Pr> {
    let id = v["pullRequestId"].as_u64()?.to_string();
    let web = v["repository"]["webUrl"].as_str().unwrap_or_default();
    let url = if web.is_empty() {
        v["url"].as_str().unwrap_or_default().to_string()
    } else {
        format!("{web}/pullrequest/{id}")
    };
    Some(Pr { number: id, url })
}

/// The PR of `branch` in this worktree's repo, in the given state (best effort: `None` on any
/// provider error, so callers treat "can't tell" like "no PR").
pub fn find(cfg: &Config, wt: &Path, branch: &str, state: PrState) -> Option<Pr> {
    match cfg.provider.default {
        ProviderKind::Gh => {
            sys::which("gh")?;
            let state = match state {
                PrState::Open => "open",
                PrState::Merged => "merged",
            };
            let out = sys::output(gh(wt).args([
                "pr",
                "list",
                "--head",
                branch,
                "--state",
                state,
                "--json",
                "number,url",
                "--limit",
                "1",
            ]))
            .ok()?;
            let v: Value = serde_json::from_str(&out).ok()?;
            let p = v.as_array()?.first()?;
            Some(Pr {
                number: p["number"].as_u64()?.to_string(),
                url: p["url"].as_str().unwrap_or_default().to_string(),
            })
        }
        ProviderKind::Az => {
            sys::which("az")?;
            let status = match state {
                PrState::Open => "active",
                PrState::Merged => "completed",
            };
            let out = sys::output(az(cfg, wt).args([
                "repos",
                "pr",
                "list",
                "--source-branch",
                branch,
                "--status",
                status,
                "--output",
                "json",
            ]))
            .ok()?;
            let v: Value = serde_json::from_str(&out).ok()?;
            az_pr(v.as_array()?.first()?)
        }
    }
}

/// `owner/repo` and number of the card's GitHub issue: from the card id, else from its URL.
fn gh_issue(cref: &CardRef, info: Option<&CardInfo>) -> Option<String> {
    if let Some(r) = &cref.repo {
        return Some(format!("{r}#{}", cref.number));
    }
    let url = info.map(|i| i.url.as_str())?;
    let c = Regex::new(r"github\.com/([^/]+/[^/]+)/issues/([0-9]+)")
        .unwrap()
        .captures(url)?;
    Some(format!("{}#{}", &c[1], &c[2]))
}

/// The PR description: the card link and, for GitHub, the closing keyword.
fn body(cfg: &Config, cref: &CardRef, info: Option<&CardInfo>) -> String {
    let url = info
        .map(|i| i.url.clone())
        .filter(|u| !u.is_empty())
        .unwrap_or_else(|| provider::fallback_url(cfg, cref));
    let mut b = match url.is_empty() {
        true => format!("Card {}", cref.display()),
        false => format!("Card {}: {url}", cref.display()),
    };
    if cfg.provider.default == ProviderKind::Gh {
        if let Some(issue) = gh_issue(cref, info) {
            b += &format!("\n\nCloses {issue}");
        }
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

/// What `twig pr` asks the provider to open.
struct NewPr<'a> {
    branch: &'a str,
    base: &'a str,
    title: &'a str,
    body: &'a str,
    /// Linked on Azure DevOps (`--work-items`).
    work_item: &'a str,
    draft: bool,
}

fn create(cfg: &Config, wt: &Path, p: &NewPr) -> Result<Pr> {
    let NewPr {
        branch,
        base,
        title,
        body,
        work_item,
        draft,
    } = *p;
    match cfg.provider.default {
        ProviderKind::Gh => {
            let mut cmd = gh(wt);
            cmd.args([
                "pr", "create", "--base", base, "--head", branch, "--title", title, "--body", body,
            ]);
            if draft {
                cmd.arg("--draft");
            }
            let out = sys::output(&mut cmd)?;
            // `gh pr create` prints the URL on the last line
            let url = out.lines().last().unwrap_or_default().trim().to_string();
            let number = url.rsplit('/').next().unwrap_or_default().to_string();
            Ok(Pr { number, url })
        }
        ProviderKind::Az => {
            let mut cmd = az(cfg, wt);
            cmd.args([
                "repos",
                "pr",
                "create",
                "--source-branch",
                branch,
                "--target-branch",
                base,
                "--title",
                title,
                "--description",
                body,
                "--work-items",
                work_item,
                "--draft",
                if draft { "true" } else { "false" },
                "--output",
                "json",
            ]);
            let out = sys::output(&mut cmd)?;
            let v: Value = serde_json::from_str(&out).context("unexpected response from az")?;
            az_pr(&v).context("az repos pr create returned no pull request id")
        }
    }
}

fn update_body(cfg: &Config, wt: &Path, pr: &Pr, body: &str) -> Result<()> {
    let mut cmd = match cfg.provider.default {
        ProviderKind::Gh => {
            let mut c = gh(wt);
            c.args(["pr", "edit", &pr.url, "--body", body]);
            c
        }
        ProviderKind::Az => {
            let mut c = az(cfg, wt);
            c.args([
                "repos",
                "pr",
                "update",
                "--id",
                &pr.number,
                "--description",
                body,
                "--output",
                "none",
            ]);
            c
        }
    };
    sys::output(&mut cmd).map(|_| ())
}

fn count(wt: &Path, range: &str) -> Option<u32> {
    sys::output(sys::git(wt).args(["rev-list", "--count", range]))
        .ok()
        .and_then(|s| s.trim().parse().ok())
}

pub fn run(cfg: &Config, id: &str, draft: bool, dry_run: bool) -> Result<()> {
    let dir = card::require_card(cfg, id)?;
    let cli = cfg.provider.default.name();
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
    let body = body(cfg, &cref, info.as_ref());

    let mut prs: Vec<(String, Pr)> = vec![];
    let mut created: Vec<String> = vec![];
    let mut failed = false;
    for repo in &repos {
        let wt = dir.join(repo);
        let Some(branch) = sys::git_current_branch(&wt).filter(|b| b != "HEAD") else {
            ui::warn(format!("[{repo}] detached HEAD, skipped"));
            continue;
        };
        if let Some(pr) = find(cfg, &wt, &branch, PrState::Open) {
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
            work_item: &cref.number,
            draft,
        };
        match create(cfg, &wt, &new) {
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
            if let Err(e) = update_body(cfg, &dir.join(repo), pr, &text) {
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

    fn cref(repo: Option<&str>) -> CardRef {
        CardRef {
            key: "12".into(),
            number: "12".into(),
            repo: repo.map(String::from),
        }
    }

    #[test]
    fn gh_issue_comes_from_the_id_or_the_url() {
        assert_eq!(
            gh_issue(&cref(Some("acme/web")), None).as_deref(),
            Some("acme/web#12")
        );
        let info = CardInfo {
            url: "https://github.com/acme/api/issues/12".into(),
            ..Default::default()
        };
        assert_eq!(
            gh_issue(&cref(None), Some(&info)).as_deref(),
            Some("acme/api#12")
        );
        assert_eq!(gh_issue(&cref(None), None), None);
    }

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

    #[test]
    fn az_web_url_is_built_from_the_repository() {
        let v = serde_json::json!({
            "pullRequestId": 5,
            "url": "https://dev.azure.com/o/p/_apis/git/repositories/x/pullRequests/5",
            "repository": {"webUrl": "https://dev.azure.com/o/p/_git/api"}
        });
        assert_eq!(
            az_pr(&v).unwrap().url,
            "https://dev.azure.com/o/p/_git/api/pullrequest/5"
        );
    }
}
