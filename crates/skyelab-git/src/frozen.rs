//! The repo's own config frozen for one git call. Filters and merge drivers have names the repo's config picks, so
//! [`crate::program_overrides`] can only switch off the ones it listed before git starts: one written between that
//! listing and git's own read would run. Here git never reads the repo's config at all. SkyeLab lists it once
//! (local and worktree scope, includes expanded), drops every program key and every include from that listing,
//! and writes the rest to a private common dir (`GIT_COMMON_DIR`). Refs and objects still come from the real common
//! dir (`GIT_REFERENCE_BACKEND`, git 2.54+, and `GIT_OBJECT_DIRECTORY`), so commits, stashes and branch moves land
//! in the repo as usual. Older git falls back to the listing, race included.

use std::borrow::{Borrow, BorrowMut};
use std::ffi::OsString;
use std::io;
use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use skyelab_core::process::Captured;

use crate::{Result, finish, hooked_command, lossy, spawn_error};

/// A git command and the frozen config it reads, which must outlive the child: keep this until it has exited.
pub(crate) struct Git {
    cmd: Command,
    _frozen: Option<tempfile::TempDir>,
}

impl Deref for Git {
    type Target = Command;
    fn deref(&self) -> &Command {
        &self.cmd
    }
}

impl DerefMut for Git {
    fn deref_mut(&mut self) -> &mut Command {
        &mut self.cmd
    }
}

impl Borrow<Command> for Git {
    fn borrow(&self) -> &Command {
        &self.cmd
    }
}

impl BorrowMut<Command> for Git {
    fn borrow_mut(&mut self) -> &mut Command {
        &mut self.cmd
    }
}

impl Git {
    /// A command that needs no frozen config (gh, or git with the repo's hooks on).
    pub(crate) fn plain(cmd: Command) -> Self {
        Self { cmd, _frozen: None }
    }
}

/// What a git child (SkyeLab's own, or the one gh starts) needs to run on the frozen config: `-c` overrides and env.
pub(crate) struct Frozen {
    pub(crate) overrides: Vec<(String, String)>,
    pub(crate) env: Vec<(&'static str, OsString)>,
    dir: Option<tempfile::TempDir>,
}

impl Frozen {
    /// Applies the overrides as `-c` args and the env to `cmd`, which then must not outlive the result.
    pub(crate) fn apply(self, mut cmd: Command) -> Git {
        for (key, value) in &self.overrides {
            cmd.arg("-c").arg(format!("{key}={value}"));
        }
        cmd.envs(self.env.iter().map(|(k, v)| (k, v)));
        Git {
            cmd,
            _frozen: self.dir,
        }
    }

    /// Keeps the frozen config alive as long as the returned guard.
    pub(crate) fn into_guard(self) -> Option<tempfile::TempDir> {
        self.dir
    }
}

/// [`hooked_command`] on the repo's config frozen as it is now, with every program it names off.
pub(crate) fn frozen_command(bin: &Path, dir: &Path) -> Result<Git> {
    Ok(freeze(bin, dir)?.apply(hooked_command(bin, dir)))
}

/// Freezes the config of the repo at `dir` (see the module docs).
pub(crate) fn freeze(bin: &Path, dir: &Path) -> Result<Frozen> {
    if !backend_dir_supported(bin) {
        return Ok(Frozen {
            overrides: crate::program_overrides(bin, dir)?,
            env: Vec::new(),
            dir: None,
        });
    }
    // Both at once: each is a process start on every git call.
    let common_args = ["rev-parse", "--path-format=absolute", "--git-common-dir"];
    let list_args = ["config", "--list", "--show-scope", "-z", "--includes"];
    let common = start(bin, dir, &common_args)?;
    let listing = start(bin, dir, &list_args)?;
    let common = finish(dir, &common_args, common.wait()?)?;
    let listing = finish(dir, &list_args, listing.wait()?)?;
    let common = PathBuf::from(lossy(&common).trim_end_matches('\n'));
    let entries = parse(&listing);
    let tmp = tempfile::Builder::new().prefix("skyelab-git-").tempdir()?;
    write_common_dir(tmp.path(), &common, &entries)?;
    // The trusted scopes' program keys, for the fixed-name ones to keep the user's own value.
    let mut trusted = Vec::new();
    for (scope, key, value) in &entries {
        if matches!(*scope, b"system" | b"global") && is_program_key(key) {
            trusted.extend_from_slice(scope);
            trusted.push(0);
            trusted.extend_from_slice(key);
            if let Some(v) = value {
                trusted.push(b'\n');
                trusted.extend_from_slice(v);
            }
            trusted.push(0);
        }
    }
    let format = entries
        .iter()
        .rev()
        .find(|(s, k, _)| local(s) && *k == b"extensions.refstorage")
        .and_then(|(_, _, v)| *v)
        .map(lossy)
        .unwrap_or_else(|| "files".to_owned());
    // A bare format name stores refs in the payload dir; a full URI already names where they live.
    let backend = if format.contains("://") {
        OsString::from(format)
    } else {
        let mut uri = OsString::from(format!("{format}://"));
        uri.push(&common);
        uri
    };
    Ok(Frozen {
        overrides: crate::overrides_from(&trusted)?,
        env: vec![
            ("GIT_COMMON_DIR", tmp.path().into()),
            ("GIT_OBJECT_DIRECTORY", common.join("objects").into()),
            ("GIT_REFERENCE_BACKEND", backend),
        ],
        dir: Some(tmp),
    })
}

fn start(bin: &Path, dir: &Path, args: &[&str]) -> Result<Captured> {
    Captured::spawn(hooked_command(bin, dir).args(args), None).map_err(spawn_error)
}

/// Git 2.54 added the ref store dir to `GIT_REFERENCE_BACKEND`; older git would read refs from the private dir.
fn backend_dir_supported(bin: &Path) -> bool {
    static SUPPORTED: OnceLock<bool> = OnceLock::new();
    *SUPPORTED.get_or_init(|| {
        let mut cmd = hooked_command(bin, Path::new("."));
        skyelab_core::process::output(cmd.arg("version"), None)
            .ok()
            .is_some_and(|out| version_at_least(&lossy(&out.stdout), (2, 54)))
    })
}

/// `git version 2.54.0 (Apple Git-157)`, `git version 2.56.0.windows.1`: at least `min`.
pub(crate) fn version_at_least(text: &str, min: (u32, u32)) -> bool {
    let mut parts = text
        .trim()
        .strip_prefix("git version ")
        .unwrap_or_default()
        .split('.')
        .map(|p| p.parse::<u32>().ok());
    match (parts.next().flatten(), parts.next().flatten()) {
        (Some(major), Some(minor)) => (major, minor) >= min,
        _ => false,
    }
}

type Entry<'a> = (&'a [u8], &'a [u8], Option<&'a [u8]>);

/// `git config --list --show-scope -z`: `scope\0key\nvalue\0`, or `scope\0key\0` for a key set with no value.
fn parse(listing: &[u8]) -> Vec<Entry<'_>> {
    let mut fields = listing.split(|b| *b == 0);
    let mut out = Vec::new();
    while let (Some(scope), Some(entry)) = (fields.next(), fields.next()) {
        match entry.iter().position(|b| *b == b'\n') {
            Some(i) => out.push((scope, &entry[..i], Some(&entry[i + 1..]))),
            None => out.push((scope, entry, None)),
        }
    }
    out
}

fn local(scope: &[u8]) -> bool {
    matches!(scope, b"local" | b"worktree")
}

/// A key that names a program git runs (the listing's `PROGRAM_KEYS`). Keys come lowercased but for the subsection.
pub(crate) fn is_program_key(key: &[u8]) -> bool {
    let key = String::from_utf8_lossy(key);
    let (section, rest) = key.split_once('.').unwrap_or((&key, ""));
    let (sub, name) = rest
        .rsplit_once('.')
        .map_or((None, rest), |(s, n)| (Some(s), n));
    match section {
        "hook" => sub.is_some() && name == "command",
        "core" => name == "sshcommand" && sub.is_none(),
        "gpg" => name == "program" || key == "gpg.ssh.defaultkeycommand",
        "filter" => sub.is_some() && matches!(name, "clean" | "smudge" | "process"),
        "merge" => sub.is_some() && name == "driver",
        _ => false,
    }
}

/// Keys the frozen config must not keep: a program, every hook key (an event left without its command is an error,
/// and hooks are off anyway), an include git would read live (the listing already expanded it), and the
/// worktree-config switch that makes git read the live `config.worktree` (its entries are in the listing too). Also
/// the transport programs a lazy fetch (a partial clone's missing object, on any read) would run: a remote's
/// upload-pack (through sh for a local path), receive-pack and `vcs` helper, and `core.alternateRefsCommand`.
fn dropped(key: &[u8]) -> bool {
    let remote_program = key.starts_with(b"remote.")
        && [&b".uploadpack"[..], b".receivepack", b".vcs"]
            .iter()
            .any(|end| key.ends_with(end));
    is_program_key(key)
        || remote_program
        || key == b"core.alternaterefscommand"
        || key.starts_with(b"hook.")
        || key.starts_with(b"include.")
        || key.starts_with(b"includeif.")
        || key == b"extensions.worktreeconfig"
}

/// The private common dir: the frozen config, empty `refs`/`objects` (git checks they exist), and copies of the
/// common files git reads that aren't refs or objects (ignore and attribute rules, shallow history).
fn write_common_dir(to: &Path, common: &Path, entries: &[Entry<'_>]) -> io::Result<()> {
    std::fs::create_dir(to.join("refs"))?;
    std::fs::create_dir(to.join("objects"))?;
    std::fs::create_dir(to.join("info"))?;
    let frozen = entries
        .iter()
        .filter(|(scope, key, _)| local(scope) && !dropped(key))
        .map(|(_, key, value)| (*key, *value));
    std::fs::write(to.join("config"), config_text(frozen))?;
    for file in ["info/exclude", "info/attributes", "shallow"] {
        match std::fs::copy(common.join(file), to.join(file)) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            r => {
                r?;
            }
        }
    }
    Ok(())
}

/// `entries` as a config file, one section header per entry so multi-valued keys keep their order.
pub(crate) fn config_text<'a>(
    entries: impl Iterator<Item = (&'a [u8], Option<&'a [u8]>)>,
) -> Vec<u8> {
    let mut out = Vec::new();
    for (key, value) in entries {
        let Some(dot) = key.iter().position(|b| *b == b'.') else {
            continue;
        };
        let (section, rest) = (&key[..dot], &key[dot + 1..]);
        out.push(b'[');
        out.extend_from_slice(section);
        let name = match rest.iter().rposition(|b| *b == b'.') {
            Some(i) => {
                out.extend_from_slice(b" \"");
                escape(&mut out, &rest[..i]);
                out.push(b'"');
                &rest[i + 1..]
            }
            None => rest,
        };
        out.extend_from_slice(b"]\n\t");
        out.extend_from_slice(name);
        if let Some(value) = value {
            out.extend_from_slice(b" = \"");
            escape(&mut out, value);
            out.push(b'"');
        }
        out.push(b'\n');
    }
    out
}

fn escape(out: &mut Vec<u8>, text: &[u8]) {
    for &b in text {
        match b {
            b'\\' => out.extend_from_slice(b"\\\\"),
            b'"' => out.extend_from_slice(b"\\\""),
            b'\n' => out.extend_from_slice(b"\\n"),
            b'\t' => out.extend_from_slice(b"\\t"),
            8 => out.extend_from_slice(b"\\b"),
            _ => out.push(b),
        }
    }
}
