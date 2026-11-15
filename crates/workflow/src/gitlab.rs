//! GitLab REST API (v4) client for merge requests.

use serde_json::{Value, json};
use susi_core::tf;

use crate::forge::{Forge, MergeRequest, MergeRequestDraft, MergeRequestQuery, MergeRequestState, User};
use crate::http::{Http, encode};
use crate::{Error, Result};

pub struct GitLab {
    /// API base, e.g. `https://gitlab.example.com/api/v4`.
    api: String,
    /// URL-encoded project path (`group%2Fapp`).
    project: String,
    token: Option<String>,
    http: Http,
}

fn user(v: &Value) -> Option<User> {
    Some(User {
        id: v.get("id")?.as_u64()?,
        username: v.get("username")?.as_str()?.to_string(),
        name: v.get("name").and_then(Value::as_str).unwrap_or_default().to_string(),
    })
}

fn merge_request(v: &Value) -> Option<MergeRequest> {
    let s = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
    let state = match v.get("state")?.as_str()? {
        "merged" => MergeRequestState::Merged,
        "closed" => MergeRequestState::Closed,
        _ => MergeRequestState::Open,
    };
    Some(MergeRequest {
        iid: v.get("iid")?.as_u64()?,
        title: s("title"),
        url: s("web_url"),
        state,
        source_branch: s("source_branch"),
        target_branch: s("target_branch"),
        author: v.get("author").and_then(user),
        reviewers: v.get("reviewers").and_then(Value::as_array).map(|a| a.iter().filter_map(user).collect()).unwrap_or_default(),
    })
}

impl GitLab {
    /// `url`: web address of the server (`https://gitlab.example.com`), `project`: `group/app`.
    pub fn new(url: &str, project: &str, token: Option<String>) -> Self {
        Self { api: format!("{}/api/v4", url.trim_end_matches('/')), project: encode(project.trim_matches('/')), token, http: Http::new() }
    }

    fn check(&self, status: u16, v: Value, body: &str) -> Result<Value> {
        if status < 400 {
            return Ok(v);
        }
        let detail = match v.get("message").or_else(|| v.get("error")) {
            Some(Value::String(s)) => s.clone(),
            Some(other) => other.to_string(),
            None => body.chars().take(200).collect(),
        };
        let hint = match status {
            401 => tf!(
                "The access token is missing, invalid or expired. Store a token with the scope \"api\" (Workflow > Sign in to the server…) or set {env}.",
                env = crate::secret::ENV_TOKEN
            ),
            403 => susi_core::i18n::t("The access token has no permission for this action (scope \"api\" and at least the role Developer are required).")
                .to_string(),
            404 => susi_core::i18n::t("Project not found - check forge.url and forge.project in the workflow configuration.").to_string(),
            _ => String::new(),
        };
        Err(Error::Forge(format!("GitLab {status}: {detail} {hint}").trim().to_string()))
    }

    fn send(&self, method: &str, path: &str, body: Option<&Value>) -> Result<Value> {
        let auth = self.token.as_ref().map(|t| format!("Bearer {t}"));
        let headers: Vec<(&str, &str)> = auth.iter().map(|a| ("Authorization", a.as_str())).collect();
        let r = self.http.send(method, &format!("{}{path}", self.api), &headers, body, &self.api)?;
        self.check(r.status, r.body, &r.text)
    }

    fn get(&self, path: &str) -> Result<Value> {
        self.send("GET", path, None)
    }

    fn reviewer_ids(&self, names: &[String]) -> Result<Vec<u64>> {
        if names.is_empty() {
            return Ok(Vec::new());
        }
        let members = self.members().unwrap_or_default();
        let mut ids = Vec::new();
        for n in names {
            let n = n.trim().trim_start_matches('@');
            if let Some(u) = members.iter().find(|u| u.username.eq_ignore_ascii_case(n)) {
                ids.push(u.id);
                continue;
            }
            let found = self.get(&format!("/users?username={}", encode(n)))?;
            match found.as_array().and_then(|a| a.first()).and_then(user) {
                Some(u) => ids.push(u.id),
                None => return Err(Error::Forge(tf!("Reviewer \"{name}\" not found on the server", name = n))),
            }
        }
        Ok(ids)
    }

    fn body(&self, d: &MergeRequestDraft, create: bool) -> Result<Value> {
        let mut b = json!({
            "title": d.title,
            "description": d.description,
            "target_branch": d.target_branch,
            "squash": d.squash,
            "remove_source_branch": d.remove_source_branch,
        });
        if create {
            b["source_branch"] = json!(d.source_branch);
        }
        if create || !d.reviewers.is_empty() {
            b["reviewer_ids"] = json!(self.reviewer_ids(&d.reviewers)?);
        }
        Ok(b)
    }

    fn parsed_merge_request(v: &Value) -> Result<MergeRequest> {
        merge_request(v).ok_or_else(|| Error::Forge(susi_core::i18n::t("Unexpected answer from the server").to_string()))
    }
}

impl Forge for GitLab {
    fn name(&self) -> &'static str {
        "GitLab"
    }

    fn current_user(&self) -> Result<User> {
        user(&self.get("/user")?).ok_or_else(|| Error::Forge(susi_core::i18n::t("Unexpected answer from the server").to_string()))
    }

    fn members(&self) -> Result<Vec<User>> {
        let mut all = Vec::new();
        for page in 1..=10 {
            let v = self.get(&format!("/projects/{}/members/all?per_page=100&page={page}", self.project))?;
            let users: Vec<User> = v.as_array().map(|a| a.iter().filter_map(user).collect()).unwrap_or_default();
            let n = users.len();
            all.extend(users.into_iter().filter(|u| !u.username.contains("_bot_")));
            if n < 100 {
                break;
            }
        }
        all.sort_by_key(|u| u.name.to_lowercase());
        all.dedup_by_key(|u| u.id);
        Ok(all)
    }

    fn merge_requests(&self, q: &MergeRequestQuery) -> Result<Vec<MergeRequest>> {
        let mut path = format!("/projects/{}/merge_requests?order_by=updated_at&sort=desc&per_page=50", self.project);
        if let Some(s) = &q.source_branch {
            path.push_str(&format!("&source_branch={}", encode(s)));
        }
        if let Some(t) = &q.target_branch {
            path.push_str(&format!("&target_branch={}", encode(t)));
        }
        if q.only_open {
            path.push_str("&state=opened");
        }
        let v = self.get(&path)?;
        Ok(v.as_array().map(|a| a.iter().filter_map(merge_request).collect()).unwrap_or_default())
    }

    fn create_merge_request(&self, d: &MergeRequestDraft) -> Result<MergeRequest> {
        let body = self.body(d, true)?;
        Self::parsed_merge_request(&self.send("POST", &format!("/projects/{}/merge_requests", self.project), Some(&body))?)
    }

    fn update_merge_request(&self, iid: u64, d: &MergeRequestDraft) -> Result<MergeRequest> {
        let body = self.body(d, false)?;
        Self::parsed_merge_request(&self.send("PUT", &format!("/projects/{}/merge_requests/{iid}", self.project), Some(&body))?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::test_server::serve;

    #[test]
    fn encodes_project_paths() {
        assert_eq!(encode("group/sub app"), "group%2Fsub%20app");
    }

    #[test]
    fn parses_merge_requests() {
        let v = json!({"iid": 7, "title": "T", "web_url": "https://g/x/-/merge_requests/7", "state": "merged",
            "source_branch": "l10n/a", "target_branch": "main", "author": {"id": 1, "username": "anna", "name": "Anna"},
            "reviewers": [{"id": 2, "username": "bob", "name": "Bob"}]});
        let mr = merge_request(&v).unwrap();
        assert_eq!((mr.iid, mr.state, mr.reviewers[0].username.as_str()), (7, MergeRequestState::Merged, "bob"));
    }

    #[test]
    fn creates_merge_requests_with_reviewers() {
        let (url, server) = serve(2, |line| {
            if line.starts_with("GET /api/v4/projects/group%2Fapp/members/all") {
                (200, "", r#"[{"id": 2, "username": "bob", "name": "Bob"}, {"id": 9, "username": "project_1_bot_x", "name": "Bot"}]"#.into())
            } else {
                (201, "", r#"{"iid": 3, "title": "T", "web_url": "u", "state": "opened", "source_branch": "l10n/a", "target_branch": "main"}"#.into())
            }
        });
        let gl = GitLab::new(&url, "group/app", Some("secret".into()));
        let d = MergeRequestDraft {
            source_branch: "l10n/a".into(),
            target_branch: "main".into(),
            title: "T".into(),
            reviewers: vec!["bob".into()],
            squash: true,
            ..Default::default()
        };
        let mr = gl.create_merge_request(&d).unwrap();
        assert_eq!((mr.iid, mr.state), (3, MergeRequestState::Open));
        let seen = server.join().unwrap();
        assert!(seen[1].line.starts_with("POST /api/v4/projects/group%2Fapp/merge_requests "), "{}", seen[1].line);
        assert!(seen[1].head.to_lowercase().contains("authorization: bearer secret"));
        let body = seen[1].json();
        assert_eq!(body["reviewer_ids"], json!([2]));
        assert_eq!(body["source_branch"], json!("l10n/a"));
    }

    #[test]
    fn reports_authentication_errors() {
        let (url, server) = serve(1, |_| (401, "", r#"{"message": "401 Unauthorized"}"#.into()));
        let err = GitLab::new(&url, "group/app", None).current_user().unwrap_err().to_string();
        assert!(err.contains("401 Unauthorized") && err.contains(crate::secret::ENV_TOKEN), "{err}");
        server.join().unwrap();
    }
}
