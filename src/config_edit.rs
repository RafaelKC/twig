//! `twig config edit`: open the config in the editor and validate on save.

use std::fs;
use std::io::{self, IsTerminal};
use std::path::Path;
use std::process::Command;

use anyhow::{bail, Context, Result};

use crate::config::{config_path, Config};
use crate::{sys, ui};

const TEMPLATE: &str = r#"# twig config. Format documented in docs/CONFIG.md.
version = 1

[agent]
provider = "claude"               # "claude" | "codex"

[provider]
default = "az"                    # "az" | "gh"

[vscode]
enabled     = true
open_on_new = true

[herdr]
open_on_new = false

[paths]
worktrees = "~/.wt"
repos     = "~"

[git]
base_branch   = "main"
branch_prefix = { Bug = "fix", default = "feat" }

# One block per repo:
# [repos.my-back]
# label   = "Back"
# short   = "back"
# default = true
# run     = "dotnet watch run"
"#;

/// $VISUAL, else $EDITOR, else an OS default.
fn editor() -> String {
    ["VISUAL", "EDITOR"]
        .iter()
        .filter_map(|k| std::env::var(k).ok())
        .find(|v| !v.trim().is_empty())
        .unwrap_or_else(|| {
            if cfg!(windows) {
                "notepad".into()
            } else {
                "nano".into()
            }
        })
}

/// Run the editor through the shell, so values like `code --wait` work.
fn open_editor(editor: &str, file: &Path) -> Result<()> {
    let mut cmd = if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.arg("/C").arg(format!("{editor} \"{}\"", file.display()));
        c
    } else {
        let mut c = Command::new("sh");
        c.arg("-c")
            .arg(format!("{editor} \"$@\""))
            .arg("sh")
            .arg(file);
        c
    };
    sys::run(&mut cmd).with_context(|| format!("editor `{editor}` failed"))
}

pub fn edit() -> Result<()> {
    let file = config_path()?;
    if !file.exists() {
        if let Some(dir) = file.parent() {
            fs::create_dir_all(dir)?;
        }
        fs::write(&file, TEMPLATE)?;
        ui::ok(format!(
            "config created from the template: {}",
            file.display()
        ));
    }

    let editor = editor();
    loop {
        open_editor(&editor, &file)?;
        match Config::load_from(&file) {
            Ok(_) => {
                ui::ok(format!("config valid: {}", file.display()));
                return Ok(());
            }
            Err(e) => {
                eprintln!("\x1b[31m✖ {e:#}\x1b[0m");
                // without a terminal, an empty answer can't mean "yes" or the loop never ends
                let interactive = io::stdout().is_terminal();
                let again = match ui::ask("reopen to fix it? [Y/n]: ").as_str() {
                    "" => interactive,
                    a => matches!(a, "y" | "Y" | "s" | "S"),
                };
                if !again {
                    bail!("config left invalid: {}", file.display());
                }
            }
        }
    }
}
