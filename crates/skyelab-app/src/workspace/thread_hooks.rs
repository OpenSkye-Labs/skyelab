//! What the working row names besides "Working for …": a hook the agent runs from the account's own config (Codex's
//! `hook/started` / `hook/completed`, Claude's `hook_started` / `hook_response`), and while a launch hasn't said
//! anything yet, the startup step its events show (T3's "Starting Claude", "Starting MCP servers", "Resuming session").

use super::ThreadView;
use crate::ui;
use gpui::Context;
use skyelab_agent::{Event, McpServer};
use skyelab_core::Provider;

/// A launch whose agent hasn't reported its session or any output yet.
pub(in crate::workspace) struct Startup {
    pub(in crate::workspace) provider: Provider,
    /// It continues a stored session (`--resume`, Codex's `thread/resume`).
    pub(in crate::workspace) resuming: bool,
}

/// The step a launch is on: once its process runs (`spawned`), the MCP servers it still connects; else resuming or
/// starting the agent. Only from events that arrived: nothing here guesses by time.
pub(super) fn startup_step(startup: &Startup, spawned: bool, mcp: &[McpServer]) -> String {
    let connecting = mcp
        .iter()
        .filter(|s| matches!(s.status.as_str(), "starting" | "pending"))
        .count();
    let name = startup.provider.label();
    if spawned && connecting > 0 && startup.resuming {
        format!("Resuming {name} session, starting MCP servers ({connecting})…")
    } else if spawned && connecting > 0 {
        format!("Starting MCP servers ({connecting})…")
    } else if startup.resuming {
        format!("Resuming {name} session…")
    } else {
        format!("Starting {name}…")
    }
}

/// Whether `event` ends the startup step: the session is up (init) or the agent answered.
pub(super) fn ends_startup(event: &Event) -> bool {
    !matches!(
        event,
        Event::Mcp { .. }
            | Event::Hook { .. }
            | Event::Stderr { .. }
            | Event::Unknown(_)
            | Event::RateLimit { .. }
            | Event::Context { .. }
    )
}

/// After this long a running hook gets a hint under the working row.
pub(super) const SLOW_HOOK_MS: i64 = 30_000;

/// A hook running now: its run id, what it is, and since when (ms since the epoch).
pub(in crate::workspace) struct Hook {
    id: String,
    name: String,
    since_ms: i64,
}

/// The working row for hook `name` that has run `ran_ms`, and the hint once it is slow.
pub(super) fn hook_line(name: &str, ran_ms: i64) -> (String, Option<String>) {
    let line = format!("Running hook {name}… {}", ui::elapsed(ran_ms));
    let hint = (ran_ms >= SLOW_HOOK_MS).then(|| {
        format!(
            "This hook comes from the account's own hooks config and is taking a while. Interrupt ({}) stops the turn.",
            ui::keys("⌘.")
        )
    });
    (line, hint)
}

impl ThreadView {
    /// A hook started or finished.
    pub(in crate::workspace) fn on_hook(
        &mut self,
        id: String,
        name: String,
        running: bool,
        cx: &mut Context<Self>,
    ) {
        self.hooks.retain(|h| h.id != id);
        if running {
            self.hooks.push(Hook {
                id,
                name,
                since_ms: ui::now_ms(),
            });
        }
        cx.notify();
    }

    /// The working row's text while a hook runs (the newest one), with its hint.
    pub(super) fn running_hook(&self) -> Option<(String, Option<String>)> {
        let h = self.hooks.last()?;
        Some(hook_line(&h.name, ui::now_ms() - h.since_ms))
    }

    /// The working row's text while the launch starts: its step and how long the turn has run.
    pub(super) fn startup_line(&self) -> Option<String> {
        let ran = ui::elapsed(ui::now_ms() - self.turn_started_ms.unwrap_or_else(ui::now_ms));
        let step = startup_step(self.startup.as_ref()?, !self.starting, &self.details.mcp);
        Some(format!("{step} {ran}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hook_line_names_the_hook_and_hints_once_slow() {
        let (line, hint) = hook_line("sessionStart (Reporting status to Orca)", 12_000);
        assert_eq!(
            line,
            "Running hook sessionStart (Reporting status to Orca)… 12s"
        );
        assert!(hint.is_none());
        let (_, hint) = hook_line("sessionStart", SLOW_HOOK_MS);
        assert!(hint.is_some_and(|h| h.contains("Interrupt")));
    }

    fn server(status: &str) -> McpServer {
        McpServer {
            name: status.into(),
            status: status.into(),
            error: None,
        }
    }

    #[test]
    fn the_startup_step_follows_the_events_that_arrived() {
        let start = Startup {
            provider: Provider::Claude,
            resuming: false,
        };
        let mcp = [server("starting"), server("pending"), server("connected")];
        assert_eq!(
            startup_step(&start, false, &mcp),
            "Starting Claude…",
            "not spawned yet: the list is the last run's"
        );
        assert_eq!(
            startup_step(&start, true, &mcp),
            "Starting MCP servers (2)…"
        );
        assert_eq!(startup_step(&start, true, &mcp[2..]), "Starting Claude…");
        let resume = Startup {
            provider: Provider::Codex,
            resuming: true,
        };
        assert_eq!(startup_step(&resume, true, &[]), "Resuming Codex session…");
        assert_eq!(
            startup_step(&resume, true, &mcp),
            "Resuming Codex session, starting MCP servers (2)…"
        );
        assert!(!ends_startup(&Event::Mcp {
            servers: vec![],
            launch: true,
            warning: None
        }));
        assert!(ends_startup(&Event::Init {
            session_id: "s".into(),
            model: "m".into()
        }));
    }
}
