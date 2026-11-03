//! Project automation: `cargo xtask <command>`.
//!
//! Commands:
//! - `ci`    – rustfmt check, clippy (`-D warnings`) and all tests, as in CI
//! - `fmt`   – format the workspace
//! - `lint`  – clippy for all targets with `-D warnings`
//! - `test`  – run all tests
//! - `dist`  – release build (icon and version info required); copies `susi` and `susi-cli` to `dist/`
//! - `icons` – renders `assets/app-icon/susi.svg` to `susi.ico` (16–256 px) and `susi-256.png`

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
        Some("icons") => icons(),
        _ => {
            eprintln!("usage: cargo xtask <ci|fmt|lint|test|dist|icons>");
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

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."))
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

fn dist() -> Result {
    // Release executables must carry the icon and version info (see apps/*/build.rs).
    cargo_env(&["build", "--release", "-p", "susi", "-p", "susi-cli"], &[("SUSI_REQUIRE_WINRES", "1")])?;
    let root = root();
    let dist = root.join("dist");
    std::fs::create_dir_all(&dist).map_err(|e| format!("{}: {e}", dist.display()))?;
    for name in ["susi", "susi-cli"] {
        let file = format!("{name}{}", std::env::consts::EXE_SUFFIX);
        let from = root.join("target").join("release").join(&file);
        let to = dist.join(&file);
        std::fs::copy(&from, &to).map_err(|e| format!("{} → {}: {e}", from.display(), to.display()))?;
        eprintln!("{}", to.display());
    }
    Ok(())
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
