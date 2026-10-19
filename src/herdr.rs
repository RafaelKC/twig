//! The card's herdr workspace: `agent` tab (one pane per repo running the AI) and
//! `run` tab (one pane per repo with `run` and `autorun`). The workspace is found by label = id.

use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use serde_json::Value;

use crate::config::Config;
use crate::{card, sys, ui};

fn herdr(args: &[&str]) -> Result<Value> {
    let out = sys::output(Command::new("herdr").args(args))?;
    serde_json::from_str(&out).with_context(|| format!("unexpected response from herdr: herdr {}", args.join(" ")))
}

fn herdr_quiet(args: &[&str]) {
    sys::quiet(Command::new("herdr").args(args));
}

fn str_at(v: &Value, ptr: &str) -> Result<String> {
    v.pointer(ptr)
        .and_then(Value::as_str)
        .map(String::from)
        .with_context(|| format!("herdr response without {ptr}"))
}

fn server_up() -> bool {
    sys::quiet(Command::new("herdr").args(["workspace", "list"]))
}

/// Make sure the herdr server is up (the workspace/pane CLI talks to it).
pub fn ensure_server() -> Result<()> {
    if sys::which("herdr").is_none() {
        bail!("herdr not found on PATH");
    }
    if server_up() {
        return Ok(());
    }
    ui::info("starting the herdr server...");
    let mut cmd = Command::new("herdr");
    cmd.arg("server").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    cmd.spawn().context("could not start the herdr server")?;
    for _ in 0..20 {
        thread::sleep(Duration::from_millis(250));
        if server_up() {
            return Ok(());
        }
    }
    bail!("herdr server did not respond")
}

pub fn find_workspace(id: &str) -> Result<Option<String>> {
    let v = herdr(&["workspace", "list"])?;
    Ok(v.pointer("/result/workspaces")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find(|w| w["label"] == id)
        .and_then(|w| w["workspace_id"].as_str().map(String::from)))
}

/// Panes whose agent is busy: `working` or `blocked` (waiting on the user).
fn busy_panes(panes: &Value) -> Vec<String> {
    panes
        .pointer("/result/panes")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|p| matches!(p["agent_status"].as_str(), Some("working" | "blocked")))
        .filter_map(|p| p["pane_id"].as_str().map(String::from))
        .collect()
}

/// Panes with a running agent in the card's workspace. Empty when herdr isn't installed,
/// isn't running or has no workspace for the card (it never starts the server).
pub fn running_agents(id: &str) -> Vec<String> {
    if sys::which("herdr").is_none() || !server_up() {
        return Vec::new();
    }
    let Ok(Some(ws)) = find_workspace(id) else { return Vec::new() };
    herdr(&["pane", "list", "--workspace", &ws]).map(|v| busy_panes(&v)).unwrap_or_default()
}

/// Close the card's herdr workspace, if there is one. Failures are only warnings.
pub fn close_workspace(id: &str) {
    if sys::which("herdr").is_none() || !server_up() {
        return;
    }
    match find_workspace(id) {
        Ok(Some(ws)) => {
            if sys::quiet(Command::new("herdr").args(["workspace", "close", &ws])) {
                ui::ok(format!("herdr workspace #{id} closed"));
            } else {
                ui::warn(format!("could not close the herdr workspace #{id}"));
            }
        }
        Ok(None) => {}
        Err(e) => ui::warn(format!("herdr: {e:#}")),
    }
}

fn find_tab(ws: &str, label: &str) -> Result<Option<String>> {
    let v = herdr(&["tab", "list", "--workspace", ws])?;
    Ok(v.pointer("/result/tabs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find(|t| t["label"] == label)
        .and_then(|t| t["tab_id"].as_str().map(String::from)))
}

/// A tab's panes, in order: (pane_id, cwd, foreground_cwd).
fn tab_panes(ws: &str, tab: &str) -> Result<Vec<(String, String, String)>> {
    let v = herdr(&["pane", "list", "--workspace", ws])?;
    Ok(v.pointer("/result/panes")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|p| p["tab_id"] == tab)
        .map(|p| {
            let s = |k: &str| p[k].as_str().unwrap_or_default().to_string();
            (s("pane_id"), s("cwd"), s("foreground_cwd"))
        })
        .collect())
}

/// Make sure a repo has a pane in a tab. Creates the tab if missing; an existing pane
/// is left alone unless `force` (root pane just created along with the workspace).
fn ensure_panel(cfg: &Config, ws: &str, tab_label: &str, repo: &str, cmd: &str, cwd: &Path, mut force: bool) -> Result<()> {
    let cwd_s = cwd.to_string_lossy().into_owned();
    let pane = match find_tab(ws, tab_label)? {
        None => {
            let v = herdr(&["tab", "create", "--workspace", ws, "--cwd", &cwd_s, "--label", tab_label, "--no-focus"])?;
            force = true;
            str_at(&v, "/result/root_pane/pane_id")?
        }
        Some(tab) => {
            let panes = tab_panes(ws, &tab)?;
            // herdr doesn't expose pane labels; the repo's pane is found by cwd
            match panes.iter().find(|(_, c, f)| *c == cwd_s || *f == cwd_s) {
                Some((id, _, _)) => id.clone(),
                None => {
                    // first split is side by side; later ones stack on the last pane
                    let last = panes.last().map(|p| p.0.clone()).context("herdr tab without panes")?;
                    let dir = if panes.len() <= 1 { "right" } else { "down" };
                    let v = herdr(&["pane", "split", &last, "--direction", dir, "--cwd", &cwd_s, "--no-focus"])?;
                    force = true;
                    str_at(&v, "/result/pane/pane_id")?
                }
            }
        }
    };

    herdr_quiet(&["pane", "rename", &pane, &cfg.short(repo)]);
    if force && !cmd.is_empty() {
        herdr_quiet(&["pane", "run", &pane, cmd]);
        ui::ok(format!("herdr: {tab_label}/{}", cfg.short(repo)));
    }
    Ok(())
}

/// Create in herdr whatever the card is missing. Idempotent: existing things are left alone,
/// and panes closed by the user come back.
pub fn sync(cfg: &Config, id: &str) -> Result<String> {
    ensure_server()?;
    let dir = card::require_card(cfg, id)?;
    let repos = card::repos_ordered(cfg, &dir);
    let first = repos.first().with_context(|| format!("card {id} has no worktrees"))?;

    let (ws, fresh) = match find_workspace(id)? {
        Some(ws) => (ws, false),
        None => {
            let cwd = dir.join(first).to_string_lossy().into_owned();
            let v = herdr(&["workspace", "create", "--cwd", &cwd, "--label", id, "--no-focus"])?;
            let ws = str_at(&v, "/result/workspace/workspace_id")?;
            herdr_quiet(&["tab", "rename", &str_at(&v, "/result/tab/tab_id")?, "agent"]);
            ui::ok(format!("herdr workspace #{id} created"));
            (ws, true)
        }
    };

    for (i, repo) in repos.iter().enumerate() {
        ensure_panel(cfg, &ws, "agent", repo, cfg.ai.command(), &dir.join(repo), fresh && i == 0)?;
    }
    for repo in &repos {
        let Some(r) = cfg.repo(repo) else { continue };
        if r.run.is_empty() || !r.autorun {
            continue;
        }
        ensure_panel(cfg, &ws, "run", repo, &r.run, &dir.join(repo), false)?;
    }
    Ok(ws)
}

/// Focus the workspace and, when outside herdr, open herdr in the terminal.
pub fn open(ws: &str) -> Result<()> {
    herdr_quiet(&["workspace", "focus", ws]);
    if std::env::var("HERDR_ENV").as_deref() != Ok("1") {
        sys::run(&mut Command::new("herdr"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn busy_panes_picks_working_and_blocked_agents() {
        let v = json!({"result": {"panes": [
            {"pane_id": "w1:p1", "agent_status": "working"},
            {"pane_id": "w1:p2", "agent_status": "idle"},
            {"pane_id": "w1:p3", "agent_status": "blocked"},
            {"pane_id": "w1:p4"},
        ]}});
        assert_eq!(busy_panes(&v), ["w1:p1", "w1:p3"]);
    }

    #[test]
    fn busy_panes_tolerates_unexpected_shapes() {
        assert!(busy_panes(&json!({})).is_empty());
        assert!(busy_panes(&json!({"result": {"panes": []}})).is_empty());
    }
}
