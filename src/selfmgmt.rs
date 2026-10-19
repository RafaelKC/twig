//! `twig update` and `twig uninstall`: managing twig's own installation.
//! Paths here mirror what install.sh writes.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};

use crate::config::{config_path, home};
use crate::{sys, ui};

const DEFAULT_REPO: &str = "RafaelKC/twig";
const HERDR_BLOCK_BEGIN: &str = "# twig:begin";
const HERDR_BLOCK_END: &str = "# twig:end";

fn repo() -> String {
    std::env::var("TWIG_REPO")
        .ok()
        .filter(|r| !r.is_empty())
        .unwrap_or_else(|| DEFAULT_REPO.into())
}

fn data_dir() -> Result<PathBuf> {
    let base = match std::env::var("XDG_DATA_HOME") {
        Ok(d) if !d.is_empty() => PathBuf::from(d),
        _ => home()?.join(".local/share"),
    };
    Ok(base.join("twig"))
}

fn herdr_config() -> Result<PathBuf> {
    match std::env::var("HERDR_CONFIG_PATH") {
        Ok(p) if !p.is_empty() => Ok(PathBuf::from(p)),
        _ => Ok(home()?.join(".config/herdr/config.toml")),
    }
}

fn curl() -> Result<Command> {
    if sys::which("curl").is_none() {
        bail!("curl not found on PATH");
    }
    let mut cmd = Command::new("curl");
    cmd.args(["-fsSL", "--retry", "2"]);
    Ok(cmd)
}

/// (major, minor, patch) from "v1.2.3" or "1.2.3".
fn parse_version(v: &str) -> Option<(u64, u64, u64)> {
    let mut parts = v
        .trim()
        .trim_start_matches('v')
        .split('.')
        .map(|p| p.parse::<u64>().ok());
    Some((parts.next()??, parts.next()??, parts.next()??))
}

fn latest_tag() -> Result<String> {
    let mut cmd = curl()?;
    cmd.args(["-H", "Accept: application/vnd.github+json"]);
    if let Ok(token) = std::env::var("GITHUB_TOKEN") {
        cmd.args(["-H", &format!("Authorization: Bearer {token}")]);
    }
    cmd.arg(format!(
        "https://api.github.com/repos/{}/releases/latest",
        repo()
    ));
    let body = sys::output(&mut cmd).context("could not query the latest release on GitHub")?;
    let json: serde_json::Value =
        serde_json::from_str(&body).context("unexpected response from GitHub")?;
    json["tag_name"]
        .as_str()
        .map(String::from)
        .context("release without tag_name")
}

pub fn update(check: bool, version: Option<String>) -> Result<()> {
    let current = env!("CARGO_PKG_VERSION");
    let forced = version.is_some();
    let tag = match version {
        Some(v) if v.starts_with('v') => v,
        Some(v) => format!("v{v}"),
        None => latest_tag()?,
    };
    let newer = match (parse_version(&tag), parse_version(current)) {
        (Some(t), Some(c)) => t > c,
        _ => true,
    };

    if !forced && !newer {
        ui::ok(format!("twig is already on the latest version ({current})"));
        return Ok(());
    }
    if check {
        ui::info(format!(
            "new version available: {tag} (current: {current}). Run `twig update`."
        ));
        return Ok(());
    }

    ui::info(format!("updating twig {current} → {tag}"));
    let script = std::env::temp_dir().join(format!("twig-install-{}.sh", std::process::id()));
    let url = format!(
        "https://raw.githubusercontent.com/{}/{tag}/install.sh",
        repo()
    );
    let result = sys::run(curl()?.arg("-o").arg(&script).arg(&url))
        .with_context(|| format!("could not download {url}"))
        .and_then(|_| {
            sys::run(
                Command::new("sh")
                    .arg(&script)
                    .args(["--update", "--version", &tag]),
            )
        });
    let _ = fs::remove_file(&script);
    result
}

// ───────────────────────── uninstall ─────────────────────────

/// Removes the `# twig:begin` … `# twig:end` block the installer appended. Returns whether it changed.
fn strip_herdr_block(text: &str) -> Option<String> {
    let start = text.find(HERDR_BLOCK_BEGIN)?;
    let end_rel = text[start..].find(HERDR_BLOCK_END)?;
    let mut end = start + end_rel + HERDR_BLOCK_END.len();
    if text[end..].starts_with('\n') {
        end += 1;
    }
    let before = text[..start].trim_end_matches('\n');
    let after = &text[end..];
    let mut out = before.to_string();
    if !out.is_empty() {
        out.push('\n');
    }
    if !after.trim().is_empty() {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(after.trim_start_matches('\n'));
    }
    Some(out)
}

fn remove_path(p: &Path, what: &str) {
    let res = if p.is_dir() && !p.is_symlink() {
        fs::remove_dir_all(p)
    } else {
        fs::remove_file(p)
    };
    match res {
        Ok(()) => ui::ok(format!("{what} removed: {}", p.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => ui::warn(format!("could not remove {}: {e}", p.display())),
    }
}

fn completion_files() -> Result<Vec<PathBuf>> {
    let data = match std::env::var("XDG_DATA_HOME") {
        Ok(d) if !d.is_empty() => PathBuf::from(d),
        _ => home()?.join(".local/share"),
    };
    Ok(vec![
        data.join("bash-completion/completions/twig"),
        data.join("zsh/site-functions/_twig"),
        home()?.join(".config/fish/completions/twig.fish"),
    ])
}

fn sudo() -> Option<&'static str> {
    let root = sys::output(Command::new("id").arg("-u"))
        .map(|s| s.trim() == "0")
        .unwrap_or(false);
    (!root && sys::which("sudo").is_some()).then_some("sudo")
}

fn run_maybe_sudo(args: &[&str]) -> Result<()> {
    let mut cmd = match sudo() {
        Some(s) => {
            let mut c = Command::new(s);
            c.args(args);
            c
        }
        None => {
            let mut c = Command::new(args[0]);
            c.args(&args[1..]);
            c
        }
    };
    sys::run(&mut cmd)
}

/// Package names per manager: (brew, apt/dpkg, dnf/rpm, pacman).
struct Packages {
    brew: &'static str,
    deb: &'static str,
    rpm: &'static str,
    pacman: &'static str,
}

/// Uninstalls a tool the way it was installed: package manager if one owns it, else the binary file.
fn remove_tool(bin: &str, pkgs: &Packages) -> Result<()> {
    if sys::which("brew").is_some()
        && sys::quiet(Command::new("brew").args(["list", "--formula", pkgs.brew]))
    {
        return sys::run(Command::new("brew").args(["uninstall", pkgs.brew]));
    }
    if sys::which("dpkg").is_some() && sys::quiet(Command::new("dpkg").args(["-s", pkgs.deb])) {
        return run_maybe_sudo(&["apt-get", "remove", "-y", pkgs.deb]);
    }
    if sys::which("rpm").is_some() && sys::quiet(Command::new("rpm").args(["-q", pkgs.rpm])) {
        return run_maybe_sudo(&["dnf", "remove", "-y", pkgs.rpm]);
    }
    if sys::which("pacman").is_some()
        && sys::quiet(Command::new("pacman").args(["-Qi", pkgs.pacman]))
    {
        return run_maybe_sudo(&["pacman", "-R", "--noconfirm", pkgs.pacman]);
    }
    let path = sys::which(bin).context("binary not found")?;
    if path.starts_with(home()?) {
        fs::remove_file(&path)?;
        ui::ok(format!("{} removed", path.display()));
        return Ok(());
    }
    bail!(
        "don't know how {bin} was installed; remove it manually: {}",
        path.display()
    )
}

pub fn uninstall(yes: bool) -> Result<()> {
    if !yes && !ui::confirm("remove twig (binary, herdr plugin, skill and completions)? [y/N]: ") {
        bail!("cancelled (use --yes to skip the prompt)");
    }

    // herdr plugin and shortcut
    if sys::which("herdr").is_some() {
        if sys::quiet(Command::new("herdr").args(["plugin", "unlink", "twig"])) {
            ui::ok("twig plugin unlinked from herdr");
        }
    }
    let hc = herdr_config()?;
    if let Ok(text) = fs::read_to_string(&hc) {
        if let Some(stripped) = strip_herdr_block(&text) {
            fs::write(&hc, stripped)?;
            ui::ok(format!("twig shortcut removed from {}", hc.display()));
            if sys::which("herdr").is_some() {
                sys::quiet(Command::new("herdr").args(["server", "reload-config"]));
            }
        }
    }
    remove_path(&data_dir()?, "twig data (herdr plugin)");

    // AI skill: only ours
    for dir in [
        ".claude/skills/twig",
        ".agents/skills/twig",
        ".codex/skills/twig",
    ] {
        let skill = home()?.join(dir);
        let ours = fs::read_to_string(skill.join("SKILL.md"))
            .map(|s| s.contains("name: twig"))
            .unwrap_or(false);
        if ours || skill.is_symlink() {
            remove_path(&skill, "skill");
        }
    }

    for f in completion_files()? {
        remove_path(&f, "completion");
    }

    // PATH line install.sh appended (`export PATH="...:$PATH"  # twig`)
    for rc in [".bashrc", ".zshrc"] {
        let rc = home()?.join(rc);
        let Ok(text) = fs::read_to_string(&rc) else {
            continue;
        };
        let kept: Vec<&str> = text
            .lines()
            .filter(|l| !(l.starts_with("export PATH=") && l.trim_end().ends_with("# twig")))
            .collect();
        if kept.len() != text.lines().count() {
            fs::write(&rc, kept.join("\n") + "\n")?;
            ui::ok(format!("PATH line removed from {}", rc.display()));
        }
    }

    let cfg = config_path()?;
    if cfg.exists() {
        if !yes
            && ui::confirm(&format!(
                "also remove the config ({})? [y/N]: ",
                cfg.display()
            ))
        {
            remove_path(&cfg, "config");
        } else {
            ui::info(format!("config kept: {}", cfg.display()));
        }
    }
    ui::info("card folders (paths.worktrees) were not touched; use `twig rm` first if you want");

    if !yes {
        let tools: [(&str, &str, Packages); 3] = [
            (
                "az",
                "Azure CLI",
                Packages {
                    brew: "azure-cli",
                    deb: "azure-cli",
                    rpm: "azure-cli",
                    pacman: "azure-cli",
                },
            ),
            (
                "gh",
                "GitHub CLI",
                Packages {
                    brew: "gh",
                    deb: "gh",
                    rpm: "gh",
                    pacman: "github-cli",
                },
            ),
            (
                "herdr",
                "herdr",
                Packages {
                    brew: "herdr",
                    deb: "herdr",
                    rpm: "herdr",
                    pacman: "herdr",
                },
            ),
        ];
        for (bin, name, pkgs) in &tools {
            if sys::which(bin).is_none() {
                continue;
            }
            if !ui::confirm(&format!("also remove {name} ({bin})? [y/N]: ")) {
                continue;
            }
            if *bin == "herdr" {
                sys::quiet(Command::new("herdr").args(["server", "stop"]));
            }
            if let Err(e) = remove_tool(bin, pkgs) {
                ui::warn(format!("{bin}: {e:#}"));
            }
        }
    }

    // last: the binary itself (the PATH entry, symlink or file)
    let bin = sys::which("twig").or_else(|| std::env::current_exe().ok());
    match bin {
        Some(b) => remove_path(&b, "binary"),
        None => ui::warn("twig binary not found on PATH"),
    }
    ui::ok("twig uninstalled");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_versions() {
        assert_eq!(parse_version("v1.2.3"), Some((1, 2, 3)));
        assert!(parse_version("v0.10.0") > parse_version("0.9.9"));
        assert_eq!(parse_version("v1.2"), None);
    }

    #[test]
    fn removes_only_the_twig_block() {
        let text = "[ui]\ntheme = \"x\"\n\n# twig:begin (managed)\n[[keys.command]]\nkey = \"a\"\n# twig:end\n\n[other]\nx = 1\n";
        assert_eq!(
            strip_herdr_block(text).unwrap(),
            "[ui]\ntheme = \"x\"\n\n[other]\nx = 1\n"
        );
        assert_eq!(strip_herdr_block("[ui]\n"), None);
        assert_eq!(
            strip_herdr_block("[ui]\n\n# twig:begin\nk\n# twig:end\n").unwrap(),
            "[ui]\n"
        );
    }
}
