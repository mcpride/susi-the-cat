//! Project statistics and reports (Projekt | Statistik, SlMake `report`).

use std::collections::BTreeMap;

use serde::Serialize;

use crate::i18n::t;
use crate::lang;
use crate::model::{Project, RowRef};
use crate::status::{RowStatus, TranslationStatus};
use crate::xml::escape_text;

#[derive(Debug, Clone, Default, Serialize)]
pub struct LangStats {
    pub lang: String,
    pub name: String,
    /// Count of cells per translation status (index = status).
    pub status: [usize; 6],
    /// Words of the originals per translation status.
    pub words: [usize; 6],
}

impl LangStats {
    pub fn total(&self) -> usize {
        self.status.iter().sum()
    }

    /// Cells that count as translated (everything except "Nicht übersetzt").
    pub fn translated(&self) -> usize {
        self.total() - self.status[0]
    }

    pub fn percent(&self) -> f64 {
        let t = self.total();
        if t == 0 { 100.0 } else { self.translated() as f64 * 100.0 / t as f64 }
    }
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Statistics {
    pub rows: usize,
    pub words: usize,
    pub characters: usize,
    pub row_status: BTreeMap<String, usize>,
    pub locked: usize,
    pub languages: Vec<LangStats>,
}

pub fn word_count(s: &str) -> usize {
    s.split(|c: char| c.is_whitespace() || "/-–—".contains(c)).filter(|w| w.chars().any(char::is_alphanumeric)).count()
}

/// Statistics over the given rows (all rows if `None`). Excluded and binary rows are skipped,
/// locked rows are counted in `locked` only.
pub fn statistics(p: &Project, rows: Option<&[RowRef]>) -> Statistics {
    let refs = rows.map(<[RowRef]>::to_vec).unwrap_or_else(|| p.rows());
    let mut st = Statistics::default();
    let ids = p.language_ids();
    let mut langs: Vec<LangStats> = ids.iter().map(|l| LangStats { lang: l.clone(), name: lang::name(l), ..Default::default() }).collect();
    for s in RowStatus::ALL {
        st.row_status.insert(s.label().to_string(), 0);
    }
    for r in &refs {
        let Some(row) = p.row(r) else {
            continue;
        };
        if row.excluded() || row.raw {
            continue;
        }
        st.rows += 1;
        *st.row_status.entry(row.status().label().to_string()).or_default() += 1;
        if row.locked() {
            st.locked += 1;
            continue;
        }
        let w = word_count(row.original());
        st.words += w;
        st.characters += row.original().chars().count();
        let orig = p.original_lang(r);
        for ls in langs.iter_mut().filter(|ls| ls.lang != orig) {
            let i = row.tstatus(&ls.lang).index() as usize;
            ls.status[i] += 1;
            ls.words[i] += w;
        }
    }
    st.languages = langs;
    st
}

impl Statistics {
    /// Plain text table (current UI language).
    pub fn to_text(&self) -> String {
        let mut s = self.totals_line("  ");
        s.push('\n');
        let rs: Vec<String> = self.row_status.iter().map(|(k, v)| format!("{k}: {v}")).collect();
        s.push_str(&format!("{}: {}\n\n", t("Row status"), rs.join(", ")));
        s.push_str(&format!("{:<28}", t("Language")));
        for st in TranslationStatus::ALL {
            s.push_str(&format!("{:>14}", st.label().chars().take(13).collect::<String>()));
        }
        s.push_str(&format!("{:>9}\n", t("Done")));
        for l in &self.languages {
            s.push_str(&format!("{:<28}", format!("{} [{}]", l.name, l.lang).chars().take(27).collect::<String>()));
            for n in l.status {
                s.push_str(&format!("{n:>14}"));
            }
            s.push_str(&format!("{:>8.1}%\n", l.percent()));
        }
        s
    }

    /// "Rows: … Words: …" summary line.
    fn totals_line(&self, sep: &str) -> String {
        [
            crate::tf!("Rows: {n}", n = self.rows),
            crate::tf!("Words: {n}", n = self.words),
            crate::tf!("Characters: {n}", n = self.characters),
            crate::tf!("Do not translate: {n}", n = self.locked),
        ]
        .join(sep)
    }

    /// Simple HTML report.
    pub fn to_html(&self, title: &str) -> String {
        let mut s = format!(
            "<!DOCTYPE html>\n<html lang=\"{lang}\"><head><meta charset=\"utf-8\"><title>{title}</title><style>body{{font-family:Segoe UI,sans-serif}}table{{border-collapse:collapse}}td,th{{border:1px solid #bbb;padding:3px 8px;text-align:right}}td:first-child,th:first-child{{text-align:left}}</style></head><body>\n<h1>{title}</h1>\n<p>{totals}</p>\n<table><tr><th>{language}</th>",
            lang = crate::i18n::current().code(),
            title = escape_text(title),
            totals = escape_text(&self.totals_line(" \u{b7} ")),
            language = escape_text(t("Language"))
        );
        for st in TranslationStatus::ALL {
            let [r, g, b] = st.color();
            s.push_str(&format!("<th style=\"color:rgb({r},{g},{b})\">{}</th>", escape_text(st.label())));
        }
        s.push_str(&format!("<th>{}</th></tr>\n", escape_text(t("Done"))));
        for l in &self.languages {
            s.push_str(&format!("<tr><td>{} [{}]</td>", escape_text(&l.name), escape_text(&l.lang)));
            for n in l.status {
                s.push_str(&format!("<td>{n}</td>"));
            }
            s.push_str(&format!("<td>{:.1}%</td></tr>\n", l.percent()));
        }
        s.push_str("</table>\n</body></html>\n");
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words() {
        assert_eq!(word_count("Hello, big world - 3 times!"), 5);
        assert_eq!(word_count("   "), 0);
    }
}
