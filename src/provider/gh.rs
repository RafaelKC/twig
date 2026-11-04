//! GitHub issues and pull requests through the `gh` CLI.

use std::path::Path;
use std::process::Command;

use anyhow::{bail, Context, Result};
use regex::Regex;
use serde_json::Value;

use super::{truncate, Candidate, CardInfo, NewPr, Pr, PrState, Provider};
use crate::card::CardRef;
use crate::config::Gh;
use crate::doctor::{succeeds, Report};
use crate::sys;

const BASE_FIELDS: &str = "number,title,url,body,state,labels,assignees";

pub struct GitHub<'a> {
    cfg: &'a Gh,
}

impl<'a> GitHub<'a> {
    pub fn new(cfg: &'a Gh) -> Self {
        Self { cfg }
    }
}

fn gh(wt: &Path) -> Command {
    let mut cmd = sys::tool("gh");
    cmd.current_dir(wt);
    cmd
}

/// `owner/repo#123`, `repo#123` (owner from `default_owner`) or a GitHub issue URL.
pub fn parse_ref(raw: &str, default_owner: &str) -> Option<CardRef> {
    let url = Regex::new(r"^https?://github\.com/([^/\s]+)/([^/\s]+)/issues/([0-9]+)/?$").unwrap();
    let short = Regex::new(r"^(?:([^/#\s]+)/)?([^/#\s]+)#([0-9]+)$").unwrap();
    let (owner, repo, number) = if let Some(c) = url.captures(raw) {
        (c[1].to_string(), c[2].to_string(), c[3].to_string())
    } else {
        let c = short.captures(raw)?;
        let owner = match c.get(1) {
            Some(o) => o.as_str().to_string(),
            None if !default_owner.is_empty() => default_owner.to_string(),
            None => return None,
        };
        (owner, c[2].to_string(), c[3].to_string())
    };
    Some(CardRef {
        key: format!("{repo}-{number}"),
        number,
        repo: Some(format!("{owner}/{repo}")),
    })
}

/// Issues of a `gh issue list --json number,labels,state` result, in `scope` (`owner/repo`).
fn parse_listed(raw: &str, scope: &str) -> Result<Vec<Candidate>> {
    let v: Value = serde_json::from_str(raw).context("unexpected response from gh")?;
    let items = v.as_array().context("unexpected response from gh")?;
    Ok(items
        .iter()
        .filter_map(|it| {
            Some(Candidate {
                id: format!("{scope}#{}", it["number"].as_u64()?),
                scope: scope.to_string(),
                tags: it["labels"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|l| l["name"].as_str().map(String::from))
                    .collect(),
                state: it["state"].as_str().unwrap_or_default().to_string(),
            })
        })
        .collect())
}

/// Who added `tag` last, from `name<TAB>login` lines of the issue timeline's `labeled` events.
fn last_tagger(tsv: &str, tag: &str) -> Option<String> {
    tsv.lines()
        .filter_map(|l| l.split_once('\t'))
        .rfind(|(name, _)| name.trim().eq_ignore_ascii_case(tag))
        .map(|(_, login)| login.trim().to_string())
}

/// `owner/repo#N` of the card's issue: from the card id, else from its cached URL.
fn issue_of(card: &CardRef, info: Option<&CardInfo>) -> Option<String> {
    if let Some(r) = &card.repo {
        return Some(format!("{r}#{}", card.number));
    }
    let url = info.map(|i| i.url.as_str())?;
    let c = Regex::new(r"github\.com/([^/]+/[^/]+)/issues/([0-9]+)")
        .unwrap()
        .captures(url)?;
    Some(format!("{}#{}", &c[1], &c[2]))
}

impl Provider for GitHub<'_> {
    fn cli(&self) -> &'static str {
        "gh"
    }

    fn cache_name(&self) -> &'static str {
        ".gh.json"
    }

    fn parse_id(&self, raw: &str) -> Option<Result<CardRef>> {
        if !raw.contains('#') && !raw.starts_with("https://github.com/") {
            return None;
        }
        Some(match parse_ref(raw, &self.cfg.owner) {
            Some(r) => Ok(r),
            None if Regex::new(r"^[^/#\s]+#[0-9]+$").unwrap().is_match(raw) => {
                Err(anyhow::anyhow!(
                    "{raw:?}: repo#123 needs provider.gh.owner in the config; use owner/repo#123"
                ))
            }
            None => Err(anyhow::anyhow!(
                "{raw:?} is not a card id (expected 123, owner/repo#123 or an issue URL)"
            )),
        })
    }

    fn fetch_card(&self, card: &CardRef, repo_dir: Option<&Path>) -> Option<String> {
        sys::which("gh")?;
        let view = |fields: &str| {
            let mut cmd = sys::tool("gh");
            cmd.args(["issue", "view", &card.number, "--json", fields]);
            if let Some(repo) = &card.repo {
                cmd.args(["--repo", repo]);
            } else if let Some(dir) = repo_dir {
                cmd.current_dir(dir);
            }
            sys::output(&mut cmd).ok()
        };
        // `subIssues` needs a recent gh: without it, retry with the basic fields
        view(&format!("{BASE_FIELDS},subIssues")).or_else(|| view(BASE_FIELDS))
    }

    fn children(&self, raw: &str) -> Option<Vec<String>> {
        let d: Value = serde_json::from_str(raw).ok()?;
        // absent when the data came from a gh without `subIssues`: unknown, not "no children"
        let subs = d.get("subIssues")?.as_array()?;
        Some(
            subs.iter()
                .filter_map(|s| s["number"].as_u64().map(|n| n.to_string()))
                .collect(),
        )
    }

    fn parse_card(&self, raw: &str) -> Option<CardInfo> {
        let d: Value = serde_json::from_str(raw).ok()?;
        let names = |k: &str, field: &str| -> String {
            d[k].as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v[field].as_str())
                        .collect::<Vec<_>>()
                        .join("; ")
                })
                .unwrap_or_default()
        };
        let labels = names("labels", "name");
        let kind = if labels.to_lowercase().contains("bug") {
            "Bug"
        } else {
            "Issue"
        };
        let info = CardInfo {
            title: d["title"].as_str().unwrap_or_default().to_string(),
            url: d["url"].as_str().unwrap_or_default().to_string(),
            kind: kind.to_string(),
            state: d["state"].as_str().unwrap_or_default().to_string(),
            tags: labels,
            assigned: names("assignees", "login"),
            description: truncate(d["body"].as_str().unwrap_or_default().trim(), 2500),
            acceptance: String::new(),
            raw: raw.to_string(),
        };
        (!info.title.is_empty()).then_some(info)
    }

    fn fallback_url(&self, card: &CardRef) -> String {
        match &card.repo {
            Some(repo) => format!("https://github.com/{repo}/issues/{}", card.number),
            None => String::new(),
        }
    }

    fn is_finished(&self, state: &str) -> bool {
        state.eq_ignore_ascii_case("closed")
    }

    fn context_hint(&self, card: &CardRef) -> String {
        let view = match &card.repo {
            Some(r) => format!("gh issue view {} --repo {r}", card.number),
            None => "gh issue view <id>".to_string(),
        };
        format!(
            "- **The `gh` CLI is available.** You can query GitHub directly\n  \
             (`{view}`, `gh pr list`, etc.) instead of asking the user.\n"
        )
    }

    fn pr_card_link(&self, card: &CardRef, info: Option<&CardInfo>) -> Option<String> {
        issue_of(card, info).map(|i| format!("Closes {i}"))
    }

    fn find_pr(&self, wt: &Path, branch: &str, state: PrState) -> Option<Pr> {
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

    fn create_pr(&self, wt: &Path, p: &NewPr) -> Result<Pr> {
        let mut cmd = gh(wt);
        cmd.args([
            "pr", "create", "--base", p.base, "--head", p.branch, "--title", p.title, "--body",
            p.body,
        ]);
        if p.draft {
            cmd.arg("--draft");
        }
        let out = sys::output(&mut cmd)?;
        // `gh pr create` prints the URL on the last line
        let url = out.lines().last().unwrap_or_default().trim().to_string();
        if url.is_empty() {
            bail!("gh pr create printed no URL");
        }
        let number = url.rsplit('/').next().unwrap_or_default().to_string();
        Ok(Pr { number, url })
    }

    fn update_pr_body(&self, wt: &Path, pr: &Pr, body: &str) -> Result<()> {
        sys::output(gh(wt).args(["pr", "edit", &pr.url, "--body", body])).map(|_| ())
    }

    fn query_tagged(&self, scope: &str, tags: &[String], mine: bool) -> Result<Vec<Candidate>> {
        sys::which("gh").context("gh not found on PATH")?;
        // `--label` repeated is an AND: one call per tag, merged by issue
        let mut found: Vec<Candidate> = vec![];
        for tag in tags {
            let mut cmd = sys::tool("gh");
            cmd.args([
                "issue", "list", "-R", scope, "--label", tag, "--state", "open",
            ]);
            if mine {
                cmd.args(["--assignee", "@me"]);
            }
            cmd.args(["--limit", "100", "--json", "number,labels,state"]);
            for c in parse_listed(&sys::output(&mut cmd)?, scope)? {
                if !found.iter().any(|f| f.id == c.id) {
                    found.push(c);
                }
            }
        }
        Ok(found)
    }

    fn is_watch_id(&self, id: &str) -> bool {
        id.contains('/') && parse_ref(id, "").is_some_and(|r| r.repo.is_some())
    }

    fn trusted_tagger(&self, c: &Candidate, tag: &str, trusted: &[String]) -> Result<bool> {
        let (repo, number) = c.id.rsplit_once('#').context("malformed issue id")?;
        let mut cmd = sys::tool("gh");
        cmd.args([
            "api",
            &format!("repos/{repo}/issues/{number}/timeline"),
            "--paginate",
            "--jq",
            r#".[] | select(.event == "labeled") | [.label.name, .actor.login] | @tsv"#,
        ]);
        let Some(who) = last_tagger(&sys::output(&mut cmd)?, tag) else {
            return Ok(false);
        };
        if trusted.is_empty() {
            let me = sys::output(sys::tool("gh").args(["api", "user", "--jq", ".login"]))?;
            return Ok(who.eq_ignore_ascii_case(me.trim()));
        }
        Ok(trusted.iter().any(|t| t.eq_ignore_ascii_case(&who)))
    }

    fn add_tag(&self, c: &Candidate, tag: &str) -> Result<()> {
        let (repo, number) = c.id.rsplit_once('#').context("malformed issue id")?;
        sys::output(sys::tool("gh").args(["issue", "edit", number, "-R", repo, "--add-label", tag]))
            .map(|_| ())
    }

    fn doctor(&self, r: &mut Report) {
        if r.require_bin("gh", "provider.default = \"gh\"") {
            if succeeds("gh", &["auth", "status"]) {
                r.ok("gh logged in");
            } else {
                r.warn("gh is not logged in (gh auth login)");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listed_issues_become_candidates() {
        let raw = r#"[{"number":4,"labels":[{"name":"agent"},{"name":"bug"}],"state":"OPEN"},
                       {"number":5,"labels":[{"name":"agent:done"}],"state":"OPEN"}]"#;
        let c = parse_listed(raw, "acme/web").unwrap();
        assert_eq!(c[0].id, "acme/web#4");
        assert_eq!(c[0].tags, ["agent", "bug"]);
        assert_eq!(c[1].tags, ["agent:done"]);
        assert!(parse_listed("{}", "a/b").is_err());
    }

    #[test]
    fn the_last_labeled_event_of_the_tag_names_the_tagger() {
        let tsv = "agent\tmallory\nbug\tbob\nagent\tRafaelKC\n";
        assert_eq!(last_tagger(tsv, "Agent").as_deref(), Some("RafaelKC"));
        assert_eq!(last_tagger(tsv, "bug").as_deref(), Some("bob"));
        assert_eq!(last_tagger(tsv, "other"), None);
    }

    #[test]
    fn watch_ids_are_owner_repo_number() {
        let cfg = Gh::default();
        let gh = GitHub::new(&cfg);
        assert!(gh.is_watch_id("acme/web#4"));
        assert!(!gh.is_watch_id("web#4"));
        assert!(!gh.is_watch_id("4"));
        assert!(!gh.is_watch_id("acme/web#4; rm -rf /"));
    }

    #[test]
    fn children_are_the_sub_issue_numbers() {
        let cfg = Gh::default();
        let gh = GitHub::new(&cfg);
        let with = r#"{"title":"t","subIssues":[{"number":5},{"number":9}]}"#;
        assert_eq!(gh.children(with), Some(vec!["5".into(), "9".into()]));
        assert_eq!(gh.children(r#"{"title":"t","subIssues":[]}"#), Some(vec![]));
        // data without `subIssues` (older gh or cache): unknown
        assert_eq!(gh.children(r#"{"title":"t"}"#), None);
    }

    fn card(repo: Option<&str>) -> CardRef {
        CardRef {
            key: "12".into(),
            number: "12".into(),
            repo: repo.map(String::from),
        }
    }

    #[test]
    fn issue_comes_from_the_id_or_the_url() {
        assert_eq!(
            issue_of(&card(Some("acme/web")), None).as_deref(),
            Some("acme/web#12")
        );
        let info = CardInfo {
            url: "https://github.com/acme/api/issues/12".into(),
            ..Default::default()
        };
        assert_eq!(
            issue_of(&card(None), Some(&info)).as_deref(),
            Some("acme/api#12")
        );
        assert_eq!(issue_of(&card(None), None), None);
    }
}
