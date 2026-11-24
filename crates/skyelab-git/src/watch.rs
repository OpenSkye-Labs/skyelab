//! Live folder watching: tells the app when a repo folder's status may have changed (a file written, a branch
//! switched, a commit), so the Changes tab and the change counts follow edits made outside SkyeLab.
//!
//! One thread per watched folder. It asks git where the repo's git dirs are, watches the folder and the git state,
//! drops events no status can show (gitignored paths, `.git` internals other than HEAD, the index and refs, our own
//! checkpoint refs, lock files) and calls back once per burst: after 300 ms without a new event, or 2 s into a
//! steady stream. A folder that isn't in a repo yet is watched for one to appear there (`git init`, a clone).

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

#[path = "watch_scan.rs"]
mod scan;
use scan::{tracked_ignored, walk};

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
    /// isn't in a git repo waits, watching only its own entries, until it is one, then is watched as any repo.
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

    /// Waits until the watches are in place: `true` once watching, `false` when it can't watch yet (not a repo), gave
    /// up, or `timeout` passed first.
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
    let config = Config::default().with_event_kinds(EventKindMask::CORE);
    let mut state = match State::open(dir, mode) {
        Some(s) => s,
        None => {
            set_ready(ready, false);
            let Some(s) = wait_for_repo(dir, mode, config.clone(), &tx, rx) else {
                return;
            };
            s
        }
    };
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

/// Watches `dir`'s own entries until it is in a git repo (read again once its events go quiet, so a `git init` has
/// finished); `None` when stopped first or `dir` can't be watched.
// shortcut: a repo made in a parent folder (`git init ..`) isn't noticed until the views open again; watch the
// parents too if that turns up.
fn wait_for_repo(
    dir: &Path,
    mode: Mode,
    config: Config,
    tx: &mpsc::Sender<Msg>,
    rx: &mpsc::Receiver<Msg>,
) -> Option<State> {
    let tx = tx.clone();
    let mut watcher = RecommendedWatcher::new(
        move |e| {
            let _ = tx.send(Msg::Event(e));
        },
        config,
    )
    .ok()?;
    watcher.watch(dir, RecursiveMode::NonRecursive).ok()?;
    // The first event since the last check: after `QUIET` without one, or `MAX_WAIT` into a stream, it checks.
    let mut first: Option<Instant> = None;
    loop {
        if first.is_some_and(|f| f.elapsed() >= MAX_WAIT) {
            first = None;
            if let Some(s) = State::open(dir, mode) {
                return Some(s);
            }
        }
        let msg = match first {
            None => rx.recv().ok(),
            Some(f) => {
                let wait = QUIET.min((f + MAX_WAIT).saturating_duration_since(Instant::now()));
                match rx.recv_timeout(wait) {
                    Ok(m) => Some(m),
                    Err(RecvTimeoutError::Timeout) => {
                        first = None;
                        if let Some(s) = State::open(dir, mode) {
                            return Some(s);
                        }
                        continue;
                    }
                    Err(RecvTimeoutError::Disconnected) => None,
                }
            }
        };
        match msg {
            None | Some(Msg::Stop) => return None,
            Some(Msg::Event(_)) => {
                first.get_or_insert_with(Instant::now);
            }
        }
    }
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
    /// Per-folder watches in place; a folder deleted or moved away leaves it.
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

    /// Folder `gone` and everything watched under it went (deleted, moved away): their watches leave the count.
    fn forget_dirs(&mut self, gone: &Path, w: &mut RecommendedWatcher) {
        self.dirs.retain(|d| {
            let keep = !d.starts_with(gone);
            if !keep {
                // The OS may have dropped it already (inotify does on delete).
                let _ = w.unwatch(d);
            }
            keep
        });
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
        if matches!(self.mode, Mode::PerDir { .. }) {
            for p in &e.paths {
                if self.dirs.contains(p) && !p.exists() {
                    self.forget_dirs(p, w);
                }
            }
        }
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
                // Gone and back before its Remove was read (`rm -rf a && mkdir a`): its old watch died with it.
                if self.dirs.contains(p) {
                    self.forget_dirs(p, w);
                }
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

#[cfg(test)]
#[path = "watch_tests.rs"]
mod tests;
