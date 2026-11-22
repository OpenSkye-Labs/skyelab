use super::*;

fn ranked<'a>(query: &str, labels: &[&'a str]) -> Vec<&'a str> {
    let mut scored: Vec<(i32, &str)> = labels
        .iter()
        .filter_map(|l| score(query, l).map(|s| (s, *l)))
        .collect();
    scored.sort_by_key(|&(s, _)| std::cmp::Reverse(s));
    scored.into_iter().map(|(_, l)| l).collect()
}

#[test]
fn non_subsequence_does_not_match() {
    assert_eq!(score("xyz", "Open Settings"), None);
    assert_eq!(score("sto", "Open Settings"), None, "order matters");
}

#[test]
fn empty_query_matches_everything() {
    assert!(score("", "Open Settings").is_some());
    assert!(score("   ", "Anything").is_some());
}

#[test]
fn ignores_case_and_spaces_in_the_query() {
    assert!(score("OPEN set", "open settings").is_some());
    assert_eq!(score("o s", "Open Settings"), score("os", "Open Settings"));
}

#[test]
fn word_starts_beat_mid_word_matches() {
    assert_eq!(
        ranked("os", &["Close Tabs", "Open Settings"]),
        ["Open Settings", "Close Tabs"]
    );
    // camelCase humps count as word starts too
    assert_eq!(
        ranked("nt", &["Cancel It", "NewThread"]),
        ["NewThread", "Cancel It"]
    );
}

#[test]
fn consecutive_runs_beat_scattered_letters() {
    assert_eq!(
        ranked("set", &["Show Every Tab", "Open Settings"]),
        ["Open Settings", "Show Every Tab"]
    );
}

#[test]
fn a_prefix_beats_a_later_match() {
    assert_eq!(
        ranked("term", &["New Terminal", "Terminal 1"]),
        ["Terminal 1", "New Terminal"]
    );
}

#[test]
fn rank_keeps_input_order_on_ties_and_drops_misses() {
    let labels = ["Open Browser", "Open Devices", "Toggle Sidebar"];
    assert_eq!(rank("", labels.iter().copied()), vec![0, 1, 2]);
    assert_eq!(rank("open", labels.iter().copied()), vec![0, 1]);
    assert_eq!(rank("dev", labels.iter().copied()), vec![1]);
}

#[test]
fn humanize_drops_the_namespace_and_splits_camel_case() {
    assert_eq!(humanize("skyelab::OpenSettings"), "Open Settings");
    assert_eq!(
        humanize("skyelab::ToggleBottomPanel"),
        "Toggle Bottom Panel"
    );
    assert_eq!(humanize("terminal::Paste"), "Paste");
}

#[test]
fn humanize_prefers_the_explicit_table() {
    assert_eq!(humanize("skyelab::ToggleEnvironments"), "Switch Machine");
    assert_eq!(humanize("skyelab::Stop"), "Stop Agent");
}

#[test]
fn palette_internal_and_typed_input_actions_are_hidden() {
    for name in [
        "skyelab::OpenCommandPalette",
        "skyelab::ConnectTyped",
        "skyelab::ConfirmDiscardWorktree",
        "skyelab::CancelDiscardWorktree",
        "skyelab::CancelEdit",
        "zed::NoAction",
        "input::Backspace",
        "dialog::Cancel",
    ] {
        assert!(!listed(name), "{name} must not be listed");
    }
    assert!(listed("skyelab::OpenSettings"));
}

#[test]
fn bindings_per_os_build_and_unbind_ctrl_k_in_terminals_off_mac() {
    let keys = |mac: bool| -> Vec<(String, String, Option<String>)> {
        let os = if mac {
            gpui::OperatingSystem::Mac
        } else {
            gpui::OperatingSystem::Linux
        };
        bindings(mac)
            .iter()
            .map(|b| {
                let keys = b
                    .keystrokes()
                    .iter()
                    .map(|k| k.unparse(os))
                    .collect::<Vec<_>>()
                    .join(" ");
                let ctx = b.predicate().map(|p| p.to_string());
                (keys, b.action().name().to_owned(), ctx)
            })
            .collect()
    };
    let mac = keys(true);
    let palette = "skyelab::OpenCommandPalette";
    let ws = Some("Workspace".to_owned());
    assert!(mac.contains(&("cmd-k".into(), palette.into(), ws.clone())));
    assert!(mac.contains(&("cmd-shift-p".into(), palette.into(), ws.clone())));
    assert!(
        !mac.iter().any(|(_, a, _)| a == "zed::NoAction"),
        "cmd-k has no shell meaning to keep"
    );

    let other = keys(false);
    assert!(other.contains(&("ctrl-k".into(), palette.into(), ws.clone())));
    assert!(other.contains(&("ctrl-shift-p".into(), palette.into(), ws)));
    assert!(other.contains(&(
        "ctrl-k".into(),
        "zed::NoAction".into(),
        Some("Terminal".into())
    )));
}

#[test]
fn query_and_text_lowercase_alike() {
    assert!(score("İstanbul", "İstanbul Office").is_some());
    assert!(score("istanbul", "İstanbul Office").is_some());
}
