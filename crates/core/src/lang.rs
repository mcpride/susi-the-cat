//! Language ids and localized output file name patterns.

use std::path::{Path, PathBuf};

/// (Language id, German name, English name, Windows code)
const LANGUAGES: &[(&str, &str, &str, &str)] = &[
    ("af", "Afrikaans", "Afrikaans", "AFK"),
    ("ar", "Arabisch", "Arabic", "ARA"),
    ("bg", "Bulgarisch", "Bulgarian", "BGR"),
    ("bs", "Bosnisch", "Bosnian", "BSB"),
    ("ca", "Katalanisch", "Catalan", "CAT"),
    ("cs", "Tschechisch", "Czech", "CSY"),
    ("cy", "Walisisch", "Welsh", "CYM"),
    ("da", "Dänisch", "Danish", "DAN"),
    ("de", "Deutsch", "German", "DEU"),
    ("de-AT", "Deutsch (Österreich)", "German (Austria)", "DEA"),
    ("de-CH", "Deutsch (Schweiz)", "German (Switzerland)", "DES"),
    ("el", "Griechisch", "Greek", "ELL"),
    ("en", "Englisch", "English", "ENU"),
    ("en-GB", "Englisch (Großbritannien)", "English (United Kingdom)", "ENG"),
    ("en-US", "Englisch (USA)", "English (United States)", "ENU"),
    ("es", "Spanisch", "Spanish", "ESN"),
    ("es-MX", "Spanisch (Mexiko)", "Spanish (Mexico)", "ESM"),
    ("et", "Estnisch", "Estonian", "ETI"),
    ("eu", "Baskisch", "Basque", "EUQ"),
    ("fa", "Persisch", "Persian", "FAR"),
    ("fi", "Finnisch", "Finnish", "FIN"),
    ("fr", "Französisch", "French", "FRA"),
    ("fr-CA", "Französisch (Kanada)", "French (Canada)", "FRC"),
    ("fr-CH", "Französisch (Schweiz)", "French (Switzerland)", "FRS"),
    ("ga", "Irisch", "Irish", "IRE"),
    ("gl", "Galizisch", "Galician", "GLC"),
    ("he", "Hebräisch", "Hebrew", "HEB"),
    ("hi", "Hindi", "Hindi", "HIN"),
    ("hr", "Kroatisch", "Croatian", "HRV"),
    ("hu", "Ungarisch", "Hungarian", "HUN"),
    ("id", "Indonesisch", "Indonesian", "IND"),
    ("is", "Isländisch", "Icelandic", "ISL"),
    ("it", "Italienisch", "Italian", "ITA"),
    ("it-CH", "Italienisch (Schweiz)", "Italian (Switzerland)", "ITS"),
    ("ja", "Japanisch", "Japanese", "JPN"),
    ("ka", "Georgisch", "Georgian", "KAT"),
    ("kk", "Kasachisch", "Kazakh", "KKZ"),
    ("ko", "Koreanisch", "Korean", "KOR"),
    ("lt", "Litauisch", "Lithuanian", "LTH"),
    ("lv", "Lettisch", "Latvian", "LVI"),
    ("mk", "Mazedonisch", "Macedonian", "MKI"),
    ("ms", "Malaiisch", "Malay", "MSL"),
    ("mt", "Maltesisch", "Maltese", "MLT"),
    ("nb", "Norwegisch (Bokmål)", "Norwegian (Bokmål)", "NOR"),
    ("nl", "Niederländisch", "Dutch", "NLD"),
    ("nl-BE", "Niederländisch (Belgien)", "Dutch (Belgium)", "NLB"),
    ("nn", "Norwegisch (Nynorsk)", "Norwegian (Nynorsk)", "NON"),
    ("no", "Norwegisch", "Norwegian", "NOR"),
    ("pl", "Polnisch", "Polish", "PLK"),
    ("pt", "Portugiesisch", "Portuguese", "PTG"),
    ("pt-BR", "Portugiesisch (Brasilien)", "Portuguese (Brazil)", "PTB"),
    ("ro", "Rumänisch", "Romanian", "ROM"),
    ("ru", "Russisch", "Russian", "RUS"),
    ("sk", "Slowakisch", "Slovak", "SKY"),
    ("sl", "Slowenisch", "Slovenian", "SLV"),
    ("sq", "Albanisch", "Albanian", "SQI"),
    ("sr", "Serbisch", "Serbian", "SRB"),
    ("sv", "Schwedisch", "Swedish", "SVE"),
    ("th", "Thai", "Thai", "THA"),
    ("tr", "Türkisch", "Turkish", "TRK"),
    ("uk", "Ukrainisch", "Ukrainian", "UKR"),
    ("ur", "Urdu", "Urdu", "URD"),
    ("vi", "Vietnamesisch", "Vietnamese", "VIT"),
    ("zh", "Chinesisch (vereinfacht)", "Chinese (Simplified)", "CHS"),
    ("zh-CN", "Chinesisch (China)", "Chinese (China)", "CHS"),
    ("zh-TW", "Chinesisch (Taiwan)", "Chinese (Taiwan)", "CHT"),
    ("zh.tra", "Chinesisch (traditionell)", "Chinese (Traditional)", "CHT"),
];

/// All known language ids with their name in the current UI language (for language pickers).
pub fn known_languages() -> impl Iterator<Item = (&'static str, &'static str)> {
    let german = crate::i18n::current() == crate::i18n::Lang::DE;
    LANGUAGES.iter().map(move |(id, de, en, _)| (*id, if german { *de } else { *en }))
}

fn lookup(id: &str) -> Option<&'static (&'static str, &'static str, &'static str, &'static str)> {
    let norm = id.replace('_', "-");
    LANGUAGES.iter().find(|(i, ..)| i.eq_ignore_ascii_case(&norm))
}

/// Whether `id` (`de`, `pt-BR`, `pt_BR`) is a known language id.
pub fn is_known(id: &str) -> bool {
    lookup(id).is_some()
}

/// German display name ("Deutsch"); unknown ids are returned unchanged.
pub fn name_de(id: &str) -> String {
    match lookup(id) {
        Some((_, de, ..)) => (*de).to_string(),
        None if id == "sis" => "Pseudo-Original (sis)".into(),
        None => id.to_string(),
    }
}

pub fn name_en(id: &str) -> String {
    match lookup(id) {
        Some((_, _, en, _)) => (*en).to_string(),
        None if id == "sis" => "Pseudo original (sis)".into(),
        None => id.to_string(),
    }
}

/// Display name in the current UI language.
pub fn name(id: &str) -> String {
    if crate::i18n::current() == crate::i18n::Lang::DE { name_de(id) } else { name_en(id) }
}

/// Whether `s` is the id, German or English name of language `id`.
pub fn matches_name(id: &str, s: &str) -> bool {
    id.eq_ignore_ascii_case(s) || name_de(id).eq_ignore_ascii_case(s) || name_en(id).eq_ignore_ascii_case(s)
}

/// `la_CO` form (`<iso>`).
pub fn iso(id: &str) -> String {
    match id {
        "zh.tra" => "zh_TW".into(),
        _ => id.replace('-', "_"),
    }
}

/// `la-CO` form (`<ish>`).
pub fn ish(id: &str) -> String {
    iso(id).replace('_', "-")
}

/// .NET culture name (`<net>`, .NET 4 style).
pub fn net(id: &str) -> String {
    match id {
        "zh" => "zh-Hans".into(),
        "zh.tra" => "zh-Hant".into(),
        _ => ish(id),
    }
}

/// IETF tag (`<tag>`).
pub fn tag(id: &str) -> String {
    net(id)
}

/// Android BCP 47 qualifier without the `b+` prefix (`<android>`): subtags joined by `+`
/// (`de`, `pt+BR`, `zh+Hans`), used as `values-b+<android>` (Android 7.0+).
pub fn android(id: &str) -> String {
    tag(id).replace('-', "+")
}

/// Legacy Android locale qualifier (`<androidr>`): language plus `-r` region (`de`, `pt-rBR`),
/// understood by every Android version. Script subtags cannot be expressed (Chinese uses the
/// regions CN/TW), and Hebrew, Indonesian and Yiddish use the old codes `iw`, `in` and `ji`.
pub fn android_r(id: &str) -> String {
    let iso = match id {
        "zh" => "zh_CN".to_string(),
        _ => iso(id),
    };
    let mut parts = iso.split('_');
    let lang = parts.next().unwrap_or("").to_ascii_lowercase();
    let mut out = match lang.as_str() {
        "he" => "iw".to_string(),
        "id" => "in".to_string(),
        "yi" => "ji".to_string(),
        _ => lang,
    };
    for p in parts {
        let region = (p.len() == 2 && p.chars().all(|c| c.is_ascii_alphabetic())) || (p.len() == 3 && p.chars().all(|c| c.is_ascii_digit()));
        if region {
            out.push_str("-r");
            out.push_str(&p.to_ascii_uppercase());
        }
    }
    out
}

/// Windows code (`<win>`).
pub fn win(id: &str) -> String {
    match lookup(id) {
        Some((.., w)) => (*w).to_string(),
        None => id.to_ascii_uppercase(),
    }
}

/// Context for expanding an output file name pattern.
pub struct PatternContext<'a> {
    /// The original file being localized.
    pub file: &'a Path,
    /// Directory of the source definition (the wildcard root for wildcard sources).
    pub source_dir: &'a Path,
    /// Output directory (already resolved to an absolute or project-relative path).
    pub out_dir: &'a Path,
    pub lang: &'a str,
}

/// Expands an output pattern such as `<iso>_v5.<ext>` or `<sl>\<file>`.
pub fn expand_pattern(pattern: &str, ctx: &PatternContext<'_>) -> PathBuf {
    let file_name = ctx.file.file_name().and_then(|s| s.to_str()).unwrap_or("");
    let body = ctx.file.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    let ext = ctx.file.extension().and_then(|s| s.to_str()).unwrap_or("");
    let rel_dir = ctx
        .file
        .parent()
        .and_then(|p| p.strip_prefix(ctx.source_dir).ok())
        .map(|p| {
            let s = p.to_string_lossy().to_string();
            if s.is_empty() { s } else { format!("{s}\\") }
        })
        .unwrap_or_default();
    let parent = ctx.file.parent().and_then(Path::parent).map(|p| p.to_string_lossy().to_string()).unwrap_or_default();

    let uses_parent = pattern.contains("<parent>");
    let mut p = pattern.to_string();
    if !p.contains("<dir>") && !uses_parent {
        p = format!("<dir>{p}");
    }
    let vars: [(&str, String); 16] = [
        ("<file>", file_name.to_string()),
        ("<body>", body.to_string()),
        ("<ext>", ext.to_string()),
        ("<dir>", rel_dir),
        ("<parent>", parent),
        ("<name>", body.to_string()),
        ("<sl>", ctx.lang.to_string()),
        ("<iso>", iso(ctx.lang)),
        ("<ish>", ish(ctx.lang)),
        ("<net>", net(ctx.lang)),
        ("<tag>", tag(ctx.lang)),
        ("<win>", win(ctx.lang)),
        ("<mfc>", win(ctx.lang)),
        ("<android>", android(ctx.lang)),
        ("<androidr>", android_r(ctx.lang)),
        ("<lang>", ctx.lang.to_string()),
    ];
    for (k, v) in vars {
        p = p.replace(k, &v);
    }
    let p = p.replace(['\\', '/'], std::path::MAIN_SEPARATOR_STR);
    if uses_parent { PathBuf::from(p) } else { ctx.out_dir.join(p) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patterns() {
        let file = Path::new("C:/p/i18n/sis.json");
        let ctx = PatternContext { file, source_dir: Path::new("C:/p/i18n"), out_dir: Path::new("C:/p/i18n"), lang: "pt-BR" };
        assert_eq!(expand_pattern("<iso>_v5.<ext>", &ctx), Path::new("C:/p/i18n").join("pt_BR_v5.json"));
        assert_eq!(expand_pattern("<sl>\\<file>", &ctx), Path::new("C:/p/i18n").join("pt-BR").join("sis.json"));
        let ctx = PatternContext { file: Path::new("C:/p/Res.resx"), source_dir: Path::new("C:/p"), out_dir: Path::new("C:/p"), lang: "de" };
        assert_eq!(expand_pattern("<body>.<net>.<ext>", &ctx), Path::new("C:/p").join("Res.de.resx"));
        let ctx = PatternContext {
            file: Path::new("C:/p/res/values/strings.xml"),
            source_dir: Path::new("C:/p/res/values"),
            out_dir: Path::new("C:/x"),
            lang: "pt-BR",
        };
        assert_eq!(expand_pattern("<parent>\\values-b+<android>\\<file>", &ctx), Path::new("C:/p/res").join("values-b+pt+BR").join("strings.xml"));
        assert_eq!(expand_pattern("<parent>\\values-<androidr>\\<file>", &ctx), Path::new("C:/p/res").join("values-pt-rBR").join("strings.xml"));
    }

    #[test]
    fn android_qualifiers() {
        assert_eq!(android("de"), "de");
        assert_eq!(android("pt-BR"), "pt+BR");
        assert_eq!(android("zh"), "zh+Hans");
        assert_eq!(android("zh.tra"), "zh+Hant");
        assert_eq!(android_r("de"), "de");
        assert_eq!(android_r("pt_BR"), "pt-rBR");
        assert_eq!(android_r("zh"), "zh-rCN");
        assert_eq!(android_r("zh.tra"), "zh-rTW");
        assert_eq!(android_r("he"), "iw");
        assert_eq!(android_r("es-419"), "es-r419");
    }

    #[test]
    fn names() {
        assert_eq!(name_de("de"), "Deutsch");
        assert_eq!(name_de("xx"), "xx");
        assert_eq!(name("de"), "German");
        assert!(matches_name("de", "deutsch") && matches_name("de", "German") && !matches_name("de", "French"));
        assert_eq!(win("de"), "DEU");
    }
}
