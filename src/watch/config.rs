//! `watch.toml`: the rules of `twig watch`, kept next to the config (or `TWIG_WATCH`) like the
//! templates. The file is optional; without it `twig watch` explains how to create one.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use regex::Regex;
use serde::Deserialize;

use crate::config::{config_path, Config, ProviderKind};
use crate::templates::Templates;

pub const MIN_INTERVAL_SECS: u64 = 15;

/// The whole file: settings at the root, one `[[rules]]` table per tag.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct WatchCfg {
    /// Time between cycles (`60s`, `5m`, `1h`); at least 15s. Each wait gets ±10% of jitter.
    pub interval: String,
    /// Only items assigned to the authenticated user.
    pub assigned_to_me: bool,
    /// Cards started per cycle.
    pub max_per_cycle: usize,
    /// Cards created by the watch that may be alive (in the worktrees folder, not parked) at once.
    pub max_active: usize,
    /// Time limit of each `twig new` (it may run installs).
    pub timeout: String,
    /// Shell command run after each dispatch with `TWIG_CARD`, `TWIG_TEMPLATE`, `TWIG_WATCH_RESULT`.
    pub on_trigger: String,
    /// Tag added to the item after a successful dispatch ("" = none).
    pub mark_tag: String,
    /// Let `twig new` start the herdr server. Off: with no server up, the dispatch waits.
    pub start_herdr: bool,
    pub rules: Vec<Rule>,
}

impl Default for WatchCfg {
    fn default() -> Self {
        Self {
            interval: "60s".into(),
            assigned_to_me: true,
            max_per_cycle: 2,
            max_active: 5,
            timeout: "30m".into(),
            on_trigger: String::new(),
            mark_tag: String::new(),
            start_herdr: false,
            rules: vec![],
        }
    }
}

/// One `[[rules]]` table: items of the scope that carry `tag` start a card from `template`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Rule {
    pub tag: String,
    pub template: String,
    /// Azure DevOps project (required with provider az).
    pub project: Option<String>,
    /// GitHub `owner/repo` (required with provider gh).
    pub repo: Option<String>,
    /// gh: who may add the tag (default: the authenticated user).
    pub trusted_taggers: Vec<String>,
}

impl Rule {
    /// What the rule watches: the project or the repo.
    pub fn scope(&self) -> &str {
        self.project
            .as_deref()
            .or(self.repo.as_deref())
            .unwrap_or_default()
    }
}

impl WatchCfg {
    pub fn interval(&self) -> Duration {
        parse_duration(&self.interval).unwrap_or(Duration::from_secs(60))
    }

    pub fn timeout(&self) -> Duration {
        parse_duration(&self.timeout).unwrap_or(Duration::from_secs(30 * 60))
    }

    /// The scopes to query, in file order, each with the tags of its rules.
    pub fn scopes(&self) -> Vec<(String, Vec<String>)> {
        let mut out: Vec<(String, Vec<String>)> = vec![];
        for r in &self.rules {
            match out
                .iter_mut()
                .find(|(s, _)| s.eq_ignore_ascii_case(r.scope()))
            {
                Some((_, tags)) => tags.push(r.tag.clone()),
                None => out.push((r.scope().to_string(), vec![r.tag.clone()])),
            }
        }
        out
    }
}

/// `45s`, `5m`, `2h`.
pub fn parse_duration(s: &str) -> Result<Duration> {
    let re = Regex::new(r"^([0-9]+)([smh])$").unwrap();
    let Some(c) = re.captures(s.trim()) else {
        bail!("`{s}` is not a duration (use 30s, 5m or 1h)");
    };
    let n: u64 = c[1].parse().context("duration too large")?;
    let unit = match &c[2] {
        "s" => 1,
        "m" => 60,
        _ => 3600,
    };
    Ok(Duration::from_secs(n.saturating_mul(unit)))
}

pub fn watch_path() -> Result<PathBuf> {
    if let Ok(p) = std::env::var("TWIG_WATCH") {
        return Ok(PathBuf::from(p));
    }
    let cfg = config_path()?;
    Ok(cfg
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default()
        .join("watch.toml"))
}

/// Parse a watch file (syntax and structure only).
pub fn parse_file(file: &Path) -> Result<WatchCfg> {
    let text =
        fs::read_to_string(file).with_context(|| format!("could not read {}", file.display()))?;
    toml::from_str(&text).with_context(|| format!("invalid watch file: {}", file.display()))
}

#[derive(Debug, Default, PartialEq)]
pub struct Findings {
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

/// Problems in the file. `cfg` and `tpls` are optional so the editor can still validate when
/// the config or the templates file is broken (the checks that need them are skipped).
pub fn validate(cfg: Option<&Config>, tpls: Option<&Templates>, w: &WatchCfg) -> Findings {
    let mut f = Findings::default();
    match parse_duration(&w.interval) {
        Ok(d) if d.as_secs() < MIN_INTERVAL_SECS => f.errors.push(format!(
            "interval `{}` is too short (minimum {MIN_INTERVAL_SECS}s)",
            w.interval
        )),
        Ok(_) => {}
        Err(e) => f.errors.push(format!("interval: {e}")),
    }
    match parse_duration(&w.timeout) {
        Ok(d) if d.is_zero() => f.errors.push("timeout must be above zero".into()),
        Ok(_) => {}
        Err(e) => f.errors.push(format!("timeout: {e}")),
    }
    if w.max_per_cycle == 0 {
        f.errors.push("max_per_cycle must be at least 1".into());
    }
    if w.max_active == 0 {
        f.errors.push("max_active must be at least 1".into());
    }
    if w.rules.is_empty() {
        f.errors
            .push("no [[rules]]: the watch would never start anything".into());
    }
    if !w.assigned_to_me {
        f.warnings.push(
            "assigned_to_me = false: without a remote lock, two machines running the watch can start the same card".into(),
        );
    }
    if w.mark_tag.contains(';') {
        f.errors.push("mark_tag cannot contain `;`".into());
    }

    let repo_re = Regex::new(r"^[^/#\s]+/[^/#\s]+$").unwrap();
    let kind = cfg.map(|c| c.provider.default);
    let mut seen: Vec<(String, String)> = vec![];
    let mut warned_templates: Vec<&str> = vec![];
    for (i, r) in w.rules.iter().enumerate() {
        let at = format!("rules[{i}]");
        if r.tag.trim().is_empty() {
            f.errors.push(format!("{at}: tag is empty"));
        } else if r.tag.contains(';') || r.tag != r.tag.trim() {
            f.errors.push(format!(
                "{at}: tag `{}` cannot have `;` or edge spaces",
                r.tag
            ));
        }
        match (&r.project, &r.repo, kind) {
            (None, None, _) => f.errors.push(format!(
                "{at}: no scope: set `project` (az) or `repo` (gh); there is no wildcard"
            )),
            (Some(_), Some(_), _) => f
                .errors
                .push(format!("{at}: set either `project` or `repo`, not both")),
            (Some(p), None, k) => {
                if p.trim().is_empty() {
                    f.errors.push(format!("{at}: project is empty"));
                }
                if k == Some(ProviderKind::Gh) {
                    f.errors.push(format!(
                        "{at}: `project` is for provider az; provider.default is gh (use `repo`)"
                    ));
                }
            }
            (None, Some(repo), k) => {
                if !repo_re.is_match(repo) {
                    f.errors
                        .push(format!("{at}: repo `{repo}` must be owner/repo"));
                }
                if k == Some(ProviderKind::Az) {
                    f.errors.push(format!(
                        "{at}: `repo` is for provider gh; provider.default is az (use `project`)"
                    ));
                }
            }
        }
        if !r.trusted_taggers.is_empty() && kind == Some(ProviderKind::Az) {
            f.warnings
                .push(format!("{at}: trusted_taggers only applies to provider gh"));
        }
        let key = (r.tag.to_lowercase(), r.scope().to_lowercase());
        if seen.contains(&key) {
            f.errors.push(format!(
                "{at}: another rule already has tag `{}` in `{}`",
                r.tag,
                r.scope()
            ));
        }
        seen.push(key);

        if r.template.trim().is_empty() {
            f.errors.push(format!(
                "{at}: template is empty (there is no default template)"
            ));
            continue;
        }
        let Some(tpls) = tpls else { continue };
        let Some(t) = tpls.get(&r.template) else {
            f.errors.push(format!(
                "{at}: template `{}` is not in templates.toml",
                r.template
            ));
            continue;
        };
        if warned_templates.contains(&r.template.as_str()) {
            continue;
        }
        warned_templates.push(&r.template);
        if t.agent_prompt
            .as_deref()
            .is_none_or(|p| p.trim().is_empty())
        {
            f.warnings.push(format!(
                "template `{}` has no agent_prompt: the agent would start idle, so the automatic start loses its point",
                r.template
            ));
        }
        let command = match (t.agent, cfg) {
            (Some(kind), _) => Some(kind.default_command().to_string()),
            (None, Some(c)) => Some(c.ai.command().to_string()),
            (None, None) => None,
        };
        if command.is_some_and(|c| {
            c.contains("dangerously-skip-permissions") || c.contains("bypassPermissions")
        }) {
            f.warnings.push(format!(
                "template `{}` runs its agent with permissions bypassed, with no one reviewing the card text (prompt injection): prefer --permission-mode acceptEdits with a restricted --allowedTools",
                r.template
            ));
        }
    }
    f
}

/// Validation used by `watch edit`.
pub fn check_file(file: &Path) -> Result<()> {
    let w = parse_file(file)?;
    let cfg = Config::load().ok();
    let tpls = crate::templates::load().ok().flatten();
    let found = validate(cfg.as_ref(), tpls.as_ref(), &w);
    if !found.errors.is_empty() {
        bail!("{}", found.errors.join("\n"));
    }
    for w in &found.warnings {
        crate::ui::warn(w);
    }
    Ok(())
}

pub const EDIT_TEMPLATE: &str = r#"# twig watch. Format documented in docs/CONFIG.md.
# `twig watch` polls the provider and runs `twig new <id> --template X` for tagged items.
# READ the security notes in docs/WATCH.md first: nobody reviews the card before the agent starts.

interval       = "60s"      # at least 15s
assigned_to_me = true       # only items assigned to you
max_per_cycle  = 2          # cards started per cycle
max_active     = 5          # cards created by the watch alive at once
timeout        = "30m"      # per `twig new`
# on_trigger   = "notify-send \"twig: $TWIG_CARD $TWIG_WATCH_RESULT\""
# mark_tag     = "agent:running"   # tag added to the item after a successful start

# One rule per tag and scope (az: `project`; gh: `repo`). No wildcards.
# [[rules]]
# tag      = "agent"
# template = "empreitada"   # must exist in templates.toml
# project  = "Frota"        # az
# repo     = "owner/repo"   # gh
# trusted_taggers = ["me"]  # gh: who may add the tag (default: the authenticated user)
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn tpls(s: &str) -> Templates {
        toml::from_str(s).unwrap()
    }

    fn cfg(provider: &str) -> Config {
        toml::from_str(&format!(
            "version = 1\n[provider]\ndefault = \"{provider}\"\n"
        ))
        .unwrap()
    }

    fn watch(s: &str) -> WatchCfg {
        toml::from_str(s).unwrap()
    }

    const T: &str = "[emp]\nagent_prompt = \"/deliver {id}\"\n[bare]\n";

    #[test]
    fn durations_parse_and_the_defaults_hold() {
        assert_eq!(parse_duration("45s").unwrap(), Duration::from_secs(45));
        assert_eq!(parse_duration("5m").unwrap(), Duration::from_secs(300));
        assert_eq!(parse_duration("2h").unwrap(), Duration::from_secs(7200));
        assert!(parse_duration("5").is_err());
        assert!(parse_duration("1d").is_err());
        let w = WatchCfg::default();
        assert!(w.assigned_to_me);
        assert_eq!((w.max_per_cycle, w.max_active), (2, 5));
        assert_eq!(w.interval(), Duration::from_secs(60));
    }

    #[test]
    fn unknown_keys_are_rejected() {
        assert!(toml::from_str::<WatchCfg>("bogus = 1").is_err());
        assert!(toml::from_str::<WatchCfg>("[[rules]]\ntag = \"a\"\nprovider = \"az\"").is_err());
    }

    #[test]
    fn a_good_file_has_no_findings() {
        let w = watch("[[rules]]\ntag = \"agent\"\ntemplate = \"emp\"\nproject = \"Frota\"\n");
        let f = validate(Some(&cfg("az")), Some(&tpls(T)), &w);
        assert_eq!(f, Findings::default());
    }

    #[test]
    fn rules_need_an_explicit_scope_of_the_providers_kind() {
        let t = tpls(T);
        let no_scope = watch("[[rules]]\ntag = \"a\"\ntemplate = \"emp\"\n");
        assert!(validate(Some(&cfg("az")), Some(&t), &no_scope).errors[0].contains("no scope"));
        let repo_on_az = watch("[[rules]]\ntag = \"a\"\ntemplate = \"emp\"\nrepo = \"o/r\"\n");
        assert!(validate(Some(&cfg("az")), Some(&t), &repo_on_az).errors[0]
            .contains("is for provider gh"));
        let bad_repo = watch("[[rules]]\ntag = \"a\"\ntemplate = \"emp\"\nrepo = \"repo\"\n");
        assert!(validate(Some(&cfg("gh")), Some(&t), &bad_repo).errors[0].contains("owner/repo"));
        let both =
            watch("[[rules]]\ntag = \"a\"\ntemplate = \"emp\"\nrepo = \"o/r\"\nproject = \"P\"\n");
        assert!(validate(None, Some(&t), &both).errors[0].contains("not both"));
    }

    #[test]
    fn unknown_template_duplicate_rule_and_empty_template_are_errors() {
        let t = tpls(T);
        let w = watch(
            "[[rules]]\ntag = \"agent\"\ntemplate = \"nope\"\nproject = \"P\"\n\
             [[rules]]\ntag = \"Agent\"\ntemplate = \"emp\"\nproject = \"p\"\n\
             [[rules]]\ntag = \"agent\"\ntemplate = \"\"\nproject = \"Q\"\n",
        );
        let e = validate(Some(&cfg("az")), Some(&t), &w).errors;
        assert!(e
            .iter()
            .any(|m| m.contains("`nope` is not in templates.toml")));
        assert!(e.iter().any(|m| m.contains("another rule already has tag")));
        assert!(e.iter().any(|m| m.contains("template is empty")));
    }

    #[test]
    fn same_tag_in_other_scopes_is_fine() {
        let w = watch(
            "[[rules]]\ntag = \"agent\"\ntemplate = \"emp\"\nproject = \"P\"\n\
             [[rules]]\ntag = \"agent\"\ntemplate = \"bare\"\nproject = \"Q\"\n",
        );
        let f = validate(Some(&cfg("az")), Some(&tpls(T)), &w);
        assert!(f.errors.is_empty(), "{:?}", f.errors);
        assert_eq!(w.scopes().len(), 2);
    }

    #[test]
    fn scopes_group_the_tags_of_a_project() {
        let w = watch(
            "[[rules]]\ntag = \"a\"\ntemplate = \"emp\"\nproject = \"P\"\n\
             [[rules]]\ntag = \"b\"\ntemplate = \"emp\"\nproject = \"p\"\n",
        );
        assert_eq!(
            w.scopes(),
            vec![("P".to_string(), vec!["a".to_string(), "b".to_string()])]
        );
    }

    #[test]
    fn warns_about_no_agent_prompt_bypass_and_unassigned_watching() {
        let t = tpls("[bare]\n[yolo]\nagent_prompt = \"x\"\n[yolo.hooks]\n");
        let c: Config = toml::from_str(
            "version = 1\n[agent]\ncommand = \"claude --dangerously-skip-permissions\"\n",
        )
        .unwrap();
        let w = watch(
            "assigned_to_me = false\n[[rules]]\ntag = \"a\"\ntemplate = \"bare\"\nproject = \"P\"\n\
             [[rules]]\ntag = \"b\"\ntemplate = \"yolo\"\nproject = \"P\"\n",
        );
        let f = validate(Some(&c), Some(&t), &w);
        assert!(f.errors.is_empty());
        let all = f.warnings.join("\n");
        assert!(all.contains("assigned_to_me = false"));
        assert!(all.contains("`bare` has no agent_prompt"));
        assert!(all.contains("`bare` runs its agent with permissions bypassed"));
        assert!(all.contains("`yolo` runs its agent with permissions bypassed"));
    }

    #[test]
    fn interval_has_a_floor() {
        let w = watch(
            "interval = \"10s\"\n[[rules]]\ntag = \"a\"\ntemplate = \"emp\"\nproject = \"P\"\n",
        );
        assert!(validate(None, Some(&tpls(T)), &w).errors[0].contains("too short"));
    }

    #[test]
    fn the_example_watch_file_is_valid() {
        let f = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/watch.toml");
        let w = parse_file(&f).unwrap();
        assert!(!w.rules.is_empty());
    }
}
