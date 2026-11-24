//! Acceptance: a project's own new-thread defaults over the app-wide ones (see `ThreadSettings::resolve_model`).

use crate::thread_defaults::{ThreadDefaults, ThreadField};
use crate::{AppSettings, Provider, ThreadSettings};
use std::path::Path;

fn pair(model: Option<&str>, effort: Option<&str>) -> (Option<String>, Option<String>) {
    (model.map(Into::into), effort.map(Into::into))
}

/// App: haiku/low, Ask. Project `/work`: opus (effort inherits), plan mode, worktrees on.
fn settings() -> ThreadSettings {
    let mut t = ThreadSettings {
        claude_model: Some("haiku".into()),
        claude_effort: Some("low".into()),
        ..Default::default()
    };
    t.projects.insert(
        "/work".into(),
        ThreadDefaults {
            claude_model: Some("opus".into()),
            permission_mode: Some("plan".into()),
            new_worktree: Some(true),
            ..Default::default()
        },
    );
    t
}

#[test]
fn thread_pick_wins() {
    let t = settings();
    let own = pair(Some("sonnet"), Some("high"));
    let got = t.resolve_model(
        Path::new("/work"),
        Provider::Claude,
        own.clone(),
        pair(None, None),
    );
    assert_eq!(got, own);
}

#[test]
fn project_model_beats_last_pick() {
    let t = settings();
    let got = t.resolve_model(
        Path::new("/work"),
        Provider::Claude,
        pair(None, None),
        pair(Some("sonnet"), None),
    );
    // The unset effort half comes from the remembered pick (its effort: the agent's default), not the app's.
    assert_eq!(got, pair(Some("opus"), None));
}

#[test]
fn last_pick_beats_app() {
    let t = settings();
    let last = pair(Some("sonnet"), None);
    let got = t.resolve_model(
        Path::new("/home"),
        Provider::Claude,
        pair(None, None),
        last.clone(),
    );
    assert_eq!(got, last);
    let got = t.resolve_model(
        Path::new("/home"),
        Provider::Claude,
        pair(None, None),
        pair(None, None),
    );
    assert_eq!(
        got,
        pair(Some("haiku"), Some("low")),
        "nothing picked: the app default"
    );
}

#[test]
fn unset_half_inherits() {
    let t = settings();
    let p = t.for_project(Path::new("/work"));
    assert_eq!(p.claude_model.as_deref(), Some("opus"));
    assert_eq!(
        p.claude_effort.as_deref(),
        Some("low"),
        "effort inherits the app's"
    );
    assert_eq!(p.permission_mode.as_deref(), Some("plan"));
    assert!(p.new_worktree);
    // A Codex thread in the same project: no Codex override, so the app's Codex default.
    let got = t.resolve_model(
        Path::new("/work"),
        Provider::Codex,
        pair(None, None),
        pair(None, None),
    );
    assert_eq!(got, pair(None, None));
    // Another project: the app defaults untouched.
    assert_eq!(t.for_project(Path::new("/home")), t);
    // Matching is by the whole folder, not a prefix.
    assert_eq!(t.for_project(Path::new("/work/sub")), t);
}

#[test]
fn round_trips_and_prunes_empty() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("settings.toml");
    let mut s = AppSettings {
        threads: settings(),
        ..Default::default()
    };
    s.save(&path).expect("save");
    assert_eq!(AppSettings::load(&path).expect("load"), s);

    // Setting every field back to inherit drops the project's table.
    let work = Path::new("/work");
    s.threads
        .set_project_field(work, ThreadField::ClaudeModel, None);
    s.threads
        .set_project_field(work, ThreadField::PermissionMode, None);
    s.threads.set_project_worktree(work, None);
    assert!(s.threads.projects.is_empty());
    s.save(&path).expect("save");
    let text = std::fs::read_to_string(&path).expect("read");
    assert!(!text.contains("projects"), "no empty table: {text}");

    s.threads
        .set_project_field(work, ThreadField::CodexEffort, Some("high".into()));
    assert_eq!(
        s.threads.projects[work]
            .field(ThreadField::CodexEffort)
            .as_deref(),
        Some("high")
    );
}
