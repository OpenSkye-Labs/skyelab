//! Installing a newer release over this one (T3's Download → Restart to install). [`prepare`] downloads the asset
//! for this install, checks its SHA-256 against the release's `SHA256SUMS` (on macOS also its code signature) and
//! stages it; nothing running changes until the user clicks Restart. Then [`recheck`] verifies the staged copy again,
//! [`swap`] puts it in place (rolling back if it can't) and [`finish`] starts what relaunches it once this process
//! has quit.
//! Every child starts from `env_clear()` plus the env allowlist, with tools at absolute paths.

use crate::update::{Asset, CHECKSUMS, RELEASES_PAGE, asset_url_ok, expected_sha256};
use anyhow::{Context as _, bail};
use std::ffi::OsStr;
use std::fs;
use std::io::{self, Read as _, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

mod mac;
mod windows;
pub use mac::{Signature, installed_team, signature_policy, team_id};
use mac::{check_app, stage_app};
pub use windows::{
    Authenticode, FAILURE_MARKER, authenticode_policy, installer_command, parse_authenticode,
    parse_failure,
};

/// How this copy of SkyeLab was installed, which decides what an update downloads and how it lands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Install {
    /// The running `.app` bundle (macOS): replaced from the release's `.dmg`.
    MacApp(PathBuf),
    /// An NSIS install (Windows), at this exe: the release's `-setup.exe` runs silently over it.
    Nsis(PathBuf),
    /// The running AppImage (`$APPIMAGE`): replaced in place.
    AppImage(PathBuf),
    /// A `.deb` install: the package is downloaded and shown in its folder for the user to install.
    Deb,
}

/// A verified download, ready for the Restart click.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Staged {
    /// A new bundle or AppImage next to `target`, to [`swap`] in. `sha256`: the AppImage's (a bundle is checked
    /// by its signature instead).
    Swap {
        new: PathBuf,
        target: PathBuf,
        sha256: Option<String>,
    },
    /// The installer for release `tag` to run once this process has quit, then the app at `exe`; a failure is
    /// written to `marker` for the next start.
    Installer {
        installer: PathBuf,
        exe: PathBuf,
        sha256: String,
        tag: String,
        marker: PathBuf,
    },
    /// A package the user installs themselves: shown in its folder.
    Folder(PathBuf),
}

/// How this copy was installed, from its exe path, `$APPIMAGE` and `os` (`std::env::consts::OS`); `None` for a
/// dev build or anything else that can't update itself (Download then opens the release page).
pub fn detect(exe: &Path, appimage: Option<&Path>, os: &str) -> Option<Install> {
    match os {
        "macos" => exe
            .ancestors()
            .nth(3)
            .filter(|app| app.extension() == Some(OsStr::new("app")))
            .filter(|app| exe.parent() == Some(&app.join("Contents").join("MacOS")))
            .map(|app| Install::MacApp(app.to_owned())),
        // cargo-packager's NSIS install puts its uninstaller next to the exe; a dev build has none.
        "windows" => exe
            .with_file_name("uninstall.exe")
            .is_file()
            .then(|| Install::Nsis(exe.to_owned())),
        // `$APPIMAGE` is inherited by everything an AppImage starts: it names this copy only when this exe runs
        // from an AppImage mount (`/tmp/.mount_*`).
        "linux" => match appimage {
            Some(p) if p.is_absolute() && from_appimage_mount(exe) => {
                Some(Install::AppImage(p.to_owned()))
            }
            _ => (exe == Path::new("/usr/bin/skyelab")).then_some(Install::Deb),
        },
        _ => None,
    }
}

fn from_appimage_mount(exe: &Path) -> bool {
    exe.ancestors().any(|a| {
        a.file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with(".mount_"))
    })
}

impl Install {
    fn suffix(&self) -> &'static str {
        match self {
            Install::MacApp(_) => ".dmg",
            Install::Nsis(_) => "-setup.exe",
            Install::AppImage(_) => ".AppImage",
            Install::Deb => ".deb",
        }
    }
}

/// Whether `a` is release `tag`'s own download: a plain file name at exactly this repo's `download/<tag>/<name>`
/// (an asset of an older release would be a downgrade).
fn of_release(a: &Asset, tag: &str) -> bool {
    plain_name(&a.name)
        && asset_url_ok(&a.url)
        && a.url == format!("{RELEASES_PAGE}download/{tag}/{}", a.name)
}

/// Release `tag`'s asset for `install` on `arch` (`std::env::consts::ARCH`): its kind's suffix and this arch as the
/// last `_` part (`SkyeLab_0.3.0_aarch64.dmg`, `_x64-setup.exe`, `_amd64.deb`).
pub fn pick<'a>(
    assets: &'a [Asset],
    tag: &str,
    install: &Install,
    arch: &str,
) -> Option<&'a Asset> {
    let tokens: &[&str] = match arch {
        "aarch64" => &["aarch64", "arm64"],
        "x86_64" => &["x86_64", "x64", "amd64"],
        _ => return None,
    };
    let suffix = install.suffix();
    assets.iter().find(|a| {
        of_release(a, tag)
            && a.name
                .strip_suffix(suffix)
                .is_some_and(|stem| tokens.iter().any(|t| stem.ends_with(&format!("_{t}"))))
    })
}

fn plain_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && !name.starts_with('.')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-._+".contains(c))
}

/// Streams a URL into a writer, reporting (bytes so far, total if known).
pub type Get<'a> =
    dyn FnMut(&str, &mut dyn Write, &mut dyn FnMut(u64, Option<u64>)) -> io::Result<()> + 'a;

/// `SHA256SUMS` is a few lines; anything past this is refused.
const SUMS_MAX: usize = 64 * 1024;

/// A writer that refuses to grow past its cap.
struct Capped(Vec<u8>);

impl Write for Capped {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.0.len() + buf.len() > SUMS_MAX {
            return Err(io::Error::other("SHA256SUMS is too large"));
        }
        self.0.extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Downloads release `tag`'s asset for this install into `dir`, checks it against that release's `SHA256SUMS` and
/// stages it. Nothing that runs is touched; on failure nothing it downloaded is left behind.
pub fn prepare(
    install: &Install,
    assets: &[Asset],
    tag: &str,
    arch: &str,
    dir: &Path,
    get: &mut Get,
    progress: &mut dyn FnMut(u64, Option<u64>),
) -> anyhow::Result<Staged> {
    let asset = pick(assets, tag, install, arch)
        .context("this release has no download for this computer")?;
    let sums = assets
        .iter()
        .find(|a| a.name == CHECKSUMS && of_release(a, tag))
        .context("this release has no SHA256SUMS, so its download can't be checked")?;
    let mut listing = Capped(Vec::new());
    get(&sums.url, &mut listing, &mut |_, _| {}).context("couldn't download SHA256SUMS")?;
    let want = std::str::from_utf8(&listing.0)
        .ok()
        .and_then(|s| expected_sha256(s, &asset.name))
        .with_context(|| format!("SHA256SUMS doesn't list {}", asset.name))?;
    fs::create_dir_all(dir).with_context(|| format!("couldn't create {}", dir.display()))?;
    let file = dir.join(&asset.name);
    let part = dir.join(format!("{}.part", asset.name));
    let fetched = (|| -> anyhow::Result<()> {
        let mut out = fresh_file(&part).context("couldn't save the download")?;
        get(&asset.url, &mut out, progress).context("the download failed")?;
        out.sync_all()?;
        // Downloaded: the notice moves on to "Checking" even when the server sent no length.
        let len = out.metadata()?.len();
        progress(len, Some(len));
        if sha256_file(&part)? != want {
            bail!("the download doesn't match the release's checksum");
        }
        remove_all(&file);
        fs::rename(&part, &file).context("couldn't save the download")?;
        Ok(())
    })();
    if let Err(e) = fetched {
        remove_all(&part);
        return Err(e);
    }
    let staged = stage(install, &file, &want, tag);
    if staged.is_err() {
        remove_all(&file);
    }
    staged
}

/// Creates `path` new, never through whatever was there (a planted symlink is removed, not followed).
fn fresh_file(path: &Path) -> io::Result<fs::File> {
    remove_all(path);
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
}

/// Lowercase hex SHA-256 of a file.
pub fn sha256_file(path: &Path) -> io::Result<String> {
    let mut f = fs::File::open(path)?;
    let mut ctx = ring::digest::Context::new(&ring::digest::SHA256);
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        ctx.update(&buf[..n]);
    }
    Ok(ctx
        .finish()
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

/// A hidden name next to `target` (same folder, so the swap is a rename on one volume).
fn beside(target: &Path, tag: &str) -> anyhow::Result<PathBuf> {
    let name = target.file_name().context("the install has no file name")?;
    Ok(target.with_file_name(format!(".{}.{tag}", name.to_string_lossy())))
}

fn stage(install: &Install, file: &Path, sha256: &str, tag: &str) -> anyhow::Result<Staged> {
    match install {
        Install::MacApp(target) => Ok(Staged::Swap {
            new: stage_app(file, target)?,
            target: target.clone(),
            sha256: None,
        }),
        Install::Nsis(exe) => {
            windows::check_installer(exe, file)?;
            Ok(Staged::Installer {
                installer: file.to_owned(),
                exe: exe.clone(),
                sha256: sha256.to_owned(),
                tag: tag.to_owned(),
                marker: file.with_file_name(FAILURE_MARKER),
            })
        }
        Install::AppImage(target) => {
            let new = beside(target, "update")?;
            let copied = (|| -> io::Result<()> {
                let mut out = fresh_file(&new)?;
                io::copy(&mut fs::File::open(file)?, &mut out)?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt as _;
                    out.set_permissions(fs::Permissions::from_mode(0o755))?;
                }
                out.sync_all()
            })();
            if let Err(e) = copied {
                remove_all(&new);
                return Err(e)
                    .with_context(|| format!("couldn't write next to {}", target.display()));
            }
            let _ = fs::remove_file(file);
            Ok(Staged::Swap {
                new,
                target: target.clone(),
                sha256: Some(sha256.to_owned()),
            })
        }
        Install::Deb => Ok(Staged::Folder(file.to_owned())),
    }
}

fn remove_all(path: &Path) {
    let _ = match fs::symlink_metadata(path) {
        Ok(m) if m.is_dir() => fs::remove_dir_all(path),
        Ok(_) => fs::remove_file(path),
        Err(_) => Ok(()),
    };
}

/// Puts `new` at `target`: `target` moves aside, `new` takes its place, the old copy is removed. If `new` can't move
/// in, `target` is put back, so a failed swap leaves the old app as it was.
pub fn swap(new: &Path, target: &Path) -> anyhow::Result<()> {
    let old = beside(target, "old")?;
    fs::symlink_metadata(target)
        .with_context(|| format!("{} isn't there to replace", target.display()))?;
    remove_all(&old);
    fs::rename(target, &old)
        .with_context(|| format!("couldn't move {} aside", target.display()))?;
    if let Err(e) = fs::rename(new, target) {
        fs::rename(&old, target).with_context(|| {
            format!(
                "couldn't put {} back; it is at {}",
                target.display(),
                old.display()
            )
        })?;
        return Err(e).context("couldn't move the update into place");
    }
    remove_all(&old);
    Ok(())
}

/// Checks the staged update again right before [`swap`] or [`finish`]: it waited for the Restart click beside the
/// app, so it is verified as it is now (an AppImage or installer by its SHA-256, a bundle by its signature).
pub fn recheck(staged: &Staged) -> anyhow::Result<()> {
    let (file, want) = match staged {
        Staged::Folder(_) => return Ok(()),
        Staged::Swap {
            new,
            target,
            sha256: None,
        } => return check_app(target, new),
        Staged::Swap {
            new,
            sha256: Some(want),
            ..
        } => (new, want),
        Staged::Installer {
            installer, sha256, ..
        } => (installer, sha256),
    };
    if &sha256_file(file).context("the staged update is gone")? != want {
        bail!("the staged update changed since it was checked");
    }
    Ok(())
}

/// Starts what finishes the update once this process (`pid`) has exited: the new app (macOS, AppImage), or the
/// installer and then the app (Windows). The caller quits right after. A [`Staged::Folder`] has nothing to run.
pub fn finish(staged: &Staged, pid: u32) -> io::Result<()> {
    let clean = |program: &Path| {
        let mut cmd = Command::new(program);
        cmd.env_clear().envs(crate::env::terminal_vars(
            std::env::vars_os(),
            cfg!(windows),
        ));
        cmd
    };
    let mut cmd = match staged {
        Staged::Folder(_) => return Ok(()),
        Staged::Swap { target, .. } => {
            let mut cmd = clean(Path::new("/bin/sh"));
            // Waits at most a minute (a zombie parent would answer `kill -0` forever).
            cmd.arg("-c")
                .arg(r#"pid=$1; shift; i=0; while kill -0 "$pid" 2>/dev/null && [ $i -lt 300 ]; do /bin/sleep 0.2; i=$((i+1)); done; exec "$@""#)
                .arg("sh")
                .arg(pid.to_string());
            if target.extension() == Some(OsStr::new("app")) {
                cmd.args(["/usr/bin/open", "-n"]);
            }
            cmd.arg(target);
            cmd
        }
        Staged::Installer {
            installer,
            exe,
            tag,
            marker,
            ..
        } => installer_command(installer, exe, pid, marker, tag),
    };
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    crate::process::spawn(&mut cmd).map(drop)
}
