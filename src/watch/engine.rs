//! One watch cycle: ask the source what is tagged, decide what to start, record it in the
//! ledger and hand it to the executor. The source (polling today, a webhook later) and the
//! executor (`twig new` as a subprocess) are traits, so the rules run without either.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use anyhow::Result;

use super::config::{Rule, WatchCfg};
use super::ledger::{Entry, Ledger, Status};
use crate::provider::Candidate;

/// Longest wait after repeated failures of one scope.
const MAX_BACKOFF_SECS: u64 = 600;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardState {
    Missing,
    Active,
    Parked,
}

/// Where candidates come from.
pub trait Source {
    /// Items of `scope` with one of `tags`. `Err` is not "nothing found".
    fn query(&self, scope: &str, tags: &[String], mine: bool) -> Result<Vec<Candidate>>;
    fn valid_id(&self, id: &str) -> bool;
    fn is_finished(&self, state: &str) -> bool;
    /// Whether the tag was added by someone allowed to (see `trusted_taggers`).
    fn trusted(&self, c: &Candidate, tag: &str, trusted: &[String]) -> Result<bool>;
    fn card_state(&self, id: &str) -> CardState;
    fn herdr_ready(&self) -> bool;
    fn mark_tag(&self, c: &Candidate, tag: &str) -> Result<()>;
}

/// What a dispatch did.
#[derive(Debug, Clone, Default)]
pub struct Dispatch {
    pub ok: bool,
    pub exit: Option<i32>,
    pub log: Option<String>,
    pub secs: u64,
    pub detail: String,
}

/// Starts the card (`twig new <id> --template <template>`).
pub trait Executor {
    fn dispatch(&self, id: &str, template: &str) -> Dispatch;
}

/// What survives between cycles of one process.
#[derive(Default)]
pub struct Memory {
    backoff: HashMap<String, (u32, u64)>,
    untrusted: HashSet<String>,
}

pub struct Ctx<'a> {
    pub w: &'a WatchCfg,
    pub src: &'a dyn Source,
    pub exec: &'a dyn Executor,
    pub ledger: &'a Path,
    /// List what would start; touch neither the ledger nor the cards.
    pub dry_run: bool,
    pub now: u64,
    pub stop: &'a dyn Fn() -> bool,
    pub log: &'a dyn Fn(&str),
}

#[derive(Debug, Default)]
pub struct Report {
    /// Items started (or, in a dry run, that would be).
    pub started: Vec<String>,
    /// Entries dropped from the ledger because the item left the matching set.
    pub pruned: Vec<String>,
    /// Failed queries, ledger writes and dispatches.
    pub errors: usize,
}

/// The rule of the scope that matches one of the item's tags exactly (`agent` is not `agent:done`).
fn rule_for<'a>(w: &'a WatchCfg, c: &Candidate) -> Option<&'a Rule> {
    w.rules.iter().find(|r| {
        r.scope().eq_ignore_ascii_case(&c.scope)
            && c.tags.iter().any(|t| t.trim().eq_ignore_ascii_case(&r.tag))
    })
}

pub fn cycle(ctx: &Ctx, mem: &mut Memory) -> Result<Report> {
    let log = ctx.log;
    let w = ctx.w;
    let mut ledger = Ledger::load(ctx.ledger)?;
    let mut report = Report::default();

    // 1. what matches now, per scope; only scopes whose query worked may prune
    let mut ok_scopes: HashSet<String> = HashSet::new();
    let mut present: Vec<(Candidate, &Rule)> = vec![];
    for (scope, tags) in w.scopes() {
        let key = scope.to_lowercase();
        if mem
            .backoff
            .get(&key)
            .is_some_and(|(_, until)| ctx.now < *until)
        {
            log(&format!("{scope}: waiting after an error"));
            continue;
        }
        match ctx.src.query(&scope, &tags, w.assigned_to_me) {
            Ok(found) => {
                mem.backoff.remove(&key);
                ok_scopes.insert(key);
                for mut c in found {
                    c.scope = scope.clone();
                    if !ctx.src.valid_id(&c.id) {
                        log(&format!("{scope}: ignoring malformed id {:?}", c.id));
                    } else if ctx.src.is_finished(&c.state) {
                        continue;
                    } else if let Some(r) = rule_for(w, &c) {
                        present.push((c, r));
                    }
                }
            }
            Err(e) => {
                report.errors += 1;
                let fails = mem.backoff.get(&key).map_or(0, |(n, _)| *n) + 1;
                let wait = w
                    .interval()
                    .as_secs()
                    .saturating_mul(1 << fails.min(10))
                    .min(MAX_BACKOFF_SECS);
                mem.backoff.insert(key, (fails, ctx.now + wait));
                log(&format!(
                    "{scope}: query failed, entries kept (retry in {wait}s): {e:#}"
                ));
            }
        }
    }

    // 2. prune what left the set (tag removed, unassigned, closed), never on a failed query
    let ids: HashSet<&str> = present.iter().map(|(c, _)| c.id.as_str()).collect();
    let gone: Vec<String> = ledger
        .entries
        .iter()
        .filter(|(id, e)| ok_scopes.contains(&e.scope.to_lowercase()) && !ids.contains(id.as_str()))
        .map(|(id, _)| id.clone())
        .collect();
    for id in &gone {
        log(&format!(
            "{id}: no longer matches, forgotten (tag it again to start it again)"
        ));
        if !ctx.dry_run {
            ledger.entries.remove(id);
        }
    }
    report.pruned = gone;
    mem.untrusted.retain(|id| ids.contains(id.as_str()));
    if !ctx.dry_run && !report.pruned.is_empty() {
        ledger.save(ctx.ledger)?;
    }

    // 3. start the new ones, within the limits
    let active = ledger
        .entries
        .iter()
        .filter(|(_, e)| matches!(e.result, Status::Ok | Status::Pending))
        .filter(|(id, _)| ctx.src.card_state(id) == CardState::Active)
        .count();
    let mut started = 0;
    for (c, rule) in present {
        if ledger.entries.contains_key(&c.id) || mem.untrusted.contains(&c.id) {
            continue;
        }
        match ctx.src.card_state(&c.id) {
            CardState::Missing => {}
            state => {
                log(&format!(
                    "{}: card already exists{}",
                    c.id,
                    if state == CardState::Parked {
                        " (parked)"
                    } else {
                        ""
                    }
                ));
                if !ctx.dry_run {
                    ledger
                        .entries
                        .insert(c.id.clone(), entry(&c, rule, Status::Exists, ctx.now));
                    ledger.save(ctx.ledger)?;
                }
                continue;
            }
        }
        if started >= w.max_per_cycle {
            log(&format!(
                "{}: over max_per_cycle ({}), left for the next cycle",
                c.id, w.max_per_cycle
            ));
            continue;
        }
        if active + started >= w.max_active {
            log(&format!(
                "{}: over max_active ({}), left for a later cycle",
                c.id, w.max_active
            ));
            continue;
        }
        if !w.start_herdr && !ctx.src.herdr_ready() {
            log(&format!("{}: the herdr server is not running, deferred (start_herdr = true lets twig start it)", c.id));
            continue;
        }
        match ctx.src.trusted(&c, &rule.tag, &rule.trusted_taggers) {
            Ok(true) => {}
            Ok(false) => {
                log(&format!(
                    "{}: tag `{}` not added by a trusted tagger, ignored",
                    c.id, rule.tag
                ));
                mem.untrusted.insert(c.id.clone());
                continue;
            }
            Err(e) => {
                report.errors += 1;
                log(&format!(
                    "{}: could not tell who added the tag, skipped: {e:#}",
                    c.id
                ));
                continue;
            }
        }
        if ctx.dry_run {
            log(&format!(
                "{}: would start `twig new {} --template {}` (rule `{}` in {})",
                c.id, c.id, rule.template, rule.tag, c.scope
            ));
            report.started.push(c.id.clone());
            started += 1;
            continue;
        }
        if (ctx.stop)() {
            break;
        }
        // recorded first: a crash in the middle must not start it again in a loop
        ledger
            .entries
            .insert(c.id.clone(), entry(&c, rule, Status::Pending, ctx.now));
        ledger.save(ctx.ledger)?;
        log(&format!(
            "{}: starting with template {} (rule `{}` in {})",
            c.id, rule.template, rule.tag, c.scope
        ));
        let d = ctx.exec.dispatch(&c.id, &rule.template);
        let e = ledger.entries.get_mut(&c.id).expect("just inserted");
        e.result = if d.ok { Status::Ok } else { Status::Fail };
        e.exit = d.exit;
        e.log = d.log.clone();
        ledger.save(ctx.ledger)?;
        started += 1;
        report.started.push(c.id.clone());
        if d.ok {
            log(&format!("{}: ok ({}s)", c.id, d.secs));
            if !w.mark_tag.is_empty() {
                if let Err(e) = ctx.src.mark_tag(&c, &w.mark_tag) {
                    log(&format!(
                        "{}: could not add the tag `{}`: {e:#}",
                        c.id, w.mark_tag
                    ));
                }
            }
        } else {
            report.errors += 1;
            log(&format!("{}: FAILED ({}s): {}", c.id, d.secs, d.detail));
        }
    }
    Ok(report)
}

fn entry(c: &Candidate, r: &Rule, result: Status, at: u64) -> Entry {
    Entry {
        scope: c.scope.clone(),
        tag: r.tag.clone(),
        template: r.template.clone(),
        result,
        at,
        exit: None,
        log: None,
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use anyhow::anyhow;

    use super::*;

    /// A scripted source and executor in one.
    #[derive(Default)]
    struct Fake {
        /// Result per scope; a missing scope fails.
        items: RefCell<HashMap<String, Vec<Candidate>>>,
        cards: RefCell<HashMap<String, CardState>>,
        untrusted: HashSet<String>,
        no_herdr: bool,
        dispatched: RefCell<Vec<(String, String)>>,
        fail_dispatch: bool,
        marked: RefCell<Vec<String>>,
    }

    fn cand(id: &str, scope: &str, tags: &[&str]) -> Candidate {
        Candidate {
            id: id.into(),
            scope: scope.into(),
            tags: tags.iter().map(|t| t.to_string()).collect(),
            state: "Active".into(),
        }
    }

    impl Source for Fake {
        fn query(&self, scope: &str, _t: &[String], _m: bool) -> Result<Vec<Candidate>> {
            self.items
                .borrow()
                .get(scope)
                .cloned()
                .ok_or_else(|| anyhow!("network down"))
        }
        fn valid_id(&self, id: &str) -> bool {
            id.bytes().all(|b| b.is_ascii_digit())
        }
        fn is_finished(&self, s: &str) -> bool {
            s == "Closed"
        }
        fn trusted(&self, c: &Candidate, _t: &str, _tr: &[String]) -> Result<bool> {
            Ok(!self.untrusted.contains(&c.id))
        }
        fn card_state(&self, id: &str) -> CardState {
            self.cards
                .borrow()
                .get(id)
                .copied()
                .unwrap_or(CardState::Missing)
        }
        fn herdr_ready(&self) -> bool {
            !self.no_herdr
        }
        fn mark_tag(&self, c: &Candidate, tag: &str) -> Result<()> {
            self.marked.borrow_mut().push(format!("{}:{tag}", c.id));
            Ok(())
        }
    }

    impl Executor for Fake {
        fn dispatch(&self, id: &str, template: &str) -> Dispatch {
            self.dispatched
                .borrow_mut()
                .push((id.into(), template.into()));
            if !self.fail_dispatch {
                // the real `twig new` creates the card
                self.cards.borrow_mut().insert(id.into(), CardState::Active);
            }
            Dispatch {
                ok: !self.fail_dispatch,
                exit: Some(0),
                secs: 1,
                ..Default::default()
            }
        }
    }

    fn watch(extra: &str) -> WatchCfg {
        toml::from_str(&format!(
            "{extra}\n[[rules]]\ntag = \"agent\"\ntemplate = \"emp\"\nproject = \"P\"\n\
             [[rules]]\ntag = \"agent\"\ntemplate = \"other\"\nproject = \"Q\"\n"
        ))
        .unwrap()
    }

    struct Run {
        dir: tempfile::TempDir,
        mem: Memory,
    }

    impl Run {
        fn new() -> Self {
            Self {
                dir: tempfile::tempdir().unwrap(),
                mem: Memory::default(),
            }
        }
        fn ledger(&self) -> Ledger {
            Ledger::load(&self.dir.path().join("watch.json")).unwrap()
        }
        fn go(&mut self, w: &WatchCfg, f: &Fake, dry: bool, now: u64) -> Report {
            let file = self.dir.path().join("watch.json");
            let ctx = Ctx {
                w,
                src: f,
                exec: f,
                ledger: &file,
                dry_run: dry,
                now,
                stop: &|| false,
                log: &|m| eprintln!("{m}"),
            };
            cycle(&ctx, &mut self.mem).unwrap()
        }
    }

    fn fake(p: &[Candidate], q: &[Candidate]) -> Fake {
        let f = Fake::default();
        f.items.borrow_mut().insert("P".into(), p.to_vec());
        f.items.borrow_mut().insert("Q".into(), q.to_vec());
        f
    }

    #[test]
    fn starts_once_with_the_scopes_template_and_not_again() {
        let f = fake(&[cand("1", "P", &["agent"])], &[cand("2", "Q", &["agent"])]);
        let mut r = Run::new();
        let w = watch("");
        assert_eq!(r.go(&w, &f, false, 10).started, ["1", "2"]);
        assert_eq!(
            *f.dispatched.borrow(),
            [("1".into(), "emp".into()), ("2".into(), "other".into())]
        );
        assert_eq!(r.ledger().entries["1"].result, Status::Ok);
        // same state, again (and after a "restart": a new Memory)
        r.mem = Memory::default();
        assert!(r.go(&w, &f, false, 20).started.is_empty());
        assert_eq!(f.dispatched.borrow().len(), 2);
    }

    #[test]
    fn the_tag_match_is_exact() {
        let f = fake(
            &[
                cand("1", "P", &["agent:done"]),
                cand("2", "P", &[" Agent "]),
            ],
            &[],
        );
        let mut r = Run::new();
        assert_eq!(r.go(&watch(""), &f, false, 1).started, ["2"]);
    }

    #[test]
    fn closed_items_do_not_start() {
        let mut c = cand("1", "P", &["agent"]);
        c.state = "Closed".into();
        let f = fake(&[c], &[]);
        assert!(Run::new().go(&watch(""), &f, false, 1).started.is_empty());
    }

    #[test]
    fn an_existing_or_parked_card_is_recorded_not_recreated() {
        let f = fake(
            &[cand("1", "P", &["agent"]), cand("2", "P", &["agent"])],
            &[],
        );
        f.cards.borrow_mut().insert("1".into(), CardState::Active);
        f.cards.borrow_mut().insert("2".into(), CardState::Parked);
        let mut r = Run::new();
        assert!(r.go(&watch(""), &f, false, 1).started.is_empty());
        assert!(f.dispatched.borrow().is_empty());
        assert_eq!(r.ledger().entries["2"].result, Status::Exists);
        // `twig rm` while the tag stays: not created again
        f.cards.borrow_mut().clear();
        assert!(r.go(&watch(""), &f, false, 2).started.is_empty());
    }

    #[test]
    fn a_crash_leaves_pending_and_does_not_retry() {
        let f = fake(&[cand("1", "P", &["agent"])], &[]);
        let mut r = Run::new();
        let mut l = Ledger::default();
        l.entries.insert(
            "1".into(),
            entry(
                &cand("1", "P", &["agent"]),
                &watch("").rules[0],
                Status::Pending,
                1,
            ),
        );
        l.save(&r.dir.path().join("watch.json")).unwrap();
        assert!(r.go(&watch(""), &f, false, 5).started.is_empty());
        assert!(f.dispatched.borrow().is_empty());
    }

    #[test]
    fn a_failed_dispatch_is_recorded_and_not_retried() {
        let mut f = fake(&[cand("1", "P", &["agent"])], &[]);
        f.fail_dispatch = true;
        let mut r = Run::new();
        let rep = r.go(&watch(""), &f, false, 1);
        assert_eq!(rep.errors, 1);
        assert_eq!(r.ledger().entries["1"].result, Status::Fail);
        r.go(&watch(""), &f, false, 2);
        assert_eq!(f.dispatched.borrow().len(), 1);
    }

    #[test]
    fn leaving_the_set_prunes_and_retagging_starts_again() {
        let f = fake(&[cand("1", "P", &["agent"])], &[]);
        let mut r = Run::new();
        let w = watch("");
        r.go(&w, &f, false, 1);
        f.items
            .borrow_mut()
            .insert("P".into(), vec![cand("1", "P", &[])]);
        f.items.borrow_mut().insert("Q".into(), vec![]);
        let rep = r.go(&w, &f, false, 2);
        assert_eq!(rep.pruned, ["1"]);
        assert!(r.ledger().entries.is_empty());
        // the card was removed and the tag is put back
        f.cards.borrow_mut().clear();
        f.items
            .borrow_mut()
            .insert("P".into(), vec![cand("1", "P", &["agent"])]);
        assert_eq!(r.go(&w, &f, false, 3).started, ["1"]);
    }

    #[test]
    fn a_failed_query_never_prunes_its_scope() {
        let f = fake(&[cand("1", "P", &["agent"])], &[cand("2", "Q", &["agent"])]);
        let mut r = Run::new();
        let w = watch("");
        r.go(&w, &f, false, 1);
        // P fails (network), Q answers with nothing
        f.items.borrow_mut().remove("P");
        f.items.borrow_mut().insert("Q".into(), vec![]);
        let rep = r.go(&w, &f, false, 2);
        assert_eq!(rep.errors, 1);
        assert_eq!(rep.pruned, ["2"]);
        assert!(r.ledger().entries.contains_key("1"));
        assert!(!r.ledger().entries.contains_key("2"));
        // back online: nothing is started again
        f.items
            .borrow_mut()
            .insert("P".into(), vec![cand("1", "P", &["agent"])]);
        assert!(r.go(&w, &f, false, 100_000).started.is_empty());
    }

    #[test]
    fn a_failing_scope_backs_off_until_it_is_time() {
        let f = fake(&[], &[]);
        f.items.borrow_mut().remove("P");
        let mut r = Run::new();
        let w = watch("");
        assert_eq!(r.go(&w, &f, false, 1000).errors, 1);
        // 60s interval, one failure: waits 120s
        assert_eq!(r.go(&w, &f, false, 1060).errors, 0);
        assert_eq!(r.go(&w, &f, false, 1121).errors, 1);
    }

    #[test]
    fn max_per_cycle_leaves_the_rest_out_of_the_ledger() {
        let f = fake(
            &[
                cand("1", "P", &["agent"]),
                cand("2", "P", &["agent"]),
                cand("3", "P", &["agent"]),
            ],
            &[],
        );
        let mut r = Run::new();
        let w = watch("max_per_cycle = 2\nmax_active = 10");
        assert_eq!(r.go(&w, &f, false, 1).started, ["1", "2"]);
        assert!(!r.ledger().entries.contains_key("3"));
        assert_eq!(r.go(&w, &f, false, 2).started, ["3"]);
    }

    #[test]
    fn max_active_counts_live_cards_of_the_watch() {
        let f = fake(
            &[cand("1", "P", &["agent"]), cand("2", "P", &["agent"])],
            &[],
        );
        let mut r = Run::new();
        let w = watch("max_per_cycle = 5\nmax_active = 1");
        assert_eq!(r.go(&w, &f, false, 1).started, ["1"]);
        assert!(r.go(&w, &f, false, 2).started.is_empty());
        assert!(!r.ledger().entries.contains_key("2"));
        // the card is parked: the slot is free
        f.cards.borrow_mut().insert("1".into(), CardState::Parked);
        assert_eq!(r.go(&w, &f, false, 3).started, ["2"]);
    }

    #[test]
    fn an_untrusted_tagger_does_not_start_and_stays_out_of_the_ledger() {
        let mut f = fake(&[cand("1", "P", &["agent"])], &[]);
        f.untrusted.insert("1".into());
        let mut r = Run::new();
        assert!(r.go(&watch(""), &f, false, 1).started.is_empty());
        assert!(f.dispatched.borrow().is_empty());
        assert!(r.ledger().entries.is_empty());
    }

    #[test]
    fn without_a_herdr_server_the_start_waits_unless_allowed() {
        let mut f = fake(&[cand("1", "P", &["agent"])], &[]);
        f.no_herdr = true;
        let mut r = Run::new();
        assert!(r.go(&watch(""), &f, false, 1).started.is_empty());
        assert!(r.ledger().entries.is_empty());
        assert_eq!(
            r.go(&watch("start_herdr = true"), &f, false, 2).started,
            ["1"]
        );
    }

    #[test]
    fn dry_run_lists_without_side_effects() {
        let f = fake(&[cand("1", "P", &["agent"])], &[]);
        let mut r = Run::new();
        assert_eq!(r.go(&watch(""), &f, true, 1).started, ["1"]);
        assert!(f.dispatched.borrow().is_empty());
        assert!(!r.dir.path().join("watch.json").exists());
    }

    #[test]
    fn mark_tag_is_added_after_a_successful_start_only() {
        let f = fake(&[cand("1", "P", &["agent"])], &[]);
        let mut r = Run::new();
        r.go(&watch("mark_tag = \"agent:running\""), &f, false, 1);
        assert_eq!(*f.marked.borrow(), ["1:agent:running"]);
        let mut g = fake(&[cand("1", "P", &["agent"])], &[]);
        g.fail_dispatch = true;
        Run::new().go(&watch("mark_tag = \"agent:running\""), &g, false, 1);
        assert!(g.marked.borrow().is_empty());
    }

    #[test]
    fn malformed_ids_never_reach_the_executor() {
        let f = fake(&[cand("1; rm -rf /", "P", &["agent"])], &[]);
        assert!(Run::new().go(&watch(""), &f, false, 1).started.is_empty());
        assert!(f.dispatched.borrow().is_empty());
    }
}
