//! `twig config get/set`: read and write one dotted key, keeping comments and formatting.

use std::fs;
use std::path::Path;

use anyhow::{anyhow, bail, Context, Result};
use toml_edit::{Array, DocumentMut, Item, Table, Value};

use crate::config::{config_path, Config};
use crate::ui;

fn read_doc(file: &Path) -> Result<DocumentMut> {
    let text = fs::read_to_string(file).with_context(|| {
        format!(
            "config not found at {} (create it with `twig config edit`)",
            file.display()
        )
    })?;
    text.parse::<DocumentMut>()
        .with_context(|| format!("config is not valid TOML: {}", file.display()))
}

fn split_key(key: &str) -> Result<Vec<&str>> {
    let parts: Vec<&str> = key.split('.').collect();
    if parts.iter().any(|p| p.trim().is_empty()) {
        bail!("invalid key `{key}` (use dotted names, e.g. git.base_branch or repos.my-back.run)");
    }
    Ok(parts)
}

/// A TOML table printed as TOML text (inline tables are expanded).
fn table_text(t: &Table) -> String {
    let mut t = t.clone();
    t.decor_mut().clear();
    DocumentMut::from(t).to_string()
}

fn render(item: &Item) -> String {
    match item {
        Item::Value(Value::String(s)) => s.value().clone(),
        Item::Value(Value::Integer(i)) => i.value().to_string(),
        Item::Value(Value::Float(f)) => f.value().to_string(),
        Item::Value(Value::Boolean(b)) => b.value().to_string(),
        Item::Value(Value::Datetime(d)) => d.value().to_string(),
        Item::Value(Value::Array(a)) => {
            let mut a = a.clone();
            a.decor_mut().clear();
            a.to_string().trim().to_string()
        }
        Item::Value(Value::InlineTable(t)) => table_text(&t.clone().into_table()),
        Item::Table(t) => table_text(t),
        Item::ArrayOfTables(_) | Item::None => item.to_string(),
    }
}

/// `twig config get <key>`: reads the file as TOML only, so it works even when the config
/// would not pass validation (a bad value can still be inspected).
pub fn get(key: &str) -> Result<()> {
    let file = config_path()?;
    let doc = read_doc(&file)?;
    let mut item = doc.as_item();
    for part in split_key(key)? {
        item = item
            .get(part)
            .ok_or_else(|| anyhow!("key not found: {key}"))?;
    }
    println!("{}", render(item).trim_end());
    Ok(())
}

fn unquote(s: &str) -> &str {
    let s = s.trim();
    for q in ['"', '\''] {
        if s.len() >= 2 && s.starts_with(q) && s.ends_with(q) {
            return &s[1..s.len() - 1];
        }
    }
    s
}

fn scalar(s: &str) -> Value {
    match s {
        "true" => Value::from(true),
        "false" => Value::from(false),
        _ => match s.parse::<i64>() {
            Ok(i) => Value::from(i),
            Err(_) => Value::from(unquote(s)),
        },
    }
}

fn list(inner: &str) -> Value {
    let mut arr = Array::new();
    if !inner.trim().is_empty() {
        for part in inner.split(',') {
            arr.push(scalar(part.trim()));
        }
    }
    Value::Array(arr)
}

/// Possible readings of the text, most natural first. The first one that passes the config
/// validation wins, so `short = 123` still works as a string and `local_files = .env` as a
/// one-element list.
pub(crate) fn candidates(raw: &str) -> Vec<Value> {
    let raw = raw.trim();
    if raw.starts_with('[') && raw.ends_with(']') {
        // real TOML first (quoted strings), then the loose `[a, b]` form
        if let Ok(doc) = format!("v = {raw}").parse::<DocumentMut>() {
            if let Some(v) = doc.get("v").and_then(Item::as_value) {
                return vec![v.clone()];
            }
        }
        return vec![list(&raw[1..raw.len() - 1])];
    }
    let mut out = vec![scalar(raw)];
    if !matches!(out[0], Value::String(_)) {
        out.push(Value::from(raw));
    }
    if raw.contains(',') {
        out.push(list(raw));
    }
    let mut one = Array::new();
    one.push(scalar(raw));
    out.push(Value::Array(one));
    out
}

pub(crate) fn set_in(item: &mut Item, parts: &[&str], mut value: Value, path: &str) -> Result<()> {
    if item.is_none() {
        *item = Item::Table(Table::new());
    }
    let table = item
        .as_table_like_mut()
        .ok_or_else(|| anyhow!("`{path}` is not a table, can't set a key inside it"))?;
    let (first, rest) = parts.split_first().expect("key has at least one part");
    if rest.is_empty() {
        if let Some(old) = table.get(first).and_then(Item::as_value) {
            // keep the spacing and the trailing comment of the old value
            *value.decor_mut() = old.decor().clone();
        }
        table.insert(first, Item::Value(value));
        return Ok(());
    }
    let child = table.entry(first).or_insert_with(|| {
        let mut t = Table::new();
        t.set_implicit(true);
        Item::Table(t)
    });
    let path = if path.is_empty() {
        first.to_string()
    } else {
        format!("{path}.{first}")
    };
    set_in(child, rest, value, &path)
}

/// `twig config set <key> <value>`: edits the document, validates the result with the same
/// loader as `config edit` and only then replaces the file. The file needs to parse as TOML
/// but may currently be invalid for twig: setting the offending key is how you fix it.
pub fn set(key: &str, raw: &str) -> Result<()> {
    let file = config_path()?;
    // follow a symlinked config, so the link survives the replacement
    let file = fs::canonicalize(&file).unwrap_or(file);
    let doc = read_doc(&file)?;
    let parts = split_key(key)?;

    let tmp = file.with_file_name(format!(
        ".{}.twig-tmp",
        file.file_name().map_or_else(
            || "config.toml".into(),
            |n| n.to_string_lossy().into_owned()
        )
    ));
    let mut first_err = None;
    for value in candidates(raw) {
        let mut next = doc.clone();
        set_in(next.as_item_mut(), &parts, value, "")?;
        let text = next.to_string();
        fs::write(&tmp, &text).with_context(|| format!("writing {}", tmp.display()))?;
        let checked = Config::load_from(&tmp);
        match checked {
            Ok(_) => {
                fs::rename(&tmp, &file).with_context(|| format!("replacing {}", file.display()))?;
                ui::ok(format!("{key} set in {}", file.display()));
                return Ok(());
            }
            Err(e) => {
                first_err.get_or_insert(e);
            }
        }
    }
    let _ = fs::remove_file(&tmp);
    let e = first_err.expect("at least one candidate");
    // the validator's message lists the valid fields/values; the tmp name is noise
    let msg = format!("{e:#}").replace(&tmp.display().to_string(), &file.display().to_string());
    bail!(
        "can't set `{key}` to `{raw}`: {msg}\nthe config was not changed (valid keys and values: docs/CONFIG.md)"
    )
}
