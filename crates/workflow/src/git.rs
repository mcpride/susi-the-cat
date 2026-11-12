//! Thin wrapper around the `git` command line tool.
//!
//! Commands never prompt (`GIT_TERMINAL_PROMPT=0`, `GCM_INTERACTIVE=never`)
//! and are recorded (without input data such as credentials) so the user interface can show them.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;

use crate::{Error, Result};

pub struct Git {
    dir: PathBuf,
    log: Mutex<Vec<String>>,
}

/// One changed path of `git status --porcelain`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    /// Two-letter status code (`XY`), e.g. ` M`, `??`, `D `.
    pub code: String,
    /// Path relative to the working copy root (with `/`).
    pub path: String,
}

impl Git {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into(), log: Mutex::new(Vec::new()) }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut c = Command::new("git");
        c.arg("-C").arg(&self.dir).args(["-c", "core.quotepath=false"]).args(args);
        c.env("GIT_TERMINAL_PROMPT", "0").env("GCM_INTERACTIVE", "never");
        c.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // CREATE_NO_WINDOW: no console window flashing up when called from the GUI.
            c.creation_flags(0x0800_0000);
        }
        c
    }

    fn exec(&self, args: &[&str], input: Option<&[u8]>, record: bool) -> Result<String> {
        let line = args.join(" ");
        if record && let Ok(mut l) = self.log.lock() {
            l.push(format!("git {line}"));
        }
        let mut cmd = self.command(args);
        if input.is_some() {
            cmd.stdin(Stdio::piped());
        }
        let mut child = cmd.spawn().map_err(|e| Error::Git { command: line.clone(), message: e.to_string() })?;
        if let (Some(data), Some(mut stdin)) = (input, child.stdin.take()) {
            stdin.write_all(data).map_err(|e| Error::Git { command: line.clone(), message: e.to_string() })?;
        }
        let out = child.wait_with_output().map_err(|e| Error::Git { command: line.clone(), message: e.to_string() })?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
            let message = if stderr.is_empty() { String::from_utf8_lossy(&out.stdout).trim().to_string() } else { stderr };
            return Err(Error::Git { command: line, message });
        }
        Ok(String::from_utf8_lossy(&out.stdout).trim_end().to_string())
    }

    /// Runs a command that changes something and records it.
    pub fn run(&self, args: &[&str]) -> Result<String> {
        self.exec(args, None, true)
    }

    /// Runs a recorded command with data on stdin (e.g. `--pathspec-from-file=-`).
    pub fn run_input(&self, args: &[&str], input: &[u8]) -> Result<String> {
        self.exec(args, Some(input), true)
    }

    /// Runs a read-only query (not recorded).
    pub fn query(&self, args: &[&str]) -> Result<String> {
        self.exec(args, None, false)
    }

    /// Read-only query with data on stdin, not recorded (credentials).
    pub fn query_input(&self, args: &[&str], input: &[u8]) -> Result<String> {
        self.exec(args, Some(input), false)
    }

    /// Query whose failure just means "no value".
    pub fn try_query(&self, args: &[&str]) -> Option<String> {
        self.query(args).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
    }

    /// Recorded commands since the last call.
    pub fn take_log(&self) -> Vec<String> {
        self.log.lock().map(|mut l| std::mem::take(&mut *l)).unwrap_or_default()
    }

    pub fn ref_exists(&self, name: &str) -> bool {
        self.query(&["rev-parse", "--verify", "--quiet", name]).is_ok()
    }

    /// Current branch, `None` when detached.
    pub fn current_branch(&self) -> Option<String> {
        self.try_query(&["symbolic-ref", "--short", "-q", "HEAD"])
    }

    pub fn count(&self, range: &str) -> usize {
        self.try_query(&["rev-list", "--count", range]).and_then(|s| s.parse().ok()).unwrap_or(0)
    }

    /// Branch, upstream and changes (staged, unstaged, untracked) with a single git call.
    pub fn snapshot(&self) -> Result<Snapshot> {
        let out = self.query(&["status", "--porcelain=v2", "--branch", "-z", "--untracked-files=all"])?;
        Ok(parse_snapshot(&out))
    }

    /// Whether git knows the user's name and e-mail address (needed for commits).
    pub fn has_identity(&self) -> bool {
        let out = self.try_query(&["config", "--get-regexp", "^user\\.(name|email)$"]).unwrap_or_default();
        out.lines().any(|l| l.starts_with("user.name ")) && out.lines().any(|l| l.starts_with("user.email "))
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Snapshot {
    /// `None` when detached.
    pub branch: Option<String>,
    pub upstream: Option<String>,
    /// Commits ahead of/behind the upstream; `None` without upstream or when it no longer exists.
    pub ahead_behind: Option<(usize, usize)>,
    pub changes: Vec<Change>,
}

impl Snapshot {
    /// The upstream branch was removed on the server.
    pub fn upstream_gone(&self) -> bool {
        self.upstream.is_some() && self.ahead_behind.is_none()
    }
}

/// Parses `git status --porcelain=v2 --branch -z`; renames yield the new and the old path.
pub fn parse_snapshot(out: &str) -> Snapshot {
    let mut s = Snapshot::default();
    let mut it = out.split('\0').filter(|e| !e.is_empty());
    while let Some(e) = it.next() {
        let field = |n: usize| e.splitn(n + 1, ' ').nth(n).map(str::to_string);
        let code = e.get(2..4).unwrap_or_default().to_string();
        if let Some(h) = e.strip_prefix("# branch.head ") {
            s.branch = (h != "(detached)").then(|| h.to_string());
        } else if let Some(u) = e.strip_prefix("# branch.upstream ") {
            s.upstream = Some(u.to_string());
        } else if let Some(ab) = e.strip_prefix("# branch.ab ") {
            let n = |p: &str| ab.split(' ').find_map(|x| x.strip_prefix(p)).and_then(|x| x.parse().ok()).unwrap_or(0);
            s.ahead_behind = Some((n("+"), n("-")));
        } else if let Some(p) = e.strip_prefix("? ") {
            s.changes.push(Change { code: "??".into(), path: p.to_string() });
        } else if e.starts_with("1 ") {
            s.changes.extend(field(8).map(|path| Change { code, path }));
        } else if e.starts_with("2 ") {
            s.changes.extend(field(9).map(|path| Change { code: code.clone(), path }));
            s.changes.extend(it.next().map(|old| Change { code, path: old.to_string() }));
        } else if e.starts_with("u ") {
            s.changes.extend(field(10).map(|path| Change { code, path }));
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_porcelain_status() {
        let out = "# branch.oid 1234\0# branch.head l10n/anna\0# branch.upstream origin/l10n/anna\0# branch.ab +2 -1\0\
            1 .M N... 100644 100644 100644 aaa bbb l10n/my app.slp\0? res/de/a.resx\0\
            2 R. N... 100644 100644 100644 aaa bbb R100 new.json\0old.json\0";
        let s = parse_snapshot(out);
        let paths: Vec<&str> = s.changes.iter().map(|c| c.path.as_str()).collect();
        assert_eq!(paths, ["l10n/my app.slp", "res/de/a.resx", "new.json", "old.json"]);
        assert_eq!((s.changes[0].code.as_str(), s.changes[1].code.as_str()), (".M", "??"));
        assert_eq!((s.branch.as_deref(), s.ahead_behind), (Some("l10n/anna"), Some((2, 1))));
        assert!(!s.upstream_gone());
        let gone = parse_snapshot("# branch.head (detached)\0# branch.upstream origin/x\0");
        assert!(gone.branch.is_none() && gone.upstream_gone());
    }
}
