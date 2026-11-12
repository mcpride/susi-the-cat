//! Task implementations (one function per SlMake task).

use std::io::Write;
use std::path::{Path, PathBuf};

use susi_core::exchange::{self, ExportOptions, ImportOptions};
use susi_core::filter::{DateField, RowFilter, parse_date};
use susi_core::i18n::{self, trn};
use susi_core::model::{Project, RowRef, SourceKind};
use susi_core::settings::Settings;
use susi_core::status::{TranslationStatus, parse_row_status_list, parse_status_list};
use susi_core::tm::{self, TranslateOptions, TranslationMemory};
use susi_core::validate::{self, Severity, ValidateOptions, ValidationKind};
use susi_core::{lang, ops, pseudo, slp, stats, tf};

use crate::args::TaskArgs;

/// Message sink with SlMake semantics: messages to stdout, errors to stderr, `-q` = errors only.
pub struct Output<'a> {
    pub out: &'a mut dyn Write,
    pub err: &'a mut dyn Write,
    pub quiet: bool,
    pub detailed: bool,
    pub errors: usize,
    pub warnings: usize,
}

impl Output<'_> {
    pub fn info(&mut self, msg: impl AsRef<str>) {
        if !self.quiet {
            let _ = writeln!(self.out, "{}", msg.as_ref());
        }
    }

    pub fn warn(&mut self, msg: impl AsRef<str>) {
        self.warnings += 1;
        if !self.quiet {
            let _ = writeln!(self.out, "{} {}", i18n::t("Warning:"), msg.as_ref());
        }
    }

    pub fn error(&mut self, msg: impl AsRef<str>) {
        self.errors += 1;
        let _ = writeln!(self.err, "{} {}", i18n::t("Error:"), msg.as_ref());
    }

    /// Informational note that does not affect the result code.
    pub fn note(&mut self, msg: impl AsRef<str>) {
        if !self.quiet {
            let _ = writeln!(self.out, "{} {}", i18n::t("Note:"), msg.as_ref());
        }
    }

    fn core(&self, e: &susi_core::Error) -> String {
        if self.detailed { format!("{e} ({e:?})") } else { e.to_string() }
    }
}

/// Project state shared by the tasks of one invocation.
pub struct State {
    pub path: PathBuf,
    pub project: Option<Project>,
    pub changed: bool,
}

impl State {
    pub fn new(path: PathBuf) -> Self {
        Self { path, project: None, changed: false }
    }

    pub(crate) fn load(&mut self, o: &Output<'_>) -> Result<&mut Project, String> {
        if self.project.is_none() {
            if !self.path.exists() {
                return Err(tf!("Project file {path} not found", path = self.path.display()));
            }
            self.project = Some(slp::load(&self.path).map_err(|e| format!("{}: {}", self.path.display(), o.core(&e)))?);
        }
        self.project.as_mut().ok_or_else(|| i18n::t("Project not loaded").to_string())
    }
}

/// Options a task accepts for SlMake compatibility but susi ignores.
fn ignored_options(task: &str) -> &'static [&'static str] {
    match task {
        "add" => &["outitem", "noimport", "subdirs"],
        "create" => &["output", "config", "input", "al"],
        "export" => &[
            "format",
            "sort",
            "separator",
            "nobom",
            "quote",
            "noheader",
            "addcomment",
            "addtcomment",
            "context",
            "invalid",
            "marked",
            "dup",
            "escape",
            "admlang",
            "nosingle",
            "srclang",
            "tmxcase",
            "tmxdtd",
            "tmxver",
            "tmxitems",
            "xlfver",
            "xlfitems",
            "tm",
        ],
        "import" => &[
            "status",
            "tcomment",
            "onlystrings",
            "empty",
            "noequal",
            "ignorecase",
            "ignorewhite",
            "ignorespe",
            "usepartial",
            "method",
            "tmarked",
            "columns",
            "textdef",
            "sheet",
            "range",
            "type",
            "tm",
        ],
        "translate" => &["first"],
        "validate" => &["name"],
        "report" => &["items", "lang"],
        _ => &[],
    }
}

pub fn run_task(t: &TaskArgs, st: &mut State, o: &mut Output<'_>) -> Result<(), String> {
    for (k, _) in &t.opts {
        if ignored_options(&t.name).contains(&k.as_str()) {
            o.note(tf!("Option -{option} is ignored by susi", option = k));
        }
    }
    match t.name.as_str() {
        "add" => add(t, st, o),
        "create" => create(t, st, o),
        "duplicate" => duplicate(t, st, o),
        "export" => export(t, st, o),
        "import" => import(t, st, o),
        "fillpseudo" => fill_pseudo(t, st, o),
        "removepseudo" => remove_pseudo(t, st, o),
        "remove" => remove(st, o),
        "report" => report(t, st, o),
        "scan" => scan(t, st, o),
        "translate" => translate(t, st, o),
        "validate" => validate(t, st, o),
        "savetotm" => save_to_tm(t, st, o),
        "info" => info(st, o),
        "workflow" => crate::workflow::run(t, st, o),
        other => Err(tf!("Task '{task}' cannot be run here", task = other)),
    }
}

/// `-lang` option, checked against the project languages.
fn languages(t: &TaskArgs, p: &Project) -> Result<Vec<String>, String> {
    let langs = t.list("lang");
    if let Some(l) = langs.iter().find(|l| !p.has_language(l)) {
        return Err(tf!("The project does not contain the language \"{lang}\" (available: {langs})", lang = l, langs = p.language_ids().join(", ")));
    }
    Ok(langs)
}

fn source_matches(name: &str, wanted: &str) -> bool {
    let n = name.replace('/', "\\").to_ascii_lowercase();
    let w = wanted.replace('/', "\\").to_ascii_lowercase();
    n == w || n.ends_with(&format!("\\{w}"))
}

/// `-include` / `-exclude` options as source indices (`None` = all sources).
fn sources(t: &TaskArgs, p: &Project) -> Result<Option<Vec<usize>>, String> {
    let inc = t.list("include");
    let exc = t.list("exclude");
    if inc.is_empty() && exc.is_empty() {
        return Ok(None);
    }
    for w in inc.iter().chain(&exc) {
        if !p.sources.iter().any(|s| source_matches(s.name(), w)) {
            return Err(tf!("The project does not contain the source \"{source}\"", source = w));
        }
    }
    let sel = (0..p.sources.len())
        .filter(|&i| {
            let name = p.sources.get(i).map(|s| s.name()).unwrap_or_default();
            (inc.is_empty() || inc.iter().any(|w| source_matches(name, w))) && !exc.iter().any(|w| source_matches(name, w))
        })
        .collect();
    Ok(Some(sel))
}

/// `-since` / `-until` / `-datetype` options as a row filter (`None` = no date range given).
fn date_filter(t: &TaskArgs) -> Result<Option<RowFilter>, String> {
    let bound = |name: &str, end: bool| {
        t.value(name).map(|v| parse_date(v, end).ok_or_else(|| tf!("Invalid value for -{name}: \"{value}\"", name = name, value = v))).transpose()
    };
    let (since, until) = (bound("since", false)?, bound("until", true)?);
    let date_field = match t.value("datetype") {
        None => DateField::Any,
        Some(v) => DateField::from_keyword(v)
            .ok_or_else(|| tf!("Invalid value for -{name}: \"{value}\"", name = "datetype", value = v) + " (any, created, changed, translated)")?,
    };
    if since.is_none() && until.is_none() {
        return Ok(None);
    }
    Ok(Some(RowFilter { date_field, since, until, ..Default::default() }))
}

/// Rows selected by the source (`sel`) and date options; `None` = all rows.
fn row_selection(t: &TaskArgs, p: &Project, langs: &[String], sel: Option<Vec<usize>>, o: &mut Output<'_>) -> Result<Option<Vec<RowRef>>, String> {
    let date = date_filter(t)?;
    if date.is_none() {
        return Ok(sel.map(|sel| p.rows().into_iter().filter(|r| sel.contains(&r.source)).collect()));
    }
    if sel.as_ref().is_some_and(Vec::is_empty) {
        return Ok(Some(Vec::new()));
    }
    let f = RowFilter { sources: sel.unwrap_or_default(), langs: langs.to_vec(), include_excluded: true, ..date.unwrap_or_default() };
    let rows = f.apply(p);
    o.info(trn(rows.len(), "{n} row matches the date filter", "{n} rows match the date filter"));
    Ok(Some(rows))
}

fn tm_path(t: &TaskArgs) -> PathBuf {
    t.value("tm").map(PathBuf::from).unwrap_or_else(|| Settings::load().tm_path())
}

fn first_file<'t>(t: &'t TaskArgs, what: &str) -> Result<&'t str, String> {
    t.files.first().map(String::as_str).ok_or_else(|| tf!("Task '{task}': {what} missing (see: susi-cli {task} -h)", task = t.name, what = what))
}

/// Case-insensitive `*`/`?` matching.
pub fn wildcard_match(name: &str, pattern: &str) -> bool {
    let n: Vec<char> = name.to_lowercase().chars().collect();
    let p: Vec<char> = pattern.to_lowercase().chars().collect();
    let (mut i, mut j, mut star, mut mark) = (0, 0, None, 0);
    while i < n.len() {
        match p.get(j) {
            Some('*') => {
                star = Some(j);
                mark = i;
                j += 1;
            }
            Some(c) if *c == '?' || Some(c) == n.get(i) => {
                i += 1;
                j += 1;
            }
            _ => match star {
                Some(s) => {
                    j = s + 1;
                    mark += 1;
                    i = mark;
                }
                None => return false,
            },
        }
    }
    p.get(j..).is_some_and(|rest| rest.iter().all(|c| *c == '*'))
}

/// Expands `*`/`?` in the file name part of `spec`.
fn expand(spec: &str) -> Vec<PathBuf> {
    let path = PathBuf::from(spec);
    let name = path.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
    if !name.contains(['*', '?']) {
        return vec![path];
    }
    let dir = path.parent().filter(|d| !d.as_os_str().is_empty()).map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."));
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .filter(|e| e.file_type().is_ok_and(|t| t.is_file()) && wildcard_match(&e.file_name().to_string_lossy(), &name))
                .map(|e| dir.join(e.file_name()))
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    files
}

fn add(t: &TaskArgs, st: &mut State, o: &mut Output<'_>) -> Result<(), String> {
    first_file(t, i18n::t("Source file"))?;
    let orig = t.value("origlang").unwrap_or("en").to_string();
    let langs = t.list("lang");
    let kind = match t.value("type").map(str::to_ascii_lowercase).as_deref() {
        None => None,
        Some("json") => Some(SourceKind::Json),
        Some("resx" | "net" | "nettxt") => Some(SourceKind::Resx),
        Some("po" | "pot" | "gettext") => Some(SourceKind::Po),
        Some("tsv") => Some(SourceKind::Tsv),
        Some("properties" | "java") => Some(SourceKind::Properties),
        Some(x) => return Err(tf!("Source type '{kind}' is not supported (json, resx, po, tsv, properties)", kind = x)),
    };
    if st.project.is_none() && !st.path.exists() {
        let mut p = Project::new();
        p.path = Some(st.path.clone());
        o.info(tf!("New project {path}", path = st.path.display()));
        st.project = Some(p);
    }
    let mut changed = st.project.is_some() && !st.path.exists();
    let p = st.load(o)?;
    for l in std::iter::once(&orig).chain(&langs) {
        if p.add_language(l) {
            o.info(tf!("Language added: {name} ({lang})", name = lang::name(l), lang = l));
            changed = true;
        }
    }
    let files: Vec<PathBuf> = t.files.iter().flat_map(|f| expand(f)).collect();
    if files.is_empty() {
        return Err(tf!("No files found: {files}", files = t.files.join(", ")));
    }
    for f in files {
        if let Some(k) = kind
            && SourceKind::from_path(&f) != Some(k)
        {
            o.error(tf!("{file}: file type does not match -type:{kind}", file = f.display(), kind = t.value("type").unwrap_or_default()));
            continue;
        }
        match ops::add_source(p, &f, &orig) {
            Ok((i, rep)) => {
                changed = true;
                let name = p.sources.get(i).map(|s| s.name()).unwrap_or_default();
                o.info(tf!("Source #{index} added: {name} – {summary}", index = i, name = name, summary = rep.summary()));
                for e in &rep.errors {
                    o.error(e);
                }
            }
            Err(e) => o.error(format!("{}: {}", f.display(), o.core(&e))),
        }
    }
    st.changed |= changed;
    Ok(())
}

fn create(t: &TaskArgs, st: &mut State, o: &mut Output<'_>) -> Result<(), String> {
    let p = st.load(o)?;
    let langs = languages(t, p)?;
    let sel = sources(t, p)?;
    let rep = ops::build_project(p, &langs, sel.as_deref());
    for f in &rep.written {
        o.info(tf!("Created: {path}", path = f.display()));
    }
    for e in &rep.errors {
        o.error(e);
    }
    o.info(rep.summary());
    Ok(())
}

fn scan(t: &TaskArgs, st: &mut State, o: &mut Output<'_>) -> Result<(), String> {
    let p = st.load(o)?;
    let sel = sources(t, p)?;
    let rep = ops::scan_project(p, sel.as_deref()).map_err(|e| o.core(&e))?;
    st.changed = true;
    for e in &rep.errors {
        o.error(e);
    }
    o.info(rep.summary());
    Ok(())
}

fn export(t: &TaskArgs, st: &mut State, o: &mut Output<'_>) -> Result<(), String> {
    let file = PathBuf::from(first_file(t, i18n::t("Export file"))?);
    if exchange::ExchangeFormat::from_path(&file).is_none() {
        return Err(tf!("{file}: unknown format (.xlsx, .csv, .txt, .xlf, .xliff, .tmx)", file = file.display()));
    }
    let p = st.load(o)?;
    let langs = languages(t, p)?;
    let rows = row_selection(t, p, &langs, sources(t, p)?, o)?;
    let opts = ExportOptions {
        langs,
        statuses: t.value("status").map(parse_status_list).transpose()?.unwrap_or_default(),
        row_statuses: t.value("rowstatus").map(parse_row_status_list).transpose()?.unwrap_or_default(),
        rows,
        include_locked: t.value("readonly").is_some_and(|v| v.eq_ignore_ascii_case("true")),
        ..Default::default()
    };
    let files = exchange::export_file(p, &file, &opts).map_err(|e| o.core(&e))?;
    for f in files {
        o.info(tf!("Exported: {path}", path = f.display()));
    }
    Ok(())
}

fn import(t: &TaskArgs, st: &mut State, o: &mut Output<'_>) -> Result<(), String> {
    let file = PathBuf::from(first_file(t, i18n::t("Import file (importing all localized files is not supported)"))?);
    if !file.exists() {
        return Err(tf!("{path} not found", path = file.display()));
    }
    let status = match t.number("setstatus")? {
        None => TranslationStatus::Translated,
        Some(n @ 1..=5) => TranslationStatus::from_index(n).unwrap_or(TranslationStatus::Translated),
        Some(n) => return Err(tf!("Invalid value for -{name}: \"{value}\"", name = "setstatus", value = n) + " (1..5)"),
    };
    let p = st.load(o)?;
    let opts = ImportOptions {
        langs: languages(t, p)?,
        overwrite: t.number("overwrite")? != Some(0),
        status,
        check_original: !t.flag("nomatch"),
        comments: t.number("rcomment")? != Some(0),
    };
    let rep = exchange::import_file(p, &file, &opts).map_err(|e| o.core(&e))?;
    st.changed |= rep.updated > 0;
    if !rep.unknown_languages.is_empty() {
        o.warn(tf!("Languages not in the project: {langs}", langs = rep.unknown_languages.join(", ")));
    }
    if rep.not_found > 0 {
        o.warn(trn(rep.not_found, "{n} row of the import file was not found in the project", "{n} rows of the import file were not found in the project"));
    }
    o.info(rep.summary());
    Ok(())
}

fn translate(t: &TaskArgs, st: &mut State, o: &mut Output<'_>) -> Result<(), String> {
    if let Some(e) = t.value("engine").filter(|e| !e.is_empty() && !e.eq_ignore_ascii_case("tm")) {
        return Err(tf!("Translation engine '{engine}' is not supported (translation memory only: -engine:tm)", engine = e));
    }
    let path = tm_path(t);
    let memory = TranslationMemory::open(&path).map_err(|e| format!("{}: {}", path.display(), o.core(&e)))?;
    if memory.is_empty() {
        o.warn(tf!("Translation memory {path} is empty", path = path.display()));
    }
    let fuzzy = t.number("fuzzy")?.map(|n| u8::try_from(n.clamp(50, 100)).unwrap_or(100));
    let p = st.load(o)?;
    let opts = TranslateOptions { langs: languages(t, p)?, overwrite: t.flag("overwrite"), fuzzy };
    let rep = tm::translate_project(p, &memory, &opts, None);
    st.changed |= rep.exact + rep.fuzzy > 0;
    o.info(tf!("{summary} ({entries} entries in TM {path})", summary = rep.summary(), entries = memory.len(), path = path.display()));
    Ok(())
}

fn duplicate(t: &TaskArgs, st: &mut State, o: &mut Output<'_>) -> Result<(), String> {
    let p = st.load(o)?;
    let langs = languages(t, p)?;
    let mut memory = TranslationMemory::new();
    let usable = [TranslationStatus::Translated, TranslationStatus::Complete, TranslationStatus::ForReview, TranslationStatus::AutoTranslated];
    tm::save_to_tm(p, &mut memory, &langs, &usable, false);
    let rep = tm::translate_project(p, &memory, &TranslateOptions { langs, overwrite: t.flag("overwrite"), fuzzy: None }, None);
    st.changed |= rep.exact > 0;
    o.info(trn(rep.exact, "{n} duplicate string translated", "{n} duplicate strings translated"));
    Ok(())
}

fn save_to_tm(t: &TaskArgs, st: &mut State, o: &mut Output<'_>) -> Result<(), String> {
    let path = tm_path(t);
    let mut memory = TranslationMemory::open(&path).map_err(|e| format!("{}: {}", path.display(), o.core(&e)))?;
    let statuses = t.value("status").map(parse_status_list).transpose()?.unwrap_or_default();
    let p = st.load(o)?;
    let langs = languages(t, p)?;
    let n = tm::save_to_tm(p, &mut memory, &langs, &statuses, true);
    memory.save().map_err(|e| format!("{}: {}", path.display(), o.core(&e)))?;
    st.changed = true;
    let msg = trn(
        n,
        "{n} translation saved to the translation memory ({entries} entries, {path})",
        "{n} translations saved to the translation memory ({entries} entries, {path})",
    );
    o.info(i18n::fmt(&msg, &[("entries", &memory.len()), ("path", &path.display())]));
    Ok(())
}

fn validate(t: &TaskArgs, st: &mut State, o: &mut Output<'_>) -> Result<(), String> {
    if t.flag("original") {
        return Err(i18n::t("Validating original values (-original) is not supported").into());
    }
    let mut opts = ValidateOptions::default();
    if t.flag("all") {
        opts.kinds = ValidationKind::ALL.to_vec();
    }
    let checks = t.list("check");
    if !checks.is_empty() {
        opts.kinds =
            checks.iter().map(|k| ValidationKind::from_keyword(k).ok_or_else(|| tf!("Unknown check '{check}'", check = k))).collect::<Result<_, _>>()?;
    }
    let p = st.load(o)?;
    opts.langs = languages(t, p)?;
    let rows = row_selection(t, p, &opts.langs, None, o)?;
    let issues = validate::validate(p, &opts, rows.as_deref());
    let (mut errors, mut warnings) = (0, 0);
    for i in &issues {
        let msg = format!("[{}] {} {}: {} – {}", i.lang, i.source, i.context, i.kind.label(), i.message);
        match i.severity {
            Severity::Error => {
                errors += 1;
                o.error(msg);
            }
            Severity::Warning => {
                warnings += 1;
                o.warn(msg);
            }
        }
    }
    let (errors, warnings) = (trn(errors, "{n} error", "{n} errors"), trn(warnings, "{n} warning", "{n} warnings"));
    o.info(tf!("Validation finished: {errors}, {warnings}", errors = errors, warnings = warnings));
    Ok(())
}

fn fill_pseudo(t: &TaskArgs, st: &mut State, o: &mut Output<'_>) -> Result<(), String> {
    let p = st.load(o)?;
    let mut langs = t.list("lang");
    if langs.is_empty() {
        langs = p.target_languages();
    }
    let opts = Settings::load().pseudo;
    let mut changed = false;
    for l in &langs {
        if !p.has_language(l) {
            pseudo::set_pseudo_language(p, l, opts.kind);
            o.info(tf!("Pseudo language {lang} added", lang = l));
        }
        let n = pseudo::fill_pseudo(p, l, &opts, None, t.flag("overwrite"));
        changed = true;
        let msg = trn(n, "{n} pseudo translation for {name} ({lang})", "{n} pseudo translations for {name} ({lang})");
        o.info(i18n::fmt(&msg, &[("name", &lang::name(l)), ("lang", l)]));
    }
    st.changed |= changed;
    Ok(())
}

fn remove_pseudo(t: &TaskArgs, st: &mut State, o: &mut Output<'_>) -> Result<(), String> {
    let p = st.load(o)?;
    let langs = languages(t, p)?;
    let n = if langs.is_empty() { pseudo::remove_pseudo(p, None) } else { langs.iter().map(|l| pseudo::remove_pseudo(p, Some(l))).sum() };
    st.changed |= n > 0;
    o.info(trn(n, "{n} pseudo translation removed", "{n} pseudo translations removed"));
    Ok(())
}

fn remove(st: &mut State, o: &mut Output<'_>) -> Result<(), String> {
    let n = st.load(o)?.remove_unused_rows();
    st.changed |= n > 0;
    o.info(trn(n, "{n} unused row removed", "{n} unused rows removed"));
    Ok(())
}

fn report(t: &TaskArgs, st: &mut State, o: &mut Output<'_>) -> Result<(), String> {
    let title = st.path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let p = st.load(o)?;
    let rows = row_selection(t, p, &[], None, o)?;
    let s = stats::statistics(p, rows.as_deref());
    let Some(file) = t.files.first().map(PathBuf::from) else {
        let text = s.to_text();
        if !o.quiet {
            let _ = write!(o.out, "{text}");
        }
        return Ok(());
    };
    let ext = file.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    let content = match ext.as_str() {
        "html" | "htm" => s.to_html(&title),
        "json" => serde_json::to_string_pretty(&s).map_err(|e| e.to_string())?,
        _ => s.to_text(),
    };
    std::fs::write(&file, content).map_err(|e| format!("{}: {e}", file.display()))?;
    o.info(tf!("Report created: {path}", path = file.display()));
    Ok(())
}

fn info(st: &mut State, o: &mut Output<'_>) -> Result<(), String> {
    let path = st.path.clone();
    let p = st.load(o)?;
    let s = stats::statistics(p, None);
    let mut lines = vec![
        tf!("Project: {path}", path = path.display()),
        format!("{}, {}", tf!("Rows: {n}", n = s.rows), tf!("Words: {n}", n = s.words)),
        i18n::t("Languages:").to_string(),
    ];
    for l in &s.languages {
        let percent = format!("{:>6.1}", l.percent());
        lines.push(format!("  {:<8} {:<28} {}", l.lang, l.name, tf!("{percent}% translated", percent = percent)));
    }
    lines.push(i18n::t("Sources:").into());
    for (i, src) in p.sources.iter().enumerate() {
        let (dir, file) = src.output_pattern();
        let output = format!("{dir}\\{file}");
        let details = tf!("Original: {lang}, {rows} rows, output: {output}", lang = src.original(), rows = src.row_count(), output = output);
        lines.push(format!("  #{i} {} [{}] {details}", src.name(), src.kind().label()));
    }
    for l in lines {
        o.info(l);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::wildcard_match;

    #[test]
    fn wildcards() {
        assert!(wildcard_match("Resources.resx", "*.resx"));
        assert!(wildcard_match("Strings.de.RESX", "strings.*.resx"));
        assert!(wildcard_match("a1.json", "a?.json"));
        assert!(!wildcard_match("a.json", "*.resx"));
        assert!(!wildcard_match("ab.json", "a?c.json"));
    }
}
