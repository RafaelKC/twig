//! Layout of the panes of a herdr tab (`[herdr.layout]`): presets and the splits that build them.
//! Pure planning; `herdr.rs` runs the splits.

use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;

/// How the panes of a tab are arranged.
#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq, clap::ValueEnum)]
#[serde(rename_all = "kebab-case")]
#[value(rename_all = "kebab-case")]
pub enum Preset {
    /// All side by side, equal widths.
    Columns,
    /// All stacked, equal heights.
    Rows,
    /// First repo in a full-height column, the others stacked on the right (the original layout).
    #[default]
    MainLeft,
    /// First repo on top, the others in columns below.
    MainTop,
    /// Two columns, as many rows as needed.
    Grid,
}

impl Preset {
    pub fn name(self) -> &'static str {
        match self {
            Preset::Columns => "columns",
            Preset::Rows => "rows",
            Preset::MainLeft => "main-left",
            Preset::MainTop => "main-top",
            Preset::Grid => "grid",
        }
    }
}

/// `[herdr.layout]`: the preset of each tab and the order of the panes.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, default)]
pub struct Layout {
    pub agent: Option<Preset>,
    pub run: Option<Preset>,
    /// Repos in the order of the panes; the others follow in the card's order.
    pub order: Vec<String>,
}

impl Layout {
    /// The tab's preset (`agent` or `run`); `main-left` when unset.
    pub fn preset(&self, tab: &str) -> Preset {
        match tab {
            "run" => self.run,
            _ => self.agent,
        }
        .unwrap_or_default()
    }

    /// Keys set in `top` replace the ones here.
    pub fn apply(&mut self, top: &Layout) {
        self.agent = top.agent.or(self.agent);
        self.run = top.run.or(self.run);
        if !top.order.is_empty() {
            self.order = top.order.clone();
        }
    }

    /// `repos` with the ones named in `order` first, in that order.
    pub fn sorted(&self, repos: &[String]) -> Vec<String> {
        let mut out: Vec<String> = vec![];
        for r in self.order.iter().chain(repos) {
            if repos.contains(r) && !out.contains(r) {
                out.push(r.clone());
            }
        }
        out
    }
}

/// Where a split puts the new pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    Right,
    Down,
}

impl Dir {
    pub fn as_str(self) -> &'static str {
        match self {
            Dir::Right => "right",
            Dir::Down => "down",
        }
    }
}

/// Split that creates pane `i` (`i >= 1`): split pane `target`, the new pane goes `dir` of it.
/// `ratio` is the share the split pane keeps; `None` leaves herdr's default (half).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Step {
    pub target: usize,
    pub dir: Dir,
    pub ratio: Option<f64>,
}

/// The split that creates pane `i` of `n` panes in `preset`, or `None` for pane 0.
pub fn step(preset: Preset, n: usize, i: usize) -> Option<Step> {
    if i == 0 || i >= n {
        return None;
    }
    // equal shares along a chain: the split pane keeps 1 of the `n - i + 1` slots left
    let equal = |k: usize| Some(1.0 / (k - i + 1) as f64);
    let s = match preset {
        Preset::Columns => Step {
            target: i - 1,
            dir: Dir::Right,
            ratio: equal(n),
        },
        Preset::Rows => Step {
            target: i - 1,
            dir: Dir::Down,
            ratio: equal(n),
        },
        Preset::MainLeft if i == 1 => Step {
            target: 0,
            dir: Dir::Right,
            ratio: None,
        },
        Preset::MainLeft => Step {
            target: i - 1,
            dir: Dir::Down,
            ratio: None,
        },
        Preset::MainTop if i == 1 => Step {
            target: 0,
            dir: Dir::Down,
            ratio: None,
        },
        Preset::MainTop => Step {
            target: i - 1,
            dir: Dir::Right,
            ratio: equal(n),
        },
        Preset::Grid if i == 1 => Step {
            target: 0,
            dir: Dir::Right,
            ratio: Some(0.5),
        },
        Preset::Grid => {
            // row-major: pane `i` is in column `i % 2`, row `i / 2`, under pane `i - 2`
            let col = i % 2;
            let rows = (n - col).div_ceil(2);
            Step {
                target: i - 2,
                dir: Dir::Down,
                ratio: Some(1.0 / (rows - i / 2 + 1) as f64),
            }
        }
    };
    Some(s)
}

/// `--ratio` argument for a step.
pub fn ratio_arg(r: f64) -> String {
    format!("{r:.4}")
}

/// `herdr.layout` of a card's `.twig.toml` (empty when the file or the key is missing).
pub fn read_card_file(file: &Path) -> Result<Layout> {
    let Ok(text) = std::fs::read_to_string(file) else {
        return Ok(Layout::default());
    };
    #[derive(Deserialize)]
    struct Herdr {
        #[serde(default)]
        layout: Layout,
    }
    #[derive(Deserialize)]
    struct Doc {
        #[serde(default)]
        herdr: Option<Herdr>,
    }
    // other keys are checked by `overrides::merge_card_file`
    let doc: toml::Table =
        toml::from_str(&text).with_context(|| format!("invalid {}", file.display()))?;
    let mut only = toml::Table::new();
    if let Some(h) = doc.get("herdr") {
        only.insert("herdr".into(), h.clone());
    }
    let d: Doc = toml::Value::Table(only)
        .try_into()
        .with_context(|| format!("invalid [herdr.layout] in {}", file.display()))?;
    Ok(d.herdr.map(|h| h.layout).unwrap_or_default())
}

/// Save the preset of `tab` in the card's `.twig.toml` (`[herdr.layout]`), keeping the rest.
pub fn save_card_file(file: &Path, tab: &str, preset: Preset) -> Result<()> {
    let text = std::fs::read_to_string(file).unwrap_or_default();
    let mut doc: toml_edit::DocumentMut = text
        .parse()
        .with_context(|| format!("invalid TOML: {}", file.display()))?;
    let table = doc.as_table_mut();
    if !table.contains_key("herdr") {
        let mut t = toml_edit::Table::new();
        t.set_implicit(true);
        table.insert("herdr", toml_edit::Item::Table(t));
    }
    let herdr = table["herdr"]
        .as_table_mut()
        .context("`herdr` must be a table")?;
    if !herdr.contains_key("layout") {
        herdr.insert("layout", toml_edit::Item::Table(toml_edit::Table::new()));
    }
    herdr["layout"][tab] = toml_edit::value(preset.name());
    std::fs::write(file, doc.to_string())
        .with_context(|| format!("could not write {}", file.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn splits(p: Preset, n: usize) -> Vec<(usize, &'static str, Option<f64>)> {
        (1..n)
            .map(|i| {
                let s = step(p, n, i).unwrap();
                (s.target, s.dir.as_str(), s.ratio)
            })
            .collect()
    }

    #[test]
    fn columns_and_rows_share_equally() {
        let t = 1.0 / 3.0;
        assert_eq!(
            splits(Preset::Columns, 3),
            [(0, "right", Some(t)), (1, "right", Some(0.5))]
        );
        assert_eq!(splits(Preset::Rows, 2), [(0, "down", Some(0.5))]);
        assert!(step(Preset::Columns, 3, 0).is_none());
        assert!(step(Preset::Columns, 3, 3).is_none());
    }

    #[test]
    fn main_left_is_the_original_layout() {
        assert_eq!(
            splits(Preset::MainLeft, 4),
            [(0, "right", None), (1, "down", None), (2, "down", None)]
        );
    }

    #[test]
    fn main_top_puts_the_rest_in_columns_below() {
        // three columns below the main pane
        assert_eq!(
            splits(Preset::MainTop, 4),
            [
                (0, "down", None),
                (1, "right", Some(1.0 / 3.0)),
                (2, "right", Some(0.5))
            ]
        );
        assert_eq!(splits(Preset::MainTop, 3)[1], (1, "right", Some(0.5)));
    }

    #[test]
    fn grid_has_two_columns() {
        // 0 1 / 2 3 / 4
        assert_eq!(
            splits(Preset::Grid, 5),
            [
                (0, "right", Some(0.5)),
                (0, "down", Some(1.0 / 3.0)),
                (1, "down", Some(0.5)),
                (2, "down", Some(0.5)),
            ]
        );
    }

    #[test]
    fn order_puts_the_named_repos_first() {
        let l = Layout {
            order: vec!["web".into(), "gone".into(), "sdk".into()],
            ..Default::default()
        };
        let repos: Vec<String> = ["api", "web", "sdk"].map(String::from).into();
        assert_eq!(l.sorted(&repos), ["web", "sdk", "api"]);
        assert_eq!(Layout::default().sorted(&repos), ["api", "web", "sdk"]);
    }

    #[test]
    fn upper_layers_replace_only_the_keys_they_set() {
        let mut l = Layout {
            agent: Some(Preset::Rows),
            run: Some(Preset::Grid),
            order: vec!["a".into()],
        };
        l.apply(&Layout {
            agent: Some(Preset::Columns),
            ..Default::default()
        });
        assert_eq!(l.preset("agent"), Preset::Columns);
        assert_eq!(l.preset("run"), Preset::Grid);
        assert_eq!(l.order, ["a"]);
        assert_eq!(Layout::default().preset("agent"), Preset::MainLeft);
    }

    #[test]
    fn card_file_roundtrip_keeps_other_keys() {
        let tmp = tempfile::tempdir().unwrap();
        let f = tmp.path().join(".twig.toml");
        std::fs::write(&f, "[repos.api]\nport = 6000\n").unwrap();
        assert_eq!(read_card_file(&f).unwrap(), Layout::default());
        save_card_file(&f, "agent", Preset::Columns).unwrap();
        save_card_file(&f, "run", Preset::Rows).unwrap();
        let l = read_card_file(&f).unwrap();
        assert_eq!(
            (l.agent, l.run),
            (Some(Preset::Columns), Some(Preset::Rows))
        );
        assert!(std::fs::read_to_string(&f).unwrap().contains("port = 6000"));
        std::fs::write(&f, "[herdr.layout]\nagent = \"diagonal\"\n").unwrap();
        assert!(read_card_file(&f).is_err());
    }
}
