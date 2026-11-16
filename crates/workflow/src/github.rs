//! GitHub REST API client for pull requests (github.com, GitHub Enterprise Cloud `*.ghe.com` and
//! GitHub Enterprise Server).
//!
//! Pull requests are the workflow's merge requests. Reviewers are user logins or teams written as
//! `org/team`; the author cannot review their own pull request and is left out. Assignees are
//! users only (teams can only review). GitHub has no
//! per-pull-request settings for squashing and deleting the branch (both are repository settings
//! and the choice when merging), so these draft fields are ignored.

use std::sync::OnceLock;

use serde_json::{Value, json};
use susi_core::i18n::t;
use susi_core::tf;

use crate::forge::{Forge, MergeRequest, MergeRequestDraft, MergeRequestQuery, MergeRequestState, Provider, User};
use crate::http::{Http, Response, encode};
use crate::{Error, Result};

/// REST API version requested with every call.
const API_VERSION: &str = "2022-11-28";

pub struct GitHub {
    /// API base, e.g. `https://api.github.com` or `https://github.example.com/api/v3`.
    api: String,
    owner: String,
    repo: String,
    token: Option<String>,
    http: Http,
    /// Login of the token's user (the author of new pull requests).
    login: OnceLock<Option<String>>,
}

/// REST API base of a GitHub web address: `https://api.github.com` for github.com,
/// `https://api.<name>.ghe.com` for GitHub Enterprise Cloud, `<url>/api/v3` for GitHub Enterprise Server.
pub fn api_base(url: &str) -> String {
    let url = url.trim().trim_end_matches('/');
    let (scheme, rest) = url.split_once("://").unwrap_or(("https", url));
    let host = rest.split('/').next().unwrap_or(rest).to_ascii_lowercase();
    if host == "github.com" || host == "www.github.com" || host == "api.github.com" {
        "https://api.github.com".into()
    } else if host.ends_with(".ghe.com") {
        if host.starts_with("api.") { format!("{scheme}://{host}") } else { format!("{scheme}://api.{host}") }
    } else {
        format!("{url}/api/v3")
    }
}

fn user(v: &Value) -> Option<User> {
    Some(User {
        id: v.get("id")?.as_u64()?,
        username: v.get("login")?.as_str()?.to_string(),
        name: v.get("name").and_then(Value::as_str).unwrap_or_default().to_string(),
    })
}

fn is_bot(v: &Value) -> bool {
    v.get("type").and_then(Value::as_str) == Some("Bot") || v.get("login").and_then(Value::as_str).is_some_and(|l| l.ends_with("[bot]"))
}

fn users(v: &Value, key: &str) -> Vec<User> {
    v.get(key).and_then(Value::as_array).map(|a| a.iter().filter_map(user).collect()).unwrap_or_default()
}

fn pull_request(v: &Value) -> Option<MergeRequest> {
    let s = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
    let branch = |k: &str| v.get(k).and_then(|b| b.get("ref")).and_then(Value::as_str).unwrap_or_default().to_string();
    let state = match (v.get("merged_at").is_some_and(|m| !m.is_null()), v.get("state")?.as_str()?) {
        (true, _) => MergeRequestState::Merged,
        (false, "closed") => MergeRequestState::Closed,
        _ => MergeRequestState::Open,
    };
    Some(MergeRequest {
        iid: v.get("number")?.as_u64()?,
        title: s("title"),
        url: s("html_url"),
        state,
        source_branch: branch("head"),
        target_branch: branch("base"),
        author: v.get("user").and_then(user),
        reviewers: users(v, "requested_reviewers"),
        assignees: users(v, "assignees"),
    })
}

fn parsed(v: &Value) -> Result<MergeRequest> {
    pull_request(v).ok_or_else(|| Error::Forge(t("Unexpected answer from the server").to_string()))
}

/// Team slug of a name written `org/team`.
fn team(name: &str) -> Option<&str> {
    name.trim().trim_start_matches('@').rsplit_once('/').map(|(_, team)| team).filter(|team| !team.is_empty())
}

/// Reviewer names split into user logins and team slugs (`org/team` → `team`).
fn split_reviewers(names: &[String]) -> (Vec<String>, Vec<String>) {
    let mut users = Vec::new();
    let mut teams = Vec::new();
    for n in names.iter().map(|n| n.trim().trim_start_matches('@')).filter(|n| !n.is_empty()) {
        match team(n) {
            Some(team) => teams.push(team.to_string()),
            None => users.push(n.to_string()),
        }
    }
    (users, teams)
}

impl GitHub {
    /// `url`: web address (`https://github.com`), `project`: `owner/repo`.
    pub fn new(url: &str, project: &str, token: Option<String>) -> Self {
        let project = project.trim().trim_matches('/');
        let (owner, repo) = project.split_once('/').unwrap_or((project, ""));
        Self { api: api_base(url), owner: owner.to_string(), repo: repo.to_string(), token, http: Http::new(), login: OnceLock::new() }
    }

    fn repo_path(&self) -> String {
        format!("/repos/{}/{}", encode(&self.owner), encode(&self.repo))
    }

    fn raw(&self, method: &str, path: &str, body: Option<&Value>) -> Result<Response> {
        let auth = self.token.as_ref().map(|t| format!("Bearer {t}"));
        let mut headers = vec![("Accept", "application/vnd.github+json"), ("X-GitHub-Api-Version", API_VERSION)];
        if let Some(a) = &auth {
            headers.push(("Authorization", a.as_str()));
        }
        self.http.send(method, &format!("{}{path}", self.api), &headers, body, &self.api)
    }

    fn send(&self, method: &str, path: &str, body: Option<&Value>) -> Result<Value> {
        check(self.raw(method, path, body)?)
    }

    /// All pages of a list (at most 1000 entries).
    fn list(&self, path: &str) -> Result<Vec<Value>> {
        let mut all = Vec::new();
        let sep = if path.contains('?') { '&' } else { '?' };
        for page in 1..=10 {
            let items = self.send("GET", &format!("{path}{sep}per_page=100&page={page}"), None)?;
            let items = items.as_array().cloned().unwrap_or_default();
            let n = items.len();
            all.extend(items);
            if n < 100 {
                break;
            }
        }
        Ok(all)
    }

    fn login(&self) -> Option<&str> {
        self.login.get_or_init(|| self.current_user().ok().map(|u| u.username)).as_deref()
    }

    /// Checks the people before the pull request is created or changed: teams can only review,
    /// users must be collaborators (when the collaborators can be listed).
    fn check_people(&self, d: &MergeRequestDraft) -> Result<()> {
        // With assign_as = "both" a team is in both lists: it reviews and is not assigned.
        if let Some(n) = d.assignees.iter().find(|n| team(n).is_some() && !d.reviewers.contains(n)) {
            return Err(Error::Forge(tf!("GitHub can only assign users, not teams (\"{name}\"); teams can only be requested as reviewers.", name = n.trim())));
        }
        let mut users = split_reviewers(&d.reviewers).0;
        users.extend(split_reviewers(&d.assignees).0);
        if users.is_empty() {
            return Ok(());
        }
        let Ok(members) = self.members() else { return Ok(()) };
        match users.iter().find(|n| !members.iter().any(|m| m.username.eq_ignore_ascii_case(n))) {
            Some(n) if !members.is_empty() => Err(Error::Forge(tf!("Reviewer \"{name}\" not found on the server", name = n))),
            _ => Ok(()),
        }
    }

    /// Requests reviews; returns the updated pull request (`None` without reviewers).
    fn request_reviewers(&self, number: u64, names: &[String]) -> Result<Option<MergeRequest>> {
        let (mut users, teams) = split_reviewers(names);
        if !users.is_empty()
            && let Some(me) = self.login()
        {
            users.retain(|u| !u.eq_ignore_ascii_case(me));
        }
        if users.is_empty() && teams.is_empty() {
            return Ok(None);
        }
        let body = json!({ "reviewers": users, "team_reviewers": teams });
        let v = self.send("POST", &format!("{}/pulls/{number}/requested_reviewers", self.repo_path()), Some(&body))?;
        Ok(pull_request(&v))
    }

    /// Assigns the users (teams only review); returns the assignees (`None` without users).
    fn assign(&self, number: u64, names: &[String]) -> Result<Option<Vec<User>>> {
        let logins = split_reviewers(names).0;
        if logins.is_empty() {
            return Ok(None);
        }
        let body = json!({ "assignees": logins });
        let v = self.send("POST", &format!("{}/issues/{number}/assignees", self.repo_path()), Some(&body))?;
        Ok(Some(users(&v, "assignees")))
    }

    /// Requests the reviews and assigns the users of a pull request that was just created or
    /// updated: a failure is reported together with the pull request, which exists in any case.
    fn with_people(&self, pr: MergeRequest, d: &MergeRequestDraft, created: bool) -> Result<MergeRequest> {
        let reference = Provider::GitHub.reference(pr.iid);
        let mut pr = match self.request_reviewers(pr.iid, &d.reviewers) {
            Ok(updated) => updated.unwrap_or(pr),
            Err(e) => {
                return Err(Error::Forge(if created {
                    tf!("{reference} was created, but the reviewers could not be requested: {error}", reference = reference, error = e)
                } else {
                    tf!("{reference} was updated, but the reviewers could not be requested: {error}", reference = reference, error = e)
                }));
            }
        };
        match self.assign(pr.iid, &d.assignees) {
            Ok(Some(assignees)) => pr.assignees = assignees,
            Ok(None) => {}
            Err(e) => {
                return Err(Error::Forge(if created {
                    tf!("{reference} was created, but the assignees could not be set: {error}", reference = reference, error = e)
                } else {
                    tf!("{reference} was updated, but the assignees could not be set: {error}", reference = reference, error = e)
                }));
            }
        }
        Ok(pr)
    }
}

/// Turns an error status into a message with a hint for the user.
fn check(r: Response) -> Result<Value> {
    if r.status < 400 {
        return Ok(r.body);
    }
    let mut detail = r.body.get("message").and_then(Value::as_str).map_or_else(|| r.text.chars().take(200).collect(), str::to_string);
    // Validation errors (422) carry the details in `errors`.
    let errors: Vec<String> = r
        .body
        .get("errors")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|e| e.get("message").and_then(Value::as_str).or_else(|| e.as_str())).map(str::to_string).collect())
        .unwrap_or_default();
    if !errors.is_empty() {
        detail = format!("{detail}: {}", errors.join("; "));
    }
    let rate_limited = matches!(r.status, 403 | 429) && r.header("x-ratelimit-remaining") == Some("0");
    let hint = match r.status {
        _ if rate_limited => t("The GitHub API rate limit is exceeded; try again later.").to_string(),
        401 => tf!(
            "The access token is missing, invalid or expired. Store a fine-grained token with the repository permissions \"Contents\" and \"Pull requests\" (read and write) or a classic token with the scope \"repo\" (Workflow > Sign in to the server…), or set {env}.",
            env = crate::secret::ENV_TOKEN
        ),
        403 if r.header("x-github-sso").is_some() => {
            t("The access token is not authorized for the organization's single sign-on (SAML SSO): authorize it in the token settings on GitHub.").to_string()
        }
        403 => t("The access token has no permission for this action (repository permissions \"Contents\" and \"Pull requests\": read and write).")
            .to_string(),
        404 => t("Repository not found or not accessible with the access token - check forge.url and forge.project in the workflow configuration and the repositories the token may access.")
            .to_string(),
        _ => String::new(),
    };
    Err(Error::Forge(format!("GitHub {}: {detail} {hint}", r.status).trim().to_string()))
}

impl Forge for GitHub {
    fn name(&self) -> &'static str {
        "GitHub"
    }

    fn current_user(&self) -> Result<User> {
        user(&self.send("GET", "/user", None)?).ok_or_else(|| Error::Forge(t("Unexpected answer from the server").to_string()))
    }

    fn members(&self) -> Result<Vec<User>> {
        // Collaborators need push access to be listed; assignees are visible with read access.
        let collaborators = format!("{}/collaborators", self.repo_path());
        let list = match self.raw("GET", &format!("{collaborators}?per_page=1"), None)?.status {
            403 => self.list(&format!("{}/assignees", self.repo_path()))?,
            _ => self.list(&collaborators)?,
        };
        let mut all: Vec<User> = list.iter().filter(|v| !is_bot(v)).filter_map(user).collect();
        all.sort_by_key(|u| (u.name.to_lowercase(), u.username.to_lowercase()));
        all.dedup_by_key(|u| u.id);
        Ok(all)
    }

    fn merge_requests(&self, q: &MergeRequestQuery) -> Result<Vec<MergeRequest>> {
        let state = if q.only_open { "open" } else { "all" };
        let mut path = format!("{}/pulls?state={state}&sort=updated&direction=desc&per_page=50", self.repo_path());
        if let Some(s) = &q.source_branch {
            path.push_str(&format!("&head={}", encode(&format!("{}:{s}", self.owner))));
        }
        if let Some(b) = &q.target_branch {
            path.push_str(&format!("&base={}", encode(b)));
        }
        let v = self.send("GET", &path, None)?;
        Ok(v.as_array().map(|a| a.iter().filter_map(pull_request).collect()).unwrap_or_default())
    }

    fn create_merge_request(&self, d: &MergeRequestDraft) -> Result<MergeRequest> {
        self.check_people(d)?;
        let body = json!({ "title": d.title, "body": d.description, "head": d.source_branch, "base": d.target_branch });
        let pr = parsed(&self.send("POST", &format!("{}/pulls", self.repo_path()), Some(&body))?)?;
        self.with_people(pr, d, true)
    }

    fn update_merge_request(&self, iid: u64, d: &MergeRequestDraft) -> Result<MergeRequest> {
        self.check_people(d)?;
        let body = json!({ "title": d.title, "body": d.description, "base": d.target_branch });
        let pr = parsed(&self.send("PATCH", &format!("{}/pulls/{iid}", self.repo_path()), Some(&body))?)?;
        self.with_people(pr, d, false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::test_server::serve;

    #[test]
    fn api_addresses() {
        assert_eq!(api_base("https://github.com/"), "https://api.github.com");
        assert_eq!(api_base("https://octocorp.ghe.com"), "https://api.octocorp.ghe.com");
        assert_eq!(api_base("https://github.example.com"), "https://github.example.com/api/v3");
        assert_eq!(api_base("http://127.0.0.1:8080"), "http://127.0.0.1:8080/api/v3");
    }

    const PR: &str = r#"{"number": 5, "title": "T", "html_url": "https://github.com/octo/app/pull/5", "state": "open", "merged_at": null,
        "head": {"ref": "l10n/anna/2026-11-15"}, "base": {"ref": "main"}, "user": {"login": "anna", "id": 1},
        "requested_reviewers": [{"login": "bob", "id": 2}], "assignees": [{"login": "anna", "id": 1}]}"#;

    #[test]
    fn parses_pull_requests() {
        let mut v: Value = serde_json::from_str(PR).unwrap();
        let pr = pull_request(&v).unwrap();
        assert_eq!((pr.iid, pr.state, pr.source_branch.as_str(), pr.target_branch.as_str()), (5, MergeRequestState::Open, "l10n/anna/2026-11-15", "main"));
        assert_eq!((pr.author.unwrap().username, pr.reviewers[0].username.clone()), ("anna".to_string(), "bob".to_string()));
        assert_eq!(pr.assignees[0].username, "anna");
        v["state"] = json!("closed");
        assert_eq!(pull_request(&v).unwrap().state, MergeRequestState::Closed);
        v["merged_at"] = json!("2026-11-16T10:00:00Z");
        assert_eq!(pull_request(&v).unwrap().state, MergeRequestState::Merged);
        assert_eq!(split_reviewers(&["@bob".into(), "octo/translators".into(), " ".into()]), (vec!["bob".into()], vec!["translators".into()]));
    }

    #[test]
    fn creates_pull_requests_and_requests_reviews() {
        let (url, server) = serve(5, |line| {
            if line.starts_with("GET /api/v3/repos/octo/app/collaborators") {
                (
                    200,
                    "",
                    r#"[{"login": "bob", "id": 2, "type": "User"}, {"login": "anna", "id": 1}, {"login": "dependabot[bot]", "id": 9, "type": "Bot"}]"#.into(),
                )
            } else if line.starts_with("GET /api/v3/user ") {
                (200, "", r#"{"login": "anna", "id": 1, "name": "Anna"}"#.into())
            } else {
                (201, "", PR.into())
            }
        });
        let gh = GitHub::new(&url, "octo/app", Some("secret".into()));
        let d = MergeRequestDraft {
            source_branch: "l10n/anna/2026-11-15".into(),
            target_branch: "main".into(),
            title: "T".into(),
            description: "D".into(),
            // The author is left out; teams are written org/team.
            reviewers: vec!["bob".into(), "Anna".into(), "octo/translators".into()],
            assignees: Vec::new(),
            squash: true,
            remove_source_branch: true,
        };
        let pr = gh.create_merge_request(&d).unwrap();
        assert_eq!((pr.iid, pr.reviewers.len()), (5, 1));
        let seen = server.join().unwrap();
        let lines: Vec<String> = seen.iter().map(|r| r.line.split(' ').take(2).collect::<Vec<_>>().join(" ")).collect();
        assert_eq!(
            lines,
            [
                "GET /api/v3/repos/octo/app/collaborators?per_page=1",
                "GET /api/v3/repos/octo/app/collaborators?per_page=100&page=1",
                "POST /api/v3/repos/octo/app/pulls",
                "GET /api/v3/user",
                "POST /api/v3/repos/octo/app/pulls/5/requested_reviewers",
            ]
        );
        let head = seen[2].head.to_lowercase();
        assert!(
            head.contains("authorization: bearer secret")
                && head.contains("accept: application/vnd.github+json")
                && head.contains("x-github-api-version: 2022-11-28"),
            "{head}"
        );
        assert_eq!(seen[2].json(), json!({"title": "T", "body": "D", "head": "l10n/anna/2026-11-15", "base": "main"}));
        assert_eq!(seen[4].json(), json!({"reviewers": ["bob"], "team_reviewers": ["translators"]}));
    }

    #[test]
    fn assigns_users_of_pull_requests() {
        let (url, server) = serve(6, |line| {
            if line.starts_with("GET /api/v3/repos/octo/app/collaborators") {
                (200, "", r#"[{"login": "bob", "id": 2}, {"login": "anna", "id": 1}]"#.into())
            } else if line.starts_with("GET /api/v3/user ") {
                (200, "", r#"{"login": "anna", "id": 1}"#.into())
            } else if line.contains("/issues/5/assignees") {
                (201, "", r#"{"number": 5, "assignees": [{"login": "anna", "id": 1}, {"login": "bob", "id": 2}]}"#.into())
            } else {
                (201, "", PR.into())
            }
        });
        let gh = GitHub::new(&url, "octo/app", Some("secret".into()));
        // assign_as = "both": the author can be assigned, the team only reviews.
        let people: Vec<String> = vec!["bob".into(), "anna".into(), "octo/translators".into()];
        let d = MergeRequestDraft { title: "T".into(), reviewers: people.clone(), assignees: people, ..Default::default() };
        let pr = gh.create_merge_request(&d).unwrap();
        assert_eq!(pr.assignees.iter().map(|u| u.username.as_str()).collect::<Vec<_>>(), ["anna", "bob"]);
        let seen = server.join().unwrap();
        assert!(seen[5].line.starts_with("POST /api/v3/repos/octo/app/issues/5/assignees "), "{}", seen[5].line);
        assert_eq!(seen[4].json(), json!({"reviewers": ["bob"], "team_reviewers": ["translators"]}));
        assert_eq!(seen[5].json(), json!({"assignees": ["bob", "anna"]}));
    }

    #[test]
    fn teams_cannot_be_assigned() {
        let (url, server) = serve(0, |_| (500, "", String::new()));
        let gh = GitHub::new(&url, "octo/app", Some("secret".into()));
        let d = MergeRequestDraft { assignees: vec!["octo/translators".into()], ..Default::default() };
        let err = gh.create_merge_request(&d).unwrap_err().to_string();
        assert!(err.contains("\"octo/translators\""), "{err}");
        assert!(server.join().unwrap().is_empty(), "no pull request was created");
    }

    #[test]
    fn unknown_reviewers_are_rejected_before_creating() {
        let (url, server) = serve(2, |_| (200, "", r#"[{"login": "bob", "id": 2}]"#.into()));
        let gh = GitHub::new(&url, "octo/app", Some("secret".into()));
        let d = MergeRequestDraft { reviewers: vec!["carol".into()], ..Default::default() };
        let err = gh.create_merge_request(&d).unwrap_err().to_string();
        assert!(err.contains("\"carol\""), "{err}");
        assert_eq!(server.join().unwrap().len(), 2, "no pull request was created");
    }

    #[test]
    fn lists_pull_requests_of_a_branch_and_updates_them() {
        let (url, server) = serve(2, |line| {
            if line.starts_with("GET ") {
                (200, "", format!("[{}]", PR.replace("\"merged_at\": null", "\"merged_at\": \"2026-11-16T10:00:00Z\"").replace("\"open\"", "\"closed\"")))
            } else {
                (200, "", PR.into())
            }
        });
        let gh = GitHub::new(&url, "octo/app", None);
        let q = MergeRequestQuery { source_branch: Some("l10n/anna/2026-11-15".into()), target_branch: Some("main".into()), only_open: false };
        let list = gh.merge_requests(&q).unwrap();
        assert_eq!(list[0].state, MergeRequestState::Merged);
        let pr = gh.update_merge_request(5, &MergeRequestDraft { title: "New".into(), target_branch: "main".into(), ..Default::default() }).unwrap();
        assert_eq!(pr.iid, 5);
        let seen = server.join().unwrap();
        assert!(
            seen[0].line.contains("/pulls?state=all&sort=updated&direction=desc&per_page=50&head=octo%3Al10n%2Fanna%2F2026-11-15&base=main"),
            "{}",
            seen[0].line
        );
        assert!(!seen[0].head.to_lowercase().contains("authorization"), "no token, no header");
        assert!(seen[1].line.starts_with("PATCH /api/v3/repos/octo/app/pulls/5 "), "{}", seen[1].line);
        // Without reviewers nothing more is requested.
        assert_eq!(seen[1].json(), json!({"title": "New", "body": "", "base": "main"}));
    }

    #[test]
    fn members_fall_back_to_assignees_without_push_access() {
        let (url, server) = serve(2, |line| {
            if line.contains("/collaborators") {
                (403, "", r#"{"message": "Must have push access to view repository collaborators."}"#.into())
            } else {
                (200, "", r#"[{"login": "zoe", "id": 3}, {"login": "bob", "id": 2}]"#.into())
            }
        });
        let users = GitHub::new(&url, "octo/app", Some("t".into())).members().unwrap();
        assert_eq!(users.iter().map(|u| u.username.as_str()).collect::<Vec<_>>(), ["bob", "zoe"]);
        assert!(server.join().unwrap()[1].line.starts_with("GET /api/v3/repos/octo/app/assignees?per_page=100&page=1 "));
    }

    #[test]
    fn explains_errors() {
        let (url, server) = serve(3, |line| {
            if line.starts_with("GET /api/v3/user") {
                (401, "", r#"{"message": "Bad credentials"}"#.into())
            } else if line.starts_with("GET") {
                (403, "X-RateLimit-Remaining: 0\r\n", r#"{"message": "API rate limit exceeded"}"#.into())
            } else {
                (422, "", r#"{"message": "Validation Failed", "errors": [{"resource": "PullRequest", "code": "custom", "message": "No commits between main and l10n/a"}]}"#.into())
            }
        });
        let gh = GitHub::new(&url, "octo/app", None);
        let e = gh.current_user().unwrap_err().to_string();
        assert!(e.contains("Bad credentials") && e.contains(crate::secret::ENV_TOKEN), "{e}");
        let e = gh.merge_requests(&MergeRequestQuery::default()).unwrap_err().to_string();
        assert!(e.contains("rate limit is exceeded"), "{e}");
        let e = gh.create_merge_request(&MergeRequestDraft::default()).unwrap_err().to_string();
        assert!(e.contains("GitHub 422: Validation Failed: No commits between main and l10n/a"), "{e}");
        server.join().unwrap();
    }
}
