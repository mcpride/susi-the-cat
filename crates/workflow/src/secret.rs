//! Access tokens are never stored in configuration files, only referenced:
//!
//! * `auto` (default): environment variable `SUSI_FORGE_TOKEN`, then the token stored for the server
//!   with git's credential helper (user `susi-token`),
//! * `env:NAME`: environment variable `NAME`,
//! * `credential`: git's credential helper, the credentials used for git itself,
//! * `credential:<user>`: git's credential helper, the entry of `<user>`,
//! * `none`: no token.
//!
//! On Windows git's credential helper (Git Credential Manager) keeps tokens in the Windows Credential Manager.

use susi_core::tf;

use crate::git::Git;
use crate::{Error, Result};

/// Environment variable checked by `auto`.
pub const ENV_TOKEN: &str = "SUSI_FORGE_TOKEN";
/// Credential helper user name under which Susi stores the API token.
pub const CREDENTIAL_USER: &str = "susi-token";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenRef {
    Auto,
    Env(String),
    Credential(Option<String>),
    None,
}

impl TokenRef {
    pub fn parse(s: &str) -> Result<TokenRef> {
        let s = s.trim();
        let (kind, arg) = match s.split_once(':') {
            Some((k, a)) => (k.trim().to_ascii_lowercase(), Some(a.trim())),
            None => (s.to_ascii_lowercase(), None),
        };
        match (kind.as_str(), arg) {
            ("auto" | "", None) => Ok(TokenRef::Auto),
            ("none", None) => Ok(TokenRef::None),
            ("env", Some(v)) if !v.is_empty() => Ok(TokenRef::Env(v.to_string())),
            ("credential" | "git-credential", None) => Ok(TokenRef::Credential(None)),
            ("credential" | "git-credential", Some(u)) if !u.is_empty() => Ok(TokenRef::Credential(Some(u.to_string()))),
            _ => Err(Error::Config(tf!(
                "Invalid token reference \"{value}\" (allowed: auto, env:NAME, credential, credential:USER, none). Never put the token itself into a configuration file.",
                value = if s.len() > 12 { format!("{}…", &s[..s.char_indices().nth(6).map(|(i, _)| i).unwrap_or(s.len())]) } else { s.to_string() }
            ))),
        }
    }
}

/// `protocol=…`/`host=…` lines of a server URL for `git credential`.
fn credential_request(url: &str, user: Option<&str>) -> String {
    let (protocol, rest) = url.split_once("://").unwrap_or(("https", url));
    let host = rest.split('/').next().unwrap_or(rest);
    let mut s = format!("protocol={protocol}\nhost={host}\n");
    if let Some(u) = user {
        s.push_str(&format!("username={u}\n"));
    }
    s.push('\n');
    s
}

fn credential_fill(git: &Git, url: &str, user: Option<&str>) -> Option<String> {
    let out = git.query_input(&["credential", "fill"], credential_request(url, user).as_bytes()).ok()?;
    out.lines().find_map(|l| l.strip_prefix("password=")).map(str::to_string).filter(|p| !p.is_empty())
}

/// Resolves a token reference for the server at `url`.
pub fn resolve(r: &TokenRef, git: &Git, url: &str) -> Option<String> {
    let env = |name: &str| std::env::var(name).ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
    match r {
        TokenRef::Auto => env(ENV_TOKEN).or_else(|| credential_fill(git, url, Some(CREDENTIAL_USER))),
        TokenRef::Env(name) => env(name),
        TokenRef::Credential(user) => credential_fill(git, url, user.as_deref()),
        TokenRef::None => None,
    }
}

/// Stores the token with git's credential helper (user [`CREDENTIAL_USER`]) and checks it can be read back.
pub fn store(git: &Git, url: &str, token: &str) -> Result<()> {
    let token = token.trim();
    if token.is_empty() || token.contains(['\n', '\r']) {
        return Err(Error::Config(susi_core::i18n::t("Invalid access token").to_string()));
    }
    let mut req = credential_request(url, Some(CREDENTIAL_USER));
    req.pop();
    req.push_str(&format!("password={token}\n\n"));
    git.query_input(&["credential", "approve"], req.as_bytes())?;
    if credential_fill(git, url, Some(CREDENTIAL_USER)).as_deref() != Some(token) {
        return Err(Error::Config(tf!(
            "The token could not be stored: git has no credential helper (git config --global credential.helper manager) - alternatively set the environment variable {env}.",
            env = ENV_TOKEN
        )));
    }
    Ok(())
}

/// Removes the stored token.
pub fn forget(git: &Git, url: &str) -> Result<()> {
    git.query_input(&["credential", "reject"], credential_request(url, Some(CREDENTIAL_USER)).as_bytes()).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_references() {
        assert_eq!(TokenRef::parse("auto").unwrap(), TokenRef::Auto);
        assert_eq!(TokenRef::parse("env:GL_TOKEN").unwrap(), TokenRef::Env("GL_TOKEN".into()));
        assert_eq!(TokenRef::parse("credential").unwrap(), TokenRef::Credential(None));
        assert_eq!(TokenRef::parse("credential:bot").unwrap(), TokenRef::Credential(Some("bot".into())));
        assert_eq!(TokenRef::parse("NONE").unwrap(), TokenRef::None);
        let err = TokenRef::parse("glpat-0123456789abcdef").unwrap_err().to_string();
        assert!(!err.contains("0123456789abcdef"), "the secret is not echoed: {err}");
        assert!(TokenRef::parse("env:").is_err());
    }

    #[test]
    fn credential_requests() {
        assert_eq!(credential_request("https://gitlab.example.com:8443/x", Some("u")), "protocol=https\nhost=gitlab.example.com:8443\nusername=u\n\n");
        assert_eq!(credential_request("http://h", None), "protocol=http\nhost=h\n\n");
    }
}
