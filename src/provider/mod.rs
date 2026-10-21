//! Card and pull request data from the work item provider. Each provider (Azure DevOps via
//! `az`, GitHub via `gh`) implements [`Provider`]; the rest of twig only talks to the trait
//! through [`current`]. Card data is best-effort: with no CLI, no login or card not found, the
//! flow continues without metadata.
//!
//! Adding a provider: a module implementing [`Provider`], a [`ProviderKind`] variant, and a
//! line in [`current`].

mod az;
mod gh;

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;
use regex::Regex;
use unicode_normalization::{char::is_combining_mark, UnicodeNormalization};

use crate::card::CardRef;
use crate::config::{Config, ProviderKind};
use crate::doctor::Report;

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

/// A pull request of one repo of the card.
#[derive(Debug, Clone)]
pub struct Pr {
    /// `gh`: the PR number; `az`: the pull request id.
    pub number: String,
    pub url: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrState {
    Open,
    Merged,
}

/// What `twig pr` asks the provider to open.
pub struct NewPr<'a> {
    pub branch: &'a str,
    pub base: &'a str,
    pub title: &'a str,
    pub body: &'a str,
    pub card: &'a CardRef,
    pub draft: bool,
}

/// One work item / repository provider. PR methods run inside a worktree (`wt`), so the
/// provider's CLI can detect the repository from `origin`.
pub trait Provider {
    /// The provider's CLI (`az`, `gh`).
    fn cli(&self) -> &'static str;

    /// File name of the card data cache in the card folder.
    fn cache_name(&self) -> &'static str;

    /// A provider-specific card id (e.g. GitHub's `owner/repo#123`): `None` when `raw` isn't
    /// one, so the generic `123` / `123-slug` parsing applies.
    fn parse_id(&self, _raw: &str) -> Option<Result<CardRef>> {
        None
    }

    /// The card's raw data, or `None` when it can't be fetched. `repo_dir` is a clone of one of
    /// the card's repos, for providers that infer the project from it.
    fn fetch_card(&self, card: &CardRef, repo_dir: Option<&Path>) -> Option<String>;

    /// Raw data (from [`Provider::fetch_card`] or the cache) as a card.
    fn parse_card(&self, raw: &str) -> Option<CardInfo>;

    /// The card URL without metadata (may be empty).
    fn fallback_url(&self, card: &CardRef) -> String;

    /// Whether a work item / issue state means the work is over.
    fn is_finished(&self, state: &str) -> bool;

    /// The bullet of the context file that tells the agent how to query the provider.
    fn context_hint(&self, card: &CardRef) -> String;

    /// Extra lines of a PR description that link it to the card (e.g. `Closes owner/repo#N`).
    fn pr_card_link(&self, _card: &CardRef, _info: Option<&CardInfo>) -> Option<String> {
        None
    }

    /// The PR of `branch` in the given state (best effort: `None` when it can't tell).
    fn find_pr(&self, wt: &Path, branch: &str, state: PrState) -> Option<Pr>;

    fn create_pr(&self, wt: &Path, pr: &NewPr) -> Result<Pr>;

    fn update_pr_body(&self, wt: &Path, pr: &Pr, body: &str) -> Result<()>;

    /// `twig doctor`: CLI, extensions, login.
    fn doctor(&self, r: &mut Report);
}

/// The provider chosen in the config (`provider.default`).
pub fn current(cfg: &Config) -> Box<dyn Provider + '_> {
    match cfg.provider.default {
        ProviderKind::Az => Box::new(az::AzureDevOps::new(&cfg.provider.az)),
        ProviderKind::Gh => Box::new(gh::GitHub::new(&cfg.provider.gh)),
    }
}

pub fn cache_file(cfg: &Config, id: &str) -> PathBuf {
    cfg.card_dir(id).join(current(cfg).cache_name())
}

/// Load from the card folder's cache; without a cache, query the provider.
/// `repo_dir` is a clone of the card's repo, from which `gh` infers owner/repo when the card
/// has no `owner/repo` of its own.
pub fn load(cfg: &Config, card: &CardRef, repo_dir: Option<&Path>) -> Option<CardInfo> {
    let p = current(cfg);
    let cache = cache_file(cfg, &card.key);
    if let Ok(raw) = fs::read_to_string(&cache) {
        if let Some(info) = p.parse_card(&raw) {
            return Some(info);
        }
    }
    let info = p.parse_card(&p.fetch_card(card, repo_dir)?)?;
    if cache.parent().is_some_and(|d| d.is_dir()) {
        let _ = fs::write(&cache, &info.raw);
    }
    Some(info)
}

/// Query the provider again (ignoring the cache) and refresh the cache; `None` when it can't.
pub fn refetch(cfg: &Config, card: &CardRef, repo_dir: Option<&Path>) -> Option<CardInfo> {
    let p = current(cfg);
    let info = p.parse_card(&p.fetch_card(card, repo_dir)?)?;
    save_cache(cfg, &card.key, &info);
    Some(info)
}

pub fn save_cache(cfg: &Config, id: &str, info: &CardInfo) {
    let _ = fs::write(cache_file(cfg, id), &info.raw);
}

pub fn drop_cache(cfg: &Config, id: &str) {
    let _ = fs::remove_file(cache_file(cfg, id));
}

pub(crate) fn truncate(t: &str, limit: usize) -> String {
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
