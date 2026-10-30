//! The card's herdr workspace: `agent` tab (one pane per repo running the AI) and
//! `run` tab (one pane per repo with `run` and `autorun`). The workspace is found by label = id.

use std::path::Path;
use std::process::Stdio;
use std::thread;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use serde_json::Value;

use crate::config::Config;
use crate::layout::{self, Preset};
use crate::{card, ports, procenv, sessions, sys, ui};

fn herdr(args: &[&str]) -> Result<Value> {
    let out = sys::output(sys::tool("herdr").args(args))?;
    serde_json::from_str(&out)
        .with_context(|| format!("unexpected response from herdr: herdr {}", args.join(" ")))
}

fn herdr_quiet(args: &[&str]) {
    sys::quiet(sys::tool("herdr").args(args));
}

fn str_at(v: &Value, ptr: &str) -> Result<String> {
    v.pointer(ptr)
        .and_then(Value::as_str)
        .map(String::from)
        .with_context(|| format!("herdr response without {ptr}"))
}

fn server_up() -> bool {
    sys::quiet(sys::tool("herdr").args(["workspace", "list"]))
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
    let mut cmd = sys::tool("herdr");
    cmd.arg("server")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    // detached, so closing the terminal that started it doesn't stop the server
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
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

/// Whether this process runs inside the card's own herdr workspace (`HERDR_WORKSPACE_ID`).
pub fn inside_workspace(id: &str) -> bool {
    let Some(here) = std::env::var("HERDR_WORKSPACE_ID")
        .ok()
        .filter(|w| !w.is_empty())
    else {
        return false;
    };
    if sys::which("herdr").is_none() || !server_up() {
        return false;
    }
    same_workspace(&here, find_workspace(id).ok().flatten().as_deref())
}

/// Whether `here` (our `HERDR_WORKSPACE_ID`) is the card's workspace `found`.
pub fn same_workspace(here: &str, found: Option<&str>) -> bool {
    found == Some(here)
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
    let Ok(Some(ws)) = find_workspace(id) else {
        return Vec::new();
    };
    herdr(&["pane", "list", "--workspace", &ws])
        .map(|v| busy_panes(&v))
        .unwrap_or_default()
}

/// Whether the card has a herdr workspace and how many of its panes have a busy agent.
/// Never starts the server; `(false, 0)` when herdr is missing or not running.
pub fn card_presence(id: &str) -> (bool, usize) {
    if sys::which("herdr").is_none() || !server_up() {
        return (false, 0);
    }
    let Ok(Some(ws)) = find_workspace(id) else {
        return (false, 0);
    };
    let busy = herdr(&["pane", "list", "--workspace", &ws])
        .map(|v| busy_panes(&v).len())
        .unwrap_or(0);
    (true, busy)
}

/// Close the card's herdr workspace, if there is one. Failures are only warnings.
pub fn close_workspace(id: &str) {
    if sys::which("herdr").is_none() || !server_up() {
        return;
    }
    match find_workspace(id) {
        Ok(Some(ws)) => {
            if sys::quiet(sys::tool("herdr").args(["workspace", "close", &ws])) {
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

/// `--env K=V` arguments for a new pane of `repo` (twig's markers, the card's ports and the repo's `env`).
fn env_args(cfg: &Config, id: &str, repo: &str) -> Vec<String> {
    procenv::pane(cfg, id, Some(repo))
        .into_iter()
        .flat_map(|(k, v)| ["--env".to_string(), format!("{k}={v}")])
        .collect()
}

fn with_env<'a>(args: &[&'a str], env: &'a [String]) -> Vec<&'a str> {
    let mut all = args.to_vec();
    all.extend(env.iter().map(String::as_str));
    all
}

/// The panes a tab is meant to have: the repos in layout order, the preset and the card folder.
struct Slots<'a> {
    preset: Preset,
    repos: &'a [String],
    card_dir: &'a Path,
}

impl Slots<'_> {
    /// The pane of repo `r` among `panes` (found by cwd, like everywhere else).
    fn pane_of(&self, panes: &[(String, String, String)], r: &str) -> Option<String> {
        let wt = self.card_dir.join(r).to_string_lossy().into_owned();
        panes
            .iter()
            .find(|(_, c, f)| *c == wt || *f == wt)
            .map(|p| p.0.clone())
    }
}

/// Slots of the `agent` tab (every repo) or of the `run` tab (repos with a `run` command, and
/// with `autorun` when `autorun_only`), sorted by `[herdr.layout] order`.
fn tab_repos(cfg: &Config, repos: &[String], run_tab: bool, autorun_only: bool) -> Vec<String> {
    let mine: Vec<String> = repos
        .iter()
        .filter(|r| {
            !run_tab
                || cfg.repo(r).is_some_and(|c| {
                    !c.run.is_empty() && (!autorun_only || autorun(cfg, r, c.autorun))
                })
        })
        .cloned()
        .collect();
    cfg.herdr.layout.sorted(&mine)
}

/// Make sure a repo has a pane in a tab. Creates the tab if missing; an existing pane
/// is left alone unless `force` (root pane just created along with the workspace).
/// New panes get the card's ports and the repo's `env` as environment variables. A new pane
/// is placed by the tab's layout preset. Returns whether it was added to a tab that already
/// existed (the caller then rebalances it).
#[allow(clippy::too_many_arguments)]
fn ensure_panel(
    cfg: &Config,
    id: &str,
    ws: &str,
    tab_label: &str,
    repo: &str,
    cmd: &str,
    cwd: &Path,
    mut force: bool,
    slots: &Slots,
) -> Result<bool> {
    let mut added = false;
    let cwd_s = cwd.to_string_lossy().into_owned();
    let env = env_args(cfg, id, repo);
    let pane = match find_tab(ws, tab_label)? {
        None => {
            let v = herdr(&with_env(
                &[
                    "tab",
                    "create",
                    "--workspace",
                    ws,
                    "--cwd",
                    &cwd_s,
                    "--label",
                    tab_label,
                    "--no-focus",
                ],
                &env,
            ))?;
            force = true;
            str_at(&v, "/result/root_pane/pane_id")?
        }
        Some(tab) => {
            let panes = tab_panes(ws, &tab)?;
            // herdr doesn't expose pane labels; the repo's pane is found by cwd
            match panes.iter().find(|(_, c, f)| *c == cwd_s || *f == cwd_s) {
                Some((id, _, _)) => id.clone(),
                None => {
                    let planned = slots
                        .repos
                        .iter()
                        .position(|r| r == repo)
                        .and_then(|i| layout::step(slots.preset, slots.repos.len(), i))
                        .and_then(|st| {
                            let t = slots.repos.get(st.target)?;
                            Some((slots.pane_of(&panes, t)?, st))
                        });
                    let (target, dir, ratio) = match planned {
                        Some((p, st)) => (p, st.dir.as_str(), st.ratio.map(layout::ratio_arg)),
                        None => {
                            // not in the layout: side by side first, then stacked on the last pane
                            let last = panes
                                .last()
                                .map(|p| p.0.clone())
                                .context("herdr tab without panes")?;
                            (last, if panes.len() <= 1 { "right" } else { "down" }, None)
                        }
                    };
                    let mut args = vec![
                        "pane",
                        "split",
                        &target,
                        "--direction",
                        dir,
                        "--cwd",
                        &cwd_s,
                        "--no-focus",
                    ];
                    if let Some(r) = &ratio {
                        args.extend(["--ratio", r]);
                    }
                    let v = herdr(&with_env(&args, &env))?;
                    added = true;
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
    Ok(added)
}

/// Quote for the pane's shell: POSIX sh, or PowerShell on Windows (`'` doubled inside).
fn shell_quote(s: &str) -> String {
    if cfg!(windows) {
        format!("'{}'", s.replace('\'', "''"))
    } else {
        format!("'{}'", s.replace('\'', r"'\''"))
    }
}

/// The agent command for a new pane: with a prompt, `<command> '<prompt>'` (Claude Code and
/// Codex both take the first message as an argument). `{id}` and `{repo}` are replaced.
fn agent_command(cfg: &Config, id: &str, repo: &str, prompt: Option<&str>) -> String {
    let cmd = cfg.ai.command();
    match prompt.map(str::trim).filter(|p| !p.is_empty()) {
        Some(p) => format!(
            "{cmd} {}",
            shell_quote(&p.replace("{id}", id).replace("{repo}", repo))
        ),
        None => cmd.to_string(),
    }
}

/// Command for an agent pane being created: a first prompt wins; otherwise, with
/// `[agent] resume_sessions`, resume the last conversation of `wt` if there is one.
fn pane_command(cfg: &Config, id: &str, repo: &str, wt: &Path, prompt: Option<&str>) -> String {
    let has_prompt = prompt.is_some_and(|p| !p.trim().is_empty());
    if !has_prompt && cfg.ai.resume_sessions && sessions::has_session(cfg, wt) {
        return format!("{} {}", cfg.ai.command(), cfg.ai.provider.resume_args());
    }
    agent_command(cfg, id, repo, prompt)
}

/// Create in herdr whatever the card is missing. Idempotent: existing things are left alone,
/// and panes closed by the user come back. `prompt` is the first message of the agents started
/// now (only `twig new` passes one).
pub fn sync(cfg: &Config, id: &str, prompt: Option<&str>) -> Result<String> {
    ensure_server()?;
    let dir = card::require_card(cfg, id)?;
    let repos = cfg.herdr.layout.sorted(&card::repos_ordered(cfg, &dir));
    let first = repos
        .first()
        .with_context(|| format!("card {id} has no worktrees"))?;

    let (ws, fresh) = match find_workspace(id)? {
        Some(ws) => (ws, false),
        None => {
            let cwd = dir.join(first).to_string_lossy().into_owned();
            let env = env_args(cfg, id, first);
            let v = herdr(&with_env(
                &[
                    "workspace",
                    "create",
                    "--cwd",
                    &cwd,
                    "--label",
                    id,
                    "--no-focus",
                ],
                &env,
            ))?;
            let ws = str_at(&v, "/result/workspace/workspace_id")?;
            herdr_quiet(&["tab", "rename", &str_at(&v, "/result/tab/tab_id")?, "agent"]);
            ui::ok(format!("herdr workspace #{id} created"));
            (ws, true)
        }
    };

    let agents = Slots {
        preset: cfg.herdr.layout.preset("agent"),
        repos: &repos,
        card_dir: &dir,
    };
    let mut grew_agent = false;
    for (i, repo) in repos.iter().enumerate() {
        grew_agent |= ensure_panel(
            cfg,
            id,
            &ws,
            "agent",
            repo,
            &pane_command(cfg, id, repo, &dir.join(repo), prompt),
            &dir.join(repo),
            fresh && i == 0,
            &agents,
        )?;
    }
    let run_repos = tab_repos(cfg, &repos, true, true);
    let runs = Slots {
        preset: cfg.herdr.layout.preset("run"),
        repos: &run_repos,
        card_dir: &dir,
    };
    let mut grew_run = false;
    for repo in &run_repos {
        let Some(r) = cfg.repo(repo) else { continue };
        grew_run |= ensure_panel(
            cfg,
            id,
            &ws,
            "run",
            repo,
            &r.run,
            &dir.join(repo),
            false,
            &runs,
        )?;
    }
    for (grew, tab, slots) in [(grew_agent, "agent", &agents), (grew_run, "run", &runs)] {
        if grew {
            if let Err(e) = rebalance(&ws, tab, slots) {
                ui::warn(format!("herdr: could not rebalance the {tab} tab: {e:#}"));
            }
        }
    }
    Ok(ws)
}

/// What `twig run` does to the apps of the `run` tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum RunAction {
    Start,
    Stop,
    Restart,
}

/// Repos `twig run` acts on: the card's repos that have a `run` command (with or without
/// `autorun`), or exactly `requested`, which must belong to the card and have a `run`.
pub fn run_targets(
    cfg: &Config,
    card_repos: &[String],
    requested: &[String],
) -> Result<Vec<String>> {
    let has_run = |r: &str| cfg.repo(r).is_some_and(|c| !c.run.is_empty());
    if requested.is_empty() {
        let all: Vec<String> = card_repos.iter().filter(|r| has_run(r)).cloned().collect();
        if all.is_empty() {
            bail!("no repo of the card has a `run` command");
        }
        return Ok(all);
    }
    for r in requested {
        if !card_repos.contains(r) {
            bail!("repo {r} is not in the card");
        }
        if !has_run(r) {
            bail!("repo {r} has no `run` command");
        }
    }
    Ok(card_repos
        .iter()
        .filter(|r| requested.contains(r))
        .cloned()
        .collect())
}

/// Time for the app to exit and the shell to show its prompt again after Ctrl+C.
const STOP_GRACE: Duration = Duration::from_millis(1500);

/// `twig run`: stop, start or restart the apps of the card's `run` tab.
pub fn run_apps(cfg: &Config, id: &str, action: RunAction, requested: &[String]) -> Result<()> {
    let dir = card::require_card(cfg, id)?;
    let targets = run_targets(cfg, &card::repos_ordered(cfg, &dir), requested)?;
    if sys::which("herdr").is_none() {
        bail!("herdr not found on PATH");
    }
    if !server_up() {
        bail!("the herdr server is not running (`twig herdr {id}` starts it)");
    }
    let Some(ws) = find_workspace(id)? else {
        bail!("card {id} has no herdr workspace (`twig herdr {id}` creates it)");
    };
    if matches!(action, RunAction::Stop | RunAction::Restart) {
        let mut sent = false;
        for repo in &targets {
            match run_pane(&ws, &dir.join(repo))? {
                Some(pane) => {
                    if sys::quiet(sys::tool("herdr").args(["pane", "send-keys", &pane, "C-c"])) {
                        sent = true;
                        if action == RunAction::Stop {
                            ui::ok(format!("[{}] stopped", cfg.short(repo)));
                        }
                    } else if action == RunAction::Stop {
                        bail!("[{}] could not send Ctrl+C to the pane", cfg.short(repo));
                    }
                }
                None if action == RunAction::Stop => {
                    ui::warn(format!("[{}] no pane in the run tab", cfg.short(repo)))
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
        let run_repos = tab_repos(cfg, &card::repos_ordered(cfg, &dir), true, false);
        let slots = Slots {
            preset: cfg.herdr.layout.preset("run"),
            repos: &run_repos,
            card_dir: &dir,
        };
        let mut grew = false;
        for repo in &targets {
            let wt = dir.join(repo);
            let cmd = &cfg.repo(repo).context("repo without config")?.run;
            let existing = run_pane(&ws, &wt)?;
            if let Some(pane) = &existing {
                // `start` on a running pane: interrupt first so the process isn't duplicated
                if action == RunAction::Start {
                    herdr_quiet(&["pane", "send-keys", pane, "C-c"]);
                    thread::sleep(STOP_GRACE);
                }
                // the pane's shell keeps the environment it was created with: re-export it
                let line = env_command(&procenv::pane(cfg, id, Some(repo)), cmd);
                if !sys::quiet(sys::tool("herdr").args(["pane", "run", pane, &line])) {
                    bail!(
                        "[{}] could not run the command in the pane",
                        cfg.short(repo)
                    );
                }
            } else {
                grew |= ensure_panel(cfg, id, &ws, "run", repo, cmd, &wt, false, &slots)?;
            }
            ui::ok(format!("[{}] running", cfg.short(repo)));
        }
        if grew {
            if let Err(e) = rebalance(&ws, "run", &slots) {
                ui::warn(format!("herdr: could not rebalance the run tab: {e:#}"));
            }
        }
    }
    Ok(())
}

/// Label of the tab panes wait in while a tab is rebuilt.
const TMP_TAB: &str = "twig-layout";

/// After panes were added to an existing tab: put the whole tab in its preset's shape (equal
/// shares). `main-left` keeps herdr's plain splits, as before presets existed.
fn rebalance(ws: &str, tab_label: &str, slots: &Slots) -> Result<()> {
    if slots.preset == Preset::MainLeft || slots.repos.len() < 3 {
        return Ok(());
    }
    arrange(ws, tab_label, slots)
}

/// Rebuild the tab's repo panes in the preset's shape without closing them: all but the first
/// leave through a temporary tab (herdr can't move a pane within its tab) and come back one by
/// one with the preset's splits, so processes and terminals are kept.
fn arrange(ws: &str, tab_label: &str, slots: &Slots) -> Result<()> {
    let tab = find_tab(ws, tab_label)?.with_context(|| format!("no {tab_label} tab"))?;
    let all = tab_panes(ws, &tab)?;
    let panes: Vec<String> = slots
        .repos
        .iter()
        .filter_map(|r| slots.pane_of(&all, r))
        .collect();
    if panes.len() != slots.repos.len() {
        bail!("some repos have no pane in the {tab_label} tab");
    }
    if panes.len() < 2 {
        return Ok(());
    }
    let mut moved = 0;
    let result = (|| -> Result<()> {
        herdr(&[
            "pane",
            "move",
            &panes[1],
            "--new-tab",
            "--label",
            TMP_TAB,
            "--no-focus",
        ])?;
        moved = 1;
        let tmp = find_tab(ws, TMP_TAB)?.context("herdr did not create the temporary tab")?;
        for p in &panes[2..] {
            herdr(&[
                "pane",
                "move",
                p,
                "--tab",
                &tmp,
                "--target-pane",
                &panes[1],
                "--split",
                "right",
                "--no-focus",
            ])?;
            moved += 1;
        }
        for i in 1..panes.len() {
            let st = layout::step(slots.preset, panes.len(), i).context("no layout step")?;
            let ratio = st.ratio.map(layout::ratio_arg);
            let mut args = vec![
                "pane",
                "move",
                &panes[i],
                "--tab",
                &tab,
                "--target-pane",
                &panes[st.target],
                "--split",
                st.dir.as_str(),
                "--no-focus",
            ];
            if let Some(r) = &ratio {
                args.extend(["--ratio", r]);
            }
            herdr(&args)?;
            moved -= 1;
        }
        Ok(())
    })();
    if result.is_err() && moved > 0 {
        // put the panes still waiting back, so none is left behind in the temporary tab
        for p in &panes[panes.len() - moved..] {
            herdr_quiet(&[
                "pane",
                "move",
                p,
                "--tab",
                &tab,
                "--target-pane",
                &panes[0],
                "--split",
                "right",
                "--no-focus",
            ]);
        }
    }
    // an emptied tab may already be gone
    if let Ok(Some(tmp)) = find_tab(ws, TMP_TAB) {
        herdr_quiet(&["tab", "close", &tmp]);
    }
    result
}

/// `twig layout`: rearrange a tab of the card's workspace in `preset`, keeping its panes and the
/// processes in them, and remember the choice in the card's `.twig.toml` so `twig add-repo` and
/// `twig herdr` follow it.
pub fn set_layout(cfg: &Config, id: &str, tab_label: &str, preset: Preset) -> Result<()> {
    let dir = card::require_card(cfg, id)?;
    if sys::which("herdr").is_none() {
        bail!("herdr not found on PATH");
    }
    if !server_up() {
        bail!("the herdr server is not running (`twig herdr {id}` starts it)");
    }
    let Some(ws) = find_workspace(id)? else {
        bail!("card {id} has no herdr workspace (`twig herdr {id}` creates it)");
    };
    let tab =
        find_tab(&ws, tab_label)?.with_context(|| format!("card {id} has no {tab_label} tab"))?;
    let panes = tab_panes(&ws, &tab)?;
    let all = card::repos_ordered(cfg, &dir);
    let sorted = tab_repos(cfg, &all, tab_label == "run", false);
    let mine: Vec<String> = sorted
        .into_iter()
        .filter(|r| {
            let wt = dir.join(r).to_string_lossy().into_owned();
            panes.iter().any(|(_, c, f)| *c == wt || *f == wt)
        })
        .collect();
    let slots = Slots {
        preset,
        repos: &mine,
        card_dir: &dir,
    };
    arrange(&ws, tab_label, &slots)?;
    layout::save_card_file(&dir.join(crate::overrides::FILE), tab_label, preset)?;
    ui::ok(format!("herdr: {tab_label} tab is now {}", preset.name()));
    Ok(())
}

/// `cmd` prefixed with the assignments of `vars`, for a shell that already exists and so
/// can't get them through `--env`.
fn env_command(vars: &[(String, String)], cmd: &str) -> String {
    let mut line = String::new();
    for (k, v) in vars {
        if cfg!(windows) {
            line.push_str(&format!("$env:{k}={}; ", shell_quote(v)));
        } else {
            line.push_str(&format!("export {k}={}; ", shell_quote(v)));
        }
    }
    line + cmd
}

/// The pane of the `run` tab that belongs to the repo worktree `wt`, if any.
fn run_pane(ws: &str, wt: &Path) -> Result<Option<String>> {
    let wt = wt.to_string_lossy();
    let Some(tab) = find_tab(ws, "run")? else {
        return Ok(None);
    };
    Ok(tab_panes(ws, &tab)?
        .into_iter()
        .find(|(_, c, f)| *c == wt || *f == wt)
        .map(|p| p.0))
}

/// Whether `repo` starts in the `run` tab: the card's template wins over the repo's own value.
fn autorun(cfg: &Config, repo: &str, repo_value: bool) -> bool {
    cfg.template
        .as_ref()
        .and_then(|t| t.autorun.as_ref())
        .and_then(|a| a.for_repo(repo))
        .unwrap_or(repo_value)
}

/// Focus the workspace and, when outside herdr, open herdr in the terminal.
pub fn open(ws: &str) -> Result<()> {
    herdr_quiet(&["workspace", "focus", ws]);
    if std::env::var("HERDR_ENV").as_deref() != Ok("1") {
        sys::run(&mut sys::tool("herdr"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn env_command_prefixes_assignments() {
        let vars = vec![
            ("PORT".to_string(), "3001".to_string()),
            ("MSG".to_string(), "it's".to_string()),
        ];
        let line = super::env_command(&vars, "npm start");
        if cfg!(windows) {
            assert_eq!(line, "$env:PORT='3001'; $env:MSG='it''s'; npm start");
        } else {
            assert_eq!(line, r"export PORT='3001'; export MSG='it'\''s'; npm start");
        }
        assert_eq!(super::env_command(&[], "x"), "x");
    }

    use super::*;

    #[test]
    fn same_workspace_only_matches_the_cards_own() {
        assert!(same_workspace("w1", Some("w1")));
        assert!(!same_workspace("w1", Some("w2")));
        assert!(!same_workspace("w1", None));
    }
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
    fn prompt_is_quoted_and_expanded() {
        if cfg!(windows) {
            assert_eq!(shell_quote("it's"), "'it''s'");
        } else {
            assert_eq!(shell_quote("it's"), r"'it'\''s'");
        }
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("config.toml");
        std::fs::write(&file, "version = 1\n").unwrap();
        let cfg = Config::load_from(&file).unwrap();
        assert_eq!(
            agent_command(&cfg, "52644", "api", Some("/deliver {id} in {repo}")),
            "claude '/deliver 52644 in api'"
        );
        assert_eq!(agent_command(&cfg, "1", "api", Some("  ")), "claude");
        assert_eq!(agent_command(&cfg, "1", "api", None), "claude");
    }

    #[test]
    fn resume_replaces_the_bare_command_only_when_enabled() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("config.toml");
        std::fs::write(&file, "version = 1\n[agent]\nprovider = \"codex\"\n").unwrap();
        let wt = tmp.path().join("wt");
        let mut cfg = Config::load_from(&file).unwrap();
        assert_eq!(pane_command(&cfg, "1", "api", &wt, None), "codex");
        cfg.ai.resume_sessions = true;
        assert_eq!(
            pane_command(&cfg, "1", "api", &wt, None),
            "codex resume --last"
        );
        assert_eq!(
            pane_command(&cfg, "1", "api", &wt, Some("hi")),
            "codex 'hi'"
        );
    }

    #[test]
    fn template_autorun_overrides_the_repo_value() {
        use crate::templates::{Autorun, Template};
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("config.toml");
        std::fs::write(&file, "version = 1\n").unwrap();
        let mut cfg = Config::load_from(&file).unwrap();
        assert!(autorun(&cfg, "api", true));
        assert!(!autorun(&cfg, "api", false));
        cfg.template = Some(Template {
            autorun: Some(Autorun::All(false)),
            ..Default::default()
        });
        assert!(!autorun(&cfg, "api", true));
        cfg.template = Some(Template {
            autorun: Some(Autorun::PerRepo([("api".to_string(), true)].into())),
            ..Default::default()
        });
        assert!(autorun(&cfg, "api", false));
        assert!(!autorun(&cfg, "ui", false));
        assert!(autorun(&cfg, "ui", true));
    }

    #[test]
    fn run_targets_picks_repos_with_a_run_command() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("config.toml");
        std::fs::write(
            &file,
            "version = 1\n[repos.api]\nrun = \"cargo run\"\nautorun = false\n[repos.web]\nrun = \"npm start\"\n[repos.lib]\n",
        )
        .unwrap();
        let cfg = Config::load_from(&file).unwrap();
        let card: Vec<String> = ["api", "web", "lib"].map(String::from).into();
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        // autorun doesn't matter
        assert_eq!(run_targets(&cfg, &card, &[]).unwrap(), ["api", "web"]);
        assert_eq!(run_targets(&cfg, &card, &s(&["web"])).unwrap(), ["web"]);
        assert_eq!(
            run_targets(&cfg, &card, &s(&["web", "api"])).unwrap(),
            ["api", "web"]
        );
        let e = run_targets(&cfg, &card, &s(&["lib"])).unwrap_err();
        assert!(e.to_string().contains("no `run` command"));
        let e = run_targets(&cfg, &card, &s(&["other"])).unwrap_err();
        assert!(e.to_string().contains("not in the card"));
        assert!(run_targets(&cfg, &card[2..], &[]).is_err());
    }

    #[test]
    fn busy_panes_tolerates_unexpected_shapes() {
        assert!(busy_panes(&json!({})).is_empty());
        assert!(busy_panes(&json!({"result": {"panes": []}})).is_empty());
    }
}
