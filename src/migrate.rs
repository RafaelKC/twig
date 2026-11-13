//! `twig migrate-herdr`: recreate the cards' herdr workspaces in the other backend (herdr CLI
//! <-> herdr app): same tabs and panes (names and folders), one terminal per pane.

use anyhow::Result;

use crate::config::{Backend, Config};
use crate::{card, herdr, herdr_app, snapshot, ui};

/// A workspace as `(tab label, pane cwds)`.
pub type Tabs = Vec<(String, Vec<String>)>;

pub struct NewPane {
    pub cwd: String,
    pub repo: Option<String>,
    /// Typed into the new pane; empty for a plain shell.
    pub cmd: String,
}

pub struct NewTab {
    pub label: String,
    pub panes: Vec<NewPane>,
}

/// What a pane of `tab` runs in the destination. The source keeps no commands, so they come from
/// the config: the agent in `agent`, the repo's `run` in `run` (only with `autorun`).
fn panes_for(cfg: &Config, id: &str, label: &str, cwds: &[String]) -> Vec<NewPane> {
    let dir = cfg.card_dir(id);
    let repos = card::repos_ordered(cfg, &dir);
    let runnable = herdr::tab_repos(cfg, &repos, true, true);
    cwds.iter()
        .map(|cwd| {
            let repo = snapshot::repo_of(&dir, &repos, cwd).map(String::from);
            let cmd = match (&repo, label) {
                (Some(r), "agent") => herdr::pane_command(cfg, id, r, &dir.join(r), None),
                (Some(r), "run") if runnable.contains(r) => {
                    cfg.repo(r).map(|c| c.run.clone()).unwrap_or_default()
                }
                _ => String::new(),
            };
            NewPane {
                cwd: cwd.clone(),
                repo,
                cmd,
            }
        })
        .collect()
}

pub fn run(cfg: &Config, to: Backend, dry_run: bool, close_source: bool) -> Result<()> {
    let (from_name, to_name) = match to {
        Backend::App => ("herdr CLI", "herdr app"),
        Backend::Cli => ("herdr app", "herdr CLI"),
    };
    let describe = |id: &str, b: Backend| match b {
        Backend::Cli => herdr::describe_cli(id),
        Backend::App => herdr_app::describe(id),
    };
    let from = if to == Backend::App {
        Backend::Cli
    } else {
        Backend::App
    };
    let (mut done, mut skipped) = (0, 0);
    herdr::set_backend(to);
    if to == Backend::Cli && !dry_run {
        herdr::ensure_server()?;
    }
    for id in card::list_cards(cfg) {
        let tabs = match describe(&id, from) {
            Ok(Some(t)) if !t.is_empty() => t,
            Ok(_) => continue,
            Err(e) => {
                ui::warn(format!(
                    "#{id}: could not read the {from_name} workspace: {e:#}"
                ));
                continue;
            }
        };
        if matches!(describe(&id, to), Ok(Some(_))) {
            ui::info(format!("#{id}: already in the {to_name}, skipped"));
            skipped += 1;
            continue;
        }
        let new: Vec<NewTab> = tabs
            .iter()
            .filter(|(_, cwds)| !cwds.is_empty())
            .map(|(label, cwds)| NewTab {
                label: label.clone(),
                panes: panes_for(cfg, &id, label, cwds),
            })
            .collect();
        let panes: usize = new.iter().map(|t| t.panes.len()).sum();
        if dry_run {
            ui::info(format!(
                "#{id}: would create {} tab(s), {panes} pane(s) in the {to_name}",
                new.len()
            ));
            continue;
        }
        let res = match to {
            Backend::App => herdr_app::create(cfg, &id, &new),
            Backend::Cli => herdr::create_cli(cfg, &id, &new),
        };
        match res {
            Ok(()) => {
                ui::ok(format!(
                    "#{id}: {} tab(s), {panes} pane(s) created in the {to_name}",
                    new.len()
                ));
                done += 1;
                if close_source {
                    close(from, &id);
                }
            }
            Err(e) => ui::warn(format!("#{id}: migration failed: {e:#}")),
        }
    }
    ui::info(format!("{done} card(s) migrated, {skipped} already there"));
    Ok(())
}

fn close(from: Backend, id: &str) {
    herdr::set_backend(from);
    herdr::close_workspace(id);
}
