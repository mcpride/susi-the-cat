//! Round-trips every project in `testdata/` (`slp/` corpus and `projects/`).
//!
//! Projects written by Sisulizer 4.0.353 or newer must be reproduced byte for byte. Older writers
//! did not escape `>` and some files were edited by hand (`<lang id="de" />`); for those the output
//! must be stable (writing the re-read output yields identical bytes).

use std::path::{Path, PathBuf};

fn samples_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata")
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            collect(&p, out);
        } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("slp")) {
            out.push(p);
        }
    }
}

fn version_of(p: &susi_core::Project) -> Vec<u32> {
    p.attrs.get("version").unwrap_or("0").split('.').filter_map(|x| x.parse().ok()).collect()
}

fn diff(a: &[u8], b: &[u8]) -> String {
    let pos = a.iter().zip(b).position(|(x, y)| x != y).unwrap_or(a.len().min(b.len()));
    let from = pos.saturating_sub(60);
    format!(
        "differs at {pos}\n  orig: {:?}\n  ours: {:?}",
        String::from_utf8_lossy(&a[from..(pos + 60).min(a.len())]),
        String::from_utf8_lossy(&b[from..(pos + 60).min(b.len())])
    )
}

#[test]
fn sample_projects_roundtrip() {
    let mut files = Vec::new();
    collect(&samples_dir(), &mut files);
    assert!(files.len() > 100, "testdata incomplete: {} projects", files.len());
    let mut failures = Vec::new();
    let mut identical = 0;
    for f in &files {
        let bytes = std::fs::read(f).unwrap();
        let p = match susi_core::slp::parse(&bytes) {
            Ok(p) => p,
            Err(e) => {
                failures.push(format!("{}: {e}", f.display()));
                continue;
            }
        };
        let out = susi_core::slp::to_bytes(&p);
        if out == bytes {
            identical += 1;
            continue;
        }
        // `#n` (legacy line break) is normalised to `#c#l` on write.
        let legacy = bytes.windows(3).any(|w| w == b" />") || bytes.windows(2).any(|w| w == b"#n");
        if version_of(&p) >= vec![4, 0, 353] && !legacy {
            failures.push(format!("{}: {}", f.display(), diff(&bytes, &out)));
            continue;
        }
        let again = susi_core::slp::to_bytes(&susi_core::slp::parse(&out).unwrap());
        if again != out {
            failures.push(format!("{}: unstable output, {}", f.display(), diff(&out, &again)));
        }
    }
    eprintln!("{identical} of {} projects byte-identical", files.len());
    assert!(failures.is_empty(), "{} of {} projects failed:\n{}", failures.len(), files.len(), failures.join("\n"));
}
