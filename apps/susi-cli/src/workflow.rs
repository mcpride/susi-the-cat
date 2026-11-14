//! `workflow` task: git workflow for translators (status, fetch, save, publish, init, config, login, logout).

use std::io::BufRead;

use susi_core::i18n::{t, trn};
use susi_core::{ops, slp, tf};
use susi_workflow::{FetchOptions, Status, StatusOptions, Workflow};

use crate::args::TaskArgs;
use crate::tasks::{Output, State};

/// Sub-actions in the order given (`susi-cli workflow save publish app.slp`).
pub const ACTIONS: &[&str] = &["status", "fetch", "save", "publish", "init", "config", "login", "logout"];

fn bool_option(a: &TaskArgs, name: &str) -> Option<bool> {
    if !a.flag(name) {
        return None;
    }
    Some(a.value(name).is_none_or(|v| !matches!(v.trim().to_ascii_lowercase().as_str(), "0" | "false" | "no" | "off")))
}

fn detect(st: &State) -> Result<Workflow, String> {
    Workflow::detect(&st.path).map_err(|e| e.to_string())?.ok_or_else(|| susi_workflow::Error::NotConfigured(st.path.display().to_string()).to_string())
}

fn log(wf: &Workflow, o: &mut Output<'_>) {
    for line in wf.take_log() {
        o.info(format!("  $ {line}"));
    }
}

pub fn run(a: &TaskArgs, st: &mut State, o: &mut Output<'_>) -> Result<(), String> {
    let actions: Vec<String> = if a.files.is_empty() { vec!["status".into()] } else { a.files.iter().map(|x| x.to_ascii_lowercase()).collect() };
    if let Some(bad) = actions.iter().find(|x| !ACTIONS.contains(&x.as_str())) {
        return Err(tf!("Unknown workflow action '{action}' (possible: {actions})", action = bad, actions = ACTIONS.join(", ")));
    }
    if actions.iter().any(|x| x == "init") {
        let path = Workflow::init(&st.path).map_err(|e| e.to_string())?;
        o.info(tf!("Created: {path}", path = path.display()));
        if actions.len() == 1 {
            return Ok(());
        }
    }
    let wf = detect(st)?;
    for action in actions.iter().filter(|x| *x != "init") {
        let r = match action.as_str() {
            "status" => status(a, st, &wf, o),
            "fetch" => fetch(a, st, &wf, o),
            "save" => save(a, st, &wf, o),
            "publish" => publish(a, &wf, o),
            "config" => config(&wf, o),
            "login" => login(&wf, o),
            _ => wf.forget_token().map_err(|e| e.to_string()).map(|()| o.info(t("The stored access token was removed."))),
        };
        log(&wf, o);
        r?;
    }
    Ok(())
}

fn print_status(s: &Status, o: &mut Output<'_>) {
    o.info(tf!("State: {state}", state = s.state.label()));
    o.info(s.state.hint());
    o.info(tf!("Branch: {branch} (default branch: {default})", branch = s.branch.as_deref().unwrap_or("-"), default = s.default_branch));
    if !s.changed.is_empty() {
        o.info(trn(s.changed.len(), "{n} changed file:", "{n} changed files:"));
        for f in &s.changed {
            o.info(format!("  {f}"));
        }
    }
    if s.unpushed > 0 {
        o.info(trn(s.unpushed, "{n} commit not yet on the server", "{n} commits not yet on the server"));
    }
    if let Some(mr) = &s.merge_request {
        o.info(tf!("Merge request: !{iid} {title} ({url})", iid = mr.iid, title = mr.title, url = mr.url));
    }
    for mr in &s.other_requests {
        o.note(tf!("Other open translation merge request: !{iid} {title} ({branch})", iid = mr.iid, title = mr.title, branch = mr.source_branch));
    }
    for w in &s.warnings {
        o.warn(w);
    }
}

fn status(a: &TaskArgs, st: &mut State, wf: &Workflow, o: &mut Output<'_>) -> Result<(), String> {
    let online = a.flag("online");
    let p = st.load(o)?;
    let s = wf.status(p, StatusOptions { fetch: online, online }).map_err(|e| e.to_string())?;
    print_status(&s, o);
    Ok(())
}

fn fetch(a: &TaskArgs, st: &mut State, wf: &Workflow, o: &mut Output<'_>) -> Result<(), String> {
    if st.changed {
        o.warn(t("Unsaved changes of previous tasks are discarded."));
    }
    let opts = FetchOptions { branch: a.value("branch").map(str::to_string), commit: a.value("commit").map(str::to_string) };
    let r = wf.fetch(&opts).map_err(|e| e.to_string())?;
    st.project = None;
    st.changed = false;
    if let Some(b) = &r.backup {
        o.note(tf!("Local changes were set aside: {backup} (git stash list)", backup = b));
    }
    if let Some(b) = &r.saved_commits {
        o.note(tf!("Local commits were kept in {backup}", backup = b));
    }
    o.info(tf!("Translation project fetched: {target} ({commit})", target = r.target, commit = r.commit));
    Ok(())
}

fn save(a: &TaskArgs, st: &mut State, wf: &Workflow, o: &mut Output<'_>) -> Result<(), String> {
    let build = wf.config().save.build && !a.flag("nobuild");
    let p = st.load(o)?;
    if build {
        let rep = ops::build_project(p, &p.language_ids(), None);
        for e in &rep.errors {
            o.error(e);
        }
        o.info(rep.summary());
    }
    if st.changed
        && let Some(p) = st.project.as_mut()
    {
        p.touch();
        slp::save(p, &st.path).map_err(|e| format!("{}: {e}", st.path.display()))?;
        st.changed = false;
        o.info(tf!("Project saved: {path}", path = st.path.display()));
    }
    let mut opts = wf.save_defaults();
    if let Some(m) = a.value("message") {
        opts.message = m.to_string();
    }
    if let Some(p) = a.value("prefix") {
        opts.prefix = p.to_string();
    }
    if let Some(c) = bool_option(a, "ciskip") {
        opts.ci_skip = c;
    }
    opts.branch = a.value("branch").map(str::to_string);
    let p = st.load(o)?;
    let r = wf.save(p, &opts).map_err(|e| e.to_string())?;
    if r.nothing_to_do {
        o.info(t("No changes to save."));
    } else {
        if r.created_branch {
            o.info(tf!("New branch: {branch}", branch = r.branch));
        }
        o.info(tf!("Changes saved on the server ({branch}): {message}", branch = r.branch, message = r.message));
    }
    Ok(())
}

fn publish(a: &TaskArgs, wf: &Workflow, o: &mut Output<'_>) -> Result<(), String> {
    let mut opts = wf.publish_defaults();
    if let Some(v) = a.value("title") {
        opts.title = v.to_string();
    }
    if let Some(v) = a.value("description") {
        opts.description = v.to_string();
    }
    if let Some(v) = a.value("target") {
        opts.target = v.to_string();
    }
    if a.flag("reviewer") {
        opts.reviewers = a.list("reviewer");
    }
    let forge = wf.forge().map_err(|e| e.to_string())?;
    let r = wf.publish(forge.as_ref(), &opts).map_err(|e| e.to_string())?;
    let mr = &r.merge_request;
    let msg = if r.created { t("Merge request created:") } else { t("Merge request updated:") };
    o.info(format!("{msg} !{} {} ({})", mr.iid, mr.title, mr.url));
    Ok(())
}

fn config(wf: &Workflow, o: &mut Output<'_>) -> Result<(), String> {
    for f in wf.config_files().all() {
        o.info(tf!("Configuration file: {path}", path = f.display()));
    }
    o.info(tf!("Working copy: {path}", path = wf.root().display()));
    o.info(tf!("Default branch: {branch}", branch = wf.default_branch()));
    if wf.config().has_forge() {
        match wf.forge_location() {
            Ok((url, project)) => o.info(tf!("Server: {url}, project {project}", url = url, project = project)),
            Err(e) => o.warn(e.to_string()),
        }
        o.info(if wf.has_token() { t("Access token: available") } else { t("Access token: missing (susi-cli workflow login)") });
    }
    o.info(String::new());
    o.info(wf.config().to_toml());
    Ok(())
}

fn login(wf: &Workflow, o: &mut Output<'_>) -> Result<(), String> {
    eprint!("{} ", t("Access token (input is visible; end with Enter):"));
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line).map_err(|e| e.to_string())?;
    wf.store_token(line.trim()).map_err(|e| e.to_string())?;
    let forge = wf.forge().map_err(|e| e.to_string())?;
    let user = forge.current_user().map_err(|e| e.to_string())?;
    o.info(tf!("Access token stored, signed in as {user}.", user = user.label()));
    Ok(())
}
