//! Card data from the provider (Azure DevOps via `az`, GitHub via `gh`).
//! All best-effort: with no CLI, no login or card not found, the flow continues without metadata.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use regex::Regex;
use serde_json::Value;
use unicode_normalization::{char::is_combining_mark, UnicodeNormalization};

use crate::card::CardRef;
use crate::config::{Config, ProviderKind};
use crate::sys;

#[derive(Debug, Default, Clone)]
pub struct CardInfo {
    pub title: String,
    pub url: String,
    pub kind: String,
    pub state: String,
    pub tags: String,
    pub assigned: String,
    pub description: String,
    pub acceptance: String,
    /// Raw JSON returned by the provider; becomes the card folder's cache.
    pub raw: String,
}

impl CardInfo {
    pub fn slug(&self) -> String {
        slugify(&self.title, 40)
    }
}

pub fn cache_file(cfg: &Config, id: &str) -> PathBuf {
    let name = match cfg.provider.default {
        ProviderKind::Az => ".ado.json",
        ProviderKind::Gh => ".gh.json",
    };
    cfg.card_dir(id).join(name)
}

/// Load from the card folder's cache; without a cache, query the provider.
/// `gh_repo_dir` is a clone of the card's repo, from which `gh` infers owner/repo when the card
/// has no `owner/repo` of its own.
pub fn load(cfg: &Config, card: &CardRef, gh_repo_dir: Option<&Path>) -> Option<CardInfo> {
    let cache = cache_file(cfg, &card.key);
    if let Ok(raw) = fs::read_to_string(&cache) {
        if let Some(info) = parse(cfg.provider.default, &raw) {
            return Some(info);
        }
    }
    let raw = fetch(cfg, card, gh_repo_dir)?;
    let info = parse(cfg.provider.default, &raw)?;
    if cache.parent().is_some_and(|d| d.is_dir()) {
        let _ = fs::write(&cache, &info.raw);
    }
    Some(info)
}

pub fn save_cache(cfg: &Config, id: &str, info: &CardInfo) {
    let _ = fs::write(cache_file(cfg, id), &info.raw);
}

pub fn drop_cache(cfg: &Config, id: &str) {
    let _ = fs::remove_file(cache_file(cfg, id));
}

/// Card URL even without metadata: the config's `item_url` (Azure DevOps), or the issue URL
/// of a repo-scoped GitHub card.
pub fn fallback_url(cfg: &Config, card: &CardRef) -> String {
    match (cfg.provider.default, &card.repo) {
        (ProviderKind::Gh, Some(repo)) => {
            format!("https://github.com/{repo}/issues/{}", card.number)
        }
        _ => cfg.provider.az.item_url.replace("{id}", &card.number),
    }
}

fn fetch(cfg: &Config, card: &CardRef, gh_repo_dir: Option<&Path>) -> Option<String> {
    let id = card.number.as_str();
    match cfg.provider.default {
        ProviderKind::Az => {
            sys::which("az")?;
            let mut cmd = Command::new("az");
            cmd.args([
                "boards",
                "work-item",
                "show",
                "--id",
                id,
                "--output",
                "json",
            ]);
            if !cfg.provider.az.organization.is_empty() {
                cmd.args(["--organization", &cfg.provider.az.organization]);
            }
            sys::output(&mut cmd).ok()
        }
        ProviderKind::Gh => {
            sys::which("gh")?;
            let mut cmd = Command::new("gh");
            cmd.args([
                "issue",
                "view",
                id,
                "--json",
                "number,title,url,body,state,labels,assignees",
            ]);
            if let Some(repo) = &card.repo {
                cmd.args(["--repo", repo]);
            } else if let Some(dir) = gh_repo_dir {
                cmd.current_dir(dir);
            }
            sys::output(&mut cmd).ok()
        }
    }
}

fn parse(kind: ProviderKind, raw: &str) -> Option<CardInfo> {
    let d: Value = serde_json::from_str(raw).ok()?;
    let info = match kind {
        ProviderKind::Az => {
            let f = &d["fields"];
            let field = |k: &str| -> String {
                match &f[k] {
                    Value::String(s) => s.clone(),
                    Value::Object(o) => o
                        .get("displayName")
                        .or_else(|| o.get("name"))
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    Value::Null => String::new(),
                    v => v.to_string(),
                }
            };
            let url = d["_links"]["html"]["href"]
                .as_str()
                .or_else(|| d["url"].as_str())
                .unwrap_or_default()
                .to_string();
            CardInfo {
                title: field("System.Title"),
                url,
                kind: field("System.WorkItemType"),
                state: field("System.State"),
                tags: field("System.Tags"),
                assigned: field("System.AssignedTo"),
                description: strip_html(&field("System.Description"), 2500),
                acceptance: strip_html(&field("Microsoft.VSTS.Common.AcceptanceCriteria"), 1200),
                raw: raw.to_string(),
            }
        }
        ProviderKind::Gh => {
            let names = |k: &str, field: &str| -> String {
                d[k].as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|v| v[field].as_str())
                            .collect::<Vec<_>>()
                            .join("; ")
                    })
                    .unwrap_or_default()
            };
            let labels = names("labels", "name");
            let kind = if labels.to_lowercase().contains("bug") {
                "Bug"
            } else {
                "Issue"
            };
            CardInfo {
                title: d["title"].as_str().unwrap_or_default().to_string(),
                url: d["url"].as_str().unwrap_or_default().to_string(),
                kind: kind.to_string(),
                state: d["state"].as_str().unwrap_or_default().to_string(),
                tags: labels,
                assigned: names("assignees", "login"),
                description: truncate(d["body"].as_str().unwrap_or_default().trim(), 2500),
                acceptance: String::new(),
                raw: raw.to_string(),
            }
        }
    };
    (!info.title.is_empty()).then_some(info)
}

fn truncate(t: &str, limit: usize) -> String {
    if t.chars().count() <= limit {
        return t.to_string();
    }
    let cut: String = t.chars().take(limit).collect();
    let cut = cut
        .rsplit_once('\n')
        .map(|(a, _)| a.to_string())
        .unwrap_or(cut);
    format!("{cut}\n\n_(truncated)_")
}

pub fn strip_html(raw: &str, limit: usize) -> String {
    if raw.is_empty() {
        return String::new();
    }
    let re = |p: &str| Regex::new(p).unwrap();
    let mut t = re(r"(?i)<\s*(br|/p|/div|/tr|/h[1-6])\s*/?\s*>")
        .replace_all(raw, "\n")
        .into_owned();
    t = re(r"(?i)<\s*li[^>]*>").replace_all(&t, "\n- ").into_owned();
    t = re(r"(?i)<\s*(td|th)[^>]*>")
        .replace_all(&t, " | ")
        .into_owned();
    t = re(r"<[^>]+>").replace_all(&t, "").into_owned();
    t = unescape_html(&t).replace('\u{a0}', " ");
    t = re(r"[ \t]+").replace_all(&t, " ").into_owned();
    t = re(r"\n\s*\n\s*\n+").replace_all(&t, "\n\n").into_owned();
    let t = t.lines().map(str::trim_end).collect::<Vec<_>>().join("\n");
    truncate(t.trim(), limit)
}

fn unescape_html(s: &str) -> String {
    let re = Regex::new(r"&(#x[0-9a-fA-F]+|#[0-9]+|[a-zA-Z]+);").unwrap();
    re.replace_all(s, |c: &regex::Captures| {
        let e = &c[1];
        let ch = if let Some(hex) = e.strip_prefix("#x").or_else(|| e.strip_prefix("#X")) {
            u32::from_str_radix(hex, 16).ok().and_then(char::from_u32)
        } else if let Some(dec) = e.strip_prefix('#') {
            dec.parse().ok().and_then(char::from_u32)
        } else {
            match e {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                "nbsp" => Some('\u{a0}'),
                _ => None,
            }
        };
        ch.map(String::from).unwrap_or_else(|| c[0].to_string())
    })
    .into_owned()
}

pub fn slugify(text: &str, maxlen: usize) -> String {
    let plain: String = text
        .nfkd()
        .filter(|c| !is_combining_mark(*c))
        .collect::<String>()
        .to_lowercase();
    let slug = Regex::new(r"[^a-z0-9]+").unwrap().replace_all(&plain, "-");
    let mut slug = slug.trim_matches('-').to_string();
    if slug.len() > maxlen {
        slug.truncate(maxlen);
        if let Some((head, _)) = slug.rsplit_once('-') {
            slug = head.to_string();
        }
    }
    slug.trim_matches('-').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_strips_accents_and_cuts_at_word() {
        assert_eq!(
            slugify("Aferição de Pneus: ajuste", 40),
            "afericao-de-pneus-ajuste"
        );
        assert_eq!(
            slugify("um titulo bem comprido que passa do limite de quarenta", 40),
            "um-titulo-bem-comprido-que-passa-do"
        );
    }

    #[test]
    fn html_becomes_text() {
        let t = strip_html(
            "<div>Oi&nbsp;<b>mundo</b></div><ul><li>a</li><li>b</li></ul>",
            100,
        );
        assert_eq!(t, "Oi mundo\n\n- a\n- b");
    }
}
