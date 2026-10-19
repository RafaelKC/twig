//! The card folder's context file for the agent (CLAUDE.md, AGENTS.md...).

use std::fs;

use anyhow::Result;

use crate::config::{Config, ProviderKind};
use crate::provider::{self, CardInfo};
use crate::{card, sys, ui, workspace};

const CONTEXT_HEADING: &str = "## Card context";
const CONTEXT_PLACEHOLDER: &str = "_(to fill in)_";
// what versions before the English translation generated; still recognized when reading
const LEGACY_CONTEXT_HEADING: &str = "## Contexto do card";
const LEGACY_CONTEXT_PLACEHOLDER: &str = "_(a preencher)_";

/// The "Card context" section of an existing context file, if it has been filled in.
/// Accepts the legacy Portuguese heading too, rewriting it to the current one (body untouched).
fn kept_context(old: &str) -> Option<String> {
    let (start, heading) = [CONTEXT_HEADING, LEGACY_CONTEXT_HEADING]
        .into_iter()
        .filter_map(|h| old.find(h).map(|i| (i, h)))
        .min_by_key(|(i, _)| *i)?;
    let ctx = &old[start..];
    if ctx.contains(CONTEXT_PLACEHOLDER) || ctx.contains(LEGACY_CONTEXT_PLACEHOLDER) {
        return None;
    }
    Some(format!(
        "{CONTEXT_HEADING}{}",
        ctx[heading.len()..].trim_end()
    ))
}

pub fn write(cfg: &Config, id: &str, slug: &str, info: Option<&CardInfo>) -> Result<()> {
    let dir = cfg.card_dir(id);
    let path = dir.join(cfg.ai.context_file());
    let repos = card::repos_ordered(cfg, &dir);

    // keep the "Card context" section if it has already been filled in
    let kept = fs::read_to_string(&path)
        .ok()
        .and_then(|old| kept_context(&old));

    let mut rows = String::new();
    let mut runs = String::new();
    for repo in &repos {
        let branch = sys::git_current_branch(&dir.join(repo)).unwrap_or_else(|| "?".into());
        let main = cfg
            .repo_path(repo)
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "?".into());
        rows += &format!(
            "| {} | `{repo}` | `{branch}` | `{main}` |\n",
            cfg.label(repo)
        );
        if let Some(r) = cfg.repo(repo).filter(|r| !r.run.is_empty()) {
            runs += &format!("- `{repo}`: `{}`\n", r.run);
        }
    }
    if runs.is_empty() {
        runs = "_(no run command configured for these repos)_\n".into();
    }

    let mut heading = format!("# Card #{id}");
    match info {
        Some(i) => heading += &format!(" — {}", i.title),
        None if !slug.is_empty() => heading += &format!(" — {slug}"),
        None => {}
    }

    let mut meta = String::new();
    let url = info
        .map(|i| i.url.clone())
        .filter(|u| !u.is_empty())
        .unwrap_or_else(|| provider::fallback_url(cfg, id));
    if !url.is_empty() {
        meta += &format!("**Work item:** {url}\n");
    }
    let mut asks = String::new();
    if let Some(i) = info {
        if !i.kind.is_empty() {
            let state = if i.state.is_empty() { "?" } else { &i.state };
            meta += &format!("**Type:** {} · **State:** {state}\n", i.kind);
        }
        if !i.assigned.is_empty() {
            meta += &format!("**Assigned to:** {}\n", i.assigned);
        }
        if !i.tags.is_empty() {
            meta += &format!("**Tags:** {}\n", i.tags);
        }
        if !i.description.is_empty() || !i.acceptance.is_empty() {
            asks += "\n## What the card asks for\n\n<!-- Copied from the provider by twig. Do not edit: `twig refresh` overwrites it. -->\n";
            if !i.description.is_empty() {
                asks += &format!("\n{}\n", i.description);
            }
            if !i.acceptance.is_empty() {
                asks += &format!("\n### Acceptance criteria\n\n{}\n", i.acceptance);
            }
        }
    }

    let debug_note = if workspace::debug_count(cfg, &repos) >= 2 && cfg.vscode.enabled {
        format!(
            "To debug everything at once, the workspace defines the compound\n\
             `▶ card #{id}: start and debug all`, at the top of the Run and Debug dropdown\n\
             (`F5`). It composes the configs that already exist in each repo's `launch.json`.\n\n"
        )
    } else {
        String::new()
    };

    let vscode_note = if cfg.vscode.enabled {
        format!(
            "The same things exist as VS Code tasks in the `{id}.code-workspace` file\n\
             (`run: <repo>` and `twig: start all`).\n\n"
        )
    } else {
        String::new()
    };

    let provider_rule = match cfg.provider.default {
        ProviderKind::Az => {
            "- **The `az` CLI is available.** You can query Azure DevOps directly\n  \
             (`az boards work-item show --id <id>`, `az repos pr list`, etc.) instead of\n  \
             asking the user.\n"
        }
        ProviderKind::Gh => {
            "- **The `gh` CLI is available.** You can query GitHub directly\n  \
             (`gh issue view <id>`, `gh pr list`, etc.) instead of asking the user.\n"
        }
    };

    let memory = crate::memo::context_section(cfg, id).unwrap_or_default();

    let context = kept.unwrap_or_else(|| {
        format!(
            "{CONTEXT_HEADING}\n\n<!-- Fill in what matters: decisions made, key files,\n     \
             loose ends. This survives across sessions and `twig refresh`. -->\n\n{CONTEXT_PLACEHOLDER}"
        )
    });

    let base = &cfg.git.base_branch;
    let dir_s = dir.display();
    let body = format!(
        "{heading}\n\n\
<!-- Generated by twig {version}. `twig refresh {id}` regenerates it. -->\n\n\
{meta}{asks}\n\
## Where you are\n\n\
This folder **is not a regular clone**. Each subdirectory below is a `git worktree`\n\
dedicated to this card. The main clones live elsewhere and stay on the\n\
base branch; touching them breaks the card's isolation.\n\n\
| name | repo (worktree here) | branch | main clone |\n\
|---|---|---|---|\n\
{rows}\n\
Base branch: `{base}`\n\n\
## How to run\n\n\
{runs}\n\
{vscode_note}{debug_note}\
## Rules for this directory\n\n\
- **Work only inside `{dir_s}`.** Do not edit, check out, or commit in the\n  \
main clones listed in the table: they are shared by all cards.\n\
- **Do not switch branches inside the worktrees.** If you need another branch,\n  \
that's another card (`twig new`).\n\
- **Each repo commits separately.** There is no atomic commit across them; when the\n  \
change is coupled, mention the card (`#{id}`) in the messages for traceability.\n\
- **Dependencies and caches are per worktree.** Do not share or symlink\n  \
`node_modules`, `bin/`, `obj/` etc. between cards.\n\
- **Do not commit this file.** It lives outside the repos on purpose.\n\
{provider_rule}\n\
{memory}{context}\n",
        version = env!("CARGO_PKG_VERSION"),
    );

    fs::write(&path, body)?;
    ui::ok(format!(
        "{} generated in {}",
        cfg.ai.context_file(),
        dir.display()
    ));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_filled_context_with_current_heading() {
        let old = "# Card #1\n\n## Rules\n\n- x\n\n## Card context\n\nDecided to use X.\n\n";
        assert_eq!(
            kept_context(old).unwrap(),
            "## Card context\n\nDecided to use X."
        );
    }

    #[test]
    fn legacy_heading_is_rewritten_and_body_kept() {
        let old = "# Card #1\n\n## Contexto do card\n\n<!-- nota -->\n\nDecidimos usar X.\n";
        assert_eq!(
            kept_context(old).unwrap(),
            "## Card context\n\n<!-- nota -->\n\nDecidimos usar X."
        );
    }

    #[test]
    fn placeholder_in_either_language_means_not_filled() {
        assert_eq!(kept_context("## Card context\n\n_(to fill in)_\n"), None);
        assert_eq!(
            kept_context("## Contexto do card\n\n_(a preencher)_\n"),
            None
        );
        assert_eq!(
            kept_context("## Contexto do card\n\n_(to fill in)_\n"),
            None
        );
        assert_eq!(kept_context("## Card context\n\n_(a preencher)_\n"), None);
    }

    #[test]
    fn no_context_section_keeps_nothing() {
        assert_eq!(kept_context("# Card #1\n\n## Where you are\n"), None);
    }

    fn write_card(agent_section: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let wt = tmp.path().join("wt");
        let text = format!(
            "version = 1\n{agent_section}\n[paths]\nworktrees = \"{}\"\n",
            wt.display()
        );
        let file = tmp.path().join("config.toml");
        fs::write(&file, text).unwrap();
        let cfg = Config::load_from(&file).unwrap();
        fs::create_dir_all(cfg.card_dir("7")).unwrap();
        write(&cfg, "7", "slug", None).unwrap();
        let dir = cfg.card_dir("7");
        (tmp, dir)
    }

    #[test]
    fn claude_writes_claude_md() {
        let (_tmp, dir) = write_card("");
        assert!(dir.join("CLAUDE.md").is_file());
        assert!(!dir.join("AGENTS.md").exists());
    }

    #[test]
    fn codex_writes_agents_md_without_claude_wording() {
        let (_tmp, dir) = write_card("[agent]\nprovider = \"codex\"\n");
        let body = fs::read_to_string(dir.join("AGENTS.md")).unwrap();
        assert!(!dir.join("CLAUDE.md").exists());
        assert!(body.contains("# Card #7"));
        assert!(!body.to_lowercase().contains("claude"));
    }

    #[test]
    fn context_file_override_is_used() {
        let (_tmp, dir) =
            write_card("[agent]\nprovider = \"codex\"\ncontext_file = \"NOTES.md\"\n");
        assert!(dir.join("NOTES.md").is_file());
    }
}
