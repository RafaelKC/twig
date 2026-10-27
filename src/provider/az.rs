//! Azure DevOps through the `az` CLI (`az boards`, `az repos`).

use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};
use serde_json::Value;

use super::{strip_html, CardInfo, NewPr, Pr, PrState, Provider};
use crate::card::CardRef;
use crate::config::Az;
use crate::doctor::{succeeds, Report};
use crate::sys;

pub struct AzureDevOps<'a> {
    cfg: &'a Az,
}

impl<'a> AzureDevOps<'a> {
    pub fn new(cfg: &'a Az) -> Self {
        Self { cfg }
    }

    /// `az <args> [--organization X]`: az only accepts `--organization` after the subcommand.
    fn az(&self, wt: Option<&Path>, args: &[&str]) -> Command {
        let mut cmd = sys::tool("az");
        if let Some(wt) = wt {
            cmd.current_dir(wt);
        }
        cmd.args(args);
        if !self.cfg.organization.is_empty() {
            cmd.args(["--organization", &self.cfg.organization]);
        }
        cmd
    }
}

/// Web URL of an `az repos pr` result (the `url` field is the REST one).
fn pr_of(v: &Value) -> Option<Pr> {
    let id = v["pullRequestId"].as_u64()?.to_string();
    let web = v["repository"]["webUrl"].as_str().unwrap_or_default();
    let url = if web.is_empty() {
        v["url"].as_str().unwrap_or_default().to_string()
    } else {
        format!("{web}/pullrequest/{id}")
    };
    Some(Pr { number: id, url })
}

/// Web page of a work item. `_links.html.href` when present; otherwise built from the REST `url`
/// (`<org>/_apis/wit/workItems/<id>`) and the project. Never a `/_apis/` URL (it returns JSON,
/// and old caches hold it); empty when it can't be built, so the caller uses the fallback.
fn web_url(d: &Value, project: &str) -> String {
    if let Some(href) = d["_links"]["html"]["href"]
        .as_str()
        .filter(|h| !h.is_empty() && !h.contains("/_apis/"))
    {
        return href.to_string();
    }
    let org = d["url"]
        .as_str()
        .and_then(|u| u.split_once("/_apis/"))
        .map(|(org, _)| org);
    let id = d["id"].as_u64();
    match (org, id) {
        (Some(org), Some(id)) if !project.is_empty() => {
            format!("{org}/{}/_workitems/edit/{id}", project.replace(' ', "%20"))
        }
        _ => String::new(),
    }
}

impl Provider for AzureDevOps<'_> {
    fn cli(&self) -> &'static str {
        "az"
    }

    fn cache_name(&self) -> &'static str {
        ".ado.json"
    }

    fn fetch_card(&self, card: &CardRef, _repo_dir: Option<&Path>) -> Option<String> {
        sys::which("az")?;
        let mut cmd = self.az(
            None,
            &[
                "boards",
                "work-item",
                "show",
                "--id",
                &card.number,
                "--output",
                "json",
            ],
        );
        sys::output(&mut cmd).ok()
    }

    fn parse_card(&self, raw: &str) -> Option<CardInfo> {
        let d: Value = serde_json::from_str(raw).ok()?;
        let f = &d["fields"];
        let field = |k: &str| -> String {
            match &f[k] {
                Value::String(s) => s.clone(),
                Value::Object(o) => o
                    .get("displayName")
                    .or_else(|| o.get("name"))
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                Value::Null => String::new(),
                v => v.to_string(),
            }
        };
        let url = web_url(&d, &field("System.TeamProject"));
        let info = CardInfo {
            title: field("System.Title"),
            url,
            kind: field("System.WorkItemType"),
            state: field("System.State"),
            tags: field("System.Tags"),
            assigned: field("System.AssignedTo"),
            description: strip_html(&field("System.Description"), 2500),
            acceptance: strip_html(&field("Microsoft.VSTS.Common.AcceptanceCriteria"), 1200),
            raw: raw.to_string(),
        };
        (!info.title.is_empty()).then_some(info)
    }

    fn children(&self, raw: &str) -> Option<Vec<String>> {
        let d: Value = serde_json::from_str(raw).ok()?;
        // `relations` is absent when the work item has none
        Some(
            d["relations"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|r| r["rel"] == "System.LinkTypes.Hierarchy-Forward")
                .filter_map(|r| r["url"].as_str()?.rsplit('/').next().map(String::from))
                .collect(),
        )
    }

    fn fallback_url(&self, card: &CardRef) -> String {
        self.cfg.item_url.replace("{id}", &card.number)
    }

    fn is_finished(&self, state: &str) -> bool {
        ["closed", "done", "removed"].contains(&state.to_lowercase().as_str())
    }

    fn context_hint(&self, _card: &CardRef) -> String {
        "- **The `az` CLI is available.** You can query Azure DevOps directly\n  \
         (`az boards work-item show --id <id>`, `az repos pr list`, etc.) instead of\n  \
         asking the user.\n"
            .to_string()
    }

    fn find_pr(&self, wt: &Path, branch: &str, state: PrState) -> Option<Pr> {
        sys::which("az")?;
        let status = match state {
            PrState::Open => "active",
            PrState::Merged => "completed",
        };
        let out = sys::output(&mut self.az(
            Some(wt),
            &[
                "repos",
                "pr",
                "list",
                "--source-branch",
                branch,
                "--status",
                status,
                "--output",
                "json",
            ],
        ))
        .ok()?;
        let v: Value = serde_json::from_str(&out).ok()?;
        pr_of(v.as_array()?.first()?)
    }

    fn create_pr(&self, wt: &Path, p: &NewPr) -> Result<Pr> {
        let mut cmd = self.az(
            Some(wt),
            &[
                "repos",
                "pr",
                "create",
                "--source-branch",
                p.branch,
                "--target-branch",
                p.base,
                "--title",
                p.title,
                "--description",
                p.body,
                "--work-items",
                &p.card.number,
                "--draft",
                if p.draft { "true" } else { "false" },
                "--output",
                "json",
            ],
        );
        let out = sys::output(&mut cmd)?;
        let v: Value = serde_json::from_str(&out).context("unexpected response from az")?;
        pr_of(&v).context("az repos pr create returned no pull request id")
    }

    fn update_pr_body(&self, wt: &Path, pr: &Pr, body: &str) -> Result<()> {
        let mut cmd = self.az(
            Some(wt),
            &[
                "repos",
                "pr",
                "update",
                "--id",
                &pr.number,
                "--description",
                body,
                "--output",
                "none",
            ],
        );
        sys::output(&mut cmd).map(|_| ())
    }

    fn doctor(&self, r: &mut Report) {
        if !r.require_bin("az", "provider.default = \"az\"") {
            return;
        }
        if !succeeds(
            "az",
            &[
                "extension",
                "show",
                "--name",
                "azure-devops",
                "--output",
                "none",
            ],
        ) {
            r.warn("azure-devops extension missing (az extension add --name azure-devops)");
            return;
        }
        r.ok("azure-devops extension installed");
        // `az account show` isn't enough: ADO access may come from `az devops login` (PAT)
        let mut args = vec![
            "devops", "project", "list", "--top", "1", "--output", "none",
        ];
        if !self.cfg.organization.is_empty() {
            args.extend(["--organization", self.cfg.organization.as_str()]);
        }
        if succeeds("az", &args) {
            r.ok("Azure DevOps access ok");
        } else {
            r.warn("az has no access to Azure DevOps (az login or az devops login; check provider.az.organization)");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn children_are_the_hierarchy_forward_relations() {
        let cfg = crate::config::Az::default();
        let az = AzureDevOps::new(&cfg);
        let raw = r#"{"relations":[
            {"rel":"System.LinkTypes.Hierarchy-Forward","url":"https://x/_apis/wit/workItems/51234"},
            {"rel":"System.LinkTypes.Hierarchy-Reverse","url":"https://x/_apis/wit/workItems/1"},
            {"rel":"System.LinkTypes.Hierarchy-Forward","url":"https://x/_apis/wit/workItems/51240"}]}"#;
        assert_eq!(az.children(raw), Some(vec!["51234".into(), "51240".into()]));
        assert_eq!(az.children("{}"), Some(vec![]));
        assert_eq!(az.children("not json"), None);
    }

    #[test]
    fn web_url_is_built_from_the_repository() {
        let v = serde_json::json!({
            "pullRequestId": 5,
            "url": "https://dev.azure.com/o/p/_apis/git/repositories/x/pullRequests/5",
            "repository": {"webUrl": "https://dev.azure.com/o/p/_git/api"}
        });
        assert_eq!(
            pr_of(&v).unwrap().url,
            "https://dev.azure.com/o/p/_git/api/pullrequest/5"
        );
    }

    fn parse(raw: &str) -> Option<CardInfo> {
        let cfg = Az::default();
        AzureDevOps::new(&cfg).parse_card(raw)
    }

    #[test]
    fn card_url_prefers_the_html_link() {
        let raw = r#"{"id":7,"url":"https://dev.azure.com/o/_apis/wit/workItems/7",
            "_links":{"html":{"href":"https://dev.azure.com/o/p/_workitems/edit/7"}},
            "fields":{"System.Title":"t","System.TeamProject":"p"}}"#;
        assert_eq!(
            parse(raw).unwrap().url,
            "https://dev.azure.com/o/p/_workitems/edit/7"
        );
    }

    #[test]
    fn card_url_is_built_when_the_html_link_is_missing_or_rest() {
        let missing = r#"{"id":7,"url":"https://dev.azure.com/o/_apis/wit/workItems/7",
            "fields":{"System.Title":"t","System.TeamProject":"My Proj"}}"#;
        let rest = r#"{"id":7,"url":"https://dev.azure.com/o/_apis/wit/workItems/7",
            "_links":{"html":{"href":"https://dev.azure.com/o/_apis/wit/workItems/7"}},
            "fields":{"System.Title":"t","System.TeamProject":"My Proj"}}"#;
        for raw in [missing, rest] {
            assert_eq!(
                parse(raw).unwrap().url,
                "https://dev.azure.com/o/My%20Proj/_workitems/edit/7"
            );
        }
    }

    #[test]
    fn card_url_is_empty_rather_than_a_rest_url() {
        let raw = r#"{"id":7,"url":"https://dev.azure.com/o/_apis/wit/workItems/7",
            "fields":{"System.Title":"t"}}"#;
        assert_eq!(parse(raw).unwrap().url, "");
    }

    #[test]
    fn finished_states() {
        let cfg = Az::default();
        let az = AzureDevOps::new(&cfg);
        assert!(az.is_finished("Closed") && az.is_finished("done"));
        assert!(!az.is_finished("Active") && !az.is_finished("Resolved"));
    }

    #[test]
    fn organization_goes_after_the_subcommand() {
        let cfg = Az {
            organization: "https://dev.azure.com/o".into(),
            ..Az::default()
        };
        let cmd = AzureDevOps::new(&cfg).az(None, &["boards", "work-item", "show"]);
        let args: Vec<_> = cmd.get_args().map(|a| a.to_string_lossy()).collect();
        assert_eq!(
            args,
            [
                "boards",
                "work-item",
                "show",
                "--organization",
                "https://dev.azure.com/o"
            ]
        );
    }
}
