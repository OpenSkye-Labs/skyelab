//! Server-dictation consent is kept per language, whatever spelling of the locale the recognizer reports.

use super::*;

#[test]
fn a_locale_is_allowed_once_in_any_spelling() {
    let mut s = AppSettings::default();
    assert!(!s.allows_server_dictation("en-US"));
    s.allow_server_dictation("en_US");
    assert!(s.allows_server_dictation("en-US"));
    assert!(s.allows_server_dictation("EN_us"));
    assert!(
        !s.allows_server_dictation("en-GB"),
        "another region asks again"
    );
    s.allow_server_dictation("en-US");
    s.allow_server_dictation("  ");
    assert_eq!(s.dictation_server_locales, ["en-us"]);
}

#[test]
fn older_files_load_and_the_list_round_trips() {
    let old: AppSettings = toml::from_str("onboarded = true\n").expect("an older file loads");
    assert!(old.dictation_server_locales.is_empty());
    let mut s = AppSettings::default();
    s.allow_server_dictation("de_DE");
    let back: AppSettings =
        toml::from_str(&toml::to_string_pretty(&s).expect("serialize")).expect("parse");
    assert!(back.allows_server_dictation("de-DE"));
}
