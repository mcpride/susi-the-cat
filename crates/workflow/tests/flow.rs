//! End-to-end test of the workflow with a real git repository (bare "server" repository) and a fake forge.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

use susi_core::{Project, ops, slp};
use susi_workflow::forge::MergeRequestQuery;
use susi_workflow::{
    FetchOptions, Forge, MergeRequest, MergeRequestDraft, MergeRequestState, PublishOptions, SaveOptions, State, StatusOptions, User, Workflow,
};

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git").arg("-C").arg(dir).args(args).output().unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn configure(dir: &Path, name: &str) {
    git(dir, &["config", "user.name", name]);
    git(dir, &["config", "user.email", &format!("{}@example.com", name.split(' ').next().unwrap().to_lowercase())]);
    git(dir, &["config", "commit.gpgsign", "false"]);
    git(dir, &["config", "core.autocrlf", "false"]);
}

const CONFIG: &str = "[forge]\nprovider = \"gitlab\"\nurl = \"https://gitlab.example.com\"\nproject = \"group/app\"\ntoken = \"none\"\n";

/// Server repository, a developer's clone (seeded with a project) and the translator's clone.
fn setup(tmp: &Path) -> (PathBuf, PathBuf) {
    let server = tmp.join("server.git");
    let dev = tmp.join("dev");
    let work = tmp.join("work");
    std::fs::create_dir_all(&server).unwrap();
    std::fs::create_dir_all(&dev).unwrap();
    git(&server, &["init", "-q", "--bare", "-b", "main"]);
    let out = Command::new("git").arg("--git-dir").arg(&server).args(["config", "receive.advertisePushOptions", "true"]).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    git(&dev, &["init", "-q", "-b", "main"]);
    configure(&dev, "Dev Eloper");
    std::fs::create_dir_all(dev.join(".susi")).unwrap();
    std::fs::write(dev.join(".susi").join("workflow.toml"), CONFIG).unwrap();
    let l10n = dev.join("l10n");
    std::fs::create_dir_all(&l10n).unwrap();
    std::fs::write(l10n.join("strings.json"), "{\n  \"hello\": \"Hello\"\n}\n").unwrap();
    let mut p = Project::new();
    p.path = Some(l10n.join("app.slp"));
    p.add_language("de");
    ops::add_source(&mut p, &l10n.join("strings.json"), "en").unwrap();
    slp::save(&mut p, &l10n.join("app.slp")).unwrap();
    std::fs::write(dev.join("README.txt"), "app\n").unwrap();
    git(&dev, &["add", "-A"]);
    git(&dev, &["commit", "-q", "-m", "init"]);
    git(&dev, &["remote", "add", "origin", server.to_str().unwrap()]);
    git(&dev, &["push", "-q", "origin", "main"]);
    // autocrlf must already apply to the checkout (Windows runners default to `true`).
    git(tmp, &["clone", "-q", "-c", "core.autocrlf=false", server.to_str().unwrap(), work.to_str().unwrap()]);
    configure(&work, "Anna Müller");
    (dev, work)
}

#[derive(Default)]
struct FakeForge {
    requests: Mutex<Vec<MergeRequest>>,
    drafts: Mutex<Vec<MergeRequestDraft>>,
}

impl FakeForge {
    fn mr(iid: u64, d: &MergeRequestDraft) -> MergeRequest {
        MergeRequest {
            iid,
            title: d.title.clone(),
            url: format!("https://gitlab.example.com/group/app/-/merge_requests/{iid}"),
            state: MergeRequestState::Open,
            source_branch: d.source_branch.clone(),
            target_branch: d.target_branch.clone(),
            author: None,
            reviewers: Vec::new(),
            assignees: Vec::new(),
        }
    }
}

impl Forge for FakeForge {
    fn name(&self) -> &'static str {
        "Fake"
    }
    fn current_user(&self) -> susi_workflow::Result<User> {
        Ok(User { id: 1, username: "anna".into(), name: "Anna Müller".into() })
    }
    fn members(&self) -> susi_workflow::Result<Vec<User>> {
        Ok(vec![User { id: 2, username: "bob".into(), name: "Bob".into() }, User { id: 3, username: "dev".into(), name: "Dev Eloper".into() }])
    }
    fn merge_requests(&self, q: &MergeRequestQuery) -> susi_workflow::Result<Vec<MergeRequest>> {
        let all = self.requests.lock().unwrap();
        Ok(all.iter().filter(|m| q.source_branch.as_ref().is_none_or(|s| *s == m.source_branch)).cloned().collect())
    }
    fn create_merge_request(&self, d: &MergeRequestDraft) -> susi_workflow::Result<MergeRequest> {
        self.drafts.lock().unwrap().push(d.clone());
        let mut all = self.requests.lock().unwrap();
        let mr = Self::mr(all.len() as u64 + 1, d);
        all.push(mr.clone());
        Ok(mr)
    }
    fn update_merge_request(&self, iid: u64, d: &MergeRequestDraft) -> susi_workflow::Result<MergeRequest> {
        self.drafts.lock().unwrap().push(d.clone());
        Ok(Self::mr(iid, d))
    }
}

/// Publish: create, then update the merge request.
fn check_publish(wf: &Workflow, branch: &str) {
    let forge = FakeForge::default();
    let mut popts = wf.publish_defaults();
    popts.reviewers = vec!["@bob".into()];
    let published = wf.publish(&forge, &popts).unwrap();
    assert!(published.created);
    let republished = wf.publish(&forge, &PublishOptions { title: "New title".into(), ..popts.clone() }).unwrap();
    assert!(!republished.created);
    let drafts = forge.drafts.lock().unwrap().clone();
    assert_eq!((drafts[0].source_branch.as_str(), drafts[0].target_branch.as_str()), (branch, "main"));
    assert_eq!(drafts[0].reviewers, ["bob"]);
    assert!(drafts[0].assignees.is_empty(), "reviewers by default");
    assert_eq!(drafts[1].title, "New title");
    let candidates = wf.reviewer_candidates(&forge).unwrap();
    assert_eq!(candidates[0].username, "dev", "previous committers of the project come first");

    // Teams working with assignees instead of reviewers.
    let local = wf.root().join(".git").join("susi").join("workflow.local.toml");
    std::fs::create_dir_all(local.parent().unwrap()).unwrap();
    let bob = |on: bool| if on { vec!["bob".to_string()] } else { Vec::new() };
    for (assign_as, reviewers, assignees) in [("assignees", false, true), ("both", true, true)] {
        std::fs::write(&local, format!("[publish]\nassign_as = \"{assign_as}\"\n")).unwrap();
        let wf = Workflow::detect_with(wf.slp(), None).unwrap().unwrap();
        let forge = FakeForge::default();
        wf.publish(&forge, &popts).unwrap();
        let draft = forge.drafts.lock().unwrap()[0].clone();
        assert_eq!((draft.reviewers, draft.assignees), (bob(reviewers), bob(assignees)), "{assign_as}");
    }
    std::fs::remove_file(&local).unwrap();
}

/// After publishing, the merge request is remembered offline, also across further saves.
fn check_saving_after_publish(wf: &Workflow, p: &mut Project, slp_path: &Path, opts: &SaveOptions) {
    let st = wf.status(p, StatusOptions::default()).unwrap();
    assert_eq!((st.state, st.merge_request.as_ref().map(|m| m.iid)), (State::InReview, Some(1)));
    // A real change: `slp::save` overwrites `date` with the current second, which may be unchanged.
    p.record_scan();
    slp::save(p, slp_path).unwrap();
    assert_eq!(wf.status(p, StatusOptions::default()).unwrap().merge_request.map(|m| m.iid), Some(1));
    assert!(wf.save(p, opts).unwrap().pushed);
    assert_eq!(wf.status(p, StatusOptions::default()).unwrap().state, State::InReview);
}

#[test]
fn fetch_save_publish_cycle() {
    let tmp = tempfile::tempdir().unwrap();
    let (dev, work) = setup(tmp.path());
    let slp_path = work.join("l10n").join("app.slp");
    assert!(Workflow::detect_with(&dev.join("README.txt"), None).unwrap().is_some(), "the developer clone is configured as well");
    assert!(Workflow::detect_with(&tmp.path().join("x.slp"), None).unwrap().is_none(), "not a working copy");
    let wf = Workflow::detect_with(&slp_path, None).unwrap().expect("configured");
    assert_eq!(wf.default_branch(), "main");
    let mut p = slp::load(&slp_path).unwrap();
    let st = wf.status(&p, StatusOptions::default()).unwrap();
    assert_eq!((st.state, st.branch.as_deref()), (State::UpToDate, Some("main")));

    // Translate: the localized file is created; an unrelated file must not be saved.
    let report = ops::build_project(&p, &["de".to_string()], None);
    assert!(!report.written.is_empty(), "{report:?}");
    // Not `touch()`: within the same second as `setup`, the file would be written unchanged.
    p.record_scan();
    slp::save(&mut p, &slp_path).unwrap();
    std::fs::write(work.join("notes.txt"), "private\n").unwrap();
    let st = wf.status(&p, StatusOptions::default()).unwrap();
    assert_eq!(st.state, State::Modified);
    assert!(st.changed.contains(&"l10n/app.slp".to_string()), "{:?}", st.changed);
    assert!(st.changed.iter().any(|f| f.starts_with("l10n/") && f.contains("de")), "localized file: {:?}", st.changed);
    assert!(!st.changed.iter().any(|f| f == "notes.txt"));

    // Save: new personal branch, commit, push (with the GitLab push option ci.skip).
    let mut opts = wf.save_defaults();
    assert_eq!(opts.message, "Update translations (app)");
    opts.ci_skip = true;
    let saved = wf.save(&p, &opts).unwrap();
    assert!(saved.created_branch && saved.pushed);
    assert!(saved.branch.starts_with("l10n/anna.mueller/"), "{}", saved.branch);
    assert_eq!(git(&work, &["log", "-1", "--format=%s"]), "fix: Update translations (app)");
    assert!(git(&work, &["ls-remote", "--heads", "origin"]).contains(&saved.branch));
    let rest = git(&work, &["status", "--porcelain"]);
    assert!(rest.contains("notes.txt") && !rest.contains("app.slp") && !rest.contains(".json"), "{rest}");
    let st = wf.status(&p, StatusOptions::default()).unwrap();
    assert_eq!(st.state, State::Saved);
    let again = wf.save(&p, &opts).unwrap();
    assert!(again.nothing_to_do);

    check_publish(&wf, &saved.branch);
    check_saving_after_publish(&wf, &mut p, &slp_path, &opts);

    // The merge request is merged and the branch removed on the server.
    git(&dev, &["push", "-q", "origin", "--delete", &saved.branch]);
    let st = wf.status(&p, StatusOptions { fetch: true, online: false }).unwrap();
    assert_eq!(st.state, State::Published);
    assert!(wf.save(&p, &opts).is_err(), "saving to a published branch is refused");

    // Get the translation project: local leftovers are set aside, the default branch is checked out.
    let fetched = wf.fetch(&FetchOptions::default()).unwrap();
    assert_eq!(fetched.target, "main");
    assert!(fetched.backup.is_some(), "notes.txt was stashed");
    assert!(!work.join("notes.txt").exists());
    assert!(git(&work, &["stash", "list"]).contains("susi backup"));
    let p = slp::load(&slp_path).unwrap();
    assert_eq!(wf.status(&p, StatusOptions::default()).unwrap().state, State::UpToDate);

    // A specific version is read-only.
    let first = git(&work, &["rev-list", "--max-parents=0", "HEAD"]);
    let fetched = wf.fetch(&FetchOptions { commit: Some(first), ..Default::default() }).unwrap();
    assert!(fetched.backup.is_none());
    assert_eq!(wf.status(&p, StatusOptions::default()).unwrap().state, State::Detached);
    assert!(!wf.take_log().is_empty());
}

#[test]
fn github_working_copies() {
    use susi_workflow::forge::Provider;
    let tmp = tempfile::tempdir().unwrap();
    let (_dev, work) = setup(tmp.path());
    let slp_path = work.join("l10n").join("app.slp");
    // The working copy file overrides the repository: `auto` recognizes github.com.
    let local = work.join(".git").join("susi").join("workflow.local.toml");
    std::fs::create_dir_all(local.parent().unwrap()).unwrap();
    std::fs::write(&local, "[forge]\nprovider = \"auto\"\nurl = \"https://github.com\"\nproject = \"octo/app\"\n").unwrap();
    let wf = Workflow::detect_with(&slp_path, None).unwrap().unwrap();
    assert_eq!(wf.provider(), Some(Provider::GitHub));
    let err = wf.forge().err().unwrap().to_string();
    assert!(err.contains("https://github.com"), "no token: {err}");

    // GitHub has no push option to skip CI: "[skip ci]" goes into the commit message.
    let mut p = slp::load(&slp_path).unwrap();
    ops::build_project(&p, &["de".to_string()], None);
    p.record_scan();
    slp::save(&mut p, &slp_path).unwrap();
    let opts = SaveOptions { ci_skip: true, ..wf.save_defaults() };
    assert!(wf.save(&p, &opts).unwrap().pushed);
    assert_eq!(git(&work, &["log", "-1", "--format=%s"]), "fix: Update translations (app) [skip ci]");
    let st = wf.status(&p, StatusOptions::default()).unwrap();
    assert_eq!((st.provider, st.reference(5)), (Some(Provider::GitHub), "#5".to_string()));
    assert!(State::InReview.hint(st.provider).contains("pull request"));

    // Other servers stay GitLab with `auto`; `none` has no server.
    std::fs::write(&local, "[forge]\nprovider = \"auto\"\nurl = \"https://git.example.com\"\nproject = \"group/app\"\n").unwrap();
    assert_eq!(Workflow::detect_with(&slp_path, None).unwrap().unwrap().provider(), Some(Provider::GitLab));
    std::fs::write(&local, "[forge]\nprovider = \"none\"\n").unwrap();
    assert_eq!(Workflow::detect_with(&slp_path, None).unwrap().unwrap().provider(), None);
}

#[test]
fn local_commits_on_the_default_branch_are_kept() {
    let tmp = tempfile::tempdir().unwrap();
    let (_dev, work) = setup(tmp.path());
    std::fs::write(work.join("README.txt"), "changed\n").unwrap();
    git(&work, &["commit", "-q", "-am", "local"]);
    let wf = Workflow::detect_with(&work.join("l10n").join("app.slp"), None).unwrap().unwrap();
    let fetched = wf.fetch(&FetchOptions::default()).unwrap();
    let backup = fetched.saved_commits.expect("backup ref");
    assert_eq!(git(&work, &["log", "-1", "--format=%s", &backup]), "local");
    assert_eq!(std::fs::read_to_string(work.join("README.txt")).unwrap(), "app\n");
}

#[test]
fn uncommitted_configuration_survives_fetch() {
    let tmp = tempfile::tempdir().unwrap();
    let (dev, work) = setup(tmp.path());
    // The configuration only exists in the working copy (not committed) or on a personal branch.
    git(&dev, &["rm", "-q", "-r", ".susi"]);
    git(&dev, &["commit", "-q", "-m", "no config"]);
    git(&dev, &["push", "-q", "origin", "main"]);
    git(&work, &["switch", "-q", "-c", "l10n/old"]);
    std::fs::write(work.join("notes.txt"), "private\n").unwrap();
    let slp_path = work.join("l10n").join("app.slp");
    let wf = Workflow::detect_with(&slp_path, None).unwrap().unwrap();
    let fetched = wf.fetch(&FetchOptions::default()).unwrap();
    assert!(fetched.backup.is_some() && !work.join("notes.txt").exists());
    assert_eq!(std::fs::read_to_string(work.join(".susi").join("workflow.toml")).unwrap(), CONFIG);
    let wf = Workflow::detect_with(&slp_path, None).unwrap().expect("still configured after fetch");
    let p = slp::load(&slp_path).unwrap();
    let st = wf.status(&p, StatusOptions::default()).unwrap();
    assert!(st.warnings.iter().any(|w| w.contains(".susi/workflow.toml")), "{:?}", st.warnings);
    // Only the configuration is left: no further backup.
    assert!(wf.fetch(&FetchOptions::default()).unwrap().backup.is_none());
    assert!(work.join(".susi").join("workflow.toml").exists());
}

#[test]
fn screenshots_are_saved_with_the_project() {
    use susi_core::screenshots::{Link, ScreenshotStore, solid_png};
    let tmp = tempfile::tempdir().unwrap();
    let (_dev, work) = setup(tmp.path());
    let slp_path = work.join("l10n").join("app.slp");
    let wf = Workflow::detect_with(&slp_path, None).unwrap().expect("configured");
    let p = slp::load(&slp_path).unwrap();
    let mut shots = ScreenshotStore::open(&slp_path).unwrap();
    let id = shots.add_bytes(&solid_png(4, 4, [1, 2, 3]), Some("main"), None).unwrap();
    shots.link(&id, [Link::to_row(&p, &p.find_row("hello").unwrap(), None)]).unwrap();
    shots.save().unwrap();

    let st = wf.status(&p, StatusOptions::default()).unwrap();
    assert_eq!(st.state, State::Modified);
    assert!(st.changed.contains(&"l10n/app.files/screenshots.json".to_string()), "{:?}", st.changed);
    assert!(wf.save(&p, &wf.save_defaults()).unwrap().pushed);
    let files = git(&work, &["show", "--name-only", "--format=", "HEAD"]);
    assert!(files.contains("l10n/app.files/screenshots.json") && files.contains("l10n/app.files/screenshots/main."), "{files}");
    assert_eq!(wf.status(&p, StatusOptions::default()).unwrap().state, State::Saved);
}

#[test]
fn linked_translation_files_are_saved_with_the_project() {
    use susi_core::model::{Item, Node, Row};
    let tmp = tempfile::tempdir().unwrap();
    let (_dev, work) = setup(tmp.path());
    let slp_path = work.join("l10n").join("app.slp");
    let wf = Workflow::detect_with(&slp_path, None).unwrap().expect("configured");
    let mut p = slp::load(&slp_path).unwrap();
    let mut row = Row::new("blob", "");
    row.native = None;
    row.attrs.set("class", susi_core::custom_data::BINARY_ROW);
    let mut node = Node::new(susi_core::custom_data::BINARY_NODE, "blob");
    node.items.push(Item::Row(row));
    p.sources[0].items.push(Item::Node(node));
    let file = work.join("l10n").join("translated").join("blob.bin");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, [1, 2, 3]).unwrap();
    let r = p.rows().into_iter().find(|r| p.row(r).unwrap().id() == "blob").unwrap();
    susi_core::custom_data::load_translation_file(&mut p, &r, "de", &file, true).unwrap();
    slp::save(&mut p, &slp_path).unwrap();

    let st = wf.status(&p, StatusOptions::default()).unwrap();
    assert!(st.changed.contains(&"l10n/translated/blob.bin".to_string()), "{:?}", st.changed);
    assert!(wf.save(&p, &wf.save_defaults()).unwrap().pushed);
    let files = git(&work, &["show", "--name-only", "--format=", "HEAD"]);
    assert!(files.contains("l10n/translated/blob.bin") && files.contains("l10n/app.slp"), "{files}");
}
