use crate::update::{Asset, CHECKSUMS, RELEASES_PAGE};
use crate::update_install::*;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

fn asset(name: &str) -> Asset {
    Asset {
        name: name.into(),
        url: format!("{RELEASES_PAGE}download/v0.3.0/{name}"),
    }
}

fn release() -> Vec<Asset> {
    [
        "SkyeLab_0.3.0_aarch64.dmg",
        "SkyeLab_0.3.0_x64-setup.exe",
        "skyelab_0.3.0_amd64.deb",
        "skyelab_0.3.0_x86_64.AppImage",
        CHECKSUMS,
    ]
    .into_iter()
    .map(asset)
    .collect()
}

#[test]
fn picks_the_asset_for_each_platform_and_arch() {
    let all = release();
    let name = |i: &Install, arch| pick(&all, "v0.3.0", i, arch).map(|a| a.name.as_str());
    let app = Install::MacApp("/Applications/SkyeLab.app".into());
    assert_eq!(name(&app, "aarch64"), Some("SkyeLab_0.3.0_aarch64.dmg"));
    assert_eq!(name(&app, "x86_64"), None, "no Intel dmg is published");
    assert_eq!(
        name(&Install::Nsis("C:\\SkyeLab\\skyelab.exe".into()), "x86_64"),
        Some("SkyeLab_0.3.0_x64-setup.exe")
    );
    assert_eq!(name(&Install::Nsis("x".into()), "aarch64"), None);
    assert_eq!(
        name(
            &Install::AppImage("/home/u/SkyeLab.AppImage".into()),
            "x86_64"
        ),
        Some("skyelab_0.3.0_x86_64.AppImage")
    );
    assert_eq!(
        name(&Install::Deb, "x86_64"),
        Some("skyelab_0.3.0_amd64.deb")
    );
    assert_eq!(name(&Install::Deb, "riscv64"), None);

    // A name that isn't a plain file name, or a URL elsewhere or not ending in the name, is never picked.
    let mut bad = asset("SkyeLab_0.3.0_aarch64.dmg");
    bad.url = "https://evil.example/download/v0.3.0/SkyeLab_0.3.0_aarch64.dmg".into();
    assert!(pick(&[bad], "v0.3.0", &app, "aarch64").is_none());
    let mut other = asset("SkyeLab_0.3.0_aarch64.dmg");
    other.url = format!("{RELEASES_PAGE}download/v0.3.0/Other_aarch64.dmg");
    assert!(pick(&[other], "v0.3.0", &app, "aarch64").is_none());
    assert!(pick(&[asset(".SkyeLab_aarch64.dmg")], "v0.3.0", &app, "aarch64").is_none());
}

#[test]
fn detects_how_it_was_installed() {
    let exe = Path::new("/Applications/SkyeLab.app/Contents/MacOS/skyelab");
    assert_eq!(
        detect(exe, None, "macos"),
        Some(Install::MacApp("/Applications/SkyeLab.app".into()))
    );
    assert_eq!(
        detect(Path::new("/w/target/debug/skyelab"), None, "macos"),
        None
    );
    let appimage = Path::new("/home/u/Apps/SkyeLab.AppImage");
    assert_eq!(
        detect(
            Path::new("/tmp/.mount_x/usr/bin/skyelab"),
            Some(appimage),
            "linux"
        ),
        Some(Install::AppImage(appimage.into()))
    );
    assert_eq!(
        detect(Path::new("/usr/bin/skyelab"), None, "linux"),
        Some(Install::Deb)
    );
    assert_eq!(
        detect(Path::new("/home/u/target/skyelab"), None, "linux"),
        None
    );

    let dir = tempfile::tempdir().expect("tempdir");
    let exe = dir.path().join("skyelab.exe");
    assert_eq!(detect(&exe, None, "windows"), None, "a dev build");
    fs::write(dir.path().join("uninstall.exe"), b"").expect("write");
    assert_eq!(
        detect(&exe, None, "windows"),
        Some(Install::Nsis(exe.clone()))
    );
}

/// A fake server: serves `files` by the URL's last segment.
fn serve(files: Vec<(&'static str, Vec<u8>)>) -> Box<Get<'static>> {
    Box::new(
        move |url: &str, out: &mut dyn Write, progress: &mut dyn FnMut(u64, Option<u64>)| {
            let name = url.rsplit('/').next().unwrap_or_default();
            let (_, body) = files
                .iter()
                .find(|(n, _)| *n == name)
                .ok_or_else(|| io::Error::other("404"))?;
            out.write_all(body)?;
            progress(body.len() as u64, Some(body.len() as u64));
            Ok(())
        },
    )
}

fn sums_for(name: &str, body: &[u8], dir: &Path) -> Vec<u8> {
    let probe = dir.join("probe");
    fs::write(&probe, body).expect("write");
    let hex = sha256_file(&probe).expect("hash");
    fs::remove_file(&probe).expect("rm");
    format!("{hex}  {name}\n").into_bytes()
}

#[test]
fn prepare_checks_the_checksum_before_staging() {
    let dir = tempfile::tempdir().expect("tempdir");
    let deb = "skyelab_0.3.0_amd64.deb";
    let body = b"package bytes".to_vec();
    let sums = sums_for(deb, &body, dir.path());
    let downloads = dir.path().join("downloads");
    let mut seen = 0;

    let staged = prepare(
        &Install::Deb,
        &release(),
        "v0.3.0",
        "x86_64",
        &downloads,
        &mut serve(vec![(CHECKSUMS, sums.clone()), (deb, body.clone())]),
        &mut |done, _| seen = done,
    )
    .expect("verified");
    assert_eq!(staged, Staged::Folder(downloads.join(deb)));
    assert_eq!(fs::read(downloads.join(deb)).expect("read"), body);
    assert_eq!(seen, body.len() as u64);

    // A tampered download is refused and nothing is left behind.
    let fresh = dir.path().join("fresh");
    let err = prepare(
        &Install::Deb,
        &release(),
        "v0.3.0",
        "x86_64",
        &fresh,
        &mut serve(vec![(CHECKSUMS, sums), (deb, b"evil".to_vec())]),
        &mut |_, _| {},
    )
    .expect_err("mismatch");
    assert!(err.to_string().contains("checksum"), "{err}");
    assert_eq!(fs::read_dir(&fresh).expect("dir").count(), 0);

    // An asset of another (older) release is never picked.
    let err = prepare(
        &Install::Deb,
        &release(),
        "v0.4.0",
        "x86_64",
        &fresh,
        &mut serve(vec![]),
        &mut |_, _| {},
    )
    .expect_err("downgrade");
    assert!(err.to_string().contains("no download"), "{err}");

    // No SHA256SUMS on the release: refused.
    let no_sums: Vec<Asset> = release()
        .into_iter()
        .filter(|a| a.name != CHECKSUMS)
        .collect();
    let err = prepare(
        &Install::Deb,
        &no_sums,
        "v0.3.0",
        "x86_64",
        &fresh,
        &mut serve(vec![]),
        &mut |_, _| {},
    )
    .expect_err("no sums");
    assert!(err.to_string().contains("SHA256SUMS"), "{err}");

    // SHA256SUMS that doesn't list the asset.
    let err = prepare(
        &Install::Deb,
        &release(),
        "v0.3.0",
        "x86_64",
        &fresh,
        &mut serve(vec![(CHECKSUMS, b"".to_vec())]),
        &mut |_, _| {},
    )
    .expect_err("unlisted");
    assert!(err.to_string().contains("doesn't list"), "{err}");
}

#[test]
fn appimage_is_staged_next_to_the_running_one_and_swapped() {
    let dir = tempfile::tempdir().expect("tempdir");
    let target = dir.path().join("SkyeLab.AppImage");
    fs::write(&target, b"old").expect("write");
    let name = "skyelab_0.3.0_x86_64.AppImage";
    let sums = sums_for(name, b"new", dir.path());
    let staged = prepare(
        &Install::AppImage(target.clone()),
        &release(),
        "v0.3.0",
        "x86_64",
        &dir.path().join("cache"),
        &mut serve(vec![(CHECKSUMS, sums), (name, b"new".to_vec())]),
        &mut |_, _| {},
    )
    .expect("staged");
    let new = dir.path().join(".SkyeLab.AppImage.update");
    assert_eq!(
        staged,
        Staged::Swap {
            new: new.clone(),
            target: target.clone(),
            sha256: Some(
                String::from_utf8(sums_for(name, b"new", dir.path())).expect("utf8")[..64]
                    .to_owned()
            ),
        }
    );
    recheck(&staged).expect("unchanged");
    fs::write(&new, b"swapped meanwhile").expect("tamper");
    assert!(
        recheck(&staged)
            .expect_err("changed")
            .to_string()
            .contains("changed")
    );
    fs::write(&new, b"new").expect("restore");
    assert_eq!(fs::read(&target).expect("old intact"), b"old");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        assert_eq!(
            fs::metadata(&new).expect("meta").permissions().mode() & 0o777,
            0o755
        );
    }
    swap(&new, &target).expect("swap");
    assert_eq!(fs::read(&target).expect("read"), b"new");
    assert!(!new.exists());
    assert!(
        !dir.path().join(".SkyeLab.AppImage.old").exists(),
        "old copy removed"
    );
}

fn app(dir: &Path, name: &str, marker: &str) -> PathBuf {
    let app = dir.join(name);
    fs::create_dir_all(app.join("Contents")).expect("mkdir");
    fs::write(app.join("Contents").join("marker"), marker).expect("write");
    app
}

fn marker(app: &Path) -> String {
    fs::read_to_string(app.join("Contents").join("marker")).expect("marker")
}

#[test]
fn swap_replaces_a_bundle_and_rolls_back_on_failure() {
    let dir = tempfile::tempdir().expect("tempdir");
    let target = app(dir.path(), "SkyeLab.app", "old");
    let new = app(dir.path(), ".SkyeLab.app.update", "new");
    // A stale .old from an earlier run doesn't block it.
    app(dir.path(), ".SkyeLab.app.old", "stale");
    swap(&new, &target).expect("swap");
    assert_eq!(marker(&target), "new");
    assert!(!new.exists() && !dir.path().join(".SkyeLab.app.old").exists());

    // The new bundle is gone: the swap fails and the installed app is back where it was.
    let err = swap(&dir.path().join(".missing.update"), &target).expect_err("no new");
    assert!(err.to_string().contains("into place"), "{err:#}");
    assert_eq!(marker(&target), "new");
    assert!(!dir.path().join(".SkyeLab.app.old").exists());

    // No install at the target: nothing moves.
    let lone = app(dir.path(), ".lone.update", "lone");
    assert!(swap(&lone, &dir.path().join("Gone.app")).is_err());
    assert_eq!(marker(&lone), "lone");
}

#[test]
fn signature_policy_refuses_unsigned_over_signed() {
    let id = Some("dev.openskye.skyelab".to_owned());
    let signed = |team: &str| Signature {
        valid: true,
        team: Some(team.into()),
        accepted: true,
        bundle_id: Some("dev.openskye.skyelab".into()),
    };
    let unsigned = Signature {
        bundle_id: id.clone(),
        ..Signature::default()
    };
    assert!(
        signature_policy(&unsigned, &unsigned).is_ok(),
        "unsigned install: checksum only"
    );
    assert!(signature_policy(&unsigned, &signed("T1")).is_ok());
    assert!(signature_policy(&signed("T1"), &signed("T1")).is_ok());
    let refused = |new: Signature| signature_policy(&signed("T1"), &new).expect_err("refused");
    assert!(refused(unsigned.clone()).contains("isn't signed"));
    assert!(refused(signed("T2")).contains("different developer"));
    assert!(
        refused(Signature {
            valid: false,
            ..signed("T1")
        })
        .contains("doesn't verify")
    );
    assert!(
        refused(Signature {
            accepted: false,
            ..signed("T1")
        })
        .contains("Gatekeeper")
    );
}

#[test]
fn the_update_must_be_the_same_app() {
    let app = |id: Option<&str>, team: Option<&str>| Signature {
        valid: true,
        team: team.map(Into::into),
        accepted: true,
        bundle_id: id.map(Into::into),
    };
    let ours = Some("dev.openskye.skyelab");
    // Another app, even from the same team or with no signature at all, is refused.
    let other = signature_policy(
        &app(ours, Some("T1")),
        &app(Some("dev.openskye.other"), Some("T1")),
    )
    .expect_err("other app");
    assert!(other.contains("different app"), "{other}");
    assert!(signature_policy(&app(ours, None), &app(Some("x.y"), None)).is_err());
    assert!(
        signature_policy(&app(ours, None), &app(None, None)).is_err(),
        "unreadable id"
    );
    assert!(
        signature_policy(&app(None, None), &app(ours, None)).is_err(),
        "installed id unreadable"
    );
    assert!(signature_policy(&app(ours, Some("T1")), &app(ours, Some("T1"))).is_ok());
}

#[test]
fn authenticode_policy_keeps_a_signed_install_signed() {
    let sig = |out: &str| parse_authenticode(out);
    let ours = sig("Valid\nCN=OpenSkye, O=OpenSkye, C=US\n");
    assert_eq!(ours.status, "Valid");
    assert_eq!(ours.subject, "CN=OpenSkye, O=OpenSkye, C=US");
    let unsigned = sig("NotSigned\n\n");
    assert!(
        authenticode_policy(&unsigned, &unsigned).is_ok(),
        "unsigned install: checksum only"
    );
    assert!(authenticode_policy(&unsigned, &ours).is_ok());
    assert!(authenticode_policy(&ours, &ours).is_ok());
    let refused = |new: &str| authenticode_policy(&ours, &sig(new)).expect_err("refused");
    assert!(refused("NotSigned\n").contains("isn't validly signed"));
    assert!(
        refused("HashMismatch\nCN=OpenSkye, O=OpenSkye, C=US").contains("isn't validly signed")
    );
    assert!(refused("Valid\nCN=Someone Else").contains("different publisher"));
    // An installed signature that can't be read never counts as unsigned.
    assert!(authenticode_policy(&sig("UnknownError\n"), &ours).is_err());
    assert!(authenticode_policy(&sig(""), &ours).is_err());
}

#[test]
fn the_installer_script_reports_a_failed_install() {
    let cmd = installer_command(
        Path::new(r"C:\Users\u\AppData\Local\skyelab\updates\SkyeLab_1.0.0_x64-setup.exe"),
        Path::new(r"C:\Program Files\SkyeLab\skyelab.exe"),
        42,
        Path::new(r"C:\Users\u\AppData\Local\skyelab\updates\update-failed"),
        "v1.0.0",
    );
    assert!(
        cmd.get_program()
            .to_string_lossy()
            .ends_with("powershell.exe")
    );
    assert!(Path::new(cmd.get_program()).is_absolute() || !cfg!(windows));
    let script = cmd
        .get_args()
        .last()
        .expect("script")
        .to_string_lossy()
        .into_owned();
    assert!(script.contains("ExitCode"), "{script}");
    assert!(script.contains("SKYELAB_UPDATE_MARKER"), "{script}");
    assert!(
        !script.contains("skyelab.exe") && !script.contains("setup.exe"),
        "no paths in the script"
    );
    let env: Vec<(String, Option<String>)> = cmd
        .get_envs()
        .map(|(k, v)| {
            (
                k.to_string_lossy().into(),
                v.map(|v| v.to_string_lossy().into()),
            )
        })
        .collect();
    let get = |k: &str| {
        env.iter()
            .find(|(n, _)| n == k)
            .and_then(|(_, v)| v.clone())
    };
    assert_eq!(get("SKYELAB_UPDATE_PID").as_deref(), Some("42"));
    assert_eq!(get("SKYELAB_UPDATE_TAG").as_deref(), Some("v1.0.0"));
    assert!(get("SKYELAB_UPDATE_MARKER").is_some_and(|m| m.ends_with("update-failed")));
    assert!(get("CLAUDE_CONFIG_DIR").is_none() && get("GH_TOKEN").is_none());
}

#[test]
fn a_failure_marker_names_the_version_and_why() {
    assert_eq!(
        parse_failure("v1.0.0\nthe installer failed (2)\n"),
        Some(("v1.0.0".into(), "the installer failed (2)".into()))
    );
    assert_eq!(
        parse_failure("v1.0.0\n"),
        Some(("v1.0.0".into(), "the installer failed".into()))
    );
    assert_eq!(parse_failure(""), None);
    assert_eq!(parse_failure("../evil\nx"), None);
}

#[test]
fn team_id_from_codesign() {
    let out = "Executable=/A.app/Contents/MacOS/a\nAuthority=Developer ID Application: X (ABCDE12345)\nTeamIdentifier=ABCDE12345\n";
    assert_eq!(team_id(out).as_deref(), Some("ABCDE12345"));
    assert_eq!(team_id("Signature=adhoc\nTeamIdentifier=not set\n"), None);
    assert_eq!(team_id("code object is not signed at all\n"), None);
}

#[test]
fn an_unreadable_signature_is_never_unsigned() {
    assert_eq!(
        installed_team(false, "/A.app: code object is not signed at all\n"),
        Ok(None)
    );
    assert_eq!(
        installed_team(true, "TeamIdentifier=ABCDE12345\n"),
        Ok(Some("ABCDE12345".into()))
    );
    assert!(installed_team(false, "/A.app: No such file or directory").is_err());
    assert!(installed_team(false, "").is_err());
}

#[test]
fn staging_never_writes_through_a_planted_symlink() {
    #[cfg(unix)]
    {
        let dir = tempfile::tempdir().expect("tempdir");
        let victim = dir.path().join("victim");
        fs::write(&victim, b"precious").expect("write");
        let downloads = dir.path().join("downloads");
        fs::create_dir(&downloads).expect("mkdir");
        let deb = "skyelab_0.3.0_amd64.deb";
        std::os::unix::fs::symlink(&victim, downloads.join(format!("{deb}.part"))).expect("plant");
        let sums = sums_for(deb, b"pkg", dir.path());
        prepare(
            &Install::Deb,
            &release(),
            "v0.3.0",
            "x86_64",
            &downloads,
            &mut serve(vec![(CHECKSUMS, sums), (deb, b"pkg".to_vec())]),
            &mut |_, _| {},
        )
        .expect("staged");
        assert_eq!(fs::read(&victim).expect("read"), b"precious");
    }
}
