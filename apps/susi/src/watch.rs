//! Monitoring of the open project file: changes made by other programs (Sisulizer, `susi-cli`,
//! the MCP server, git) are detected and the user is asked whether to reload the project.
//!
//! A background thread polls the modification time and size of the `.slp` and wakes the UI once
//! they have changed and then stayed the same for one interval, so a file being written is not
//! reported half-way. The UI thread compares the file with the version the open project was loaded
//! from or saved to; a file rewritten with the same content is not reported.

use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, SystemTime};

use susi_core::tf;

use crate::app::SusiApp;
use crate::dialogs::Dialog;

/// Polling interval of the background thread.
#[cfg(not(test))]
const INTERVAL: Duration = Duration::from_secs(1);
#[cfg(test)]
const INTERVAL: Duration = Duration::from_millis(20);

/// Modification time and size of a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Stamp {
    modified: Option<SystemTime>,
    len: u64,
}

impl Stamp {
    /// `None` if the file does not exist or cannot be accessed.
    fn of(path: &Path) -> Option<Self> {
        let m = std::fs::metadata(path).ok()?;
        Some(Self { modified: m.modified().ok(), len: m.len() })
    }
}

/// One version of the project file on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fingerprint {
    path: PathBuf,
    stamp: Stamp,
    hash: u64,
}

impl Fingerprint {
    /// Reads the file; `None` if it cannot be read.
    pub fn of(path: &Path) -> Option<Self> {
        let stamp = Stamp::of(path)?;
        let bytes = std::fs::read(path).ok()?;
        let mut h = DefaultHasher::new();
        bytes.hash(&mut h);
        Some(Self { path: path.to_path_buf(), stamp, hash: h.finish() })
    }
}

/// Background thread watching one file; it ends when the poller is dropped.
struct Poller {
    path: PathBuf,
    /// Set by the thread when the file has changed; cleared by the UI when it checks.
    changed: Arc<AtomicBool>,
    /// Dropping the sender ends the thread.
    _stop: mpsc::Sender<()>,
}

impl Poller {
    fn start(path: &Path, ctx: &egui::Context) -> Self {
        // Checked once at the start: the file may have changed since it was loaded.
        let changed = Arc::new(AtomicBool::new(true));
        let (stop, stopped) = mpsc::channel::<()>();
        let (flag, file, ctx) = (Arc::clone(&changed), path.to_path_buf(), ctx.clone());
        // Taken here, not in the thread: a change before the thread runs is reported as well.
        let mut last = Stamp::of(path);
        // Without the thread (cannot be spawned) changes are not noticed; nothing else depends on it.
        let _ = std::thread::Builder::new().name("susi-watch".into()).spawn(move || {
            let mut reported = last;
            while stopped.recv_timeout(INTERVAL) == Err(RecvTimeoutError::Timeout) {
                let now = Stamp::of(&file);
                if now == last && now != reported {
                    reported = now;
                    flag.store(true, Ordering::Relaxed);
                    ctx.request_repaint();
                }
                last = now;
            }
        });
        Self { path: path.to_path_buf(), changed, _stop: stop }
    }
}

/// Watch of the open project file.
#[derive(Default)]
pub struct FileWatch {
    /// Version on disk the open project corresponds to: loaded, saved, or kept by the user.
    known: Option<Fingerprint>,
    poller: Option<Poller>,
    /// The file was missing at the last check (reported once).
    missing: bool,
}

impl FileWatch {
    /// Records the version on disk after the project was loaded or saved.
    pub fn set_known(&mut self, fp: Option<Fingerprint>) {
        self.known = fp;
        self.missing = false;
    }
}

/// Watches the file of the open project and asks once it was changed by another program. While a
/// dialog is open or a git action runs (the workflow reloads the project itself), it waits.
pub fn poll(app: &mut SusiApp, ctx: &egui::Context) {
    let Some(path) = app.path().map(Path::to_path_buf) else {
        app.watch.poller = None;
        return;
    };
    if app.watch.poller.as_ref().is_none_or(|p| p.path != path) {
        app.watch.poller = Some(Poller::start(&path, ctx));
    }
    let Some(changed) = app.watch.poller.as_ref().map(|p| Arc::clone(&p.changed)) else {
        return;
    };
    if !changed.load(Ordering::Relaxed) {
        return;
    }
    if app.dialog.is_some() || app.wf_job.is_some() {
        ctx.request_repaint_after(INTERVAL);
        return;
    }
    changed.store(false, Ordering::Relaxed);
    check(app);
}

/// Compares the project file with the known version and opens [`Dialog::Reload`] if its content
/// has changed. Each version is asked for once: after *Don't reload* the open project is kept
/// until the file changes again.
pub fn check(app: &mut SusiApp) {
    let Some(path) = app.path().map(Path::to_path_buf) else {
        return;
    };
    let Some(stamp) = Stamp::of(&path) else {
        if !std::mem::replace(&mut app.watch.missing, true) {
            app.warn(tf!("The project file no longer exists: {path}", path = path.display()));
        }
        return;
    };
    app.watch.missing = false;
    // Without a known version of this file (another project, not loaded from disk) the current one is taken.
    let known = app.watch.known.as_ref().filter(|k| k.path == path);
    if known.is_some_and(|k| k.stamp == stamp) {
        return;
    }
    let Some(now) = Fingerprint::of(&path) else {
        return;
    };
    let changed = known.is_some_and(|k| k.hash != now.hash);
    app.watch.known = Some(now);
    if changed {
        app.info(tf!("The project file was changed by another program: {path}", path = path.display()));
        app.dialog = Some(Dialog::Reload);
    }
}
