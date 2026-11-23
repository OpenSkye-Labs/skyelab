//! The setup page's look, after T3's Welcome page: a 768 px column, a title, then bands (a 96 px muted label on
//! the left, 40 px rows on the right) split by hairlines, and the finish buttons under them.

use super::{Onboarding, OnboardingEvent};
use crate::ui;
use gpui::{
    AnyElement, Context, Div, FontWeight, IntoElement, ParentElement as _, SharedString, Window,
    div, prelude::*, px,
};
use gpui_component::{
    ActiveTheme as _, Disableable as _, Icon, IconName, Selectable as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::Input,
    v_flex,
};
use gpui_kit_assets::IconName as Lucide;

const BAND_LABEL_W: f32 = 96.;
const ROW_MIN_H: f32 = 40.;

fn band(title: &'static str, body: impl IntoElement, cx: &gpui::App) -> Div {
    let theme = cx.theme();
    h_flex()
        .items_start()
        .gap_10()
        .py_4()
        .border_b_1()
        .border_color(theme.border.opacity(0.6))
        .child(
            div()
                .w(px(BAND_LABEL_W))
                .flex_shrink_0()
                .line_height(px(ROW_MIN_H))
                .text_size(ui::BODY)
                .text_color(theme.muted_foreground)
                .child(title),
        )
        .child(v_flex().flex_1().min_w_0().child(body))
}

/// One 40 px row: a 20 px icon column, a name over a muted detail line, and a trailing control.
fn row(
    icon: impl IntoElement,
    name: impl IntoElement,
    detail: Option<SharedString>,
    trailing: impl IntoElement,
    cx: &gpui::App,
) -> Div {
    let theme = cx.theme();
    h_flex()
        .min_h(px(ROW_MIN_H))
        .py_1()
        .gap_3()
        .child(
            div()
                .size_5()
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_center()
                .child(icon),
        )
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .child(div().truncate().text_size(ui::BODY).child(name))
                .children(detail.map(|d| {
                    div()
                        .truncate()
                        .text_size(ui::META)
                        .text_color(theme.muted_foreground)
                        .child(d)
                })),
        )
        .child(div().flex_shrink_0().child(trailing))
}

fn icon(icon: Icon, cx: &gpui::App) -> Icon {
    icon.size_4().text_color(cx.theme().muted_foreground)
}

fn status(text: &'static str, ok: bool, cx: &gpui::App) -> AnyElement {
    let theme = cx.theme();
    let color = if ok {
        ui::ink(theme, theme.success)
    } else {
        theme.muted_foreground
    };
    h_flex()
        .gap_1p5()
        .text_size(ui::META)
        .font_weight(FontWeight::MEDIUM)
        .text_color(color)
        .when(ok, |el| el.child(Icon::new(IconName::Check).size_3p5()))
        .child(text)
        .into_any_element()
}

fn home_relative(p: &std::path::Path, home: Option<&std::path::Path>) -> SharedString {
    match home.and_then(|h| p.strip_prefix(h).ok()) {
        Some(rest) => format!("~/{}", rest.display()).into(),
        None => p.display().to_string().into(),
    }
}

impl Onboarding {
    fn agents_band(&self, cx: &Context<Self>) -> Div {
        let rows = self.agents.iter().map(|a| {
            let (detail, trailing) = match &a.version {
                None => (None, status("Checking…", false, cx)),
                Some(Ok(v)) => (Some(v.clone().into()), status("Ready", true, cx)),
                Some(Err(_)) => (
                    Some("Not found in PATH".into()),
                    status("Not found", false, cx),
                ),
            };
            let name = div().font_weight(FontWeight::MEDIUM).child(a.name);
            row(
                icon(Icon::new(IconName::SquareTerminal), cx),
                name,
                detail,
                trailing,
                cx,
            )
        });
        band("Agents", v_flex().children(rows), cx)
    }

    fn accounts_band(&self, cx: &mut Context<Self>) -> Div {
        let home = self.home.clone();
        let theme_muted = cx.theme().muted_foreground;
        let mut body = v_flex();
        for a in &self.config.accounts {
            let detail = format!(
                "{} · {}",
                a.provider.label(),
                home_relative(&a.config_dir, home.as_deref())
            );
            body = body.child(row(
                div()
                    .size_2()
                    .rounded_full()
                    .bg(ui::account_color_in(cx, &a.id)),
                div().font_weight(FontWeight::MEDIUM).child(a.label.clone()),
                Some(detail.into()),
                status("Added", true, cx),
                cx,
            ));
        }
        for (ix, f) in self.found.iter().enumerate() {
            let dir = f.found.config_dir.clone();
            let name: AnyElement = match &f.label {
                Some(input) => div()
                    .max_w(px(280.))
                    .child(Input::new(input).small())
                    .into_any_element(),
                None => h_flex()
                    .gap_1()
                    .child(f.found.label.clone())
                    .child(
                        Button::new(("setup-rename", ix))
                            .ghost()
                            .xsmall()
                            .icon(Icon::new(Lucide::PenLine))
                            .tooltip("Rename before adding")
                            .on_click(cx.listener({
                                let dir = dir.clone();
                                move |this, _, window, cx| this.rename(&dir, window, cx)
                            })),
                    )
                    .into_any_element(),
            };
            let detail = format!(
                "{} · {}",
                f.found.provider.label(),
                home_relative(&f.found.config_dir, home.as_deref())
            );
            let add = Button::new(("setup-add", ix))
                .outline()
                .small()
                .icon(Icon::new(IconName::Plus))
                .label("Add")
                .on_click(cx.listener(move |this, _, _, cx| this.add_account(&dir, cx)));
            let mark = div()
                .size_2()
                .rounded_full()
                .border_1()
                .border_color(theme_muted);
            body = body.child(row(mark, name, Some(detail.into()), add, cx));
        }
        if self.found.is_empty() && self.config.accounts.is_empty() {
            body = body.child(row(
                icon(Icon::new(IconName::CircleUser), cx),
                div()
                    .text_color(cx.theme().muted_foreground)
                    .child("No Claude Code or Codex config folders found in your home folder."),
                None,
                Button::new("setup-settings")
                    .ghost()
                    .small()
                    .label("Open Settings")
                    .on_click(|_, window, cx| {
                        window.dispatch_action(Box::new(super::super::OpenSettings), cx)
                    }),
                cx,
            ));
        }
        band("Accounts", body, cx)
    }

    fn projects_band(&self, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme().clone();
        let home = self.home.clone();
        let muted = |t: SharedString| {
            div()
                .text_size(ui::META)
                .text_color(theme.muted_foreground)
                .child(t)
        };
        let mut body = v_flex();
        if !self.projects.is_empty() {
            let names: Vec<_> = self
                .projects
                .iter()
                .filter_map(|p| p.file_name())
                .map(|n| n.to_string_lossy())
                .collect();
            body = body.child(row(
                icon(Icon::new(IconName::Folder), cx),
                names.join(", "),
                None,
                status("Added", true, cx),
                cx,
            ));
        }
        let folder = self
            .folder
            .as_ref()
            .map(|d| home_relative(d, home.as_deref()));
        body =
            body.child(row(
                icon(Icon::new(Lucide::FolderGit2), cx),
                div()
                    .font_weight(FontWeight::MEDIUM)
                    .child(if self.projects.is_empty() {
                        "Add a project"
                    } else {
                        "Add another project"
                    }),
                Some(folder.unwrap_or_else(|| {
                    "Pick a folder. It runs on the account you bind it to.".into()
                })),
                Button::new("setup-choose")
                    .outline()
                    .small()
                    .label("Choose folder…")
                    .on_click(cx.listener(|this, _, _, cx| this.choose_folder(cx))),
                cx,
            ));
        let repos: Vec<_> = self
            .repos
            .iter()
            .filter(|r| !self.projects.contains(r))
            .cloned()
            .collect();
        if self.folder.is_none() && !repos.is_empty() {
            body = body.child(
                div()
                    .pl_8()
                    .pt_1()
                    .child(muted("Git repos in your home folder".into())),
            );
            for (ix, repo) in repos.into_iter().enumerate() {
                let name: SharedString = repo
                    .file_name()
                    .map_or_else(
                        || repo.display().to_string(),
                        |n| n.to_string_lossy().into_owned(),
                    )
                    .into();
                let path = home_relative(&repo, home.as_deref());
                body = body.child(
                    h_flex()
                        .id(("setup-repo", ix))
                        .ml_6()
                        .h(px(28.))
                        .px_2()
                        .gap_2()
                        .rounded(ui::RADIUS_MD)
                        .cursor_pointer()
                        .hover(|el| el.bg(theme.accent))
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.set_folder(repo.clone(), cx)),
                        )
                        .child(div().text_size(ui::BODY).child(name))
                        .child(div().truncate().child(muted(path))),
                );
            }
        }
        if self.folder.is_some() {
            let mut choices = h_flex().gap_1().flex_wrap();
            for a in &self.config.accounts {
                let id = a.id.clone();
                choices = choices.child(
                    Button::new(SharedString::from(format!("setup-account-{}", a.id)))
                        .outline()
                        .small()
                        .selected(self.account.as_deref() == Some(a.id.as_str()))
                        .child(ui::AccountBadge::of(a))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            (this.account, this.auto_account) = (Some(id.clone()), false);
                            cx.notify();
                        })),
                );
            }
            let hint = if self.config.accounts.is_empty() {
                "Add an account above first."
            } else {
                "Runs on"
            };
            body = body
                .child(
                    h_flex()
                        .pl_8()
                        .pt_2()
                        .gap_3()
                        .child(muted(hint.into()))
                        .child(choices),
                )
                .child(
                    h_flex().pl_8().pt_3().child(
                        Button::new("setup-bind")
                            .primary()
                            .small()
                            .label("Bind and add project")
                            .disabled(self.account.is_none())
                            .on_click(cx.listener(|this, _, _, cx| this.add_project(cx))),
                    ),
                );
        }
        band("Projects", body, cx)
    }
}

impl gpui::Render for Onboarding {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let complete = self.complete();
        let error = self.error.clone().or_else(|| {
            self.load_error
                .clone()
                .map(|e| format!("accounts.toml could not be read, so nothing can be saved: {e}"))
        });
        let column = v_flex()
            .w_full()
            .max_w(px(768.))
            .mx_auto()
            .px_6()
            .pt_12()
            .pb_8()
            .child(ui::grip_mark(px(28.), theme.foreground))
            .child(
                div().mt_3().text_size(px(24.)).font_weight(FontWeight::SEMIBOLD).child("Welcome to SkyeLab"),
            )
            .child(
                div().mt_1p5().text_size(ui::BODY).text_color(theme.muted_foreground)
                    .child("Every folder runs on the account it is bound to. Add your accounts, then bind a project folder to one."),
            )
            .child(
                v_flex()
                    .mt_10()
                    .border_t_1()
                    .border_color(theme.border.opacity(0.6))
                    .child(self.agents_band(cx))
                    .child(self.accounts_band(cx))
                    .child(self.projects_band(cx)),
            )
            .children(error.map(|e| div().pt_3().text_size(ui::META).text_color(ui::ink(&theme, theme.danger)).child(e)))
            .child(
                h_flex()
                    .mt_6()
                    .justify_between()
                    .child(
                        Button::new("setup-skip").ghost().small().label("Skip for now")
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(OnboardingEvent::Close))),
                    )
                    .child(
                        h_flex()
                            .gap_3()
                            .when(!complete, |el| {
                                el.child(div().text_size(ui::META).text_color(theme.muted_foreground).child("Add an account and a project to start."))
                            })
                            .child(
                                Button::new("setup-done").primary().small().label("Done").disabled(!complete)
                                    .on_click(cx.listener(|_, _, _, cx| cx.emit(OnboardingEvent::Close))),
                            ),
                    ),
            );
        div()
            .id("setup-scroll")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .child(column)
    }
}
