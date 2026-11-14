//! Server ("forge") abstraction: merge requests and users. GitLab is implemented in [`crate::gitlab`];
//! GitHub can be added as a further implementation of [`Forge`].

use serde::{Deserialize, Serialize};

use crate::Result;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct User {
    pub id: u64,
    pub username: String,
    pub name: String,
}

impl User {
    /// `Full Name (@username)`.
    pub fn label(&self) -> String {
        if self.name.is_empty() { format!("@{}", self.username) } else { format!("{} (@{})", self.name, self.username) }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MergeRequestState {
    Open,
    Merged,
    Closed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MergeRequest {
    /// Project-local number (`!12`).
    pub iid: u64,
    pub title: String,
    pub url: String,
    pub state: MergeRequestState,
    pub source_branch: String,
    pub target_branch: String,
    pub author: Option<User>,
    pub reviewers: Vec<User>,
}

/// Content of a merge request to create or update.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MergeRequestDraft {
    pub source_branch: String,
    pub target_branch: String,
    pub title: String,
    pub description: String,
    /// Usernames; empty on update keeps the current reviewers.
    pub reviewers: Vec<String>,
    pub squash: bool,
    pub remove_source_branch: bool,
}

/// Filter for [`Forge::merge_requests`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MergeRequestQuery {
    pub source_branch: Option<String>,
    pub target_branch: Option<String>,
    pub only_open: bool,
}

pub trait Forge: Send + Sync {
    /// Short name for messages ("GitLab").
    fn name(&self) -> &'static str;
    /// User owning the access token.
    fn current_user(&self) -> Result<User>;
    /// Project members (candidates for reviewers).
    fn members(&self) -> Result<Vec<User>>;
    /// Merge requests, most recently updated first.
    fn merge_requests(&self, query: &MergeRequestQuery) -> Result<Vec<MergeRequest>>;
    fn create_merge_request(&self, draft: &MergeRequestDraft) -> Result<MergeRequest>;
    fn update_merge_request(&self, iid: u64, draft: &MergeRequestDraft) -> Result<MergeRequest>;
}

/// Web address and project path from a git remote URL (`https://host/group/app.git`,
/// `git@host:group/app.git`, `ssh://git@host:2222/group/app.git`). SSH remotes map to `https://host`.
pub fn parse_remote(url: &str) -> Option<(String, String)> {
    let url = url.trim();
    let (base, path) = if let Some((scheme, rest)) = url.split_once("://") {
        let rest = rest.rsplit_once('@').map_or(rest, |(_, r)| if r.contains('/') { r } else { rest });
        let (host, path) = rest.split_once('/')?;
        match scheme.to_ascii_lowercase().as_str() {
            "http" | "https" => (format!("{}://{host}", scheme.to_ascii_lowercase()), path),
            "ssh" | "git" => (format!("https://{}", host.split(':').next().unwrap_or(host)), path),
            _ => return None,
        }
    } else {
        // scp-like syntax: [user@]host:path
        let (host, path) = url.split_once(':')?;
        if host.len() == 1 || host.contains(['/', '\\']) {
            return None; // Windows drive letter or local path
        }
        let host = host.rsplit_once('@').map_or(host, |(_, h)| h);
        (format!("https://{host}"), path)
    };
    let path = path.trim_matches('/').trim_end_matches(".git").trim_matches('/');
    (!path.is_empty()).then(|| (base, path.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_remote_urls() {
        let want = Some(("https://gitlab.example.com".to_string(), "group/sub/app".to_string()));
        assert_eq!(parse_remote("https://gitlab.example.com/group/sub/app.git"), want);
        assert_eq!(parse_remote("https://user:pw@gitlab.example.com/group/sub/app"), want);
        assert_eq!(parse_remote("git@gitlab.example.com:group/sub/app.git"), want);
        assert_eq!(parse_remote("ssh://git@gitlab.example.com:2222/group/sub/app.git"), want);
        assert_eq!(parse_remote("http://localhost:8080/a/b.git"), Some(("http://localhost:8080".into(), "a/b".into())));
        assert_eq!(parse_remote("C:/repos/app.git"), None);
        assert_eq!(parse_remote("/srv/git/app.git"), None);
        assert_eq!(parse_remote("file:///srv/git/app.git"), None);
    }
}
