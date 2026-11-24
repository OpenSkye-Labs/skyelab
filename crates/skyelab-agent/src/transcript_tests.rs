//! The one transcript builder: fork and side chat use it messages-only with the newest cut, handoff with T3's
//! priority cut.

use super::*;

fn item(role: &'static str, kind: &'static str, text: &str) -> Item {
    Item {
        role,
        kind,
        text: text.into(),
    }
}

fn newest(budget: usize) -> Options {
    Options {
        budget,
        tools: false,
        cut: Cut::Newest,
    }
}

#[test]
fn messages_only_drops_tool_items_and_kind() {
    let items = [
        item("user", "message", "fix it"),
        item("assistant", "command", "Command: ls"),
        item("assistant", "message", "done"),
        item("assistant", "error", "boom"),
    ];
    let t = transcript(&items, &newest(10_000));
    assert_eq!(
        t.body,
        "{\"role\":\"user\",\"text\":\"fix it\"}\n{\"role\":\"assistant\",\"text\":\"done\"}\n"
    );
    assert_eq!((t.selected, t.total), (2, 2));
    let all = transcript(
        &items,
        &Options {
            budget: 10_000,
            tools: true,
            cut: Cut::Priority,
        },
    );
    assert!(all.body.contains("\"kind\":\"command\""), "{}", all.body);
    assert_eq!(all.total, 4);
}

#[test]
fn the_newest_cut_keeps_a_run_of_newest_whole_items() {
    let items: Vec<Item> = (0..50)
        .map(|i| item("user", "message", &format!("m{i} {}", "x".repeat(50))))
        .collect();
    let t = transcript(&items, &newest(1_000));
    let lines: Vec<&str> = t.body.lines().collect();
    assert_eq!(lines[0], "[earlier messages left out]");
    assert!(lines.last().expect("some").contains("m49 "));
    assert!(t.body.len() <= 1_000, "{}", t.body.len());
    for l in &lines[1..] {
        serde_json::from_str::<Value>(l).expect("whole lines only");
    }
    // One run: no gap inside what is kept.
    let first: usize = lines[1]
        .split("\"m")
        .nth(1)
        .and_then(|s| s.split(' ').next())
        .and_then(|n| n.parse().ok())
        .expect("a numbered line");
    assert_eq!(first + lines.len() - 1, 50);
}

#[test]
fn the_newest_cut_keeps_the_end_of_one_item_over_the_budget() {
    let items = [item(
        "user",
        "message",
        &format!("{}<end", "y".repeat(5_000)),
    )];
    let t = transcript(&items, &newest(1_000));
    assert!(t.body.len() <= 1_000, "{}", t.body.len());
    let last: Value = serde_json::from_str(t.body.lines().last().expect("one")).expect("JSON");
    assert!(last["text"].as_str().expect("text").ends_with("<end"));
    assert!(!t.body.contains('<'), "quoted: {}", t.body);
}

#[test]
fn the_handoff_prompt_is_the_priority_transcript() {
    let items: Vec<Item> = (0..6)
        .map(|i| item("user", "message", &format!("m{i}")))
        .collect();
    let o = Options {
        budget: 60,
        tools: true,
        cut: Cut::Priority,
    };
    let t = transcript(&items, &o);
    let p = prompt("T", "Claude", "Codex", &items, 60);
    assert!(
        p.contains(&format!("<conversation>\n{}</conversation>", t.body)),
        "{p}"
    );
    assert!(
        p.contains(&format!("Selected {} of 6 items", t.selected)),
        "{p}"
    );
}
