#![cfg(unix)]
use skyelab_core::update::{Asset, CHECKSUMS, RELEASES_PAGE};
use skyelab_core::update_install::{Install, prepare, sha256_file};
use std::fs;
use std::io::{self, Write};

#[test]
fn appimage_stage_does_not_follow_a_planted_symlink() {
    let dir = tempfile::tempdir().expect("tempdir");
    let target = dir.path().join("SkyeLab.AppImage");
    fs::write(&target, b"old").expect("write target");
    let victim = dir.path().join("victim.txt");
    fs::write(&victim, b"precious").expect("write victim");
    std::os::unix::fs::symlink(&victim, dir.path().join(".SkyeLab.AppImage.update"))
        .expect("plant");
    let name = "skyelab_0.3.0_x86_64.AppImage";
    let body = b"new".to_vec();
    let probe = dir.path().join("probe");
    fs::write(&probe, &body).expect("write probe");
    let hex = sha256_file(&probe).expect("hash");
    let sums = format!("{hex}  {name}\n").into_bytes();
    let assets: Vec<Asset> = [name, CHECKSUMS]
        .iter()
        .map(|n| Asset {
            name: (*n).into(),
            url: format!("{RELEASES_PAGE}download/v0.3.0/{n}"),
        })
        .collect();
    let mut get =
        |url: &str, out: &mut dyn Write, _p: &mut dyn FnMut(u64, Option<u64>)| -> io::Result<()> {
            if url.ends_with(CHECKSUMS) {
                out.write_all(&sums)
            } else {
                out.write_all(&body)
            }
        };
    let _ = prepare(
        &Install::AppImage(target.clone()),
        &assets,
        "v0.3.0",
        "x86_64",
        &dir.path().join("cache"),
        &mut get,
        &mut |_, _| {},
    );
    assert_eq!(
        fs::read(&victim).expect("read victim"),
        b"precious",
        "SYMLINK_FOLLOWED"
    );
}
