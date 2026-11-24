//! Acceptance: when the `@` `/` `$` menus open, their order, and that commands and skills come only from the
//! account the folder is bound to.

use super::composer_menu::{
    MAX_MODEL_ROWS, MenuItem, MenuKind, Trigger, command_items, model_items, rank, skill_items, trigger,
};
use super::threads_test::{fixture, open_workspace, project_thread, setup, wait_until};
use gpui::TestAppContext;
use skyelab_core::Provider;
use std::fs;

fn t(kind: MenuKind, start: usize, query: &str) -> Option<Trigger> {
    Some(Trigger {
        kind,
        start,
        query: query.into(),
    })
}

#[test]
fn triggers_open_on_the_word_before_the_cursor() {
    assert_eq!(trigger("@", 1), t(MenuKind::Files, 0, ""));
    assert_eq!(
        trigger("look at @src/li", 15),
        t(MenuKind::Files, 8, "src/li")
    );
    assert_eq!(trigger("/comp", 5), t(MenuKind::Commands, 0, "comp"));
    assert_eq!(trigger("  /comp", 7), t(MenuKind::Commands, 2, "comp"));
    assert_eq!(trigger("use $rev", 8), t(MenuKind::Skills, 4, "rev"));
    // `/` only opens at the start; a path in the middle is just text.
    assert_eq!(trigger("see a/b", 7), None);
    // The cursor past the word (a space typed) closes it.
    assert_eq!(trigger("@src ", 5), None);
    // `#` opens the threads menu, but `#12` is an issue number.
    assert_eq!(trigger("#", 1), t(MenuKind::Context, 0, ""));
    assert_eq!(trigger("see #fix", 8), t(MenuKind::Context, 4, "fix"));
    assert_eq!(trigger("see #12", 7), None);
    assert_eq!(trigger("mail me@x", 9), None);
    // Multi-byte text before the token.
    assert_eq!(
        trigger("héllo @ä", "héllo @ä".len()),
        t(MenuKind::Files, 7, "ä")
    );
    // A cursor inside a character is no trigger, never a panic.
    assert_eq!(trigger("é", 1), None);
}

fn item(label: &str) -> MenuItem {
    MenuItem {
        label: label.into(),
        detail: String::new(),
        insert: label.into(),
        ..Default::default()
    }
}

#[test]
fn rank_prefers_file_name_prefixes_then_substrings_then_scattered() {
    let items: Vec<_> = [
        "docs/readme.md",
        "src/lib.rs",
        "src/library/mod.rs",
        "crates/lib_test.rs",
        "l/i/b",
    ]
    .into_iter()
    .map(item)
    .collect();
    let order: Vec<&str> = rank(&items, "lib")
        .iter()
        .map(|&i| items[i].label.as_str())
        .collect();
    assert_eq!(
        order,
        [
            "src/lib.rs",
            "crates/lib_test.rs",
            "src/library/mod.rs",
            "l/i/b"
        ]
    );
    assert_eq!(rank(&items, "zzz"), Vec::<usize>::new());
    assert_eq!(rank(&items, "").len(), items.len());
}

#[test]
fn commands_and_skills_come_from_the_given_account_dir() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let account = tmp.path().join("acct");
    let other = tmp.path().join("other");
    let project = tmp.path().join("proj");
    fs::create_dir_all(account.join("commands/team")).expect("mkdir");
    fs::create_dir_all(other.join("commands")).expect("mkdir");
    fs::create_dir_all(project.join(".claude/commands")).expect("mkdir");
    fs::create_dir_all(account.join("skills/review")).expect("mkdir");
    fs::write(
        account.join("commands/ship.md"),
        "---\ndescription: Ship it\n---\nbody",
    )
    .expect("write");
    fs::write(account.join("commands/team/sync.md"), "# Sync the team").expect("write");
    fs::write(other.join("commands/leak.md"), "x").expect("write");
    fs::write(project.join(".claude/commands/local.md"), "Local one").expect("write");
    fs::write(
        account.join("skills/review/SKILL.md"),
        "---\ndescription: Reviews\n---",
    )
    .expect("write");

    let labels: Vec<String> = command_items(Provider::Claude, &account, &project, false)
        .into_iter()
        .map(|i| i.label)
        .collect();
    assert!(
        labels.contains(&"/compact".to_owned()),
        "built-ins: {labels:?}"
    );
    assert!(labels.contains(&"/ship".to_owned()));
    assert!(labels.contains(&"/team:sync".to_owned()));
    assert!(labels.contains(&"/local".to_owned()));
    assert!(
        !labels.iter().any(|l| l == "/leak"),
        "another account's command: {labels:?}"
    );
    let ship = command_items(Provider::Claude, &account, &project, false)
        .into_iter()
        .find(|i| i.label == "/ship")
        .expect("ship");
    assert_eq!(ship.detail, "Ship it");

    let skills = skill_items(Provider::Claude, &account);
    assert_eq!(skills.len(), 1);
    assert_eq!(
        (skills[0].label.as_str(), skills[0].insert.as_str()),
        ("$review", "/review ")
    );
    assert_eq!(skill_items(Provider::Codex, &account)[0].insert, "$review ");
    assert!(command_items(Provider::Codex, &account, &project, false).is_empty());
}

#[gpui::test]
fn slash_commands_come_from_the_bound_account_dir_only(cx: &mut TestAppContext) {
    setup(cx);
    let f = fixture(true);
    // The bound account's command, and one in a folder that is not that account's.
    fs::create_dir_all(f.account_dir.join("commands")).expect("mkdir");
    fs::write(f.account_dir.join("commands/mine.md"), "Mine").expect("write");
    let stray = f
        .project_dir
        .parent()
        .expect("root")
        .join(".claude/commands");
    fs::create_dir_all(&stray).expect("mkdir");
    fs::write(stray.join("parent.md"), "Not this account's").expect("write");
    let (ws, cx) = open_workspace(&f, cx);
    let thread = project_thread(&f, &ws, cx);
    thread.update_in(cx, |t, window, cx| t.focus_prompt(window, cx));
    cx.simulate_keystrokes("/");
    wait_until(cx, "the command menu to load", |cx| {
        thread.read_with(cx, |t, _| {
            t.composer.menu.as_ref().is_some_and(|m| m.items.is_some())
        })
    });
    let labels: Vec<String> = thread.read_with(cx, |t, _| {
        let m = t.composer.menu.as_ref().expect("menu");
        assert_eq!(m.kind, MenuKind::Commands);
        m.rows
            .iter()
            .map(|&i| m.items.as_ref().expect("items")[i].label.clone())
            .collect()
    });
    assert!(labels.contains(&"/mine".to_owned()), "{labels:?}");
    assert!(!labels.contains(&"/parent".to_owned()), "{labels:?}");

    // Narrowing, then enter, puts the command in the prompt.
    cx.simulate_keystrokes("m i n");
    cx.run_until_parked();
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    thread.read_with(cx, |t, cx| {
        assert_eq!(t.prompt.read(cx).value().as_ref(), "/mine ");
        assert!(t.composer.menu.is_none());
    });
}

#[gpui::test]
fn at_menu_lists_tracked_files_and_inserts_a_mention(cx: &mut TestAppContext) {
    setup(cx);
    let f = fixture(true);
    let git = |args: &[&str]| {
        let ok = skyelab_core::process::output(
            std::process::Command::new("git")
                .arg("-C")
                .arg(&f.project_dir)
                .args([
                    "-c",
                    "user.name=t",
                    "-c",
                    "user.email=t@e",
                    "-c",
                    "commit.gpgsign=false",
                ])
                .args(args),
            None,
        )
        .expect("git")
        .status
        .success();
        assert!(ok, "git {args:?}");
    };
    git(&["init", "-q", "-b", "main"]);
    fs::write(f.project_dir.join("tracked.rs"), "").expect("write");
    fs::write(f.project_dir.join(".gitignore"), "ignored.rs\n").expect("write");
    git(&["add", "."]);
    git(&["commit", "-q", "-m", "init"]);
    fs::write(f.project_dir.join("ignored.rs"), "").expect("write");
    let (ws, cx) = open_workspace(&f, cx);
    let thread = project_thread(&f, &ws, cx);
    thread.update_in(cx, |t, window, cx| t.focus_prompt(window, cx));
    cx.simulate_keystrokes("s e e space @ r s");
    wait_until(cx, "the file menu to load", |cx| {
        thread.read_with(cx, |t, _| {
            t.composer.menu.as_ref().is_some_and(|m| m.items.is_some())
        })
    });
    let labels: Vec<String> = thread.read_with(cx, |t, _| {
        let m = t.composer.menu.as_ref().expect("menu");
        m.rows
            .iter()
            .map(|&i| m.items.as_ref().expect("items")[i].label.clone())
            .collect()
    });
    assert_eq!(
        labels,
        ["tracked.rs"],
        "ignored and untracked files stay out"
    );
    cx.simulate_keystrokes("tab");
    cx.run_until_parked();
    thread.read_with(cx, |t, cx| {
        assert_eq!(t.prompt.read(cx).value().as_ref(), "see @tracked.rs ");
    });
    // The footer knows the branch.
    wait_until(cx, "the footer's branch", |cx| {
        thread.read_with(cx, |t, _| t.composer.footer.branch.is_some())
    });
}

#[test]
fn at_models_need_two_letters_match_word_starts_and_rank_first() {
    let mut items = model_items(
        &[
            ("sonnet".into(), "Sonnet".into()),
            ("gpt-5.1-codex".into(), "GPT-5.1 Codex".into()),
        ],
        Provider::Claude,
    );
    items.push(MenuItem {
        label: "src/sonnet.rs".into(),
        ..Default::default()
    });
    let labels = |q: &str| -> Vec<String> {
        rank(&items, q)
            .into_iter()
            .map(|i| items[i].label.clone())
            .collect()
    };
    assert_eq!(labels("so"), ["Sonnet", "src/sonnet.rs"]);
    assert_eq!(
        labels("s"),
        ["src/sonnet.rs"],
        "one letter lists files only"
    );
    assert_eq!(
        labels("onnet"),
        ["src/sonnet.rs"],
        "a model matches by word start only"
    );
    assert_eq!(labels("gpt-cod"), ["GPT-5.1 Codex"]);
    assert_eq!(labels("5.1"), ["GPT-5.1 Codex"]);
}

#[test]
fn at_models_insert_a_model_reference_and_stop_at_eight() {
    let models: Vec<(String, String)> = (0..12)
        .map(|i| (format!("gpt-{i}"), format!("GPT {i}")))
        .collect();
    let mut items = model_items(&models, Provider::Codex);
    assert_eq!(items[3].insert, "model:codex/gpt-3 ");
    items.push(MenuItem {
        label: "gpt.md".into(),
        ..Default::default()
    });
    let rows = rank(&items, "gpt");
    assert_eq!(
        rows.iter().filter(|&&i| items[i].model.is_some()).count(),
        MAX_MODEL_ROWS
    );
    assert_eq!(items[*rows.last().expect("rows")].label, "gpt.md");
}

#[gpui::test]
fn at_model_pick_inserts_a_reference_and_keeps_the_threads_model(cx: &mut TestAppContext) {
    setup(cx);
    let f = fixture(true);
    let (ws, cx) = open_workspace(&f, cx);
    let thread = project_thread(&f, &ws, cx);
    thread.update_in(cx, |t, window, cx| t.focus_prompt(window, cx));
    cx.simulate_keystrokes("f i x space @ s o n");
    wait_until(cx, "the @ menu to rank", |cx| {
        thread.read_with(cx, |t, _| {
            t.composer
                .menu
                .as_ref()
                .is_some_and(|m| m.ranked.as_deref() == Some("son"))
        })
    });
    let first = thread.read_with(cx, |t, _| {
        let m = t.composer.menu.as_ref().expect("menu");
        m.items.as_ref().expect("items")[m.rows[0]].model.clone()
    });
    assert_eq!(first.as_deref(), Some("sonnet"));
    cx.simulate_keystrokes("tab");
    cx.run_until_parked();
    thread.read_with(cx, |t, cx| {
        assert_eq!(
            t.prompt.read(cx).value().as_ref(),
            "fix model:claude/sonnet "
        );
        assert_eq!(t.composer.pick.model, None, "the thread's model stays");
    });
}
