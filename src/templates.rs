//! Card templates: named presets (`twig new ID --template NAME`) kept in `templates.toml`,
//! next to the config (or `TWIG_TEMPLATES`). The file is optional.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use indexmap::IndexMap;
use serde::Deserialize;

use crate::config::{config_path, AgentKind, Config, Hooks};
use crate::{hooks, ui};

/// Marker in the card folder that records which template created it.
const MARKER: &str = ".twig-template";

/// One `[name]` table of the templates file.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Template {
    pub repos: Vec<String>,
    pub agent: Option<AgentKind>,
    pub install: Option<bool>,
    pub branch_prefix: Option<String>,
    pub base: Option<String>,
    /// Card-level hooks that only apply to cards created from this template.
    pub hooks: Hooks,
}

pub type Templates = IndexMap<String, Template>;

pub fn templates_path() -> Result<PathBuf> {
    if let Ok(p) = std::env::var("TWIG_TEMPLATES") {
        return Ok(PathBuf::from(p));
    }
    let cfg = config_path()?;
    Ok(cfg
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default()
        .join("templates.toml"))
}

/// Parse a templates file (syntax and structure only).
pub fn parse_file(file: &Path) -> Result<Templates> {
    let text =
        fs::read_to_string(file).with_context(|| format!("could not read {}", file.display()))?;
    toml::from_str(&text).with_context(|| format!("invalid templates file: {}", file.display()))
}

/// The templates file, or `None` when it does not exist.
pub fn load() -> Result<Option<Templates>> {
    let file = templates_path()?;
    if !file.exists() {
        return Ok(None);
    }
    parse_file(&file).map(Some)
}

/// Problems in the templates (empty names, unknown repos, bad hooks). `cfg` is optional so the
/// editor can still validate when the config itself is broken.
pub fn validate(cfg: Option<&Config>, t: &Templates) -> Vec<String> {
    let mut errors = vec![];
    for (name, tpl) in t {
        if name.trim().is_empty() {
            errors.push("a template has an empty name".to_string());
        }
        for (i, r) in tpl.repos.iter().enumerate() {
            if r.trim().is_empty() {
                errors.push(format!("[{name}] repos[{i}] is an empty name"));
            } else if let Some(cfg) = cfg {
                if !cfg.repos.contains_key(r) {
                    errors.push(format!(
                        "[{name}] repo `{r}` is not in the config (add it under [repos.{r}])"
                    ));
                }
            }
        }
        for e in hooks::validate_hooks(&format!("{name}.hooks"), &tpl.hooks).1 {
            errors.push(format!("[{name}] {e}"));
        }
    }
    errors
}

/// Validation used by `template edit`.
pub fn check_file(file: &Path) -> Result<()> {
    let t = parse_file(file)?;
    let cfg = Config::load().ok();
    let errors = validate(cfg.as_ref(), &t);
    if !errors.is_empty() {
        bail!("{}", errors.join("\n"));
    }
    Ok(())
}

fn names(t: &Templates) -> String {
    t.keys().cloned().collect::<Vec<_>>().join(", ")
}

/// The template for `twig new`: `--template`, else `[templates] default`, else none.
pub fn resolve(cfg: &Config, explicit: Option<&str>) -> Result<Option<(String, Template)>> {
    let Some(name) = explicit.or(cfg.templates.default.as_deref()) else {
        return Ok(None);
    };
    let from = if explicit.is_some() {
        "--template"
    } else {
        "templates.default"
    };
    let Some(mut all) = load()? else {
        bail!(
            "{from} = `{name}` but there is no templates file ({}; see docs/CONFIG.md)",
            templates_path()?.display()
        );
    };
    match all.shift_remove(name) {
        Some(t) => Ok(Some((name.to_string(), t))),
        None if all.is_empty() => bail!("unknown template `{name}`: the templates file is empty"),
        None => bail!("unknown template `{name}` (available: {})", names(&all)),
    }
}

/// Make the template active for this run: its agent and card-level hooks.
pub fn apply(cfg: &mut Config, t: Template) {
    if let Some(kind) = t.agent {
        cfg.ai.use_agent(kind);
    }
    cfg.template = Some(t);
}

/// Remember which template created the card.
pub fn record(cfg: &Config, id: &str, name: &str) {
    let _ = fs::write(cfg.card_dir(id).join(MARKER), format!("{name}\n"));
}

/// For an existing card: re-activate the template it was created from (best effort).
pub fn activate(cfg: &mut Config, id: &str) {
    let Ok(raw) = fs::read_to_string(cfg.card_dir(id).join(MARKER)) else {
        return;
    };
    let name = raw.trim();
    if name.is_empty() {
        return;
    }
    match load() {
        Ok(Some(mut all)) => match all.shift_remove(name) {
            Some(t) => apply(cfg, t),
            None => ui::warn(format!(
                "card {id} was created from template `{name}`, which no longer exists"
            )),
        },
        Ok(None) => ui::warn(format!(
            "card {id} was created from template `{name}`, but there is no templates file"
        )),
        Err(e) => ui::warn(format!("templates ignored: {e:#}")),
    }
}

pub fn ls() -> Result<()> {
    let Some(all) = load()? else {
        ui::info(format!(
            "no templates file ({}); `twig template edit` creates one",
            templates_path()?.display()
        ));
        return Ok(());
    };
    if all.is_empty() {
        ui::info("the templates file has no templates");
    }
    for (name, t) in &all {
        println!("\x1b[1m  {name}\x1b[0m  {}", t.repos.join(", "));
    }
    Ok(())
}

pub fn show(name: &str) -> Result<()> {
    let Some(all) = load()? else {
        bail!("no templates file ({})", templates_path()?.display());
    };
    let Some(t) = all.get(name) else {
        bail!("unknown template `{name}` (available: {})", names(&all));
    };
    println!("\x1b[1m{name}\x1b[0m");
    println!("  repos          {}", t.repos.join(", "));
    if let Some(a) = t.agent {
        println!("  agent          {}", a.default_command());
    }
    if let Some(i) = t.install {
        println!("  install        {i}");
    }
    if let Some(p) = &t.branch_prefix {
        println!("  branch_prefix  {p}");
    }
    if let Some(b) = &t.base {
        println!("  base           {b}");
    }
    for (ev, cmds) in hooks::entries(&t.hooks) {
        for c in cmds {
            println!("  hooks.{ev:<12} {c}");
        }
    }
    Ok(())
}

pub const EDIT_TEMPLATE: &str = r#"# twig templates. Format documented in docs/CONFIG.md.
# One table per template; use with `twig new <id> --template NAME`.
#
# [fullstack]
# repos         = ["api", "ui"]    # repos from your config
# agent         = "claude"         # "claude" | "codex"
# install       = true             # run each repo's install/post_create
# branch_prefix = "feat"           # optional
# base          = "main"           # optional
#
# [fullstack.hooks]                # only for cards created from this template
# post_create = ["echo created $TWIG_CARD"]
"#;
