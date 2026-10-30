//! The park snapshot: `<card>/.twig-park-layout.json`, written by `twig park` before it closes
//! the herdr workspace and read by `twig open` to rebuild it (tabs, splits, panes and each
//! agent's conversation). Kept apart from `.parked.json`, whose format is a stable contract.
//! Pure model and planning; `herdr.rs` talks to herdr.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config::Config;
use crate::layout::Dir;

pub const FILE: &str = ".twig-park-layout.json";
const VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub v: u32,
    /// In the order of the workspace.
    pub tabs: Vec<Tab>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tab {
    pub label: String,
    pub layout: Node,
}

/// A tab's tree of splits.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Node {
    /// `second` goes `direction` of `first`; `ratio` is the share `first` keeps.
    Split {
        direction: Dir,
        ratio: f64,
        first: Box<Node>,
        second: Box<Node>,
    },
    Pane(Pane),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// An AI agent: comes back resuming its conversation.
    Agent,
    /// The repo's `run` command.
    Run,
    /// Anything else: comes back as a shell in its folder.
    Shell,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pane {
    pub label: Option<String>,
    pub cwd: String,
    pub kind: Kind,
    /// `claude` / `codex` (agent panes).
    pub agent: Option<String>,
    /// The agent's session id, when herdr knew it.
    pub session: Option<String>,
    /// The command of a `run` pane.
    pub command: Option<String>,
}

pub fn path(cfg: &Config, id: &str) -> PathBuf {
    cfg.card_dir(id).join(FILE)
}

/// The card's snapshot; `None` when there is none or it can't be read.
pub fn read(cfg: &Config, id: &str) -> Option<Snapshot> {
    parse(&fs::read_to_string(path(cfg, id)).ok()?)
}

fn parse(text: &str) -> Option<Snapshot> {
    serde_json::from_str::<Snapshot>(text)
        .ok()
        .filter(|s| s.v == VERSION)
}

pub fn write(cfg: &Config, id: &str, tabs: Vec<Tab>) -> Result<()> {
    let snap = Snapshot { v: VERSION, tabs };
    let file = path(cfg, id);
    fs::write(&file, serde_json::to_string_pretty(&snap)? + "\n")
        .with_context(|| format!("writing {}", file.display()))
}

/// Delete the snapshot; whether there was one.
pub fn clear(cfg: &Config, id: &str) -> bool {
    fs::remove_file(path(cfg, id)).is_ok()
}

/// Whether `id` can be put in a shell command as is (session ids are UUIDs or similar).
pub fn safe_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn parse_dir(s: &str) -> Option<Dir> {
    match s.to_ascii_lowercase().as_str() {
        "right" | "horizontal" | "row" => Some(Dir::Right),
        "down" | "vertical" | "column" => Some(Dir::Down),
        _ => None,
    }
}

/// The tree in the answer of `herdr pane layout`: a split has a `direction`, a `ratio` and two
/// children (`first`/`second`, or a `children` list); a leaf has the `pane_id`, which `pane`
/// turns into the pane to record (`None` drops the leaf).
pub fn from_herdr(v: &Value, pane: &mut dyn FnMut(&str) -> Option<Pane>) -> Option<Node> {
    ["/result/layout", "/result/tree", "/result/root", "/result"]
        .iter()
        .filter_map(|p| v.pointer(p))
        .find_map(|root| node_from(root, pane))
}

fn node_from(v: &Value, pane: &mut dyn FnMut(&str) -> Option<Pane>) -> Option<Node> {
    if let Some(id) = v["pane_id"].as_str() {
        return pane(id).map(Node::Pane);
    }
    let kids: Vec<&Value> = match (v.get("first"), v.get("second")) {
        (Some(a), Some(b)) => vec![a, b],
        _ => v["children"].as_array()?.iter().collect(),
    };
    let dir = parse_dir(v["direction"].as_str()?)?;
    let ratio = v["ratio"].as_f64().unwrap_or(0.5);
    let nodes: Vec<Node> = kids
        .into_iter()
        .filter_map(|k| node_from(k, pane))
        .collect();
    chain(dir, ratio, nodes)
}

/// `nodes` as nested splits in one direction: the first keeps `ratio` of two, an equal share of more.
fn chain(dir: Dir, ratio: f64, mut nodes: Vec<Node>) -> Option<Node> {
    if nodes.is_empty() {
        return None;
    }
    if nodes.len() == 1 {
        return nodes.pop();
    }
    let n = nodes.len();
    let first = nodes.remove(0);
    let ratio = if n == 2 { ratio } else { 1.0 / n as f64 };
    Some(Node::Split {
        direction: dir,
        ratio: ratio.clamp(0.05, 0.95),
        first: Box::new(first),
        second: Box::new(chain(dir, 0.5, nodes)?),
    })
}

/// Panes side by side with equal widths, for when herdr's layout couldn't be read.
pub fn row(mut panes: Vec<Pane>) -> Option<Node> {
    let n = panes.len();
    let nodes: Vec<Node> = panes.drain(..).map(Node::Pane).collect();
    chain(Dir::Right, 1.0 / n.max(1) as f64, nodes)
}

/// The tree without the panes `keep` rejects: a split left with one child becomes that child.
pub fn prune(node: Node, keep: &dyn Fn(&Pane) -> bool) -> Option<Node> {
    match node {
        Node::Pane(p) => keep(&p).then_some(Node::Pane(p)),
        Node::Split {
            direction,
            ratio,
            first,
            second,
        } => match (prune(*first, keep), prune(*second, keep)) {
            (Some(a), Some(b)) => Some(Node::Split {
                direction,
                ratio,
                first: Box::new(a),
                second: Box::new(b),
            }),
            (a, b) => a.or(b),
        },
    }
}

/// A split to make while rebuilding a tab: split leaf `target`; the new pane is leaf `new`.
#[derive(Debug, PartialEq)]
pub struct Cut {
    pub target: usize,
    pub new: usize,
    pub dir: Dir,
    /// The share `target` keeps.
    pub ratio: f64,
}

/// The tab's panes in tree order (leaf 0 is the tab's root pane) and the splits that build it,
/// each one after the split that creates its target.
pub fn plan(node: &Node) -> (Vec<&Pane>, Vec<Cut>) {
    fn walk<'a>(n: &'a Node, leaves: &mut Vec<&'a Pane>, cuts: &mut Vec<Cut>) -> usize {
        match n {
            Node::Pane(p) => {
                leaves.push(p);
                leaves.len() - 1
            }
            Node::Split {
                direction,
                ratio,
                first,
                second,
            } => {
                let slot = cuts.len();
                cuts.push(Cut {
                    target: 0,
                    new: 0,
                    dir: *direction,
                    ratio: *ratio,
                });
                let target = walk(first, leaves, cuts);
                let new = walk(second, leaves, cuts);
                cuts[slot].target = target;
                cuts[slot].new = new;
                target
            }
        }
    }
    let (mut leaves, mut cuts) = (vec![], vec![]);
    walk(node, &mut leaves, &mut cuts);
    (leaves, cuts)
}

/// The folder `cwd` of a pane belongs to, among `repos` of the card in `card_dir`.
pub fn repo_of<'a>(card_dir: &Path, repos: &'a [String], cwd: &str) -> Option<&'a str> {
    let cwd = Path::new(cwd);
    repos
        .iter()
        .find(|r| cwd.starts_with(card_dir.join(r)))
        .map(String::as_str)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn pane(cwd: &str) -> Pane {
        Pane {
            label: None,
            cwd: cwd.into(),
            kind: Kind::Shell,
            agent: None,
            session: None,
            command: None,
        }
    }

    fn lookup(id: &str) -> Option<Pane> {
        Some(pane(&format!("/wt/{id}")))
    }

    #[test]
    fn reads_the_herdr_tree() {
        let v = json!({"result": {"layout": {
            "direction": "right", "ratio": 0.3,
            "first": {"pane_id": "a"},
            "second": {"direction": "down", "ratio": 0.6,
                "first": {"pane_id": "b"}, "second": {"pane_id": "c"}}
        }}});
        let n = from_herdr(&v, &mut lookup).unwrap();
        let (leaves, cuts) = plan(&n);
        let cwds: Vec<_> = leaves.iter().map(|p| p.cwd.as_str()).collect();
        assert_eq!(cwds, ["/wt/a", "/wt/b", "/wt/c"]);
        assert_eq!(
            cuts,
            [
                Cut {
                    target: 0,
                    new: 1,
                    dir: Dir::Right,
                    ratio: 0.3
                },
                Cut {
                    target: 1,
                    new: 2,
                    dir: Dir::Down,
                    ratio: 0.6
                },
            ]
        );
    }

    #[test]
    fn many_children_share_equally() {
        let v = json!({"result": {"direction": "horizontal", "children": [
            {"pane_id": "a"}, {"pane_id": "b"}, {"pane_id": "c"}]}});
        let n = from_herdr(&v, &mut lookup).unwrap();
        let (_, cuts) = plan(&n);
        assert_eq!(cuts.len(), 2);
        assert!((cuts[0].ratio - 1.0 / 3.0).abs() < 1e-9);
        assert!((cuts[1].ratio - 0.5).abs() < 1e-9);
        assert_eq!((cuts[1].target, cuts[1].new), (1, 2));
    }

    #[test]
    fn a_single_pane_tab_is_a_leaf() {
        let v = json!({"result": {"layout": {"pane_id": "a"}}});
        assert!(matches!(from_herdr(&v, &mut lookup), Some(Node::Pane(_))));
        assert!(from_herdr(&json!({"result": {}}), &mut lookup).is_none());
    }

    #[test]
    fn nested_first_child_keeps_the_target_leaf() {
        // (a | b) over c: c splits `a`, then b splits `a`
        let n = Node::Split {
            direction: Dir::Down,
            ratio: 0.5,
            first: Box::new(Node::Split {
                direction: Dir::Right,
                ratio: 0.5,
                first: Box::new(Node::Pane(pane("a"))),
                second: Box::new(Node::Pane(pane("b"))),
            }),
            second: Box::new(Node::Pane(pane("c"))),
        };
        let (leaves, cuts) = plan(&n);
        assert_eq!(leaves.len(), 3);
        assert_eq!(
            (cuts[0].target, cuts[0].new, cuts[0].dir),
            (0, 2, Dir::Down)
        );
        assert_eq!(
            (cuts[1].target, cuts[1].new, cuts[1].dir),
            (0, 1, Dir::Right)
        );
    }

    #[test]
    fn pruning_collapses_splits() {
        let n = row(vec![pane("a"), pane("gone"), pane("c")]).unwrap();
        let n = prune(n, &|p| p.cwd != "gone").unwrap();
        let (leaves, cuts) = plan(&n);
        assert_eq!(leaves.len(), 2);
        assert_eq!(cuts.len(), 1);
        assert!(prune(n, &|_| false).is_none());
    }

    #[test]
    fn round_trips_through_json() {
        let snap = Snapshot {
            v: VERSION,
            tabs: vec![Tab {
                label: "agent".into(),
                layout: row(vec![
                    Pane {
                        kind: Kind::Agent,
                        agent: Some("claude".into()),
                        session: Some("98a4aa39".into()),
                        ..pane("/wt/1/api")
                    },
                    pane("/wt/1/web"),
                ])
                .unwrap(),
            }],
        };
        let text = serde_json::to_string(&snap).unwrap();
        assert_eq!(parse(&text), Some(snap));
        assert!(parse(&text.replace("\"v\":1", "\"v\":9")).is_none());
    }

    #[test]
    fn session_ids_are_checked_before_reaching_a_shell() {
        assert!(safe_id("98a4aa39-1b2c-4d5e-8f90-aabbccddeeff"));
        assert!(!safe_id("x; rm -rf ~"));
        assert!(!safe_id(""));
    }

    #[test]
    fn finds_the_repo_of_a_cwd() {
        let repos = vec!["api".to_string(), "web".to_string()];
        let dir = Path::new("/wt/1");
        assert_eq!(repo_of(dir, &repos, "/wt/1/web/src"), Some("web"));
        assert_eq!(repo_of(dir, &repos, "/wt/1"), None);
    }
}
