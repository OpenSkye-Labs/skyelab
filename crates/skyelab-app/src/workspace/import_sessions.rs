//! "Import sessions…": the project's existing Claude Code or Codex sessions become threads. Only the account the
//! folder is bound to is read, and only sessions whose folder still resolves to that account are offered. Sending
//! in an imported thread resumes its session.

use super::thread_model::Record;
use super::{Workspace, lock};
use crate::ui;
use crate::ui::A11yScrollArea as _;
use crate::ui::{Button, Checkbox};
use anyhow::{Context as _, Result, bail};
use gpui::{
    App, AppContext as _, Context, InteractiveElement as _, IntoElement, ParentElement as _,
    Render, SharedString, StatefulInteractiveElement as _, Styled as _, WeakEntity, Window,
    actions, div, prelude::FluentBuilder as _,
};
use gpui_component::{ActiveTheme as _, WindowExt as _, h_flex, v_flex};
use skyelab_agent::import::{self, Entry, MAX_TRANSCRIPT_BYTES, Transcript};
use skyelab_core::{Account, AccountsConfig};
use skyelab_store::{NewThread, Project, Thread};

actions!(skyelab, [ImportSessions]);

/// What the picker lists: the bound account and its sessions for the folder.
pub(super) struct Scan {
    pub account: Account,
    pub rows: Vec<Row>,
}

pub(super) struct Row {
    pub t: Transcript,
    /// A thread (of any project) already continues this session.
    pub imported: bool,
    pub checked: bool,
}

/// Lists the sessions `project`'s bound account has for its folder. Never reads another account's dir.
pub(super) fn scan(
    config: &AccountsConfig,
    project: &Project,
    taken: &std::collections::HashSet<String>,
) -> Result<Scan> {
    let Some(account) = config.resolve(&project.path).cloned() else {
        bail!(
            "{} isn't bound to an account. Bind it in Settings, then import its sessions.",
            project.path.display()
        );
    };
    // Agents record the real folder (`/private/tmp`, not `/tmp`): compare with that.
    let folder = project.path.canonicalize().map_or_else(
        |_| project.path.clone(),
        |p| skyelab_core::files::without_verbatim(&p),
    );
    let rows = import::list(account.provider, &account.config_dir, &folder)
        .into_iter()
        // A subfolder bound to another account runs there: its sessions aren't this account's to continue.
        .filter(|t| config.resolve(&t.cwd).is_some_and(|a| a.id == account.id))
        .map(|t| Row {
            imported: taken.contains(&t.id),
            checked: false,
            t,
        })
        .collect();
    Ok(Scan { account, rows })
}

/// What an import did, for the notice.
#[derive(Debug, Default)]
pub(super) struct Imported {
    pub threads: Vec<Thread>,
    /// Lines that didn't parse, over every session.
    pub skipped: usize,
    /// Sessions read only up to the size cap.
    pub truncated: usize,
    /// Sessions some project got a thread for (imported or started) after the picker listed them.
    pub already: usize,
    /// Sessions not imported: unreadable, no longer this account's, or not saved.
    pub left: usize,
}

/// Imports `picks` into `project` as `account_id`'s threads, checking the bindings again as they are now.
pub(super) fn import_picks(
    store: &super::SharedStore,
    config: &AccountsConfig,
    project: &Project,
    account_id: &str,
    picks: &[Transcript],
) -> Result<Imported> {
    let account = config
        .resolve(&project.path)
        .filter(|a| a.id == account_id)
        .with_context(|| {
            format!(
                "{} isn't bound to {account_id} any more",
                project.path.display()
            )
        })?;
    let mut done = Imported::default();
    for pick in picks {
        let ours = pick.path.starts_with(&account.config_dir)
            && config
                .resolve(&pick.cwd)
                .is_some_and(|a| a.id == account.id);
        let read = ours
            .then(|| {
                import::read(
                    account.provider,
                    &account.config_dir,
                    &pick.path,
                    MAX_TRANSCRIPT_BYTES,
                )
            })
            .flatten()
            .filter(|t| t.id == pick.id && t.cwd == pick.cwd);
        let Some(t) = read else {
            done.left += 1;
            continue;
        };
        // Each entry at the session's own time; entries before the first timed line at the file's.
        let rows: Vec<(&str, String, i64)> = t
            .entries
            .into_iter()
            .zip(t.times.iter().chain(std::iter::repeat(&0)))
            .map(|(e, &at)| {
                let (kind, payload) = match e {
                    Entry::Prompt(p) => Record::Prompt(p),
                    Entry::Agent(e) => Record::Agent(e),
                }
                .to_row();
                (kind, payload, if at > 0 { at } else { t.modified_ms })
            })
            .collect();
        let new = NewThread {
            project_id: project.id,
            title: &t.title,
            cwd: &t.cwd,
            account_id: &account.id,
            status: "stopped",
        };
        // One failed write leaves that session out; the ones already stored still reach the sidebar.
        let stored =
            lock(store).and_then(|mut s| s.import_thread(&new, &t.id, t.modified_ms, &rows));
        match stored {
            Ok(Some(thread)) => {
                done.skipped += t.skipped;
                done.truncated += usize::from(t.truncated);
                done.threads.push(thread);
            }
            Ok(None) => done.already += 1,
            Err(_) => done.left += 1,
        }
    }
    Ok(done)
}

/// The picker dialog's body: one checkbox row per session.
pub(super) struct ImportPicker {
    pub project: Project,
    pub account: Account,
    pub rows: Vec<Row>,
    ws: WeakEntity<Workspace>,
}

impl ImportPicker {
    pub(super) fn picked(&self) -> Vec<Transcript> {
        self.rows
            .iter()
            .filter(|r| r.checked && !r.imported)
            .map(|r| r.t.clone())
            .collect()
    }

    pub(super) fn toggle(&mut self, ix: usize, cx: &mut Context<Self>) {
        if let Some(r) = self.rows.get_mut(ix).filter(|r| !r.imported) {
            r.checked = !r.checked;
            cx.notify();
        }
    }

    fn toggle_all(&mut self, cx: &mut Context<Self>) {
        let all = self.rows.iter().all(|r| r.imported || r.checked);
        for r in self.rows.iter_mut().filter(|r| !r.imported) {
            r.checked = !all;
        }
        cx.notify();
    }

    /// Closes the dialog and imports the checked sessions.
    pub(super) fn import(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let picks = self.picked();
        if picks.is_empty() {
            return;
        }
        window.close_dialog(cx);
        let (project, account) = (self.project.clone(), self.account.id.clone());
        let _ = self.ws.update(cx, |ws, cx| {
            ws.run_import(project, account, picks, window, cx)
        });
    }
}

impl Render for ImportPicker {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let _render = crate::debug_renders::count("ImportPicker");
        let theme = cx.theme();
        let (now, project) = (ui::now_ms(), &self.project.path);
        let open = self.rows.iter().filter(|r| !r.imported).count();
        let all = open > 0 && self.rows.iter().all(|r| r.imported || r.checked);
        let caption = format!(
            "{} · {} for this folder",
            self.account.label,
            match self.rows.len() {
                1 => "1 session".to_owned(),
                n => format!("{n} sessions"),
            }
        );
        if self.rows.is_empty() {
            return v_flex()
                .gap_1()
                .text_size(ui::BODY)
                .child(format!(
                    "{} has no sessions for this folder.",
                    self.account.label
                ))
                .child(
                    div()
                        .text_size(ui::META)
                        .text_color(theme.muted_foreground)
                        .child(format!("Looked in {}", self.account.config_dir.display())),
                );
        }
        let rows = self.rows.iter().enumerate().map(|(ix, r)| {
            let mut meta = vec![
                ui::ago(now, r.t.modified_ms),
                match r.t.messages {
                    1 => "1 message".to_owned(),
                    n => format!("{n} messages"),
                },
            ];
            if let Ok(sub) = r.t.cwd.strip_prefix(project)
                && !sub.as_os_str().is_empty()
            {
                meta.push(sub.display().to_string());
            }
            h_flex()
                .id(SharedString::from(format!("import-{}", r.t.id)))
                .role(gpui::accesskit::Role::CheckBox)
                .aria_label(r.t.title.clone())
                .aria_toggled((r.checked || r.imported).into())
                .min_h(ui::ROW_H + gpui::px(8.))
                .px_2()
                .gap_2p5()
                .rounded(ui::RADIUS)
                .when(!r.imported, |row| {
                    row.hover(|s| s.bg(theme.accent))
                        .on_click(cx.listener(move |this, _, _, cx| this.toggle(ix, cx)))
                })
                .child(
                    Checkbox::new(SharedString::from(format!("import-check-{}", r.t.id)))
                        .checked(r.checked || r.imported)
                        .decorative()
                        // The row takes the click: a handler here too would toggle twice.
                        .disabled(r.imported),
                )
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .py_1()
                        .child(
                            div()
                                .text_size(ui::BODY)
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .when(r.imported, |d| d.text_color(theme.muted_foreground))
                                .truncate()
                                .child(r.t.title.clone()),
                        )
                        .child(
                            div()
                                .text_size(ui::META)
                                .text_color(theme.muted_foreground)
                                .truncate()
                                .child(meta.join(" · ")),
                        ),
                )
                .when(r.imported, |row| row.child(ui::Pill::new("Imported")))
        });
        v_flex()
            .gap_2()
            .child(
                h_flex()
                    .justify_between()
                    .gap_2()
                    .text_size(ui::META)
                    .text_color(theme.muted_foreground)
                    .child(caption)
                    .when(open > 0, |h| {
                        h.child(
                            Checkbox::new("import-all")
                                .xsmall()
                                .label("Select all")
                                .checked(all)
                                .on_click(cx.listener(|this, _, _, cx| this.toggle_all(cx))),
                        )
                    }),
            )
            .child(
                v_flex()
                    .id("import-rows")
                    .max_h(gpui::px(360.))
                    .overflow_y_scroll()
                    .a11y_scroll_area()
                    .children(rows),
            )
    }
}

impl Workspace {
    /// The palette's entry: the selected project's sessions.
    pub(super) fn import_sessions(
        &mut self,
        _: &ImportSessions,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match self.project.as_ref().map(|p| p.id) {
            Some(id) => self.open_import(id, window, cx),
            None => self.say("Pick a project first.", window, cx),
        }
    }

    /// Lists project `id`'s sessions off the UI thread, then opens the picker.
    pub(super) fn open_import(&mut self, id: i64, window: &mut Window, cx: &mut Context<Self>) {
        if self.is_remote() {
            return; // another machine's sessions live there
        }
        let Some(project) = self.projects.iter().find(|p| p.id == id).cloned() else {
            return;
        };
        let (store, config) = (self.backend.store.clone(), self.accounts_job(cx));
        let job = cx.background_spawn({
            let project = project.clone();
            async move {
                let taken = lock(&store)?.session_ids()?;
                scan(&config(), &project, &taken)
            }
        });
        cx.spawn_in(window, async move |this, cx| {
            let scanned = job.await;
            let _ = this.update_in(cx, |ws, window, cx| match scanned {
                Ok(s) => ws.show_picker(project, s, window, cx),
                Err(e) => ws.say(format!("Couldn't list sessions: {e:#}"), window, cx),
            });
        })
        .detach(); // short; `this` is weak
    }

    fn show_picker(
        &mut self,
        project: Project,
        s: Scan,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ws = cx.entity().downgrade();
        let picker = cx.new(|_| ImportPicker {
            project,
            account: s.account,
            rows: s.rows,
            ws,
        });
        #[cfg(test)]
        {
            self.import_picker = Some(picker.downgrade());
        }
        crate::ui::open_dialog(window, cx, move |dialog, _, cx| {
            let n = picker.read(cx).picked().len();
            let go = picker.downgrade();
            dialog
                .w(gpui::px(560.))
                .title("Import sessions")
                .child(picker.clone())
                .footer(
                    ui::dialog_footer()
                        .child(ui::dialog_cancel("import-cancel"))
                        .child(
                            Button::new("import-go")
                                .primary()
                                .small()
                                .label(if n == 1 {
                                    "Import 1".to_owned()
                                } else {
                                    format!("Import {n}")
                                })
                                .disabled(n == 0)
                                .on_click(move |_, window, cx: &mut App| {
                                    let _ = go.update(cx, |p, cx| p.import(window, cx));
                                }),
                        ),
                )
        });
    }

    pub(super) fn run_import(
        &mut self,
        project: Project,
        account_id: String,
        picks: Vec<Transcript>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (store, config) = (self.backend.store.clone(), self.accounts_job(cx));
        let job = cx.background_spawn(async move {
            import_picks(&store, &config(), &project, &account_id, &picks)
        });
        cx.spawn_in(window, async move |this, cx| {
            let done = job.await;
            let _ = this.update_in(cx, |ws, window, cx| match done {
                Ok(done) => {
                    for t in &done.threads {
                        ws.thread_started(t, cx);
                    }
                    let text = summary(&done);
                    #[cfg(test)]
                    ws.notices.push(text.clone().into());
                    crate::ui::toast::push(window, crate::ui::toast::Toast::info(text), cx);
                }
                Err(e) => ws.say(format!("Couldn't import: {e:#}"), window, cx),
            });
        })
        .detach();
    }
}

pub(super) fn summary(done: &Imported) -> String {
    let mut text = match done.threads.len() {
        1 => "Imported 1 session.".to_owned(),
        n => format!("Imported {n} sessions."),
    };
    if done.already > 0 {
        text += &format!(
            " {} already had a thread in a project, so left alone: a session continues in one thread only.",
            done.already
        );
    }
    if done.left > 0 {
        text += &format!(
            " {} left out (unreadable, bound elsewhere, or not saved).",
            done.left
        );
    }
    if done.skipped > 0 {
        text += &format!(" {} unreadable lines skipped.", done.skipped);
    }
    if done.truncated > 0 {
        text += &format!(" {} only partly read (too large).", done.truncated);
    }
    text
}
