//! Settings → Scheduled tasks, drawn in T3's shape: a header with "New task", the missed-runs setting, the editor
//! card while open, then one row per task (title, prompt, schedule · next run, last run, its thread, the controls).

use super::form::Kind;
use super::page::{Editor, Pick, Row, ScheduledPage};
use crate::ui;
use crate::ui::SkyeIcon;
use crate::ui::{Button, Checkbox, Switch, TextField};
use crate::workspace::OpenThread;
use gpui::{
    AnyElement, Context, IntoElement, ParentElement as _, Render, SharedString, Styled as _,
    Window, div, prelude::*,
};
use gpui_component::{
    ActiveTheme as _, Icon, h_flex,
    input::Textarea,
    menu::{DropdownMenu as _, PopupMenuItem},
    v_flex,
};
use skyelab_agent::Mode;
use skyelab_core::schedule::Schedule;

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
        let schedule = t
            .schedule
            .parse::<Schedule>()
            .map_or_else(|_| t.schedule.clone(), |s| s.label());
        let when = match (t.enabled, t.next_run_at) {
            (false, _) => "Paused".to_owned(),
            (true, Some(at)) => format!("Next run {}", until(now, at)),
            (true, None) => "Not scheduled".to_owned(),
        };
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
        ui::group_row(first, theme)
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
                            .child(format!("{} · {schedule} · {when}", row.project))
                            .children(status)
                            .children(link),
                    )
                    .children(
                        t.last_error
                            .clone()
                            .filter(|_| t.last_status.as_deref() != Some("started"))
                            .map(|e| div().text_size(ui::META).text_color(danger).child(e)),
                    ),
            )
            .child(controls)
            .into_any_element()
    }

    fn render_editor(&self, e: &Editor, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let (muted, danger) = (theme.muted_foreground, ui::ink(theme, theme.danger));
        let d = &e.draft;
        let field = |label: &str, control: AnyElement| {
            v_flex()
                .gap_1p5()
                .child(
                    div()
                        .text_size(ui::META)
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(muted)
                        .child(label.to_owned()),
                )
                .child(control)
        };
        let page = cx.entity().downgrade();
        let menu = |id: &'static str, label: String, items: Vec<(String, bool, Pick)>| {
            let page = page.clone();
            Button::new(id)
                .outline()
                .small()
                .label(label)
                .dropdown_caret(true)
                .dropdown_menu(move |mut menu, _, _| {
                    for (label, on, pick) in items.clone() {
                        let page = page.clone();
                        menu = menu.item(PopupMenuItem::new(label).checked(on).on_click(
                            move |_, window, cx| {
                                let _ = page.update(cx, |this, cx| this.pick(pick, window, cx));
                            },
                        ));
                    }
                    menu
                })
        };
        let project_name = d
            .project
            .and_then(|id| self.projects.iter().find(|p| p.id == id))
            .map_or_else(|| "Pick a project".to_owned(), |p| p.name.clone());
        let projects = self
            .projects
            .iter()
            .map(|p| (p.name.clone(), d.project == Some(p.id), Pick::Project(p.id)))
            .collect();
        let kinds = Kind::ALL
            .iter()
            .map(|&k| (k.label().to_owned(), d.kind == k, Pick::Kind(k)))
            .collect();
        let modes = Mode::ALL
            .iter()
            .map(|&m| {
                (
                    ScheduledPage::mode_label(m).to_owned(),
                    d.mode == m,
                    Pick::Mode(m),
                )
            })
            .collect();
        let places = [(false, "Local checkout"), (true, "New worktree")]
            .iter()
            .map(|&(w, l)| (l.to_owned(), d.worktree == w, Pick::Worktree(w)))
            .collect();
        let where_label = if d.worktree {
            "New worktree"
        } else {
            "Local checkout"
        };
        v_flex()
            .id("sched-editor")
            .gap_4()
            .p_4()
            .rounded(ui::RADIUS_LG)
            .border_1()
            .border_color(theme.border)
            .bg(theme.popover)
            .child(ui::section_title(if d.id == 0 { "New task" } else { "Edit task" }, theme))
            .child(h_flex().gap_3().child(div().flex_1().child(field("Name", TextField::new(&e.title).small().into_any_element()))).child(field("Project", menu("sched-project", project_name, projects).into_any_element())))
            .child(field("Prompt", Textarea::new(&e.prompt).into_any_element()))
            .child(
                h_flex()
                    .gap_3()
                    .items_end()
                    .child(field("Schedule", menu("sched-kind", d.kind.label().to_owned(), kinds).into_any_element()))
                    .child(div().w(gpui::px(160.)).child(field(if d.kind == Kind::Cron { "Expression" } else { "Value" }, TextField::new(&e.value).small().into_any_element())))
                    .child(field("Runs in", menu("sched-where", where_label.to_owned(), places).into_any_element()))
                    .child(field("Permissions", menu("sched-mode", ScheduledPage::mode_label(d.mode).to_owned(), modes).into_any_element()))
                    .child(div().w(gpui::px(160.)).child(field("Model", TextField::new(&e.model).small().into_any_element()))),
            )
            .when(d.mode == Mode::Bypass, |this| {
                this.child(
                    Checkbox::new("sched-bypass-confirm")
                        .checked(d.bypass_confirmed)
                        .label("Run this task with Full access: no permission checks, and nobody watching")
                        .on_click(cx.listener(|this, on: &bool, _, cx| {
                            let on = *on;
                            this.update_draft(|d| d.bypass_confirmed = on, cx)
                        })),
                )
            })
            // Beside the buttons, not above them: showing it never moves Create task from under the pointer.
            .child(
                h_flex()
                    .gap_2()
                    .justify_end()
                    .child(
                        div()
                            .id("sched-error")
                            .flex_1()
                            .min_w_0()
                            .text_size(ui::META)
                            .text_color(danger)
                            .children(e.error.clone()),
                    )
                    .child(Button::new("sched-cancel").ghost().small().label("Cancel").on_click(cx.listener(|this, _, _, cx| this.cancel(cx))))
                    .child(
                        Button::new("sched-save")
                            .primary()
                            .small()
                            .label(if d.id == 0 { "Create task" } else { "Save task" })
                            .on_click(cx.listener(|this, _, _, cx| this.save(cx))),
                    ),
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
                    .child(ui::section_title("Scheduled tasks", theme))
                    .child(div().px_1().text_size(ui::BODY).text_color(muted).child(
                        "A saved prompt that starts a new thread on a schedule, while SkyeLab is running. Each run uses \
                         the account its folder is bound to.",
                    )),
            )
            .child(
                Button::new("sched-new")
                    .primary()
                    .small()
                    .icon(Icon::new(SkyeIcon::Plus))
                    .label("New task")
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
                        .child(ui::EmptyState::new("No scheduled tasks").hint(
                            "Create one, or pick \"Schedule this prompt…\" from a composer.",
                        )),
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
