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
            skipped: Some("v0.3.0".into()),
            channel: None,
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

#[test]
fn nightlies_order_by_semver_below_their_release() {
    let chain = [
        "v0.1.7",
        "v0.1.8-nightly.20261127",
        "v0.1.8-nightly.20261128",
        "v0.1.8-nightly.20261128.2",
        "v0.1.8-nightly.20261128.10",
        "v0.1.8",
        "v0.1.9-nightly.20261201",
    ];
    for pair in chain.windows(2) {
        assert!(is_newer(pair[1], pair[0]), "{} > {}", pair[1], pair[0]);
        assert!(!is_newer(pair[0], pair[1]), "{} < {}", pair[0], pair[1]);
    }
    assert!(is_newer("0.1.8", "0.1.8-dev"));
    assert!(is_newer("0.1.8-nightly.20261127", "0.1.7"));
    assert!(!is_newer(
        "0.1.8-nightly.20261127",
        "0.1.8-nightly.20261127"
    ));
    // Numeric identifiers sort below alphanumeric ones, and a bad identifier isn't a version.
    assert!(is_newer("1.0.0-rc", "1.0.0-1"));
    assert!(!is_newer("1.0.0-a..b", "0.1.0"));
    assert!(
        is_nightly("v0.1.8-nightly.20261127.2")
            && !is_nightly("v0.1.8")
            && !is_nightly("1.0.0-rc.1")
    );
    assert!(is_prerelease("1.0.0-rc.1") && !is_prerelease("v1.0.0+build.5"));
}

#[test]
fn channels_choose_and_offer() {
    let tags = [
        "v0.1.7",
        "v0.1.8-nightly.20261128.2",
        "v0.1.8-nightly.20261128",
        "v0.1.6",
    ];
    assert_eq!(newest(tags, Channel::Stable), Some("v0.1.7"));
    assert_eq!(
        newest(tags, Channel::Nightly),
        Some("v0.1.8-nightly.20261128.2")
    );
    // A newer stable wins on Nightly too.
    let with_stable = ["v0.1.8-nightly.20261128", "v0.1.8", "junk"];
    assert_eq!(newest(with_stable, Channel::Nightly), Some("v0.1.8"));
    assert_eq!(newest(["junk"], Channel::Nightly), None);
    // A stable user is never offered a pre-release, however new.
    assert!(!offered(
        "v0.2.0-nightly.20261128",
        "0.1.7",
        Channel::Stable
    ));
    assert!(offered(
        "v0.2.0-nightly.20261128",
        "0.1.7",
        Channel::Nightly
    ));
    assert!(offered("v0.1.8", "0.1.8-nightly.20261128", Channel::Stable));
    assert!(!offered(
        "v0.1.7",
        "0.1.8-nightly.20261128",
        Channel::Nightly
    ));
}

#[test]
fn channel_follows_the_build_until_chosen() {
    let mut s = UpdateSettings::default();
    assert_eq!(s.channel("0.1.7"), Channel::Stable);
    assert_eq!(s.channel("0.1.8-nightly.20261128"), Channel::Nightly);
    // An unset channel isn't written, so an older build still reads the file.
    let text = toml::to_string(&s).expect("serializes");
    assert!(!text.contains("channel"), "{text}");
    s.channel = Some(Channel::Stable);
    assert_eq!(s.channel("0.1.8-nightly.20261128"), Channel::Stable);
    let text = toml::to_string(&s).expect("serializes");
    assert!(text.contains("channel = \"stable\""), "{text}");
    let back: UpdateSettings = toml::from_str(&text).expect("parses");
    assert_eq!(back, s);
    let old: UpdateSettings = toml::from_str("check = true").expect("an older file parses");
    assert_eq!(old.channel, None);
}
