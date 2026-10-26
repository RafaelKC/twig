//! Environment of the processes twig starts for a card (herdr panes, setup, hooks, VS Code
//! tasks): the card's ports (`ports.rs`) plus the repo's `env` table, whose values may use
//! `$VAR`, `${VAR}` and `${VAR:-default}`.

use crate::config::Config;
use crate::ports;

/// Whether `name` can be an environment variable (`[A-Za-z_][A-Za-z0-9_]*`).
pub fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Expand `$VAR`, `${VAR}` and `${VAR:-default}` with `lookup`; `$$` is a literal `$`. An
/// unknown variable without a default expands to nothing, as in the shell.
pub fn expand(text: &str, lookup: impl Fn(&str) -> Option<String>) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(i) = rest.find('$') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        if let Some(tail) = after.strip_prefix('$') {
            out.push('$');
            rest = tail;
        } else if let Some(inner) = after.strip_prefix('{') {
            let Some(end) = inner.find('}') else {
                out.push_str(&rest[i..]);
                return out;
            };
            let (name, default) = match inner[..end].split_once(":-") {
                Some((n, d)) => (n, Some(d)),
                None => (&inner[..end], None),
            };
            match lookup(name).filter(|v| !v.is_empty()) {
                Some(v) => out.push_str(&v),
                None => out.push_str(default.unwrap_or_default()),
            }
            rest = &inner[end + 1..];
        } else {
            let len = after
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .unwrap_or(after.len());
            if len == 0 {
                out.push('$');
            } else {
                out.push_str(&lookup(&after[..len]).unwrap_or_default());
            }
            rest = &after[len..];
        }
    }
    out.push_str(rest);
    out
}

/// Environment for a process of the card: the ports, then (for a repo) its `env` entries,
/// expanded against the ports, the earlier entries, `TWIG_CARD` and the current environment.
pub fn of(cfg: &Config, id: &str, repo: Option<&str>) -> Vec<(String, String)> {
    let mut vars = ports::env(cfg, id, repo);
    let Some(r) = repo.and_then(|r| cfg.repo(r)) else {
        return vars;
    };
    for (k, v) in &r.env {
        let value = expand(v, |name| {
            if name == "TWIG_CARD" {
                return Some(id.to_string());
            }
            vars.iter()
                .rev()
                .find(|(k, _)| k == name)
                .map(|(_, v)| v.clone())
                .or_else(|| std::env::var(name).ok())
        });
        vars.retain(|(old, _)| old != k);
        vars.push((k.clone(), value));
    }
    vars
}

/// Environment of a herdr pane (and of `twig env`): `of` plus the markers that tell a pane
/// twig set up from a shell herdr restored without the environment (`TWIG_CARD_ID`, `TWIG_REPO`).
pub fn pane(cfg: &Config, id: &str, repo: Option<&str>) -> Vec<(String, String)> {
    let mut vars = vec![("TWIG_CARD_ID".to_string(), id.to_string())];
    if let Some(r) = repo {
        vars.push(("TWIG_REPO".to_string(), r.to_string()));
    }
    vars.extend(of(cfg, id, repo));
    vars
}

/// Quote for a POSIX shell (`'` closed, escaped and reopened inside).
fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// `twig env [id] [--repo X] [--auto]`: `export` lines with everything a pane of the card
/// receives (ports, `PORT`, the repo's `env` with every config layer, the markers).
///
/// `auto` is for the shell rc hook: it prints nothing, and never fails, unless the current
/// directory is inside a card and this shell has no twig environment yet.
pub fn show(cfg: &Config, id: Option<&str>, repo: Option<&str>, auto: bool) -> anyhow::Result<()> {
    let id = match id {
        Some(id) => id.to_string(),
        None => match crate::card::infer(cfg) {
            crate::card::Current::Card(id) => id,
            crate::card::Current::Dir(_) if auto => return Ok(()),
            crate::card::Current::Dir(dir) => anyhow::bail!(
                "{} is not inside a card; usage: twig env <id>",
                dir.display()
            ),
        },
    };
    if auto && std::env::var_os("TWIG_CARD_ID").is_some() {
        return Ok(());
    }
    if auto && crate::card::require_card(cfg, &id).is_err() {
        return Ok(());
    }
    crate::card::require_card(cfg, &id)?;
    let here;
    let repo = match repo {
        Some(r) if auto && cfg.repo(r).is_none() => return Ok(()),
        Some(r) if cfg.repo(r).is_none() => anyhow::bail!("repo {r} is not in the config"),
        Some(r) => Some(r),
        None => {
            here = crate::card::repo_here(cfg, &id);
            here.as_deref()
        }
    };
    for (k, v) in pane(cfg, &id, repo) {
        println!("export {k}={}", sh_quote(&v));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn look(name: &str) -> Option<String> {
        match name {
            "PORT" => Some("4244".into()),
            "EMPTY" => Some(String::new()),
            _ => None,
        }
    }

    #[test]
    fn expands_the_shell_forms() {
        assert_eq!(
            expand("http://localhost:$PORT/x", look),
            "http://localhost:4244/x"
        );
        assert_eq!(expand("${PORT}0", look), "42440");
        assert_eq!(expand("${API:-5000}", look), "5000");
        assert_eq!(expand("${PORT:-5000}", look), "4244");
        assert_eq!(expand("${EMPTY:-d}", look), "d");
        assert_eq!(expand("a$MISSING-b", look), "a-b");
        assert_eq!(expand("cost $$5 and $ alone", look), "cost $5 and $ alone");
        assert_eq!(expand("${unclosed", look), "${unclosed");
    }

    #[test]
    fn quotes_for_the_shell() {
        assert_eq!(sh_quote("a b"), "'a b'");
        assert_eq!(sh_quote("it's $x"), r"'it'\''s $x'");
    }

    #[test]
    fn pane_env_starts_with_the_markers() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("config.toml");
        std::fs::write(
            &file,
            "version = 1\n[repos.api]\nport = 5000\n[repos.api.env]\nMODE = \"dev-$PORT\"\n",
        )
        .unwrap();
        let cfg = Config::load_from(&file).unwrap();
        let get = |v: &[(String, String)], k: &str| {
            v.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone())
        };
        let v = pane(&cfg, "7", Some("api"));
        assert_eq!(get(&v, "TWIG_CARD_ID").as_deref(), Some("7"));
        assert_eq!(get(&v, "TWIG_REPO").as_deref(), Some("api"));
        let v = pane(&cfg, "7", None);
        assert_eq!(get(&v, "TWIG_REPO"), None);
    }

    #[test]
    fn names() {
        assert!(valid_name("Cors__Origins__0") && valid_name("_X"));
        assert!(!valid_name("") && !valid_name("1A") && !valid_name("A-B"));
    }
}
