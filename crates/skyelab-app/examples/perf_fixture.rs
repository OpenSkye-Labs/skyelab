//! Builds a throwaway HOME for performance runs (docs/debugging.md, "Performance fixtures"): many projects bound to
//! one fake Claude account, hundreds of short threads, and one thread with a very long history (prompts, thinking,
//! markdown, tool rows, diffs, mermaid diagrams, tables and HTML cards).
//!
//! `cargo run --release -p skyelab-app --example perf_fixture -- <home> [projects] [threads per project] [turns]`
//! (defaults 40, 15, 600). Then run the app with `HOME=<home>`. Never point it at your real HOME: it overwrites
//! `accounts.toml` there.

use anyhow::{Context as _, Result, bail};
use serde_json::json;
use skyelab_agent::timeline::Record;
use skyelab_agent::{Block, Event, Usage};
use skyelab_store::{NewThread, Store};
use std::path::{Path, PathBuf};

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let home = PathBuf::from(
        args.next()
            .context("usage: perf_fixture <home> [projects] [threads] [turns]")?,
    );
    let num = |a: Option<String>, d: usize| a.and_then(|s| s.parse().ok()).unwrap_or(d);
    let (projects, threads, turns) = (
        num(args.next(), 40),
        num(args.next(), 15),
        num(args.next(), 600),
    );
    // The app's own data dir: `skyelab-dev` in perf.sh's profiling build (debug assertions on).
    let support = home
        .join("Library/Application Support")
        .join(skyelab_core::instance::APP_DIR);
    if support.join("skyelab.db").exists() {
        bail!("{} already has a store: use a fresh folder", home.display());
    }
    let work = home.join("work");
    std::fs::create_dir_all(&support)?;
    std::fs::create_dir_all(home.join(".claude-work"))?;
    let work = canonical_dir(&work)?;
    let accounts = format!(
        "[[account]]\nid = \"work\"\nlabel = \"Work\"\nconfig_dir = {:?}\n\n[[binding]]\npath = {w:?}\nresolved = {w:?}\naccount = \"work\"\n",
        home.join(".claude-work").display().to_string(),
        w = work.display().to_string(),
    );
    std::fs::write(support.join("accounts.toml"), accounts)?;

    let mut store = Store::open(&support.join("skyelab.db"))?;
    let mut total = 0;
    for p in 0..projects {
        let dir = work.join(format!("project-{p:02}"));
        std::fs::create_dir_all(&dir)?;
        let _ = std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(&dir)
            .status();
        let project = store.create_project(&format!("project-{p:02}"), &dir)?;
        for t in 0..threads {
            let title = format!("Thread {t} of project {p}: fix the flaky parser test");
            let thread = new_thread(&store, project.id, &title, &dir)?;
            for r in short_thread(t) {
                append(&mut store, thread.id, &r)?;
            }
            total += 1;
        }
        if p == 0 {
            let thread = new_thread(&store, project.id, "Long history", &dir)?;
            store.set_agent_session_id(thread.id, Some("sess-long"))?; // resumable: the stream run sends to it
            let records = long_thread(turns, &dir);
            for r in &records {
                append(&mut store, thread.id, r)?;
            }
            println!("long thread {} has {} events", thread.id, records.len());
            // The window reopens on it, like any thread left open (`WindowLayout::view`).
            let settings = format!(
                "developer_mode = true\n\n[window]\nx = 80\ny = 60\nwidth = 1400\nheight = 900\nview = \"thread:{}\"\n",
                thread.id
            );
            std::fs::write(support.join("settings.toml"), settings)?;
            total += 1;
        }
    }
    println!("{projects} projects, {total} threads in {}", home.display());
    Ok(())
}

fn canonical_dir(dir: &Path) -> Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    Ok(dir.canonicalize()?)
}

fn new_thread(
    store: &Store,
    project_id: i64,
    title: &str,
    cwd: &Path,
) -> Result<skyelab_store::Thread> {
    let t = store.create_thread(&NewThread {
        project_id,
        title,
        cwd,
        account_id: "work",
        status: "done",
    })?;
    Ok(t)
}

fn append(store: &mut Store, id: i64, r: &Record) -> Result<()> {
    let (kind, payload) = r.to_row();
    store.append_event(id, kind, &payload)?;
    Ok(())
}

fn agent(content: Vec<Block>) -> Record {
    Record::Agent(Event::Assistant { content })
}

fn text(s: impl Into<String>) -> Block {
    Block::Text { text: s.into() }
}

fn tool(id: &str, name: &str, input: serde_json::Value) -> Record {
    agent(vec![Block::ToolUse {
        id: id.into(),
        name: name.into(),
        input,
    }])
}

fn result(id: &str, content: impl Into<String>) -> Record {
    Record::Agent(Event::User {
        content: vec![Block::ToolResult {
            tool_use_id: id.into(),
            content: json!(content.into()),
            is_error: false,
        }],
    })
}

fn done(i: usize) -> Record {
    Record::Agent(Event::Result {
        is_error: false,
        duration_ms: 4000 + i as u64,
        total_cost_usd: 0.01 * i as f64,
        turn_cost_usd: Some(0.01),
        result: Some("Done.".into()),
        prompts: 1,
        usage: Some(Usage {
            input: 12_000,
            output: 400,
        }),
    })
}

fn short_thread(t: usize) -> Vec<Record> {
    vec![
        Record::Prompt(format!("Please look at the flaky test number {t}")),
        Record::Agent(Event::Init {
            session_id: format!("sess-{t}"),
            model: "claude-opus-5-5".into(),
        }),
        agent(vec![text(
            "I found the race: the reader drops the last line. Fixed and tested.",
        )]),
        done(t),
    ]
}

/// `turns` turns of a realistic working session, about 12 events each.
fn long_thread(turns: usize, dir: &Path) -> Vec<Record> {
    let file = dir.join("src/parser.rs").display().to_string();
    let code: String = (0..60)
        .map(|l| format!("    let field_{l} = read_field(&mut cursor, {l})?;\n"))
        .collect();
    let out: String = (0..20)
        .map(|l| format!("test parser::case_{l} ... ok\n"))
        .collect();
    let mut r = vec![Record::Agent(Event::Init {
        session_id: "sess-long".into(),
        model: "claude-opus-5-5".into(),
    })];
    for i in 0..turns {
        r.push(Record::Prompt(format!(
            "Turn {i}: the parser drops the last field when the line has no newline. Fix it and add a test."
        )));
        r.push(agent(vec![Block::Thinking {
            thinking: format!(
                "Turn {i}. The reader stops at EOF without flushing the cursor; check `read_field`."
            ),
        }]));
        r.push(agent(vec![text(format!(
            "Looking at **`read_field`** in [parser.rs](src/parser.rs) for turn {i}. The cursor is not flushed at EOF:\n\n```rust\nfn read_field(c: &mut Cursor, n: usize) -> Result<Field> {{\n    let raw = c.until(b',')?;\n    Field::parse(raw, n)\n}}\n```\n\n- first, keep the tail\n- then, test it"
        ))]));
        r.push(tool(&format!("r{i}"), "Read", json!({ "file_path": file })));
        r.push(result(&format!("r{i}"), code.clone()));
        r.push(tool(
            &format!("e{i}"),
            "Edit",
            json!({
                "file_path": file,
                "old_string": format!("    let raw = c.until(b',')?;\n    Field::parse(raw, {i})"),
                "new_string": format!("    let raw = c.until(b',').or_else(|_| c.rest())?;\n    Field::parse(raw, {i})\n    // turn {i}"),
                "replace_all": false
            }),
        ));
        r.push(result(&format!("e{i}"), "The file has been updated."));
        r.push(tool(
            &format!("b{i}"),
            "Bash",
            json!({ "command": "cargo test -p parser", "description": "Run the tests" }),
        ));
        r.push(result(
            &format!("b{i}"),
            format!("running 20 tests\n{out}\ntest result: ok. 20 passed"),
        ));
        match i % 10 {
            3 => r.push(agent(vec![text(
                "The flow now:\n\n```mermaid\nflowchart LR\n    A[Line] --> B{Newline?}\n    B -- yes --> C[Split]\n    B -- no --> D[Keep tail]\n    C --> E[(Fields)]\n    D --> E\n```",
            )])),
            6 => {
                let page = format!(
                    "<div style='display:flex;gap:12px;align-items:flex-end;height:160px'>{}</div>",
                    (1..=5)
                        .map(|b| format!("<div style='flex:1;height:{}%;background:var(--chart-{b})'></div>", 20 * b))
                        .collect::<String>()
                );
                let id = format!("h{i}");
                r.push(tool(
                    &id,
                    "mcp__skyelab__html_render",
                    json!({ "html": page, "title": format!("Parse times, turn {i}"), "height": 220 }),
                ));
                r.push(result(&id, "Rendered."));
            }
            8 => r.push(agent(vec![text(
                "| Case | Before | After |\n|---|---:|---:|\n| no newline | fail | ok |\n| CRLF | ok | ok |\n| empty | ok | ok |",
            )])),
            _ => {}
        }
        r.push(agent(vec![text(format!(
            "Fixed in turn {i}: the tail is kept and the test passes."
        ))]));
        r.push(done(i));
    }
    r
}
