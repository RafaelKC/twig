//! `twig watch install` / `uninstall`: run the watch as a user service (systemd on Linux, a
//! LaunchAgent on macOS). Windows only gets the `schtasks` command to run by hand.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};

use crate::config::home;
use crate::doctor::Report;
use crate::{sys, ui};

const UNIT: &str = "twig-watch.service";
const LABEL: &str = "dev.twig.watch";

/// Variables the service must keep from the installing shell besides `PATH`.
fn forwarded_env() -> Vec<(String, String)> {
    std::env::vars()
        .filter(|(k, _)| {
            matches!(k.as_str(), "TWIG_CONFIG" | "TWIG_TEMPLATES" | "TWIG_WATCH")
                || k.starts_with("TWIG__")
        })
        .collect()
}

fn systemd_quote(s: &str) -> String {
    let body = s
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('%', "%%")
        .replace('$', "$$");
    format!("\"{body}\"")
}

/// The user unit. `KillMode=process` is what keeps a herdr server started by `twig new` (and
/// every agent in it) alive when the service restarts or stops: with the default
/// (`control-group`) systemd would kill the whole cgroup.
pub fn systemd_unit(exe: &Path, path: &str, env: &[(String, String)]) -> String {
    let mut vars = format!("Environment={}\n", systemd_quote(&format!("PATH={path}")));
    for (k, v) in env {
        vars += &format!("Environment={}\n", systemd_quote(&format!("{k}={v}")));
    }
    format!(
        "[Unit]\n\
         Description=twig watch: start cards from tagged work items\n\
         After=network-online.target\n\
         \n\
         [Service]\n\
         Type=simple\n\
         ExecStart={} watch\n\
         {vars}\
         Restart=on-failure\n\
         RestartSec=30\n\
         KillMode=process\n\
         \n\
         [Install]\n\
         WantedBy=default.target\n",
        systemd_quote(&exe.display().to_string())
    )
}

fn xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// The LaunchAgent. `AbandonProcessGroup` is the `KillMode=process` of launchd.
pub fn launchd_plist(exe: &Path, path: &str, env: &[(String, String)], log: &Path) -> String {
    let mut vars = format!("    <key>PATH</key><string>{}</string>\n", xml(path));
    for (k, v) in env {
        vars += &format!("    <key>{}</key><string>{}</string>\n", xml(k), xml(v));
    }
    let log = xml(&log.display().to_string());
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\">\n\
         <dict>\n\
         \x20 <key>Label</key><string>{LABEL}</string>\n\
         \x20 <key>ProgramArguments</key>\n\
         \x20 <array><string>{}</string><string>watch</string></array>\n\
         \x20 <key>EnvironmentVariables</key>\n\
         \x20 <dict>\n\
         {vars}\
         \x20 </dict>\n\
         \x20 <key>RunAtLoad</key><true/>\n\
         \x20 <key>KeepAlive</key><true/>\n\
         \x20 <key>AbandonProcessGroup</key><true/>\n\
         \x20 <key>StandardOutPath</key><string>{log}</string>\n\
         \x20 <key>StandardErrorPath</key><string>{log}</string>\n\
         </dict>\n\
         </plist>\n",
        xml(&exe.display().to_string())
    )
}

pub fn schtasks_command(exe: &Path) -> String {
    format!(
        "schtasks /Create /SC ONLOGON /TN twig-watch /TR \"\\\"{}\\\" watch\"",
        exe.display()
    )
}

fn unit_file() -> Result<PathBuf> {
    let base = dirs::config_dir().context("user config folder not found")?;
    Ok(base.join("systemd/user").join(UNIT))
}

fn plist_file() -> Result<PathBuf> {
    Ok(home()?
        .join("Library/LaunchAgents")
        .join(format!("{LABEL}.plist")))
}

fn macos() -> bool {
    cfg!(target_os = "macos")
}

#[cfg(unix)]
fn uid() -> u32 {
    // SAFETY: getuid has no preconditions.
    unsafe { libc::getuid() }
}

#[cfg(not(unix))]
fn uid() -> u32 {
    0
}

fn is_wsl() -> bool {
    fs::read_to_string("/proc/version").is_ok_and(|v| v.to_lowercase().contains("microsoft"))
}

fn wsl_systemd_enabled() -> bool {
    fs::read_to_string("/etc/wsl.conf").is_ok_and(|c| {
        c.lines()
            .any(|l| l.replace(' ', "").eq_ignore_ascii_case("systemd=true"))
    })
}

fn systemctl(args: &[&str]) -> Command {
    let mut c = Command::new("systemctl");
    c.arg("--user").args(args);
    c
}

/// `twig watch --once --dry-run` with the environment the service will have, so a missing
/// tool or a login that needs the shell's keyring fails here and not at the first cycle.
fn preflight(exe: &Path, path: &str, env: &[(String, String)]) -> Result<()> {
    ui::info("checking the setup with the service's environment (twig watch --once --dry-run)...");
    let mut cmd = Command::new(exe);
    cmd.args(["watch", "--once", "--dry-run"])
        .env_clear()
        .env("PATH", path);
    for k in [
        "HOME",
        "USER",
        "LOGNAME",
        "XDG_RUNTIME_DIR",
        "XDG_CONFIG_HOME",
        "XDG_STATE_HOME",
    ] {
        if let Some(v) = std::env::var_os(k) {
            cmd.env(k, v);
        }
    }
    cmd.envs(env.iter().cloned());
    let out = cmd.stdin(std::process::Stdio::null()).output()?;
    if !out.status.success() {
        bail!(
            "the watch does not work with the service's environment (a service does not inherit your shell: no ~/.bashrc, no keyring session):\n{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Ok(())
}

pub fn install() -> Result<()> {
    let exe = std::env::current_exe().context("could not find the twig binary")?;
    if cfg!(windows) {
        ui::info(
            "no service on Windows yet: run this once, in a terminal, to start the watch at logon",
        );
        println!("  {}", schtasks_command(&exe));
        ui::info("or run `twig watch` in a herdr pane (docs/WATCH.md)");
        return Ok(());
    }
    let path = std::env::var("PATH").context("PATH is not set")?;
    let env = forwarded_env();
    if macos() {
        let file = plist_file()?;
        preflight(&exe, &path, &env)?;
        let log = super::ledger::state_dir()?
            .join("watch")
            .join("launchd.log");
        fs::create_dir_all(log.parent().unwrap())?;
        if let Some(dir) = file.parent() {
            fs::create_dir_all(dir)?;
        }
        fs::write(&file, launchd_plist(&exe, &path, &env, &log))?;
        let domain = format!("gui/{}", uid());
        sys::quiet(Command::new("launchctl").args(["bootout", &format!("{domain}/{LABEL}")]));
        sys::run(Command::new("launchctl").args([
            "bootstrap",
            &domain,
            &file.display().to_string(),
        ]))?;
        ui::ok(format!(
            "LaunchAgent installed and started: {}",
            file.display()
        ));
        ui::info(format!("logs: {}", log.display()));
        return Ok(());
    }

    if sys::which("systemctl").is_none() || !sys::quiet(&mut systemctl(&["show-environment"])) {
        if is_wsl() && !wsl_systemd_enabled() {
            bail!("systemd is off in this WSL distro: add `[boot]\\nsystemd=true` to /etc/wsl.conf, run `wsl --shutdown` from Windows and try again (or run `twig watch` in a herdr pane)");
        }
        bail!("no systemd user session here (systemctl --user does not work); run `twig watch` in a herdr pane instead");
    }
    preflight(&exe, &path, &env)?;
    let file = unit_file()?;
    if let Some(dir) = file.parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(&file, systemd_unit(&exe, &path, &env))?;
    sys::run(&mut systemctl(&["daemon-reload"]))?;
    sys::run(&mut systemctl(&["enable", "--now", UNIT]))?;
    ui::ok(format!("service installed and started: {}", file.display()));
    ui::info("logs: journalctl --user -u twig-watch -f");
    ui::info("to keep it running with no session open: sudo loginctl enable-linger $USER");
    if is_wsl() {
        ui::warn("WSL stops the distro when no terminal is open, and the watch stops with it");
    }
    Ok(())
}

fn installed_file() -> Result<PathBuf> {
    if macos() {
        plist_file()
    } else {
        unit_file()
    }
}

/// Stop and delete the service; whether there was one.
fn remove() -> Result<bool> {
    if cfg!(windows) {
        return Ok(false);
    }
    let file = installed_file()?;
    if !file.exists() {
        return Ok(false);
    }
    if macos() {
        sys::quiet(Command::new("launchctl").args(["bootout", &format!("gui/{}/{LABEL}", uid())]));
    } else {
        sys::quiet(&mut systemctl(&["disable", "--now", UNIT]));
    }
    fs::remove_file(&file)?;
    if !macos() {
        sys::quiet(&mut systemctl(&["daemon-reload"]));
    }
    Ok(true)
}

pub fn uninstall() -> Result<()> {
    if cfg!(windows) {
        ui::info("remove the task with: schtasks /Delete /TN twig-watch /F");
        return Ok(());
    }
    if remove()? {
        ui::ok("watch service removed");
    } else {
        ui::info("the watch service was not installed");
    }
    Ok(())
}

/// For `twig uninstall`: remove the service if there is one.
pub fn remove_for_uninstall() {
    match remove() {
        Ok(true) => ui::ok("watch service removed"),
        Ok(false) => {}
        Err(e) => ui::warn(format!("watch service: {e:#}")),
    }
}

fn installed() -> bool {
    installed_file().is_ok_and(|f| f.exists())
}

fn running() -> bool {
    if macos() {
        sys::quiet(Command::new("launchctl").args(["print", &format!("gui/{}/{LABEL}", uid())]))
    } else {
        sys::quiet(&mut systemctl(&["is-active", "--quiet", UNIT]))
    }
}

pub fn describe() -> String {
    if cfg!(windows) {
        return "no service support on Windows (see `twig watch install`)".into();
    }
    match (installed(), installed() && running()) {
        (false, _) => {
            "not installed (`twig watch install`, or run `twig watch` in a herdr pane)".into()
        }
        (true, true) => "installed, running".into(),
        (true, false) => "installed, NOT running".into(),
    }
}

pub fn doctor(r: &mut Report) {
    if cfg!(windows) {
        return;
    }
    if !installed() {
        r.info("service not installed (`twig watch install`, or run `twig watch` in a herdr pane)");
        return;
    }
    if running() {
        r.ok("service installed and running");
    } else {
        r.warn("service installed but not running (journalctl --user -u twig-watch)");
    }
    if is_wsl() {
        r.warn("WSL stops the distro when no terminal is open, and the watch stops with it");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_unit_keeps_the_herdr_server_alive_across_restarts() {
        let u = systemd_unit(
            Path::new("/home/me/.local/bin/twig"),
            "/usr/bin:/home/me/bin",
            &[("TWIG_CONFIG".into(), "/x/config.toml".into())],
        );
        assert!(u.contains("ExecStart=\"/home/me/.local/bin/twig\" watch\n"));
        assert!(u.contains("Environment=\"PATH=/usr/bin:/home/me/bin\"\n"));
        assert!(u.contains("Environment=\"TWIG_CONFIG=/x/config.toml\"\n"));
        assert!(u.contains("KillMode=process\n"));
        assert!(u.contains("Restart=on-failure\nRestartSec=30\n"));
        assert!(u.contains("WantedBy=default.target"));
    }

    #[test]
    fn unit_values_are_quoted_and_percent_escaped() {
        let u = systemd_unit(Path::new("/opt/my twig/twig"), "/a%b:$c", &[]);
        assert!(u.contains("ExecStart=\"/opt/my twig/twig\" watch"));
        assert!(u.contains("PATH=/a%%b:$$c"));
    }

    #[test]
    fn the_plist_abandons_the_process_group() {
        let p = launchd_plist(
            Path::new("/usr/local/bin/twig"),
            "/usr/bin:/a&b",
            &[],
            Path::new("/Users/me/Library/twig/launchd.log"),
        );
        assert!(p.contains("<key>Label</key><string>dev.twig.watch</string>"));
        assert!(p.contains("<string>/usr/local/bin/twig</string><string>watch</string>"));
        assert!(p.contains("<key>PATH</key><string>/usr/bin:/a&amp;b</string>"));
        assert!(p.contains("<key>AbandonProcessGroup</key><true/>"));
        assert!(p.contains("<key>RunAtLoad</key><true/>"));
        assert!(p.contains("<key>KeepAlive</key><true/>"));
        assert!(p.contains("StandardOutPath"));
    }

    #[test]
    fn schtasks_runs_the_watch_at_logon() {
        let c = schtasks_command(Path::new("C:\\bin\\twig.exe"));
        assert!(c.starts_with("schtasks /Create /SC ONLOGON /TN twig-watch /TR"));
        assert!(c.contains("twig.exe"));
    }
}
