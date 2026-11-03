//! Shared fixtures for tests that need real git repos.

use std::path::Path;

use crate::config::Config;
use crate::sys;

pub fn git(dir: &Path, args: &[&str]) {
    let ok = sys::quiet(
        sys::git(dir)
            .args([
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args),
    );
    assert!(ok, "git {args:?} failed in {}", dir.display());
}

pub fn commit(dir: &Path, file: &str) {
    std::fs::write(dir.join(file), file).unwrap();
    git(dir, &["add", "."]);
    git(dir, &["commit", "-m", file]);
}

/// A card `1` under `<root>/wt` with two clones, each one commit ahead of its own base:
/// `api` (base `dev`, set by `git.base_branch`) and `sdk` (base `main`, set by `repos.sdk.base`,
/// and it has no `dev` branch at all).
pub fn mixed_bases_card(root: &Path) -> (Config, String) {
    for (repo, base) in [("api", "dev"), ("sdk", "main")] {
        let origin = root.join("origin").join(repo);
        std::fs::create_dir_all(&origin).unwrap();
        git(&origin, &["init", "-q", "-b", "main"]);
        git(&origin, &["config", "receive.denyCurrentBranch", "ignore"]);
        commit(&origin, "a");
        if base == "dev" {
            git(&origin, &["checkout", "-q", "-b", "dev"]);
            commit(&origin, "d");
            git(&origin, &["checkout", "-q", "main"]);
        }
        let wt = root.join("wt").join("1").join(repo);
        std::fs::create_dir_all(wt.parent().unwrap()).unwrap();
        git(
            root,
            &[
                "clone",
                "-q",
                origin.to_str().unwrap(),
                wt.to_str().unwrap(),
            ],
        );
        git(&wt, &["config", "user.name", "t"]);
        git(&wt, &["config", "user.email", "t@t"]);
        git(
            &wt,
            &["checkout", "-q", "-b", "card", &format!("origin/{base}")],
        );
        commit(&wt, "c");
    }
    let toml = format!(
        "version = 1\n[paths]\nworktrees = {:?}\n[git]\nbase_branch = \"dev\"\n\
         [repos.api]\n[repos.sdk]\nbase = \"main\"\n",
        root.join("wt").to_str().unwrap()
    );
    (toml::from_str(&toml).unwrap(), "1".into())
}
