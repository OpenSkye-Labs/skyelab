//! The Bots page's list, drawn in T3's scheduled tasks shape: a header with "New bot", the missed-runs setting, the
//! editor card while open, then one card per bot (title, prompt, what starts it · next run, last run, its thread, the
//! controls, and its run history when expanded).

use super::page::{Row, ScheduledPage};
use crate::ui;
use crate::ui::SkyeIcon;
use crate::ui::{Button, Switch};
use crate::workspace::OpenThread;
use gpui::{
    AnyElement, Context, IntoElement, ParentElement as _, Render, SharedString, Styled as _,
    Window, div, prelude::*,
};
use gpui_component::{ActiveTheme as _, Icon, h_flex, v_flex};
use skyelab_agent::Mode;
use skyelab_core::bots::{Trigger, parse_triggers};
use skyelab_core::schedule::Schedule;

/// The empty list's starters: a name, a prompt and the trigger it fires on (each opens the editor in Plan mode).
const STARTERS: [(&str, &str, Trigger); 2] = [
    (
        "PR review",
        "Review this pull request: summarize the change, then list bugs, risks and missing tests. Read the diff with \
         `gh pr diff`. Don't change any files.",
        Trigger::PrOpened,
    ),
    (
        "Issue triage",
        "Triage this issue: say what it asks, find the code it touches, and suggest a label and next step. Don't \
         change any files.",
        Trigger::IssueOpened,
    ),
];

/// "in 5m", "in 3h": a future time, the way the sidebar shows past ones.
fn until(now: i64, at: i64) -> String {
    if at <= now {
        return "now".into();
    }
    format!("in {}", ui::ago(at, now))
}

impl ScheduledPage {
    fn render_row(&self, first: bool, row: &Row, now: i64, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let (muted, danger) = (theme.muted_foreground, ui::ink(theme, theme.danger));
        let t = &row.task;
        let id = t.id;
        let schedule = t.schedule.parse::<Schedule>().ok().map(|s| s.label());
        let triggers = parse_triggers(&t.triggers);
        let when = match (t.enabled, t.next_run_at) {
            (false, _) if t.last_status.as_deref() == Some("paused") => "Paused".to_owned(),
            (false, _) => "Off".to_owned(),
            (true, Some(at)) => format!("Next run {}", until(now, at)),
            (true, None) if !triggers.is_empty() => "Watching GitHub".to_owned(),
            (true, None) => "Not scheduled".to_owned(),
        };
        let chips = schedule
            .into_iter()
            .chain(triggers.iter().map(|t| t.label().to_owned()))
            .chain([ScheduledPage::mode_label(Mode::parse(&t.permission_mode)).to_owned()])
            .map(ui::Pill::new);
        let poll_error = cx
            .try_global::<super::Scheduler>()
            .and_then(|s| s.polls.errors.get(&t.project_id).cloned())
            .filter(|_| !triggers.is_empty() && t.enabled)
            .map(|e| format!("GitHub: {e}"));
        let open = row.runs.is_some();
        let status = t.last_status.as_deref().map(|s| {
            let color = match s {
                "started" => theme.success,
                "paused" | "failed" => theme.danger,
                _ => theme.warning,
            };
            let at = t
                .last_run_at
                .map(|at| format!(" {}", ui::ago(now, at)))
                .unwrap_or_default();
            ui::Pill::new(format!("{}{at}", capitalize(s))).color(color)
        });
        let link = row.thread.clone().map(|(thread, title)| {
            div()
                .id(SharedString::from(format!("sched-thread-{id}")))
                .debug_selector(move || format!("sched-thread-{id}"))
                .max_w(gpui::px(240.))
                .truncate()
                .underline()
                .cursor_pointer()
                .hover(|d| d.text_color(cx.theme().foreground))
                .child(title)
                .on_click(move |_, window, cx| {
                    crate::workspace::dispatch(Box::new(OpenThread { id: thread }), window, cx)
                })
        });
        let armed = self.deleting == Some(id);
        let page = cx.entity().downgrade();
        let controls = h_flex()
            .flex_shrink_0()
            .gap_1()
            .child(
                Button::new(SharedString::from(format!("sched-history-{id}")))
                    .ghost()
                    .xsmall()
                    .icon(Icon::new(if open {
                        SkyeIcon::ChevronDown
                    } else {
                        SkyeIcon::ChevronRight
                    }))
                    .tooltip(if open { "Hide runs" } else { "Show runs" })
                    .on_click(cx.listener(move |this, _, _, cx| this.toggle_history(id, cx))),
            )
            .child(
                Switch::new(SharedString::from(format!("sched-on-{id}")))
                    .checked(t.enabled)
                    .accessibility_label(format!("Enable {}", t.title))
                    .on_click(
                        cx.listener(move |this, on: &bool, _, cx| this.set_enabled(id, *on, cx)),
                    ),
            )
            .child(
                Button::new(SharedString::from(format!("sched-run-{id}")))
                    .ghost()
                    .xsmall()
                    .icon(SkyeIcon::Play)
                    .tooltip("Run now")
                    .on_click(cx.listener(move |this, _, _, cx| this.run_now(id, cx))),
            )
            .child(
                Button::new(SharedString::from(format!("sched-edit-{id}")))
                    .ghost()
                    .xsmall()
                    .icon(Icon::new(SkyeIcon::PenLine))
                    .tooltip("Edit")
                    .on_click(move |_, window, cx| {
                        let _ = page.update(cx, |this, cx| this.edit(id, window, cx));
                    }),
            )
            .child(
                Button::new(SharedString::from(format!("sched-delete-{id}")))
                    .ghost()
                    .xsmall()
                    .when(armed, |b| b.danger().label("Delete?"))
                    .when(!armed, |b| {
                        b.icon(Icon::new(SkyeIcon::Trash)).tooltip("Delete")
                    })
                    .on_click(cx.listener(move |this, _, _, cx| this.delete(id, cx))),
            );
        let card = ui::group_row(first, theme)
            .items_start()
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_1()
                    .child(
                        div()
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .truncate()
                            .child(t.title.clone()),
                    )
                    .child(
                        div()
                            .id(SharedString::from(format!("sched-prompt-{id}")))
                            .text_size(ui::META)
                            .text_color(muted)
                            .line_clamp(2)
                            .text_ellipsis()
                            .child(t.prompt.clone()),
                    )
                    .child(
                        h_flex()
                            .flex_wrap()
                            .gap_2()
                            .text_size(ui::META)
                            .text_color(muted)
                            .child(format!("{} · {when}", row.project))
                            .children(chips)
                            .children(status)
                            .children(link),
                    )
                    .children(
                        poll_error.map(|e| div().text_size(ui::META).text_color(danger).child(e)),
                    )
                    .children(
                        t.last_error
                            .clone()
                            .filter(|_| t.last_status.as_deref() != Some("started"))
                            .map(|e| div().text_size(ui::META).text_color(danger).child(e)),
                    ),
            )
            .child(controls);
        v_flex()
            .child(card)
            .children(
                row.runs
                    .as_ref()
                    .map(|runs| self.render_history(id, runs, now, cx)),
            )
            .into_any_element()
    }
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map(|f| f.to_uppercase().chain(c).collect())
        .unwrap_or_default()
}

impl Render for ScheduledPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let now = super::now_ms(cx);
        let skip = super::settings(cx).skip_missed;
        let theme = cx.theme();
        let (muted, danger) = (theme.muted_foreground, ui::ink(theme, theme.danger));
        let header = h_flex()
            .gap_4()
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_1()
                    .child(ui::page_title("Bots", theme))
                    .child(div().text_size(ui::BODY).text_color(muted).child(
                        "A saved prompt that starts a new thread on a schedule or when GitHub has a new PR, a new issue \
                         or failing checks, while SkyeLab is running. Each run uses the account its folder is bound to.",
                    )),
            )
            .child(
                Button::new("sched-new")
                    .primary()
                    .small()
                    .icon(Icon::new(SkyeIcon::Plus))
                    .label("New bot")
                    .on_click(cx.listener(|this, _, window, cx| this.new_task(None, None, window, cx))),
            );
        let missed = ui::group(theme).child(
            ui::group_row(true, theme)
                .child(
                    v_flex()
                        .flex_1()
                        .gap_0p5()
                        .child(div().font_weight(gpui::FontWeight::MEDIUM).child("Run missed tasks at start"))
                        .child(div().text_size(ui::META).text_color(muted).child(
                            "A run missed while SkyeLab was closed runs once when it starts. Off skips it.",
                        )),
                )
                .child(
                    Switch::new("sched-missed")
                        .checked(!skip)
                        .accessibility_label("Run missed tasks at start")
                        .on_click(|on: &bool, _, cx| {
                            let s = skyelab_core::ScheduledSettings { skip_missed: !*on, ..super::settings(cx) };
                            super::save_settings(s, cx).detach();
                        }),
                ),
        );
        let editor = self.editor.as_ref().map(|e| self.render_editor(e, cx));
        let rows: Vec<AnyElement> = self
            .rows
            .iter()
            .enumerate()
            .map(|(i, r)| self.render_row(i == 0, r, now, cx))
            .collect();
        // The open New task form is the empty state's answer: the card would only repeat "No scheduled tasks" under it.
        let list = if rows.is_empty() && self.editor.is_some() {
            div().into_any_element()
        } else if rows.is_empty() {
            div()
                .debug_selector(|| "sched-empty".into())
                .child(
                    ui::group(cx.theme())
                        .p_6()
                        .child(ui::EmptyState::new("No bots yet").hint(
                            "Make one, start from a template, or pick \"Schedule this prompt…\" from a composer.",
                        ))
                        .child(h_flex().pt_3().gap_2().justify_center().children(STARTERS.iter().enumerate().map(
                            |(i, &(title, prompt, trigger))| {
                                Button::new(("sched-starter", i))
                                    .outline()
                                    .small()
                                    .icon(Icon::new(if trigger == Trigger::PrOpened { SkyeIcon::GitPullRequest } else { SkyeIcon::CircleDot }))
                                    .label(title)
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.new_from(title, prompt, trigger, window, cx)
                                    }))
                            },
                        ))),
                )
                .into_any_element()
        } else {
            ui::group(cx.theme()).children(rows).into_any_element()
        };
        v_flex()
            .id("scheduled-page")
            .track_focus(&self.focus_handle(cx))
            .gap_6()
            .child(header)
            .children(
                self.error
                    .clone()
                    .map(|e| div().text_size(ui::META).text_color(danger).child(e)),
            )
            .child(missed)
            .children(editor)
            .child(list)
    }
}

use gpui::Focusable as _;
