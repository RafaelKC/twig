//! VS Code `.code-workspace` file.

use std::fs;

use anyhow::Result;
use serde_json::{json, Map, Value};

use crate::card;
use crate::config::Config;
use crate::ui;

pub fn write(cfg: &Config, id: &str, title: &str) -> Result<()> {
    let dir = cfg.card_dir(id);
    let color = card::color(id);
    let repos = card::repos_ordered(cfg, &dir);

    let mut folders = vec![];
    let mut tasks = vec![];
    let mut deps = vec![];
    let mut compound = vec![];

    for repo in &repos {
        let label = cfg.label(repo);
        folders.push(json!({ "path": repo, "name": label }));

        let Some(r) = cfg.repo(repo) else { continue };
        // the compound's folder is the same "name" as the folder above, so they stay in sync
        for dbg in &r.debug {
            compound.push(json!({ "folder": label, "name": dbg }));
        }
        if r.run.is_empty() {
            continue;
        }
        // absolute cwd: ${workspaceFolder:NAME} would depend on emoji/spaces in the label
        let task_label = format!("run: {repo}");
        tasks.push(json!({
            "label": task_label,
            "type": "shell",
            "command": r.run,
            "options": { "cwd": dir.join(repo) },
            "isBackground": true,
            "problemMatcher": []
        }));
        deps.push(task_label);
    }

    if !deps.is_empty() {
        tasks.push(json!({
            "label": "twig: start all",
            "dependsOn": deps,
            "dependsOrder": "parallel",
            "problemMatcher": []
        }));
    }

    let mut ws = Map::new();
    ws.insert("folders".into(), Value::Array(folders));
    ws.insert(
        "settings".into(),
        json!({
            "window.title": format!("#{id} · {title} ${{separator}} ${{activeEditorShort}}"),
            "workbench.colorCustomizations": {
                "titleBar.activeBackground": color,
                "titleBar.activeForeground": "#ffffff",
                "titleBar.inactiveBackground": format!("{color}cc"),
                "titleBar.inactiveForeground": "#ffffffaa",
                "activityBar.background": color,
                "activityBar.foreground": "#ffffff",
                "activityBar.inactiveForeground": "#ffffff99",
                "statusBar.background": color,
                "statusBar.foreground": "#ffffff",
                "statusBar.debuggingBackground": color,
                "statusBar.debuggingForeground": "#ffffff",
                "statusBar.debuggingBorder": color,
                "statusBarItem.hoverBackground": "#ffffff22",
                "statusBarItem.remoteBackground": color,
                "sash.hoverBorder": color
            },
            "search.exclude": {
                "**/node_modules": true,
                "**/bin": true,
                "**/obj": true,
                "**/dist": true,
                "**/.angular": true,
                "**/.nx": true
            },
            "files.watcherExclude": {
                "**/node_modules/**": true,
                "**/bin/**": true,
                "**/obj/**": true,
                "**/.angular/**": true,
                "**/.nx/**": true
            },
            "git.openRepositoryInParentFolders": "never",
            "terminal.integrated.env.linux": { "CARD_ID": id }
        }),
    );
    ws.insert(
        "tasks".into(),
        json!({ "version": "2.0.0", "tasks": tasks }),
    );

    // a compound only makes sense with 2+ processes; with fewer, the repo's dropdown is enough
    let n_dbg = compound.len();
    if n_dbg >= 2 {
        ws.insert(
            "launch".into(),
            json!({
                "version": "0.2.0",
                "configurations": [],
                "compounds": [{
                    "name": format!("▶ card #{id}: start and debug all"),
                    "configurations": compound,
                    "stopAll": true,
                    "presentation": { "group": "0-card", "order": 1 }
                }]
            }),
        );
    }

    fs::write(
        cfg.workspace_file(id),
        serde_json::to_string_pretty(&Value::Object(ws))? + "\n",
    )?;
    if n_dbg >= 2 {
        ui::ok(format!(
            "VS Code workspace generated (color {color}, compound with {n_dbg} processes)"
        ));
    } else {
        ui::ok(format!("VS Code workspace generated (color {color})"));
    }
    Ok(())
}

pub fn debug_count(cfg: &Config, repos: &[String]) -> usize {
    repos
        .iter()
        .filter_map(|r| cfg.repo(r))
        .map(|r| r.debug.len())
        .sum()
}
