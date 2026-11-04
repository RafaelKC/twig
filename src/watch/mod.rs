//! `twig watch`: poll the provider for items that carry a rule's tag and run
//! `twig new <id> --template <template>` for each one, once. Rules live in `watch.toml`
//! ([`config`]), what was started in a ledger ([`ledger`]), a cycle is decided in [`engine`] and
//! the background service is set up in [`service`]. Security notes: docs/WATCH.md.

pub mod config;
mod engine;
mod ledger;
pub mod service;

use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};

use crate::config::Config;
use crate::provider::{self, Candidate, Provider};
use crate::templates::{self, Templates};
use crate::{card, config_edit, parked, sys, ui};
use config::{watch_path, WatchCfg};
use engine::{CardState, Ctx, Dispatch, Executor, Memory, Source};
use ledger::Ledger;

static STOP: AtomicBool = AtomicBool::new(false);

fn stopping() -> bool {
    STOP.load(Ordering::Relaxed)
}

#[cfg(unix)]
fn catch_signals() {
    extern "C" fn on_signal(_: libc::c_int) {
        STOP.store(true, Ordering::Relaxed);
    }
    // SAFETY: the handler only stores into an atomic.
    unsafe {
        libc::signal(libc::SIGINT, on_signal as *const () as libc::sighandler_t);
        libc::signal(libc::SIGTERM, on_signal as *const () as libc::sighandler_t);
    }
}

#[cfg(not(unix))]
fn catch_signals() {}

/// A timestamped line on stdout (journald and launchd add their own, a terminal has none).
fn log(msg: &str) {
    let ts = parked::now_local().unwrap_or_else(parked::now_utc);
    println!("{ts} {msg}");
}

// ───────────────────────── single instance ─────────────────────────

/// Held for the life of the process; a second `twig watch` fails to get it.
struct Lock {
    _file: File,
}

fn lock() -> Result<Lock> {
    let file = ledger::state_dir()?.join("watch.lock");
    if let Some(dir) = file.parent() {
        fs::create_dir_all(dir)?;
    }
    let busy = || {
        anyhow::anyhow!(
            "another `twig watch` is already running (lock: {})",
            file.display()
        )
    };
    #[cfg(unix)]
    {
        use std::os::unix::io::AsRawFd;
        let f = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&file)?;
        // SAFETY: plain flock on a file descriptor we own; released when the file closes.
        if unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            return Err(busy());
        }
        Ok(Lock { _file: f })
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        let f = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .share_mode(0)
            .open(&file)
            .map_err(|_| busy())?;
        Ok(Lock { _file: f })
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = busy;
        let f = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&file)?;
        Ok(Lock { _file: f })
    }
}

// ───────────────────────── the real source and executor ─────────────────────────

struct ProviderSource<'a> {
    cfg: &'a Config,
    p: Box<dyn Provider + 'a>,
}

impl Source for ProviderSource<'_> {
    fn query(&self, scope: &str, tags: &[String], mine: bool) -> Result<Vec<Candidate>> {
        self.p.query_tagged(scope, tags, mine)
    }

    fn valid_id(&self, id: &str) -> bool {
        self.p.is_watch_id(id)
    }

    fn is_finished(&self, state: &str) -> bool {
        self.p.is_finished(state)
    }

    fn trusted(&self, c: &Candidate, tag: &str, trusted: &[String]) -> Result<bool> {
        self.p.trusted_tagger(c, tag, trusted)
    }

    fn card_state(&self, id: &str) -> CardState {
        let Ok(key) = card::key(self.cfg, id) else {
            return CardState::Missing;
        };
        let dir = self.cfg.card_dir(&key);
        if !dir.is_dir() || card::repos_in(&dir).is_empty() {
            return CardState::Missing;
        }
        if parked::read(self.cfg, &key).is_some() {
            CardState::Parked
        } else {
            CardState::Active
        }
    }

    fn herdr_ready(&self) -> bool {
        crate::herdr::server_up()
    }

    fn mark_tag(&self, c: &Candidate, tag: &str) -> Result<()> {
        self.p.add_tag(c, tag)
    }
}

/// `twig new` in a child process: it mutates the config, prints straight to stdout and may
/// abort, and none of that may take the loop down with it.
struct Subprocess<'a> {
    w: &'a WatchCfg,
    logs: PathBuf,
}

fn log_name(id: &str) -> String {
    id.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

impl Subprocess<'_> {
    fn run(&self, id: &str, template: &str, log_file: &Path) -> Result<Dispatch> {
        let began = Instant::now();
        fs::create_dir_all(&self.logs)?;
        let out = File::create(log_file)?;
        let mut cmd =
            Command::new(std::env::current_exe().context("could not find the twig binary")?);
        // no -H / -c: nothing opens a window or attaches to herdr
        cmd.args(["new", id, "--template", template])
            .env("TWIG_NO_UPDATE_CHECK", "1")
            .stdin(Stdio::null())
            .stdout(out.try_clone()?)
            .stderr(out);
        // its own process group: Ctrl+C aimed at the watch must not cut a `twig new` in half
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
        }
        let mut child = cmd.spawn().context("could not run `twig new`")?;
        let limit = self.w.timeout();
        let status = loop {
            if let Some(s) = child.try_wait()? {
                break Some(s);
            }
            if began.elapsed() >= limit {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            thread::sleep(Duration::from_millis(200));
        };
        let secs = began.elapsed().as_secs();
        let log = Some(log_file.display().to_string());
        Ok(match status {
            Some(s) => Dispatch {
                ok: s.success(),
                exit: s.code(),
                log,
                secs,
                detail: format!("`twig new` exited with {s}; see the log"),
            },
            None => Dispatch {
                ok: false,
                exit: None,
                log,
                secs,
                detail: format!("`twig new` timed out after {}", self.w.timeout),
            },
        })
    }
}

impl Executor for Subprocess<'_> {
    fn dispatch(&self, id: &str, template: &str) -> Dispatch {
        let log_file = self.logs.join(format!("{}.log", log_name(id)));
        let d = self
            .run(id, template, &log_file)
            .unwrap_or_else(|e| Dispatch {
                detail: format!("{e:#}"),
                ..Default::default()
            });
        if !self.w.on_trigger.is_empty() {
            let result = if d.ok { "ok" } else { "fail" };
            let mut cmd = sys::shell(&self.w.on_trigger, &std::env::temp_dir());
            cmd.env("TWIG_CARD", id)
                .env("TWIG_TEMPLATE", template)
                .env("TWIG_WATCH_RESULT", result);
            if !sys::quiet(&mut cmd) {
                log(&format!("{id}: on_trigger failed"));
            }
        }
        d
    }
}

// ───────────────────────── loading ─────────────────────────

/// Config, watch file and templates, validated. Errors in the watch file are fatal; warnings
/// are returned.
fn load_all() -> Result<(Config, WatchCfg, Vec<String>)> {
    let cfg = Config::load()?;
    let file = watch_path()?;
    if !file.exists() {
        bail!(
            "no watch file ({}): `twig watch edit` creates one (format in docs/CONFIG.md)",
            file.display()
        );
    }
    let w = config::parse_file(&file)?;
    let tpls: Option<Templates> = templates::load()?;
    let found = config::validate(Some(&cfg), tpls.as_ref(), &w);
    if !found.errors.is_empty() {
        bail!("{}: {}", file.display(), found.errors.join("; "));
    }
    Ok((cfg, w, found.warnings))
}

/// Wait `total`, waking often to notice a stop request.
fn sleep(total: Duration) {
    let end = Instant::now() + total;
    while !stopping() && Instant::now() < end {
        thread::sleep(
            Duration::from_millis(250).min(end.saturating_duration_since(Instant::now())),
        );
    }
}

/// `interval` ±10%.
fn jittered(base: Duration) -> Duration {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.subsec_nanos());
    let pct = (nanos % 2001) as f64 / 10_000.0 - 0.1; // -0.1 ..= 0.1
    base.mul_f64(1.0 + pct)
}

/// `twig watch [--once] [--dry-run]`.
pub fn run(once: bool, dry_run: bool) -> Result<()> {
    let (mut cfg, mut w, warnings) = load_all()?;
    for m in &warnings {
        ui::warn(m);
    }
    // a dry run changes nothing, so it may run beside the real watch
    let _lock = if dry_run { None } else { Some(lock()?) };
    catch_signals();
    let ledger_file = ledger::path()?;
    let logs = ledger::state_dir()?.join("watch").join("logs");
    log(&format!(
        "watching {} rule(s) every {} (ledger: {}){}",
        w.rules.len(),
        w.interval,
        ledger_file.display(),
        if dry_run { ", dry run" } else { "" }
    ));
    let mut mem = Memory::default();
    let mut last_invalid = String::new();
    let mut first = true;
    loop {
        if !first {
            // reload every cycle; an invalid edit keeps the last good version
            match load_all() {
                Ok((c, nw, _)) => {
                    cfg = c;
                    w = nw;
                    last_invalid.clear();
                }
                Err(e) => {
                    let msg = format!("{e:#}");
                    if msg != last_invalid {
                        log(&format!(
                            "config invalid, keeping the last valid one: {msg}"
                        ));
                        last_invalid = msg;
                    }
                }
            }
        }
        first = false;
        let src = ProviderSource {
            cfg: &cfg,
            p: provider::current(&cfg),
        };
        let exec = Subprocess {
            w: &w,
            logs: logs.clone(),
        };
        let ctx = Ctx {
            w: &w,
            src: &src,
            exec: &exec,
            ledger: &ledger_file,
            dry_run,
            now: ledger::now(),
            stop: &stopping,
            log: &log,
        };
        let report = match engine::cycle(&ctx, &mut mem) {
            Ok(r) => Some(r),
            Err(e) => {
                log(&format!("cycle failed: {e:#}"));
                None
            }
        };
        if once {
            return match report {
                Some(r) if r.errors == 0 => Ok(()),
                Some(r) => bail!("{} error(s) in the cycle (see above)", r.errors),
                None => bail!("the cycle failed"),
            };
        }
        sleep(jittered(w.interval()));
        if stopping() {
            log("stopping");
            return Ok(());
        }
    }
}

// ───────────────────────── subcommands ─────────────────────────

pub enum Sub {
    Status,
    Forget { id: String },
    Show,
    Edit,
    Validate,
    Install,
    Uninstall,
}

pub fn subcommand(s: Sub) -> Result<()> {
    match s {
        Sub::Status => status(),
        Sub::Forget { id } => forget(&id),
        Sub::Show => show(),
        Sub::Edit => config_edit::edit_file(
            &watch_path()?,
            config::EDIT_TEMPLATE,
            "watch",
            config::check_file,
        ),
        Sub::Validate => validate(),
        Sub::Install => service::install(),
        Sub::Uninstall => service::uninstall(),
    }
}

fn read_file() -> Result<WatchCfg> {
    let file = watch_path()?;
    if !file.exists() {
        bail!(
            "no watch file ({}); `twig watch edit` creates one",
            file.display()
        );
    }
    config::parse_file(&file)
}

fn show() -> Result<()> {
    let w = read_file()?;
    println!("\x1b[1mtwig watch\x1b[0m  {}", watch_path()?.display());
    println!("  interval        {}", w.interval);
    println!("  assigned_to_me  {}", w.assigned_to_me);
    println!("  max_per_cycle   {}", w.max_per_cycle);
    println!("  max_active      {}", w.max_active);
    println!("  timeout         {}", w.timeout);
    println!("  start_herdr     {}", w.start_herdr);
    if !w.mark_tag.is_empty() {
        println!("  mark_tag        {}", w.mark_tag);
    }
    if !w.on_trigger.is_empty() {
        println!("  on_trigger      {}", w.on_trigger);
    }
    for r in &w.rules {
        let mut line = format!(
            "  rule  tag `{}` in {} → template {}",
            r.tag,
            r.scope(),
            r.template
        );
        if !r.trusted_taggers.is_empty() {
            line += &format!("  (trusted: {})", r.trusted_taggers.join(", "));
        }
        println!("{line}");
    }
    Ok(())
}

fn validate() -> Result<()> {
    let w = read_file()?;
    let cfg = Config::load().ok();
    if cfg.is_none() {
        ui::warn("the config is invalid: provider and repo checks were skipped");
    }
    let tpls = templates::load().ok().flatten();
    if tpls.is_none() {
        ui::warn("no usable templates file: template checks were skipped");
    }
    let found = config::validate(cfg.as_ref(), tpls.as_ref(), &w);
    for m in &found.warnings {
        ui::warn(m);
    }
    if !found.errors.is_empty() {
        bail!("{}", found.errors.join("\n"));
    }
    ui::ok(format!(
        "{} valid ({} rule(s))",
        watch_path()?.display(),
        w.rules.len()
    ));
    Ok(())
}

fn ago(at: u64) -> String {
    let d = ledger::now().saturating_sub(at);
    match d {
        0..=89 => format!("{d}s ago"),
        90..=5399 => format!("{}m ago", d / 60),
        5400..=129_599 => format!("{}h ago", d / 3600),
        _ => format!("{}d ago", d / 86_400),
    }
}

fn status() -> Result<()> {
    println!("\x1b[1mservice\x1b[0m");
    println!("  {}", service::describe());
    let file = ledger::path()?;
    let l = Ledger::load(&file)?;
    println!("\x1b[1mledger\x1b[0m  {}", file.display());
    if l.entries.is_empty() {
        println!("  empty");
    }
    let mut rows: Vec<_> = l.entries.iter().collect();
    rows.sort_by_key(|(_, e)| std::cmp::Reverse(e.at));
    for (id, e) in rows {
        let exit = e.exit.map(|c| format!(" exit {c}")).unwrap_or_default();
        println!(
            "  {id}  {:<7} {}  template {}{exit}",
            e.result.name(),
            ago(e.at),
            e.template
        );
        if let Some(log) = &e.log {
            if e.result == ledger::Status::Fail {
                println!("      log: {log}");
            }
        }
    }
    Ok(())
}

fn forget(id: &str) -> Result<()> {
    let file = ledger::path()?;
    let mut l = Ledger::load(&file)?;
    // `owner/repo#N` is matched as typed; a bare number also matches a lone `…#N`
    let key = if l.entries.contains_key(id) {
        Some(id.to_string())
    } else {
        let hits: Vec<_> = l
            .entries
            .keys()
            .filter(|k| k.rsplit_once('#').is_some_and(|(_, n)| n == id))
            .cloned()
            .collect();
        match hits.as_slice() {
            [one] => Some(one.clone()),
            [] => None,
            many => bail!("`{id}` is ambiguous: {}", many.join(", ")),
        }
    };
    let Some(key) = key else {
        bail!("`{id}` is not in the ledger (see `twig watch status`)");
    };
    l.entries.remove(&key);
    l.save(&file)?;
    ui::ok(format!(
        "{key} forgotten: it starts again on the next cycle if it still has the tag"
    ));
    Ok(())
}

/// Ids in the ledger, for shell completion.
pub fn ledger_ids() -> Vec<String> {
    ledger::path()
        .ok()
        .and_then(|f| Ledger::load(&f).ok())
        .map(|l| l.entries.into_keys().collect())
        .unwrap_or_default()
}

/// `twig doctor`: the "watch" section.
pub fn doctor(r: &mut crate::doctor::Report, cfg: &Config) {
    let Ok(file) = watch_path() else { return };
    if !file.exists() {
        return;
    }
    r.section("watch");
    let w = match config::parse_file(&file) {
        Ok(w) => w,
        Err(e) => {
            r.error(format!("{e:#}"));
            return;
        }
    };
    let tpls = match templates::load() {
        Ok(t) => t,
        Err(e) => {
            r.error(format!("templates: {e:#}"));
            None
        }
    };
    let found = config::validate(Some(cfg), tpls.as_ref(), &w);
    for e in &found.errors {
        r.error(e);
    }
    for m in &found.warnings {
        r.warn(m);
    }
    if found.errors.is_empty() {
        r.ok(format!(
            "{} valid ({} rule(s))",
            file.display(),
            w.rules.len()
        ));
    }
    // the provider's own login check is in the "provider" section; the service is what is new
    service::doctor(r);
}
