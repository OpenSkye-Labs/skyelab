//! The task (bot) editor card: name, project, prompt, what starts it (a schedule, GitHub triggers), where it runs and
//! with which permissions. With a GitHub trigger on, only Plan and Ask are offered.

use super::form::Kind;
use super::page::{Editor, Pick, ScheduledPage};
use crate::ui;
use crate::ui::{Button, Checkbox, TextField};
use gpui::{
    AnyElement, Context, IntoElement, ParentElement as _, SharedString, Styled as _, div,
    prelude::*,
};
use gpui_component::{
    ActiveTheme as _, h_flex,
    input::Textarea,
    menu::{DropdownMenu as _, PopupMenuItem},
    v_flex,
};
use skyelab_agent::Mode;
use skyelab_core::bots::{MAX_RUNS_PER_HOUR, Trigger};

impl ScheduledPage {
    pub(super) fn render_editor(&self, e: &Editor, cx: &mut Context<Self>) -> AnyElement {
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
        let gh = !d.triggers.is_empty();
        let modes = Mode::ALL
            .iter()
            .filter(|&&m| !gh || matches!(m, Mode::Plan | Mode::Default))
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
                    .when(d.kind != Kind::Never, |row| row.child(div().w(gpui::px(160.)).child(field(if d.kind == Kind::Cron { "Expression" } else { "Value" }, TextField::new(&e.value).small().into_any_element()))))
                    .child(field("Runs in", menu("sched-where", where_label.to_owned(), places).into_any_element()))
                    .child(field("Permissions", menu("sched-mode", ScheduledPage::mode_label(d.mode).to_owned(), modes).into_any_element()))
                    .child(div().w(gpui::px(160.)).child(field("Model", TextField::new(&e.model).small().into_any_element()))),
            )
            .child(self.render_triggers(e, cx))
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

    /// GitHub triggers: the three events, whose items fire, and the hourly cap. Off with the reason when the project
    /// can't take them (no GitHub remote, its account's gh not signed in).
    fn render_triggers(&self, e: &Editor, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let d = &e.draft;
        let blocked = match &e.github {
            Some(Err(why)) => Some(why.clone()),
            _ => None,
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
        let boxes = h_flex().gap_4().flex_wrap().children(Trigger::ALL.map(|t| {
            let on = d.triggers.contains(&t);
            Checkbox::new(SharedString::from(format!("sched-trigger-{}", t.as_str())))
                .checked(on)
                // A trigger already on can always be turned off.
                .disabled(blocked.is_some() && !on)
                .label(t.label())
                .on_click(cx.listener(move |this, _: &bool, window, cx| {
                    this.pick(Pick::Trigger(t), window, cx)
                }))
        }));
        let who = [(false, "Anyone's"), (true, "Only mine")]
            .map(|(m, l)| (l.to_owned(), d.mine == m, Pick::Mine(m)))
            .to_vec();
        let caps = [1, 3, 6, 12, MAX_RUNS_PER_HOUR]
            .map(|n| (format!("{n} an hour"), d.per_hour == n, Pick::PerHour(n)))
            .to_vec();
        let note = match (&blocked, d.triggers.is_empty()) {
            (Some(why), _) => Some(why.to_string()),
            (None, false) => Some(
                "PR and issue text is anyone's: these runs get it as quoted context, run in Plan or Ask, and get none of \
                 SkyeLab's own tools."
                    .to_owned(),
            ),
            (None, true) => None,
        };
        v_flex()
            .id("sched-triggers")
            .gap_2()
            .child(
                div()
                    .text_size(ui::META)
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(muted)
                    .child("GitHub triggers"),
            )
            .child(boxes)
            .when(!d.triggers.is_empty(), |this| {
                this.child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .text_size(ui::META)
                        .text_color(muted)
                        .child(menu(
                            "sched-author",
                            (if d.mine { "Only mine" } else { "Anyone's" }).to_owned(),
                            who,
                        ))
                        .child(menu(
                            "sched-per-hour",
                            format!("At most {} an hour", d.per_hour),
                            caps,
                        )),
                )
            })
            .children(note.map(|n| {
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        div()
                            .id("sched-triggers-note")
                            .text_size(ui::META)
                            .text_color(muted)
                            .child(n),
                    )
                    .when(
                        blocked.as_ref().is_some_and(|w| w.contains("signed in")),
                        |row| {
                            row.child(
                                Button::new("sched-gh-sign-in")
                                    .ghost()
                                    .xsmall()
                                    .label("Sign in")
                                    .on_click(|_, window, cx| {
                                        let section = crate::settings::Section::Accounts;
                                        crate::workspace::dispatch(
                                            Box::new(crate::workspace::OpenSettingsSection {
                                                section,
                                            }),
                                            window,
                                            cx,
                                        )
                                    }),
                            )
                        },
                    )
            }))
            .into_any_element()
    }
}
