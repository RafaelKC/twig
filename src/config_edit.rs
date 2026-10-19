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
    edit_file(&config_path()?, TEMPLATE, "config", |f| {
        Config::load_from(f).map(|_| ())
    })
}

/// Open `file` in the editor (creating it from `initial` if missing) until `validate` passes.
pub fn edit_file(
    file: &Path,
    initial: &str,
    what: &str,
    validate: impl Fn(&Path) -> Result<()>,
) -> Result<()> {
    if !file.exists() {
        if let Some(dir) = file.parent() {
            fs::create_dir_all(dir)?;
        }
        fs::write(file, initial)?;
        ui::ok(format!(
            "{what} created from the template: {}",
            file.display()
        ));
    }

    let editor = editor();
    loop {
        open_editor(&editor, file)?;
        match validate(file) {
            Ok(()) => {
                ui::ok(format!("{what} valid: {}", file.display()));
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
                    bail!("{what} left invalid: {}", file.display());
                }
            }
        }
    }
}
