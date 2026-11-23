//! Context help: `F1` opens the page of the user guide (`docs/user`) for the open dialog, the tab
//! of the output area or the panel under the mouse pointer, in the user interface language.
//!
//! The guide of this version, shipped next to the executable (`help/<lang>/`, see
//! `cargo xtask dist`), is preferred; without it the online help of the latest release opens.

use std::path::{Path, PathBuf};

use crate::app::{BottomTab, SusiApp};
use crate::dialogs::Dialog;

/// Online help (GitHub Pages, built from the latest release).
pub const ONLINE: &str = "https://mcpride.github.io/susi-the-cat/";

/// Languages of the user guide; other user interface languages get English.
const LANGUAGES: [&str; 2] = ["en", "de"];

/// Part of the main window under the mouse pointer, updated by `panels::show` every frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Area {
    /// Menu, toolbar, status bar.
    #[default]
    Window,
    Tree,
    Sheet,
    Filter,
    Output,
}

/// Every page `topic` can return: file names of `docs/user/src` without `.md` (checked by the tests).
#[cfg(test)]
pub const TOPICS: [&str; 23] = [
    "index",
    "exchange",
    "options",
    "git-workflow",
    "main-window",
    "project-tree",
    "sheet",
    "filter",
    "translating",
    "text-editor",
    "copy-and-clear",
    "translation-memory",
    "validation",
    "search",
    "comments",
    "statistics",
    "output",
    "screenshots",
    "pictures",
    "projects",
    "languages",
    "sources",
    "pseudo-translation",
];

/// The page for the current context: the open dialog, otherwise the area under the mouse pointer.
pub fn topic(app: &SusiApp) -> &'static str {
    if let Some(d) = &app.dialog {
        return dialog_topic(d);
    }
    match app.help_area {
        Area::Window => "main-window",
        Area::Tree => "project-tree",
        Area::Sheet => "sheet",
        Area::Filter => "filter",
        Area::Output => tab_topic(app),
    }
}

fn dialog_topic(d: &Dialog) -> &'static str {
    match d {
        Dialog::Confirm { .. } => "projects",
        Dialog::Languages { .. } => "languages",
        Dialog::AddSource { .. } | Dialog::SourceProps(_) | Dialog::RemoveSource(_) => "sources",
        Dialog::RowProps(_) => "translating",
        Dialog::Export(_) | Dialog::Import(_) => "exchange",
        Dialog::TmTranslate(_) => "translation-memory",
        Dialog::Pseudo(_) => "pseudo-translation",
        Dialog::Copy(_) | Dialog::ClearColumn(_) => "copy-and-clear",
        Dialog::Settings(_) => "options",
        Dialog::About => "index",
        Dialog::WorkflowFetch(_) | Dialog::WorkflowSave(_) | Dialog::WorkflowPublish(_) | Dialog::WorkflowToken(_) => "git-workflow",
        Dialog::Screenshots => "screenshots",
    }
}

fn tab_topic(app: &SusiApp) -> &'static str {
    match app.tab {
        BottomTab::Output => "output",
        BottomTab::Validation => "validation",
        BottomTab::Search => "search",
        BottomTab::Statistics => "statistics",
        BottomTab::Comments => "comments",
        BottomTab::Editor if app.selected.as_ref().and_then(|r| app.project.as_ref()?.row(r)).is_some_and(susi_core::Row::is_data) => "pictures",
        BottomTab::Editor => "text-editor",
        BottomTab::Screenshots => "screenshots",
    }
}

/// URL of `page` in the help of the user interface language `lang`: the local guide if it is
/// there, otherwise the online help.
pub fn url(lang: &str, page: &str) -> String {
    let lang = LANGUAGES.into_iter().find(|l| *l == lang).unwrap_or(LANGUAGES[0]);
    let exe_dir = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf));
    exe_dir
        .iter()
        .flat_map(|d| local_dirs(d))
        .map(|d| d.join(lang).join(format!("{page}.html")))
        .find(|p| p.is_file())
        .map_or_else(|| format!("{ONLINE}{lang}/{page}.html"), |p| file_url(&p))
}

/// Folders that may hold the guide: `help` next to the executable (release archive) and, for a
/// development build in `target/debug`, `target/help` (`cargo xtask help`).
fn local_dirs(exe_dir: &Path) -> Vec<PathBuf> {
    let mut dirs = vec![exe_dir.join("help")];
    dirs.extend(exe_dir.parent().map(|p| p.join("help")));
    dirs
}

/// `file://` URL of an absolute path, percent-encoded (spaces, umlauts); UNC paths as
/// `file://server/share/…`.
pub fn file_url(path: &Path) -> String {
    let path = path.to_string_lossy().replace('\\', "/");
    let prefix = if path.starts_with("//") {
        "file:"
    } else if path.starts_with('/') {
        "file://"
    } else {
        "file:///"
    };
    let mut url = String::from(prefix);
    for b in path.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~/:".contains(&b) {
            url.push(char::from(b));
        } else {
            url.push_str(&format!("%{b:02X}"));
        }
    }
    url
}

/// Opens `page` in the web browser and notes the address in the output.
pub fn open(app: &mut SusiApp, ctx: &egui::Context, page: &str) {
    let url = url(app.ui_lang.code(), page);
    app.info(susi_core::tf!("Help opened: {url}", url = url));
    ctx.open_url(egui::OpenUrl::new_tab(url));
}

#[cfg(test)]
mod tests {
    use super::{TOPICS, file_url};
    use std::path::Path;

    fn guide() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/user")
    }

    /// `F1` never opens a page that the user guide does not have.
    #[test]
    fn every_topic_is_a_page_of_the_guide() {
        let summary = std::fs::read_to_string(guide().join("src/SUMMARY.md")).unwrap();
        for page in TOPICS {
            assert!(guide().join(format!("src/{page}.md")).is_file(), "docs/user/src/{page}.md is missing");
            assert!(summary.contains(&format!("({page}.md)")), "{page}.md is not in SUMMARY.md");
        }
    }

    #[test]
    fn file_urls_are_encoded() {
        assert_eq!(file_url(Path::new(r"C:\Program Files\Susi\help\de\index.html")), "file:///C:/Program%20Files/Susi/help/de/index.html");
        assert_eq!(file_url(Path::new("/opt/susi/help/en/sheet.html")), "file:///opt/susi/help/en/sheet.html");
        assert_eq!(file_url(Path::new(r"\\server\share\Übersetzung\help\de\faq.html")), "file://server/share/%C3%9Cbersetzung/help/de/faq.html");
    }

    /// The German guide is maintained with Susi: `docs/user/susi-help.slp` must be up to date with
    /// `po/messages.pot` (extracted by `cargo xtask help`) and building it must reproduce `po/de.po`.
    #[test]
    fn translations_match_the_susi_project() {
        let hint = "update the project with `susi-cli scan docs/user/susi-help.slp`, translate and build it with Susi (docs/dev/user-guide.md)";
        let tmp = std::env::temp_dir().join(format!("susi-help-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("po")).unwrap();
        std::fs::copy(guide().join("susi-help.slp"), tmp.join("susi-help.slp")).unwrap();
        for e in std::fs::read_dir(guide().join("po")).unwrap().flatten() {
            std::fs::copy(e.path(), tmp.join("po").join(e.file_name())).unwrap();
        }
        let mut p = susi_core::slp::load(&tmp.join("susi-help.slp")).unwrap();
        let rep = susi_core::ops::scan_project(&mut p, None).unwrap();
        assert!(rep.errors.is_empty(), "{:?}", rep.errors);
        assert_eq!((rep.new, rep.changed, rep.unused), (0, 0, 0), "po/messages.pot differs from susi-help.slp; {hint}");
        let b = susi_core::ops::build_project(&p, &[], None);
        assert!(b.errors.is_empty(), "{:?}", b.errors);
        assert!(b.written.is_empty(), "building susi-help.slp would change {:?}; {hint}", b.written);
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
