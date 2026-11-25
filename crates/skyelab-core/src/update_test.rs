use crate::update::*;

#[test]
fn newer_compares_numbers_not_strings() {
    assert!(is_newer("v0.10.0", "0.9.3"));
    assert!(is_newer("1.0", "0.99.99"));
    assert!(is_newer("v0.2.1", "0.2.0"));
    assert!(!is_newer("v0.2.0", "0.2.0"));
    assert!(!is_newer("0.2", "0.2.0"));
    assert!(!is_newer("v0.1.9", "0.2.0"));
    // A pre-release suffix is ignored: the same numbers are not newer.
    assert!(!is_newer("v0.2.0-beta.1", "0.2.0"));
    assert!(is_newer("v0.3.0-rc.1", "0.2.0"));
    // Not a version: never newer.
    assert!(!is_newer("nightly", "0.2.0"));
    assert!(!is_newer("", "0.2.0"));
    assert!(!is_newer("v1..2", "0.2.0"));
}

#[test]
fn only_this_repos_release_pages_open() {
    assert!(release_page_ok(
        "https://github.com/OpenSkye-Labs/skyelab/releases/tag/v0.3.0"
    ));
    for bad in [
        "https://github.com/OpenSkye-Labs/skyelab/releases/",
        "https://github.com/evil/skyelab/releases/tag/v1",
        "http://github.com/OpenSkye-Labs/skyelab/releases/tag/v1",
        "https://github.com/OpenSkye-Labs/skyelab/releases/tag/v1?x=y",
        "https://github.com/OpenSkye-Labs/skyelab/releases/../../evil",
        "file:///etc/passwd",
        "https://github.com.evil.io/OpenSkye-Labs/skyelab/releases/tag/v1",
    ] {
        assert!(!release_page_ok(bad), "{bad}");
    }
}

#[test]
fn due_daily_by_wall_clock() {
    let day = CHECK_EVERY_SECS;
    assert!(due(None, 1_000));
    assert!(!due(Some(1_000), 1_000 + day - 1));
    assert!(due(Some(1_000), 1_000 + day));
    // The clock went back: check rather than wait for it to catch up.
    assert!(due(Some(1_000 + day), 1_000));
}

#[test]
fn settings_default_on_and_round_trip() {
    let s = crate::AppSettings::default();
    assert!(s.updates.check);
    let toml = "[updates]\ncheck = false\nlast_check = 5\nskipped = \"v0.3.0\"\n";
    let parsed: crate::AppSettings = toml::from_str(toml).expect("parses");
    assert_eq!(
        parsed.updates,
        UpdateSettings {
            check: false,
            last_check: Some(5),
            skipped: Some("v0.3.0".into())
        }
    );
    assert_eq!(display_version("v0.3.0"), "0.3.0");
}

#[test]
fn version_in_finds_the_version_word_of_a_version_line() {
    use crate::update::version_in;
    assert_eq!(version_in("2.1.3 (Claude Code)"), Some("2.1.3"));
    assert_eq!(version_in("codex-cli 0.46.0"), Some("0.46.0"));
    assert_eq!(version_in("claude v1.0.0-beta.2"), Some("v1.0.0-beta.2"));
    assert_eq!(version_in("unknown"), None);
    assert_eq!(version_in(""), None);
}

#[test]
fn each_agent_asks_its_own_npm_package() {
    use crate::update::agent_latest_url;
    assert!(
        agent_latest_url(crate::Provider::Claude)
            .is_some_and(|u| u.ends_with("/@anthropic-ai/claude-code/latest"))
    );
    assert!(
        agent_latest_url(crate::Provider::Codex)
            .is_some_and(|u| u.ends_with("/@openai/codex/latest"))
    );
    assert_eq!(agent_latest_url(crate::Provider::Cursor), None);
    assert_eq!(agent_latest_url(crate::Provider::Antigravity), None);
}

#[test]
fn asset_urls_are_this_repos_downloads_only() {
    let ok = format!("{RELEASES_PAGE}download/v0.3.0/SkyeLab_0.3.0_aarch64.dmg");
    assert!(asset_url_ok(&ok));
    assert!(!asset_url_ok(
        "https://evil.example/SkyeLab_0.3.0_aarch64.dmg"
    ));
    assert!(!asset_url_ok(&format!("{RELEASES_PAGE}tag/v0.3.0")));
    assert!(!asset_url_ok(&format!(
        "{RELEASES_PAGE}download/v0.3.0/../x.dmg"
    )));
    assert!(!asset_url_ok(&format!(
        "{RELEASES_PAGE}download/v0.3.0/a/b.dmg"
    )));
}

#[test]
fn checksums_listing() {
    let a = "a".repeat(64);
    let b = "B".repeat(64);
    let sums = format!("{a}  SkyeLab_0.3.0_aarch64.dmg\n{b} *skyelab_0.3.0_amd64.deb\nbad  x\n");
    assert_eq!(expected_sha256(&sums, "SkyeLab_0.3.0_aarch64.dmg"), Some(a));
    assert_eq!(
        expected_sha256(&sums, "skyelab_0.3.0_amd64.deb"),
        Some("b".repeat(64))
    );
    assert_eq!(expected_sha256(&sums, "x"), None, "not a sha256");
    assert_eq!(expected_sha256(&sums, "SkyeLab"), None, "whole names only");
}
