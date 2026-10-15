//! Integration tests: run the binary with controlled PATH, HOME and TWIG_CONFIG.
//! External tools (herdr, az, claude, code, editor) are fake scripts.
#![cfg(unix)]

use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::io::Write;

use tempfile::TempDir;

struct Sandbox {
    _root: TempDir,
    bin: PathBuf,
    home: PathBuf,
    config: PathBuf,
}

fn real(bin: &str) -> PathBuf {
    let out = Command::new("sh").args(["-c", &format!("command -v {bin}")]).output().unwrap();
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
        Self { bin, home, config, _root: root }
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
        self.stub("herdr", "exit 0").stub("az", "exit 0").stub("claude", "exit 0").stub("code", "exit 0")
    }

    fn git_identity(&self) -> &Self {
        fs::write(self.home.join(".gitconfig"), "[user]\n\tname = Teste\n\temail = t@t\n").unwrap();
        self
    }

    fn git_repo(&self, rel: &str) -> PathBuf {
        let p = self.path(rel);
        fs::create_dir_all(&p).unwrap();
        assert!(Command::new("git").args(["init", "-q"]).current_dir(&p).status().unwrap().success());
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
        let mut child = c.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
        child.stdin.take().unwrap().write_all(stdin.as_bytes()).unwrap();
        child.wait_with_output().unwrap()
    }
}

fn text(o: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

fn assert_exit(o: &Output, code: i32) {
    assert_eq!(o.status.code(), Some(code), "saída:\n{}", text(o));
}

fn assert_has(o: &Output, needle: &str) {
    assert!(text(o).contains(needle), "esperava {needle:?} na saída:\n{}", text(o));
}

// ───────────────────────── doctor ─────────────────────────

#[test]
fn doctor_tudo_certo() {
    let s = Sandbox::new();
    s.all_tools().git_identity();
    s.valid_config("");
    let o = s.run(&["doctor"]);
    assert_exit(&o, 0);
    assert_has(&o, "tudo certo");
}

#[test]
fn doctor_sem_herdr_e_erro() {
    let s = Sandbox::new();
    s.stub("az", "exit 0").stub("claude", "exit 0").stub("code", "exit 0").git_identity();
    s.valid_config("");
    let o = s.run(&["doctor"]);
    assert_exit(&o, 1);
    assert_has(&o, "herdr não encontrado no PATH");
}

#[test]
fn doctor_sem_acesso_ao_ado_e_aviso() {
    let s = Sandbox::new();
    s.all_tools().git_identity();
    s.stub("az", "case \"$*\" in *\"devops project\"*) exit 1;; esac\nexit 0");
    s.valid_config("");
    let o = s.run(&["doctor"]);
    assert_exit(&o, 0);
    assert_has(&o, "az sem acesso ao Azure DevOps");
}

#[test]
fn doctor_sem_identidade_do_git_e_aviso() {
    let s = Sandbox::new();
    s.all_tools();
    s.valid_config("");
    let o = s.run(&["doctor"]);
    assert_exit(&o, 0);
    assert_has(&o, "user.name não configurado");
    assert_has(&o, "user.email não configurado");
}

#[test]
fn doctor_config_invalido_e_erro() {
    let s = Sandbox::new();
    s.all_tools().git_identity();
    s.write_config("version = 1\n[vscode]\nativo = true\n");
    let o = s.run(&["doctor"]);
    assert_exit(&o, 1);
    assert_has(&o, "config inválido");
}

#[test]
fn doctor_vscode_desativado_dispensa_code() {
    let s = Sandbox::new();
    s.stub("herdr", "exit 0").stub("az", "exit 0").stub("claude", "exit 0").git_identity();
    s.valid_config("[vscode]\nenabled = false\n");
    let o = s.run(&["doctor"]);
    assert_exit(&o, 0);
    assert_has(&o, "desativado no config");
}

#[test]
fn doctor_repo_que_nao_e_git_e_erro() {
    let s = Sandbox::new();
    s.all_tools().git_identity();
    s.valid_config("");
    fs::create_dir_all(s.path("solto")).unwrap();
    let mut cfg = fs::read_to_string(&s.config).unwrap();
    cfg += &format!("\n[repos.solto]\npath = \"{}\"\n", s.path("solto").display());
    fs::write(&s.config, cfg).unwrap();
    let o = s.run(&["doctor"]);
    assert_exit(&o, 1);
    assert_has(&o, "solto: clone não encontrado");
}

// ───────────────────────── config edit ─────────────────────────

fn edit(s: &Sandbox, editor: &Path, stdin: &str) -> Output {
    let mut c = s.cmd(&["config", "edit"]);
    c.env("EDITOR", editor);
    s.run_with(c, stdin)
}

#[test]
fn config_edit_sem_mudanca_e_ok() {
    let s = Sandbox::new();
    s.valid_config("");
    let o = edit(&s, &s.script("noop.sh", "exit 0"), "");
    assert_exit(&o, 0);
    assert_has(&o, "config válido");
}

#[test]
fn config_edit_invalido_e_recusa_reabrir() {
    let s = Sandbox::new();
    s.valid_config("");
    let o = edit(&s, &s.script("estraga.sh", "echo 'lixo =' >> \"$1\""), "n\n");
    assert_exit(&o, 1);
    assert_has(&o, "config ficou inválido");
}

#[test]
fn config_edit_reabre_ate_corrigir() {
    let s = Sandbox::new();
    s.valid_config("");
    let original = fs::read_to_string(&s.config).unwrap();
    let backup = s.path("backup.toml");
    fs::write(&backup, original).unwrap();
    let marker = s.path("ja-estragou");
    // 1st run breaks it, 2nd run restores the original
    let editor = s.script(
        "ida-e-volta.sh",
        &format!(
            "if [ -f {m} ]; then {cat} {b} > \"$1\"; else : > {m}; echo 'lixo =' >> \"$1\"; fi",
            m = marker.display(),
            b = backup.display(),
            cat = real("cat").display()
        ),
    );
    let o = edit(&s, &editor, "y\n");
    assert_exit(&o, 0);
    assert_has(&o, "reabrir para corrigir");
    assert_has(&o, "config válido");
}

#[test]
fn config_edit_cria_modelo_quando_nao_existe() {
    let s = Sandbox::new();
    let o = edit(&s, &s.script("noop.sh", "exit 0"), "");
    assert_exit(&o, 0);
    assert_has(&o, "config criado a partir do modelo");
    assert!(fs::read_to_string(&s.config).unwrap().contains("version = 1"));
}

#[test]
fn config_edit_prefere_visual_a_editor() {
    let s = Sandbox::new();
    s.valid_config("");
    let marker = s.path("quem-abriu");
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
    fs::read_to_string(log).unwrap_or_default().trim().to_string()
}

#[test]
fn code_com_id_abre_o_workspace_do_card() {
    let (s, log) = code_sandbox();
    let o = s.run(&["code", "123"]);
    assert_exit(&o, 0);
    assert_eq!(opened(&log), s.path("wt/123/123.code-workspace").display().to_string());
    assert!(s.path("wt/123/123.code-workspace").exists());
}

#[test]
fn code_sem_id_usa_label_do_workspace_do_herdr() {
    let (s, log) = code_sandbox();
    let mut c = s.cmd(&["code"]);
    c.env("HERDR_PLUGIN_CONTEXT_JSON", r#"{"workspace_label":"123"}"#);
    let o = s.run_with(c, "");
    assert_exit(&o, 0);
    assert_eq!(opened(&log), s.path("wt/123/123.code-workspace").display().to_string());
}

#[test]
fn code_sem_id_usa_cwd_dentro_do_card() {
    let (s, log) = code_sandbox();
    let mut c = s.cmd(&["code"]);
    c.current_dir(s.path("wt/123/r1"));
    let o = s.run_with(c, "");
    assert_exit(&o, 0);
    assert_eq!(opened(&log), s.path("wt/123/123.code-workspace").display().to_string());
}

#[test]
fn code_fora_de_card_abre_a_pasta() {
    let (s, log) = code_sandbox();
    fs::create_dir_all(s.path("outro")).unwrap();
    let mut c = s.cmd(&["code"]);
    let ctx = format!(r#"{{"workspace_label":"xyz","focused_pane_cwd":"{}"}}"#, s.path("outro").display());
    c.env("HERDR_PLUGIN_CONTEXT_JSON", ctx);
    let o = s.run_with(c, "");
    assert_exit(&o, 0);
    assert_eq!(opened(&log), s.path("outro").display().to_string());
}

#[test]
fn plugin_do_herdr_abre_o_card_focado() {
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
    assert_eq!(opened(&log), s.path("wt/123/123.code-workspace").display().to_string());
}
