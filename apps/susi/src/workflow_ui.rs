//! Git workflow for translators: state display, background git jobs and the dialogs
//! "Get translation project", "Save changes" and "Prepare publication".

use std::path::Path;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::Duration;

use egui::{Color32, RichText};
use susi_core::i18n::{t, trn};
use susi_core::tf;
use susi_workflow::{FetchOptions, FetchReport, PublishOptions, PublishReport, SaveOptions, SaveReport, State, Status, StatusOptions, User, Workflow};

use crate::actions::Action;
use crate::app::SusiApp;
use crate::dialogs::{Dialog, ok_cancel};

/// Workflow commands (menu, toolbar, dialogs).
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// Re-read the state and ask the server (git fetch, merge requests).
    Refresh,
    FetchDialog,
    SaveDialog,
    PublishDialog,
    TokenDialog,
    /// Open the merge request in the browser.
    OpenRequest,
    Fetch(FetchOptions),
    Save {
        opts: SaveOptions,
        build: bool,
    },
    Publish {
        opts: PublishOptions,
        save: Option<SaveOptions>,
        build: bool,
    },
}

type Detected = Option<Box<(Workflow, Status)>>;

enum Outcome {
    Detected(Result<Detected, String>),
    Status(Result<Status, String>),
    Fetched(Result<FetchReport, String>),
    Saved(Result<SaveReport, String>),
    Published(Result<(Option<SaveReport>, PublishReport), String>),
    Users(Result<Vec<User>, String>),
    Token(Result<User, String>),
}

struct Done {
    outcome: Outcome,
    log: Vec<String>,
}

/// A git action running in a background thread.
pub struct Job {
    pub label: String,
    /// Read-only job (state, reviewers) that a user action may replace.
    replaceable: bool,
    rx: Receiver<Done>,
}

/// Workflow of the open project (if it is in a configured git working copy).
pub struct WorkflowUi {
    pub wf: Arc<Workflow>,
    pub status: Option<Status>,
    /// Reviewer candidates from the server (loaded for the publish dialog).
    pub users: Vec<User>,
}

pub struct FetchDialog {
    /// The open project has unsaved changes: save them before (they are then set aside by git).
    pub save_first: bool,
    /// Changes that are not on the server yet will be set aside.
    pub pending: bool,
    pub advanced: bool,
    pub branch: String,
    pub commit: String,
    pub default_branch: String,
    pub branches: Vec<String>,
}

pub struct SaveDialog {
    pub message: String,
    pub prefix: String,
    pub ci_skip: bool,
    pub build: bool,
    /// New branch name (advanced); empty = automatic.
    pub branch: String,
    /// A personal branch is created (currently on the default branch).
    pub new_branch: bool,
    pub files: Vec<String>,
    pub advanced: bool,
}

pub struct PublishDialog {
    pub title: String,
    pub description: String,
    /// Reviewer user names, separated by `;`.
    pub reviewers: String,
    pub target: String,
    pub squash: bool,
    pub remove_source_branch: bool,
    pub advanced: bool,
    /// Changes are saved (committed and pushed) first.
    pub save: Option<SaveOptions>,
    pub build: bool,
    pub branches: Vec<String>,
}

pub struct TokenDialog {
    pub server: String,
    pub token: String,
    pub has_token: bool,
}

/// Prefix choices of "Save changes" in plain language: (prefix, label).
fn prefixes() -> [(&'static str, &'static str); 4] {
    [("fix", t("Corrected translations (fix)")), ("feat", t("New translations (feat)")), ("chore", t("Maintenance (chore)")), ("", t("No prefix"))]
}

fn state_color(s: State) -> Color32 {
    match s {
        State::UpToDate | State::Published => Color32::from_rgb(40, 150, 60),
        State::UpdateAvailable | State::Modified => Color32::from_rgb(210, 120, 20),
        State::Saved => Color32::from_rgb(40, 110, 200),
        State::InReview => Color32::from_rgb(140, 70, 190),
        State::Detached => Color32::GRAY,
    }
}

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

fn same_file(a: &Path, b: &Path) -> bool {
    let abs = |p: &Path| std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf());
    abs(a) == abs(b)
}

fn spawn(app: &mut SusiApp, label: &str, replaceable: bool, f: impl FnOnce() -> Done + Send + 'static) {
    let (tx, rx) = channel();
    let lang = app.ui_lang;
    std::thread::spawn(move || {
        susi_core::i18n::set_current(lang);
        let _ = tx.send(f());
    });
    app.wf_job = Some(Job { label: label.to_string(), replaceable, rx });
}

/// Runs `f` with the workflow in a background thread (one job at a time; read-only jobs are replaced).
fn start(app: &mut SusiApp, label: &str, replaceable: bool, f: impl FnOnce(&Workflow) -> Outcome + Send + 'static) {
    let Some(w) = &app.workflow else {
        return;
    };
    if app.wf_job.as_ref().is_some_and(|j| !j.replaceable) {
        app.warn(t("Please wait until the current git action has finished."));
        return;
    }
    let wf = Arc::clone(&w.wf);
    spawn(app, label, replaceable, move || {
        let outcome = f(&wf);
        Done { outcome, log: wf.take_log() }
    });
}

/// Looks for a workflow configuration of the open project (in the background).
pub fn detect(app: &mut SusiApp) {
    let Some(path) = app.path().map(Path::to_path_buf) else {
        app.workflow = None;
        return;
    };
    if app.workflow.as_ref().is_some_and(|w| same_file(w.wf.slp(), &path)) {
        return;
    }
    app.workflow = None;
    let Some(p) = app.project.clone() else {
        return;
    };
    spawn(app, t("Checking git working copy…"), false, move || {
        let result = Workflow::detect(&path).and_then(|wf| match wf {
            Some(wf) => wf.status(&p, StatusOptions::default()).map(|s| Some(Box::new((wf, s)))),
            None => Ok(None),
        });
        let log = match &result {
            Ok(Some(b)) => b.0.take_log(),
            _ => Vec::new(),
        };
        Done { outcome: Outcome::Detected(result.map_err(err)), log }
    });
}

/// Re-reads the state; `online` also asks the server.
pub fn refresh(app: &mut SusiApp, online: bool) {
    let Some(p) = app.project.clone() else {
        return;
    };
    let label = if online { t("Asking the server…") } else { t("Checking state…") };
    start(app, label, true, move |wf| Outcome::Status(wf.status(&p, StatusOptions { fetch: online, online }).map_err(err)));
}

/// Collects the result of a finished background job.
pub fn poll(app: &mut SusiApp, ctx: &egui::Context) {
    let Some(job) = &app.wf_job else {
        return;
    };
    match job.rx.try_recv() {
        Ok(done) => {
            app.wf_job = None;
            for line in done.log {
                app.info(format!("$ {line}"));
            }
            finish(app, done.outcome);
        }
        Err(TryRecvError::Empty) => ctx.request_repaint_after(Duration::from_millis(150)),
        Err(TryRecvError::Disconnected) => {
            app.wf_job = None;
            app.error(t("The git action ended unexpectedly."));
        }
    }
}

fn set_status(app: &mut SusiApp, s: Status) {
    for w in &s.warnings {
        app.warn(w.clone());
    }
    if let Some(w) = app.workflow.as_mut() {
        w.status = Some(s);
    }
}

fn finish(app: &mut SusiApp, outcome: Outcome) {
    match outcome {
        Outcome::Detected(Ok(Some(b))) => {
            let (wf, s) = *b;
            app.info(tf!("Git workflow active ({path}): {state}", path = wf.root().display(), state = s.state.label()));
            app.workflow = Some(WorkflowUi { wf: Arc::new(wf), status: None, users: Vec::new() });
            set_status(app, s);
        }
        Outcome::Detected(Ok(None)) => app.workflow = None,
        Outcome::Status(Ok(s)) => set_status(app, s),
        Outcome::Fetched(Ok(r)) => fetched(app, &r),
        Outcome::Saved(Ok(r)) => {
            saved(app, &r);
            refresh(app, false);
        }
        Outcome::Published(Ok((save, r))) => {
            if let Some(s) = &save {
                saved(app, s);
            }
            let mr = &r.merge_request;
            let msg = if r.created { t("Merge request created:") } else { t("Merge request updated:") };
            app.info(format!("{msg} !{} {} ({})", mr.iid, mr.title, mr.url));
            refresh(app, true);
        }
        Outcome::Users(Ok(users)) => {
            if let Some(w) = app.workflow.as_mut() {
                w.users = users;
            }
        }
        Outcome::Token(Ok(user)) => app.info(tf!("Access token stored, signed in as {user}.", user = user.label())),
        Outcome::Detected(Err(e))
        | Outcome::Status(Err(e))
        | Outcome::Fetched(Err(e))
        | Outcome::Saved(Err(e))
        | Outcome::Published(Err(e))
        | Outcome::Users(Err(e))
        | Outcome::Token(Err(e)) => app.error(e),
    }
}

fn fetched(app: &mut SusiApp, r: &FetchReport) {
    if let Some(b) = &r.backup {
        app.warn(tf!("Local changes were set aside: {backup} (git stash list)", backup = b));
    }
    if let Some(b) = &r.saved_commits {
        app.warn(tf!("Local commits were kept in {backup}", backup = b));
    }
    app.info(tf!("Translation project fetched: {target} ({commit})", target = r.target, commit = r.commit));
    if let Some(path) = app.path().map(Path::to_path_buf) {
        app.open(&path);
    }
    refresh(app, false);
}

fn saved(app: &mut SusiApp, r: &SaveReport) {
    if r.nothing_to_do {
        app.info(t("No changes to save."));
        return;
    }
    if r.created_branch {
        app.info(tf!("New branch: {branch}", branch = r.branch));
    }
    app.info(tf!("Changes saved on the server ({branch}): {message}", branch = r.branch, message = r.message));
}

/// Saves the open project if it has unsaved changes; `false` if that failed.
fn save_project(app: &mut SusiApp) -> bool {
    !app.dirty || app.save()
}

/// Executes a workflow command.
pub fn run(app: &mut SusiApp, c: Command, ctx: &egui::Context) {
    if app.workflow.is_none() {
        app.warn(t("The project is not in a git working copy configured for the translation workflow."));
        return;
    }
    match c {
        Command::Refresh => refresh(app, true),
        Command::FetchDialog => open_fetch(app),
        Command::SaveDialog => open_save(app),
        Command::PublishDialog => open_publish(app),
        Command::TokenDialog => open_token(app),
        Command::OpenRequest => {
            if let Some(mr) = app.workflow.as_ref().and_then(|w| w.status.as_ref()?.merge_request.as_ref()) {
                ctx.open_url(egui::OpenUrl::new_tab(&mr.url));
            }
        }
        Command::Fetch(opts) => start(app, t("Getting translation project…"), false, move |wf| Outcome::Fetched(wf.fetch(&opts).map_err(err))),
        Command::Save { opts, build } => {
            if build {
                app.build(None);
            }
            let Some(p) = app.project.clone() else {
                return;
            };
            start(app, t("Saving changes…"), false, move |wf| Outcome::Saved(wf.save(&p, &opts).map_err(err)));
        }
        Command::Publish { opts, save, build } => {
            if build && save.is_some() {
                app.build(None);
            }
            let Some(p) = app.project.clone() else {
                return;
            };
            start(app, t("Preparing publication…"), false, move |wf| Outcome::Published(publish(wf, &p, save.as_ref(), &opts)));
        }
    }
}

fn publish(
    wf: &Workflow,
    p: &susi_core::model::Project,
    save: Option<&SaveOptions>,
    opts: &PublishOptions,
) -> Result<(Option<SaveReport>, PublishReport), String> {
    let saved = save.map(|s| wf.save(p, s)).transpose().map_err(err)?;
    let forge = wf.forge().map_err(err)?;
    let r = wf.publish(forge.as_ref(), opts).map_err(err)?;
    Ok((saved, r))
}

fn wf(app: &SusiApp) -> Option<Arc<Workflow>> {
    app.workflow.as_ref().map(|w| Arc::clone(&w.wf))
}

fn status(app: &SusiApp) -> Option<&Status> {
    app.workflow.as_ref()?.status.as_ref()
}

/// Whether there are changes that are not on the server yet.
fn pending(app: &SusiApp) -> bool {
    app.dirty || status(app).is_some_and(|s| !s.changed.is_empty() || s.unpushed > 0)
}

fn open_fetch(app: &mut SusiApp) {
    let Some(wf) = wf(app) else {
        return;
    };
    let default_branch = wf.default_branch();
    let mut branches = wf.remote_branches();
    branches.retain(|b| *b != default_branch);
    branches.insert(0, default_branch.clone());
    let pending = status(app).is_some_and(|s| !s.changed.is_empty() || s.unpushed > 0);
    app.dialog = Some(Dialog::WorkflowFetch(FetchDialog {
        save_first: app.dirty,
        pending,
        advanced: false,
        branch: default_branch.clone(),
        commit: String::new(),
        default_branch,
        branches,
    }));
}

fn open_save(app: &mut SusiApp) {
    let Some(wf) = wf(app) else {
        return;
    };
    let d = wf.save_defaults();
    let (files, new_branch) = status(app).map(|s| (s.changed.clone(), s.on_default_branch() || s.branch.is_none())).unwrap_or_default();
    app.dialog = Some(Dialog::WorkflowSave(SaveDialog {
        message: d.message,
        prefix: d.prefix,
        ci_skip: d.ci_skip,
        build: wf.config().save.build,
        branch: String::new(),
        new_branch,
        files,
        advanced: false,
    }));
}

fn open_publish(app: &mut SusiApp) {
    let Some(wf) = wf(app) else {
        return;
    };
    if !wf.has_token() {
        app.warn(t("Sign in to the server first (access token)."));
        open_token(app);
        return;
    }
    let d = wf.publish_defaults();
    let save = (pending(app) || status(app).is_some_and(Status::on_default_branch)).then(|| wf.save_defaults());
    let mut branches = wf.remote_branches();
    branches.retain(|b| *b != d.target);
    branches.insert(0, d.target.clone());
    app.dialog = Some(Dialog::WorkflowPublish(PublishDialog {
        title: d.title,
        description: d.description,
        reviewers: d.reviewers.join("; "),
        target: d.target,
        squash: d.squash,
        remove_source_branch: d.remove_source_branch,
        advanced: false,
        save,
        build: wf.config().save.build,
        branches,
    }));
    if app.workflow.as_ref().is_some_and(|w| w.users.is_empty()) {
        start(app, t("Loading reviewers…"), true, |wf| Outcome::Users(wf.forge().and_then(|f| wf.reviewer_candidates(f.as_ref())).map_err(err)));
    }
}

fn open_token(app: &mut SusiApp) {
    let Some(wf) = wf(app) else {
        return;
    };
    let server = match wf.forge_location() {
        Ok((url, project)) => format!("{url} ({project})"),
        Err(e) => e.to_string(),
    };
    app.dialog = Some(Dialog::WorkflowToken(TokenDialog { server, token: String::new(), has_token: wf.has_token() }));
}

fn busy(app: &SusiApp) -> bool {
    app.wf_job.as_ref().is_some_and(|j| !j.replaceable)
}

/// "Workflow" menu (only for configured working copies).
pub fn menu(app: &SusiApp, ui: &mut egui::Ui, acts: &mut Vec<Action>) {
    if app.workflow.is_none() {
        return;
    }
    let idle = !busy(app);
    let mut item = |ui: &mut egui::Ui, text: &str, c: Command, enabled: bool| {
        if ui.add_enabled(enabled, egui::Button::new(text)).clicked() {
            acts.push(Action::Workflow(c));
            ui.close();
        }
    };
    ui.menu_button(t("Workflow"), |ui| {
        item(ui, t("Get translation project…"), Command::FetchDialog, idle);
        item(ui, t("Save changes…"), Command::SaveDialog, idle);
        item(ui, t("Prepare publication…"), Command::PublishDialog, idle);
        ui.separator();
        item(ui, t("Refresh state"), Command::Refresh, idle);
        let has_mr = status(app).is_some_and(|s| s.merge_request.is_some());
        item(ui, t("Open merge request in the browser"), Command::OpenRequest, has_mr);
        ui.separator();
        item(ui, t("Sign in to the server…"), Command::TokenDialog, idle);
    });
}

/// Toolbar buttons (only for configured working copies).
pub fn toolbar(app: &SusiApp, ui: &mut egui::Ui, acts: &mut Vec<Action>) {
    if app.workflow.is_none() {
        return;
    }
    ui.separator();
    let idle = !busy(app);
    let buttons = [
        ("📥", t("Get"), t("Get translation project: latest version from the server"), Command::FetchDialog),
        ("📤", t("Save"), t("Save changes on the server"), Command::SaveDialog),
        ("🚀", t("Publish"), t("Prepare publication: create a merge request for review"), Command::PublishDialog),
    ];
    for (icon, text, hint, c) in buttons {
        if ui.add_enabled(idle, egui::Button::new(format!("{icon} {text}"))).on_hover_text(hint).clicked() {
            acts.push(Action::Workflow(c));
        }
    }
}

/// Status bar: running git action and workflow state (click: refresh).
pub fn status_bar(app: &SusiApp, ui: &mut egui::Ui, acts: &mut Vec<Action>) {
    if let Some(job) = &app.wf_job {
        ui.spinner();
        ui.label(&job.label);
        ui.separator();
    }
    let Some(s) = status(app) else {
        return;
    };
    let mut text = format!("git: {}", s.state.label());
    if let Some(b) = &s.branch {
        text += &format!(" [{b}]");
    }
    let mut hint = s.state.hint();
    if !s.changed.is_empty() {
        hint += "\n";
        hint += &trn(s.changed.len(), "{n} changed file", "{n} changed files");
    }
    if let Some(mr) = &s.merge_request {
        hint += &format!("\n!{} {}", mr.iid, mr.title);
    }
    hint += "\n";
    hint += t("Click to refresh (asks the server).");
    let label = egui::Label::new(RichText::new(text).color(state_color(s.state))).sense(egui::Sense::click());
    if ui.add(label).on_hover_text(hint).clicked() && !busy(app) {
        acts.push(Action::Workflow(Command::Refresh));
    }
    ui.separator();
}

fn advanced(ui: &mut egui::Ui, open: &mut bool, add: impl FnOnce(&mut egui::Ui)) {
    let r = egui::CollapsingHeader::new(t("Advanced settings")).open(Some(*open)).show(ui, add);
    if r.header_response.clicked() {
        *open = !*open;
    }
}

fn branch_combo(ui: &mut egui::Ui, id: &str, value: &mut String, branches: &[String]) {
    egui::ComboBox::from_id_salt(id).width(260.0).selected_text(value.as_str()).show_ui(ui, |ui| {
        for b in branches {
            ui.selectable_value(value, b.clone(), b);
        }
    });
}

fn warning(ui: &mut egui::Ui, text: &str) {
    ui.colored_label(Color32::from_rgb(210, 120, 20), text);
}

pub fn fetch_dialog(app: &mut SusiApp, ui: &mut egui::Ui, d: &mut FetchDialog) -> bool {
    ui.heading(t("Get translation project"));
    ui.label(t("Gets the latest version of the translation project from the server and opens it."));
    if d.pending {
        warning(
            ui,
            t(
                "Your changes are not saved on the server yet. They are set aside and can only be restored by a developer. Save the changes first if you want to keep working on them.",
            ),
        );
    }
    if app.dirty {
        ui.checkbox(&mut d.save_first, t("Save the open project first"));
        if !d.save_first {
            warning(ui, t("Unsaved changes of the open project will be lost."));
        }
    }
    advanced(ui, &mut d.advanced, |ui| {
        egui::Grid::new("wf-fetch").num_columns(2).show(ui, |ui| {
            ui.label(t("Branch:"));
            branch_combo(ui, "wf-fetch-branch", &mut d.branch, &d.branches);
            ui.end_row();
            ui.label(t("Specific version:"));
            ui.add(egui::TextEdit::singleline(&mut d.commit).hint_text(t("commit, tag (optional, read-only)")));
            ui.end_row();
        });
    });
    match ok_cancel(ui, t("Get"), true) {
        Some(true) => {
            if d.save_first && !save_project(app) {
                return false;
            }
            let branch = (d.branch != d.default_branch).then(|| d.branch.clone());
            let commit = Some(d.commit.trim().to_string()).filter(|c| !c.is_empty());
            app.actions.push(Action::Workflow(Command::Fetch(FetchOptions { branch, commit })));
            true
        }
        Some(false) => true,
        None => false,
    }
}

fn prefix_combo(ui: &mut egui::Ui, value: &mut String) {
    let choices = prefixes();
    let current = choices.iter().find(|(p, _)| p == value).map_or_else(|| value.clone(), |(_, l)| (*l).to_string());
    egui::ComboBox::from_id_salt("wf-prefix").width(260.0).selected_text(current).show_ui(ui, |ui| {
        for (p, label) in choices {
            ui.selectable_value(value, p.to_string(), label);
        }
    });
}

pub fn save_dialog(app: &mut SusiApp, ui: &mut egui::Ui, d: &mut SaveDialog) -> bool {
    ui.heading(t("Save changes"));
    ui.label(t("Saves the project and sends the changed files to the server."));
    if d.new_branch {
        ui.label(t("A personal branch is created for your changes."));
    }
    if !d.files.is_empty() {
        ui.label(trn(d.files.len(), "{n} changed file:", "{n} changed files:"));
        egui::ScrollArea::vertical().max_height(90.0).show(ui, |ui| {
            for f in &d.files {
                ui.monospace(f);
            }
        });
    }
    if app.dirty {
        ui.label(t("The open project has unsaved changes; it is saved first."));
    }
    egui::Grid::new("wf-save").num_columns(2).show(ui, |ui| {
        ui.label(t("Description:"));
        ui.add(egui::TextEdit::singleline(&mut d.message).desired_width(300.0));
        ui.end_row();
        ui.label(t("Kind of change:"));
        prefix_combo(ui, &mut d.prefix);
        ui.end_row();
    });
    ui.checkbox(&mut d.build, t("Build the localized files first"));
    advanced(ui, &mut d.advanced, |ui| {
        ui.checkbox(&mut d.ci_skip, t("Skip the build pipeline (CI) on the server"));
        ui.horizontal(|ui| {
            ui.label(t("New branch:"));
            ui.add(egui::TextEdit::singleline(&mut d.branch).hint_text(t("automatic")));
        });
    });
    match ok_cancel(ui, t("Save"), !d.message.trim().is_empty()) {
        Some(true) => {
            if !save_project(app) {
                return false;
            }
            let opts = SaveOptions {
                message: d.message.trim().to_string(),
                prefix: d.prefix.clone(),
                ci_skip: d.ci_skip,
                branch: Some(d.branch.trim().to_string()).filter(|b| !b.is_empty()),
            };
            app.actions.push(Action::Workflow(Command::Save { opts, build: d.build }));
            true
        }
        Some(false) => true,
        None => false,
    }
}

fn reviewer_picker(app: &SusiApp, ui: &mut egui::Ui, reviewers: &mut String) {
    let users = app.workflow.as_ref().map(|w| w.users.as_slice()).unwrap_or_default();
    if users.is_empty() {
        return;
    }
    egui::ComboBox::from_id_salt("wf-reviewer").width(260.0).selected_text(t("Add reviewer…")).show_ui(ui, |ui| {
        for u in users {
            if ui.selectable_label(false, u.label()).clicked() {
                let mut names: Vec<String> = split_names(reviewers);
                if !names.iter().any(|n| n.eq_ignore_ascii_case(&u.username)) {
                    names.push(u.username.clone());
                }
                *reviewers = names.join("; ");
            }
        }
    });
}

fn split_names(s: &str) -> Vec<String> {
    s.split([';', ',']).map(str::trim).filter(|n| !n.is_empty()).map(str::to_string).collect()
}

pub fn publish_dialog(app: &mut SusiApp, ui: &mut egui::Ui, d: &mut PublishDialog) -> bool {
    ui.heading(t("Prepare publication"));
    ui.label(t("Creates a merge request: a reviewer checks your changes and adopts them."));
    if d.save.is_some() {
        ui.label(t("Your changes are saved on the server first."));
    }
    egui::Grid::new("wf-publish").num_columns(2).show(ui, |ui| {
        ui.label(t("Title:"));
        ui.add(egui::TextEdit::singleline(&mut d.title).desired_width(320.0));
        ui.end_row();
        ui.label(t("Description:"));
        ui.add(egui::TextEdit::multiline(&mut d.description).desired_width(320.0).desired_rows(3));
        ui.end_row();
        ui.label(t("Reviewers:"));
        ui.vertical(|ui| {
            ui.add(egui::TextEdit::singleline(&mut d.reviewers).desired_width(320.0).hint_text(t("user names, separated by ;")));
            reviewer_picker(app, ui, &mut d.reviewers);
        });
        ui.end_row();
    });
    if let Some(job) = &app.wf_job {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(&job.label);
        });
    }
    advanced(ui, &mut d.advanced, |ui| {
        ui.horizontal(|ui| {
            ui.label(t("Target branch:"));
            branch_combo(ui, "wf-target", &mut d.target, &d.branches);
        });
        ui.checkbox(&mut d.squash, t("Combine the commits when merging (squash)"));
        ui.checkbox(&mut d.remove_source_branch, t("Delete the branch after merging"));
        if d.save.is_some() {
            ui.checkbox(&mut d.build, t("Build the localized files first"));
        }
    });
    let ready = !d.title.trim().is_empty() && !busy(app);
    match ok_cancel(ui, t("Create merge request"), ready) {
        Some(true) => {
            if d.save.is_some() && !save_project(app) {
                return false;
            }
            let opts = PublishOptions {
                title: d.title.trim().to_string(),
                description: d.description.clone(),
                target: d.target.clone(),
                reviewers: split_names(&d.reviewers),
                squash: d.squash,
                remove_source_branch: d.remove_source_branch,
            };
            app.actions.push(Action::Workflow(Command::Publish { opts, save: d.save.clone(), build: d.build }));
            true
        }
        Some(false) => true,
        None => false,
    }
}

pub fn token_dialog(app: &mut SusiApp, ui: &mut egui::Ui, d: &mut TokenDialog) -> bool {
    ui.heading(t("Sign in to the server"));
    ui.label(tf!("Server: {server}", server = d.server));
    ui.label(t(
        "Create a personal access token with the scope \"api\" in GitLab (user settings, access tokens) and paste it here. It is stored in the credential manager of the operating system, not in the project.",
    ));
    ui.horizontal(|ui| {
        ui.label(t("Access token:"));
        ui.add(egui::TextEdit::singleline(&mut d.token).password(true).desired_width(300.0));
    });
    if d.has_token {
        ui.label(t("An access token is already stored."));
        if ui.button(t("Remove stored token")).clicked() {
            match wf(app).map(|wf| wf.forget_token()) {
                Some(Err(e)) => app.error(e.to_string()),
                _ => app.info(t("The stored access token was removed.")),
            }
            d.has_token = false;
        }
    }
    match ok_cancel(ui, t("Sign in"), !d.token.trim().is_empty() && !busy(app)) {
        Some(true) => {
            let token = std::mem::take(&mut d.token).trim().to_string();
            start(app, t("Signing in…"), false, move |wf| {
                Outcome::Token(wf.store_token(&token).and_then(|()| wf.forge()).and_then(|f| f.current_user()).map_err(err))
            });
            true
        }
        Some(false) => true,
        None => false,
    }
}
