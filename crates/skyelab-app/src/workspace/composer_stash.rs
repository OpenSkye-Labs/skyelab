//! The prompt stash (T3's `ComposerStashMenu`): mod+s sets the composer's text, files and chips aside and empties it;
//! a "Stash" tab on the composer's shoulder opens the list to restore (append after what is typed) or delete one.
//!
//! One list for the whole app, newest first, at most [`MAX`] (T3's). Each entry is a draft owned by `stash:<ms>`: its
//! text is [`Stashed`] as JSON, its files the draft's files. Every thread view draws from the [`PromptStash`] global.

use super::composer_context::ChipKind;
use super::composer_context::MAX_CONTEXTS;
use super::lock;
use super::thread::ThreadView;
use crate::ui;
use gpui::{App, AppContext as _, Context, Global, SharedString, Window, actions};
use serde::{Deserialize, Serialize};
use skyelab_agent::{Attachment, MAX_ATTACHMENTS};
use std::path::PathBuf;

actions!(skyelab, [StashPrompt]);

pub(in crate::workspace) const STASH_KEY: &str = if cfg!(target_os = "macos") {
    "cmd-s"
} else {
    "ctrl-s"
};
pub(super) const SHORTCUT: &str = if cfg!(target_os = "macos") {
    "⌘S"
} else {
    "Ctrl+S"
};
const PREFIX: &str = "stash:";
/// T3 keeps 20; a 21st drops the oldest.
const MAX: usize = 20;
const SNIPPET_CHARS: usize = 90;

/// What a stash entry's draft text holds.
#[derive(Debug, Default, Serialize, Deserialize)]
struct Stashed {
    text: String,
    #[serde(default)]
    chips: Vec<Chip>,
    /// The account the composer ran on: chips without a binding of their own (a quote of its agent's reply, terminal
    /// output, diff comments) go back only into a composer on the same account.
    #[serde(default)]
    account: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Chip {
    kind: ChipKind,
    label: String,
    text: String,
    /// Checked again on restore (`push_context`), so a rebind since keeps another account's thread out.
    bound: Option<(String, PathBuf)>,
}

impl Stashed {
    fn parse(raw: String) -> Self {
        // An entry this build can't read (a newer chip kind) gives back only its text: its chips and account stay out.
        #[derive(Deserialize)]
        struct TextOnly {
            text: String,
        }
        serde_json::from_str(&raw).unwrap_or_else(|_| Stashed {
            text: match serde_json::from_str::<TextOnly>(&raw) {
                Ok(t) => t.text,
                Err(_) if raw.trim_start().starts_with('{') => String::new(),
                Err(_) => raw,
            },
            ..Default::default()
        })
    }
}

/// Unbound chips go back only into a composer on the same, known account.
fn same_account(from: &Option<String>, here: &Option<String>) -> bool {
    from.is_some() && from == here
}

/// One entry as the menu lists it.
#[derive(Debug, Clone)]
pub(crate) struct StashRow {
    pub owner: String,
    pub snippet: SharedString,
    pub files: usize,
    pub chips: usize,
    pub at: i64,
}

/// The stash's entries, shared by every thread view; set again after each change.
#[derive(Default)]
pub(crate) struct PromptStash {
    rows: Vec<StashRow>,
}

impl Global for PromptStash {}

impl PromptStash {
    pub(crate) fn rows(cx: &App) -> &[StashRow] {
        cx.try_global::<Self>().map_or(&[], |s| &s.rows)
    }
}

fn snippet(s: &Stashed, files: usize) -> SharedString {
    let flat = s.text.split_whitespace().collect::<Vec<_>>().join(" ");
    if !flat.is_empty() {
        return match flat.char_indices().nth(SNIPPET_CHARS) {
            Some((at, _)) => format!("{}…", &flat[..at]).into(),
            None => flat.into(),
        };
    }
    match (s.chips.first(), files) {
        (Some(c), _) => format!("({})", c.label).into(),
        (None, 1) => "(1 file)".into(),
        (None, 0) => "(empty)".into(),
        (None, n) => format!("({n} files)").into(),
    }
}

/// `typed`, then `stashed` after a blank line.
fn appended(typed: &str, stashed: &str) -> String {
    match (typed.trim().is_empty(), stashed.is_empty()) {
        (true, _) => stashed.to_owned(),
        (false, true) => typed.to_owned(),
        (false, false) => format!("{}\n\n{stashed}", typed.trim_end()),
    }
}

impl ThreadView {
    /// Loads the list once per app, and redraws this view when it changes.
    pub(in crate::workspace) fn init_stash(&mut self, cx: &mut Context<Self>) {
        self._subs
            .push(cx.observe_global::<PromptStash>(|_, cx| cx.notify()));
        if !cx.has_global::<PromptStash>() {
            cx.set_global(PromptStash::default());
            self.reload_stash(cx);
        }
    }

    fn reload_stash(&mut self, cx: &mut Context<Self>) {
        let store = self.backend.store.clone();
        let task = cx.background_spawn(async move { lock(&store)?.drafts_with_prefix(PREFIX) });
        cx.spawn(async move |this, cx| {
            let result = task.await.map(|rows| {
                (rows.into_iter())
                    .map(|r| {
                        let stashed = Stashed::parse(r.text);
                        StashRow {
                            snippet: snippet(&stashed, r.files),
                            chips: stashed.chips.len(),
                            owner: r.owner,
                            files: r.files,
                            at: r.updated_at,
                        }
                    })
                    .collect()
            });
            let _ = this.update(cx, |this, cx| match result {
                Ok(rows) => cx.set_global(PromptStash { rows }),
                Err(e) => this.show_error(e, cx),
            });
        })
        .detach();
    }

    /// mod+s: sets the composer aside and empties it. An empty composer opens the list instead.
    pub(in crate::workspace) fn stash_prompt(
        &mut self,
        _: &StashPrompt,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text = self.prompt.read(cx).value().to_string();
        let account = self.composer_account().map(|a| a.id);
        let composer = &mut self.composer;
        if text.trim().is_empty() && composer.attachments.is_empty() && composer.contexts.is_empty()
        {
            composer.stash_open = match composer.stash_open {
                Some(_) => None,
                None => Some(0),
            };
            return cx.notify();
        }
        if self.read_only {
            return;
        }
        let stashed = Stashed {
            text,
            chips: (composer.contexts.drain(..))
                .map(|c| Chip {
                    kind: c.kind,
                    label: c.label.to_string(),
                    text: c.text,
                    bound: c.bound,
                })
                .collect(),
            account,
        };
        let files: Vec<(String, Vec<u8>)> = (composer.attachments.drain(..))
            .map(|a| (a.name, a.data))
            .collect();
        composer.stash_open = None;
        self.prompt.update(cx, |p, cx| p.set_value("", window, cx));
        cx.notify();
        let json = match serde_json::to_string(&stashed) {
            Ok(json) => json,
            Err(e) => return self.put_back(stashed, files, Some(e.into()), window, cx),
        };
        let store = self.backend.store.clone();
        let written = files.clone();
        let task = cx.background_spawn(async move {
            let mut s = lock(&store)?;
            let rows = s.drafts_with_prefix(PREFIX)?;
            // Owners are unique: two stashes in one millisecond must not replace each other.
            let last = rows
                .iter()
                .filter_map(|r| r.owner.strip_prefix(PREFIX)?.parse::<i64>().ok())
                .max();
            let owner = format!("{PREFIX}{}", ui::now_ms().max(last.map_or(0, |l| l + 1)));
            s.set_draft_text(&owner, &json)?;
            if let Err(e) = s.set_draft_files(&owner, &written) {
                s.delete_draft(&owner)?;
                return Err(e);
            }
            // The entry is stored: a failed trim must not put it back in the composer too.
            for old in rows.iter().skip(MAX - 1) {
                let _ = s.delete_draft(&old.owner);
            }
            anyhow::Ok(())
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |this, window, cx| match result {
                Ok(()) => this.reload_stash(cx),
                // Not stored: it goes back into the composer rather than being lost.
                Err(e) => this.put_back(stashed, files, Some(e), window, cx),
            });
        })
        .detach();
    }

    /// Takes the entry out of the stash and appends it to the composer.
    pub(in crate::workspace) fn restore_stash(
        &mut self,
        owner: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.composer.stash_open = None;
        cx.notify();
        if self.read_only {
            return;
        }
        // Check the caps before taking it out: past them a chip or a file would be lost, or the send refused.
        let row = PromptStash::rows(cx).iter().find(|r| r.owner == owner);
        let (chips, files) = row.map_or((0, 0), |r| (r.chips, r.files));
        let composer = &mut self.composer;
        if composer.contexts.len() + chips > MAX_CONTEXTS
            || composer.attachments.len() + files > MAX_ATTACHMENTS
        {
            composer.notice = Some(
                format!(
                    "Remove some chips or files first: a message carries up to {MAX_CONTEXTS} chips and \
                     {MAX_ATTACHMENTS} files."
                )
                .into(),
            );
            return;
        }
        let store = self.backend.store.clone();
        let task = cx.background_spawn(async move { lock(&store)?.take_draft(&owner) });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |this, window, cx| {
                match result {
                    Ok(d) => this.put_back(Stashed::parse(d.text), d.files, None, window, cx),
                    Err(e) => this.show_error(e, cx),
                }
                this.reload_stash(cx);
            });
        })
        .detach();
    }

    pub(in crate::workspace) fn delete_stash(&mut self, owner: String, cx: &mut Context<Self>) {
        let store = self.backend.store.clone();
        let task = cx.background_spawn(async move { lock(&store)?.delete_draft(&owner) });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                if let Err(e) = result {
                    this.show_error(e, cx);
                }
                this.reload_stash(cx);
            });
        })
        .detach();
    }

    fn put_back(
        &mut self,
        stashed: Stashed,
        files: Vec<(String, Vec<u8>)>,
        error: Option<anyhow::Error>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text = appended(&self.prompt.read(cx).value(), &stashed.text);
        self.prompt
            .update(cx, |p, cx| p.set_value(text, window, cx));
        // Read back through the same checks as an attached file.
        let files = files.into_iter();
        (self.composer.attachments).extend(files.filter_map(|(n, d)| Attachment::new(n, d).ok()));
        let here = self.composer_account().map(|a| a.id);
        let mut left_out = 0;
        for c in stashed.chips {
            if c.bound.is_none() && !same_account(&stashed.account, &here) {
                left_out += 1;
                continue;
            }
            self.push_context(c.kind, c.label.into(), c.text, c.bound, cx);
        }
        if left_out > 0 {
            self.composer.notice = Some(
                format!("{left_out} chip(s) left out: they came from another account's thread.")
                    .into(),
            );
        }
        if let Some(e) = error {
            self.show_error(e, cx);
        }
        self.focus_prompt(window, cx);
        cx.notify();
    }

    /// Up/Down/Enter/Esc while the list is open (the `@` menu's keys). `false`: it isn't open.
    pub(in crate::workspace) fn stash_key(
        &mut self,
        key: StashKey,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(selected) = self.composer.stash_open else {
            return false;
        };
        let rows = PromptStash::rows(cx);
        match key {
            StashKey::Move(by) if !rows.is_empty() => {
                let n = rows.len() as isize;
                self.composer.stash_open = Some((selected as isize + by).rem_euclid(n) as usize);
            }
            StashKey::Move(_) => {}
            StashKey::Close => self.composer.stash_open = None,
            StashKey::Accept => match rows.get(selected).map(|r| r.owner.clone()) {
                Some(owner) => self.restore_stash(owner, window, cx),
                None => self.composer.stash_open = None,
            },
        }
        cx.notify();
        true
    }
}

/// A list key, from the `@` menu's actions.
#[derive(Clone, Copy)]
pub(in crate::workspace) enum StashKey {
    Move(isize),
    Accept,
    Close,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appended_and_snippet() {
        assert_eq!(appended("now  \n", "stashed"), "now\n\nstashed");
        assert_eq!(appended(" ", "stashed"), "stashed");
        assert_eq!(appended("now", ""), "now");
        let s = |text: &str| Stashed {
            text: text.into(),
            ..Default::default()
        };
        assert_eq!(snippet(&s(" a\n\nb "), 0), "a b");
        assert_eq!(snippet(&s(""), 2), "(2 files)");
        assert_eq!(snippet(&s(&"x".repeat(100)), 0).chars().count(), 91);
        assert_eq!(Stashed::parse("not json".into()).text, "not json");
    }

    #[test]
    fn unreadable_entry_gives_back_only_its_text() {
        let raw = r#"{"text":"mine","chips":[{"kind":"FutureKind","label":"a","text":"ACME_ONLY","bound":null}],"account":"acme"}"#;
        let s = Stashed::parse(raw.into());
        assert_eq!(s.text, "mine");
        assert!(s.chips.is_empty());
        assert_eq!(Stashed::parse(r#"{"broken"#.into()).text, "");
    }

    #[test]
    fn unbound_chips_need_the_same_known_account() {
        let (a, b) = (Some("acme".to_string()), Some("beta".to_string()));
        assert!(same_account(&a, &a));
        assert!(!same_account(&a, &b));
        assert!(!same_account(&None, &a));
        assert!(!same_account(&None, &None), "no account on either side");
    }
}
