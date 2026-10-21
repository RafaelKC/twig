//! Per-card dev server ports, so two cards on the same repo can run at the same time.
//!
//! Deterministic, no allocation state: a repo with `port = <base>` gets `base + offset` in a
//! card, where the offset comes from the card number (`1 + (n - 1) % ports.span`, so card 52644
//! gets 44). The same card always gets the same ports, on any machine and after any `rm`/`new`.
//! A repo can also have named ports (`ports = { https = 5100 }`), `TWIG_PORT_<REPO>_<NAME>`.
//! The ports reach the processes as environment variables (every port of the card, plus `PORT`
//! for the repo's own main one) and are written to `<card>/.twig-ports.env`. Base ports must be
//! at least `ports.span` apart, or two cards would land on the same port.

use std::fs;
use std::net::TcpListener;
use std::path::PathBuf;

use anyhow::Result;

use crate::card::{self, CardRef};
use crate::config::Config;
use crate::ui;

pub const PORTS_FILE: &str = ".twig-ports.env";

/// One port of a card: the repo's main one (`port`, `name = None`) or a named one (`ports`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Port {
    pub repo: String,
    pub name: Option<String>,
    pub port: u16,
}

impl Port {
    pub fn var(&self) -> String {
        var_name(&self.repo, self.name.as_deref())
    }

    /// `repo` or `repo.name`, for messages.
    pub fn label(&self) -> String {
        match &self.name {
            Some(n) => format!("{}.{n}", self.repo),
            None => self.repo.clone(),
        }
    }
}

fn upper(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .collect()
}

/// `TWIG_PORT_<REPO>` or `TWIG_PORT_<REPO>_<NAME>`: upper-cased, non-alphanumerics as `_`.
pub fn var_name(repo: &str, name: Option<&str>) -> String {
    match name {
        Some(n) => format!("TWIG_PORT_{}_{}", upper(repo), upper(n)),
        None => format!("TWIG_PORT_{}", upper(repo)),
    }
}

/// Whether `name` can name a port (`[A-Za-z0-9_]+`, so the variable stays readable).
pub fn valid_name(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// FNV-1a: stable across Rust versions and machines (unlike `DefaultHasher`).
fn stable_hash(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// The card's offset, in `1..=span` (never 0: the base port is the main clone's).
pub fn offset(card: &CardRef, span: u16) -> u16 {
    let span = u64::from(span.max(1));
    let n = card
        .number
        .parse::<u64>()
        .unwrap_or_else(|_| stable_hash(&card.key));
    (1 + n.saturating_sub(1) % span) as u16
}

/// Base ports of a set of repos: `(label, base)`, main port first, then the named ones.
fn bases<'a>(
    repos: impl Iterator<Item = (&'a String, &'a crate::config::Repo)>,
) -> Vec<(String, u16)> {
    let mut out = vec![];
    for (name, r) in repos {
        if let Some(p) = r.port {
            out.push((name.clone(), p));
        }
        out.extend(r.ports.iter().map(|(n, p)| (format!("{name}.{n}"), *p)));
    }
    out
}

/// Base ports closer than `span`: two cards would get the same port from them (card k on the
/// higher one = card k + distance on the lower one). Returns one message per pair.
pub fn base_conflicts(
    repos: &indexmap::IndexMap<String, crate::config::Repo>,
    span: u16,
) -> Vec<String> {
    let mut all = bases(repos.iter());
    all.sort_by_key(|(_, p)| *p);
    let mut out = vec![];
    for (i, (a, pa)) in all.iter().enumerate() {
        for (b, pb) in &all[i + 1..] {
            if pb - pa >= span {
                break;
            }
            out.push(format!(
                "ports {pa} ({a}) and {pb} ({b}) are {} apart, less than ports.span = {span}: cards would share ports (move them at least {span} apart)",
                pb - pa
            ));
        }
    }
    out
}

/// The card's ports, in repo order (main port, then named ones). Only repos with ports.
pub fn of(cfg: &Config, id: &str) -> Vec<Port> {
    let off = offset(&CardRef::of(cfg, id), cfg.ports.span);
    let mut out = vec![];
    for repo in card::repos_ordered(cfg, &cfg.card_dir(id)) {
        let Some(r) = cfg.repo(&repo) else { continue };
        let at = |base: u16| base.saturating_add(off);
        if let Some(p) = r.port {
            out.push(Port {
                repo: repo.clone(),
                name: None,
                port: at(p),
            });
        }
        for (n, p) in &r.ports {
            out.push(Port {
                repo: repo.clone(),
                name: Some(n.clone()),
                port: at(*p),
            });
        }
    }
    out
}

/// Environment for a process of the card: every port's variable, plus `PORT` when the process
/// belongs to a repo with a main `port`.
pub fn env(cfg: &Config, id: &str, repo: Option<&str>) -> Vec<(String, String)> {
    let ports = of(cfg, id);
    let mut env: Vec<(String, String)> = ports
        .iter()
        .map(|p| (p.var(), p.port.to_string()))
        .collect();
    if let Some(p) = repo.and_then(|repo| ports.iter().find(|p| p.repo == repo && p.name.is_none()))
    {
        env.push(("PORT".into(), p.port.to_string()));
    }
    env
}

pub fn file(cfg: &Config, id: &str) -> PathBuf {
    cfg.card_dir(id).join(PORTS_FILE)
}

fn in_use(port: u16) -> bool {
    TcpListener::bind(("127.0.0.1", port)).is_err()
}

/// Write `<card>/.twig-ports.env` (removed when the card has no ports) and warn about ports
/// shared with another card or already taken by some process.
pub fn write(cfg: &Config, id: &str) -> Result<()> {
    let ports = of(cfg, id);
    let path = file(cfg, id);
    if ports.is_empty() {
        let _ = fs::remove_file(&path);
        return Ok(());
    }
    let off = offset(&CardRef::of(cfg, id), cfg.ports.span);
    let mut text = format!(
        "# Ports of card {id} (base port + {off}), generated by twig; `twig refresh` rewrites it.\n"
    );
    for p in &ports {
        text += &format!("{}={}\n", p.var(), p.port);
    }
    fs::write(&path, text)?;

    for other in card::list_cards(cfg).into_iter().filter(|o| o != id) {
        let theirs: Vec<u16> = of(cfg, &other).iter().map(|p| p.port).collect();
        let shared: Vec<String> = ports
            .iter()
            .filter(|p| theirs.contains(&p.port))
            .map(|p| format!("{} {}", p.label(), p.port))
            .collect();
        if !shared.is_empty() {
            ui::warn(format!(
                "card {other} uses the same ports ({}): don't run both at once, or change ports.span",
                shared.join(", ")
            ));
        }
    }
    for p in &ports {
        if in_use(p.port) {
            ui::warn(format!(
                "[{}] port {} is already in use by another process",
                p.label(),
                p.port
            ));
        }
    }
    Ok(())
}

/// `twig ports [id] [--env]`.
pub fn show(cfg: &Config, id: &str, as_env: bool) -> Result<()> {
    card::require_card(cfg, id)?;
    let ports = of(cfg, id);
    if as_env {
        for p in &ports {
            println!("export {}={}", p.var(), p.port);
        }
        return Ok(());
    }
    if ports.is_empty() {
        ui::info(format!(
            "card {id} has no ports (set `port` on a repo: [repos.<name>] port = 4200)"
        ));
        return Ok(());
    }
    let off = offset(&CardRef::of(cfg, id), cfg.ports.span);
    println!("\x1b[1m  #{id}\x1b[0m  \x1b[2mbase port + {off}\x1b[0m");
    for p in &ports {
        let busy = if in_use(p.port) {
            "  \x1b[33min use\x1b[0m"
        } else {
            ""
        };
        println!(
            "      {:<18} {}  \x1b[2m${}\x1b[0m{busy}",
            p.label(),
            p.port,
            p.var()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(key: &str, number: &str) -> CardRef {
        CardRef {
            key: key.into(),
            number: number.into(),
            repo: None,
        }
    }

    #[test]
    fn offset_is_readable_and_never_zero() {
        assert_eq!(offset(&card("52644", "52644"), 100), 44);
        assert_eq!(offset(&card("100", "100"), 100), 100);
        assert_eq!(offset(&card("101", "101"), 100), 1);
        assert_eq!(offset(&card("7", "7"), 10), 7);
        assert_eq!(offset(&card("0", "0"), 100), 1);
    }

    #[test]
    fn non_numeric_ids_get_a_stable_offset() {
        let a = offset(&card("spike", "spike"), 100);
        assert_eq!(a, offset(&card("spike", "spike"), 100));
        assert!((1..=100).contains(&a));
        // FNV-1a is fixed: this must never change between releases
        assert_eq!(stable_hash("spike"), 0x8b2a_5a8a_db9b_8b75);
    }

    #[test]
    fn var_names_are_shell_safe() {
        assert_eq!(var_name("my-back.end", None), "TWIG_PORT_MY_BACK_END");
        assert_eq!(var_name("ui", None), "TWIG_PORT_UI");
        assert_eq!(var_name("my-api", Some("https")), "TWIG_PORT_MY_API_HTTPS");
        assert!(valid_name("debug_1") && !valid_name("a-b") && !valid_name(""));
    }

    fn repo(port: Option<u16>, named: &[(&str, u16)]) -> crate::config::Repo {
        crate::config::Repo {
            port,
            ports: named.iter().map(|(n, p)| (n.to_string(), *p)).collect(),
            ..Default::default()
        }
    }

    #[test]
    fn base_ports_closer_than_span_conflict() {
        let mut repos = indexmap::IndexMap::new();
        repos.insert(
            "api".to_string(),
            repo(Some(5000), &[("https", 5100), ("debug", 9200)]),
        );
        repos.insert("web".to_string(), repo(Some(4200), &[]));
        assert!(base_conflicts(&repos, 100).is_empty());

        repos.insert("book".to_string(), repo(Some(4250), &[]));
        let c = base_conflicts(&repos, 100);
        assert_eq!(c.len(), 1, "{c:?}");
        assert!(
            c[0].contains("4200 (web) and 4250 (book) are 50 apart"),
            "{}",
            c[0]
        );
        assert!(base_conflicts(&repos, 50).is_empty());

        let mut same = indexmap::IndexMap::new();
        same.insert("api".to_string(), repo(Some(5000), &[("https", 5001)]));
        assert!(base_conflicts(&same, 100)[0].contains("(api.https)"));
    }
}
