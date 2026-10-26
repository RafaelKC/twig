//! Integration tests: run the binary with controlled PATH, HOME and TWIG_CONFIG.
//! External tools (herdr, az, claude, code, editor) are fake scripts.
#![cfg(unix)]

use std::fs;
use std::io::Write;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use tempfile::TempDir;

struct Sandbox {
    _root: TempDir,
    bin: PathBuf,
    home: PathBuf,
    config: PathBuf,
}

fn real(bin: &str) -> PathBuf {
    let out = Command::new("sh")
        .args(["-c", &format!("command -v {bin}")])
        .output()
        .unwrap();
    PathBuf::from(String::from_utf8(out.stdout).unwrap().trim())
}

impl Sandbox {
    /// Real git and sh; everything else exists only if the test creates a stub.
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let bin = root.path().join("bin");
        let home = root.path().join("home");
        fs::create_dir_all(&bin).unwrap();
        fs::create_dir_all(&home).unwrap();
        symlink(real("git"), bin.join("git")).unwrap();
        symlink(real("sh"), bin.join("sh")).unwrap();
        let config = home.join(".config/twig/config.toml");
        Self {
            bin,
            home,
            config,
            _root: root,
        }
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.home.join(rel)
    }

    fn stub(&self, name: &str, body: &str) -> &Self {
        let p = self.bin.join(name);
        fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
        self
    }

    /// Executable script outside PATH (to use as an editor).
    fn script(&self, name: &str, body: &str) -> PathBuf {
        let p = self.home.join(name);
        fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
        p
    }

    fn all_tools(&self) -> &Self {
        self.stub("herdr", "exit 0")
            .stub("az", "exit 0")
            .stub("claude", "exit 0")
            .stub("code", "exit 0")
    }

    fn git_identity(&self) -> &Self {
        fs::write(
            self.home.join(".gitconfig"),
            "[user]\n\tname = Test\n\temail = t@t\n",
        )
        .unwrap();
        self
    }

    fn git_repo(&self, rel: &str) -> PathBuf {
        let p = self.path(rel);
        fs::create_dir_all(&p).unwrap();
        assert!(Command::new("git")
            .args(["init", "-q"])
            .current_dir(&p)
            .status()
            .unwrap()
            .success());
        p
    }

    fn write_config(&self, body: &str) {
        fs::create_dir_all(self.config.parent().unwrap()).unwrap();
        fs::write(&self.config, body).unwrap();
    }

    /// Valid config with a repo `r1` at <home>/repos/r1.
    fn valid_config(&self, extra: &str) {
        self.git_repo("repos/r1");
        self.write_config(&format!(
            "version = 1\n{extra}\n[paths]\nworktrees = \"{wt}\"\nrepos = \"{repos}\"\n\n[repos.r1]\ndefault = true\n",
            wt = self.path("wt").display(),
            repos = self.path("repos").display(),
        ));
    }

    fn cmd(&self, args: &[&str]) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_twig"));
        c.args(args)
            .env_clear()
            .env("PATH", &self.bin)
            .env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", self.home.join(".config"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("TWIG_CONFIG", &self.config);
        c
    }

    fn run(&self, args: &[&str]) -> Output {
        self.run_with(self.cmd(args), "")
    }

    fn run_with(&self, mut c: Command, stdin: &str) -> Output {
        c.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // a file just written as executable (stub, copied binary) can still be open for writing
        // in a child another test thread is forking: ETXTBSY until that child execs
        let mut tries = 0;
        let mut child = loop {
            match c.spawn() {
                Err(e) if e.kind() == std::io::ErrorKind::ExecutableFileBusy && tries < 50 => {
                    tries += 1;
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
                r => break r.unwrap(),
            }
        };
        child
            .stdin
            .take()
            .unwrap()
            .write_all(stdin.as_bytes())
            .unwrap();
        child.wait_with_output().unwrap()
    }
}

fn text(o: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

fn assert_exit(o: &Output, code: i32) {
    assert_eq!(o.status.code(), Some(code), "output:\n{}", text(o));
}

fn assert_has(o: &Output, needle: &str) {
    assert!(
        text(o).contains(needle),
        "expected {needle:?} in the output:\n{}",
        text(o)
    );
}

// ───────────────────────── doctor ─────────────────────────

#[test]
fn doctor_all_good() {
    let s = Sandbox::new();
    s.all_tools().git_identity();
    s.valid_config("");
    let o = s.run(&["doctor"]);
    assert_exit(&o, 0);
    assert_has(&o, "all good");
}

#[test]
fn doctor_without_herdr_is_error() {
    let s = Sandbox::new();
    s.stub("az", "exit 0")
        .stub("claude", "exit 0")
        .stub("code", "exit 0")
        .git_identity();
    s.valid_config("");
    let o = s.run(&["doctor"]);
    assert_exit(&o, 1);
    assert_has(&o, "herdr not found on PATH");
}

#[test]
fn doctor_without_ado_access_is_warning() {
    let s = Sandbox::new();
    s.all_tools().git_identity();
    s.stub(
        "az",
        "case \"$*\" in *\"devops project\"*) exit 1;; esac\nexit 0",
    );
    s.valid_config("");
    let o = s.run(&["doctor"]);
    assert_exit(&o, 0);
    assert_has(&o, "az has no access to Azure DevOps");
}

#[test]
fn doctor_without_git_identity_is_warning() {
    let s = Sandbox::new();
    s.all_tools();
    s.valid_config("");
    let o = s.run(&["doctor"]);
    assert_exit(&o, 0);
    assert_has(&o, "user.name not set");
    assert_has(&o, "user.email not set");
}

#[test]
fn doctor_invalid_config_is_error() {
    let s = Sandbox::new();
    s.all_tools().git_identity();
    s.write_config("version = 1\n[vscode]\nactive = true\n");
    let o = s.run(&["doctor"]);
    assert_exit(&o, 1);
    assert_has(&o, "invalid config");
}

#[test]
fn doctor_vscode_disabled_skips_code() {
    let s = Sandbox::new();
    s.stub("herdr", "exit 0")
        .stub("az", "exit 0")
        .stub("claude", "exit 0")
        .git_identity();
    s.valid_config("[vscode]\nenabled = false\n");
    let o = s.run(&["doctor"]);
    assert_exit(&o, 0);
    assert_has(&o, "disabled in the config");
}

#[test]
fn doctor_repo_that_is_not_git_is_error() {
    let s = Sandbox::new();
    s.all_tools().git_identity();
    s.valid_config("");
    fs::create_dir_all(s.path("loose")).unwrap();
    let mut cfg = fs::read_to_string(&s.config).unwrap();
    cfg += &format!(
        "\n[repos.loose]\npath = \"{}\"\n",
        s.path("loose").display()
    );
    fs::write(&s.config, cfg).unwrap();
    let o = s.run(&["doctor"]);
    assert_exit(&o, 1);
    assert_has(&o, "loose: clone not found");
}

// ───────────────────────── config edit ─────────────────────────

fn edit(s: &Sandbox, editor: &Path, stdin: &str) -> Output {
    let mut c = s.cmd(&["config", "edit"]);
    c.env("EDITOR", editor);
    s.run_with(c, stdin)
}

#[test]
fn config_edit_without_changes_is_ok() {
    let s = Sandbox::new();
    s.valid_config("");
    let o = edit(&s, &s.script("noop.sh", "exit 0"), "");
    assert_exit(&o, 0);
    assert_has(&o, "config valid");
}

#[test]
fn config_edit_invalid_and_declines_reopen() {
    let s = Sandbox::new();
    s.valid_config("");
    let o = edit(&s, &s.script("breaks.sh", "echo 'junk =' >> \"$1\""), "n\n");
    assert_exit(&o, 1);
    assert_has(&o, "config left invalid");
}

#[test]
fn config_edit_reopens_until_fixed() {
    let s = Sandbox::new();
    s.valid_config("");
    let original = fs::read_to_string(&s.config).unwrap();
    let backup = s.path("backup.toml");
    fs::write(&backup, original).unwrap();
    let marker = s.path("already-broken");
    // 1st run breaks it, 2nd run restores the original
    let editor = s.script(
        "round-trip.sh",
        &format!(
            "if [ -f {m} ]; then {cat} {b} > \"$1\"; else : > {m}; echo 'junk =' >> \"$1\"; fi",
            m = marker.display(),
            b = backup.display(),
            cat = real("cat").display()
        ),
    );
    let o = edit(&s, &editor, "y\n");
    assert_exit(&o, 0);
    assert_has(&o, "reopen to fix it");
    assert_has(&o, "config valid");
}

#[test]
fn config_edit_creates_template_when_missing() {
    let s = Sandbox::new();
    let o = edit(&s, &s.script("noop.sh", "exit 0"), "");
    assert_exit(&o, 0);
    assert_has(&o, "config created from the template");
    assert!(fs::read_to_string(&s.config)
        .unwrap()
        .contains("version = 1"));
}

#[test]
fn config_edit_prefers_visual_over_editor() {
    let s = Sandbox::new();
    s.valid_config("");
    let marker = s.path("who-opened");
    let visual = s.script("visual.sh", &format!("echo visual > {}", marker.display()));
    let editor = s.script("editor.sh", &format!("echo editor > {}", marker.display()));
    let mut c = s.cmd(&["config", "edit"]);
    c.env("VISUAL", &visual).env("EDITOR", &editor);
    let o = s.run_with(c, "");
    assert_exit(&o, 0);
    assert_eq!(fs::read_to_string(&marker).unwrap().trim(), "visual");
}

// ───────────────────────── code / herdr plugin ─────────────────────────

/// Sandbox with a card `123` (one fake worktree) and a `code` stub that records its argument.
fn code_sandbox() -> (Sandbox, PathBuf) {
    let s = Sandbox::new();
    s.valid_config("");
    fs::create_dir_all(s.path("wt/123/r1/.git")).unwrap();
    let log = s.path("code.log");
    s.stub("code", &format!("echo \"$1\" > {}", log.display()));
    (s, log)
}

fn opened(log: &Path) -> String {
    fs::read_to_string(log)
        .unwrap_or_default()
        .trim()
        .to_string()
}

#[test]
fn code_with_id_opens_the_card_workspace() {
    let (s, log) = code_sandbox();
    let o = s.run(&["code", "123"]);
    assert_exit(&o, 0);
    assert_eq!(
        opened(&log),
        s.path("wt/123/123.code-workspace").display().to_string()
    );
    assert!(s.path("wt/123/123.code-workspace").exists());
}

#[test]
fn code_falls_back_to_known_install_dirs_when_not_on_path() {
    let (s, log) = code_sandbox();
    // the herdr server's PATH may lack the dir that has `code`
    fs::create_dir_all(s.home.join(".local/bin")).unwrap();
    fs::rename(s.bin.join("code"), s.home.join(".local/bin/code")).unwrap();
    let o = s.run(&["code", "123"]);
    assert_exit(&o, 0);
    assert_eq!(
        opened(&log),
        s.path("wt/123/123.code-workspace").display().to_string()
    );
}

#[test]
fn code_without_id_uses_herdr_workspace_label() {
    let (s, log) = code_sandbox();
    let mut c = s.cmd(&["code"]);
    c.env("HERDR_PLUGIN_CONTEXT_JSON", r#"{"workspace_label":"123"}"#);
    let o = s.run_with(c, "");
    assert_exit(&o, 0);
    assert_eq!(
        opened(&log),
        s.path("wt/123/123.code-workspace").display().to_string()
    );
}

#[test]
fn code_without_id_uses_cwd_inside_the_card() {
    let (s, log) = code_sandbox();
    let mut c = s.cmd(&["code"]);
    c.current_dir(s.path("wt/123/r1"));
    let o = s.run_with(c, "");
    assert_exit(&o, 0);
    assert_eq!(
        opened(&log),
        s.path("wt/123/123.code-workspace").display().to_string()
    );
}

#[test]
fn code_outside_a_card_opens_the_folder() {
    let (s, log) = code_sandbox();
    fs::create_dir_all(s.path("other")).unwrap();
    let mut c = s.cmd(&["code"]);
    let ctx = format!(
        r#"{{"workspace_label":"xyz","focused_pane_cwd":"{}"}}"#,
        s.path("other").display()
    );
    c.env("HERDR_PLUGIN_CONTEXT_JSON", ctx);
    let o = s.run_with(c, "");
    assert_exit(&o, 0);
    assert_eq!(opened(&log), s.path("other").display().to_string());
}

#[test]
fn herdr_plugin_opens_the_focused_card() {
    let (s, log) = code_sandbox();
    symlink(env!("CARGO_BIN_EXE_twig"), s.bin.join("twig")).unwrap();
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("herdr-plugins/twig/open-code.sh");
    let mut c = Command::new(real("sh"));
    c.arg(&script)
        .env_clear()
        .env("PATH", &s.bin)
        .env("HOME", &s.home)
        .env("TWIG_CONFIG", &s.config)
        .env("HERDR_PLUGIN_CONTEXT_JSON", r#"{"workspace_label":"123"}"#)
        .env("HERDR_ENV", "1");
    let o = s.run_with(c, "");
    assert_exit(&o, 0);
    assert_eq!(
        opened(&log),
        s.path("wt/123/123.code-workspace").display().to_string()
    );
}

/// Runs `herdr-plugins/twig/launcher.sh` with `input` on stdin, from a card pane's env.
fn run_launcher(input: &str) -> (Sandbox, Output) {
    let s = Sandbox::new();
    s.valid_config("");
    symlink(env!("CARGO_BIN_EXE_twig"), s.bin.join("twig")).unwrap();
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("herdr-plugins/twig/launcher.sh");
    let mut c = Command::new(real("sh"));
    c.arg(&script)
        .env_clear()
        .env("PATH", format!("{}:/usr/bin:/bin", s.bin.display()))
        .env("HOME", &s.home)
        .env("TWIG_CONFIG", &s.config)
        .env("TWIG_CARD_ID", "123")
        .env("PORT", "4244");
    let o = s.run_with(c, input);
    (s, o)
}

#[test]
fn herdr_launcher_runs_twig_subcommands_and_shows_the_exit_code() {
    let (_s, o) = run_launcher("ls --help\n\n");
    assert_exit(&o, 0);
    let out = stdout(&o);
    assert!(out.contains("Usage: twig ls"), "{out}");
    assert!(out.contains("[exit 0]") && out.contains("[enter]"), "{out}");
}

#[test]
fn herdr_launcher_recognises_twig_aliases_and_flags() {
    let (_s, o) = run_launcher("st --help\n\n");
    assert!(stdout(&o).contains("Usage: twig status"), "{}", stdout(&o));
    let (_s, o) = run_launcher("--version\n\n");
    assert!(stdout(&o).contains("twig"), "{}", stdout(&o));
    assert!(stdout(&o).contains("[exit 0]"), "{}", stdout(&o));
    let (_s, o) = run_launcher("echo hi-there\n\n");
    assert!(stdout(&o).contains("hi-there"), "{}", stdout(&o));
}

#[test]
fn herdr_launcher_finds_twig_in_shell_lines_via_local_bin() {
    let s = Sandbox::new();
    s.valid_config("");
    let lb = s.home.join(".local/bin");
    fs::create_dir_all(&lb).unwrap();
    symlink(env!("CARGO_BIN_EXE_twig"), lb.join("twig")).unwrap();
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("herdr-plugins/twig/launcher.sh");
    let mut c = Command::new(real("sh"));
    c.arg(&script)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", &s.home)
        .env("TWIG_CONFIG", &s.config);
    let o = s.run_with(c, "twig ls --help && true\n\n");
    assert!(stdout(&o).contains("Usage: twig ls"), "{}", stdout(&o));
}

#[test]
fn herdr_launcher_runs_other_commands_in_the_shell_without_the_card_env() {
    let (_s, o) =
        run_launcher("echo \"card=${TWIG_CARD_ID:-none} port=${PORT:-none}\"; exit 3\n\n");
    assert_exit(&o, 0);
    let out = stdout(&o);
    assert!(out.contains("card=none port=none"), "{out}");
    assert!(out.contains("[exit 3]"), "{out}");
}

#[test]
fn herdr_launcher_closes_on_an_empty_line_or_esc() {
    for input in ["\n", "\u{1b}\n"] {
        let (_s, o) = run_launcher(input);
        assert_exit(&o, 0);
        assert!(!stdout(&o).contains("[exit"), "{}", stdout(&o));
    }
}

// ───────────────────────── open ─────────────────────────

/// Card `123` with a `code` stub and a `herdr` stub that logs its calls and reports a workspace `123`.
fn open_sandbox(extra: &str) -> (Sandbox, PathBuf, PathBuf) {
    let s = Sandbox::new();
    s.valid_config(extra);
    fs::create_dir_all(s.path("wt/123/r1/.git")).unwrap();
    let code_log = s.path("code.log");
    s.stub("code", &format!("echo \"$1\" > {}", code_log.display()));
    let herdr_log = s.path("herdr.log");
    s.stub(
        "herdr",
        &format!(
            r#"echo "$@" >> {log}
case "$1 $2" in
  "workspace list") echo '{{"result":{{"workspaces":[{{"label":"123","workspace_id":"w1"}}]}}}}' ;;
  "tab list") echo '{{"result":{{"tabs":[{{"label":"agent","tab_id":"t1"}}]}}}}' ;;
  "pane list") echo '{{"result":{{"panes":[{{"pane_id":"w1:p1","tab_id":"t1","cwd":"{cwd}"}}]}}}}' ;;
esac"#,
            log = herdr_log.display(),
            cwd = s.path("wt/123/r1").display()
        ),
    );
    (s, code_log, herdr_log)
}

/// HERDR_ENV=1 so that opening herdr only focuses the workspace instead of launching the UI.
fn run_open(s: &Sandbox, args: &[&str], cwd: Option<PathBuf>) -> Output {
    let mut c = s.cmd(args);
    c.env("HERDR_ENV", "1");
    if let Some(d) = cwd {
        c.current_dir(d);
    }
    s.run_with(c, "")
}

fn workspace_file(s: &Sandbox) -> String {
    s.path("wt/123/123.code-workspace").display().to_string()
}

fn assert_herdr_focused(log: &Path) {
    assert!(
        herdr_calls(log).contains("workspace focus w1"),
        "{}",
        herdr_calls(log)
    );
}

#[test]
fn open_herdr_target_focuses_the_workspace() {
    let (s, code_log, herdr_log) = open_sandbox("");
    assert_exit(&run_open(&s, &["open", "123", "herdr"], None), 0);
    assert_herdr_focused(&herdr_log);
    assert_eq!(opened(&code_log), "");
}

#[test]
fn open_code_target_opens_vscode() {
    let (s, code_log, herdr_log) = open_sandbox("");
    assert_exit(&run_open(&s, &["open", "123", "code"], None), 0);
    assert_eq!(opened(&code_log), workspace_file(&s));
    assert_eq!(herdr_calls(&herdr_log), "");
}

#[test]
fn open_lone_target_infers_the_card() {
    let (s, code_log, herdr_log) = open_sandbox("");
    let cwd = s.path("wt/123/r1");
    assert_exit(&run_open(&s, &["open", "code"], Some(cwd)), 0);
    assert_eq!(opened(&code_log), workspace_file(&s));
    assert_eq!(herdr_calls(&herdr_log), "");
}

#[test]
fn open_without_target_defaults_to_herdr() {
    let (s, code_log, herdr_log) = open_sandbox("");
    assert_exit(&run_open(&s, &["open", "123"], None), 0);
    assert_herdr_focused(&herdr_log);
    assert_eq!(opened(&code_log), "");
}

#[test]
fn open_without_target_uses_the_config_default() {
    let (s, code_log, herdr_log) = open_sandbox("[open]\ndefault = \"code\"\n");
    assert_exit(&run_open(&s, &["open", "123"], None), 0);
    assert_eq!(opened(&code_log), workspace_file(&s));
    assert_eq!(herdr_calls(&herdr_log), "");
}

#[test]
fn open_default_target_means_the_config_default() {
    let (s, code_log, _) = open_sandbox("[open]\ndefault = \"code\"\n");
    assert_exit(&run_open(&s, &["open", "123", "default"], None), 0);
    assert_eq!(opened(&code_log), workspace_file(&s));
}

#[test]
fn open_explicit_target_overrides_the_config_default() {
    let (s, code_log, herdr_log) = open_sandbox("[open]\ndefault = \"code\"\n");
    assert_exit(&run_open(&s, &["open", "123", "herdr"], None), 0);
    assert_herdr_focused(&herdr_log);
    assert_eq!(opened(&code_log), "");
}

#[test]
fn open_without_id_infers_the_current_card() {
    let (s, code_log, herdr_log) = open_sandbox("");
    let cwd = s.path("wt/123/r1");
    assert_exit(&run_open(&s, &["open"], Some(cwd.clone())), 0);
    assert_herdr_focused(&herdr_log);
    assert_eq!(opened(&code_log), "");
    // the target can only follow an id on the command line, so `code` here comes from the config
    let (s, code_log, _) = open_sandbox("[open]\ndefault = \"code\"\n");
    assert_exit(&run_open(&s, &["open"], Some(s.path("wt/123/r1"))), 0);
    assert_eq!(opened(&code_log), workspace_file(&s));
}

#[test]
fn open_herdr_without_id_outside_a_card_errors() {
    let (s, _, herdr_log) = open_sandbox("");
    fs::create_dir_all(s.path("other")).unwrap();
    let o = run_open(&s, &["open"], Some(s.path("other")));
    assert_exit(&o, 1);
    assert_has(&o, "not inside a card");
    assert_eq!(herdr_calls(&herdr_log), "");
}

#[test]
fn open_unknown_target_is_rejected() {
    let (s, code_log, herdr_log) = open_sandbox("");
    let o = run_open(&s, &["open", "123", "emacs"], None);
    assert_exit(&o, 2);
    assert_has(&o, "emacs");
    assert_eq!(opened(&code_log), "");
    assert_eq!(herdr_calls(&herdr_log), "");
}

#[test]
fn open_invalid_config_default_is_a_config_error() {
    let (s, _, _) = open_sandbox("[open]\ndefault = \"emacs\"\n");
    let o = run_open(&s, &["open", "123"], None);
    assert_exit(&o, 1);
    assert_has(&o, "invalid config");
}

#[test]
fn doctor_reports_an_invalid_open_default() {
    let s = Sandbox::new();
    s.all_tools().git_identity();
    s.valid_config("[open]\ndefault = \"emacs\"\n");
    let o = s.run(&["doctor"]);
    assert_exit(&o, 1);
    assert_has(&o, "emacs");
}

#[test]
fn herdr_and_code_aliases_still_work() {
    let (s, code_log, herdr_log) = open_sandbox("");
    assert_exit(&run_open(&s, &["herdr", "123"], None), 0);
    assert_herdr_focused(&herdr_log);
    assert_exit(&run_open(&s, &["code", "123"], None), 0);
    assert_eq!(opened(&code_log), workspace_file(&s));
}

/// What bash would offer for the last word of `twig <words>` (dynamic completion).
fn complete(s: &Sandbox, words: &[&str]) -> Vec<String> {
    let mut args = vec!["--", "twig"];
    args.extend_from_slice(words);
    let mut c = s.cmd(&args);
    c.env("COMPLETE", "bash")
        .env("_CLAP_COMPLETE_INDEX", words.len().to_string())
        .env("_CLAP_COMPLETE_COMP_TYPE", "9")
        .env("_CLAP_COMPLETE_SPACE", "true")
        .env("_CLAP_IFS", "\n");
    let o = s.run_with(c, "");
    assert_exit(&o, 0);
    // with an empty word bash also gets the flags; the tests look at the values
    stdout(&o)
        .lines()
        .filter(|l| !l.starts_with('-'))
        .map(String::from)
        .collect()
}

#[test]
fn completions_hook_is_small_and_calls_back_into_twig() {
    let s = Sandbox::new();
    let o = s.run(&["completions", "bash"]);
    assert_exit(&o, 0);
    let out = stdout(&o);
    assert!(out.lines().count() < 60, "{out}");
    assert!(
        out.contains("COMPLETE=\"bash\"") && out.contains("complete -o nospace"),
        "{out}"
    );
    assert!(stdout(&s.run(&["completions", "zsh"])).contains("compdef"));
    assert_eq!(stdout(&s.run(&["completions", "fish"])).lines().count(), 1);
}

#[test]
fn completions_list_the_open_targets() {
    let s = Sandbox::new();
    let got = complete(&s, &["open", ""]);
    for t in ["default", "herdr", "code"] {
        assert!(got.iter().any(|g| g == t), "{got:?}");
    }
}

#[test]
fn completions_offer_cards_repos_and_templates() {
    let s = new_sandbox();
    fs::create_dir_all(s.path("wt/52644/a/.git")).unwrap();
    fs::create_dir_all(s.path("wt/52700/a/.git")).unwrap();
    fs::create_dir_all(s.path("wt/memos")).unwrap();
    write_templates(
        &s,
        "[fullstack]\nrepos = [\"a\"]\n[hotfix]\nrepos = [\"b\"]\n",
    );
    assert_eq!(complete(&s, &["rm", "52"]), ["52644", "52700"]);
    assert_eq!(complete(&s, &["status", "526"]), ["52644"]);
    assert_eq!(complete(&s, &["new", "1", "--repos", ""]), ["a", "b"]);
    assert_eq!(complete(&s, &["add-repo", "52644", ""]), ["a", "b"]);
    assert_eq!(
        complete(&s, &["new", "1", "--branch-for", ""]),
        ["a=", "b="]
    );
    assert_eq!(
        complete(&s, &["new", "1", "--template", ""]),
        ["fullstack", "hotfix"]
    );
    assert_eq!(complete(&s, &["template", "show", "h"]), ["hotfix"]);
}

// ───────────────────────── config get / set ─────────────────────────

const COMMENTED: &str = "# my config\nversion = 1\n\n[git]\nbase_branch = \"main\" # trunk\nbranch_prefix = { Bug = \"fix\", default = \"feat\" }\n\n# the repos\n[repos.my-back]\nrun = \"dotnet run\"\n";

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

#[test]
fn config_get_scalar_table_and_missing() {
    let s = Sandbox::new();
    s.write_config(COMMENTED);
    let o = s.run(&["config", "get", "git.base_branch"]);
    assert_exit(&o, 0);
    assert_eq!(stdout(&o), "main\n");
    assert_eq!(stdout(&s.run(&["config", "get", "version"])), "1\n");
    let o = s.run(&["config", "get", "repos.my-back"]);
    assert_exit(&o, 0);
    assert_eq!(stdout(&o), "run = \"dotnet run\"\n");
    let o = s.run(&["config", "get", "git.branch_prefix"]);
    assert_exit(&o, 0);
    assert_has(&o, "Bug = \"fix\"");
    let o = s.run(&["config", "get", "git.nope"]);
    assert_exit(&o, 1);
    assert_has(&o, "key not found: git.nope");
}

#[test]
fn config_set_preserves_comments() {
    let s = Sandbox::new();
    s.write_config(COMMENTED);
    let o = s.run(&["config", "set", "git.base_branch", "develop"]);
    assert_exit(&o, 0);
    assert_eq!(
        fs::read_to_string(&s.config).unwrap(),
        COMMENTED.replace("\"main\" # trunk", "\"develop\" # trunk")
    );
    let o = s.run(&["config", "get", "git.base_branch"]);
    assert_eq!(stdout(&o), "develop\n");
}

#[test]
fn config_set_invalid_value_is_rejected_and_file_untouched() {
    let s = Sandbox::new();
    s.write_config(COMMENTED);
    let o = s.run(&["config", "set", "open.default", "banana"]);
    assert_exit(&o, 1);
    assert_has(&o, "open.default");
    assert_has(&o, "herdr");
    let o2 = s.run(&["config", "set", "git.bogus", "1"]);
    assert_exit(&o2, 1);
    assert_has(&o2, "bogus");
    assert_eq!(fs::read_to_string(&s.config).unwrap(), COMMENTED);
    // no temp file left behind
    assert_eq!(fs::read_dir(s.config.parent().unwrap()).unwrap().count(), 1);
}

#[test]
fn config_set_infers_types_and_arrays() {
    let s = Sandbox::new();
    s.write_config(COMMENTED);
    assert_exit(&s.run(&["config", "set", "update.check", "false"]), 0);
    assert_exit(
        &s.run(&["config", "set", "update.check_interval_hours", "6"]),
        0,
    );
    assert_exit(
        &s.run(&["config", "set", "worktree.local_files", ".env,.env.local"]),
        0,
    );
    assert_exit(
        &s.run(&[
            "config",
            "set",
            "repos.my-back.debug",
            "[Debug: Back, Other]",
        ]),
        0,
    );
    let after = fs::read_to_string(&s.config).unwrap();
    assert!(after.contains("check = false"), "{after}");
    assert!(after.contains("check_interval_hours = 6"), "{after}");
    let o = s.run(&["config", "get", "worktree.local_files"]);
    assert_eq!(stdout(&o), "[\".env\", \".env.local\"]\n");
    let o = s.run(&["config", "get", "repos.my-back.debug"]);
    assert_eq!(stdout(&o), "[\"Debug: Back\", \"Other\"]\n");
    // a lone scalar for an array field becomes a one-element list
    assert_exit(
        &s.run(&["config", "set", "repos.my-back.local_files", ".env"]),
        0,
    );
    let o = s.run(&["config", "get", "repos.my-back.local_files"]);
    assert_eq!(stdout(&o), "[\".env\"]\n");
    // a number for a string field stays a string
    assert_exit(&s.run(&["config", "set", "repos.my-back.short", "42"]), 0);
    let o = s.run(&["config", "get", "repos.my-back.short"]);
    assert_eq!(stdout(&o), "42\n");
}

#[test]
fn config_set_creates_nested_repo_key() {
    let s = Sandbox::new();
    s.write_config(COMMENTED);
    let o = s.run(&["config", "set", "repos.new-front.run", "pnpm dev"]);
    assert_exit(&o, 0);
    let o = s.run(&["config", "get", "repos.new-front.run"]);
    assert_eq!(stdout(&o), "pnpm dev\n");
    let after = fs::read_to_string(&s.config).unwrap();
    assert!(after.starts_with("# my config\n"), "{after}");
    assert!(after.contains("[repos.new-front]"), "{after}");
}

#[test]
fn config_set_can_fix_an_invalid_config() {
    let s = Sandbox::new();
    let bad = "version = 1\n[open]\ndefault = \"nope\"\n";
    s.write_config(bad);
    assert_exit(&s.run(&["config", "get", "open.default"]), 0);
    // unrelated key: the result is still invalid, so it is rejected
    assert_exit(&s.run(&["config", "set", "git.base_branch", "x"]), 1);
    assert_eq!(fs::read_to_string(&s.config).unwrap(), bad);
    // fixing the offending key works
    assert_exit(&s.run(&["config", "set", "open.default", "code"]), 0);
    let o = s.run(&["config", "get", "open.default"]);
    assert_eq!(stdout(&o), "code\n");
}

#[test]
fn completions_list_the_config_subcommands() {
    let s = Sandbox::new();
    let got = complete(&s, &["config", ""]);
    for sub in ["edit", "get", "set"] {
        assert!(got.iter().any(|g| g == sub), "{got:?}");
    }
}

// ───────────────────────── update / uninstall ─────────────────────────

/// `curl` stub: answers the GitHub "latest release" API with `tag` and serves an
/// install.sh that records how it was called.
fn fake_github(s: &Sandbox, tag: &str) -> PathBuf {
    let calls = s.path("install-calls.log");
    let installer = s.script(
        "fake-install.sh",
        &format!("echo \"$@\" >> {}", calls.display()),
    );
    s.stub(
        "curl",
        &format!(
            r#"out=""; url=""
while [ $# -gt 0 ]; do case "$1" in -o) out="$2"; shift 2;; -H) shift 2;; -*) shift;; *) url="$1"; shift;; esac; done
case "$url" in
  *releases/latest) echo '{{"tag_name": "{tag}"}}' ;;
  */install.sh) {cat} {installer} > "$out" ;;
  *) exit 22 ;;
esac"#,
            cat = real("cat").display(),
            installer = installer.display(),
        ),
    );
    calls
}

#[test]
fn update_check_reports_new_version() {
    let s = Sandbox::new();
    fake_github(&s, "v99.0.0");
    let o = s.run(&["update", "--check"]);
    assert_exit(&o, 0);
    assert_has(&o, "new version available: v99.0.0");
}

#[test]
fn update_on_latest_version_does_nothing() {
    let s = Sandbox::new();
    let calls = fake_github(&s, &format!("v{}", env!("CARGO_PKG_VERSION")));
    let o = s.run(&["update"]);
    assert_exit(&o, 0);
    assert_has(&o, "already on the latest version");
    assert!(!calls.exists());
}

#[test]
fn update_runs_the_tag_installer() {
    let s = Sandbox::new();
    let calls = fake_github(&s, "v99.0.0");
    let o = s.run(&["update"]);
    assert_exit(&o, 0);
    assert_eq!(
        fs::read_to_string(&calls).unwrap().trim(),
        "--update --version v99.0.0"
    );
}

#[test]
fn uninstall_removes_twigs_pieces() {
    let s = Sandbox::new();
    let herdr_log = s.path("herdr.log");
    s.stub("herdr", &format!("echo \"$@\" >> {}", herdr_log.display()));
    s.valid_config("");

    // what install.sh leaves behind
    let bin = s.bin.join("twig");
    fs::copy(env!("CARGO_BIN_EXE_twig"), &bin).unwrap();
    let skill = s.path(".claude/skills/twig");
    fs::create_dir_all(&skill).unwrap();
    fs::write(skill.join("SKILL.md"), "---\nname: twig\n---\n").unwrap();
    let other_skill = s.path(".claude/skills/other");
    fs::create_dir_all(&other_skill).unwrap();
    let plugin = s.path(".local/share/twig/herdr-plugins/twig");
    fs::create_dir_all(&plugin).unwrap();
    let completion = s.path(".local/share/bash-completion/completions/twig");
    fs::create_dir_all(completion.parent().unwrap()).unwrap();
    fs::write(&completion, "complete").unwrap();
    let herdr_cfg = s.path(".config/herdr/config.toml");
    fs::create_dir_all(herdr_cfg.parent().unwrap()).unwrap();
    fs::write(
        &herdr_cfg,
        "[ui]\nx = 1\n\n# twig:begin (managed)\n[[keys.command]]\nkey = \"prefix+shift+o\"\n# twig:end\n",
    )
    .unwrap();
    let bashrc = s.path(".bashrc");
    fs::write(
        &bashrc,
        "alias ll='ls -l'\nexport PATH=\"/x/bin:$PATH\"  # twig\ncommand -v twig >/dev/null 2>&1 && source <(COMPLETE=bash twig)  # twig completions\n",
    )
    .unwrap();

    let mut c = Command::new(&bin);
    c.args(["uninstall", "--yes"])
        .env_clear()
        .env("PATH", &s.bin)
        .env("HOME", &s.home)
        .env("TWIG_CONFIG", &s.config);
    let o = s.run_with(c, "");
    assert_exit(&o, 0);

    assert!(!bin.exists(), "binary is still there");
    assert!(!skill.exists() && other_skill.exists());
    assert!(!s.path(".local/share/twig").exists());
    assert!(!completion.exists());
    assert_eq!(fs::read_to_string(&bashrc).unwrap(), "alias ll='ls -l'\n");
    assert_eq!(fs::read_to_string(&herdr_cfg).unwrap(), "[ui]\nx = 1\n");
    assert!(s.config.exists(), "--yes keeps the config");
    let herdr_calls = fs::read_to_string(&herdr_log).unwrap();
    assert!(herdr_calls.contains("plugin unlink twig"), "{herdr_calls}");
}

// ───────────────────────── rm ─────────────────────────

/// Sandbox with card `123` (a real worktree of `r1`) and a fake `herdr` that logs its calls.
/// `agent_status` is what its single pane reports.
fn rm_sandbox(agent_status: &str) -> (Sandbox, PathBuf) {
    let s = Sandbox::new();
    s.git_identity();
    s.valid_config("");
    let main = s.path("repos/r1");
    let git = |args: &[&str], dir: &Path| {
        assert!(Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@t")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@t")
            .status()
            .unwrap()
            .success());
    };
    git(&["commit", "-q", "--allow-empty", "-m", "init"], &main);
    fs::create_dir_all(s.path("wt/123")).unwrap();
    git(
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "card-123",
            &s.path("wt/123/r1").to_string_lossy(),
        ],
        &main,
    );
    let log = s.path("herdr.log");
    let body = format!(
        r#"echo "$@" >> {log}
case "$1 $2" in
  "workspace list") echo '{{"result":{{"workspaces":[{{"label":"123","workspace_id":"w1"}}]}}}}' ;;
  "pane list") echo '{{"result":{{"panes":[{{"pane_id":"w1:p1","agent_status":"{agent_status}"}}]}}}}' ;;
esac"#,
        log = log.display()
    );
    s.stub("herdr", &body);
    (s, log)
}

fn herdr_calls(log: &Path) -> String {
    fs::read_to_string(log).unwrap_or_default()
}

#[test]
fn rm_removes_card_and_closes_herdr_workspace() {
    let (s, log) = rm_sandbox("idle");
    let o = s.run(&["rm", "123"]);
    assert_exit(&o, 0);
    assert!(!s.path("wt/123").exists());
    assert!(
        herdr_calls(&log).contains("workspace close w1"),
        "{}",
        herdr_calls(&log)
    );
}

#[test]
fn rm_blocks_when_agent_is_running() {
    let (s, log) = rm_sandbox("working");
    let o = s.run(&["rm", "123"]);
    assert_exit(&o, 1);
    assert_has(&o, "an agent is running");
    assert!(s.path("wt/123/r1").exists());
    assert!(!herdr_calls(&log).contains("workspace close"));
}

#[test]
fn rm_force_ignores_running_agent_and_dirty_worktree() {
    let (s, log) = rm_sandbox("working");
    fs::write(s.path("wt/123/r1/dirty.txt"), "x").unwrap();
    let blocked = s.run(&["rm", "123"]);
    assert_exit(&blocked, 1);
    assert!(s.path("wt/123").exists());

    let o = s.run(&["rm", "-f", "123"]);
    assert_exit(&o, 0);
    assert!(!s.path("wt/123").exists());
    assert!(herdr_calls(&log).contains("workspace close w1"));
}

#[test]
fn rm_works_without_herdr() {
    let (s, _log) = rm_sandbox("idle");
    fs::remove_file(s.bin.join("herdr")).unwrap();
    let o = s.run(&["rm", "123"]);
    assert_exit(&o, 0);
    assert!(!s.path("wt/123").exists());
}

#[test]
fn park_closes_the_workspace_and_keeps_the_card() {
    let (s, log) = rm_sandbox("idle");
    let o = s.run(&["park", "123"]);
    assert_exit(&o, 0);
    assert_has(&o, "close the card's window by hand");
    assert!(s.path("wt/123/r1").exists());
    assert!(herdr_calls(&log).contains("workspace close w1"));
}

#[test]
fn park_blocks_on_a_running_agent_unless_forced() {
    let (s, log) = rm_sandbox("working");
    let o = s.run(&["park", "123"]);
    assert_exit(&o, 1);
    assert_has(&o, "an agent is running");
    assert!(!herdr_calls(&log).contains("workspace close"));

    assert_exit(&s.run(&["park", "-f", "123"]), 0);
    assert!(herdr_calls(&log).contains("workspace close w1"));
    assert!(s.path("wt/123/r1").exists());
}

#[test]
fn park_unknown_card_fails() {
    let (s, _log) = rm_sandbox("idle");
    assert_exit(&s.run(&["park", "999"]), 1);
}

// ───────────────────────── open-card ─────────────────────────

const CARD_URL: &str = "https://dev.azure.com/o/p/_workitems/edit/123";

/// Card `123` with a cached ADO work item and a fake browser opener (`xdg-open` and `open`) that logs its argument.
fn open_card_sandbox(extra: &str) -> (Sandbox, PathBuf) {
    let s = Sandbox::new();
    s.valid_config(extra);
    fs::create_dir_all(s.path("wt/123/r1/.git")).unwrap();
    fs::write(
        s.path("wt/123/.ado.json"),
        format!(
            r#"{{"fields":{{"System.Title":"T"}},"_links":{{"html":{{"href":"{CARD_URL}"}}}}}}"#
        ),
    )
    .unwrap();
    let log = s.path("opener.log");
    for name in ["xdg-open", "open"] {
        s.stub(name, &format!("echo \"$1\" > {}", log.display()));
    }
    (s, log)
}

#[test]
fn open_card_opens_the_cached_url() {
    let (s, log) = open_card_sandbox("");
    assert_exit(&s.run(&["open-card", "123"]), 0);
    assert_eq!(opened(&log), CARD_URL);
}

#[test]
fn opcd_alias_infers_the_current_card() {
    let (s, log) = open_card_sandbox("");
    let mut c = s.cmd(&["opcd"]);
    c.current_dir(s.path("wt/123/r1"));
    assert_exit(&s.run_with(c, ""), 0);
    assert_eq!(opened(&log), CARD_URL);
}

#[test]
fn open_card_without_id_outside_a_card_is_a_usage_error() {
    let (s, log) = open_card_sandbox("");
    let o = s.run(&["open-card"]);
    assert_exit(&o, 1);
    assert_has(&o, "usage: twig open-card <id>");
    assert_eq!(opened(&log), "");
}

#[test]
fn open_card_prints_the_url_without_an_opener() {
    let (s, _log) = open_card_sandbox("");
    fs::remove_file(s.bin.join("xdg-open")).unwrap();
    fs::remove_file(s.bin.join("open")).unwrap();
    let o = s.run(&["open-card", "123"]);
    assert_exit(&o, 0);
    assert_has(&o, CARD_URL);
}

#[test]
fn open_card_with_gh_uses_the_issue_url_of_a_repo_scoped_card() {
    let (s, log) = open_card_sandbox("[provider]\ndefault = \"gh\"\n");
    fs::create_dir_all(s.path("wt/web-9/r1/.git")).unwrap();
    fs::write(s.path("wt/web-9/.twig-issue"), "acme/web#9\n").unwrap();
    // no cache and no `gh`: the URL is derived from owner/repo#number
    assert_exit(&s.run(&["open-card", "acme/web#9"]), 0);
    assert_eq!(opened(&log), "https://github.com/acme/web/issues/9");
}

#[test]
fn open_card_with_gh_opens_the_cached_issue_url() {
    let (s, log) = open_card_sandbox("[provider]\ndefault = \"gh\"\n");
    fs::write(
        s.path("wt/123/.gh.json"),
        r#"{"title":"T","url":"https://github.com/acme/web/issues/123"}"#,
    )
    .unwrap();
    assert_exit(&s.run(&["open-card", "123"]), 0);
    assert_eq!(opened(&log), "https://github.com/acme/web/issues/123");
}

// ───────────────────────── status ─────────────────────────

fn git_quiet(dir: &Path, args: &[&str]) {
    assert!(
        Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@t")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@t")
            .status()
            .unwrap()
            .success(),
        "git {args:?} failed"
    );
}

/// Card `123` with worktree `r1` on `card-123`: 2 commits ahead of `origin/main`, 1 ahead of
/// `origin/card-123` (pushed one commit earlier), 1 uncommitted file. Fake herdr like `rm_sandbox`.
fn status_sandbox(agent_status: &str) -> Sandbox {
    let (s, _log) = rm_sandbox(agent_status);
    let main = s.path("repos/r1");
    let wt = s.path("wt/123/r1");
    git_quiet(&main, &["branch", "-M", "main"]);
    let remote = s.path("remote.git");
    fs::create_dir_all(&remote).unwrap();
    git_quiet(&remote, &["init", "-q", "--bare"]);
    git_quiet(
        &main,
        &["remote", "add", "origin", &remote.to_string_lossy()],
    );
    git_quiet(&main, &["push", "-q", "origin", "main"]);
    git_quiet(&wt, &["commit", "-q", "--allow-empty", "-m", "one"]);
    git_quiet(&wt, &["push", "-q", "-u", "origin", "card-123"]);
    git_quiet(&wt, &["commit", "-q", "--allow-empty", "-m", "two"]);
    fs::write(wt.join("dirty.txt"), "x").unwrap();
    s
}

#[test]
fn status_shows_branch_dirty_ahead_behind_and_herdr() {
    let s = status_sandbox("working");
    let o = s.run(&["status", "123"]);
    assert_exit(&o, 0);
    assert_has(&o, "card-123");
    assert_has(&o, "*1");
    assert_has(&o, "base ↑2 ↓0");
    assert_has(&o, "origin ↑1 ↓0");
    assert_has(&o, "workspace, 1 agent(s) running");
}

#[test]
fn status_without_id_lists_every_card_compact() {
    let s = status_sandbox("idle");
    let o = s.run(&["status"]);
    assert_exit(&o, 0);
    assert_has(&o, "#123");
    assert_has(&o, "workspace, no agent running");
    assert!(!text(&o).contains("uncommitted files:"), "{}", text(&o));
}

#[test]
fn status_without_herdr_reports_no_workspace() {
    let s = status_sandbox("idle");
    fs::remove_file(s.bin.join("herdr")).unwrap();
    let o = s.run(&["status", "123"]);
    assert_exit(&o, 0);
    assert_has(&o, "no workspace");
    assert_has(&o, "uncommitted files: 1");
}

#[test]
fn status_unknown_card_is_an_error() {
    let s = Sandbox::new();
    s.valid_config("");
    let o = s.run(&["status", "999"]);
    assert_exit(&o, 1);
}

// ───────────────────── new / add-repo / prune (local git remotes) ─────────────────────

fn git_in(dir: &Path, args: &[&str]) -> String {
    let o = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .output()
        .unwrap();
    assert!(
        o.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

/// Bare `origin/<name>.git` with a `main` branch, cloned to `repos/<name>`.
fn remote_repo(s: &Sandbox, name: &str) -> PathBuf {
    let origin = s.path(&format!("origin/{name}.git"));
    fs::create_dir_all(&origin).unwrap();
    git_in(&origin, &["init", "-q", "--bare", "-b", "main"]);
    let clone = s.path(&format!("repos/{name}"));
    fs::create_dir_all(&clone).unwrap();
    git_in(&clone, &["init", "-q", "-b", "main"]);
    git_in(
        &clone,
        &["remote", "add", "origin", &origin.to_string_lossy()],
    );
    git_in(&clone, &["commit", "-q", "--allow-empty", "-m", "init"]);
    git_in(&clone, &["push", "-q", "origin", "main"]);
    origin
}

/// Sandbox with repos `a` (default) and `b` (not default), each with a local bare origin.
fn new_sandbox() -> Sandbox {
    let s = Sandbox::new();
    s.all_tools();
    remote_repo(&s, "a");
    remote_repo(&s, "b");
    s.write_config(&format!(
        "version = 1\n\n[paths]\nworktrees = \"{wt}\"\nrepos = \"{repos}\"\n\n[repos.a]\ndefault = true\n\n[repos.b]\n",
        wt = s.path("wt").display(),
        repos = s.path("repos").display(),
    ));
    s
}

fn branch_of(wt: PathBuf) -> String {
    git_in(&wt, &["rev-parse", "--abbrev-ref", "HEAD"])
}

#[test]
fn new_creates_worktree_branch_from_base_and_context_file() {
    let s = new_sandbox();
    let o = s.run(&["new", "7", "my-slug", "--no-ado"]);
    assert_exit(&o, 0);
    let wt = s.path("wt/7/a");
    assert!(wt.join(".git").exists());
    assert_eq!(branch_of(wt.clone()), "feat/7-my-slug");
    // branched from origin/main
    assert_eq!(
        git_in(&wt, &["rev-parse", "HEAD"]),
        git_in(&wt, &["rev-parse", "origin/main"])
    );
    assert!(!s.path("wt/7/b").exists(), "only default repos");
    let ctx = fs::read_to_string(s.path("wt/7/CLAUDE.md")).unwrap();
    assert!(ctx.contains("Card #7"), "{ctx}");
    assert!(ctx.contains("feat/7-my-slug"), "{ctx}");
}

/// Fake `gh` that logs its arguments and answers `issue view` with issue 12 of acme/tracker.
fn gh_issue_stub(s: &Sandbox) -> PathBuf {
    let log = s.path("gh.log");
    s.stub(
        "gh",
        &format!(
            r#"echo "$@" >> {log}
case "$1 $2" in
  "issue view") echo '{{"number":12,"title":"Login breaks on Safari","url":"https://github.com/acme/tracker/issues/12","body":"steps","state":"OPEN","labels":[{{"name":"bug"}}],"assignees":[]}}' ;;
esac"#,
            log = log.display()
        ),
    );
    log
}

#[test]
fn new_with_a_repo_scoped_gh_id() {
    let s = new_sandbox();
    append_config(&s, "\n[provider]\ndefault = \"gh\"\n");
    let log = gh_issue_stub(&s);
    let o = s.run(&["new", "acme/tracker#12"]);
    assert_exit(&o, 0);
    let gh = fs::read_to_string(&log).unwrap();
    assert!(
        gh.contains("issue view 12") && gh.contains("--repo acme/tracker"),
        "{gh}"
    );
    let dir = s.path("wt/tracker-12");
    assert_eq!(
        fs::read_to_string(dir.join(".twig-issue")).unwrap().trim(),
        "acme/tracker#12"
    );
    // the branch uses the issue number and the title; the bug label picks no special prefix
    assert_eq!(branch_of(dir.join("a")), "feat/12-login-breaks-on-safari");
    let ctx = fs::read_to_string(dir.join("CLAUDE.md")).unwrap();
    assert!(
        ctx.contains("# Card acme/tracker#12 — Login breaks on Safari"),
        "{ctx}"
    );
    assert!(
        ctx.contains("gh issue view 12 --repo acme/tracker"),
        "{ctx}"
    );

    // the same id names the existing card
    let st = s.run(&["status", "acme/tracker#12", "--json"]);
    assert_exit(&st, 0);
    assert_eq!(json_of(&st)["cards"][0]["id"], "tracker-12");
    assert_exit(&s.run(&["rm", "acme/tracker#12"]), 0);
    assert!(!dir.exists());
}

#[test]
fn repo_scoped_ids_are_rejected_for_azure_devops() {
    let s = new_sandbox();
    let o = s.run(&["new", "acme/tracker#12", "--no-ado"]);
    assert_exit(&o, 1);
    assert_has(&o, "only for provider \"gh\"");
    assert!(!s.path("wt/tracker-12").exists());
}

#[test]
fn new_uses_the_base_and_fix_prefix_flags() {
    let s = new_sandbox();
    let a = s.path("repos/a");
    git_in(&a, &["checkout", "-q", "-b", "dev"]);
    git_in(&a, &["commit", "-q", "--allow-empty", "-m", "on dev"]);
    git_in(&a, &["push", "-q", "origin", "dev"]);
    git_in(&a, &["checkout", "-q", "main"]);
    let o = s.run(&["new", "8", "x", "--no-ado", "--base", "dev", "--fix"]);
    assert_exit(&o, 0);
    let wt = s.path("wt/8/a");
    assert_eq!(branch_of(wt.clone()), "fix/8-x");
    assert_eq!(
        git_in(&wt, &["rev-parse", "HEAD"]),
        git_in(&wt, &["rev-parse", "origin/dev"])
    );
}

#[test]
fn new_prefers_an_origin_branch_with_the_id() {
    let s = new_sandbox();
    let a = s.path("repos/a");
    git_in(&a, &["checkout", "-q", "-b", "feature/9-existing"]);
    git_in(&a, &["commit", "-q", "--allow-empty", "-m", "remote work"]);
    git_in(&a, &["push", "-q", "origin", "feature/9-existing"]);
    git_in(&a, &["checkout", "-q", "main"]);
    git_in(&a, &["branch", "-q", "-D", "feature/9-existing"]);
    let o = s.run(&["new", "9", "ignored", "--no-ado"]);
    assert_exit(&o, 0);
    let wt = s.path("wt/9/a");
    assert_eq!(branch_of(wt.clone()), "feature/9-existing");
    assert_eq!(
        git_in(&wt, &["rev-parse", "--abbrev-ref", "@{upstream}"]),
        "origin/feature/9-existing"
    );
}

#[test]
fn new_branch_flag_forces_the_branch() {
    let s = new_sandbox();
    let o = s.run(&["new", "10", "--no-ado", "--branch", "custom/thing"]);
    assert_exit(&o, 0);
    assert_eq!(branch_of(s.path("wt/10/a")), "custom/thing");
}

#[test]
fn new_branch_for_mixes_with_branch() {
    let s = new_sandbox();
    let o = s.run(&[
        "new",
        "11",
        "--no-ado",
        "--repos",
        "a,b",
        "--branch",
        "all/11",
        "--branch-for",
        "a=feature/11-api",
    ]);
    assert_exit(&o, 0);
    assert_eq!(branch_of(s.path("wt/11/a")), "feature/11-api");
    assert_eq!(branch_of(s.path("wt/11/b")), "all/11");
}

#[test]
fn new_branch_for_without_branch_falls_back_to_the_normal_resolution() {
    let s = new_sandbox();
    let o = s.run(&[
        "new",
        "12",
        "s",
        "--no-ado",
        "--repos",
        "a,b",
        "--branch-for",
        "b=feature/12-tela",
    ]);
    assert_exit(&o, 0);
    assert_eq!(branch_of(s.path("wt/12/a")), "feat/12-s");
    assert_eq!(branch_of(s.path("wt/12/b")), "feature/12-tela");
}

#[test]
fn branch_for_rejects_a_repo_outside_the_card_and_a_bad_format() {
    let s = new_sandbox();
    let o = s.run(&["new", "13", "--no-ado", "--branch-for", "b=x/13"]);
    assert_exit(&o, 1);
    assert_has(&o, "not among the card's repos");
    assert!(!s.path("wt/13").exists());
    let o = s.run(&["new", "13", "--no-ado", "--branch-for", "nobranch"]);
    assert_exit(&o, 2);
    assert_has(&o, "<repo>=<branch>");
}

#[test]
fn add_repo_branch_for_wins_over_branch() {
    let s = new_sandbox();
    assert_exit(&s.run(&["new", "14", "s", "--no-ado"]), 0);
    let o = s.run(&[
        "add-repo",
        "14",
        "b",
        "--branch",
        "x/14",
        "--branch-for",
        "b=y/14",
    ]);
    assert_exit(&o, 0);
    assert_eq!(branch_of(s.path("wt/14/b")), "y/14");
    let o = s.run(&["add-repo", "14", "b", "--branch-for", "a=z"]);
    assert_exit(&o, 1);
    assert_has(&o, "not among the card's repos");
}

#[test]
fn new_refuses_an_existing_card() {
    let s = new_sandbox();
    assert_exit(&s.run(&["new", "7", "s", "--no-ado"]), 0);
    let o = s.run(&["new", "7", "s", "--no-ado"]);
    assert_exit(&o, 1);
    assert_has(&o, "already exists");
}

#[test]
fn add_repo_adds_a_repo_to_the_card() {
    let s = new_sandbox();
    assert_exit(&s.run(&["new", "7", "s", "--no-ado"]), 0);
    let o = s.run(&["add-repo", "7", "b", "--branch", "feat/7-b"]);
    assert_exit(&o, 0);
    assert!(s.path("wt/7/b/.git").exists());
    assert_eq!(branch_of(s.path("wt/7/b")), "feat/7-b");
    let ctx = fs::read_to_string(s.path("wt/7/CLAUDE.md")).unwrap();
    assert!(ctx.contains("`b`"), "{ctx}");
    // safe to re-run
    let again = s.run(&["add-repo", "7", "b"]);
    assert_exit(&again, 0);
    assert_has(&again, "already in the card");
}

#[test]
fn add_repo_to_a_missing_card_fails() {
    let s = new_sandbox();
    let o = s.run(&["add-repo", "404", "b"]);
    assert_exit(&o, 1);
}

#[test]
fn rm_refuses_a_dirty_worktree_and_force_removes_it() {
    let s = new_sandbox();
    assert_exit(&s.run(&["new", "7", "s", "--no-ado"]), 0);
    fs::write(s.path("wt/7/a/dirty.txt"), "x").unwrap();
    let o = s.run(&["rm", "7"]);
    assert_exit(&o, 1);
    assert_has(&o, "uncommitted changes");
    assert!(s.path("wt/7/a").exists());
    let o = s.run(&["rm", "--force", "--branches", "7"]);
    assert_exit(&o, 0);
    assert!(!s.path("wt/7").exists());
    let branches = git_in(&s.path("repos/a"), &["branch", "--list", "feat/7-s"]);
    assert!(branches.is_empty(), "branch should be deleted: {branches}");
}

#[test]
fn rm_keeps_branches_by_default() {
    let s = new_sandbox();
    assert_exit(&s.run(&["new", "7", "s", "--no-ado"]), 0);
    assert_exit(&s.run(&["rm", "7"]), 0);
    assert!(!s.path("wt/7").exists());
    let branches = git_in(&s.path("repos/a"), &["branch", "--list", "feat/7-s"]);
    assert!(branches.contains("feat/7-s"), "{branches}");
}

#[test]
fn prune_cleans_orphaned_worktree_records() {
    let s = new_sandbox();
    assert_exit(&s.run(&["new", "7", "s", "--no-ado"]), 0);
    // the worktree folder disappears behind git's back
    fs::remove_dir_all(s.path("wt/7/a")).unwrap();
    let main = s.path("repos/a");
    assert!(git_in(&main, &["worktree", "list", "--porcelain"]).contains("wt/7/a"));

    let dry = s.run(&["prune", "--dry-run"]);
    assert_exit(&dry, 0);
    assert!(git_in(&main, &["worktree", "list", "--porcelain"]).contains("wt/7/a"));

    let o = s.run_with(s.cmd(&["prune"]), "n\n");
    assert_exit(&o, 0);
    assert!(!git_in(&main, &["worktree", "list", "--porcelain"]).contains("wt/7/a"));
    // the now-empty card folder was only listed; answering "n" keeps it
    assert!(s.path("wt/7").exists());
}

#[test]
fn prune_removes_empty_card_folders_when_confirmed() {
    let s = new_sandbox();
    fs::create_dir_all(s.path("wt/55")).unwrap();
    let o = s.run_with(s.cmd(&["prune"]), "y\n");
    assert_exit(&o, 0);
    assert!(!s.path("wt/55").exists());
}

// ───────────────────────── sync ─────────────────────────

/// Commit `file` = `content` on `dir`'s current branch.
fn commit_file(dir: &Path, file: &str, content: &str, msg: &str) {
    fs::write(dir.join(file), content).unwrap();
    git_in(dir, &["add", file]);
    git_in(dir, &["commit", "-q", "-m", msg]);
}

/// Card `123` with a worktree of `r1` on `card-123` (one own commit, `mine.txt`), cloned from
/// a bare origin whose `main` has one commit (`base.txt`). `config_extra` goes in the config.
fn sync_sandbox(config_extra: &str) -> (Sandbox, PathBuf) {
    let s = Sandbox::new();
    s.git_identity();
    s.valid_config(config_extra);
    let origin = s.path("origin.git");
    fs::create_dir_all(&origin).unwrap();
    git_in(&origin, &["init", "-q", "--bare", "-b", "main"]);
    // valid_config already ran `git init` in repos/r1; replace it with a clone of origin
    let main = s.path("repos/r1");
    fs::remove_dir_all(&main).unwrap();
    git_in(
        &s.home,
        &[
            "clone",
            "-q",
            &origin.to_string_lossy(),
            &main.to_string_lossy(),
        ],
    );
    git_in(&main, &["checkout", "-q", "-B", "main"]);
    commit_file(&main, "base.txt", "base\n", "base");
    git_in(&main, &["push", "-q", "origin", "main"]);
    fs::create_dir_all(s.path("wt/123")).unwrap();
    let wt = s.path("wt/123/r1");
    git_in(
        &main,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "card-123",
            &wt.to_string_lossy(),
        ],
    );
    commit_file(&wt, "mine.txt", "mine\n", "card work");
    (s, wt)
}

/// Push a new commit to origin's main from a throwaway clone.
fn advance_base(s: &Sandbox, file: &str, content: &str) {
    let other = s.path("other");
    if !other.exists() {
        git_in(
            &s.home,
            &[
                "clone",
                "-q",
                &s.path("origin.git").to_string_lossy(),
                &other.to_string_lossy(),
            ],
        );
    }
    commit_file(&other, file, content, "advance base");
    git_in(&other, &["push", "-q", "origin", "main"]);
}

fn log_subjects(dir: &Path) -> String {
    git_in(dir, &["log", "--format=%s"])
}

fn has_merges(dir: &Path) -> bool {
    !git_in(dir, &["rev-list", "--merges", "HEAD"])
        .trim()
        .is_empty()
}

#[test]
fn sync_rebases_the_card_branch_onto_the_base() {
    let (s, wt) = sync_sandbox("");
    advance_base(&s, "new.txt", "new\n");
    let o = s.run(&["sync", "123"]);
    assert_exit(&o, 0);
    assert!(wt.join("new.txt").exists(), "{}", text(&o));
    assert_eq!(
        log_subjects(&wt).lines().collect::<Vec<_>>(),
        ["card work", "advance base", "base"]
    );
    assert!(!has_merges(&wt));
}

#[test]
fn sync_merge_flag_creates_a_merge_commit() {
    let (s, wt) = sync_sandbox("");
    advance_base(&s, "new.txt", "new\n");
    let o = s.run(&["sync", "123", "--merge"]);
    assert_exit(&o, 0);
    assert!(wt.join("new.txt").exists());
    assert!(has_merges(&wt));
}

#[test]
fn sync_uses_the_configured_strategy_and_rebase_overrides_it() {
    let (s, wt) = sync_sandbox("[git]\nsync = \"merge\"\n");
    advance_base(&s, "new.txt", "new\n");
    assert_exit(&s.run(&["sync", "123"]), 0);
    assert!(has_merges(&wt));

    let (s, wt) = sync_sandbox("[git]\nsync = \"merge\"\n");
    advance_base(&s, "new.txt", "new\n");
    assert_exit(&s.run(&["sync", "123", "--rebase"]), 0);
    assert!(!has_merges(&wt));
}

#[test]
fn sync_rejects_an_invalid_strategy_in_the_config() {
    let (s, _wt) = sync_sandbox("[git]\nsync = \"squash\"\n");
    let o = s.run(&["sync", "123"]);
    assert_exit(&o, 1);
    assert_has(&o, "squash");
}

#[test]
fn sync_merge_and_rebase_are_mutually_exclusive() {
    let (s, _wt) = sync_sandbox("");
    let o = s.run(&["sync", "123", "--merge", "--rebase"]);
    assert_exit(&o, 2);
}

#[test]
fn sync_dry_run_changes_nothing() {
    let (s, wt) = sync_sandbox("");
    advance_base(&s, "new.txt", "new\n");
    let head = git_in(&wt, &["rev-parse", "HEAD"]);
    let o = s.run(&["sync", "123", "--dry-run"]);
    assert_exit(&o, 0);
    assert_has(&o, "would rebase 1 commit(s)");
    assert_eq!(git_in(&wt, &["rev-parse", "HEAD"]), head);
    assert!(!wt.join("new.txt").exists());
}

#[test]
fn sync_reports_when_already_up_to_date() {
    let (s, _wt) = sync_sandbox("");
    let o = s.run(&["sync", "123"]);
    assert_exit(&o, 0);
    assert_has(&o, "up to date");
}

#[test]
fn sync_skips_repos_with_uncommitted_changes() {
    let (s, wt) = sync_sandbox("");
    advance_base(&s, "new.txt", "new\n");
    fs::write(wt.join("mine.txt"), "edited\n").unwrap();
    let o = s.run(&["sync", "123"]);
    assert_exit(&o, 0);
    assert_has(&o, "r1: uncommitted changes, skipped");
    assert!(!wt.join("new.txt").exists());
    assert_eq!(fs::read_to_string(wt.join("mine.txt")).unwrap(), "edited\n");
}

#[test]
fn sync_stops_on_conflict_naming_repo_and_files() {
    let (s, wt) = sync_sandbox("");
    // the card edits base.txt too, and so does the base
    commit_file(&wt, "base.txt", "card side\n", "card edits base");
    advance_base(&s, "base.txt", "base side\n");
    let o = s.run(&["sync", "123"]);
    assert_exit(&o, 1);
    assert_has(&o, "r1: conflict in rebase");
    assert_has(&o, "base.txt");
    assert_has(&o, "--abort");
}

#[test]
fn sync_unknown_card_fails() {
    let (s, _wt) = sync_sandbox("");
    let o = s.run(&["sync", "999"]);
    assert_exit(&o, 1);
    assert_has(&o, "card 999 not found");
}

// ───────────────────────── --json ─────────────────────────

fn json_of(o: &Output) -> serde_json::Value {
    let out = String::from_utf8_lossy(&o.stdout);
    assert!(!out.contains('\x1b'), "colors in JSON output:\n{out}");
    serde_json::from_str(&out).unwrap_or_else(|e| panic!("stdout is not JSON ({e}):\n{out}"))
}

#[test]
fn ls_json_lists_cards_repos_and_dirty() {
    let s = status_sandbox("idle");
    let o = s.run(&["ls", "--json"]);
    assert_exit(&o, 0);
    let v = json_of(&o);
    assert_eq!(v["cards"].as_array().unwrap().len(), 1);
    assert_eq!(v["cards"][0]["id"], "123");
    assert_eq!(v["cards"][0]["repos"][0]["repo"], "r1");
    assert_eq!(v["cards"][0]["repos"][0]["branch"], "card-123");
    assert_eq!(v["cards"][0]["repos"][0]["dirty"], true);
}

#[test]
fn ls_json_without_cards_is_an_empty_list() {
    let s = Sandbox::new();
    s.valid_config("");
    let o = s.run(&["ls", "--json"]);
    assert_exit(&o, 0);
    assert_eq!(json_of(&o)["cards"].as_array().unwrap().len(), 0);
}

#[test]
fn status_json_for_one_card() {
    let s = status_sandbox("working");
    let o = s.run(&["status", "123", "--json"]);
    assert_exit(&o, 0);
    let v = json_of(&o);
    let c = &v["cards"][0];
    assert_eq!(c["id"], "123");
    assert_eq!(c["herdr"]["workspace"], true);
    assert_eq!(c["herdr"]["agents_running"], 1);
    let r = &c["repos"][0];
    assert_eq!(r["repo"], "r1");
    assert_eq!(r["branch"], "card-123");
    assert_eq!(r["dirty"], 1);
    assert_eq!(r["base_ref"], "origin/main");
    assert_eq!(r["vs_base"]["ahead"], 2);
    assert_eq!(r["vs_base"]["behind"], 0);
    assert_eq!(r["origin_ref"], "origin/card-123");
    assert_eq!(r["vs_origin"]["ahead"], 1);
}

#[test]
fn status_json_without_id_lists_every_card_and_without_herdr() {
    let s = status_sandbox("idle");
    fs::remove_file(s.bin.join("herdr")).unwrap();
    let o = s.run(&["status", "--json"]);
    assert_exit(&o, 0);
    let v = json_of(&o);
    assert_eq!(v["cards"].as_array().unwrap().len(), 1);
    assert_eq!(v["cards"][0]["herdr"]["workspace"], false);
    assert_eq!(v["cards"][0]["herdr"]["agents_running"], 0);
}

#[test]
fn status_json_unknown_card_fails_with_empty_stdout() {
    let s = Sandbox::new();
    s.valid_config("");
    let o = s.run(&["status", "999", "--json"]);
    assert_exit(&o, 1);
    assert!(o.stdout.is_empty(), "{}", text(&o));
}

#[test]
fn doctor_json_all_good() {
    let s = Sandbox::new();
    s.all_tools().git_identity();
    s.valid_config("");
    let o = s.run(&["doctor", "--json"]);
    assert_exit(&o, 0);
    let v = json_of(&o);
    assert_eq!(v["ok"], true);
    assert_eq!(v["errors"], 0);
    let sections = v["sections"].as_array().unwrap();
    assert_eq!(sections[0]["name"], "config");
    assert_eq!(sections[0]["checks"][0]["level"], "ok");
    assert!(sections
        .iter()
        .flat_map(|s| s["checks"].as_array().unwrap())
        .all(|c| c["level"] != "error"));
}

#[test]
fn doctor_json_reports_errors_and_still_exits_1() {
    let s = Sandbox::new();
    s.stub("az", "exit 0")
        .stub("claude", "exit 0")
        .stub("code", "exit 0")
        .git_identity();
    s.valid_config("");
    let o = s.run(&["doctor", "--json"]);
    assert_exit(&o, 1);
    let v = json_of(&o);
    assert_eq!(v["ok"], false);
    assert_eq!(v["errors"], 1);
    let herdr = v["sections"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == "herdr")
        .unwrap();
    assert_eq!(herdr["checks"][0]["level"], "error");
    assert!(herdr["checks"][0]["message"]
        .as_str()
        .unwrap()
        .contains("herdr not found"));
}

#[test]
fn doctor_json_with_invalid_config_is_an_error_entry() {
    let s = Sandbox::new();
    s.all_tools().git_identity();
    s.write_config("version = 1\n[vscode]\nactive = true\n");
    let o = s.run(&["doctor", "--json"]);
    assert_exit(&o, 1);
    let v = json_of(&o);
    assert_eq!(v["ok"], false);
    assert_eq!(v["sections"][0]["checks"][0]["level"], "error");
}

// ───────────────────────── memo ─────────────────────────

fn memo_sandbox() -> Sandbox {
    let s = new_sandbox();
    assert_exit(&s.run(&["new", "7", "slug", "--no-ado"]), 0);
    s
}

fn memo_in(s: &Sandbox, dir: &Path, args: &[&str]) -> Output {
    let mut c = s.cmd(args);
    c.current_dir(dir);
    s.run_with(c, "")
}

fn memo_file(s: &Sandbox) -> PathBuf {
    s.path("wt/memos/7-az/MEMORY.md")
}

#[test]
fn memo_appends_lines_in_order_with_timestamp() {
    let s = memo_sandbox();
    let card = s.path("wt/7");
    assert_exit(&memo_in(&s, &card, &["memo", "first\nline"]), 0);
    assert_exit(&memo_in(&s, &card.join("a"), &["memo", "second"]), 0);
    let body = fs::read_to_string(memo_file(&s)).unwrap();
    let lines: Vec<&str> = body.lines().collect();
    assert_eq!(lines.len(), 2, "{body}");
    let re = regex::Regex::new(r"^\d{4}-\d{2}-\d{2} \d{2}:\d{2}: ").unwrap();
    assert!(re.is_match(lines[0]) && lines[0].ends_with(": first line"));
    assert!(re.is_match(lines[1]) && lines[1].ends_with(": second"));
    // without text: prints path and content
    let o = memo_in(&s, &card, &["memo"]);
    assert_exit(&o, 0);
    assert_has(&o, "memos/7-az/MEMORY.md");
    assert_has(&o, ": second");
}

#[test]
fn memo_symlink_is_relative_and_points_into_memos() {
    let s = memo_sandbox();
    let link = s.path("wt/7/MEMORY.md");
    assert!(fs::symlink_metadata(&link)
        .unwrap()
        .file_type()
        .is_symlink());
    assert_eq!(
        fs::read_link(&link).unwrap(),
        PathBuf::from("../memos/7-az/MEMORY.md")
    );
    assert_exit(&memo_in(&s, &s.path("wt/7"), &["memo", "hello"]), 0);
    assert!(fs::read_to_string(&link).unwrap().contains(": hello"));
}

#[test]
fn memo_survives_rm_and_new_relinks_it() {
    let s = memo_sandbox();
    assert_exit(&memo_in(&s, &s.path("wt/7"), &["memo", "keep me"]), 0);
    let o = s.run(&["rm", "7", "--force"]);
    assert_exit(&o, 0);
    assert_has(&o, "memo kept");
    assert!(!s.path("wt/7").exists());
    assert!(memo_file(&s).is_file());
    // the card is gone but the memo can still be extended with --card
    assert_exit(&s.run(&["memo", "-c", "7", "after rm"]), 0);
    let o = s.run(&["new", "7", "slug", "--no-ado"]);
    assert_exit(&o, 0);
    assert_has(&o, "memo restored");
    let body = fs::read_to_string(s.path("wt/7/MEMORY.md")).unwrap();
    assert!(
        body.contains(": keep me") && body.contains(": after rm"),
        "{body}"
    );
}

#[test]
fn memo_context_file_shows_only_the_last_10_entries() {
    let s = memo_sandbox();
    let card = s.path("wt/7");
    for i in 1..=12 {
        assert_exit(&memo_in(&s, &card, &["memo", &format!("entry-{i:02}")]), 0);
    }
    assert_exit(&s.run(&["refresh", "7"]), 0);
    let ctx = fs::read_to_string(card.join("CLAUDE.md")).unwrap();
    assert!(ctx.contains("## Memory (last 10 entries"), "{ctx}");
    assert!(
        !ctx.contains("entry-01") && !ctx.contains("entry-02"),
        "{ctx}"
    );
    assert!(
        ctx.contains("entry-03") && ctx.contains("entry-12"),
        "{ctx}"
    );
}

#[test]
fn memos_dir_is_never_a_card() {
    let s = memo_sandbox();
    assert_exit(&memo_in(&s, &s.path("wt/7"), &["memo", "x"]), 0);
    let o = s.run(&["ls", "--json"]);
    assert_exit(&o, 0);
    let out = String::from_utf8_lossy(&o.stdout).into_owned();
    assert!(out.contains("\"7\"") && !out.contains("memos"), "{out}");
    assert_exit(&s.run(&["status"]), 0);
    let o = s.run(&["prune", "-n"]);
    assert_exit(&o, 0);
    assert!(!text(&o).contains("memos"), "{}", text(&o));
    // cwd inside memos does not infer a card
    let o = memo_in(&s, &s.path("wt/memos/7-az"), &["memo", "nope"]);
    assert_exit(&o, 1);
    assert_has(&o, "not inside a card");
    assert_exit(&s.run(&["memo", "-c", "memos", "nope"]), 1);
    assert_exit(&s.run(&["new", "memos", "--no-ado"]), 1);
}

#[test]
fn memo_empty_text_is_an_error() {
    let s = memo_sandbox();
    let o = memo_in(&s, &s.path("wt/7"), &["memo", "  \n "]);
    assert_exit(&o, 1);
    assert_has(&o, "empty memo text");
    assert!(fs::read_to_string(memo_file(&s)).unwrap().is_empty());
}

#[test]
fn memo_card_flag_targets_another_card_from_anywhere() {
    let s = memo_sandbox();
    let o = s.run(&["memo", "--card", "7", "from outside"]);
    assert_exit(&o, 0);
    assert!(fs::read_to_string(memo_file(&s))
        .unwrap()
        .contains(": from outside"));
    assert_exit(&s.run(&["memo", "-c", "999", "x"]), 1);
}

// ───────────────────────── hooks ─────────────────────────

/// Append raw TOML to the sandbox config.
fn append_config(s: &Sandbox, extra: &str) {
    let mut cfg = fs::read_to_string(&s.config).unwrap();
    cfg.push_str(extra);
    fs::write(&s.config, cfg).unwrap();
}

/// Shell snippet that appends one line of hook context to `log`.
fn hook_line(log: &Path, tag: &str) -> String {
    format!(
        r#"echo "{tag}|$TWIG_EVENT|$TWIG_CARD_ID|$TWIG_REPO|$TWIG_BRANCH|${{TWIG_CARD_DIR##*/}}|${{TWIG_REPO_DIR##*/}}|${{PWD##*/}}" >> {log}"#,
        log = log.display()
    )
}

fn hook_log(log: &Path) -> String {
    fs::read_to_string(log).unwrap_or_default()
}

#[test]
fn hooks_post_create_fire_on_new_and_add_repo_with_env() {
    let s = new_sandbox();
    let log = s.path("hooks.log");
    append_config(
        &s,
        &format!(
            "\n[hooks]\npost_create = [{card:?}]\n\n[repos.a.hooks]\npost_create = [{a:?}]\n\n[repos.b.hooks]\npost_create = [{b:?}]\n",
            card = hook_line(&log, "card"),
            a = hook_line(&log, "a"),
            b = hook_line(&log, "b"),
        ),
    );
    assert_exit(&s.run(&["new", "7", "slug", "--no-ado"]), 0);
    let l = hook_log(&log);
    // repo-level runs in the worktree; card-level in the card folder with no repo
    assert!(l.contains("a|post_create|7|a|feat/7-slug|7|a|a\n"), "{l}");
    assert!(l.contains("card|post_create|7|||7||7\n"), "{l}");
    assert!(!l.contains("b|post_create"), "{l}");

    assert_exit(&s.run(&["add-repo", "7", "b"]), 0);
    let l = hook_log(&log);
    assert!(l.contains("b|post_create|7|b|feat/7|7|b|b\n"), "{l}");
    assert_eq!(l.matches("a|post_create").count(), 1, "{l}");
}

#[test]
fn hooks_legacy_repo_post_create_is_unchanged() {
    let s = new_sandbox();
    let log = s.path("legacy.log");
    let cfg = fs::read_to_string(&s.config).unwrap().replace(
        "[repos.a]\ndefault = true\n",
        &format!(
            "[repos.a]\ndefault = true\ninstall = \"echo install >> {log}\"\npost_create = [\"echo legacy $TWIG_CARD $TWIG_REPO >> {log}\"]\n",
            log = log.display()
        ),
    );
    fs::write(&s.config, cfg).unwrap();
    // not run without auto_install / --install
    assert_exit(&s.run(&["new", "7", "slug", "--no-ado"]), 0);
    assert_eq!(hook_log(&log), "");
    assert_exit(&s.run(&["rm", "7"]), 0);
    assert_exit(&s.run(&["new", "7", "slug", "--no-ado", "--install"]), 0);
    assert_eq!(hook_log(&log), "install\nlegacy 7 a\n");
}

#[test]
fn hooks_pre_rm_and_post_rm_fire_with_env() {
    let (s, _log) = rm_sandbox("idle");
    let log = s.path("hooks.log");
    let cpost = format!(
        r#"echo "cpost|$TWIG_EVENT|$TWIG_CARD_ID|$TWIG_REPO" >> {}"#,
        log.display()
    );
    let rpost = format!(
        r#"echo "rpost|$TWIG_EVENT|$TWIG_REPO|$TWIG_BRANCH|${{TWIG_REPO_DIR##*/}}" >> {}"#,
        log.display()
    );
    append_config(
        &s,
        &format!(
            "\n[hooks]\npre_rm = [{:?}]\npost_rm = [{cpost:?}]\n\n[repos.r1.hooks]\npre_rm = [{:?}]\npost_rm = [{rpost:?}]\n",
            hook_line(&log, "cpre"),
            hook_line(&log, "rpre"),
        ),
    );
    assert_exit(&s.run(&["rm", "123"]), 0);
    assert_eq!(
        hook_log(&log),
        "cpre|pre_rm|123|||123||123\n\
         rpre|pre_rm|123|r1|card-123|123|r1|r1\n\
         rpost|post_rm|r1|card-123|r1\n\
         cpost|post_rm|123|\n"
    );
    assert!(!s.path("wt/123").exists());
}

#[test]
fn hooks_failing_pre_rm_blocks_and_force_bypasses() {
    let (s, _log) = rm_sandbox("idle");
    let marker = s.path("pre.log");
    append_config(
        &s,
        &format!(
            "\n[hooks]\npre_rm = [\"echo ran >> {}; exit 3\"]\n",
            marker.display()
        ),
    );
    let o = s.run(&["rm", "123"]);
    assert_exit(&o, 1);
    assert_has(&o, "pre_rm hook");
    assert!(s.path("wt/123/r1").exists(), "card must survive");
    assert_eq!(hook_log(&marker), "ran\n");

    // --force skips pre_* hooks
    assert_exit(&s.run(&["rm", "-f", "123"]), 0);
    assert_eq!(hook_log(&marker), "ran\n");
    assert!(!s.path("wt/123").exists());
}

#[test]
fn hooks_failing_post_hook_only_warns() {
    let (s, _log) = rm_sandbox("idle");
    append_config(&s, "\n[hooks]\npost_rm = [\"exit 5\"]\n");
    let o = s.run(&["rm", "123"]);
    assert_exit(&o, 0);
    assert_has(&o, "post_rm hook `exit 5` failed");
    assert!(!s.path("wt/123").exists());
}

#[test]
fn hooks_no_hooks_flag_skips_everything() {
    let (s, _log) = rm_sandbox("idle");
    let marker = s.path("any.log");
    append_config(
        &s,
        &format!(
            "\n[hooks]\npre_rm = [\"echo pre >> {m}; exit 1\"]\npost_rm = [\"echo post >> {m}\"]\n",
            m = marker.display()
        ),
    );
    assert_exit(&s.run(&["rm", "123", "--no-hooks"]), 0);
    assert_eq!(hook_log(&marker), "");
    assert!(!s.path("wt/123").exists());
}

#[test]
fn hooks_post_open_fires_for_code_and_herdr() {
    let (s, _code_log, _herdr_log) = open_sandbox("");
    let log = s.path("hooks.log");
    append_config(
        &s,
        &format!(
            "\n[hooks]\npost_open = [{:?}]\n\n[repos.r1.hooks]\npost_open = [{:?}]\n",
            hook_line(&log, "card"),
            hook_line(&log, "r1")
        ),
    );
    assert_exit(&run_open(&s, &["code", "123"], None), 0);
    let l = hook_log(&log);
    assert!(l.contains("r1|post_open|123|r1||123|r1|r1\n"), "{l}");
    assert!(l.contains("card|post_open|123|||123||123\n"), "{l}");

    fs::remove_file(&log).unwrap();
    assert_exit(&run_open(&s, &["herdr", "123"], None), 0);
    assert!(hook_log(&log).contains("card|post_open|123"));

    fs::remove_file(&log).unwrap();
    assert_exit(&run_open(&s, &["open", "123", "code"], None), 0);
    assert!(hook_log(&log).contains("card|post_open|123"));

    fs::remove_file(&log).unwrap();
    assert_exit(&run_open(&s, &["herdr", "123", "--no-hooks"], None), 0);
    assert_eq!(hook_log(&log), "");
}

#[test]
fn hooks_post_refresh_fires() {
    let (s, _code_log) = code_sandbox();
    let log = s.path("hooks.log");
    append_config(
        &s,
        &format!(
            "\n[hooks]\npost_refresh = [{:?}]\n",
            hook_line(&log, "card")
        ),
    );
    assert_exit(&s.run(&["refresh", "123"]), 0);
    assert!(hook_log(&log).contains("card|post_refresh|123|||123||123\n"));
}

#[test]
fn doctor_flags_empty_hook_commands() {
    let s = Sandbox::new();
    s.all_tools().git_identity();
    s.valid_config("");
    append_config(
        &s,
        "\n[hooks]\npost_open = [\"echo ok\", \"  \"]\n\n[repos.r1.hooks]\npre_rm = [\"\"]\n",
    );
    let o = s.run(&["doctor"]);
    assert_exit(&o, 1);
    assert_has(&o, "hooks.post_open[1] is an empty command");
    assert_has(&o, "repos.r1.hooks.pre_rm[0] is an empty command");
}

#[test]
fn doctor_counts_valid_hooks() {
    let s = Sandbox::new();
    s.all_tools().git_identity();
    s.valid_config("");
    append_config(&s, "\n[hooks]\npost_open = [\"echo ok\"]\n");
    let o = s.run(&["doctor"]);
    assert_exit(&o, 0);
    assert_has(&o, "1 hook command(s) configured");
}

// ───────────────────────── templates ─────────────────────────

fn write_templates(s: &Sandbox, body: &str) {
    fs::write(s.config.parent().unwrap().join("templates.toml"), body).unwrap();
}

fn tpl_sandbox(templates: &str) -> Sandbox {
    let s = new_sandbox();
    write_templates(&s, templates);
    s
}

#[test]
fn template_applies_its_repos() {
    let s = tpl_sandbox("[both]\nrepos = [\"a\", \"b\"]\n");
    assert_exit(
        &s.run(&["new", "7", "x", "--no-ado", "--template", "both"]),
        0,
    );
    assert!(s.path("wt/7/a/.git").exists());
    assert!(s.path("wt/7/b/.git").exists());
}

#[test]
fn explicit_repos_and_flags_override_the_template() {
    let s =
        tpl_sandbox("[t]\nrepos = [\"a\", \"b\"]\nbranch_prefix = \"chore\"\nbase = \"nope\"\n");
    let o = s.run(&[
        "new",
        "7",
        "x",
        "--no-ado",
        "--template",
        "t",
        "--repos",
        "b",
        "--base",
        "main",
    ]);
    assert_exit(&o, 0);
    assert!(!s.path("wt/7/a").exists());
    // the template's branch_prefix applies (no --fix); --base beat the template's bad base
    assert_eq!(branch_of(s.path("wt/7/b")), "chore/7-x");

    let o = s.run(&[
        "new",
        "8",
        "x",
        "--no-ado",
        "--template",
        "t",
        "--base",
        "main",
        "--fix",
    ]);
    assert_exit(&o, 0);
    assert_eq!(branch_of(s.path("wt/8/a")), "fix/8-x");
}

#[test]
fn default_template_from_the_config_is_used() {
    let s = tpl_sandbox("[both]\nrepos = [\"a\", \"b\"]\n");
    append_config(&s, "\n[templates]\ndefault = \"both\"\n");
    assert_exit(&s.run(&["new", "7", "x", "--no-ado"]), 0);
    assert!(s.path("wt/7/b/.git").exists());
}

#[test]
fn unknown_template_lists_the_available_ones() {
    let s = tpl_sandbox("[one]\nrepos = [\"a\"]\n\n[two]\nrepos = [\"b\"]\n");
    let o = s.run(&["new", "7", "x", "--no-ado", "--template", "nope"]);
    assert_exit(&o, 1);
    assert_has(&o, "unknown template `nope`");
    assert_has(&o, "one, two");
    assert!(!s.path("wt/7").exists());
}

#[test]
fn template_hooks_fire_after_config_hooks_and_follow_the_card() {
    let s = tpl_sandbox("");
    let log = s.path("hooks.log");
    append_config(
        &s,
        &format!("\n[hooks]\npost_create = [{:?}]\n", hook_line(&log, "cfg")),
    );
    write_templates(
        &s,
        &format!(
            "[t]\nrepos = [\"a\"]\n\n[t.hooks]\npost_create = [{:?}]\npost_refresh = [{:?}]\n",
            hook_line(&log, "tpl"),
            hook_line(&log, "tplrefresh"),
        ),
    );
    assert_exit(&s.run(&["new", "7", "x", "--no-ado", "--template", "t"]), 0);
    let l = hook_log(&log);
    let (c, t) = (l.find("cfg|post_create"), l.find("tpl|post_create"));
    assert!(c.is_some() && t.is_some() && c < t, "{l}");

    // refresh re-reads the template recorded for the card
    assert_exit(&s.run(&["refresh", "7"]), 0);
    assert!(hook_log(&log).contains("tplrefresh|post_refresh|7"));

    // a card made without the template does not run its hooks
    assert_exit(&s.run(&["new", "8", "x", "--no-ado"]), 0);
    assert!(!hook_log(&log).contains("tpl|post_create|8"));
}

#[test]
fn template_agent_picks_the_context_file() {
    let s = tpl_sandbox("[cx]\nrepos = [\"a\"]\nagent = \"codex\"\n");
    assert_exit(
        &s.run(&["new", "7", "x", "--no-ado", "--template", "cx"]),
        0,
    );
    assert!(s.path("wt/7/AGENTS.md").exists());
    assert!(!s.path("wt/7/CLAUDE.md").exists());
    assert_exit(&s.run(&["refresh", "7"]), 0);
    assert!(s.path("wt/7/AGENTS.md").exists());
    assert!(!s.path("wt/7/CLAUDE.md").exists());
}

#[test]
fn template_install_runs_the_repo_setup() {
    let s = tpl_sandbox("[t]\nrepos = [\"a\"]\ninstall = true\n");
    let log = s.path("install.log");
    let cfg = fs::read_to_string(&s.config).unwrap().replace(
        "[repos.a]\ndefault = true\n",
        &format!(
            "[repos.a]\ndefault = true\ninstall = \"echo installed >> {}\"\n",
            log.display()
        ),
    );
    fs::write(&s.config, cfg).unwrap();
    assert_exit(&s.run(&["new", "7", "x", "--no-ado", "--template", "t"]), 0);
    assert!(hook_log(&log).contains("installed"));
    assert_exit(
        &s.run(&["new", "8", "x", "--no-ado", "--template", "t", "--no-setup"]),
        0,
    );
    assert_eq!(hook_log(&log).matches("installed").count(), 1);
}

#[test]
fn no_templates_file_changes_nothing() {
    let s = new_sandbox();
    assert_exit(&s.run(&["new", "7", "x", "--no-ado"]), 0);
    assert!(s.path("wt/7/CLAUDE.md").exists());
    let o = s.run(&["new", "8", "x", "--no-ado", "--template", "t"]);
    assert_exit(&o, 1);
    assert_has(&o, "no templates file");
    assert_exit(&s.run(&["template", "ls"]), 0);
}

#[test]
fn template_env_var_overrides_the_path() {
    let s = new_sandbox();
    let f = s.path("elsewhere.toml");
    fs::write(&f, "[x]\nrepos = [\"b\"]\n").unwrap();
    let mut c = s.cmd(&["new", "7", "x", "--no-ado", "--template", "x"]);
    c.env("TWIG_TEMPLATES", &f);
    assert_exit(&s.run_with(c, ""), 0);
    assert!(s.path("wt/7/b/.git").exists());
}

#[test]
fn doctor_catches_a_template_with_an_unknown_repo_agent_or_hook() {
    let s = Sandbox::new();
    s.all_tools().git_identity();
    s.valid_config("");
    write_templates(
        &s,
        "[bad]\nrepos = [\"r1\", \"ghost\"]\n\n[bad.hooks]\npre_rm = [\" \"]\n",
    );
    let o = s.run(&["doctor"]);
    assert_exit(&o, 1);
    assert_has(&o, "[bad] repo `ghost` is not in the config");
    assert_has(&o, "[bad] bad.hooks.pre_rm[0] is an empty command");

    write_templates(&s, "[bad]\nagent = \"gemini\"\n");
    let o = s.run(&["doctor"]);
    assert_exit(&o, 1);
    assert_has(&o, "claude");

    write_templates(&s, "[ok]\nrepos = [\"r1\"]\n");
    append_config(&s, "\n[templates]\ndefault = \"missing\"\n");
    let o = s.run(&["doctor"]);
    assert_exit(&o, 1);
    assert_has(&o, "templates.default = `missing`");

    let cfg = fs::read_to_string(&s.config)
        .unwrap()
        .replace("missing", "ok");
    fs::write(&s.config, cfg).unwrap();
    assert_exit(&s.run(&["doctor"]), 0);
}

#[test]
fn template_ls_and_show() {
    let s = tpl_sandbox(
        "[full]\nrepos = [\"a\", \"b\"]\nagent = \"codex\"\ninstall = true\n\n[full.hooks]\npost_create = [\"echo hi\"]\n\n[solo]\nrepos = [\"a\"]\n",
    );
    let o = s.run(&["template", "ls"]);
    assert_exit(&o, 0);
    assert_has(&o, "full");
    assert_has(&o, "a, b");
    assert_has(&o, "solo");

    let o = s.run(&["template", "show", "full"]);
    assert_exit(&o, 0);
    assert_has(&o, "codex");
    assert_has(&o, "install        true");
    assert_has(&o, "echo hi");

    let o = s.run(&["template", "show", "nope"]);
    assert_exit(&o, 1);
    assert_has(&o, "full, solo");
}

#[test]
fn template_edit_creates_and_validates() {
    let s = new_sandbox();
    let ed = s.script("ed.sh", "printf '[t]\\nrepos = [\"ghost\"]\\n' > \"$1\"");
    let mut c = s.cmd(&["template", "edit"]);
    c.env("EDITOR", &ed);
    // the unknown repo is invalid; declining the reopen leaves an error
    let o = s.run_with(c, "n\n");
    assert_exit(&o, 1);
    assert_has(&o, "ghost");

    let ed = s.script("ed2.sh", "printf '[t]\\nrepos = [\"a\"]\\n' > \"$1\"");
    let mut c = s.cmd(&["template", "edit"]);
    c.env("EDITOR", &ed);
    assert_exit(&s.run_with(c, ""), 0);
    assert_has(&s.run(&["template", "ls"]), "t");
}

// ───────────────────────── pr ─────────────────────────

/// Card `7` with repos `a` and `b` (local bare origins). `commit_in` get one commit each.
fn pr_sandbox(provider: &str, commit_in: &[&str]) -> Sandbox {
    let s = new_sandbox();
    append_config(&s, &format!("\n[provider]\ndefault = \"{provider}\"\n"));
    assert_exit(&s.run(&["new", "7", "x", "--repos", "a,b", "--no-ado"]), 0);
    for r in commit_in {
        commit_file(&s.path(&format!("wt/7/{r}")), "f.txt", r, "work");
    }
    s
}

/// Fake `gh`: logs `<repo dir> <args>`; `pr list` answers `$GH_OPEN` for repo `b` (else `[]`),
/// `pr create` prints a URL with the repo dir name.
fn gh_pr_stub(s: &Sandbox, open_in_b: &str) -> PathBuf {
    let log = s.path("gh.log");
    s.stub(
        "gh",
        &format!(
            r#"echo "${{PWD##*/}} $@" >> {log}
case "$1 $2" in
  "pr list") if [ "${{PWD##*/}}" = b ]; then echo '{open_in_b}'; else echo '[]'; fi ;;
  "pr create") echo "https://github.com/acme/${{PWD##*/}}/pull/3" ;;
esac"#,
            log = log.display()
        ),
    );
    log
}

fn origin_has(s: &Sandbox, repo: &str, branch: &str) -> bool {
    Command::new("git")
        .args([
            "show-ref",
            "--verify",
            "--quiet",
            &format!("refs/heads/{branch}"),
        ])
        .current_dir(s.path(&format!("origin/{repo}.git")))
        .status()
        .unwrap()
        .success()
}

#[test]
fn pr_pushes_and_opens_one_pr_per_repo_and_links_them() {
    let s = pr_sandbox("gh", &["a", "b"]);
    let log = gh_pr_stub(&s, "[]");
    let o = s.run(&["pr", "7", "--draft"]);
    assert_exit(&o, 0);
    let gh = fs::read_to_string(&log).unwrap();
    for r in ["a", "b"] {
        assert!(origin_has(&s, r, "feat/7-x"), "{r} not pushed");
        assert!(
            gh.contains(&format!(
                "{r} pr create --base main --head feat/7-x --title feat/7-x --body Card #7 --draft"
            )),
            "{gh}"
        );
    }
    assert!(
        gh.contains("a pr edit https://github.com/acme/a/pull/3 --body Card #7"),
        "{gh}"
    );
    assert!(gh.contains("- b: https://github.com/acme/b/pull/3"), "{gh}");
    assert!(gh.contains("- a: https://github.com/acme/a/pull/3"), "{gh}");
    assert_has(&o, "https://github.com/acme/b/pull/3");
}

#[test]
fn pr_skips_repos_without_commits_and_reuses_an_open_pr() {
    let s = pr_sandbox("gh", &["a"]);
    let log = gh_pr_stub(&s, "[]");
    let o = s.run(&["pr", "7"]);
    assert_exit(&o, 0);
    assert_has(
        &o,
        "[b] feat/7-x has no commits ahead of origin/main, skipped",
    );
    let gh = fs::read_to_string(&log).unwrap();
    assert!(
        !gh.contains("b pr create") && !gh.contains("pr edit"),
        "{gh}"
    );

    let s = pr_sandbox("gh", &["a", "b"]);
    let log = gh_pr_stub(
        &s,
        r#"[{"number":9,"url":"https://github.com/acme/b/pull/9"}]"#,
    );
    let o = s.run(&["pr", "7"]);
    assert_exit(&o, 0);
    assert_has(&o, "[b] PR already open: https://github.com/acme/b/pull/9");
    let gh = fs::read_to_string(&log).unwrap();
    assert!(!gh.contains("b pr create"), "{gh}");
    // only the PR opened now gets the sibling list
    assert!(
        gh.contains("a pr edit") && !gh.contains("b pr edit"),
        "{gh}"
    );
    assert!(gh.contains("- b: https://github.com/acme/b/pull/9"), "{gh}");
}

#[test]
fn pr_dry_run_pushes_and_opens_nothing() {
    let s = pr_sandbox("gh", &["a"]);
    let log = gh_pr_stub(&s, "[]");
    let o = s.run(&["pr", "7", "-n"]);
    assert_exit(&o, 0);
    assert_has(
        &o,
        "[a] would push feat/7-x (1 commit(s)) and open a PR into main",
    );
    assert!(!origin_has(&s, "a", "feat/7-x"));
    assert!(!fs::read_to_string(&log).unwrap().contains("pr create"));
}

#[test]
fn pr_with_azure_devops_links_the_work_item() {
    let s = pr_sandbox("az", &["a"]);
    let log = s.path("az.log");
    s.stub(
        "az",
        &format!(
            r#"echo "$@" >> {log}
case "$1 $2 $3" in
  "repos pr list") echo '[]' ;;
  "repos pr create") echo '{{"pullRequestId":5,"repository":{{"webUrl":"https://dev.azure.com/o/p/_git/a"}}}}' ;;
esac"#,
            log = log.display()
        ),
    );
    let o = s.run(&["pr", "7"]);
    assert_exit(&o, 0);
    let az = fs::read_to_string(&log).unwrap();
    assert!(
        az.contains("repos pr create --source-branch feat/7-x --target-branch main --title feat/7-x --description Card #7 --work-items 7 --draft false"),
        "{az}"
    );
    assert_has(&o, "https://dev.azure.com/o/p/_git/a/pullrequest/5");
}

#[test]
fn pr_without_the_provider_cli_fails() {
    let s = pr_sandbox("gh", &["a"]);
    let o = s.run(&["pr", "7"]);
    assert_exit(&o, 1);
    assert_has(&o, "`gh` is not on PATH");
}

// ───────────────────────── clean ─────────────────────────

/// Merge `branch` of repo `a` into origin's main through the main clone.
fn merge_into_origin_main(s: &Sandbox, branch: &str) {
    let main = s.path("repos/a");
    git_in(&main, &["fetch", "-q", "origin"]);
    git_in(
        &main,
        &["merge", "-q", "--no-edit", &format!("origin/{branch}")],
    );
    git_in(&main, &["push", "-q", "origin", "main"]);
}

/// Cards 7 (own commit, merged into main), 8 (no commits) and 9 (own commit, not merged).
fn clean_sandbox() -> Sandbox {
    let s = new_sandbox();
    for id in ["7", "8", "9"] {
        assert_exit(&s.run(&["new", id, "x", "--no-ado"]), 0);
    }
    for id in ["7", "9"] {
        let wt = s.path(&format!("wt/{id}/a"));
        commit_file(&wt, "f.txt", id, "work");
        git_in(&wt, &["push", "-q", "origin", &format!("feat/{id}-x")]);
    }
    merge_into_origin_main(&s, "feat/7-x");
    s
}

#[test]
fn clean_dry_run_lists_only_cards_whose_branches_were_merged() {
    let s = clean_sandbox();
    let o = s.run(&["clean", "-n"]);
    assert_exit(&o, 0);
    assert_has(&o, "#7");
    assert_has(&o, "every branch merged");
    let out = text(&o);
    assert!(!out.contains("#8") && !out.contains("#9"), "{out}");
    assert!(s.path("wt/7").exists());
}

#[test]
fn clean_asks_per_card_and_yes_removes_without_asking() {
    let s = clean_sandbox();
    let declined = s.run_with(s.cmd(&["clean"]), "n\n");
    assert_exit(&declined, 0);
    assert!(s.path("wt/7").exists());

    let o = s.run_with(s.cmd(&["clean"]), "y\n");
    assert_exit(&o, 0);
    assert!(!s.path("wt/7").exists());
    assert!(s.path("wt/8").exists() && s.path("wt/9").exists());

    let s = clean_sandbox();
    assert_exit(&s.run(&["clean", "--yes", "--branches"]), 0);
    assert!(!s.path("wt/7").exists());
    assert!(!git_in(&s.path("repos/a"), &["branch", "--list", "feat/7-x"]).contains("feat/7-x"));
}

#[test]
fn clean_keeps_the_rm_safety_checks() {
    let s = clean_sandbox();
    fs::write(s.path("wt/7/a/dirty.txt"), "x").unwrap();
    let o = s.run(&["clean", "--yes"]);
    assert_exit(&o, 1);
    assert_has(&o, "uncommitted changes");
    assert!(s.path("wt/7").exists());
    assert_exit(&s.run(&["clean", "--yes", "--force"]), 0);
    assert!(!s.path("wt/7").exists());
}

#[test]
fn clean_uses_closed_issues_and_merged_prs_from_the_provider() {
    let s = new_sandbox();
    append_config(&s, "\n[provider]\ndefault = \"gh\"\n");
    s.stub("gh", "exit 1");
    for id in ["5", "6", "8"] {
        assert_exit(&s.run(&["new", id, "x", "--no-ado"]), 0);
    }
    // 5: issue closed; 6: squash-merged PR (the branch itself is not in main); 8: open
    commit_file(&s.path("wt/6/a"), "f.txt", "6", "work");
    s.stub(
        "gh",
        r#"case "$1 $2 $3" in
  "issue view 5") echo '{"title":"T","state":"CLOSED"}' ;;
  "issue view 6"|"issue view 8") echo '{"title":"T","state":"OPEN"}' ;;
  "pr list --head")
    if [ "$4 $6" = "feat/6-x merged" ]; then echo '[{"number":1,"url":"u"}]'; else echo '[]'; fi ;;
esac"#,
    );
    let o = s.run(&["clean", "-n"]);
    assert_exit(&o, 0);
    assert_has(&o, "work item CLOSED");
    assert_has(&o, "#6");
    assert!(!text(&o).contains("#8"), "{}", text(&o));
}

// ───────────────────────── ports ─────────────────────────

/// Fake `herdr` that logs its calls and answers just enough for `twig herdr` to build a card.
fn herdr_builder_stub(s: &Sandbox) -> PathBuf {
    let log = s.path("herdr.log");
    s.stub(
        "herdr",
        &format!(
            r#"echo "$@" >> {log}
case "$1 $2" in
  "workspace list") echo '{{"result":{{"workspaces":[]}}}}' ;;
  "workspace create") echo '{{"result":{{"workspace":{{"workspace_id":"w1"}},"tab":{{"tab_id":"t1"}}}}}}' ;;
  "tab list") echo '{{"result":{{"tabs":[{{"label":"agent","tab_id":"t1"}}]}}}}' ;;
  "pane list") echo '{{"result":{{"panes":[{{"pane_id":"p1","tab_id":"t1","cwd":"","foreground_cwd":""}}]}}}}' ;;
  "tab create") echo '{{"result":{{"root_pane":{{"pane_id":"p9"}}}}}}' ;;
  "pane split") echo '{{"result":{{"pane":{{"pane_id":"p2"}}}}}}' ;;
esac"#,
            log = log.display()
        ),
    );
    log
}

/// Repos `a` (port 4200, with a run command) and `b` (port 5000).
fn ports_sandbox() -> Sandbox {
    let s = new_sandbox();
    let cfg = fs::read_to_string(&s.config)
        .unwrap()
        .replace(
            "[repos.a]\ndefault = true\n",
            "[repos.a]\ndefault = true\nrun = \"serve\"\nport = 4200\n",
        )
        .replace("[repos.b]\n", "[repos.b]\nport = 5000\n");
    fs::write(&s.config, cfg).unwrap();
    s
}

#[test]
fn ports_are_deterministic_per_card_and_written_to_the_card() {
    let s = ports_sandbox();
    assert_exit(
        &s.run(&["new", "52644", "x", "--repos", "a,b", "--no-ado"]),
        0,
    );
    let env = fs::read_to_string(s.path("wt/52644/.twig-ports.env")).unwrap();
    assert!(
        env.contains("TWIG_PORT_A=4244\nTWIG_PORT_B=5044\n"),
        "{env}"
    );
    let ctx = fs::read_to_string(s.path("wt/52644/CLAUDE.md")).unwrap();
    assert!(ctx.contains("- `a`: 4244 (`$TWIG_PORT_A`)"), "{ctx}");

    // same card again → same ports
    assert_exit(&s.run(&["rm", "52644"]), 0);
    assert_exit(
        &s.run(&["new", "52644", "x", "--repos", "a,b", "--no-ado"]),
        0,
    );
    let again = fs::read_to_string(s.path("wt/52644/.twig-ports.env")).unwrap();
    assert_eq!(env, again);

    let o = s.run(&["ports", "52644", "--env"]);
    assert_exit(&o, 0);
    assert_eq!(
        stdout(&o),
        "export TWIG_PORT_A=4244\nexport TWIG_PORT_B=5044\n"
    );
    let o = s.run(&["ports", "52644"]);
    assert_has(&o, "base port + 44");
}

#[test]
fn env_prints_the_full_pane_environment() {
    let s = ports_sandbox();
    append_config(
        &s,
        "\n[repos.a.env]\nAPI_URL = \"http://localhost:$PORT/it's\"\n",
    );
    assert_exit(
        &s.run(&["new", "52644", "x", "--repos", "a,b", "--no-ado"]),
        0,
    );

    let o = s.run(&["env", "52644", "--repo", "a"]);
    assert_exit(&o, 0);
    assert_eq!(
        stdout(&o),
        "export TWIG_CARD_ID='52644'\nexport TWIG_PANE_REPO='a'\nexport TWIG_PORT_A='4244'\n\
         export TWIG_PORT_B='5044'\nexport PORT='4244'\n\
         export API_URL='http://localhost:4244/it'\\''s'\n"
    );

    // inside a repo's worktree the card and the repo are inferred
    let mut c = s.cmd(&["env"]);
    c.current_dir(s.path("wt/52644/b"));
    let o = s.run_with(c, "");
    assert_exit(&o, 0);
    assert!(
        stdout(&o).contains("export TWIG_PANE_REPO='b'\n")
            && stdout(&o).contains("export PORT='5044'\n"),
        "{}",
        stdout(&o)
    );

    assert_exit(&s.run(&["env", "52644", "--repo", "nope"]), 1);
}

#[test]
fn doctor_flags_a_herdr_pane_without_the_twig_env() {
    let s = ports_sandbox();
    assert_exit(
        &s.run(&["new", "52644", "x", "--repos", "a", "--no-ado"]),
        0,
    );
    let doctor = |card_id: Option<&str>| {
        let mut c = s.cmd(&["doctor"]);
        c.current_dir(s.path("wt/52644/a")).env("HERDR_ENV", "1");
        match card_id {
            Some(v) => c.env("TWIG_CARD_ID", v),
            None => c.env_remove("TWIG_CARD_ID"),
        };
        s.run_with(c, "")
    };
    assert_has(&doctor(None), "has no twig environment");
    let o = doctor(Some("52644"));
    assert!(
        !text(&o).contains("has no twig environment"),
        "{}",
        text(&o)
    );
}

#[test]
fn env_auto_is_silent_outside_a_card_or_in_a_set_up_pane() {
    let s = ports_sandbox();
    assert_exit(
        &s.run(&["new", "52644", "x", "--repos", "a", "--no-ado"]),
        0,
    );

    let mut c = s.cmd(&["env", "--auto"]);
    c.current_dir(&s.home);
    let o = s.run_with(c, "");
    assert_exit(&o, 0);
    assert_eq!(stdout(&o), "");

    let mut c = s.cmd(&["env", "--auto"]);
    c.current_dir(s.path("wt/52644/a"))
        .env("TWIG_CARD_ID", "52644");
    let o = s.run_with(c, "");
    assert_exit(&o, 0);
    assert_eq!(stdout(&o), "");

    let mut c = s.cmd(&["env", "--auto"]);
    c.current_dir(s.path("wt/52644/a"));
    let o = s.run_with(c, "");
    assert_exit(&o, 0);
    assert!(
        stdout(&o).contains("export PORT='4244'\n"),
        "{}",
        stdout(&o)
    );
}

#[test]
fn ports_reach_herdr_panes_hooks_and_vscode() {
    let s = ports_sandbox();
    let log = herdr_builder_stub(&s);
    let hook_log = s.path("hook.log");
    append_config(
        &s,
        &format!(
            "\n[repos.b.hooks]\npost_create = [\"echo \\\"$PORT $TWIG_PORT_A\\\" >> {}\"]\n",
            hook_log.display()
        ),
    );
    assert_exit(&s.run(&["new", "7", "x", "--repos", "a,b", "--no-ado"]), 0);
    let calls = fs::read_to_string(&log).unwrap();
    assert!(
        calls.contains("workspace create")
            && calls.contains("--env TWIG_CARD_ID=7 --env TWIG_PANE_REPO=a --env TWIG_PORT_A=4207 --env TWIG_PORT_B=5007 --env PORT=4207"),
        "{calls}"
    );
    assert!(
        calls.contains(
            "--label run --no-focus --env TWIG_CARD_ID=7 --env TWIG_PANE_REPO=a --env TWIG_PORT_A=4207 --env TWIG_PORT_B=5007 --env PORT=4207"
        ),
        "{calls}"
    );
    assert_eq!(fs::read_to_string(&hook_log).unwrap(), "5007 4207\n");
    let ws: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(s.path("wt/7/7.code-workspace")).unwrap())
            .unwrap();
    assert_eq!(ws["tasks"]["tasks"][0]["options"]["env"]["PORT"], "4207");
    assert_eq!(
        ws["settings"]["terminal.integrated.env.linux"]["TWIG_PORT_B"],
        "5007"
    );
}

#[test]
fn ports_shared_with_another_card_only_warn() {
    let s = ports_sandbox();
    assert_exit(&s.run(&["new", "44", "x", "--no-ado"]), 0);
    let o = s.run(&["new", "144", "x", "--no-ado"]);
    assert_exit(&o, 0);
    assert_has(&o, "card 44 uses the same ports (a 4244)");
}

#[test]
fn cards_without_ports_have_no_ports_file_and_span_is_validated() {
    let s = new_sandbox();
    assert_exit(&s.run(&["new", "7", "x", "--no-ado"]), 0);
    assert!(!s.path("wt/7/.twig-ports.env").exists());
    assert_has(&s.run(&["ports", "7"]), "has no ports");
    append_config(&s, "\n[ports]\nspan = 0\n");
    assert_has(&s.run(&["ls"]), "ports.span must be at least 1");
}

// ───────────────────────── overrides (.twig.toml, env) ─────────────────────────

/// `ports_sandbox` whose repo `a` versions a `.twig.toml` on origin's main.
fn overrides_sandbox(repo_file: &str) -> Sandbox {
    let s = ports_sandbox();
    let main = s.path("repos/a");
    commit_file(&main, ".twig.toml", repo_file, "twig settings");
    git_in(&main, &["push", "-q", "origin", "main"]);
    s
}

fn ports_env(s: &Sandbox, extra_env: &[(&str, &str)]) -> Output {
    let mut c = s.cmd(&["ports", "7", "--env"]);
    for (k, v) in extra_env {
        c.env(k, v);
    }
    s.run_with(c, "")
}

#[test]
fn overrides_follow_repo_then_card_then_env() {
    let s = overrides_sandbox("port = 4300\nrun = \"repo-run\"\n");
    assert_exit(&s.run(&["new", "7", "x", "--no-ado"]), 0);
    // the repo's .twig.toml beats the user config
    assert_eq!(stdout(&ports_env(&s, &[])), "export TWIG_PORT_A=4307\n");
    let ctx = fs::read_to_string(s.path("wt/7/CLAUDE.md")).unwrap();
    assert!(ctx.contains("`repo-run`"), "{ctx}");

    // the card's .twig.toml beats the repo's
    fs::write(s.path("wt/7/.twig.toml"), "[repos.a]\nport = 4400\n").unwrap();
    assert_eq!(stdout(&ports_env(&s, &[])), "export TWIG_PORT_A=4407\n");
    assert_exit(&s.run(&["refresh", "7"]), 0);
    let env = fs::read_to_string(s.path("wt/7/.twig-ports.env")).unwrap();
    assert!(env.contains("TWIG_PORT_A=4407"), "{env}");

    // the environment beats both
    let o = ports_env(&s, &[("TWIG__REPOS__A__PORT", "4500")]);
    assert_eq!(stdout(&o), "export TWIG_PORT_A=4507\n");
}

#[test]
fn env_override_of_a_global_key_and_invalid_values() {
    let s = new_sandbox();
    let mut c = s.cmd(&["new", "7", "x", "--no-ado"]);
    c.env("TWIG__GIT__BASE_BRANCH", "nope");
    let o = s.run_with(c, "");
    // the override is in effect: the branch is created from origin/nope, which doesn't exist
    assert_exit(&o, 1);
    assert_has(&o, "origin/nope");

    let mut c = s.cmd(&["ls"]);
    c.env("TWIG__REPOS__A__PORT", "not-a-port");
    let o = s.run_with(c, "");
    assert_exit(&o, 1);
    assert_has(&o, "TWIG__REPOS__A__PORT");

    let mut c = s.cmd(&["doctor"]);
    c.env("TWIG__OPEN__DEFAULT", "code");
    assert_has(
        &s.run_with(c, ""),
        "overridden by TWIG__OPEN__DEFAULT (environment)",
    );
}

#[test]
fn repo_file_with_a_forbidden_key_is_an_error_and_doctor_reports_it() {
    let s = overrides_sandbox("path = \"/tmp\"\n");
    let o = s.run(&["new", "7", "x", "--no-ado"]);
    assert_has(&o, "`path` can't be overridden");
    let o = s.run(&["ports", "7"]);
    assert_exit(&o, 1);
    assert_has(&o, "`path` can't be overridden");
    let d = s.run(&["doctor"]);
    assert_has(&d, "a: ");
    assert_has(&d, "`path` can't be overridden");
}

// ───────────────────────── agent prompt ─────────────────────────

#[test]
fn new_prompt_is_the_first_message_of_every_agent() {
    let s = new_sandbox();
    let log = herdr_builder_stub(&s);
    let o = s.run(&[
        "new",
        "7",
        "x",
        "--repos",
        "a,b",
        "--no-ado",
        "--prompt",
        "/deliver {id} ({repo})",
    ]);
    assert_exit(&o, 0);
    let calls = fs::read_to_string(&log).unwrap();
    assert!(calls.contains(" claude '/deliver 7 (a)'\n"), "{calls}");
    assert!(calls.contains(" claude '/deliver 7 (b)'\n"), "{calls}");
}

#[test]
fn template_agent_prompt_and_the_flag_overriding_it() {
    let s = tpl_sandbox("[deliver]\nrepos = [\"a\"]\nagent_prompt = \"/entregar {id}\"\n");
    let log = herdr_builder_stub(&s);
    assert_exit(
        &s.run(&["new", "7", "x", "--template", "deliver", "--no-ado"]),
        0,
    );
    assert!(fs::read_to_string(&log)
        .unwrap()
        .contains("claude '/entregar 7'"));

    fs::remove_file(&log).unwrap();
    assert_exit(
        &s.run(&[
            "new",
            "8",
            "x",
            "--template",
            "deliver",
            "--no-ado",
            "--prompt",
            "",
        ]),
        0,
    );
    let calls = fs::read_to_string(&log).unwrap();
    assert!(
        calls.contains(" claude\n") && !calls.contains("entregar"),
        "{calls}"
    );
}

// ───────────────────────── repo env ─────────────────────────

#[test]
fn repo_env_is_expanded_per_card_and_reaches_panes_hooks_and_vscode() {
    let s = ports_sandbox();
    let log = herdr_builder_stub(&s);
    let hook_log = s.path("hook.log");
    let cfg = fs::read_to_string(&s.config).unwrap().replace(
        "[repos.b]\nport = 5000\n",
        &format!(
            "[repos.b]\nport = 5000\nrun = \"api\"\nenv = {{ Cors__Origins__0 = \"http://localhost:$TWIG_PORT_A\", Front = \"${{TWIG_PORT_NOPE:-4200}}\", Card = \"$TWIG_CARD\" }}\n\n[repos.b.hooks]\npost_create = [\"echo \\\"$Cors__Origins__0 $Front $Card\\\" >> {}\"]\n",
            hook_log.display()
        ),
    );
    fs::write(&s.config, cfg).unwrap();
    assert_exit(
        &s.run(&["new", "52644", "x", "--repos", "a,b", "--no-ado"]),
        0,
    );

    assert_eq!(
        fs::read_to_string(&hook_log).unwrap(),
        "http://localhost:4244 4200 52644\n"
    );
    let calls = fs::read_to_string(&log).unwrap();
    assert!(
        calls.contains("--env PORT=5044 --env Cors__Origins__0=http://localhost:4244 --env Front=4200 --env Card=52644"),
        "{calls}"
    );
    let ws: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(s.path("wt/52644/52644.code-workspace")).unwrap())
            .unwrap();
    let task_env = &ws["tasks"]["tasks"][1]["options"]["env"];
    assert_eq!(task_env["Cors__Origins__0"], "http://localhost:4244");
    let ctx = fs::read_to_string(s.path("wt/52644/CLAUDE.md")).unwrap();
    assert!(
        ctx.contains("with `Cors__Origins__0=http://localhost:4244`"),
        "{ctx}"
    );
}

#[test]
fn repo_env_with_an_invalid_name_is_a_config_error() {
    let s = new_sandbox();
    append_config(&s, "\n[repos.a.env]\n\"BAD-NAME\" = \"x\"\n");
    let o = s.run(&["ls"]);
    assert_exit(&o, 1);
    assert_has(
        &o,
        "repos.a.env: `BAD-NAME` is not a valid environment variable name",
    );
}

#[test]
fn repo_env_can_come_from_the_repo_and_card_twig_toml() {
    let s = overrides_sandbox("env = { API = \"x:$PORT\" }\n");
    assert_exit(&s.run(&["new", "7", "x", "--no-ado"]), 0);
    let ctx = fs::read_to_string(s.path("wt/7/CLAUDE.md")).unwrap();
    assert!(ctx.contains("with `API=x:4207`"), "{ctx}");
    fs::write(
        s.path("wt/7/.twig.toml"),
        "[repos.a]\nenv = { API = \"card:$TWIG_CARD\" }\n",
    )
    .unwrap();
    assert_exit(&s.run(&["refresh", "7"]), 0);
    let ctx = fs::read_to_string(s.path("wt/7/CLAUDE.md")).unwrap();
    assert!(ctx.contains("with `API=card:7`"), "{ctx}");
}

// ───────────────────────── named ports ─────────────────────────

#[test]
fn named_ports_get_their_own_variables_everywhere() {
    let s = ports_sandbox();
    let cfg = fs::read_to_string(&s.config).unwrap().replace(
        "[repos.b]\nport = 5000\n",
        "[repos.b]\nport = 5000\nports = { https = 5100, debug = 9200 }\nenv = { Url = \"https://localhost:$TWIG_PORT_B_HTTPS\" }\n",
    );
    fs::write(&s.config, cfg).unwrap();
    assert_exit(
        &s.run(&["new", "52644", "x", "--repos", "a,b", "--no-ado"]),
        0,
    );

    assert_eq!(
        stdout(&s.run(&["ports", "52644", "--env"])),
        "export TWIG_PORT_A=4244\nexport TWIG_PORT_B=5044\nexport TWIG_PORT_B_HTTPS=5144\nexport TWIG_PORT_B_DEBUG=9244\n"
    );
    assert_has(&s.run(&["ports", "52644"]), "b.https");
    let file = fs::read_to_string(s.path("wt/52644/.twig-ports.env")).unwrap();
    assert!(file.contains("TWIG_PORT_B_DEBUG=9244"), "{file}");
    let st = json_of(&s.run(&["status", "52644", "--json"]));
    let b = &st["cards"][0]["repos"][1];
    assert_eq!(b["port"], 5044);
    assert_eq!(b["ports"]["https"], 5144);
    let ctx = fs::read_to_string(s.path("wt/52644/CLAUDE.md")).unwrap();
    assert!(
        ctx.contains("- `b.https`: 5144 (`$TWIG_PORT_B_HTTPS`)"),
        "{ctx}"
    );
    // env sees the named ports
    let ws: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(s.path("wt/52644/52644.code-workspace")).unwrap())
            .unwrap();
    assert_eq!(
        ws["settings"]["terminal.integrated.env.linux"]["TWIG_PORT_B_HTTPS"],
        "5144"
    );
}

#[test]
fn base_ports_closer_than_the_span_are_rejected() {
    let s = ports_sandbox();
    append_config(&s, "\n[repos.c]\nport = 4250\n");
    let o = s.run(&["ls"]);
    assert_exit(&o, 1);
    assert_has(
        &o,
        "ports 4200 (a) and 4250 (c) are 50 apart, less than ports.span = 100",
    );
    assert_has(&s.run(&["doctor"]), "are 50 apart");
    // a smaller span makes them fit
    append_config(&s, "\n[ports]\nspan = 50\n");
    assert_exit(&s.run(&["ls"]), 0);
}

#[test]
fn named_ports_from_a_repo_file_are_checked_too() {
    let s = overrides_sandbox("ports = { https = 4210 }\n");
    let o = s.run(&["new", "7", "x", "--no-ado"]);
    assert_has(&o, "(a.https)");
    let o = s.run(&["ports", "7"]);
    assert_exit(&o, 1);
    assert_has(&o, "ports 4200 (a) and 4210 (a.https) are 10 apart");

    let s = new_sandbox();
    append_config(&s, "\n[repos.a.ports]\n\"bad-name\" = 6000\n");
    assert_has(&s.run(&["ls"]), "`bad-name` is not a valid port name");
}

// ───────────────────────── short aliases ─────────────────────────

#[test]
fn short_flag_and_command_aliases_match_the_long_forms() {
    let s = status_sandbox("idle");
    let long = s.run(&["status", "123", "--json"]);
    let short = s.run(&["st", "123", "-j"]);
    assert_exit(&short, 0);
    assert_eq!(json_of(&long), json_of(&short));
    assert_eq!(
        json_of(&s.run(&["ls", "--json"])),
        json_of(&s.run(&["ls", "-j"]))
    );
}

#[test]
fn short_aliases_show_in_help() {
    let s = Sandbox::new();
    let o = s.run(&["new", "--help"]);
    assert_has(&o, "-t, --template");
    assert_has(&o, "-r, --repos");
}

#[test]
fn command_aliases_show_in_help_and_completions() {
    let s = Sandbox::new();
    let o = s.run(&["--help"]);
    assert_has(&o, "add-repo");
    assert!(stdout(&o).contains("[alias: ar]"), "{}", stdout(&o));
    assert!(stdout(&o).contains("[alias: st]"), "{}", stdout(&o));
    let got = complete(&s, &[""]);
    // clap lists each command once, under its visible alias when it has one
    assert!(got.iter().any(|g| g == "st"), "{got:?}");
    assert_exit(&s.run(&["ar", "--help"]), 0);
}
