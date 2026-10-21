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
    fn names() {
        assert!(valid_name("Cors__Origins__0") && valid_name("_X"));
        assert!(!valid_name("") && !valid_name("1A") && !valid_name("A-B"));
    }
}
