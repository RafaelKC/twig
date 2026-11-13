//! The `app` backend (`[herdr] backend = "app"`): the card's workspace lives in the herdr app
//! (`herdr-app`, core `hc`) instead of the herdr CLI. A card is a *space* named after its id,
//! with the same `agent` and `run` tabs. Spaces, tabs and terminals come from `herdr-app ls`,
//! shell cwds and agent status from `hc ls`, and commands are typed into terminals with `hc send`.
//!
//! Differences from the CLI backend: the app keeps no pane labels, so panes are found by cwd;
//! layout presets and park snapshots are not supported (new panes split right, then down).

use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use std::thread;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use serde_json::{json, Value};

use crate::config::Config;
use crate::herdr::{env_command, pane_command, tab_repos, RunAction, STOP_GRACE};
use crate::{card, ports, procenv, sys, ui};

/// `herdr-app do <command> '<json>'`. The JSON form is always used: a numeric space name
/// (`52645`) fails as `--name`.
fn app_do(cmd: &str, args: Value) -> Result<Value> {
    let out = sys::output(sys::tool("herdr-app").args(["do", cmd, &args.to_string()]))?;
    Ok(serde_json::from_str(&out).unwrap_or(Value::Null))
}

fn app_do_quiet(cmd: &str, args: Value) {
    sys::quiet(sys::tool("herdr-app").args(["do", cmd, &args.to_string()]));
}

fn ls() -> Result<Value> {
    let out = sys::output(sys::tool("herdr-app").arg("ls"))?;
    serde_json::from_str(&out).context("unexpected response from `herdr-app ls`")
}

/// Whether the app window is open and answering.
pub fn server_up() -> bool {
    sys::quiet(sys::tool("herdr-app").arg("ls"))
}

/// Make sure the app is running (opens the window when it isn't).
pub fn ensure_server() -> Result<()> {
    if sys::which("herdr-app").is_none() {
        bail!("herdr-app not found on PATH (needed by `[herdr] backend = \"app\"`)");
    }
    if server_up() {
        return Ok(());
    }
    ui::info("opening herdr-app...");
    spawn_window()?;
    for _ in 0..40 {
        thread::sleep(Duration::from_millis(250));
        if server_up() {
            return Ok(());
        }
    }
    bail!("herdr-app did not respond")
}

fn spawn_window() -> Result<()> {
    let mut cmd = sys::tool("herdr-app");
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    cmd.spawn().context("could not start herdr-app")?;
    Ok(())
}

/// The card's space (named after its id), as in `herdr-app ls`.
pub fn find_space(id: &str) -> Result<Option<Value>> {
    Ok(ls()?["spaces"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|s| s["name"] == id)
        .cloned())
}

pub fn find_workspace(id: &str) -> Result<Option<String>> {
    Ok(find_space(id)?.and_then(|s| s["id"].as_str().map(String::from)))
}

/// `(terminal, pane, cwd)` of a tab's terminals, in order.
fn tab_terminals(tab: &Value, cwds: &HashMap<String, String>) -> Vec<(String, String, String)> {
    tab["terminals"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|t| {
            let term = t["terminal"].as_str()?.to_string();
            let cwd = cwds.get(&term).cloned().unwrap_or_default();
            Some((term, t["pane"].as_str()?.to_string(), cwd))
        })
        .collect()
}

/// Terminal address -> cwd, from `hc ls`.
fn terminal_cwds() -> HashMap<String, String> {
    let Ok(out) = sys::output(sys::tool("hc").arg("ls")) else {
        return HashMap::new();
    };
    let v: Value = serde_json::from_str(&out).unwrap_or(Value::Null);
    v["terminals"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|t| {
            Some((
                t["address"].as_str()?.to_string(),
                t["cwd"].as_str()?.to_string(),
            ))
        })
        .collect()
}

fn find_tab<'a>(space: &'a Value, label: &str) -> Option<&'a Value> {
    space["tabs"]
        .as_array()?
        .iter()
        .find(|t| t["name"] == label)
}

fn busy_terminals(space: &Value) -> Vec<String> {
    space["tabs"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|t| t["terminals"].as_array().into_iter().flatten())
        .filter(|t| matches!(t["status"].as_str(), Some("working" | "blocked")))
        .filter_map(|t| t["terminal"].as_str().map(String::from))
        .collect()
}

/// Terminals with a busy agent in the card's space; empty when the app isn't open.
pub fn running_agents(id: &str) -> Vec<String> {
    if sys::which("herdr-app").is_none() {
        return Vec::new();
    }
    match find_space(id) {
        Ok(Some(s)) => busy_terminals(&s),
        _ => Vec::new(),
    }
}

pub fn card_presence(id: &str) -> (bool, usize) {
    if sys::which("herdr-app").is_none() {
        return (false, 0);
    }
    match find_space(id) {
        Ok(Some(s)) => (true, busy_terminals(&s).len()),
        _ => (false, 0),
    }
}

/// Whether this process runs in a terminal of the card's space (`HC_TERMINAL_ID`).
pub fn inside_workspace(id: &str) -> bool {
    let Some(me) = std::env::var("HC_TERMINAL_ID")
        .ok()
        .filter(|v| !v.is_empty())
    else {
        return false;
    };
    let Ok(Some(space)) = find_space(id) else {
        return false;
    };
    space["tabs"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|t| t["terminals"].as_array().into_iter().flatten())
        .any(|t| t["terminal"] == me.as_str())
}

pub fn close_workspace(id: &str) {
    if sys::which("herdr-app").is_none() {
        return;
    }
    match find_workspace(id) {
        Ok(Some(sp)) => {
            if sys::quiet(sys::tool("herdr-app").args([
                "do",
                "space.close",
                &json!({"id": sp, "confirmed": true}).to_string(),
            ])) {
                ui::ok(format!("herdr-app space #{id} closed"));
            } else {
                ui::warn(format!("could not close the herdr-app space #{id}"));
            }
        }
        Ok(None) => {}
        Err(_) => {}
    }
}

/// Type a line into a terminal (`hc send`: pastes it and presses enter).
fn send(terminal: &str, line: &str) -> bool {
    sys::quiet(sys::tool("hc").args(["send", terminal, line]))
}

/// Make sure a repo has a terminal in a tab: creates the tab if missing, splits a new terminal
/// when the repo has none. The command only runs in a terminal created here (or when `force`).
#[allow(clippy::too_many_arguments)]
fn ensure_panel(
    cfg: &Config,
    id: &str,
    space_id: &str,
    tab_label: &str,
    repo: &str,
    cmd: &str,
    cwd: &Path,
    force: bool,
) -> Result<()> {
    let cwd_s = cwd.to_string_lossy().into_owned();
    let space = ls()?["spaces"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|s| s["id"] == space_id)
        .cloned()
        .context("herdr-app space disappeared")?;
    let cwds = terminal_cwds();
    let mut run_now = force;
    let terminal = match find_tab(&space, tab_label) {
        None => {
            run_now = true;
            let v = app_do(
                "tab.new",
                json!({"name": tab_label, "space": space_id, "folder": cwd_s}),
            )?;
            v["terminal"]
                .as_str()
                .context("tab.new without a terminal")?
                .to_string()
        }
        Some(tab) => {
            let terms = tab_terminals(tab, &cwds);
            match terms.iter().find(|(_, _, c)| *c == cwd_s) {
                Some((t, _, _)) => t.clone(),
                None => {
                    run_now = true;
                    let (_, last_pane, _) = terms.last().context("herdr-app tab without panes")?;
                    let place = if terms.len() <= 1 { "right" } else { "down" };
                    // the split starts from the focused pane: focus the tab and the pane first
                    app_do("tab.select", json!({"id": tab["id"]}))?;
                    app_do("pane.focus", json!({"pane": last_pane}))?;
                    let v = app_do(
                        "terminal.open",
                        json!({"place": place, "folder": cwd_s, "name": cfg.short(repo)}),
                    )?;
                    v["terminal"]
                        .as_str()
                        .context("terminal.open without a terminal")?
                        .to_string()
                }
            }
        }
    };
    if run_now && !cmd.is_empty() {
        let line = env_command(&procenv::pane(cfg, id, Some(repo)), cmd);
        if send(&terminal, &line) {
            ui::ok(format!("herdr-app: {tab_label}/{}", cfg.short(repo)));
        } else {
            ui::warn(format!(
                "herdr-app: could not run the command in {tab_label}/{}",
                cfg.short(repo)
            ));
        }
    }
    Ok(())
}

/// Create in the app whatever the card is missing. Idempotent, like the CLI backend's `sync`.
pub fn sync(cfg: &Config, id: &str, prompt: Option<&str>) -> Result<String> {
    ensure_server()?;
    let dir = card::require_card(cfg, id)?;
    let repos = cfg.herdr.layout.sorted(&card::repos_ordered(cfg, &dir));
    let first = repos
        .first()
        .with_context(|| format!("card {id} has no worktrees"))?;
    let (space_id, fresh) = match find_workspace(id)? {
        Some(s) => (s, false),
        None => {
            let v = app_do(
                "space.new",
                json!({"name": id, "folder": dir.join(first).to_string_lossy(), "tab": "agent"}),
            )?;
            ui::ok(format!("herdr-app space #{id} created"));
            (
                v["space"]
                    .as_str()
                    .context("space.new without a space")?
                    .to_string(),
                true,
            )
        }
    };
    for (i, repo) in repos.iter().enumerate() {
        ensure_panel(
            cfg,
            id,
            &space_id,
            "agent",
            repo,
            &pane_command(cfg, id, repo, &dir.join(repo), prompt),
            &dir.join(repo),
            fresh && i == 0,
        )?;
    }
    for repo in tab_repos(cfg, &repos, true, true) {
        let Some(r) = cfg.repo(&repo) else { continue };
        ensure_panel(
            cfg,
            id,
            &space_id,
            "run",
            &repo,
            &r.run,
            &dir.join(&repo),
            false,
        )?;
    }
    Ok(space_id)
}

/// Show the space; when outside the app, open its window.
pub fn open(space_id: &str) -> Result<()> {
    app_do_quiet("space.select", json!({"id": space_id}));
    if std::env::var("HC_TERMINAL_ID").map_or(true, |v| v.is_empty()) {
        spawn_window()?;
    }
    Ok(())
}

/// `twig run` on the app backend: Ctrl+C and the repo's `run` command, typed into the run tab.
pub fn run_apps(cfg: &Config, id: &str, action: RunAction, targets: &[String]) -> Result<()> {
    let dir = card::require_card(cfg, id)?;
    if sys::which("herdr-app").is_none() {
        bail!("herdr-app not found on PATH");
    }
    if !server_up() {
        bail!("herdr-app is not open (`twig herdr {id}` opens it)");
    }
    let Some(space_id) = find_workspace(id)? else {
        bail!("card {id} has no herdr-app space (`twig herdr {id}` creates it)");
    };
    let run_terminal = |wt: &Path| -> Result<Option<String>> {
        let Some(space) = find_space(id)? else {
            return Ok(None);
        };
        let Some(tab) = find_tab(&space, "run") else {
            return Ok(None);
        };
        let wt = wt.to_string_lossy();
        Ok(tab_terminals(tab, &terminal_cwds())
            .into_iter()
            .find(|(_, _, c)| *c == wt)
            .map(|t| t.0))
    };
    if matches!(action, RunAction::Stop | RunAction::Restart) {
        let mut sent = false;
        for repo in targets {
            match run_terminal(&dir.join(repo))? {
                Some(t) => {
                    if sys::quiet(sys::tool("hc").args(["keys", &t, "ctrl+c"])) {
                        sent = true;
                        if action == RunAction::Stop {
                            ui::ok(format!("[{}] stopped", cfg.short(repo)));
                        }
                    } else if action == RunAction::Stop {
                        bail!(
                            "[{}] could not send Ctrl+C to the terminal",
                            cfg.short(repo)
                        );
                    }
                }
                None if action == RunAction::Stop => {
                    ui::warn(format!("[{}] no terminal in the run tab", cfg.short(repo)))
                }
                None => {}
            }
        }
        if sent && action == RunAction::Restart {
            thread::sleep(STOP_GRACE);
        }
    }
    if matches!(action, RunAction::Start | RunAction::Restart) {
        ports::write(cfg, id)?;
        for repo in targets {
            let wt = dir.join(repo);
            let cmd = &cfg.repo(repo).context("repo without config")?.run;
            match run_terminal(&wt)? {
                Some(t) => {
                    if action == RunAction::Start {
                        sys::quiet(sys::tool("hc").args(["keys", &t, "ctrl+c"]));
                        thread::sleep(STOP_GRACE);
                    }
                    let line = env_command(&procenv::pane(cfg, id, Some(repo)), cmd);
                    if !send(&t, &line) {
                        bail!(
                            "[{}] could not run the command in the terminal",
                            cfg.short(repo)
                        );
                    }
                }
                None => ensure_panel(cfg, id, &space_id, "run", repo, cmd, &wt, false)?,
            }
            ui::ok(format!("[{}] running", cfg.short(repo)));
        }
    }
    Ok(())
}

/// The card's tabs as `(label, pane cwds)`, for `twig migrate-herdr`. `None` without a space.
pub fn describe(id: &str) -> Result<Option<crate::migrate::Tabs>> {
    if sys::which("herdr-app").is_none() || !server_up() {
        return Ok(None);
    }
    let Some(space) = find_space(id)? else {
        return Ok(None);
    };
    let cwds = terminal_cwds();
    Ok(Some(
        space["tabs"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|t| {
                let panes = tab_terminals(t, &cwds).into_iter().map(|p| p.2).collect();
                (t["name"].as_str().unwrap_or_default().to_string(), panes)
            })
            .collect(),
    ))
}

/// Build the card's space from `tabs` (the first pane of a tab is created with it, the others
/// split right, then down). Fails if the space exists: callers check first.
pub fn create(cfg: &Config, id: &str, tabs: &[crate::migrate::NewTab]) -> Result<()> {
    ensure_server()?;
    let first = tabs
        .first()
        .and_then(|t| t.panes.first())
        .context("nothing to create")?;
    let v = app_do(
        "space.new",
        json!({"name": id, "folder": first.cwd, "tab": tabs[0].label}),
    )?;
    let space_id = v["space"]
        .as_str()
        .context("space.new without a space")?
        .to_string();
    let mut tab_id = v["tab"]
        .as_str()
        .context("space.new without a tab")?
        .to_string();
    let mut terminal = v["terminal"]
        .as_str()
        .context("space.new without a terminal")?
        .to_string();
    for (ti, tab) in tabs.iter().enumerate() {
        if ti > 0 {
            let v = app_do(
                "tab.new",
                json!({"name": tab.label, "space": space_id, "folder": tab.panes[0].cwd}),
            )?;
            tab_id = v["tab"]
                .as_str()
                .context("tab.new without a tab")?
                .to_string();
            terminal = v["terminal"]
                .as_str()
                .context("tab.new without a terminal")?
                .to_string();
        }
        for (pi, pane) in tab.panes.iter().enumerate() {
            if pi > 0 {
                let space = ls()?["spaces"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .find(|s| s["id"] == space_id.as_str())
                    .cloned()
                    .context("herdr-app space disappeared")?;
                let last_pane = find_tab_by_id(&space, &tab_id)
                    .and_then(|t| {
                        t["terminals"].as_array()?.last()?["pane"]
                            .as_str()
                            .map(String::from)
                    })
                    .context("herdr-app tab without panes")?;
                let mut args =
                    json!({"place": if pi == 1 { "right" } else { "down" }, "folder": pane.cwd});
                if let Some(r) = &pane.repo {
                    args["name"] = json!(cfg.short(r));
                }
                app_do("tab.select", json!({"id": tab_id}))?;
                app_do("pane.focus", json!({"pane": last_pane}))?;
                let v = app_do("terminal.open", args)?;
                terminal = v["terminal"]
                    .as_str()
                    .context("terminal.open without a terminal")?
                    .to_string();
            }
            if !pane.cmd.is_empty() {
                let vars = pane
                    .repo
                    .as_deref()
                    .map(|r| procenv::pane(cfg, id, Some(r)))
                    .unwrap_or_default();
                send(&terminal, &env_command(&vars, &pane.cmd));
            }
        }
    }
    Ok(())
}

fn find_tab_by_id<'a>(space: &'a Value, tab_id: &str) -> Option<&'a Value> {
    space["tabs"].as_array()?.iter().find(|t| t["id"] == tab_id)
}
