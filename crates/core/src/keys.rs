//! Resolves rows by key: the context (node names and row id joined by `/`, as in exchange files)
//! or, as a fallback, the plain row id, optionally restricted to one source. Used by screenshot
//! links and the MCP server.

use std::collections::HashMap;

use crate::model::{Project, RowRef};

/// How a key matched its rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyMatch {
    Context,
    Id,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyError {
    NotFound,
    /// Several rows match (their number); a source would disambiguate.
    Ambiguous(usize),
}

/// Lookup tables of a project state; rebuild after scans or structural changes.
#[derive(Debug, Clone, Default)]
pub struct RowKeys {
    by_context: HashMap<String, Vec<RowRef>>,
    by_id: HashMap<String, Vec<RowRef>>,
}

impl RowKeys {
    pub fn new(p: &Project) -> Self {
        let mut keys = Self::default();
        for r in p.rows() {
            if let Some(row) = p.row(&r) {
                keys.by_id.entry(row.id().to_string()).or_default().push(r.clone());
            }
            keys.by_context.entry(p.context(&r)).or_default().push(r);
        }
        keys
    }

    /// Rows with context `key`, else rows with id `key`; then restricted to `source` (index).
    pub fn candidates(&self, key: &str, source: Option<usize>) -> (Vec<RowRef>, KeyMatch) {
        let (rows, how) = match self.by_context.get(key) {
            Some(v) => (v.clone(), KeyMatch::Context),
            None => (self.by_id.get(key).cloned().unwrap_or_default(), KeyMatch::Id),
        };
        (rows.into_iter().filter(|r| source.is_none_or(|s| r.source == s)).collect(), how)
    }

    /// Rows selected by `pattern`: with wildcards (`*`, `?`) all rows whose context matches,
    /// otherwise like [`Self::candidates`], where a row id matching several rows selects nothing.
    pub fn select(&self, pattern: &str, source: Option<usize>) -> Vec<RowRef> {
        if pattern.contains(['*', '?']) {
            let mut rows: Vec<RowRef> = self
                .by_context
                .iter()
                .filter(|(c, _)| wildcard(pattern, c))
                .flat_map(|(_, v)| v.iter().cloned())
                .filter(|r| source.is_none_or(|s| r.source == s))
                .collect();
            rows.sort();
            return rows;
        }
        let (rows, how) = self.candidates(pattern, source);
        if how == KeyMatch::Id && rows.len() > 1 { Vec::new() } else { rows }
    }

    /// The single row with this key.
    pub fn find(&self, key: &str, source: Option<usize>) -> Result<RowRef, KeyError> {
        let (mut rows, _) = self.candidates(key, source);
        match rows.len() {
            0 => Err(KeyError::NotFound),
            1 => Ok(rows.remove(0)),
            n => Err(KeyError::Ambiguous(n)),
        }
    }
}

/// Whether `text` matches `pattern`: `*` matches any characters (including `/`), `?` one.
pub fn wildcard(pattern: &str, text: &str) -> bool {
    let (p, t): (Vec<char>, Vec<char>) = (pattern.chars().collect(), text.chars().collect());
    let (mut pi, mut ti) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some((pi, ti));
            pi += 1;
        } else if let Some((sp, st)) = star {
            pi = sp + 1;
            ti = st + 1;
            star = Some((sp, st + 1));
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|&c| c == '*')
}

/// Index of the source called `name`; case and path separators (`/`, `\`) are ignored.
pub fn source_index(p: &Project, name: &str) -> Option<usize> {
    let norm = |s: &str| s.replace('/', "\\").to_lowercase();
    let name = norm(name);
    p.sources.iter().position(|s| norm(s.name()) == name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Item, Node, Row, Source, SourceKind};

    fn project() -> Project {
        let mut p = Project::new();
        for name in ["src\\a.json", "src\\b.json"] {
            let mut s = Source::new(SourceKind::Json, name, "en");
            let mut n = Node::new("TJsonNode", "LOGIN");
            n.items.push(Item::Row(Row::new("TITLE", "Login")));
            s.items.push(Item::Node(n));
            s.items.push(Item::Row(Row::new(if name.contains('a') { "ok" } else { "cancel" }, "x")));
            p.sources.push(s);
        }
        p
    }

    #[test]
    fn context_then_id() {
        let p = project();
        let k = RowKeys::new(&p);
        assert_eq!(k.find("LOGIN/TITLE", None), Err(KeyError::Ambiguous(2)));
        assert_eq!(k.find("LOGIN/TITLE", Some(1)).map(|r| r.source), Ok(1));
        assert_eq!(k.candidates("TITLE", None).1, KeyMatch::Id);
        assert_eq!(k.find("ok", None).map(|r| r.source), Ok(0));
        assert_eq!(k.find("nope", None), Err(KeyError::NotFound));
        assert_eq!(source_index(&p, "SRC/b.json"), Some(1));
    }

    #[test]
    fn wildcards() {
        let p = project();
        let k = RowKeys::new(&p);
        assert_eq!(k.select("LOGIN/*", None).len(), 2);
        assert_eq!(k.select("LOGIN/*", Some(0)).len(), 1);
        assert_eq!(k.select("*", None).len(), 4);
        assert!(k.select("TITLE", None).is_empty(), "ambiguous row id");
        assert_eq!(k.select("cancel", None).len(), 1);
        assert!(wildcard("a*b?d", "aXYbcd") && wildcard("*", "") && !wildcard("a?", "a") && wildcard("*x*", "abxcd"));
    }
}
