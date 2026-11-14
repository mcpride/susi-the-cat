//! Susi command line tool, compatible with Sisulizer's SlMake.
//!
//! ```text
//! susi-cli scan create -lang:de;fr app.slp -w
//! susi-cli export Uebersetzung.xlsx -lang:de -status:untranslated app.slp
//! susi-cli import Uebersetzung.xlsx -lang:de app.slp
//! susi-cli mcp app.slp
//! susi-cli workflow save publish app.slp
//! ```

pub mod args;
pub mod help;
pub mod tasks;
pub mod workflow;

use std::io::Write;

use susi_core::i18n::{self, Lang, t};
use susi_core::settings::Settings;
use susi_core::{slp, tf};

use crate::args::Invocation;
use crate::tasks::{Output, State};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Runs the CLI with the process' stdout/stderr and returns the exit code.
pub fn run(args: &[String]) -> i32 {
    // No long-lived stdout lock: the MCP server writes stdout from tokio's blocking threads.
    run_with(args, &mut std::io::stdout(), &mut std::io::stderr())
}

/// Result code per SlMake: with `-w` 2 = errors, 1 = warnings; otherwise 1 = errors.
pub fn exit_code(inv: &Invocation, errors: usize, warnings: usize) -> i32 {
    match (inv.warning_codes, errors > 0, warnings > 0) {
        (true, true, _) => 2,
        (true, false, true) => 1,
        (false, true, _) => 1,
        _ => 0,
    }
}

/// Language of help and messages: `-uilang` option > `SUSI_LANG` > GUI setting (`settings`, loaded lazily) > system.
///
/// An explicit `auto` (option or setting) means the system language, which itself honours `SUSI_LANG`.
pub fn resolve_lang(option: Option<&str>, env: Option<&str>, settings: impl FnOnce() -> String) -> Lang {
    let explicit = |v: &str| !v.trim().is_empty() && !v.trim().eq_ignore_ascii_case("auto");
    match (option, env) {
        (Some(o), _) => Lang::from_pref(&o.trim().to_ascii_lowercase()),
        (None, Some(e)) if explicit(e) => Lang::from_pref(e),
        _ => Lang::from_pref(&settings()),
    }
}

/// Language of the task output: MCP results are read by AI agents and always English.
pub fn output_lang(inv: &Invocation, ui: Lang) -> Lang {
    if inv.tasks.iter().any(|t| t.name == "mcp") { Lang::EN } else { ui }
}

/// Selects the language for an invocation from its arguments, the environment and the settings.
fn init_lang(args: &[String]) {
    let env = std::env::var(i18n::ENV_LANG).ok();
    i18n::set_current(resolve_lang(args::ui_lang_arg(args).as_deref(), env.as_deref(), || Settings::load().ui_language));
}

fn print_error(err: &mut dyn Write, msg: impl std::fmt::Display) {
    let _ = writeln!(err, "{} {msg}", t("Error:"));
}

pub fn run_with(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    init_lang(args);
    let inv = match args::parse(args) {
        Ok(i) => i,
        Err(e) => {
            print_error(err, e);
            return 1;
        }
    };
    if inv.help || inv.tasks.is_empty() {
        let text = match inv.tasks.first() {
            Some(t) => help::task(&t.name),
            None => help::general(VERSION),
        };
        let _ = write!(out, "{text}");
        return 0;
    }
    if inv.tasks.iter().any(|t| t.name == "mcp") {
        return run_mcp(&inv, err);
    }
    let Some(path) = inv.project.clone() else {
        print_error(err, t("Project file (.slp) missing (see: susi-cli -h)"));
        return 1;
    };
    let mut o = Output { out, err, quiet: inv.quiet, detailed: inv.detailed, errors: 0, warnings: 0 };
    o.info(format!("susi-cli {VERSION}"));
    let mut st = State::new(path);
    for t in &inv.tasks {
        if let Err(e) = tasks::run_task(t, &mut st, &mut o) {
            o.error(e);
            break;
        }
    }
    save(&inv, &mut st, &mut o);
    exit_code(&inv, o.errors, o.warnings)
}

fn save(inv: &Invocation, st: &mut State, o: &mut Output<'_>) {
    let Some(p) = st.project.as_mut().filter(|_| st.changed) else {
        return;
    };
    if inv.nosave && !inv.forcesave {
        o.info(t("Project was changed but not saved (-nosave)"));
        return;
    }
    if o.errors > 0 && !inv.forcesave {
        o.info(t("Project was not saved because of errors (see -forcesave)"));
        return;
    }
    p.touch();
    match slp::save(p, &st.path) {
        Ok(()) => o.info(tf!("Project saved: {path}", path = st.path.display())),
        Err(e) => o.error(format!("{}: {e}", st.path.display())),
    }
}

/// `mcp` task: serves MCP on stdio. Nothing else may be written to stdout.
fn run_mcp(inv: &Invocation, err: &mut dyn Write) -> i32 {
    if inv.tasks.len() > 1 {
        print_error(err, t("'mcp' cannot be combined with other tasks"));
        return 1;
    }
    i18n::set_current(output_lang(inv, i18n::current()));
    let tm = inv.tasks.first().and_then(|t| t.value("tm")).map(std::path::PathBuf::from);
    let rt = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
        Ok(rt) => rt,
        Err(e) => {
            print_error(err, e);
            return 1;
        }
    };
    match rt.block_on(susi_mcp::serve_stdio(inv.project.as_deref(), tm.as_deref())) {
        Ok(()) => 0,
        Err(e) => {
            print_error(err, e);
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(v: &'static str) -> impl FnOnce() -> String {
        move || v.to_string()
    }

    #[test]
    fn language_precedence() {
        assert_eq!(resolve_lang(Some("de"), Some("en"), settings("en")), Lang::DE);
        assert_eq!(resolve_lang(Some("EN"), Some("de"), || unreachable!("settings not needed")), Lang::EN);
        assert_eq!(resolve_lang(None, Some("de"), settings("en")), Lang::DE);
        assert_eq!(resolve_lang(None, Some("en-US"), settings("de")), Lang::EN);
        assert_eq!(resolve_lang(None, Some("auto"), settings("de")), Lang::DE);
        assert_eq!(resolve_lang(None, Some(" "), settings("de")), Lang::DE);
        assert_eq!(resolve_lang(None, None, settings("de")), Lang::DE);
        assert_eq!(resolve_lang(None, None, settings("en")), Lang::EN);
    }

    #[test]
    fn mcp_output_is_english() {
        let parse = |a: &[&str]| args::parse(&a.iter().map(|s| s.to_string()).collect::<Vec<_>>()).unwrap();
        assert_eq!(output_lang(&parse(&["mcp", "-uilang:de"]), Lang::DE), Lang::EN);
        assert_eq!(output_lang(&parse(&["mcp", "app.slp", "-tm:x.tmx"]), Lang::DE), Lang::EN);
        assert_eq!(output_lang(&parse(&["info", "app.slp", "-uilang:de"]), Lang::DE), Lang::DE);
    }
}
