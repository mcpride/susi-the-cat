//! Row filters (Sisulizer's filter panel / SlMake `-status:` style options).

use serde::{Deserialize, Serialize};

use chrono::{Duration, Local, NaiveDate, NaiveDateTime, NaiveTime};

use crate::i18n::t;
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

/// Timestamp a date range is applied to (Sisulizer's filter sheet "Time").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DateField {
    /// Any of the timestamps below.
    #[default]
    Any,
    /// Row created (first found by a scan).
    Created,
    /// Original text changed by a scan.
    Changed,
    /// Translation of a considered language last edited.
    Translated,
}

impl DateField {
    pub const ALL: [DateField; 4] = [DateField::Any, DateField::Created, DateField::Changed, DateField::Translated];

    /// Keyword used by the CLI (`-datetype:`) and MCP.
    pub fn keyword(self) -> &'static str {
        match self {
            Self::Any => "any",
            Self::Created => "created",
            Self::Changed => "changed",
            Self::Translated => "translated",
        }
    }

    pub fn from_keyword(s: &str) -> Option<Self> {
        let s = s.trim().to_ascii_lowercase();
        Self::ALL.into_iter().find(|f| f.keyword() == s)
    }

    /// Label in the current UI language.
    pub fn label(self) -> &'static str {
        match self {
            Self::Any => t("Any change"),
            Self::Created => t("Row created"),
            Self::Changed => t("Original changed"),
            Self::Translated => t("Translated"),
        }
    }
}

/// Parses a date for a filter bound into a `yyyymmddhhnnss` timestamp (local time).
///
/// Accepts `2026-11-01`, `2026-11-01 14:30[:00]` (also with `T`), `01.11.2026 [14:30]`, `20261101`,
/// `20261101143000`, `today`, `yesterday`, `7d` (7 days ago) and `12h` (12 hours ago).
/// A date without time means the start of the day, or its end if `end` is set.
pub fn parse_date(s: &str, end: bool) -> Option<String> {
    let s = s.trim();
    let day = |d: NaiveDate| d.and_time(if end { NaiveTime::from_hms_opt(23, 59, 59).unwrap_or_default() } else { NaiveTime::MIN });
    let today = Local::now().date_naive();
    let dt = match s.to_ascii_lowercase().as_str() {
        "today" | "heute" => Some(day(today)),
        "yesterday" | "gestern" => Some(day(today - Duration::days(1))),
        rel if rel.len() > 1 && rel.ends_with(['d', 't', 'h']) && rel[..rel.len() - 1].bytes().all(|b| b.is_ascii_digit()) => {
            let n: i64 = rel[..rel.len() - 1].parse().ok()?;
            match rel.as_bytes()[rel.len() - 1] {
                b'd' | b't' => Some(day(today - Duration::days(n))),
                b'h' => Some(Local::now().naive_local() - Duration::hours(n)),
                _ => None,
            }
        }
        _ => absolute_date(s).map(|(dt, has_time)| if has_time { dt } else { day(dt.date()) }),
    }?;
    Some(dt.format("%Y%m%d%H%M%S").to_string())
}

/// Parses an absolute date (time); returns whether a time was given.
fn absolute_date(s: &str) -> Option<(NaiveDateTime, bool)> {
    let digits = s.bytes().all(|b| b.is_ascii_digit());
    if digits && s.len() == 14 {
        return NaiveDateTime::parse_from_str(s, "%Y%m%d%H%M%S").ok().map(|d| (d, true));
    }
    if digits && s.len() == 8 {
        return NaiveDate::parse_from_str(&format!("{}-{}-{}", &s[..4], &s[4..6], &s[6..]), "%Y-%m-%d").ok().map(|d| (d.and_time(NaiveTime::MIN), false));
    }
    const WITH_TIME: [&str; 6] = ["%Y-%m-%d %H:%M:%S", "%Y-%m-%dT%H:%M:%S", "%Y-%m-%d %H:%M", "%Y-%m-%dT%H:%M", "%d.%m.%Y %H:%M:%S", "%d.%m.%Y %H:%M"];
    if let Some(d) = WITH_TIME.iter().find_map(|f| NaiveDateTime::parse_from_str(s, f).ok()) {
        return Some((d, true));
    }
    ["%Y-%m-%d", "%d.%m.%Y"].iter().find_map(|f| NaiveDate::parse_from_str(s, f).ok()).map(|d| (d.and_time(NaiveTime::MIN), false))
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
    /// Timestamp checked against `since` / `until`.
    pub date_field: DateField,
    /// Earliest timestamp (`yyyymmddhhnnss`, inclusive), see [`parse_date`].
    pub since: Option<String>,
    /// Latest timestamp (`yyyymmddhhnnss`, inclusive).
    pub until: Option<String>,
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
            && !self.has_date_range()
    }

    fn has_date_range(&self) -> bool {
        self.since.is_some() || self.until.is_some()
    }

    /// Whether any timestamp selected by `date_field` lies in the range.
    fn date_matches(&self, row: &Row, targets: &[&String]) -> bool {
        let in_range = |s: &str| {
            let s = format!("{s:0<14}");
            self.since.as_deref().is_none_or(|v| s.as_str() >= v) && self.until.as_deref().is_none_or(|v| s.as_str() <= v)
        };
        let f = self.date_field;
        let row_stamp = |name: &str| row.attrs.get(name).is_some_and(in_range);
        (matches!(f, DateField::Any | DateField::Created) && row_stamp("created"))
            || (matches!(f, DateField::Any | DateField::Changed) && row_stamp("changed"))
            || (matches!(f, DateField::Any | DateField::Translated) && targets.iter().any(|l| row.translation_date(l).is_some_and(in_range)))
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
        if self.has_date_range() && !self.date_matches(row, &targets) {
            return false;
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

    #[test]
    fn date_range() {
        let mut p = project();
        let rows = p.rows();
        for (r, (created, date)) in rows.iter().zip([("20240101120000", "20260310080000"), ("20250615093000", "")]) {
            let row = p.row_mut(r).unwrap();
            row.attrs.set("created", created);
            if let Some(t) = row.translation_mut("de") {
                t.attrs.set("date", date);
            }
        }
        let count = |field, since: &str, until: &str| {
            let f = RowFilter { date_field: field, since: parse_date(since, false), until: parse_date(until, true), ..Default::default() };
            assert!(!f.is_empty());
            f.apply(&p).len()
        };
        assert_eq!(count(DateField::Created, "2025-01-01", "2025-12-31"), 1);
        assert_eq!(count(DateField::Created, "20240101", "15.06.2025 09:30"), 2);
        assert_eq!(count(DateField::Translated, "2026-03-10", "2026-03-10"), 1);
        assert_eq!(count(DateField::Translated, "2026-03-11", "today"), 0);
        assert_eq!(count(DateField::Changed, "2000-01-01", "today"), 0);
        assert_eq!(count(DateField::Any, "2025-06-15T09:30", "today"), 2);
    }

    #[test]
    fn date_parsing() {
        assert_eq!(parse_date("2026-11-01", false).as_deref(), Some("20261101000000"));
        assert_eq!(parse_date("2026-11-01", true).as_deref(), Some("20261101235959"));
        assert_eq!(parse_date(" 01.11.2026 14:30 ", true).as_deref(), Some("20261101143000"));
        assert_eq!(parse_date("20261101143005", false).as_deref(), Some("20261101143005"));
        let today = Local::now().format("%Y%m%d").to_string();
        assert_eq!(parse_date("today", false), Some(format!("{today}000000")));
        assert_eq!(parse_date("0d", true), Some(format!("{today}235959")));
        assert!(parse_date("7d", false).is_some_and(|s| s < today));
        for bad in ["", "2026-13-01", "next week", "d", "7x"] {
            assert_eq!(parse_date(bad, false), None, "{bad}");
        }
    }
}
