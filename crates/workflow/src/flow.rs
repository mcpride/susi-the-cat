//! Workflow state and the actions "get translation project", "save changes" and "prepare publication".

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde::Serialize;
use susi_core::i18n::t;
use susi_core::{Project, tf};

use crate::config::{self, Config, Files};
use crate::forge::{self, Forge, MergeRequest, MergeRequestDraft, MergeRequestQuery, MergeRequestState, User};
use crate::git::{Change, Git, Snapshot};
use crate::gitlab::GitLab;
use crate::secret::{self, TokenRef};
use crate::{Error, Result, paths};

/// State of the working copy from the translator's point of view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    /// On the default branch, nothing changed, nothing new on the server.
    UpToDate,
    /// On the default branch, the server has a newer version.
    UpdateAvailable,
    /// Project files changed or commits not yet pushed.
    Modified,
    /// Changes are on the server in a personal branch, no merge request yet.
    Saved,
    /// A merge request is open.
    InReview,
    /// The merge request was merged (or the branch removed on the server).
    Published,
    /// A specific commit is checked out.
    Detached,
}

impl State {
    pub fn label(self) -> String {
        match self {
            State::UpToDate => t("Up to date"),
            State::UpdateAvailable => t("Update available"),
            State::Modified => t("Changes not saved on the server"),
            State::Saved => t("Saved, not published"),
            State::InReview => t("In review"),
            State::Published => t("Published"),
            State::Detached => t("Fixed version"),
        }
        .to_string()
    }

    /// What to do next.
    pub fn hint(self) -> String {
        match self {
            State::UpToDate => t("The translation project is up to date. Translate, then save your changes."),
            State::UpdateAvailable => t("A newer version is available on the server: get the translation project."),
            State::Modified => t("There are changes that are not yet saved on the server: save the changes."),
            State::Saved => t("Your changes are saved on the server. When you are done, prepare the publication."),
            State::InReview => t("The merge request is waiting for review. You can still save further changes."),
            State::Published => t("Your changes were accepted. Get the translation project to continue with the latest version."),
            State::Detached => t("A fixed version is checked out. Get the translation project to continue working."),
        }
        .to_string()
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StatusOptions {
    /// Run `git fetch` first.
    pub fetch: bool,
    /// Ask the server for merge requests.
    pub online: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Status {
    pub state: State,
    /// `None` when detached.
    pub branch: Option<String>,
    pub default_branch: String,
    /// Changed project files (working copy paths).
    pub changed: Vec<String>,
    /// Local commits not on the server.
    pub unpushed: usize,
    /// Commits on the server's default branch missing locally (on the default branch only).
    pub behind: usize,
    /// The branch was removed on the server (typically after merging).
    pub upstream_gone: bool,
    pub merge_request: Option<MergeRequest>,
    /// Other open translation merge requests (possible conflicts in the project file).
    pub other_requests: Vec<MergeRequest>,
    /// Non-fatal problems (offline, missing token, ...).
    pub warnings: Vec<String>,
}

impl Status {
    pub fn on_default_branch(&self) -> bool {
        self.branch.as_deref() == Some(self.default_branch.as_str())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FetchOptions {
    /// Branch to check out (default: the default branch).
    pub branch: Option<String>,
    /// Specific commit, tag or remote branch to check out read-only (overrides `branch`).
    pub commit: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FetchReport {
    /// Branch or commit now checked out.
    pub target: String,
    /// Short commit hash.
    pub commit: String,
    /// Stash message when local changes were set aside (`git stash list`).
    pub backup: Option<String>,
    /// Backup ref of local commits that were not on the server.
    pub saved_commits: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveOptions {
    /// Commit message without prefix.
    pub message: String,
    /// Conventional commit type (`fix`, `feat`, `chore`, empty).
    pub prefix: String,
    pub ci_skip: bool,
    /// Name of a new branch (default: from the pattern when on the default branch).
    pub branch: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SaveReport {
    pub branch: String,
    pub created_branch: bool,
    /// Short hash of the new commit.
    pub commit: Option<String>,
    pub message: String,
    pub files: Vec<String>,
    pub pushed: bool,
    pub nothing_to_do: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishOptions {
    pub title: String,
    pub description: String,
    /// Target branch (empty: configured target or default branch).
    pub target: String,
    /// Usernames.
    pub reviewers: Vec<String>,
    pub squash: bool,
    pub remove_source_branch: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PublishReport {
    pub merge_request: MergeRequest,
    pub created: bool,
}

/// A git working copy configured for the translation workflow, for one project file.
pub struct Workflow {
    git: Git,
    config: Config,
    files: Files,
    root: PathBuf,
    slp: PathBuf,
    /// Working copy path of the project directory (`l10n/`, empty at the root).
    prefix: String,
    default_branch: OnceLock<String>,
}

fn now() -> chrono::DateTime<chrono::Local> {
    chrono::Local::now()
}

/// Lower-case branch name component: `Anna Müller` → `anna.mueller`.
pub fn sanitize(s: &str) -> String {
    let mut out = String::new();
    for c in s.trim().to_lowercase().chars() {
        match c {
            'ä' => out.push_str("ae"),
            'ö' => out.push_str("oe"),
            'ü' => out.push_str("ue"),
            'ß' => out.push_str("ss"),
            c if c.is_ascii_alphanumeric() || c == '_' || c == '-' => out.push(c),
            c if c.is_whitespace() || c == '.' => out.push('.'),
            _ => out.push('-'),
        }
    }
    let mut cleaned = String::new();
    for c in out.chars() {
        if matches!(c, '.' | '-') && cleaned.ends_with(['.', '-']) {
            continue;
        }
        cleaned.push(c);
    }
    cleaned.trim_matches(['.', '-']).to_string()
}

/// Path (relative to the working copy root) inside the repository configuration directory.
fn is_config_path(path: &str) -> bool {
    path.strip_prefix(config::REPO_DIR).is_some_and(|r| r.is_empty() || r.starts_with('/'))
}

impl Workflow {
    /// Workflow for the project file `slp`, `None` if it is not in a configured git working copy.
    pub fn detect(slp: &Path) -> Result<Option<Workflow>> {
        let user = susi_core::settings::config_dir().join(config::USER_FILE);
        Self::detect_with(slp, Some(&user))
    }

    /// Like [`Workflow::detect`] with an explicit user configuration file.
    pub fn detect_with(slp: &Path, user_config: Option<&Path>) -> Result<Option<Workflow>> {
        let slp = std::path::absolute(slp).unwrap_or_else(|_| slp.to_path_buf());
        let Some(dir) = slp.parent().filter(|d| d.is_dir()) else { return Ok(None) };
        let probe = Git::new(dir);
        let top = match probe.query(&["rev-parse", "--show-toplevel"]) {
            Ok(top) => top,
            Err(Error::Git { message, .. }) if message.contains("safe.directory") => {
                return Err(Error::Workflow(tf!(
                    "git does not trust the working copy (it belongs to another user). A developer can allow it with: git config --global --add safe.directory \"{dir}\"",
                    dir = dir.display().to_string().replace('\\', "/")
                )));
            }
            Err(_) => return Ok(None),
        };
        let prefix = probe.query(&["rev-parse", "--show-prefix"])?;
        let git_dir = PathBuf::from(probe.query(&["rev-parse", "--absolute-git-dir"])?);
        let root = PathBuf::from(top);
        let Some((config, files)) = config::load(user_config, &root.join(config::REPO_FILE), &git_dir.join(config::LOCAL_FILE))? else {
            return Ok(None);
        };
        Ok(Some(Workflow { git: Git::new(&root), config, files, root, slp, prefix: prefix.trim().to_string(), default_branch: OnceLock::new() }))
    }

    /// Writes the configuration template `.susi/workflow.toml` into the working copy containing `slp`.
    pub fn init(slp: &Path) -> Result<PathBuf> {
        let slp = std::path::absolute(slp).unwrap_or_else(|_| slp.to_path_buf());
        let dir = slp.parent().unwrap_or(Path::new("."));
        let top = Git::new(dir).query(&["rev-parse", "--show-toplevel"]).map_err(|_| Error::NotConfigured(dir.display().to_string()))?;
        let path = PathBuf::from(top).join(config::REPO_FILE);
        if path.exists() {
            return Err(Error::Workflow(tf!("{path} already exists", path = path.display())));
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| susi_core::Error::io(parent, e))?;
        }
        std::fs::write(&path, config::TEMPLATE).map_err(|e| susi_core::Error::io(&path, e))?;
        Ok(path)
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn config_files(&self) -> &Files {
        &self.files
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn slp(&self) -> &Path {
        &self.slp
    }

    /// git commands run since the last call (for the output window).
    pub fn take_log(&self) -> Vec<String> {
        self.git.take_log()
    }

    fn remote(&self) -> &str {
        self.config.git.remote.trim()
    }

    fn remote_ref(&self, branch: &str) -> String {
        format!("refs/remotes/{}/{branch}", self.remote())
    }

    /// Default branch: configured, else the remote's HEAD, else `main`/`master`.
    pub fn default_branch(&self) -> String {
        self.default_branch
            .get_or_init(|| {
                let configured = self.config.git.default_branch.trim();
                if !configured.is_empty() {
                    return configured.to_string();
                }
                let remote = self.remote();
                let strip = |s: &str| s.strip_prefix(&format!("{remote}/")).map(str::to_string);
                if let Some(b) = self.git.try_query(&["symbolic-ref", "--short", &format!("refs/remotes/{remote}/HEAD")]).as_deref().and_then(strip) {
                    return b;
                }
                if let Some(out) = self.git.try_query(&["ls-remote", "--symref", remote, "HEAD"])
                    && let Some(b) = out.lines().find_map(|l| l.strip_prefix("ref: refs/heads/")).and_then(|l| l.split_whitespace().next())
                {
                    return b.to_string();
                }
                ["main", "master"].into_iter().find(|b| self.git.ref_exists(&self.remote_ref(b))).unwrap_or("main").to_string()
            })
            .clone()
    }

    /// Target branch for merge requests.
    pub fn target_branch(&self) -> String {
        let t = self.config.publish.target_branch.trim();
        if t.is_empty() { self.default_branch() } else { t.to_string() }
    }

    fn project_name(&self) -> String {
        self.slp.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default()
    }

    /// Display name of the user (configured, git `user.name`, login name).
    pub fn user_name(&self) -> String {
        let configured = self.config.git.user.trim();
        if !configured.is_empty() {
            return configured.to_string();
        }
        self.git
            .try_query(&["config", "user.name"])
            .or_else(|| std::env::var("USERNAME").ok())
            .or_else(|| std::env::var("USER").ok())
            .unwrap_or_else(|| "user".to_string())
    }

    fn vars(&self) -> Vec<(&'static str, String)> {
        let now = now();
        vec![
            ("user", self.user_name()),
            ("date", now.format("%Y-%m-%d").to_string()),
            ("time", now.format("%H%M").to_string()),
            ("project", self.project_name()),
            ("branch", self.git.current_branch().unwrap_or_default()),
        ]
    }

    fn expand(&self, template: &str) -> String {
        let vars = self.vars();
        let refs: Vec<(&str, &str)> = vars.iter().map(|(k, v)| (*k, v.as_str())).collect();
        config::expand(template, &refs)
    }

    /// Default options of "save changes" (from the configuration).
    pub fn save_defaults(&self) -> SaveOptions {
        let s = &self.config.save;
        SaveOptions { message: self.expand(&s.message), prefix: s.semantic_prefix.trim().to_string(), ci_skip: s.ci_skip, branch: None }
    }

    /// Default options of "prepare publication" (from the configuration).
    pub fn publish_defaults(&self) -> PublishOptions {
        let p = &self.config.publish;
        PublishOptions {
            title: self.expand(&p.title),
            description: self.expand(&p.description),
            target: self.target_branch(),
            reviewers: p.reviewers.clone(),
            squash: p.squash,
            remove_source_branch: p.remove_source_branch,
        }
    }

    /// Name for a new branch from the pattern, unique locally and on the server.
    pub fn new_branch_name(&self) -> String {
        let vars: Vec<(&str, String)> = self.vars().into_iter().map(|(k, v)| (k, sanitize(&v))).collect();
        let refs: Vec<(&str, &str)> = vars.iter().map(|(k, v)| (*k, v.as_str())).collect();
        let base = config::expand(&self.config.git.branch_pattern, &refs);
        let base: String = base.split('/').map(sanitize).filter(|s| !s.is_empty()).collect::<Vec<_>>().join("/");
        let base = if base.is_empty() { "l10n".to_string() } else { base };
        let taken = |name: &str| self.git.ref_exists(&format!("refs/heads/{name}")) || self.git.ref_exists(&self.remote_ref(name));
        if !taken(&base) {
            return base;
        }
        (2..1000).map(|n| format!("{base}-{n}")).find(|n| !taken(n)).unwrap_or(base)
    }

    /// Project files as working copy paths (always including the project file).
    pub fn project_paths(&self, p: &Project) -> Vec<String> {
        let dir = self.slp.parent().unwrap_or(&self.root);
        let mut files = paths::repo_files(p, dir, &self.prefix);
        if let Some(name) = self.slp.file_name().and_then(|n| n.to_str())
            && let Some(slp) = paths::repo_path(&self.prefix, name)
            && !files.contains(&slp)
        {
            files.push(slp);
        }
        files
    }

    fn project_changes(&self, p: &Project, changes: &[Change]) -> Vec<String> {
        let files = self.project_paths(p);
        let mut changed: Vec<String> = changes.iter().map(|c| c.path.clone()).filter(|f| paths::matches(f, &files, &self.config.git.paths)).collect();
        changed.sort();
        changed.dedup();
        changed
    }

    /// Changed project files.
    pub fn changed_files(&self, p: &Project) -> Result<Vec<String>> {
        Ok(self.project_changes(p, &self.git.snapshot()?.changes))
    }

    /// Local commits that are not on the server.
    fn unpushed(&self, snap: &Snapshot) -> usize {
        if snap.upstream_gone() {
            return 0;
        }
        if let Some((ahead, _)) = snap.ahead_behind {
            return ahead;
        }
        let base = self.remote_ref(&self.default_branch());
        if self.git.ref_exists(&base) { self.git.count(&format!("{base}..HEAD")) } else { 0 }
    }

    /// Current state. With `online`, merge requests are read from the server (errors become warnings).
    pub fn status(&self, p: &Project, opts: StatusOptions) -> Result<Status> {
        let mut warnings = Vec::new();
        if opts.fetch
            && let Err(e) = self.git.query(&["fetch", "--prune", self.remote()])
        {
            warnings.push(tf!("The server could not be reached: {error}", error = e));
        }
        let snap = self.git.snapshot()?;
        if let Some(c) = snap.changes.iter().find(|c| c.code == "??" && is_config_path(&c.path)) {
            warnings.push(tf!("The workflow configuration {path} is not committed. A developer should commit it to the default branch.", path = c.path));
        }
        let default_branch = self.default_branch();
        let changed = self.project_changes(p, &snap.changes);
        let on_default = snap.branch.as_deref() == Some(default_branch.as_str());
        let upstream_gone = snap.branch.is_some() && !on_default && snap.upstream_gone();
        let unpushed = self.unpushed(&snap);
        let behind = match snap.ahead_behind {
            _ if !on_default => 0,
            Some((_, behind)) => behind,
            None => self.git.count(&format!("HEAD..{}", self.remote_ref(&default_branch))),
        };
        let mut st = Status {
            state: State::UpToDate,
            branch: snap.branch,
            default_branch,
            changed,
            unpushed,
            behind,
            upstream_gone,
            merge_request: None,
            other_requests: Vec::new(),
            warnings,
        };
        if opts.online && self.config.has_forge() {
            self.query_forge(&mut st);
        }
        st.state = state_of(&st);
        Ok(st)
    }

    fn query_forge(&self, st: &mut Status) {
        let forge = match self.forge() {
            Ok(f) => f,
            Err(e) => return st.warnings.push(e.to_string()),
        };
        if let Some(b) = st.branch.as_deref().filter(|b| *b != st.default_branch) {
            let q = MergeRequestQuery { source_branch: Some(b.to_string()), ..Default::default() };
            match forge.merge_requests(&q) {
                Ok(list) => {
                    st.merge_request = list
                        .iter()
                        .find(|m| m.state == MergeRequestState::Open)
                        .or_else(|| list.iter().find(|m| m.state == MergeRequestState::Merged))
                        .cloned();
                }
                Err(e) => return st.warnings.push(e.to_string()),
            }
        }
        let stem = self.config.git.branch_pattern.split('{').next().unwrap_or_default().to_string();
        let q = MergeRequestQuery { target_branch: Some(self.target_branch()), only_open: true, ..Default::default() };
        match forge.merge_requests(&q) {
            Ok(list) => {
                st.other_requests =
                    list.into_iter().filter(|m| Some(m.source_branch.as_str()) != st.branch.as_deref() && m.source_branch.starts_with(&stem)).collect();
            }
            Err(e) => st.warnings.push(e.to_string()),
        }
    }

    /// Branches on the server (for the advanced options).
    pub fn remote_branches(&self) -> Vec<String> {
        let out = self.git.try_query(&["for-each-ref", "--format=%(refname:strip=3)", &format!("refs/remotes/{}", self.remote())]).unwrap_or_default();
        out.lines().map(str::trim).filter(|b| !b.is_empty() && *b != "HEAD").map(str::to_string).collect()
    }

    /// People who changed the project file, most recent first: `(name, email)`.
    pub fn committers(&self) -> Vec<(String, String)> {
        let Some(slp) = self.slp.file_name().and_then(|n| n.to_str()).and_then(|n| paths::repo_path(&self.prefix, n)) else { return Vec::new() };
        let out = self.git.try_query(&["log", "-n", "500", "--format=%an%x09%ae", "--", &slp]).unwrap_or_default();
        let mut seen = std::collections::HashSet::new();
        out.lines()
            .filter_map(|l| l.split_once('\t'))
            .filter(|(_, mail)| seen.insert(mail.to_lowercase()))
            .map(|(n, m)| (n.to_string(), m.to_string()))
            .collect()
    }

    /// "Get translation project": sets local changes aside and checks out the latest version of the
    /// default branch (or the given branch/commit).
    pub fn fetch(&self, opts: &FetchOptions) -> Result<FetchReport> {
        let remote = self.remote().to_string();
        self.git.run(&["fetch", "--prune", &remote])?;
        let commit = opts.commit.as_deref().map(str::trim).filter(|c| !c.is_empty());
        let branch = opts.branch.as_deref().map(str::trim).filter(|b| !b.is_empty()).map_or_else(|| self.default_branch(), str::to_string);
        if commit.is_none() && !self.git.ref_exists(&self.remote_ref(&branch)) {
            return Err(Error::Workflow(tf!("The branch \"{branch}\" does not exist on the server", branch = branch)));
        }
        let ts = now().format("%Y%m%d-%H%M%S").to_string();
        let mut backup = None;
        let snap = self.git.snapshot()?;
        // The workflow configuration stays in place (it may not be committed yet), everything else is set aside.
        let config_path = self.root.join(config::REPO_FILE);
        let kept_config = std::fs::read(&config_path).ok();
        if snap.changes.iter().any(|c| !is_config_path(&c.path)) {
            let msg = format!("susi backup {ts} ({})", snap.branch.as_deref().unwrap_or("HEAD"));
            let exclude = format!(":(top,exclude){}", config::REPO_DIR);
            self.git.run(&["stash", "push", "--include-untracked", "-m", &msg, "--", ":/", &exclude])?;
            backup = Some(msg);
        }
        let mut saved_commits = None;
        if commit.is_none()
            && self.git.ref_exists(&format!("refs/heads/{branch}"))
            && self.git.count(&format!("{}..refs/heads/{branch}", self.remote_ref(&branch))) > 0
        {
            let r = format!("refs/susi/backup/{branch}-{ts}");
            self.git.run(&["update-ref", &r, &format!("refs/heads/{branch}")])?;
            saved_commits = Some(r);
        }
        // --discard-changes: only the configuration can still differ; the checked-out version wins.
        let target = match commit {
            Some(c) => {
                self.git.run(&["switch", "--discard-changes", "--detach", c])?;
                c.to_string()
            }
            None => {
                self.git.run(&["switch", "--discard-changes", "--force-create", &branch, "--track", &format!("{remote}/{branch}")])?;
                branch
            }
        };
        if let Some(bytes) = kept_config.filter(|_| !config_path.exists()) {
            // Not part of the checked-out version: keep the workflow enabled.
            if let Some(dir) = config_path.parent() {
                std::fs::create_dir_all(dir).map_err(|e| susi_core::Error::io(dir, e))?;
            }
            std::fs::write(&config_path, bytes).map_err(|e| susi_core::Error::io(&config_path, e))?;
        }
        let commit = self.git.query(&["rev-parse", "--short", "HEAD"])?;
        Ok(FetchReport { target, commit, backup, saved_commits })
    }

    fn check_identity(&self) -> Result<()> {
        if !self.git.has_identity() {
            return Err(Error::Workflow(
                t("git does not know your name and e-mail address yet. A developer can set them with git config --global user.name / user.email.").to_string(),
            ));
        }
        Ok(())
    }

    /// "Save changes": commits the changed project files (on a new personal branch when on the
    /// default branch) and pushes them. The project file must have been saved before.
    pub fn save(&self, p: &Project, opts: &SaveOptions) -> Result<SaveReport> {
        let snap = self.git.snapshot()?;
        let current = snap.branch.clone();
        let default_branch = self.default_branch();
        let explicit = opts.branch.as_deref().map(str::trim).filter(|b| !b.is_empty() && Some(*b) != current.as_deref());
        if explicit.is_none()
            && snap.upstream_gone()
            && let Some(b) = current.as_deref().filter(|b| *b != default_branch)
        {
            return Err(Error::Workflow(tf!(
                "The branch \"{branch}\" was already published and removed on the server. Get the translation project first.",
                branch = b
            )));
        }
        let changed = self.project_changes(p, &snap.changes);
        let unpushed = self.unpushed(&snap);
        let message = config::commit_message(&opts.prefix, &opts.message, opts.ci_skip && !self.config.is_gitlab());
        let mut report = SaveReport {
            branch: current.clone().unwrap_or_default(),
            created_branch: false,
            commit: None,
            message: message.clone(),
            files: changed.clone(),
            pushed: false,
            nothing_to_do: false,
        };
        if changed.is_empty() && unpushed == 0 && explicit.is_none() {
            report.nothing_to_do = true;
            return Ok(report);
        }
        if !changed.is_empty() {
            self.check_identity()?;
        }
        if explicit.is_some() || current.is_none() || current.as_deref() == Some(default_branch.as_str()) {
            let name = explicit.map_or_else(|| self.new_branch_name(), str::to_string);
            self.git.query(&["check-ref-format", "--branch", &name]).map_err(|_| Error::Workflow(tf!("Invalid branch name \"{name}\"", name = name)))?;
            self.git.run(&["switch", "-c", &name])?;
            report.branch = name;
            report.created_branch = true;
        }
        if !changed.is_empty() {
            let spec = changed.join("\0");
            self.git.run_input(&["--literal-pathspecs", "add", "-A", "--pathspec-from-file=-", "--pathspec-file-nul"], spec.as_bytes())?;
            self.git.run_input(&["--literal-pathspecs", "commit", "-q", "-m", &message, "--pathspec-from-file=-", "--pathspec-file-nul"], spec.as_bytes())?;
            report.commit = self.git.try_query(&["rev-parse", "--short", "HEAD"]);
        }
        let mut push = vec!["push", "-u"];
        if opts.ci_skip && self.config.is_gitlab() {
            push.extend(["-o", "ci.skip"]);
        }
        push.extend([self.remote(), report.branch.as_str()]);
        self.git.run(&push)?;
        report.pushed = true;
        Ok(report)
    }

    /// Web address and project path of the server.
    pub fn forge_location(&self) -> Result<(String, String)> {
        let f = &self.config.forge;
        let (url, project) = (f.url.trim().trim_end_matches('/'), f.project.trim().trim_matches('/'));
        if !url.is_empty() && !project.is_empty() {
            return Ok((url.to_string(), project.to_string()));
        }
        let remote_url = self.git.query(&["remote", "get-url", self.remote()])?;
        let (u, p) = forge::parse_remote(&remote_url).ok_or_else(|| {
            Error::Config(tf!("The server address cannot be derived from the remote URL \"{url}\"; set forge.url and forge.project.", url = remote_url))
        })?;
        Ok((if url.is_empty() { u } else { url.to_string() }, if project.is_empty() { p } else { project.to_string() }))
    }

    fn token(&self, url: &str) -> Result<Option<String>> {
        Ok(secret::resolve(&TokenRef::parse(&self.config.forge.token)?, &self.git, url))
    }

    /// Whether an access token is available.
    pub fn has_token(&self) -> bool {
        self.forge_location().ok().and_then(|(url, _)| self.token(&url).ok().flatten()).is_some()
    }

    /// Stores the access token with git's credential helper.
    pub fn store_token(&self, token: &str) -> Result<()> {
        let (url, _) = self.forge_location()?;
        secret::store(&self.git, &url, token)
    }

    /// Removes the stored access token.
    pub fn forget_token(&self) -> Result<()> {
        let (url, _) = self.forge_location()?;
        secret::forget(&self.git, &url)
    }

    /// Server client according to the configuration.
    pub fn forge(&self) -> Result<Box<dyn Forge>> {
        if !self.config.has_forge() {
            return Err(Error::Workflow(t("No server is configured for merge requests (forge.provider = \"none\").").to_string()));
        }
        let (url, project) = self.forge_location()?;
        let token = self.token(&url)?;
        if token.is_none() {
            return Err(Error::Forge(tf!(
                "No access token for {url}. Store one (Workflow > Sign in to the server…, susi-cli workflow login) or set {env}.",
                url = url,
                env = secret::ENV_TOKEN
            )));
        }
        Ok(Box::new(GitLab::new(&url, &project, token)))
    }

    /// Reviewer candidates: project members, previous committers of the project file first.
    pub fn reviewer_candidates(&self, forge: &dyn Forge) -> Result<Vec<User>> {
        let mut members = forge.members()?;
        let committers: Vec<String> = self.committers().into_iter().map(|(n, _)| n.to_lowercase()).collect();
        let rank = |u: &User| committers.iter().position(|c| *c == u.name.to_lowercase() || *c == u.username.to_lowercase()).unwrap_or(usize::MAX);
        members.sort_by_key(|u| rank(u));
        Ok(members)
    }

    /// "Prepare publication": creates or updates the merge request of the current branch.
    pub fn publish(&self, forge: &dyn Forge, opts: &PublishOptions) -> Result<PublishReport> {
        let default_branch = self.default_branch();
        let snap = self.git.snapshot()?;
        let Some(branch) = snap.branch.clone().filter(|b| *b != default_branch) else {
            return Err(Error::Workflow(t("Save your changes first: publishing needs your own branch.").to_string()));
        };
        if snap.ahead_behind.is_none() || self.unpushed(&snap) > 0 {
            return Err(Error::Workflow(t("Not all changes are saved on the server yet: save the changes first.").to_string()));
        }
        let target = if opts.target.trim().is_empty() { self.target_branch() } else { opts.target.trim().to_string() };
        let draft = MergeRequestDraft {
            source_branch: branch.clone(),
            target_branch: target,
            title: opts.title.trim().to_string(),
            description: opts.description.clone(),
            reviewers: opts.reviewers.iter().map(|r| r.trim().trim_start_matches('@').to_string()).filter(|r| !r.is_empty()).collect(),
            squash: opts.squash,
            remove_source_branch: opts.remove_source_branch,
        };
        let q = MergeRequestQuery { source_branch: Some(branch), only_open: true, ..Default::default() };
        match forge.merge_requests(&q)?.into_iter().next() {
            Some(mr) => Ok(PublishReport { merge_request: forge.update_merge_request(mr.iid, &draft)?, created: false }),
            None => Ok(PublishReport { merge_request: forge.create_merge_request(&draft)?, created: true }),
        }
    }
}

fn state_of(st: &Status) -> State {
    if !st.changed.is_empty() || st.unpushed > 0 {
        return State::Modified;
    }
    let Some(branch) = st.branch.as_deref() else { return State::Detached };
    if branch == st.default_branch {
        return if st.behind > 0 { State::UpdateAvailable } else { State::UpToDate };
    }
    match st.merge_request.as_ref().map(|m| m.state) {
        Some(MergeRequestState::Open) => State::InReview,
        Some(MergeRequestState::Merged) => State::Published,
        _ if st.upstream_gone => State::Published,
        _ => State::Saved,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizes_branch_components() {
        assert_eq!(sanitize("Anna Müller"), "anna.mueller");
        assert_eq!(sanitize(" Groß/Straße? "), "gross-strasse");
        assert_eq!(sanitize("a..b--c"), "a.b-c");
    }
}
