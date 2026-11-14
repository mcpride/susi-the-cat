//! Workflow configuration.
//!
//! Three TOML files with the same structure are merged (later files override earlier ones):
//!
//! 1. user defaults: `<config dir>/workflow.toml` (`%APPDATA%\susi` on Windows),
//! 2. repository: `.susi/workflow.toml` (checked in, shared by the team),
//! 3. working copy: `<git dir>/susi/workflow.local.toml` (inside `.git`, can never be committed).
//!
//! The workflow is enabled when the repository or the working copy file exists. No file may contain
//! a secret: the access token is configured as a reference ([`crate::secret::TokenRef`]).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use susi_core::tf;

use crate::secret::TokenRef;
use crate::{Error, Result};

/// Directory of the repository configuration, relative to the working copy root.
pub const REPO_DIR: &str = ".susi";
/// Repository configuration, relative to the working copy root.
pub const REPO_FILE: &str = ".susi/workflow.toml";
/// Working copy configuration, relative to the git directory.
pub const LOCAL_FILE: &str = "susi/workflow.local.toml";
/// User defaults, relative to Susi's configuration directory.
pub const USER_FILE: &str = "workflow.toml";

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub forge: ForgeConfig,
    pub git: GitConfig,
    pub save: SaveConfig,
    pub publish: PublishConfig,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ForgeConfig {
    /// `gitlab` or `none` (git only, without merge requests).
    pub provider: String,
    /// Web address of the server, e.g. `https://gitlab.example.com` (empty: derived from the remote URL).
    pub url: String,
    /// Project path on the server, e.g. `group/app` (empty: derived from the remote URL).
    pub project: String,
    /// Reference to the access token: `auto`, `env:NAME`, `credential[:user]` or `none`.
    pub token: String,
}

impl Default for ForgeConfig {
    fn default() -> Self {
        Self { provider: "gitlab".into(), url: String::new(), project: String::new(), token: "auto".into() }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GitConfig {
    pub remote: String,
    /// Empty: the remote's default branch.
    pub default_branch: String,
    /// Name of new branches; placeholders `{user}`, `{date}`, `{time}`, `{project}`.
    pub branch_pattern: String,
    /// Value of `{user}` (empty: git `user.name`).
    pub user: String,
    /// Further files or directories (relative to the working copy root) saved with the project.
    pub paths: Vec<String>,
}

impl Default for GitConfig {
    fn default() -> Self {
        Self { remote: "origin".into(), default_branch: String::new(), branch_pattern: "l10n/{user}/{date}".into(), user: String::new(), paths: Vec::new() }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SaveConfig {
    /// Commit message; placeholders `{user}`, `{date}`, `{time}`, `{project}`.
    pub message: String,
    /// Conventional commit type put in front of the message (`fix`, `feat`, `chore`; empty: none).
    pub semantic_prefix: String,
    /// Skip the CI pipeline for the push.
    pub ci_skip: bool,
    /// Build the localized files before saving.
    pub build: bool,
}

impl Default for SaveConfig {
    fn default() -> Self {
        Self { message: "Update translations ({project})".into(), semantic_prefix: "fix".into(), ci_skip: false, build: true }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PublishConfig {
    /// Merge request title; placeholders as for the commit message plus `{branch}`.
    pub title: String,
    pub description: String,
    /// Empty: the default branch.
    pub target_branch: String,
    /// Usernames proposed as reviewers.
    pub reviewers: Vec<String>,
    /// Offer previous committers of the project as reviewers.
    pub offer_committers: bool,
    pub squash: bool,
    pub remove_source_branch: bool,
}

impl Default for PublishConfig {
    fn default() -> Self {
        Self {
            title: "Translations: {project} ({user}, {date})".into(),
            description: "Translations updated with Susi.".into(),
            target_branch: String::new(),
            reviewers: Vec::new(),
            offer_committers: true,
            squash: false,
            remove_source_branch: true,
        }
    }
}

/// Configuration files found for a working copy.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Files {
    pub user: Option<PathBuf>,
    pub repo: Option<PathBuf>,
    pub local: Option<PathBuf>,
}

impl Files {
    pub fn all(&self) -> impl Iterator<Item = &PathBuf> {
        self.user.iter().chain(&self.repo).chain(&self.local)
    }
}

/// Template for `.susi/workflow.toml` (written by `susi-cli workflow init`).
pub const TEMPLATE: &str = r#"# Susi translation workflow (see README, "Git workflow").
# Checked in with the repository - never put secrets here, only references to them.
# Personal overrides: .git/susi/workflow.local.toml; user defaults: %APPDATA%\susi\workflow.toml

[forge]
provider = "gitlab"        # gitlab | none (git only)
# url = "https://gitlab.example.com"   # default: derived from the remote URL
# project = "group/app"                # default: derived from the remote URL
token = "auto"             # auto | env:VARIABLE | credential | credential:<user> | none

[git]
remote = "origin"
# default_branch = "main"              # default: the remote's default branch
branch_pattern = "l10n/{user}/{date}"
# paths = ["docs/i18n"]                # further files saved together with the project

[save]
message = "Update translations ({project})"
semantic_prefix = "fix"    # fix = new patch version, feat = new minor version, chore = no release, "" = none
ci_skip = false
build = true

[publish]
title = "Translations: {project} ({user}, {date})"
description = "Translations updated with Susi."
# target_branch = "main"
reviewers = []             # GitLab usernames
offer_committers = true
squash = false
remove_source_branch = true
"#;

fn read_table(path: &Path) -> Result<Option<toml::Table>> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(Error::Core(susi_core::Error::io(path, e))),
    };
    let table: toml::Table = text.parse().map_err(|e| Error::Config(format!("{}: {e}", path.display())))?;
    if let Some(key) = find_secret(&table, "") {
        return Err(Error::Config(tf!(
            "{path}: \"{key}\" looks like an access token. Configuration files must not contain secrets; use a reference such as token = \"credential\" or token = \"env:SUSI_FORGE_TOKEN\".",
            path = path.display(),
            key = key
        )));
    }
    Ok(Some(table))
}

/// Key of the first value that looks like a token of a known forge.
fn find_secret(table: &toml::Table, prefix: &str) -> Option<String> {
    const TOKEN_PREFIXES: [&str; 6] = ["glpat-", "gloas-", "gldt-", "ghp_", "gho_", "github_pat_"];
    table.iter().find_map(|(k, v)| {
        let key = if prefix.is_empty() { k.clone() } else { format!("{prefix}.{k}") };
        match v {
            toml::Value::String(s) if TOKEN_PREFIXES.iter().any(|p| s.trim().starts_with(p)) => Some(key),
            toml::Value::Table(t) => find_secret(t, &key),
            _ => None,
        }
    })
}

fn merge(base: &mut toml::Table, over: toml::Table) {
    for (k, v) in over {
        match (base.get_mut(&k), v) {
            (Some(toml::Value::Table(b)), toml::Value::Table(o)) => merge(b, o),
            (_, v) => {
                base.insert(k, v);
            }
        }
    }
}

/// Loads and merges the configuration. `None` if neither the repository nor the working copy file exists.
pub fn load(user: Option<&Path>, repo: &Path, local: &Path) -> Result<Option<(Config, Files)>> {
    let mut files = Files::default();
    let repo_table = read_table(repo)?;
    let local_table = read_table(local)?;
    if repo_table.is_none() && local_table.is_none() {
        return Ok(None);
    }
    let mut merged = toml::Table::new();
    if let Some(u) = user
        && let Some(t) = read_table(u)?
    {
        files.user = Some(u.to_path_buf());
        merge(&mut merged, t);
    }
    if let Some(t) = repo_table {
        files.repo = Some(repo.to_path_buf());
        merge(&mut merged, t);
    }
    if let Some(t) = local_table {
        files.local = Some(local.to_path_buf());
        merge(&mut merged, t);
    }
    let config: Config = toml::Value::Table(merged).try_into().map_err(|e: toml::de::Error| Error::Config(e.to_string()))?;
    config.validate()?;
    Ok(Some((config, files)))
}

impl Config {
    pub fn validate(&self) -> Result<()> {
        TokenRef::parse(&self.forge.token)?;
        match self.forge.provider.trim().to_ascii_lowercase().as_str() {
            "gitlab" | "none" | "" => {}
            other => return Err(Error::Config(tf!("Unknown provider \"{provider}\" (supported: gitlab, none)", provider = other))),
        }
        if self.git.remote.trim().is_empty() {
            return Err(Error::Config(susi_core::i18n::t("The git remote must not be empty").to_string()));
        }
        if self.git.branch_pattern.trim().is_empty() {
            return Err(Error::Config(susi_core::i18n::t("The branch pattern must not be empty").to_string()));
        }
        Ok(())
    }

    pub fn has_forge(&self) -> bool {
        !matches!(self.forge.provider.trim().to_ascii_lowercase().as_str(), "none" | "")
    }

    pub fn is_gitlab(&self) -> bool {
        self.forge.provider.trim().eq_ignore_ascii_case("gitlab")
    }

    /// The effective configuration as TOML.
    pub fn to_toml(&self) -> String {
        toml::to_string_pretty(self).unwrap_or_default()
    }
}

/// Replaces `{name}` placeholders.
pub fn expand(template: &str, vars: &[(&str, &str)]) -> String {
    let mut s = template.to_string();
    for (k, v) in vars {
        s = s.replace(&format!("{{{k}}}"), v);
    }
    s
}

/// Conventional commit message: `prefix: message` (and `[skip ci]` when the server has no push option for it).
pub fn commit_message(prefix: &str, message: &str, skip_ci_in_message: bool) -> String {
    let prefix = prefix.trim().trim_end_matches(':');
    let message = message.trim();
    let mut s = if prefix.is_empty() { message.to_string() } else { format!("{prefix}: {message}") };
    if skip_ci_in_message {
        s.push_str(" [skip ci]");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, name: &str, text: &str) -> PathBuf {
        let p = dir.join(name);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, text).unwrap();
        p
    }

    #[test]
    fn merges_user_repo_and_local_files() {
        let tmp = tempfile::tempdir().unwrap();
        let user = write(tmp.path(), "user.toml", "[publish]\nreviewers = [\"lead\"]\n[save]\nci_skip = true\n");
        let repo = write(tmp.path(), "repo.toml", "[git]\nbranch_pattern = \"i18n/{user}\"\n[save]\nsemantic_prefix = \"chore\"\n");
        let local = write(tmp.path(), "local.toml", "[save]\nsemantic_prefix = \"\"\n");
        let (c, files) = load(Some(&user), &repo, &local).unwrap().unwrap();
        assert_eq!(c.git.branch_pattern, "i18n/{user}");
        assert_eq!(c.git.remote, "origin", "defaults fill missing keys");
        assert_eq!(c.publish.reviewers, ["lead"]);
        assert!(c.save.ci_skip);
        assert_eq!(c.save.semantic_prefix, "", "working copy overrides the repository");
        assert_eq!(files.all().count(), 3);
        // Without a repository or working copy file the workflow is off, even with user defaults.
        assert!(load(Some(&user), &tmp.path().join("no.toml"), &tmp.path().join("none.toml")).unwrap().is_none());
    }

    #[test]
    fn rejects_secrets_and_invalid_references() {
        let tmp = tempfile::tempdir().unwrap();
        let none = tmp.path().join("none.toml");
        let repo = write(tmp.path(), "a.toml", "[forge]\ntoken = \"glpat-abcdef123456\"\n");
        assert!(matches!(load(None, &repo, &none), Err(Error::Config(m)) if m.contains("forge.token")));
        let repo = write(tmp.path(), "b.toml", "[forge]\ntoken = \"s3cr3t\"\n");
        assert!(load(None, &repo, &none).is_err(), "plain values are not a token reference");
        let repo = write(tmp.path(), "c.toml", "[forge]\nprovider = \"bitbucket\"\n");
        assert!(load(None, &repo, &none).is_err());
        let repo = write(tmp.path(), "d.toml", "[forge]\ntoken = \"env:MY_TOKEN\"\n");
        assert!(load(None, &repo, &none).unwrap().is_some());
    }

    #[test]
    fn template_is_valid_and_matches_defaults() {
        let c: Config = toml::from_str(TEMPLATE).unwrap();
        c.validate().unwrap();
        assert_eq!(c, Config::default());
    }

    #[test]
    fn messages() {
        assert_eq!(commit_message("fix", "Update", false), "fix: Update");
        assert_eq!(commit_message("", "Update", true), "Update [skip ci]");
        assert_eq!(expand("l10n/{user}/{date}", &[("user", "anna"), ("date", "2026-11-05")]), "l10n/anna/2026-11-05");
    }
}
