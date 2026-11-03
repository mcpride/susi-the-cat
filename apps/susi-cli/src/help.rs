//! Help texts (modeled on `SlMake -h` / `SlMake <task> -h`).
//!
//! Layout, syntax lines, option names, keywords and examples stay in code; descriptions are translated.

use susi_core::i18n::t;
use susi_core::tf;

/// Column of option/task descriptions.
const INDENT: usize = 14;

/// One task or option line: name padded to the description column, continuation lines indented.
fn item(name: &str, desc: &str) -> String {
    let mut s = String::new();
    for (i, line) in desc.lines().enumerate() {
        if i == 0 {
            s += &format!(" {name:<w$} {line}\n", w = INDENT - 2);
        } else {
            s += &format!("{:INDENT$}{line}\n", "");
        }
    }
    s
}

/// Task line of a susi extension (not in SlMake), marked with "(susi)".
fn susi_item(name: &str, desc: &str) -> String {
    item(name, &format!("{desc:<46} (susi)"))
}

pub fn general(version: &str) -> String {
    let tasks = [
        ("add", t("Add a source (creates the project if needed)")),
        ("create", t("Create localized files (alias: build, make)")),
        ("duplicate", t("Translate duplicate strings")),
        ("export", t("Export data to a file (.xlsx, .csv, .txt, .xlf, .tmx)")),
        ("fillpseudo", t("Insert pseudo translation")),
        ("import", t("Import translations from a file")),
        ("remove", t("Remove unused items")),
        ("removepseudo", t("Remove pseudo translation")),
        ("report", t("Create a project report (.html, .txt, .json)")),
        ("scan", t("Scan the project")),
        ("translate", t("Translate with the translation memory")),
        ("validate", t("Validate translations")),
    ];
    let susi_tasks = [
        ("savetotm", t("Save translations to the translation memory")),
        ("info", t("Show languages, sources and progress")),
        ("mcp", t("Start the MCP server (stdio) for AI agents")),
    ];
    let options = [
        ("-q", t("Quiet mode. Only errors are shown.")),
        ("-a", t("ANSI mode (accepted; susi always writes UTF-8).")),
        ("-w", t("Result code 2 on errors, 1 on warnings, otherwise 0.\nWithout -w: 1 on errors, otherwise 0.")),
        ("-e", t("Show detailed error information.")),
        ("-h", t("Show help for a task, e.g. susi-cli export -h")),
        ("-nosave", t("Do not save the project file, even if it was changed.")),
        ("-forcesave", t("Save the project file even after an error.")),
        (
            "-uilang:X",
            t(
                "Language of help and messages: en, de or auto (SUSI_LANG or system language).\nDefault: environment variable SUSI_LANG, then the language setting of\nthe Susi GUI, then the system language.",
            ),
        ),
    ];
    let mut s = tf!("susi-cli {version} – Susi the CAT (SlMake compatible)", version = version);
    s += "\n\nSyntax: susi-cli <tasks> project.slp [-q] [-a] [-w] [-e] [-h] [-uilang:X]\n\n";
    s += t("<tasks> contains one or more tasks with their options:");
    s += "\n";
    s.extend(tasks.iter().map(|(n, d)| item(n, d)));
    s.extend(susi_tasks.iter().map(|(n, d)| susi_item(n, d)));
    s += t("Without a task, 'create' is run.");
    s += "\n\n";
    s += t("Options:");
    s += "\n";
    s.extend(options.iter().map(|(n, d)| item(n, d)));
    s += "\n";
    s += t("Options can also be given in the form --lang=de,fr.");
    s += "\n";
    s
}

fn lang_opt() -> String {
    item("-lang:X", t("Language codes separated by ; (e.g. de;fr-CA). Default: all languages."))
}

fn status_opt() -> String {
    item("-status:S", &format!("{}\nuntranslated, best, auto, translated, review, complete", t("Translation statuses, separated by ;:")))
}

fn filter_opts() -> String {
    item("-include:\"S\"", t("Only these sources (names, separated by ;).")) + &item("-exclude:\"S\"", t("Skip these sources."))
}

fn overwrite_opt() -> String {
    item("-overwrite", t("Replace existing translations."))
}

/// Help text: syntax line, description paragraph, option lines and optional examples.
fn page(syntax: &str, desc: &str, opts: &str, examples: &[&str]) -> String {
    let mut s = format!("Syntax: {syntax}\n{desc}\n");
    if !opts.is_empty() {
        s += "\n";
        s += opts;
    }
    if !examples.is_empty() {
        s += "\n";
        s += if examples.len() == 1 { t("Example:") } else { t("Examples:") };
        s += "\n";
        for e in examples {
            s += &format!("  {e}\n");
        }
    }
    s
}

pub fn task(name: &str) -> String {
    match name {
        "add" => page(
            "susi-cli add file [options] project.slp",
            t("Adds sources (.json, .resx, .po/.pot, .tsv; wildcards * and ? allowed) and scans them.\nIf the project does not exist, it is created."),
            &(lang_opt()
                + &item("-origlang:X", t("Original language of the source (default: en)."))
                + &item("-type:X", t("json, resx, po or tsv (default: by file extension)."))),
            &[
                "susi-cli add src\\i18n\\en.json -origlang:en -lang:de;fr app.slp",
                "susi-cli add *.resx app.slp",
                "susi-cli add locale\\messages.pot -lang:de;fr app.slp",
                "susi-cli add i18n\\de-gui.tsv -origlang:en -lang:de;fr app.slp",
            ],
        ),
        "create" => page(
            "susi-cli create [options] project.slp",
            t("Creates the localized files (Project | Build)."),
            &(lang_opt() + &filter_opts()),
            &["susi-cli create -lang:de;ja app.slp"],
        ),
        "duplicate" => page(
            "susi-cli duplicate [options] project.slp",
            t("Translates duplicate strings with existing translations of the same original."),
            &(lang_opt() + &overwrite_opt()),
            &[],
        ),
        "export" => page(
            "susi-cli export file [options] project.slp",
            t(
                "Exports rows to a file: .xlsx (Excel), .csv, .txt (tab separated), .xlf/.xliff, .tmx.\nXLIFF with several languages creates one file per language (name.<language>.xlf).",
            ),
            &(lang_opt()
                + &status_opt()
                + &item("-rowstatus:S", &format!("{} new, unused, inuse, changed", t("Row status:")))
                + &item("-readonly:S", t("true = also export \"Do not translate\" rows"))
                + &filter_opts()),
            &["susi-cli export Translation.xlsx -lang:fi;de app.slp", "susi-cli export de.xlf -lang:de -status:untranslated;review app.slp"],
        ),
        "import" => page(
            "susi-cli import file [options] project.slp",
            t("Imports translations from .xlsx, .csv, .txt, .xlf/.xliff or .tmx.\nRows are matched by source+context, then context, then original text."),
            &(lang_opt()
                + &item("-overwrite:X", t("Overwrite existing translations: 0 = never, 1..3 = yes (default)"))
                + &item(
                    "-setstatus:X",
                    t(
                        "Status of imported texts without a status column:\n1 = Best guess, 2 = Auto translated, 3 = Translated (default),\n4 = For review, 5 = Complete",
                    ),
                )
                + &item("-nomatch", t("Import even if the original has changed."))
                + &item("-rcomment:0", t("Do not import comments."))),
            &["susi-cli import Translation.xlsx -lang:de -overwrite:3 app.slp"],
        ),
        "fillpseudo" => page(
            "susi-cli fillpseudo [options] project.slp",
            t("Fills empty cells with pseudo translations (Column | Pseudo translation).\nUnknown languages are added as pseudo languages."),
            &(lang_opt() + &item("-overwrite", t("Also replace existing translations."))),
            &[],
        ),
        "removepseudo" => page("susi-cli removepseudo [options] project.slp", t("Removes pseudo translations."), &lang_opt(), &[]),
        "remove" => page("susi-cli remove project.slp", t("Removes unused rows (Project | Remove unused items)."), "", &[]),
        "report" => page(
            "susi-cli report [file] project.slp",
            t("Creates a project report. Format by extension: .html, .txt, .json; without a file: output to stdout."),
            "",
            &[],
        ),
        "scan" => page("susi-cli scan [options] project.slp", t("Scans the sources (new, changed and unused strings)."), &filter_opts(), &[]),
        "translate" => page(
            "susi-cli translate [options] project.slp",
            t("Translates untranslated rows from the translation memory (TMX).\nExact matches get the status \"Auto translated\", fuzzy ones \"Best guess\"."),
            &(lang_opt()
                + &item("-tm:file", t("TMX file (default: settings, %APPDATA%\\susi\\susi-tm.tmx)"))
                + &item("-fuzzy:N", t("Use fuzzy matches from N % (50..100)."))
                + &overwrite_opt()
                + &item("-engine:S", t("Only \"tm\" is supported."))),
            &[],
        ),
        "validate" => page(
            "susi-cli validate [options] project.slp",
            t(
                "Validates translations (placeholders, tags, spaces, line breaks, punctuation, length, ...).\nErrors and warnings determine the result code (see -w).",
            ),
            &(lang_opt()
                + &item("-all", t("All checks (including missing translations)."))
                + &item("-default", t("Default checks."))
                + &item(
                    "-check:S",
                    &format!(
                        "{}\nmissing, format, tags, whitespace, newline, period, colon,\nexclamation, question, length, allspaces, hotkey, duplicate",
                        t("Specific checks:")
                    ),
                )),
            &[],
        ),
        "savetotm" => page(
            "susi-cli savetotm [options] project.slp",
            t("Saves translations to the translation memory."),
            &(lang_opt() + &item("-tm:file", t("TMX file")) + &status_opt() + &format!("{:INDENT$}{} translated;complete\n", "", t("Default:"))),
            &[],
        ),
        "info" => page("susi-cli info project.slp", t("Shows languages, sources and the translation progress."), "", &[]),
        "mcp" => {
            let mut s =
                page("susi-cli mcp [project.slp] [-tm:file]", t("Starts the MCP server (Model Context Protocol) on stdin/stdout for AI agents."), "", &[]);
            s += t("Example configuration (mcp.json):");
            s += "\n  {\"servers\": {\"susi\": {\"command\": \"susi-cli\", \"args\": [\"mcp\"]}}}\n";
            s
        }
        _ => tf!("Unknown task '{name}'.", name = name) + "\n",
    }
}
