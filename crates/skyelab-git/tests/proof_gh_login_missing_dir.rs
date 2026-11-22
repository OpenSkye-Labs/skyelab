use std::path::Path;

use skyelab_git::GitHub;

#[test]
fn gh_login_in_a_dir_not_created_yet_is_not_logged_in() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let dir = tmp.path().join("gh-new");
    let bin = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-gh.sh");
    let got = skyelab_git::gh_login(&GitHub {
        gh: &bin,
        config_dir: &dir,
    });
    assert!(matches!(got, Ok(None)), "got {got:?}");
}
