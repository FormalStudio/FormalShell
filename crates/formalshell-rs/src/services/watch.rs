//! Tells a service when a user-authored file may have changed.
//!
//! inotify resolves a path when the watch is added, so a watch on the file
//! sits on whatever the symlink pointed at then. Home-manager retargets
//! `~/.config/formalshell/*` into a new store path on every activation, which
//! leaves the old inode untouched and says nothing. What does announce it is
//! the directory holding the link, so this watches the directory of every hop
//! from the path to the file it finally lands on (and the nearest existing
//! ancestor while a directory is missing, for a config that shows up later),
//! and re-resolves after every event so the next retarget is seen the same way.
//! [`Watch::dirs`] watches what is inside directories as well: every
//! directory under each one, re-walked after every event, so a subdirectory
//! made later is watched too.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Events arrive in bursts (a write is several), and a rename-over is a
/// create plus a delete; one wake per burst.
const SETTLE: Duration = Duration::from_millis(50);

/// A directory of desktop entries changes a file per entry, and a package
/// lands several entries and icons a few milliseconds apart.
const DIRS_SETTLE: Duration = Duration::from_millis(200);

const MAX_HOPS: usize = 40;

/// How deep [`Watch::dirs`] follows subdirectories, a symlink loop included.
const MAX_DEPTH: usize = 8;

/// `dir` and every directory under it.
fn subtree(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if depth > MAX_DEPTH || !dir.is_dir() {
        return;
    }
    out.push(dir.to_path_buf());
    let Ok(read) = std::fs::read_dir(dir) else { return };
    for item in read.flatten() {
        subtree(&item.path(), depth + 1, out);
    }
}

/// Each hop of the symlink chain from `path`, `path` first and the file it
/// lands on last. A hop that is not a symlink ends the chain.
fn chain(path: &Path) -> Vec<PathBuf> {
    let mut hops = vec![path.to_path_buf()];
    while hops.len() < MAX_HOPS {
        let last = hops.last().unwrap();
        let Ok(target) = std::fs::read_link(last) else { break };
        let next = last.parent().map(|p| p.join(&target)).unwrap_or(target);
        hops.push(next);
    }
    hops
}

/// The directories whose events can mean `path` now reads differently, and
/// the names within them that matter.
fn targets(path: &Path) -> (Vec<PathBuf>, Vec<OsString>) {
    let mut dirs: Vec<PathBuf> = Vec::new();
    let mut names: Vec<OsString> = Vec::new();
    let mut note = |dir: &Path, name: Option<&OsStr>| {
        if !dirs.iter().any(|d| d == dir) {
            dirs.push(dir.to_path_buf());
        }
        if let Some(name) = name {
            if !names.iter().any(|n| n == name) {
                names.push(name.to_os_string());
            }
        }
    };
    for hop in chain(path) {
        let mut current = hop.as_path();
        // A missing directory has nothing to watch, so its nearest existing
        // ancestor sees it created. A directory that is itself a symlink is
        // retargeted from its own parent.
        while let Some(parent) = current.parent() {
            note(parent, current.file_name());
            if parent.exists() && !parent.is_symlink() {
                break;
            }
            current = parent;
        }
    }
    (dirs, names)
}

#[cfg(target_os = "linux")]
pub use linux::Watch;

#[cfg(not(target_os = "linux"))]
pub use poll::Watch;

#[cfg(target_os = "linux")]
mod linux {
    use std::ffi::OsString;
    use std::io;
    use std::os::fd::{AsFd, OwnedFd};
    use std::path::PathBuf;
    use std::time::Duration;

    use async_io::{Async, Timer};
    use inotify::{Inotify, WatchDescriptor, WatchMask};

    use super::{DIRS_SETTLE, SETTLE, subtree, targets};

    const MASK: WatchMask = WatchMask::CREATE
        .union(WatchMask::DELETE)
        .union(WatchMask::MOVED_TO)
        .union(WatchMask::MOVED_FROM)
        .union(WatchMask::CLOSE_WRITE)
        .union(WatchMask::ATTRIB);

    pub struct Watch {
        paths: Vec<PathBuf>,
        /// Whether an event inside each path counts, not only the path's own.
        contents: bool,
        settle: Duration,
        inotify: Inotify,
        /// A dup of the inotify fd, only to wait for readability on.
        ready: Async<OwnedFd>,
        watches: Vec<WatchDescriptor>,
        /// The directories inside a path, where every event counts.
        trees: Vec<WatchDescriptor>,
        names: Vec<OsString>,
        /// Set once a relevant event has been read and cleared when
        /// [`Watch::changed`] returns, so a caller that drops the future
        /// mid-settle is still told on the next call.
        pending: bool,
    }

    /// One read of what is queued; true when any event names something in
    /// the chain or lands inside a watched tree.
    fn read(inotify: &mut Inotify, names: &[OsString], trees: &[WatchDescriptor]) -> io::Result<bool> {
        let mut buffer = [0u8; 4096];
        let events = inotify.read_events(&mut buffer)?;
        Ok(events.into_iter().any(|e| trees.contains(&e.wd) || e.name.is_some_and(|n| names.iter().any(|m| m == n))))
    }

    impl Watch {
        pub fn new(path: PathBuf) -> io::Result<Self> {
            Self::open(vec![path], false, SETTLE)
        }

        /// Resolves on any change inside any of `dirs`, one created after
        /// the watch included.
        pub fn dirs(dirs: Vec<PathBuf>) -> io::Result<Self> {
            Self::open(dirs, true, DIRS_SETTLE)
        }

        fn open(paths: Vec<PathBuf>, contents: bool, settle: Duration) -> io::Result<Self> {
            let inotify = Inotify::init()?;
            let ready = Async::new(inotify.as_fd().try_clone_to_owned()?)?;
            let mut watch =
                Self { paths, contents, settle, inotify, ready, watches: Vec::new(), trees: Vec::new(), names: Vec::new(), pending: false };
            watch.rearm();
            Ok(watch)
        }

        fn rearm(&mut self) {
            let inotify = &mut self.inotify;
            for wd in self.watches.drain(..) {
                let _ = inotify.watches().remove(wd);
            }
            self.trees.clear();
            self.names.clear();
            for path in &self.paths {
                let (dirs, names) = targets(path);
                for name in names {
                    if !self.names.contains(&name) {
                        self.names.push(name);
                    }
                }
                for dir in dirs {
                    if let Ok(wd) = inotify.watches().add(&dir, MASK) {
                        self.watches.push(wd);
                    }
                }
                if self.contents {
                    let mut tree = Vec::new();
                    subtree(path, 0, &mut tree);
                    for dir in tree {
                        if let Ok(wd) = inotify.watches().add(&dir, MASK) {
                            self.trees.push(wd.clone());
                            self.watches.push(wd);
                        }
                    }
                }
            }
        }

        /// Reads until an event names something in the chain.
        async fn wait(&mut self) -> io::Result<()> {
            loop {
                match read(&mut self.inotify, &self.names, &self.trees) {
                    Ok(true) => return Ok(()),
                    Ok(false) => {}
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => self.ready.readable().await?,
                    Err(e) => return Err(e),
                }
            }
        }

        /// Resolves once the file `path` names may read differently.
        pub async fn changed(&mut self) {
            while !self.pending {
                if let Err(e) = self.wait().await {
                    eprintln!("watch {}: {e}", self.paths.first().map(|p| p.display().to_string()).unwrap_or_default());
                    Timer::after(Duration::from_secs(5)).await;
                }
                self.pending = true;
            }
            Timer::after(self.settle).await;
            while read(&mut self.inotify, &self.names, &self.trees).is_ok() {}
            self.rearm();
            self.pending = false;
        }
    }
}

#[cfg(not(target_os = "linux"))]
mod poll {
    use std::io;
    use std::path::PathBuf;
    use std::time::Duration;

    use async_io::Timer;

    pub struct Watch;

    impl Watch {
        pub fn new(_path: PathBuf) -> io::Result<Self> {
            Ok(Self)
        }

        pub async fn changed(&mut self) {
            Timer::after(Duration::from_secs(5)).await;
        }
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use std::os::unix::fs::symlink;
    use std::time::Duration;

    use async_io::Timer;
    use futures_lite::future::{block_on, or};

    use super::*;

    async fn within(watch: &mut Watch, ms: u64) -> bool {
        or(
            async {
                watch.changed().await;
                true
            },
            async {
                Timer::after(Duration::from_millis(ms)).await;
                false
            },
        )
        .await
    }

    /// The home-manager shape: a link into one generation, then swapped for a
    /// link into another while the first is left exactly as it was.
    #[test]
    fn symlink_retarget_is_seen() {
        let dir = tempfile::tempdir().unwrap();
        let (gen1, gen2) = (dir.path().join("gen1.json"), dir.path().join("gen2.json"));
        std::fs::write(&gen1, "{}").unwrap();
        std::fs::write(&gen2, "{}").unwrap();
        let cfg = dir.path().join("cfg");
        std::fs::create_dir(&cfg).unwrap();
        let link = cfg.join("settings.json");
        symlink(&gen1, &link).unwrap();
        block_on(async {
            let mut watch = Watch::new(link.clone()).unwrap();
            assert!(!within(&mut watch, 200).await, "woke with nothing changed");

            let tmp = cfg.join("settings.tmp");
            symlink(&gen2, &tmp).unwrap();
            std::fs::rename(&tmp, &link).unwrap();
            assert!(within(&mut watch, 2000).await, "rename over the link went unseen");

            std::fs::remove_file(&link).unwrap();
            symlink(&gen1, &link).unwrap();
            assert!(within(&mut watch, 2000).await, "unlink and recreate went unseen");

            std::fs::write(&gen1, "{\"a\":1}").unwrap();
            assert!(within(&mut watch, 2000).await, "an edit in place of the target went unseen");

            std::fs::write(cfg.join("other"), "x").unwrap();
            assert!(!within(&mut watch, 300).await, "an unrelated name woke the watch");
        });
    }

    #[test]
    fn missing_directory_is_seen_when_created() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("cfg");
        let file = cfg.join("settings.json");
        block_on(async {
            let mut watch = Watch::new(file.clone()).unwrap();
            std::fs::create_dir(&cfg).unwrap();
            assert!(within(&mut watch, 2000).await, "the directory appearing went unseen");
            std::fs::write(&file, "{}").unwrap();
            assert!(within(&mut watch, 2000).await, "the file appearing in the new directory went unseen");
        });
    }

    /// fspec's shape: a data directory made after the watch starts, its
    /// entries symlinks into a store path, and a subdirectory of them.
    #[test]
    fn dirs_see_a_late_directory_and_its_contents() {
        let dir = tempfile::tempdir().unwrap();
        let store = dir.path().join("store/app.desktop");
        std::fs::create_dir_all(store.parent().unwrap()).unwrap();
        std::fs::write(&store, "[Desktop Entry]").unwrap();
        let apps = dir.path().join("late/share/applications");
        block_on(async {
            let mut watch = Watch::dirs(vec![apps.clone()]).unwrap();
            assert!(!within(&mut watch, 300).await, "woke with nothing changed");
            std::fs::create_dir_all(&apps).unwrap();
            assert!(within(&mut watch, 2000).await, "the directory appearing went unseen");
            symlink(&store, apps.join("app.desktop")).unwrap();
            assert!(within(&mut watch, 2000).await, "a linked entry went unseen");
            std::fs::create_dir(apps.join("sub")).unwrap();
            assert!(within(&mut watch, 2000).await, "a subdirectory appearing went unseen");
            std::fs::write(apps.join("sub/other.desktop"), "[Desktop Entry]").unwrap();
            assert!(within(&mut watch, 2000).await, "an entry in the subdirectory went unseen");
            std::fs::remove_file(apps.join("app.desktop")).unwrap();
            assert!(within(&mut watch, 2000).await, "a removed entry went unseen");
            std::fs::write(dir.path().join("late/share/unrelated"), "x").unwrap();
            assert!(!within(&mut watch, 400).await, "an unrelated name woke the watch");
        });
    }
}
