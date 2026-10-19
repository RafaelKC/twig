//! Per-card memory (in development): one line per entry in a MEMORY.md that outlives the card.
//!
//! The real file is `<worktrees>/memos/<id>-<provider>/MEMORY.md`; the card folder holds a
//! relative symlink to it, so `twig rm` (which deletes the card folder) never touches the memo.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

use crate::config::Config;
use crate::{card, ui};

pub const MEMO_FILE: &str = "MEMORY.md";
/// How many of the latest entries go into the generated context file.
pub const MEMO_CONTEXT_ENTRIES: usize = 10;

/// `YYYY-MM-DD HH:MM: TEXT`; whitespace (newlines included) collapses to single spaces.
/// None when the text is empty.
pub fn format_line(stamp: &str, text: &str) -> Option<String> {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.is_empty() {
        None
    } else {
        Some(format!("{stamp}: {text}"))
    }
}

/// The last `n` non-empty lines, oldest first.
pub fn tail_lines(content: &str, n: usize) -> Vec<&str> {
    let lines: Vec<&str> = content.lines().filter(|l| !l.trim().is_empty()).collect();
    lines[lines.len().saturating_sub(n)..].to_vec()
}

#[cfg(unix)]
fn now_stamp() -> Result<String> {
    // SAFETY: plain libc calls on a zeroed `tm` owned by this frame.
    let tm = unsafe {
        let t = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&t, &mut tm).is_null() {
            bail!("could not read the local time");
        }
        tm
    };
    Ok(format!(
        "{:04}-{:02}-{:02} {:02}:{:02}",
        tm.tm_year + 1900,
        tm.tm_mon + 1,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min
    ))
}

#[cfg(not(unix))]
fn now_stamp() -> Result<String> {
    bail!("twig memo needs a Unix system for the local time")
}

/// Number of entries in the card's memo (0 when there is none).
pub fn entries(cfg: &Config, id: &str) -> usize {
    fs::read_to_string(cfg.memo_file(id))
        .map(|c| tail_lines(&c, usize::MAX).len())
        .unwrap_or(0)
}

/// Make sure `<card>/MEMORY.md` is a relative symlink to the memo. Never fails the caller:
/// problems become warnings.
pub fn ensure_link(cfg: &Config, id: &str) {
    if let Err(e) = try_link(cfg, id) {
        ui::warn(format!(
            "could not link MEMORY.md ({e:#}); the memo is at {}",
            cfg.memo_file(id).display()
        ));
    }
}

fn try_link(cfg: &Config, id: &str) -> Result<()> {
    let memo = cfg.memo_file(id);
    let dir = memo.parent().unwrap();
    fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let existed = memo.exists();
    if !existed {
        fs::write(&memo, "").with_context(|| format!("creating {}", memo.display()))?;
    }

    let link = cfg.card_dir(id).join(MEMO_FILE);
    let target: PathBuf = [
        "..",
        card::MEMOS_DIR,
        &format!("{id}-{}", cfg.provider.default.name()),
        MEMO_FILE,
    ]
    .iter()
    .collect();

    match fs::symlink_metadata(&link) {
        Ok(m) if m.file_type().is_symlink() => {
            if fs::read_link(&link).ok().as_deref() == Some(target.as_path()) {
                return Ok(());
            }
            fs::remove_file(&link)?;
        }
        Ok(_) => bail!(
            "{} exists and is not a symlink, leaving it alone",
            link.display()
        ),
        Err(_) => {}
    }
    make_symlink(&target, &link)?;
    let n = entries(cfg, id);
    if existed && n > 0 {
        ui::ok(format!(
            "memo restored: MEMORY.md -> {} ({n} entries)",
            memo.display()
        ));
    }
    Ok(())
}

#[cfg(unix)]
fn make_symlink(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(not(unix))]
fn make_symlink(_: &Path, _: &Path) -> std::io::Result<()> {
    Err(std::io::Error::other("symlinks are only supported on Unix"))
}

/// Markdown section with the latest entries for the context file; None without a memo.
pub fn context_section(cfg: &Config, id: &str) -> Option<String> {
    let content = fs::read_to_string(cfg.memo_file(id)).ok()?;
    let last = tail_lines(&content, MEMO_CONTEXT_ENTRIES);
    if last.is_empty() {
        return None;
    }
    let mut out = format!(
        "## Memory (last {MEMO_CONTEXT_ENTRIES} entries, full file: {MEMO_FILE})\n\n\
         <!-- From {MEMO_FILE} (newest last). Add entries with `twig memo \"...\"`; `twig refresh` updates this section. -->\n\n"
    );
    for l in last {
        out += &format!("- {l}\n");
    }
    out.push('\n');
    Some(out)
}

/// `twig memo [TEXT...] [--card ID]`.
pub fn run(cfg: &Config, text: &[String], card_arg: Option<&str>) -> Result<()> {
    let id = match card_arg {
        Some(id) => id.to_string(),
        None => match card::infer(cfg) {
            card::Current::Card(id) => id,
            card::Current::Dir(dir) => {
                bail!("{} is not inside a card; use --card <id>", dir.display())
            }
        },
    };
    if card::is_reserved(&id) {
        bail!("card {id} not found in {}", cfg.worktrees().display());
    }
    let memo = cfg.memo_file(&id);
    let card_exists = cfg.card_dir(&id).is_dir();
    if !card_exists && !memo.exists() {
        bail!("card {id} not found in {}", cfg.worktrees().display());
    }

    if text.is_empty() {
        println!("{}", memo.display());
        match fs::read_to_string(&memo) {
            Ok(c) if !c.trim().is_empty() => print!("\n{c}"),
            _ => ui::info("no memo entries yet"),
        }
        return Ok(());
    }

    let Some(line) = format_line(&now_stamp()?, &text.join(" ")) else {
        bail!("empty memo text; usage: twig memo \"TEXT\"");
    };
    fs::create_dir_all(memo.parent().unwrap())?;
    let needs_newline = fs::read_to_string(&memo)
        .map(|c| !c.is_empty() && !c.ends_with('\n'))
        .unwrap_or(false);
    let mut f = OpenOptions::new().create(true).append(true).open(&memo)?;
    if needs_newline {
        f.write_all(b"\n")?;
    }
    writeln!(f, "{line}")?;
    if card_exists {
        ensure_link(cfg, &id);
    }
    ui::ok(format!("memo saved: {}", memo.display()));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_has_stamp_and_collapses_whitespace() {
        assert_eq!(
            format_line("2026-01-02 03:04", "  did  a\nthing\r\n").unwrap(),
            "2026-01-02 03:04: did a thing"
        );
    }

    #[test]
    fn empty_text_has_no_line() {
        assert_eq!(format_line("2026-01-02 03:04", " \n\t "), None);
    }

    #[test]
    fn tail_keeps_the_last_n_non_empty_lines_in_order() {
        let c = "a\nb\n\nc\nd\n";
        assert_eq!(tail_lines(c, 2), vec!["c", "d"]);
        assert_eq!(tail_lines(c, 10), vec!["a", "b", "c", "d"]);
        assert!(tail_lines("", 3).is_empty());
    }
}
