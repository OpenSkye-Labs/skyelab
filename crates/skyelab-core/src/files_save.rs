//! The Preview tab's save (docs/decisions/files.md). The thread's folder belongs to the agent too: it can plant a
//! symlink where a temp file would go and swap a folder of the path for a symlink while the save runs. On Unix every
//! folder below the root is opened without following links, and the temp file is created (exclusively, with a random
//! name, owner-only) and renamed relative to that folder handle, so neither can redirect the write.

use super::inside;
use anyhow::{Context as _, Result};
use std::path::Path;

/// What [`save_text`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Saved {
    Written,
    /// The file on disk is no longer what was loaded (an agent or git wrote it): nothing was written.
    Changed,
}

const NOT_INSIDE: &str = "Not a file in this thread's folder.";

/// Writes `text` over the file `path` of `root` (relative, or absolute under it), through a temp file and a rename,
/// keeping its permissions. Refused unless it resolves to an existing file inside `root` ([`inside`]: `..`, foreign
/// roots and symlinks pointing out are refused). With `base` (the text loaded), a file that no longer holds exactly
/// that is left alone and [`Saved::Changed`] comes back; `None` overwrites.
pub fn save_text(root: &Path, path: &Path, text: &str, base: Option<&str>) -> Result<Saved> {
    let full = inside(root, path).context(NOT_INSIDE)?;
    let root = root.canonicalize().context(NOT_INSIDE)?;
    let rel = full.strip_prefix(&root).context(NOT_INSIDE)?;
    imp::save(&root, rel, text.as_bytes(), base.map(str::as_bytes))
}

#[cfg(unix)]
mod imp {
    use super::{NOT_INSIDE, Saved};
    use anyhow::{Context as _, Result, bail};
    use rustix::fd::OwnedFd;
    use rustix::fs::{AtFlags, Mode, OFlags};
    use std::io::{Read as _, Write as _};
    use std::path::{Component, Path};

    const DIR: OFlags = OFlags::DIRECTORY
        .union(OFlags::RDONLY)
        .union(OFlags::CLOEXEC);

    /// `rel`'s folder, each step below `root` opened without following a link.
    fn open_parent(root: &Path, rel: &Path) -> Result<OwnedFd> {
        let mut dir = rustix::fs::open(root, DIR, Mode::empty()).context(NOT_INSIDE)?;
        for c in rel.parent().into_iter().flat_map(Path::components) {
            let Component::Normal(name) = c else {
                bail!(NOT_INSIDE)
            };
            dir = rustix::fs::openat(&dir, name, DIR | OFlags::NOFOLLOW, Mode::empty())
                .context(NOT_INSIDE)?;
        }
        Ok(dir)
    }

    pub(super) fn save(
        root: &Path,
        rel: &Path,
        bytes: &[u8],
        base: Option<&[u8]>,
    ) -> Result<Saved> {
        let name = rel.file_name().context(NOT_INSIDE)?;
        let dir = open_parent(root, rel)?;
        let file = rustix::fs::openat(
            &dir,
            name,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .context(NOT_INSIDE)?;
        let stat = rustix::fs::fstat(&file).context("Can't read this file.")?;
        if !rustix::fs::FileType::from_raw_mode(stat.st_mode).is_file() {
            bail!("Not a file.");
        }
        if let Some(base) = base {
            let mut now = Vec::new();
            std::fs::File::from(file)
                .read_to_end(&mut now)
                .context("Can't read this file.")?;
            if now != base {
                return Ok(Saved::Changed);
            }
        }
        let (tmp, fd) = create_temp(&dir, name)?;
        let written = (|| {
            let mut out = std::fs::File::from(fd);
            out.write_all(bytes)?;
            rustix::fs::fchmod(&out, Mode::from_raw_mode(stat.st_mode & 0o7777))?;
            out.sync_all()?;
            rustix::fs::renameat(&dir, &tmp, &dir, name)?;
            std::io::Result::Ok(())
        })();
        if written.is_err() {
            let _ = rustix::fs::unlinkat(&dir, &tmp, AtFlags::empty()); // best effort; the file is untouched
        }
        written.context("Can't save this file.")?;
        Ok(Saved::Written)
    }

    /// A new owner-only `.<name>.<random>.tmp` in `dir`, never through anything already there.
    fn create_temp(dir: &OwnedFd, name: &std::ffi::OsStr) -> Result<(String, OwnedFd)> {
        use std::hash::{BuildHasher as _, Hasher as _};
        for attempt in 0..16u32 {
            let mut h = std::hash::RandomState::new().build_hasher();
            h.write_u32(attempt);
            h.write_u32(std::process::id());
            let tmp = format!(".{}.{:016x}.tmp", name.to_string_lossy(), h.finish());
            let flags =
                OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC;
            match rustix::fs::openat(dir, tmp.as_str(), flags, Mode::from_raw_mode(0o600)) {
                Ok(fd) => return Ok((tmp, fd)),
                Err(rustix::io::Errno::EXIST) => continue,
                Err(e) => return Err(e).context("Can't save this file."),
            }
        }
        bail!("Can't save this file.")
    }
}

#[cfg(not(unix))]
mod imp {
    use super::{NOT_INSIDE, Saved};
    use anyhow::{Context as _, Result, bail};
    use std::path::Path;

    // shortcut: Windows saves by path (a random, exclusive temp file, then a rename), so a folder swapped for a
    // junction between the check and the rename could redirect it; handle-relative writes need NtCreateFile.
    pub(super) fn save(
        root: &Path,
        rel: &Path,
        bytes: &[u8],
        base: Option<&[u8]>,
    ) -> Result<Saved> {
        let full = root.join(rel);
        if !std::fs::symlink_metadata(&full).is_ok_and(|m| m.is_file()) {
            bail!("Not a file.");
        }
        if let Some(base) = base
            && std::fs::read(&full).context("Can't read this file.")? != base
        {
            return Ok(Saved::Changed);
        }
        // Checked again right before the write: still this file, inside the folder.
        if super::inside(root, rel).as_deref() != Some(full.as_path()) {
            bail!(NOT_INSIDE);
        }
        crate::write_atomic::write_atomic_at(&full, bytes)?;
        Ok(Saved::Written)
    }
}

/// What a folder or file swapped for a link after [`inside`] passed looks like to the write: refused, nothing written.
#[cfg(all(test, unix))]
#[test]
fn the_write_itself_refuses_links_swapped_in_after_the_check() {
    use std::fs;
    let tmp = tempfile::tempdir().expect("temp dir");
    let base = fs::canonicalize(tmp.path()).expect("canonical");
    let (root, out) = (base.join("proj"), base.join("out"));
    fs::create_dir_all(&root).expect("root");
    fs::create_dir_all(&out).expect("out");
    fs::write(out.join("notes.md"), "keep").expect("victim");
    std::os::unix::fs::symlink(&out, root.join("sub")).expect("folder link");
    std::os::unix::fs::symlink(out.join("notes.md"), root.join("top.md")).expect("file link");
    for rel in ["sub/notes.md", "top.md"] {
        assert!(
            imp::save(&root, Path::new(rel), b"PWNED", None).is_err(),
            "{rel}"
        );
    }
    assert_eq!(
        fs::read_to_string(out.join("notes.md")).expect("read"),
        "keep"
    );
    assert_eq!(
        fs::read_dir(&out).expect("list").count(),
        1,
        "no temp file made outside"
    );
}
