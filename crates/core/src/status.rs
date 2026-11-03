//! Translation and row status values as stored in Susi/Sisulizer `.slp` files.
//!
//! Observed: `<lang status="N">` uses the SlMake numbering 0..5; a translation without a `status`
//! attribute is shown by Sisulizer as "Complete". `<row state="0">` is shown as "New".
//! Inferred: row `state` 1 = Unused, 3 = Changed; a row without `state` is "In use".

use serde::{Deserialize, Serialize};

use crate::i18n::t;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TranslationStatus {
    NotTranslated = 0,
    BestGuess = 1,
    AutoTranslated = 2,
    Translated = 3,
    ForReview = 4,
    Complete = 5,
}

impl TranslationStatus {
    pub const ALL: [TranslationStatus; 6] = [
        TranslationStatus::NotTranslated,
        TranslationStatus::BestGuess,
        TranslationStatus::AutoTranslated,
        TranslationStatus::Translated,
        TranslationStatus::ForReview,
        TranslationStatus::Complete,
    ];

    pub fn from_index(i: u32) -> Option<Self> {
        Self::ALL.get(i as usize).copied()
    }

    pub fn index(self) -> u32 {
        self as u32
    }

    /// Label in the current UI language.
    pub fn label(self) -> &'static str {
        match self {
            Self::NotTranslated => t("Not translated"),
            Self::BestGuess => t("Best guess"),
            Self::AutoTranslated => t("Auto translated"),
            Self::Translated => t("Translated"),
            Self::ForReview => t("For review"),
            Self::Complete => t("Complete"),
        }
    }

    /// Parses a label in any UI language, a keyword or an index.
    pub fn from_label(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| crate::i18n::matches_any(t.label_en(), s)).or_else(|| Self::from_keyword(s))
    }

    pub fn label_en(self) -> &'static str {
        match self {
            Self::NotTranslated => "Not translated",
            Self::BestGuess => "Best guess",
            Self::AutoTranslated => "Auto translated",
            Self::Translated => "Translated",
            Self::ForReview => "For review",
            Self::Complete => "Complete",
        }
    }

    /// Keyword used by SlMake's `-status:` option.
    pub fn keyword(self) -> &'static str {
        match self {
            Self::NotTranslated => "untranslated",
            Self::BestGuess => "best",
            Self::AutoTranslated => "auto",
            Self::Translated => "translated",
            Self::ForReview => "review",
            Self::Complete => "complete",
        }
    }

    pub fn from_keyword(s: &str) -> Option<Self> {
        let s = s.trim().to_ascii_lowercase();
        if let Ok(n) = s.parse::<u32>() {
            return Self::from_index(n);
        }
        Self::ALL.into_iter().find(|t| t.keyword() == s || t.label_en().eq_ignore_ascii_case(&s) || format!("{t:?}").eq_ignore_ascii_case(&s))
    }

    /// Whether the text counts as translated for build purposes.
    pub fn is_translated(self) -> bool {
        self != Self::NotTranslated
    }

    /// RGB colour used in the grid (green for complete, as in Sisulizer).
    pub fn color(self) -> [u8; 3] {
        match self {
            Self::NotTranslated => [220, 60, 60],
            Self::BestGuess => [230, 150, 30],
            Self::AutoTranslated => [200, 170, 0],
            Self::Translated => [40, 120, 220],
            Self::ForReview => [150, 80, 200],
            Self::Complete => [30, 150, 60],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RowStatus {
    New = 0,
    Unused = 1,
    InUse = 2,
    Changed = 3,
}

impl RowStatus {
    pub const ALL: [RowStatus; 4] = [RowStatus::New, RowStatus::Unused, RowStatus::InUse, RowStatus::Changed];

    pub fn from_index(i: u32) -> Option<Self> {
        Self::ALL.get(i as usize).copied()
    }

    /// Label in the current UI language.
    pub fn label(self) -> &'static str {
        match self {
            Self::New => t("New"),
            Self::Unused => t("Unused"),
            Self::InUse => t("In use"),
            Self::Changed => t("Changed"),
        }
    }

    pub fn label_en(self) -> &'static str {
        match self {
            Self::New => "New",
            Self::Unused => "Unused",
            Self::InUse => "In use",
            Self::Changed => "Changed",
        }
    }

    /// Keyword used by SlMake's `-rowstatus:` option.
    pub fn keyword(self) -> &'static str {
        match self {
            Self::New => "new",
            Self::Unused => "unused",
            Self::InUse => "inuse",
            Self::Changed => "changed",
        }
    }

    pub fn from_keyword(s: &str) -> Option<Self> {
        let s = s.trim().to_ascii_lowercase();
        if let Ok(n) = s.parse::<u32>() {
            return Self::from_index(n);
        }
        Self::ALL.into_iter().find(|t| t.keyword() == s)
    }
}

/// Parses a `;`/`,` separated list of status keywords.
pub fn parse_status_list(s: &str) -> Result<Vec<TranslationStatus>, String> {
    s.split([';', ','])
        .filter(|p| !p.trim().is_empty())
        .map(|p| TranslationStatus::from_keyword(p).ok_or_else(|| format!("unknown translation status '{p}'")))
        .collect()
}

pub fn parse_row_status_list(s: &str) -> Result<Vec<RowStatus>, String> {
    s.split([';', ',']).filter(|p| !p.trim().is_empty()).map(|p| RowStatus::from_keyword(p).ok_or_else(|| format!("unknown row status '{p}'"))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keywords() {
        assert_eq!(parse_status_list("untranslated;best").unwrap(), vec![TranslationStatus::NotTranslated, TranslationStatus::BestGuess]);
        assert_eq!(TranslationStatus::from_keyword("3"), Some(TranslationStatus::Translated));
        assert!(parse_status_list("foo").is_err());
        assert_eq!(parse_row_status_list("new,changed").unwrap(), vec![RowStatus::New, RowStatus::Changed]);
    }
}
