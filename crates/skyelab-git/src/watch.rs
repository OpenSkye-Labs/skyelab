//! Live folder watching: tells the app when a repo folder's status may have changed (a file written, a branch
//! switched, a commit), so the Changes tab and the change counts follow edits made outside SkyeLab.
//!
//! One thread per watched folder. It asks git where the repo's git dirs are, watches the folder and the git state,
//! drops events no status can show (gitignored paths, `.git` internals other than HEAD, the index and refs, our own
//! checkpoint refs, lock files) and calls back once per burst: after 300 ms without a new event, or 2 s into a
//! steady stream.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::{Duration, Instant};

use ignore::Match;
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use notify::event::{CreateKind, EventKind, ModifyKind};
use notify::{Config, EventKindMask, RecommendedWatcher, RecursiveMode, Watcher};
use skyelab_core::debug::{self, Entry, Kind};

/// Folders watched one by one past which a folder falls back to its top level plus git state.
pub const DIR_CAP: usize = 8192;
const QUIET: Duration = Duration::from_millis(300);
const MAX_WAIT: Duration = Duration::from_secs(2);

/// How the OS watches a tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// One watch covers the whole tree (FSEvents, ReadDirectoryChangesW): no count to cap.
    Recursive,
    /// One watch per folder (inotify, kqueue): only folders git can see, at most `cap`.
    PerDir { cap: usize },
}

impl Mode {
    /// The cheap mode for this platform's backend.
    pub fn native() -> Self {
        if cfg!(any(target_os = "macos", windows)) {
            Mode::Recursive
        } else {
            Mode::PerDir { cap: DIR_CAP }
        }
    }
}

enum Msg {
    Event(notify::Result<notify::Event>),
    Stop,
}

type Ready = Arc<(Mutex<Option<bool>>, Condvar)>;

/// A running watch. Dropping it stops the thread and the OS watches.
pub struct FolderWatch {
    tx: mpsc::Sender<Msg>,
    ready: Ready,
}

impl FolderWatch {
    /// Starts watching `dir` on its own thread and returns at once; `on_change` runs on that thread. A folder that
    /// isn't in a git repo is not watched (the thread ends).
    pub fn start(dir: &Path, mode: Mode, on_change: impl Fn() + Send + 'static) -> Self {
        let (tx, rx) = mpsc::channel();
        let ready: Ready = Arc::default();
        let (dir, events, r) = (dir.to_owned(), tx.clone(), ready.clone());
        let spawned = std::thread::Builder::new()
            .name("folder-watch".into())
            .spawn(move || run(&dir, mode, events, &rx, &r, on_change));
        if spawned.is_err() {
            set_ready(&ready, false);
        }
        Self { tx, ready }
    }

    /// Waits until the watches are in place: `true` once watching, `false` when it gave up (not a repo) or `timeout`
    /// passed first.
    pub fn wait_ready(&self, timeout: Duration) -> bool {
        let (lock, cvar) = &*self.ready;
        let guard = lock.lock().unwrap_or_else(PoisonError::into_inner);
        let (guard, _) = cvar
            .wait_timeout_while(guard, timeout, |r| r.is_none())
            .unwrap_or_else(PoisonError::into_inner);
        guard.unwrap_or(false)
    }
}

impl Drop for FolderWatch {
    fn drop(&mut self) {
        let _ = self.tx.send(Msg::Stop);
    }
}

fn set_ready(ready: &Ready, ok: bool) {
    let (lock, cvar) = &**ready;
    *lock.lock().unwrap_or_else(PoisonError::into_inner) = Some(ok);
    cvar.notify_all();
}

fn run(
    dir: &Path,
    mode: Mode,
    tx: mpsc::Sender<Msg>,
    rx: &mpsc::Receiver<Msg>,
    ready: &Ready,
    on_change: impl Fn(),
) {
    let _live = debug::Live::new("folder watches");
    let Some(mut state) = State::open(dir, mode) else {
        set_ready(ready, false);
        return;
    };
    let config = Config::default().with_event_kinds(EventKindMask::CORE);
    let handler = move |e| {
        let _ = tx.send(Msg::Event(e));
    };
    let mut watcher = match RecommendedWatcher::new(handler, config) {
        Ok(w) => w,
        Err(e) => {
            debug::record(|| Entry::new(Kind::Watch, "folder watch").failed(&e.to_string()));
            set_ready(ready, false);
            return;
        }
    };
    if !state.watch_all(&mut watcher) {
        set_ready(ready, false);
        return;
    }
    set_ready(ready, true);
    // The views took their first reading before the watches were in place: one read now covers that gap.
    on_change();
    // `(first, last)` relevant event of the burst being collected.
    let mut burst: Option<(Instant, Instant)> = None;
    loop {
        let msg = match burst {
            None => rx.recv().ok(),
            Some((first, last)) => {
                let due = (last + QUIET).min(first + MAX_WAIT);
                match rx.recv_timeout(due.saturating_duration_since(Instant::now())) {
                    Ok(m) => Some(m),
                    Err(RecvTimeoutError::Timeout) => {
                        burst = None;
                        state.reread_tracked(&mut watcher);
                        on_change();
                        continue;
                    }
                    Err(RecvTimeoutError::Disconnected) => None,
                }
            }
        };
        match msg {
            None | Some(Msg::Stop) => break,
            Some(Msg::Event(e)) => {
                if state.handle(e, &mut watcher) {
                    let now = Instant::now();
                    burst = Some((burst.map_or(now, |(first, _)| first), now));
                }
            }
        }
    }
    debug::record(|| Entry::new(Kind::Watch, "folder watch stopped"));
}

struct State {
    /// The repo's top folder: status and the diff cover the whole repo, so a subfolder project watches all of it.
    dir: PathBuf,
    toplevel: PathBuf,
    git_dir: PathBuf,
    common: PathBuf,
    mode: Mode,
    /// `.gitignore` files under the folder found so far.
    nested: Vec<PathBuf>,
    /// (root, matcher), most specific first: the first that decides wins, as in git.
    ignores: Vec<(PathBuf, Gitignore)>,
    /// Tracked files an ignore pattern matches (a force-added `target/keep.txt`): git still shows their edits.
    tracked_ignored: HashSet<PathBuf>,
    /// The index changed: `tracked_ignored` is read again before the next callback.
    index_moved: bool,
    /// Per-folder watches in place.
    // shortcut: never shrinks when folders go, so heavy folder churn can reach the cap early; track removals if seen.
    dirs: HashSet<PathBuf>,
    /// Over the cap: new folders are no longer watched.
    capped: bool,
}

impl State {
    fn open(dir: &Path, mode: Mode) -> Option<Self> {
        let out = crate::git(
            dir,
            &[
                "rev-parse",
                "--path-format=absolute",
                "--git-dir",
                "--git-common-dir",
                "--show-toplevel",
            ],
        )
        .ok()?;
        let out = String::from_utf8_lossy(&out).into_owned();
        let mut lines = out.lines().map(|l| std::fs::canonicalize(l).ok());
        let (git_dir, common, toplevel) = (lines.next()??, lines.next()??, lines.next()??);
        let mut state = Self {
            dir: toplevel.clone(),
            toplevel,
            git_dir,
            common,
            mode,
            nested: Vec::new(),
            ignores: Vec::new(),
            tracked_ignored: HashSet::new(),
            index_moved: false,
            dirs: HashSet::new(),
            capped: false,
        };
        state.build_ignores();
        state.tracked_ignored = tracked_ignored(&state.toplevel);
        Some(state)
    }

    fn build_ignores(&mut self) {
        let mut files: Vec<PathBuf> = self.nested.clone();
        // The folder and its parents up to the repo root (a project can be a subfolder of its repo).
        let above = self
            .dir
            .ancestors()
            .take_while(|a| a.starts_with(&self.toplevel));
        files.extend(above.map(|a| a.join(".gitignore")));
        files.sort_by_key(|f| std::cmp::Reverse(f.components().count()));
        files.dedup();
        let mut ignores: Vec<(PathBuf, Gitignore)> = files
            .iter()
            .filter(|f| f.is_file())
            .filter_map(|f| Some((f.parent()?.to_owned(), Gitignore::new(f).0)))
            .collect();
        let mut exclude = GitignoreBuilder::new(&self.toplevel);
        let _ = exclude.add(self.common.join("info/exclude"));
        if let Ok(gi) = exclude.build() {
            ignores.push((self.toplevel.clone(), gi));
        }
        ignores.push((
            self.toplevel.clone(),
            GitignoreBuilder::new(&self.toplevel).build_global().0,
        ));
        self.ignores = ignores;
    }

    fn is_ignored(&self, p: &Path) -> bool {
        // A deleted path can't be asked: count it as a folder too, so removing `target/` matches `target/`.
        let is_dir = p.is_dir() || !p.exists();
        for (root, gi) in &self.ignores {
            let Ok(rel) = p.strip_prefix(root) else {
                continue;
            };
            if rel.as_os_str().is_empty() {
                continue;
            }
            match gi.matched_path_or_any_parents(rel, is_dir) {
                Match::Ignore(_) => return true,
                Match::Whitelist(_) => return false,
                Match::None => {}
            }
        }
        false
    }

    /// Whether a change at `p` can change what status, the branch or a diff shows.
    fn relevant(&self, p: &Path) -> bool {
        if let Ok(rel) = p.strip_prefix(&self.git_dir) {
            if rel == Path::new("HEAD") || rel == Path::new("index") {
                return true;
            }
            if self.git_dir != self.common {
                return false;
            }
        }
        if let Ok(rel) = p.strip_prefix(&self.common) {
            let lock = p.extension().is_some_and(|e| e == "lock");
            return !lock
                && (rel == Path::new("packed-refs")
                    || (rel.starts_with("refs") && !rel.starts_with("refs/skyelab")));
        }
        let Ok(rel) = p.strip_prefix(&self.dir) else {
            return false;
        };
        // Another repo's git dir inside this one (a nested clone, a submodule).
        if rel.components().any(|c| c.as_os_str() == ".git") {
            return false;
        }
        !self.is_ignored(p) || self.tracked_ignored.contains(p)
    }

    /// Reads `tracked_ignored` again after the index moved (a commit, `git add -f`), and watches their folders.
    fn reread_tracked(&mut self, w: &mut RecommendedWatcher) {
        if std::mem::take(&mut self.index_moved) {
            self.tracked_ignored = tracked_ignored(&self.toplevel);
            self.watch_tracked_dirs(w);
        }
    }

    /// Per-folder mode skips ignored folders: the ones holding tracked files get a watch of their own.
    fn watch_tracked_dirs(&mut self, w: &mut RecommendedWatcher) {
        if let Mode::PerDir { cap } = self.mode {
            let dirs: HashSet<PathBuf> = self
                .tracked_ignored
                .iter()
                .filter_map(|f| Some(f.parent()?.to_owned()))
                .collect();
            for d in dirs {
                self.watch_dir(w, &d, cap);
            }
        }
    }

    /// Puts the watches in place; `false` when not even the folder could be watched.
    fn watch_all(&mut self, w: &mut RecommendedWatcher) -> bool {
        let watched = match self.mode {
            Mode::Recursive => {
                if w.watch(&self.dir, RecursiveMode::Recursive).is_err() {
                    return false;
                }
                // Only to learn the nested .gitignore files; nothing is watched per folder.
                let (dirs, ignores, _) = walk(&self.dir, DIR_CAP);
                self.nested = ignores;
                dirs.len()
            }
            Mode::PerDir { cap } => {
                let (dirs, ignores, over) = walk(&self.dir, cap);
                self.nested = ignores;
                if over {
                    self.fall_back(cap);
                    if w.watch(&self.dir, RecursiveMode::NonRecursive).is_err() {
                        return false;
                    }
                    self.dirs.insert(self.dir.clone());
                } else {
                    for d in &dirs {
                        self.watch_dir(w, d, cap);
                    }
                    if self.dirs.is_empty() {
                        return false;
                    }
                    self.watch_tracked_dirs(w);
                }
                self.dirs.len()
            }
        };
        self.build_ignores();
        // Git state outside the walked tree (a linked worktree's git dir, `.git` itself in per-folder mode).
        let recursive_covers = |p: &Path| self.mode == Mode::Recursive && p.starts_with(&self.dir);
        if !recursive_covers(&self.git_dir) {
            let _ = w.watch(&self.git_dir, RecursiveMode::NonRecursive);
        }
        if !recursive_covers(&self.common) {
            if self.common != self.git_dir {
                let _ = w.watch(&self.common, RecursiveMode::NonRecursive);
            }
            let _ = w.watch(&self.common.join("refs"), RecursiveMode::Recursive);
        }
        debug::record(|| {
            Entry::new(Kind::Watch, "folder watch started").detail(format!(
                "{:?}, folders: {watched}{}",
                self.mode,
                if self.capped { ", capped" } else { "" }
            ))
        });
        true
    }

    fn fall_back(&mut self, cap: usize) {
        self.capped = true;
        debug::record(|| {
            Entry::new(Kind::Watch, "folder watch capped").detail(format!(
                "more than {cap} folders: watching the top folder and git state only"
            ))
        });
    }

    fn watch_dir(&mut self, w: &mut RecommendedWatcher, d: &Path, cap: usize) {
        if self.capped || self.dirs.contains(d) {
            return;
        }
        if self.dirs.len() >= cap || w.watch(d, RecursiveMode::NonRecursive).is_err() {
            // Out of the cap or of the OS's watches (inotify's max_user_watches): stop adding.
            self.fall_back(cap);
            return;
        }
        self.dirs.insert(d.to_owned());
    }

    /// Takes one event: `true` when it may change what the app shows.
    fn handle(&mut self, e: notify::Result<notify::Event>, w: &mut RecommendedWatcher) -> bool {
        let e = match e {
            Ok(e) => e,
            // A dropped or overflowed queue: anything may have changed.
            Err(_) => return true,
        };
        if e.need_rescan() {
            return true;
        }
        if matches!(e.kind, EventKind::Access(_)) {
            return false;
        }
        let new_dir = matches!(
            e.kind,
            EventKind::Create(CreateKind::Folder | CreateKind::Any)
                | EventKind::Modify(ModifyKind::Name(_))
        );
        let mut hit = false;
        let mut rebuild = false;
        for p in &e.paths {
            if !self.relevant(p) {
                continue;
            }
            hit = true;
            if p.strip_prefix(&self.git_dir)
                .is_ok_and(|r| r == Path::new("index"))
            {
                self.index_moved = true;
            }
            if p.file_name().is_some_and(|n| n == ".gitignore") && p.starts_with(&self.dir) {
                if !self.nested.contains(p) {
                    self.nested.push(p.clone());
                }
                rebuild = true;
            }
            if let Mode::PerDir { cap } = self.mode
                && new_dir
                && p.is_dir()
                && p.starts_with(&self.dir)
            {
                for d in walk(p, cap).0 {
                    self.watch_dir(w, &d, cap);
                }
            }
        }
        if rebuild {
            self.build_ignores();
        }
        hit
    }
}

/// Tracked files under `toplevel` that an ignore pattern matches, as absolute paths.
fn tracked_ignored(toplevel: &Path) -> HashSet<PathBuf> {
    let args = [
        "ls-files",
        "-z",
        "--cached",
        "--ignored",
        "--exclude-standard",
    ];
    let out = crate::git(toplevel, &args).unwrap_or_default();
    out.split(|b| *b == 0)
        .filter(|p| !p.is_empty())
        .map(|p| toplevel.join(String::from_utf8_lossy(p).as_ref()))
        .collect()
}

/// The folders under `root` git can see (gitignored ones and `.git` skipped), up to `cap`, and the `.gitignore`
/// files in them. The flag says the walk stopped at the cap.
fn walk(root: &Path, cap: usize) -> (Vec<PathBuf>, Vec<PathBuf>, bool) {
    let (mut dirs, mut ignores) = (Vec::new(), Vec::new());
    let walker = ignore::WalkBuilder::new(root)
        .hidden(false)
        .parents(true)
        .filter_entry(|e| e.file_name() != ".git")
        .build();
    for entry in walker.flatten() {
        let Some(ft) = entry.file_type() else {
            continue;
        };
        if ft.is_dir() {
            if dirs.len() == cap {
                return (dirs, ignores, true);
            }
            dirs.push(entry.into_path());
        } else if entry.file_name() == ".gitignore" {
            ignores.push(entry.into_path());
        }
    }
    (dirs, ignores, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(dir: &str, git_dir: &str, common: &str) -> State {
        State {
            dir: dir.into(),
            toplevel: dir.into(),
            git_dir: git_dir.into(),
            common: common.into(),
            mode: Mode::Recursive,
            nested: Vec::new(),
            ignores: Vec::new(),
            tracked_ignored: HashSet::new(),
            index_moved: false,
            dirs: HashSet::new(),
            capped: false,
        }
    }

    #[test]
    fn git_state_that_counts() {
        let s = state("/r", "/r/.git", "/r/.git");
        for (p, want) in [
            ("/r/.git/HEAD", true),
            ("/r/.git/index", true),
            ("/r/.git/index.lock", false),
            ("/r/.git/refs/heads/main", true),
            ("/r/.git/refs/heads/main.lock", false),
            ("/r/.git/packed-refs", true),
            ("/r/.git/refs/skyelab/checkpoints/k/1", false),
            ("/r/.git/objects/ab/cdef", false),
            ("/r/.git/logs/HEAD", false),
            ("/r/.git/worktrees/wt/HEAD", false),
            ("/r/src/a.rs", true),
            ("/r/vendor/x/.git/HEAD", false),
            ("/elsewhere/a.rs", false),
        ] {
            assert_eq!(s.relevant(Path::new(p)), want, "{p}");
        }
    }

    #[test]
    fn a_linked_worktree_reads_its_own_head_and_the_shared_refs() {
        let s = state("/wt", "/r/.git/worktrees/wt", "/r/.git");
        for (p, want) in [
            ("/r/.git/worktrees/wt/HEAD", true),
            ("/r/.git/worktrees/wt/index", true),
            ("/r/.git/worktrees/wt/logs/HEAD", false),
            ("/r/.git/HEAD", false),
            ("/r/.git/index", false),
            ("/r/.git/refs/heads/x", true),
            ("/r/src/a.rs", false),
            ("/wt/a.rs", true),
        ] {
            assert_eq!(s.relevant(Path::new(p)), want, "{p}");
        }
    }
}
