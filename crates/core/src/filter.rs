//! Row filters (Sisulizer's filter panel / SlMake `-status:` style options).

use serde::{Deserialize, Serialize};

use crate::model::{NodeRef, Project, Row, RowRef};
use crate::status::{RowStatus, TranslationStatus};

/// Where the search text is looked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SearchIn {
    #[default]
    All,
    Original,
    Translation,
    Id,
    Comment,
}

/// Filter applied to the rows of a project. Empty / `None` fields do not restrict.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct RowFilter {
    /// Free text search.
    pub text: String,
    pub case_sensitive: bool,
    pub search_in: SearchIn,
    /// Languages whose translations are considered (status filter, translation search). Empty = all project languages.
    pub langs: Vec<String>,
    /// Translation status: a row matches if *any* considered language has one of these.
    pub statuses: Vec<TranslationStatus>,
    pub row_statuses: Vec<RowStatus>,
    /// Restrict to these sources (indices).
    pub sources: Vec<usize>,
    /// Restrict to rows inside this container.
    pub node: Option<NodeRef>,
    /// "Nicht übersetzen" flag.
    pub locked: Option<bool>,
    pub invalidated: Option<bool>,
    pub marked: Option<bool>,
    /// Hide excluded rows.
    pub include_excluded: bool,
}

impl RowFilter {
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
            && self.statuses.is_empty()
            && self.row_statuses.is_empty()
            && self.sources.is_empty()
            && self.node.is_none()
            && self.locked.is_none()
            && self.invalidated.is_none()
            && self.marked.is_none()
    }

    fn languages(&self, p: &Project) -> Vec<String> {
        if self.langs.is_empty() { p.language_ids() } else { self.langs.clone() }
    }

    pub fn matches(&self, p: &Project, r: &RowRef) -> bool {
        let Some(row) = p.row(r) else {
            return false;
        };
        let langs = self.languages(p);
        self.matches_row(p, r, row, &langs)
    }

    fn matches_row(&self, p: &Project, r: &RowRef, row: &Row, langs: &[String]) -> bool {
        if !self.include_excluded && row.excluded() {
            return false;
        }
        if !self.sources.is_empty() && !self.sources.contains(&r.source) {
            return false;
        }
        if let Some(node) = &self.node {
            if node.path.is_empty() {
                if node.source != r.source {
                    return false;
                }
            } else if !r.is_inside(node) {
                return false;
            }
        }
        if !self.row_statuses.is_empty() && !self.row_statuses.contains(&row.status()) {
            return false;
        }
        if self.locked.is_some_and(|v| v != row.locked()) || self.marked.is_some_and(|v| v != row.marked()) {
            return false;
        }
        let orig = p.original_lang(r);
        let targets: Vec<&String> = langs.iter().filter(|l| l.as_str() != orig).collect();
        if !self.statuses.is_empty() && !targets.iter().any(|l| self.statuses.contains(&row.tstatus(l))) {
            return false;
        }
        if let Some(inv) = self.invalidated {
            let any = targets.iter().any(|l| row.translation(l).is_some_and(|t| t.attrs.get("invalidated") == Some("1")));
            if any != inv {
                return false;
            }
        }
        self.text.is_empty() || self.text_matches(p, r, row, &targets)
    }

    fn text_matches(&self, p: &Project, r: &RowRef, row: &Row, targets: &[&String]) -> bool {
        let needle = if self.case_sensitive { self.text.clone() } else { self.text.to_lowercase() };
        let hit = |s: &str| if self.case_sensitive { s.contains(&needle) } else { s.to_lowercase().contains(&needle) };
        let all = self.search_in == SearchIn::All;
        if (all || self.search_in == SearchIn::Original) && hit(row.original()) {
            return true;
        }
        if (all || self.search_in == SearchIn::Id) && hit(&p.context(r)) {
            return true;
        }
        if (all || self.search_in == SearchIn::Comment) && (hit(row.note()) || targets.iter().any(|l| row.translation(l).is_some_and(|t| hit(t.comment())))) {
            return true;
        }
        (all || self.search_in == SearchIn::Translation) && targets.iter().any(|l| row.text(l).is_some_and(hit))
    }

    /// All matching rows in document order.
    pub fn apply(&self, p: &Project) -> Vec<RowRef> {
        let langs = self.languages(p);
        p.rows().into_iter().filter(|r| p.row(r).is_some_and(|row| self.matches_row(p, r, row, &langs))).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Item, Source, SourceKind};

    fn project() -> Project {
        let mut p = Project::new();
        p.add_language("de");
        let mut s = Source::new(SourceKind::Json, "a.json", "en");
        let mut a = Row::new("hello", "Hello");
        a.set_translation("de", "Hallo", TranslationStatus::Translated);
        s.items.push(Item::Row(a));
        s.items.push(Item::Row(Row::new("bye", "Goodbye")));
        p.sources.push(s);
        p
    }

    #[test]
    fn status_and_text() {
        let p = project();
        let f = RowFilter { statuses: vec![TranslationStatus::NotTranslated], ..Default::default() };
        assert_eq!(f.apply(&p).len(), 1);
        let f = RowFilter { text: "hallo".into(), ..Default::default() };
        assert_eq!(f.apply(&p).len(), 1);
        let f = RowFilter { text: "hallo".into(), search_in: SearchIn::Original, ..Default::default() };
        assert!(f.apply(&p).is_empty());
        assert_eq!(RowFilter::default().apply(&p).len(), 2);
    }
}
