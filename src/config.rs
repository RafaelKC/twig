use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use indexmap::IndexMap;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub version: u32,
    // `[ai]` is the old name of the section, still accepted
    #[serde(default, rename = "agent", alias = "ai")]
    pub ai: Ai,
    #[serde(default)]
    pub provider: Provider,
    #[serde(default)]
    pub vscode: Vscode,
    #[serde(default)]
    pub herdr: Herdr,
    #[serde(default)]
    pub open: Open,
    #[serde(default)]
    pub paths: Paths,
    #[serde(default)]
    pub git: Git,
    #[serde(default)]
    pub worktree: Worktree,
    #[serde(default)]
    pub update: Update,
    #[serde(default)]
    pub repos: IndexMap<String, Repo>,

    #[serde(skip)]
    pub file: PathBuf,
}

/// `[update]`: the passive "new version available" hint.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Update {
    pub check: bool,
    pub check_interval_hours: u64,
}

impl Default for Update {
    fn default() -> Self {
        Self {
            check: true,
            check_interval_hours: 24,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Ai {
    pub provider: AgentKind,
    /// empty = the provider's default command
    command: String,
    /// empty = the provider's default context file
    context_file: String,
}

impl Default for Ai {
    fn default() -> Self {
        Self {
            provider: AgentKind::Claude,
            command: String::new(),
            context_file: String::new(),
        }
    }
}

impl Ai {
    /// What `twig herdr` runs in the agent tab.
    pub fn command(&self) -> &str {
        if self.command.is_empty() {
            self.provider.default_command()
        } else {
            &self.command
        }
    }

    /// Name of the card's context file (CLAUDE.md, AGENTS.md, or the configured override).
    pub fn context_file(&self) -> &str {
        if self.context_file.is_empty() {
            self.provider.default_context_file()
        } else {
            &self.context_file
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AgentKind {
    #[default]
    Claude,
    Codex,
}

impl AgentKind {
    pub fn default_command(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
        }
    }

    pub fn default_context_file(self) -> &'static str {
        match self {
            Self::Claude => "CLAUDE.md",
            Self::Codex => "AGENTS.md",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ProviderKind {
    #[default]
    Az,
    Gh,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Provider {
    pub default: ProviderKind,
    pub az: Az,
    pub gh: Gh,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Az {
    pub organization: String,
    pub project: String,
    pub item_url: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Gh {
    pub owner: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Vscode {
    pub enabled: bool,
    pub open_on_new: bool,
}

impl Default for Vscode {
    fn default() -> Self {
        Self {
            enabled: true,
            open_on_new: true,
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Herdr {
    pub open_on_new: bool,
}

/// Where `twig open` goes when no target is given.
#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum OpenTarget {
    #[default]
    Herdr,
    Code,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Open {
    pub default: OpenTarget,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Paths {
    pub worktrees: String,
    pub repos: String,
    pub search: Vec<String>,
}

impl Default for Paths {
    fn default() -> Self {
        Self {
            worktrees: "~/.wt".into(),
            repos: "~".into(),
            search: vec![],
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Git {
    pub base_branch: String,
    pub branch_prefix: HashMap<String, String>,
    /// How `twig sync` brings the base branch in.
    pub sync: SyncMode,
}

/// `twig sync` strategy.
#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SyncMode {
    #[default]
    Rebase,
    Merge,
}

impl Default for Git {
    fn default() -> Self {
        Self {
            base_branch: "main".into(),
            branch_prefix: HashMap::new(),
            sync: SyncMode::default(),
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Worktree {
    pub local_files: Vec<String>,
    /// Run each repo's `install` (then `post_create`) automatically on new worktrees.
    pub auto_install: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Repo {
    pub path: String,
    pub label: String,
    pub short: String,
    pub default: bool,
    pub run: String,
    pub autorun: bool,
    pub install: String,
    pub post_create: Vec<String>,
    pub debug: Vec<String>,
    pub local_files: Vec<String>,
}

impl Default for Repo {
    fn default() -> Self {
        Self {
            path: String::new(),
            label: String::new(),
            short: String::new(),
            default: false,
            run: String::new(),
            autorun: true,
            install: String::new(),
            post_create: vec![],
            debug: vec![],
            local_files: vec![],
        }
    }
}

pub fn config_path() -> Result<PathBuf> {
    if let Ok(p) = std::env::var("TWIG_CONFIG") {
        return Ok(PathBuf::from(p));
    }
    let base = if cfg!(windows) {
        dirs::config_dir().context("user config folder not found")?
    } else {
        home()?.join(".config")
    };
    Ok(base.join("twig").join("config.toml"))
}

pub fn home() -> Result<PathBuf> {
    dirs::home_dir().context("user home not found")
}

/// Expand a leading `~` in the path.
pub fn expand(p: &str) -> PathBuf {
    if p == "~" {
        return home().unwrap_or_default();
    }
    if let Some(rest) = p.strip_prefix("~/").or_else(|| p.strip_prefix("~\\")) {
        return home().unwrap_or_default().join(rest);
    }
    PathBuf::from(p)
}

fn is_git_repo(p: &Path) -> bool {
    p.join(".git").exists()
}

impl Config {
    pub fn load() -> Result<Self> {
        Self::load_from(&config_path()?)
    }

    pub fn load_from(file: &Path) -> Result<Self> {
        let file = file.to_path_buf();
        let text = fs::read_to_string(&file).with_context(|| {
            format!(
                "config not found at {} (see docs/CONFIG.md; you can point to another one with TWIG_CONFIG)",
                file.display()
            )
        })?;
        let mut cfg: Config =
            toml::from_str(&text).with_context(|| format!("invalid config: {}", file.display()))?;
        if cfg.version != 1 {
            bail!("unsupported config version: {} (expected 1)", cfg.version);
        }
        let cf = cfg.ai.context_file();
        if cf.contains(['/', '\\']) {
            bail!("agent.context_file must be a file name, not a path: {cf}");
        }
        cfg.file = file;
        Ok(cfg)
    }

    pub fn worktrees(&self) -> PathBuf {
        expand(&self.paths.worktrees)
    }

    pub fn card_dir(&self, id: &str) -> PathBuf {
        self.worktrees().join(id)
    }

    pub fn workspace_file(&self, id: &str) -> PathBuf {
        self.card_dir(id).join(format!("{id}.code-workspace"))
    }

    pub fn repo(&self, name: &str) -> Option<&Repo> {
        self.repos.get(name)
    }

    pub fn label(&self, name: &str) -> String {
        match self.repo(name) {
            Some(r) if !r.label.is_empty() => r.label.clone(),
            _ => name.to_string(),
        }
    }

    pub fn short(&self, name: &str) -> String {
        match self.repo(name) {
            Some(r) if !r.short.is_empty() => r.short.clone(),
            _ => name.to_string(),
        }
    }

    pub fn default_repos(&self) -> Vec<String> {
        self.repos
            .iter()
            .filter(|(_, r)| r.default)
            .map(|(n, _)| n.clone())
            .collect()
    }

    /// Folders searched, in order, for a repo without `path`.
    pub fn search_roots(&self) -> Vec<PathBuf> {
        let mut roots = vec![expand(&self.paths.repos)];
        roots.extend(self.paths.search.iter().map(|s| expand(s)));
        roots
    }

    /// Repos to inspect during maintenance: those in the config plus those found in existing cards.
    pub fn known_repos(&self) -> Vec<String> {
        let mut repos: Vec<String> = self.repos.keys().cloned().collect();
        for id in crate::card::list_cards(self) {
            for r in crate::card::repos_in(&self.card_dir(&id)) {
                if !repos.contains(&r) {
                    repos.push(r);
                }
            }
        }
        repos
    }

    /// A repo's main clone: `path` from the config, or the first search folder that has the repo.
    pub fn repo_path(&self, name: &str) -> Option<PathBuf> {
        if let Some(r) = self.repo(name) {
            if !r.path.is_empty() {
                let p = expand(&r.path);
                return is_git_repo(&p).then_some(p);
            }
        }
        self.search_roots()
            .into_iter()
            .map(|b| b.join(name))
            .find(|p| is_git_repo(p))
    }

    pub fn branch_prefix(&self, work_item_type: &str) -> String {
        self.git
            .branch_prefix
            .get(work_item_type)
            .or_else(|| self.git.branch_prefix.get("default"))
            .cloned()
            .unwrap_or_else(|| "feat".into())
    }

    /// Register a new repo in the config file, preserving comments and formatting.
    /// `path` is only written when the clone lives outside `paths.repos`.
    pub fn register_repo(&mut self, name: &str, clone: &Path) -> Result<()> {
        let text = fs::read_to_string(&self.file)?;
        let mut doc: toml_edit::DocumentMut = text.parse()?;

        let in_root = clone == expand(&self.paths.repos).join(name);
        let mut table = toml_edit::Table::new();
        if !in_root {
            table["path"] = toml_edit::value(clone.display().to_string());
        }
        if doc.get("repos").is_none() {
            let mut repos = toml_edit::Table::new();
            repos.set_implicit(true);
            doc["repos"] = toml_edit::Item::Table(repos);
        }
        doc["repos"][name] = toml_edit::Item::Table(table);
        fs::write(&self.file, doc.to_string())
            .with_context(|| format!("could not write {}", self.file.display()))?;

        let repo = Repo {
            path: if in_root {
                String::new()
            } else {
                clone.display().to_string()
            },
            ..Default::default()
        };
        self.repos.insert(name.to_string(), repo);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(ai: &str) -> Result<Config, toml::de::Error> {
        toml::from_str(&format!("version = 1\n{ai}"))
    }

    #[test]
    fn claude_is_the_default() {
        let cfg = parse("").unwrap();
        assert_eq!(cfg.ai.command(), "claude");
        assert_eq!(cfg.ai.context_file(), "CLAUDE.md");
    }

    #[test]
    fn codex_derives_command_and_context_file() {
        let cfg = parse("[agent]\nprovider = \"codex\"").unwrap();
        assert_eq!(cfg.ai.command(), "codex");
        assert_eq!(cfg.ai.context_file(), "AGENTS.md");
    }

    #[test]
    fn command_and_context_file_can_be_overridden() {
        let cfg = parse("[agent]\nprovider = \"codex\"\ncommand = \"codex --full-auto\"\ncontext_file = \"NOTES.md\"").unwrap();
        assert_eq!(cfg.ai.command(), "codex --full-auto");
        assert_eq!(cfg.ai.context_file(), "NOTES.md");
    }

    #[test]
    fn old_ai_section_name_still_works() {
        let cfg = parse("[ai]\nprovider = \"claude\"\ncommand = \"claude\"").unwrap();
        assert_eq!(cfg.ai.provider, AgentKind::Claude);
    }

    #[test]
    fn git_sync_defaults_to_rebase_and_is_validated() {
        assert_eq!(parse("").unwrap().git.sync, SyncMode::Rebase);
        assert_eq!(
            parse("[git]\nsync = \"merge\"").unwrap().git.sync,
            SyncMode::Merge
        );
        let err = parse("[git]\nsync = \"squash\"").unwrap_err().to_string();
        assert!(err.contains("rebase") && err.contains("merge"), "{err}");
    }

    #[test]
    fn unknown_agent_is_rejected_with_the_valid_options() {
        let err = parse("[agent]\nprovider = \"gemini\"")
            .unwrap_err()
            .to_string();
        assert!(err.contains("claude") && err.contains("codex"), "{err}");
    }
}
