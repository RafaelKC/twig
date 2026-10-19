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
        let mut child = c
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
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

#[test]
fn completions_list_the_open_targets() {
    let s = Sandbox::new();
    let o = s.run(&["completions", "bash"]);
    assert_exit(&o, 0);
    let out = text(&o);
    assert!(
        out.contains("opts=\"-h --help default herdr code\""),
        "{out}"
    );
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
fn open_card_with_gh_provider_fails_clearly() {
    let (s, log) = open_card_sandbox("[provider]\ndefault = \"gh\"\n");
    let o = s.run(&["open-card", "123"]);
    assert_exit(&o, 1);
    assert_has(&o, "not supported yet for provider \"gh\"");
    assert_eq!(opened(&log), "");
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
