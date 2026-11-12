//! Files belonging to a translation project, as paths relative to the working copy root.

use std::path::{Component, Path, PathBuf};

use susi_core::Project;
use susi_core::ops;

/// Absolute paths of the project file, the original source files and all localized files.
pub fn project_files(p: &Project) -> Vec<PathBuf> {
    let mut files = Vec::new();
    if let Some(path) = &p.path {
        files.push(path.clone());
    }
    let langs = p.language_ids();
    for si in 0..p.sources.len() {
        for f in ops::source_files(p, si).unwrap_or_default() {
            files.extend(langs.iter().filter_map(|l| ops::output_path(p, si, &f, l)));
            files.push(f);
        }
    }
    files.into_iter().map(|f| std::path::absolute(&f).unwrap_or(f)).collect()
}

fn same(a: &Component, b: &Component) -> bool {
    if cfg!(windows) { a.as_os_str().eq_ignore_ascii_case(b.as_os_str()) } else { a == b }
}

/// `path` relative to `base` (both absolute), lexically; `None` on different drives.
pub fn relative(path: &Path, base: &Path) -> Option<String> {
    let (path, base) = (ops::normalize(path), ops::normalize(base));
    let p: Vec<Component> = path.components().collect();
    let b: Vec<Component> = base.components().collect();
    if p.first().zip(b.first()).is_none_or(|(x, y)| !same(x, y)) {
        return None;
    }
    let common = p.iter().zip(&b).take_while(|(x, y)| same(x, y)).count();
    let mut parts: Vec<String> = vec!["..".into(); b.len() - common];
    parts.extend(p[common..].iter().map(|c| c.as_os_str().to_string_lossy().to_string()));
    Some(parts.join("/"))
}

/// Joins the working copy prefix of the project directory (`git rev-parse --show-prefix`, e.g. `l10n/`)
/// with a path relative to that directory; `None` if the result lies outside the working copy.
pub fn repo_path(prefix: &str, rel: &str) -> Option<String> {
    let mut out: Vec<&str> = Vec::new();
    for part in prefix.split('/').chain(rel.split('/')) {
        match part {
            "" | "." => {}
            ".." => {
                out.pop()?;
            }
            p => out.push(p),
        }
    }
    (!out.is_empty()).then(|| out.join("/"))
}

/// Project files as working copy paths (`/`-separated), sorted and without duplicates.
pub fn repo_files(p: &Project, slp_dir: &Path, prefix: &str) -> Vec<String> {
    let base = std::path::absolute(slp_dir).unwrap_or_else(|_| slp_dir.to_path_buf());
    let mut files: Vec<String> = project_files(p).iter().filter_map(|f| relative(f, &base)).filter_map(|r| repo_path(prefix, &r)).collect();
    files.sort();
    files.dedup();
    files
}

/// Whether a changed working copy path belongs to the project files or extra configured paths
/// (case-insensitive on Windows).
pub fn matches(path: &str, files: &[String], extra: &[String]) -> bool {
    let eq = |a: &str, b: &str| if cfg!(windows) { a.eq_ignore_ascii_case(b) } else { a == b };
    let under = |dir: &str| {
        let dir = dir.trim_matches('/');
        !dir.is_empty() && path.len() > dir.len() && eq(&path[..dir.len()], dir) && path.as_bytes()[dir.len()] == b'/'
    };
    files.iter().any(|f| eq(f, path)) || extra.iter().any(|e| eq(e.trim_matches('/'), path) || under(e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_paths() {
        let base = if cfg!(windows) { Path::new("C:\\repo\\l10n") } else { Path::new("/repo/l10n") };
        let file = base.join("..").join("src").join("Res.de.resx");
        assert_eq!(relative(&file, base).as_deref(), Some("../src/Res.de.resx"));
        assert_eq!(relative(&base.join("app.slp"), base).as_deref(), Some("app.slp"));
    }

    #[test]
    fn working_copy_paths() {
        assert_eq!(repo_path("l10n/", "../src/a.resx").as_deref(), Some("src/a.resx"));
        assert_eq!(repo_path("", "app.slp").as_deref(), Some("app.slp"));
        assert_eq!(repo_path("", "../outside.txt"), None);
    }

    #[test]
    fn matching() {
        let files = vec!["l10n/app.slp".to_string()];
        let extra = vec!["docs/i18n".to_string()];
        assert!(matches("l10n/app.slp", &files, &extra));
        assert!(matches("docs/i18n/x.md", &files, &extra));
        assert!(!matches("docs/i18n2/x.md", &files, &extra));
        assert!(!matches("src/main.rs", &files, &extra));
    }
}
