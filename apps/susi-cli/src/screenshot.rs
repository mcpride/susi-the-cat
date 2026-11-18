//! `screenshot` task: screenshots of rows as context for translators, stored next to the project in
//! `<project>.files/` (the `.slp` stays unchanged).

use std::path::Path;

use susi_core::i18n::{t, trn};
use susi_core::keys::{RowKeys, source_index};
use susi_core::model::{Project, RowRef};
use susi_core::screenshots::{Area, Link, ScreenshotStore};
use susi_core::tf;

use crate::args::TaskArgs;
use crate::tasks::{Output, State};

/// Sub-actions (`susi-cli screenshot add login.png -key:LOGIN/* app.slp`).
pub const ACTIONS: &[&str] = &["list", "add", "link", "unlink", "remove", "prune", "refresh", "import"];

pub fn run(a: &TaskArgs, st: &mut State, o: &mut Output<'_>) -> Result<(), String> {
    let (action, args) = match a.files.split_first() {
        Some((x, rest)) => (x.to_ascii_lowercase(), rest),
        None => ("list".to_string(), &[][..]),
    };
    if !ACTIONS.contains(&action.as_str()) {
        return Err(tf!("Unknown screenshot action '{action}' (possible: {actions})", action = action, actions = ACTIONS.join(", ")));
    }
    let area = match a.value("area") {
        Some(v) => Some(Area::parse(v).ok_or_else(|| tf!("Invalid value for -{name}: \"{value}\"", name = "area", value = v) + " (x,y,w,h)")?),
        None => None,
    };
    let (p, shots) = st.project_and_screenshots(o)?;
    let core = |e: susi_core::Error| e.to_string();
    match action.as_str() {
        "list" => list(p, shots, o),
        "add" => {
            if args.is_empty() {
                return Err(t("Image file missing").into());
            }
            if args.len() > 1 && a.value("id").is_some() {
                return Err(t("-id can only be used with a single image").into());
            }
            let rows = if a.flag("key") { selected_rows(a, p, o)? } else { Vec::new() };
            for f in args {
                let id = shots.add_file(Path::new(f), a.value("id"), a.value("title")).map_err(core)?;
                o.info(tf!("Screenshot added: {id}", id = id));
                link_rows(p, shots, &id, &rows, area, o)?;
            }
            Ok(())
        }
        "link" => {
            let id = existing_id(args, shots)?;
            let rows = selected_rows(a, p, o)?;
            link_rows(p, shots, &id, &rows, area, o)
        }
        "unlink" => {
            let id = existing_id(args, shots)?;
            let n: usize = selected_rows(a, p, o)?.iter().map(|r| shots.unlink_row(p, &id, r)).sum();
            o.info(trn(n, "{n} link removed", "{n} links removed"));
            Ok(())
        }
        "remove" => {
            if args.is_empty() {
                return Err(t("Screenshot id missing").into());
            }
            for id in args {
                if shots.remove(id) {
                    o.info(tf!("Screenshot removed: {id}", id = id));
                } else {
                    o.error(tf!("Screenshot not found: {id}", id = id));
                }
            }
            Ok(())
        }
        "prune" => {
            let rep = shots.prune(p);
            o.info(tf!("{links} orphaned links removed, {files} unused image files deleted", links = rep.links, files = rep.files));
            Ok(())
        }
        "refresh" => {
            let ids: Vec<Option<&str>> = if args.is_empty() { vec![None] } else { args.iter().map(|x| Some(x.as_str())).collect() };
            for id in ids {
                if let Some(id) = id.filter(|i| shots.get(i).is_none()) {
                    o.error(tf!("Screenshot not found: {id}", id = id));
                    continue;
                }
                let rep = shots.refresh(id).map_err(core)?;
                for id in &rep.updated {
                    o.info(tf!("Screenshot updated: {id}", id = id));
                }
                for id in &rep.missing {
                    o.warn(tf!("Origin file of screenshot {id} not found", id = id));
                }
                o.info(tf!("{updated} screenshots updated, {unchanged} unchanged", updated = rep.updated.len(), unchanged = rep.unchanged));
            }
            Ok(())
        }
        _ => {
            let file = args.first().ok_or_else(|| t("Manifest file missing").to_string())?;
            let rep = shots.import_manifest(Path::new(file), a.flag("prune")).map_err(core)?;
            o.info(tf!(
                "{added} screenshots added, {replaced} replaced, {removed} removed, {links} links",
                added = rep.added,
                replaced = rep.replaced,
                removed = rep.removed,
                links = rep.links
            ));
            let orphans = shots.index(p).orphan_count();
            if orphans > 0 {
                o.warn(trn(orphans, "{n} link does not match any row", "{n} links do not match any row"));
            }
            Ok(())
        }
    }
}

/// The screenshot id given as first argument.
fn existing_id(args: &[String], shots: &ScreenshotStore) -> Result<String, String> {
    let id = args.first().ok_or_else(|| t("Screenshot id missing").to_string())?;
    shots.get(id).map(|s| s.id.clone()).ok_or_else(|| tf!("Screenshot not found: {id}", id = id))
}

/// Rows selected by `-key` (contexts or row ids, wildcards `*` and `?`) and `-source`.
fn selected_rows(a: &TaskArgs, p: &Project, o: &mut Output<'_>) -> Result<Vec<RowRef>, String> {
    let keys = a.list("key");
    if keys.is_empty() {
        return Err(t("Option -key is missing (contexts or row ids, wildcards * and ?)").into());
    }
    let source = match a.value("source") {
        Some(s) => Some(source_index(p, s).ok_or_else(|| tf!("The project does not contain the source \"{source}\"", source = s))?),
        None => None,
    };
    let index = RowKeys::new(p);
    let mut rows = Vec::new();
    for k in &keys {
        let found = index.select(k, source);
        if found.is_empty() {
            o.warn(tf!("No row matches the key \"{key}\"", key = k));
        }
        rows.extend(found);
    }
    rows.sort();
    rows.dedup();
    Ok(rows)
}

fn link_rows(p: &Project, shots: &mut ScreenshotStore, id: &str, rows: &[RowRef], area: Option<Area>, o: &mut Output<'_>) -> Result<(), String> {
    if rows.is_empty() {
        return Ok(());
    }
    let n = shots.link(id, rows.iter().map(|r| Link::to_row(p, r, area))).map_err(|e| e.to_string())?;
    o.info(trn(n, "{n} row linked", "{n} rows linked"));
    Ok(())
}

fn list(p: &Project, shots: &ScreenshotStore, o: &mut Output<'_>) -> Result<(), String> {
    let idx = shots.index(p);
    let orphans = shots.orphans(&idx);
    for s in shots.screenshots() {
        let links = trn(s.links.len(), "{n} link", "{n} links");
        o.info(format!("{}  \"{}\"  {} × {}  {links}", s.id, s.title, s.width, s.height));
        for l in &s.links {
            let orphan = orphans.iter().any(|(x, y)| x.id == s.id && *y == l);
            let area = l.area.map(|a| format!("  [{a}]")).unwrap_or_default();
            let mark = if orphan { format!("  {}", t("(no matching row)")) } else { String::new() };
            o.info(format!("    {} | {}{area}{mark}", l.source.as_deref().unwrap_or("*"), l.key));
        }
    }
    for s in shots.missing_files() {
        o.warn(tf!("Image file missing: {path}", path = shots.image_dir().join(&s.file).display()));
    }
    o.info(screenshot_summary(shots, idx.orphan_count()));
    Ok(())
}

/// `3 screenshots, 1 orphaned link`.
pub fn screenshot_summary(shots: &ScreenshotStore, orphans: usize) -> String {
    let mut s = trn(shots.len(), "{n} screenshot", "{n} screenshots");
    if orphans > 0 {
        s += ", ";
        s += &trn(orphans, "{n} orphaned link", "{n} orphaned links");
    }
    s
}
