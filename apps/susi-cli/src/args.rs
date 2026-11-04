//! SlMake compatible command line parsing.
//!
//! Syntax: `susi-cli <task> [file] [options] [<task> ...] project.slp [-q] [-a] [-w] [-e] [-h]`
//!
//! Options are accepted in SlMake form (`-lang:de;fr`, `-overwrite:3`) and in GNU form
//! (`--lang=de,fr`). Option names are case-insensitive and may be abbreviated as long as the
//! abbreviation is unique for the task (SlMake's own help uses `-overwri:3`).

use std::path::PathBuf;

use susi_core::i18n::{Lang, t};
use susi_core::tf;

/// All tasks known to susi (SlMake tasks plus susi extensions).
pub const TASKS: &[&str] = &[
    "add",
    "create",
    "duplicate",
    "export",
    "fillpseudo",
    "import",
    "remove",
    "removepseudo",
    "report",
    "scan",
    "translate",
    "validate",
    "savetotm",
    "info",
    "mcp",
];

/// SlMake tasks susi does not implement.
pub const UNSUPPORTED: &[&str] = &["exchange", "merge", "sanitize"];

/// Hint for an [`UNSUPPORTED`] task, in the current language.
pub fn unsupported_hint(task: &str) -> &'static str {
    match task {
        "exchange" => t("Sisulizer exchange packages (.sle) are not supported – use 'export' (xlsx/xlf/tmx)."),
        "merge" => t("Merging projects is not supported yet."),
        _ => t("'sanitize' is not supported."),
    }
}

const GLOBAL_FLAGS: &[&str] = &["q", "a", "w", "e", "h", "nosave", "forcesave", "help"];

/// Global option selecting the language of help and messages (`-uilang:de`).
pub const UI_LANG_OPTION: &str = "uilang";

/// Options understood per task (used for abbreviation and validation).
pub fn task_options(task: &str) -> &'static [&'static str] {
    const FILTER: &[&str] = &["lang", "include", "exclude"];
    match task {
        "add" => &["lang", "origlang", "type", "outitem", "noimport", "subdirs"],
        "create" => &["lang", "include", "exclude", "output", "config", "input", "al"],
        "duplicate" => &["lang", "overwrite"],
        "export" => &[
            "lang",
            "status",
            "rowstatus",
            "readonly",
            "include",
            "exclude",
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
            "since",
            "until",
            "datetype",
        ],
        "import" => &[
            "lang",
            "overwrite",
            "setstatus",
            "status",
            "nomatch",
            "rcomment",
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
        "fillpseudo" | "removepseudo" => &["lang", "overwrite"],
        "report" => &["items", "lang", "since", "until", "datetype"],
        "scan" | "remove" => FILTER,
        "translate" => &["lang", "engine", "first", "tm", "fuzzy", "overwrite"],
        "validate" => &["lang", "original", "all", "default", "name", "check", "since", "until", "datetype"],
        "savetotm" => &["lang", "tm", "status"],
        "mcp" => &["tm"],
        _ => &[],
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct TaskArgs {
    pub name: String,
    /// Positional arguments (e.g. the export/import file).
    pub files: Vec<String>,
    /// Options with canonical (unabbreviated, lower case) names.
    pub opts: Vec<(String, Option<String>)>,
}

impl TaskArgs {
    pub fn flag(&self, name: &str) -> bool {
        self.opts.iter().any(|(k, _)| k == name)
    }

    pub fn value(&self, name: &str) -> Option<&str> {
        self.opts.iter().rev().find(|(k, _)| k == name).and_then(|(_, v)| v.as_deref())
    }

    /// `;`/`,` separated list option (e.g. `-lang:de;fr`).
    pub fn list(&self, name: &str) -> Vec<String> {
        self.value(name).map(split_list).unwrap_or_default()
    }

    pub fn number(&self, name: &str) -> Result<Option<u32>, String> {
        self.value(name).map(|v| v.trim().parse::<u32>().map_err(|_| tf!("Invalid value for -{name}: \"{value}\"", name = name, value = v))).transpose()
    }
}

pub fn split_list(v: &str) -> Vec<String> {
    v.split([';', ',']).map(|s| s.trim().trim_matches('"').to_string()).filter(|s| !s.is_empty()).collect()
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Invocation {
    pub tasks: Vec<TaskArgs>,
    pub project: Option<PathBuf>,
    pub quiet: bool,
    pub ansi: bool,
    pub warning_codes: bool,
    pub detailed: bool,
    pub help: bool,
    pub nosave: bool,
    pub forcesave: bool,
    /// `-uilang:<auto|en|de>`.
    pub ui_lang: Option<String>,
}

/// Value of the last `-uilang` option, found without parsing the rest (so parse errors can be localized).
pub fn ui_lang_arg(args: &[String]) -> Option<String> {
    args.iter().filter_map(|a| split_option(a)).filter(|(k, _)| k == UI_LANG_OPTION).filter_map(|(_, v)| v).next_back()
}

fn valid_ui_lang(v: &str) -> bool {
    v.trim().eq_ignore_ascii_case("auto") || Lang::from_code(v).is_some()
}

fn canonical_task(s: &str) -> Option<&'static str> {
    let l = s.to_ascii_lowercase();
    match l.as_str() {
        "build" | "make" => Some("create"),
        "stats" | "statistics" => Some("report"),
        "pseudo" => Some("fillpseudo"),
        _ => TASKS.iter().copied().find(|t| *t == l),
    }
}

/// Splits `-name:value`, `--name=value`, `/name:value` style options. Returns `None` for positionals.
fn split_option(arg: &str) -> Option<(String, Option<String>)> {
    let body = arg.strip_prefix("--").or_else(|| arg.strip_prefix('-'))?;
    if body.is_empty() {
        return None;
    }
    let (k, v) = match body.find([':', '=']) {
        Some(i) => (&body[..i], Some(body[i + 1..].trim_matches('"').to_string())),
        None => (body, None),
    };
    Some((k.to_ascii_lowercase(), v))
}

fn resolve_option(task: &str, key: &str) -> Result<String, String> {
    let known = task_options(task);
    if known.contains(&key) {
        return Ok(key.to_string());
    }
    let candidates: Vec<&&str> = known.iter().filter(|o| o.starts_with(key)).collect();
    match candidates.as_slice() {
        [one] => Ok((**one).to_string()),
        [] => Err(tf!("Unknown option -{option} for task '{task}' (see: susi-cli {task} -h)", option = key, task = task)),
        _ => Err(tf!("Ambiguous option -{option} for task '{task}'", option = key, task = task)),
    }
}

pub fn parse(args: &[String]) -> Result<Invocation, String> {
    let mut inv = Invocation::default();
    let mut leading: Vec<String> = Vec::new();
    for arg in args {
        if let Some(t) = canonical_task(arg) {
            inv.tasks.push(TaskArgs { name: t.to_string(), ..Default::default() });
            continue;
        }
        if let Some(task) = UNSUPPORTED.iter().find(|t| t.eq_ignore_ascii_case(arg)) {
            return Err(format!("Task '{arg}': {}", unsupported_hint(task)));
        }
        match split_option(arg) {
            Some((k, v)) if k == UI_LANG_OPTION => match v {
                Some(v) if valid_ui_lang(&v) => inv.ui_lang = Some(v),
                v => return Err(tf!("Invalid value for -{name}: \"{value}\"", name = k, value = v.unwrap_or_default()) + " (auto, en, de)"),
            },
            Some((k, v)) if GLOBAL_FLAGS.contains(&k.as_str()) && v.is_none() => match k.as_str() {
                "q" => inv.quiet = true,
                "a" => inv.ansi = true,
                "w" => inv.warning_codes = true,
                "e" => inv.detailed = true,
                "nosave" => inv.nosave = true,
                "forcesave" => inv.forcesave = true,
                _ => inv.help = true,
            },
            Some((k, v)) => {
                let Some(task) = inv.tasks.last_mut() else {
                    return Err(tf!("Option -{option} before the first task", option = k));
                };
                let name = resolve_option(&task.name, &k)?;
                task.opts.push((name, v));
            }
            None => match inv.tasks.last_mut() {
                Some(t) => t.files.push(arg.clone()),
                None => leading.push(arg.clone()),
            },
        }
    }
    // The project is the last positional argument (SlMake: `<tasks> project`).
    let last_owner = inv.tasks.iter_mut().rev().find(|t| !t.files.is_empty());
    let project = match last_owner {
        Some(t) => {
            let only_mcp_arg = t.name == "mcp" && !t.files.last().is_some_and(|f| f.to_ascii_lowercase().ends_with(".slp"));
            if only_mcp_arg { None } else { t.files.pop() }
        }
        None => leading.pop(),
    };
    if let Some(extra) = leading.first() {
        return Err(tf!("Unknown task '{task}' (see: susi-cli -h)", task = extra));
    }
    inv.project = project.map(PathBuf::from);
    if inv.tasks.is_empty() && inv.project.is_some() {
        inv.tasks.push(TaskArgs { name: "create".into(), ..Default::default() });
    }
    Ok(inv)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> Invocation {
        parse(&s.split_whitespace().map(String::from).collect::<Vec<_>>()).unwrap()
    }

    #[test]
    fn slmake_syntax() {
        let i = p("import Translated.xlsx -lang:de -overwri:3 Sample.slp -w");
        assert_eq!(i.project, Some(PathBuf::from("Sample.slp")));
        assert!(i.warning_codes);
        let t = &i.tasks[0];
        assert_eq!(t.name, "import");
        assert_eq!(t.files, vec!["Translated.xlsx"]);
        assert_eq!(t.value("overwrite"), Some("3"));
        assert_eq!(t.list("lang"), vec!["de"]);
    }

    #[test]
    fn gnu_syntax_and_multiple_tasks() {
        let i = p("scan build --lang=de,fr app.slp");
        assert_eq!(i.tasks.len(), 2);
        assert_eq!(i.tasks[1].name, "create");
        assert_eq!(i.tasks[1].list("lang"), vec!["de", "fr"]);
        assert_eq!(i.project, Some(PathBuf::from("app.slp")));
    }

    #[test]
    fn defaults_and_errors() {
        let i = p("Sample.slp");
        assert_eq!(i.tasks[0].name, "create");
        let i = p("mcp");
        assert!(i.project.is_none());
        assert!(parse(&["export".into(), "-bogus".into(), "a.slp".into()]).is_err());
        assert!(parse(&["exchange".into(), "a.slp".into()]).is_err());
        assert!(parse(&["export".into(), "-s:1".into(), "a.slp".into()]).is_err(), "ambiguous abbreviation");
        assert_eq!(p("validate -orig a.slp").tasks[0].opts[0].0, "original");
    }

    #[test]
    fn ui_lang_option() {
        let i = p("-uilang:de info a.slp");
        assert_eq!((i.ui_lang.as_deref(), i.project), (Some("de"), Some(PathBuf::from("a.slp"))));
        assert_eq!(p("info --uilang=EN a.slp").ui_lang.as_deref(), Some("EN"));
        let args: Vec<String> = ["-uilang:auto", "bogus", "-uilang:de"].map(String::from).into();
        assert_eq!(ui_lang_arg(&args).as_deref(), Some("de"), "found despite the parse error");
        assert!(parse(&["info".into(), "-uilang:xx".into(), "a.slp".into()]).is_err());
        assert!(parse(&["info".into(), "-uilang".into(), "a.slp".into()]).is_err());
    }
}
