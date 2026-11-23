//! "Fork from here" on a finished reply: a new thread in the same project, under the same account, with the history
//! up to that reply copied in. Agent sessions can't be forked, so its first session starts with a context prompt
//! built from that history (stored on the row until the session reports its id).

use crate::workspace::thread_model::{Item, Record, apply};
use crate::workspace::{OpenThread, Workspace, lock};
use anyhow::{Context as _, bail};
use gpui::{Action, AnyElement, AppContext as _, Context, SharedString, Window, div, prelude::*};
use gpui_component::{
    Icon, Sizable as _,
    button::{Button, ButtonVariants as _},
};
use skyelab_agent::{Block, Event};
use skyelab_store::NewThread;

/// The most of the copied history the context prompt carries; the newest part is kept.
const MAX_CONTEXT: usize = 60_000;
/// A side chat's summary of its parent is shorter: it is context for a question, not a history to continue.
const MAX_SIDE_CONTEXT: usize = 12_000;

/// Forks thread `id` after its `turns`-th finished turn.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = skyelab, no_json)]
pub struct ForkThread {
    pub id: i64,
    pub turns: usize,
}

/// The reply row's fork button.
pub(in crate::workspace) fn button(action: ForkThread) -> AnyElement {
    div()
        .opacity(0.6)
        .child(
            Button::new(SharedString::from(format!("fork-{}", action.turns)))
                .ghost()
                .xsmall()
                .icon(Icon::new(gpui_kit_assets::IconName::GitBranch))
                .tooltip("Fork from here")
                .on_click(move |_, window, cx| window.dispatch_action(action.boxed_clone(), cx)),
        )
        .into_any_element()
}

/// `events` up to and including the turn whose result the timeline shows as its `turns`-th finished turn (all of them
/// if there are fewer). Counted as the timeline counts, so a result it draws no row for (a gone session) isn't a turn.
pub(in crate::workspace) fn cut(
    events: &[skyelab_store::Event],
    turns: usize,
) -> &[skyelab_store::Event] {
    let done = |items: &[Item]| {
        items
            .iter()
            .filter(|i| matches!(i, Item::Done { .. }))
            .count()
    };
    let mut items = Vec::new();
    for (i, e) in events.iter().enumerate() {
        let Ok(record) = Record::from_row(&e.kind, &e.payload_json) else {
            continue;
        };
        let result = matches!(record, Record::Agent(Event::Result { .. }));
        apply(&mut items, &record);
        if result && done(&items) == turns {
            return &events[..=i];
        }
    }
    events
}

/// The copied conversation's messages (tool calls left out), at most `cap` characters of its newest part.
fn conversation(events: &[skyelab_store::Event], cap: usize) -> String {
    let mut log = String::new();
    for e in events {
        match Record::from_row(&e.kind, &e.payload_json) {
            Ok(Record::Prompt(t) | Record::Steer(t)) => log.push_str(&format!("User: {t}\n\n")),
            Ok(Record::Agent(Event::Assistant { content })) => {
                for b in content {
                    if let Block::Text { text } = b {
                        log.push_str(&format!("Assistant: {text}\n\n"));
                    }
                }
            }
            _ => {}
        }
    }
    let n = log.chars().count();
    if n > cap {
        log = format!(
            "[earlier messages left out]\n\n{}",
            log.chars().skip(n - cap).collect::<String>()
        );
    }
    log
}

/// The prompt the fork's first session starts with, ahead of the user's message: the copied conversation, at most
/// [`MAX_CONTEXT`] characters of its newest part.
pub(in crate::workspace) fn context_prompt(title: &str, events: &[skyelab_store::Event]) -> String {
    format!(
        "This thread is a fork of the thread \"{title}\". Its earlier session isn't available, so here is that \
         conversation up to the fork (tool calls left out). Take it as context, then answer the new message after \
         it.\n\n<conversation>\n{}</conversation>\n\nNew message:\n",
        conversation(events, MAX_CONTEXT)
    )
}

/// The prompt a side chat's first session starts with: a short summary of the main thread (its newest
/// [`MAX_SIDE_CONTEXT`] characters), and that it answers on the side without changing files.
pub(in crate::workspace) fn side_prompt(title: &str, events: &[skyelab_store::Event]) -> String {
    format!(
        "This is a side chat next to the thread \"{title}\", in the same folder. Another agent is working in that \
         thread, so answer questions here and don't change any files. Here is the newest part of that thread's \
         conversation (tool calls left out), as context.\n\n<conversation>\n{}</conversation>\n\nQuestion:\n",
        conversation(events, MAX_SIDE_CONTEXT)
    )
}

impl Workspace {
    /// Copies the thread's history up to the picked reply into a new thread and opens it. Refused, creating nothing,
    /// when the project's folder is no longer bound to the thread's account.
    pub(in crate::workspace) fn fork_thread(
        &mut self,
        a: &ForkThread,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.is_remote() {
            return; // another machine's threads fork there
        }
        let (store, config, id, turns) = (
            self.backend.store.clone(),
            self.accounts_job(cx),
            a.id,
            a.turns,
        );
        let task = cx.background_spawn(async move {
            let config = config();
            let mut s = lock(&store)?;
            let src = s.thread(id)?.context("the thread is gone")?;
            let project = s.project(src.project_id)?.context("its project is gone")?;
            match config.resolve(&project.path) {
                Some(a) if a.id == src.account_id => {}
                _ => bail!(
                    "{} isn't bound to {} any more, so this thread can't be forked",
                    project.path.display(),
                    src.account_id
                ),
            }
            let events = s.events_since(id, 0)?;
            let kept = cut(&events, turns);
            let context = context_prompt(&src.title, kept);
            s.create_fork(
                &NewThread {
                    project_id: project.id,
                    title: &src.title,
                    cwd: &project.path,
                    account_id: &src.account_id,
                    status: "stopped",
                },
                id,
                &context,
                kept,
            )
        });
        cx.spawn_in(window, async move |this, cx| {
            let done = task.await;
            let _ = this.update_in(cx, |ws, window, cx| match done {
                Ok(fork) => {
                    let id = fork.id;
                    ws.thread_started(&fork, cx);
                    ws.open_thread(&OpenThread { id }, window, cx);
                }
                Err(e) => ws.say(format!("Couldn't fork the thread: {e:#}"), window, cx),
            });
        })
        .detach();
    }
}

/// "Forked from <title>" in the header.
pub(in crate::workspace) fn forked_label(title: &SharedString) -> SharedString {
    format!("Forked from {title}").into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(seq: i64, kind: &str, payload: &str) -> skyelab_store::Event {
        skyelab_store::Event {
            thread_id: 1,
            seq,
            kind: kind.into(),
            payload_json: payload.into(),
            created_at: 0,
        }
    }

    fn result() -> String {
        Record::Agent(Event::Result {
            is_error: false,
            duration_ms: 1,
            total_cost_usd: 0.0,
            result: None,
            prompts: 1,
            usage: None,
        })
        .to_row()
        .1
    }

    #[test]
    fn cut_stops_after_the_picked_turn_and_context_holds_messages_only() {
        let reply = Record::Agent(Event::Assistant {
            content: vec![Block::Text {
                text: "first answer".into(),
            }],
        })
        .to_row()
        .1;
        let events = [
            ev(1, "prompt", "\"one\""),
            ev(2, "agent", &reply),
            ev(3, "agent", &result()),
            ev(4, "prompt", "\"two\""),
            ev(5, "agent", &result()),
        ];
        assert_eq!(cut(&events, 1).len(), 3);
        assert_eq!(cut(&events, 2).len(), 5);
        assert_eq!(cut(&events, 9).len(), 5);
        let c = context_prompt("Login", cut(&events, 1));
        assert!(
            c.contains("User: one") && c.contains("Assistant: first answer"),
            "{c}"
        );
        assert!(!c.contains("two"));
        let big = ev(
            1,
            "prompt",
            &serde_json::Value::String("x".repeat(100_000)).to_string(),
        );
        assert!(context_prompt("t", &[big]).len() < MAX_CONTEXT + 1000);
    }
}
