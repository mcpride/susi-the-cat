//! Project automation: `cargo xtask <command>`.
//!
//! Commands:
//! - `ci`    – rustfmt check, clippy (`-D warnings`) and all tests, as in CI
//! - `fmt`   – format the workspace
//! - `lint`  – clippy for all targets with `-D warnings`
//! - `test`  – run all tests
//! - `dist`  – release build (icon and version info required on Windows); copies `susi`, `susi-cli`,
//!   `README.md` and `CHANGELOG.md` to `dist/` and builds the user guide into `dist/help/` (without
//!   mdbook only a warning, unless `SUSI_REQUIRE_HELP` is set, as in the release build)
//! - `help`  – user guide (`docs/user`, mdBook): updates `po/messages.pot` from the English texts and
//!   builds all languages into `target/help/<lang>/`; `--check` fails if `messages.pot` was out of
//!   date (CI). Requires mdbook 0.5 and mdbook-i18n-helpers 0.4 (see `MDBOOK_HINT`)
//! - `sbom`  – CycloneDX SBOM (JSON) of `susi` and `susi-cli` for the host platform into `dist/`
//!   (`susi.cdx.json`, `susi-cli.cdx.json`; requires `cargo install cargo-cyclonedx --locked`)
//! - `icons` – renders `assets/app-icon/susi.svg` to `susi.ico` (16–256 px) and `susi-256.png`
//! - `set-version <x.y.z>` – sets the workspace version in `Cargo.toml` and `Cargo.lock` (used by semantic-release)

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

type Result<T = ()> = std::result::Result<T, String>;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("ci") => ci(),
        Some("fmt") => cargo(&["fmt", "--all"]),
        Some("lint") => lint(),
        Some("test") => test(),
        Some("dist") => dist(),
        Some("help") => help(&args[1..]),
        Some("sbom") => sbom(),
        Some("icons") => icons(),
        Some("set-version") => set_version(args.get(1).map(String::as_str)),
        _ => {
            eprintln!("usage: cargo xtask <ci|fmt|lint|test|dist|help [--check]|sbom|icons|set-version <x.y.z>>");
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("xtask: {e}");
            ExitCode::FAILURE
        }
    }
}

/// The workspace root. `cargo run` sets `CARGO_MANIFEST_DIR` at run time as well; that value wins
/// over the compile-time one, so a binary shared via `CARGO_TARGET_DIR` works on its own checkout.
fn root() -> PathBuf {
    let manifest_dir = std::env::var_os("CARGO_MANIFEST_DIR").map_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")), PathBuf::from);
    manifest_dir.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."))
}

fn cargo(args: &[&str]) -> Result {
    cargo_env(args, &[])
}

fn cargo_env(args: &[&str], env: &[(&str, &str)]) -> Result {
    let exe = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    eprintln!("> cargo {}", args.join(" "));
    let status = Command::new(exe).args(args).envs(env.iter().copied()).current_dir(root()).status().map_err(|e| format!("cargo could not be started: {e}"))?;
    if status.success() { Ok(()) } else { Err(format!("`cargo {}` failed ({status})", args.join(" "))) }
}

fn lint() -> Result {
    cargo(&["clippy", "--workspace", "--all-targets", "--", "-D", "warnings"])
}

fn test() -> Result {
    cargo(&["test", "--workspace"])
}

fn ci() -> Result {
    cargo(&["fmt", "--all", "--check"])?;
    lint()?;
    test()
}

/// The applications shipped in `dist/` (package and executable names).
const APPS: [&str; 2] = ["susi", "susi-cli"];

fn dist() -> Result {
    // Release executables must carry the icon and version info (see apps/*/build.rs).
    cargo_env(&["build", "--release", "-p", "susi", "-p", "susi-cli"], &[("SUSI_REQUIRE_WINRES", "1")])?;
    let root = root();
    let dist = root.join("dist");
    std::fs::create_dir_all(&dist).map_err(|e| format!("{}: {e}", dist.display()))?;
    for name in APPS {
        let file = format!("{name}{}", std::env::consts::EXE_SUFFIX);
        let from = root.join("target").join("release").join(&file);
        let to = dist.join(&file);
        std::fs::copy(&from, &to).map_err(|e| format!("{} → {}: {e}", from.display(), to.display()))?;
        eprintln!("{}", to.display());
    }
    for file in ["README.md", "CHANGELOG.md"] {
        let from = root.join(file);
        if from.exists() {
            std::fs::copy(&from, dist.join(file)).map_err(|e| format!("{}: {e}", from.display()))?;
        }
    }
    // The user guide of this version next to the executables: `F1` opens it before the online help.
    match build_help(&dist.join("help")) {
        Err(e) if std::env::var_os("SUSI_REQUIRE_HELP").is_none() => eprintln!("xtask: warning: dist/ has no user guide: {e}"),
        result => result?,
    }
    Ok(())
}

/// Languages of the user guide with their titles (mdbook-gettext translates the chapters, not the
/// book title). English is written in `docs/user/src`, the others are translated in `po/<lang>.po`.
const HELP_LANGUAGES: [(&str, &str); 2] = [("en", "Susi the CAT {version} – User guide"), ("de", "Susi the CAT {version} – Benutzerhandbuch")];
/// Path of the online help (GitHub Pages: https://mcpride.github.io/susi-the-cat/).
const HELP_SITE: &str = "/susi-the-cat/";
const MDBOOK_HINT: &str = "mdbook 0.5 and mdbook-i18n-helpers 0.4 are required: \
    cargo install mdbook --version 0.5.4 --locked; cargo install mdbook-i18n-helpers --version 0.4.0 --locked";

fn help_dir() -> PathBuf {
    root().join("docs").join("user")
}

fn help(args: &[String]) -> Result {
    let check = match args {
        [] => false,
        [a] if a == "--check" => true,
        _ => return Err("usage: cargo xtask help [--check]".into()),
    };
    if update_pot()? && check {
        return Err("docs/user/po/messages.pot was out of date (now updated): commit it and update the translations with Susi (docs/dev/user-guide.md)".into());
    }
    build_help(&root().join("target").join("help"))
}

fn mdbook(args: &[&str], env: &[(&str, &str)]) -> Result {
    eprintln!("> mdbook {}", args.join(" "));
    let status = Command::new("mdbook")
        .args(args)
        .envs(env.iter().copied())
        .current_dir(help_dir())
        .status()
        .map_err(|e| format!("mdbook could not be started ({e}); {MDBOOK_HINT}"))?;
    if status.success() { Ok(()) } else { Err(format!("`mdbook {}` failed ({status}); {MDBOOK_HINT}", args.join(" "))) }
}

/// Extracts the English texts into `docs/user/po/messages.pot`, the source of the translation
/// project `susi-help.slp`. Returns whether the file changed.
fn update_pot() -> Result<bool> {
    let tmp = root().join("target").join("help-pot");
    mdbook(&["build", "-d", &tmp.to_string_lossy()], &[("MDBOOK_OUTPUT", r#"{"xgettext": {"granularity": 0}}"#)])?;
    let generated = tmp.join("messages.pot");
    let text = std::fs::read_to_string(&generated).map_err(|e| format!("{}: {e}", generated.display()))?;
    let pot = normalize_pot(&text);
    let path = help_dir().join("po").join("messages.pot");
    if std::fs::read_to_string(&path).is_ok_and(|old| old == pot) {
        return Ok(false);
    }
    std::fs::write(&path, pot).map_err(|e| format!("{}: {e}", path.display()))?;
    eprintln!("{}", path.display());
    Ok(true)
}

/// Makes the POT depend on the texts only: no creation time, `/` in the references on every platform.
fn normalize_pot(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.lines() {
        if line.starts_with("\"POT-Creation-Date:") {
            out.push_str("\"POT-Creation-Date: \\n\"");
        } else if let Some(refs) = line.strip_prefix("#: ") {
            out.push_str("#: ");
            out.push_str(&refs.replace('\\', "/"));
        } else {
            out.push_str(line);
        }
        out.push('\n');
    }
    out
}

/// Builds the user guide in every language into `out/<lang>/`, plus `out/index.html` choosing
/// the language of the browser (the start page of the online help).
fn build_help(out: &Path) -> Result {
    for (lang, title) in HELP_LANGUAGES {
        let dir = out.join(lang);
        let title = title.replace("{version}", env!("CARGO_PKG_VERSION"));
        let site = format!("{HELP_SITE}{lang}/");
        let env = [("MDBOOK_BOOK__LANGUAGE", lang), ("MDBOOK_BOOK__TITLE", title.as_str()), ("MDBOOK_OUTPUT__HTML__SITE_URL", site.as_str())];
        eprintln!("Building mdbook with the following environment: {env:?}");
        mdbook(&["build", "-d", &dir.to_string_lossy()], &env)?;
        eprintln!("{}", dir.display());
    }
    let from = help_dir().join("language-select.html");
    std::fs::copy(&from, out.join("index.html")).map_err(|e| format!("{}: {e}", from.display()))?;
    Ok(())
}

/// Directories of the workspace members (`members` in Cargo.toml: `crates/*`, `apps/*`, `xtask`).
fn member_dirs(root: &Path) -> Vec<PathBuf> {
    let mut dirs = vec![root.join("xtask")];
    for group in ["crates", "apps"] {
        let entries = std::fs::read_dir(root.join(group)).into_iter().flatten().flatten();
        dirs.extend(entries.map(|e| e.path()).filter(|p| p.join("Cargo.toml").is_file()));
    }
    dirs
}

/// CycloneDX SBOM per application with cargo-cyclonedx: the dependencies built for the host
/// platform (the release target in CI) with versions, licenses and checksums; build dependencies
/// are listed with scope "excluded", dev-dependencies not at all.
fn sbom() -> Result {
    let root = root();
    let dist = root.join("dist");
    std::fs::create_dir_all(&dist).map_err(|e| format!("{}: {e}", dist.display()))?;
    // Reproducible: the time of the checked-out commit instead of the current time.
    let epoch = std::env::var("SOURCE_DATE_EPOCH").ok().or_else(|| {
        let out = Command::new("git").args(["log", "-1", "--format=%ct"]).current_dir(&root).output().ok()?;
        out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
    });
    let env: Vec<(&str, &str)> = epoch.iter().map(|e| ("SOURCE_DATE_EPOCH", e.as_str())).collect();
    cargo_env(&["cyclonedx", "--format", "json", "--spec-version", "1.6", "--quiet"], &env)
        .map_err(|e| format!("{e}; cargo-cyclonedx is required: cargo install cargo-cyclonedx --locked"))?;
    // cargo-cyclonedx writes `<package>.cdx.json` next to the manifest of every workspace member.
    for dir in member_dirs(&root) {
        for path in std::fs::read_dir(&dir).into_iter().flatten().flatten().map(|e| e.path()) {
            let Some(package) = path.file_name().and_then(|n| n.to_str()).and_then(|n| n.strip_suffix(".cdx.json")) else { continue };
            if APPS.contains(&package) {
                let to = dist.join(format!("{package}.cdx.json"));
                std::fs::rename(&path, &to).map_err(|e| format!("{} → {}: {e}", path.display(), to.display()))?;
                eprintln!("{}", to.display());
            } else {
                std::fs::remove_file(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            }
        }
    }
    match APPS.iter().map(|a| dist.join(format!("{a}.cdx.json"))).find(|p| !p.is_file()) {
        Some(missing) => Err(format!("{} was not created", missing.display())),
        None => Ok(()),
    }
}

fn set_version(version: Option<&str>) -> Result {
    let version = version.ok_or("usage: cargo xtask set-version <x.y.z>")?;
    if !is_semver(version) {
        return Err(format!("`{version}` is not a semantic version (x.y.z[-pre][+build])"));
    }
    let root = root();
    for (file, bump) in [("Cargo.toml", bump_manifest as fn(&str, &str) -> Result<String>), ("Cargo.lock", bump_lock)] {
        let path = root.join(file);
        let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        std::fs::write(&path, bump(&text, version)?).map_err(|e| format!("{}: {e}", path.display()))?;
        eprintln!("{}: {version}", path.display());
    }
    Ok(())
}

fn is_semver(v: &str) -> bool {
    let ident = |s: &str| !s.is_empty() && s.split('.').all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'));
    let (rest, build) = v.split_once('+').map_or((v, None), |(r, b)| (r, Some(b)));
    let (core, pre) = rest.split_once('-').map_or((rest, None), |(c, p)| (c, Some(p)));
    let numbers: Vec<&str> = core.split('.').collect();
    numbers.len() == 3
        && numbers.iter().all(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()) && (n.len() == 1 || !n.starts_with('0')))
        && pre.is_none_or(ident)
        && build.is_none_or(ident)
}

/// Replaces the value of a `version = "..."` line, keeping indentation and line ending.
fn replace_version_line(line: &str, version: &str) -> Option<String> {
    let rest = line.trim_start().strip_prefix("version")?.trim_start().strip_prefix('=')?.trim();
    if !rest.starts_with('"') {
        return None;
    }
    let indent = &line[..line.len() - line.trim_start().len()];
    let ending = if line.ends_with("\r\n") {
        "\r\n"
    } else if line.ends_with('\n') {
        "\n"
    } else {
        ""
    };
    Some(format!("{indent}version = \"{version}\"{ending}"))
}

/// Sets `version` in the `[workspace.package]` table of the workspace manifest.
fn bump_manifest(text: &str, version: &str) -> Result<String> {
    let mut out = String::with_capacity(text.len());
    let (mut in_table, mut done) = (false, false);
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_table = trimmed == "[workspace.package]";
        }
        match replace_version_line(line, version).filter(|_| in_table && !done) {
            Some(new) => {
                out.push_str(&new);
                done = true;
            }
            None => out.push_str(line),
        }
    }
    if done { Ok(out) } else { Err("Cargo.toml: no `version` in [workspace.package]".into()) }
}

/// Sets the version of every local package in `Cargo.lock`: the workspace members are the
/// `[[package]]` entries without a `source`.
fn bump_lock(text: &str, version: &str) -> Result<String> {
    let mut out = String::with_capacity(text.len());
    let mut count = 0;
    let blocks = text.split_inclusive('\n').fold(Vec::<Vec<&str>>::new(), |mut blocks, line| {
        if line.trim() == "[[package]]" || blocks.is_empty() {
            blocks.push(Vec::new());
        }
        if let Some(block) = blocks.last_mut() {
            block.push(line);
        }
        blocks
    });
    for block in blocks {
        let local = block.first().is_some_and(|l| l.trim() == "[[package]]") && !block.iter().any(|l| l.starts_with("source = "));
        for line in block {
            match replace_version_line(line, version).filter(|_| local && line.starts_with("version")) {
                Some(new) => {
                    out.push_str(&new);
                    count += 1;
                }
                None => out.push_str(line),
            }
        }
    }
    if count > 0 { Ok(out) } else { Err("Cargo.lock: no workspace packages found".into()) }
}

/// Pixel sizes in `susi.ico`: the Windows shell picks 16–48 for lists and menus, 256 for large tiles.
const ICO_SIZES: [u32; 9] = [16, 20, 24, 32, 40, 48, 64, 128, 256];

fn render_png(tree: &resvg::usvg::Tree, size: u32) -> Result<Vec<u8>> {
    let mut pixmap = resvg::tiny_skia::Pixmap::new(size, size).ok_or("invalid icon size")?;
    let scale = size as f32 / tree.size().width();
    resvg::render(tree, resvg::tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    pixmap.encode_png().map_err(|e| format!("PNG {size} px: {e}"))
}

fn icons() -> Result {
    let dir = root().join("assets").join("app-icon");
    let svg = std::fs::read(dir.join("susi.svg")).map_err(|e| format!("susi.svg: {e}"))?;
    let tree = resvg::usvg::Tree::from_data(&svg, &resvg::usvg::Options::default()).map_err(|e| format!("susi.svg: {e}"))?;
    let mut ico = ico::IconDir::new(ico::ResourceType::Icon);
    for size in ICO_SIZES {
        let image = ico::IconImage::read_png(std::io::Cursor::new(render_png(&tree, size)?)).map_err(|e| format!("{size} px: {e}"))?;
        ico.add_entry(ico::IconDirEntry::encode(&image).map_err(|e| format!("{size} px: {e}"))?);
    }
    let path = dir.join("susi.ico");
    let file = std::fs::File::create(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    ico.write(file).map_err(|e| format!("{}: {e}", path.display()))?;
    eprintln!("{}", path.display());
    let path = dir.join("susi-256.png");
    std::fs::write(&path, render_png(&tree, 256)?).map_err(|e| format!("{}: {e}", path.display()))?;
    eprintln!("{}", path.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{bump_lock, bump_manifest, is_semver};

    #[test]
    fn semver_validation() {
        for ok in ["0.2.0", "1.10.3", "1.0.0-rc.1", "1.0.0-alpha-2+build.5"] {
            assert!(is_semver(ok), "{ok}");
        }
        for bad in ["", "1.0", "1.0.0.0", "01.0.0", "1.0.x", "v1.0.0", "1.0.0-", "1.0.0-a..b", "1.0.0 "] {
            assert!(!is_semver(bad), "{bad}");
        }
    }

    #[test]
    fn manifest_bump_only_touches_workspace_package() {
        let text = "[workspace]\r\nmembers = []\r\n\r\n[workspace.package]\r\nversion = \"0.2.0\"\r\nedition = \"2024\"\r\n\r\n[workspace.dependencies]\r\nfoo = { version = \"1\" }\r\n";
        let out = bump_manifest(text, "0.3.0").unwrap();
        assert_eq!(out, text.replace("version = \"0.2.0\"", "version = \"0.3.0\""));
        assert!(bump_manifest("[package]\nversion = \"1.0.0\"\n", "2.0.0").is_err());
    }

    #[test]
    fn lock_bump_only_touches_local_packages() {
        let text = "version = 4\n\n[[package]]\nname = \"anyhow\"\nversion = \"0.2.0\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\n\n[[package]]\nname = \"susi\"\nversion = \"0.2.0\"\ndependencies = [\n \"anyhow\",\n]\n";
        let out = bump_lock(text, "1.0.0").unwrap();
        assert!(out.starts_with("version = 4\n"));
        assert!(out.contains("name = \"anyhow\"\nversion = \"0.2.0\"\n"));
        assert!(out.contains("name = \"susi\"\nversion = \"1.0.0\"\n"));
        assert_eq!(out.len(), text.len());
    }
}
