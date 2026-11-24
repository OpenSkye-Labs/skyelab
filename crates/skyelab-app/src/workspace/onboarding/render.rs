//! The setup page's look, in Settings' card language: the Bean mark and one line on the idea (folder → account), a
//! compact strip of the agents found, then two numbered steps. Accounts are cards (provider mark in the account's
//! color, name, folder in small type); found config folders are dashed suggestion cards with their own Add.

use super::{Onboarding, OnboardingEvent};
use crate::ui;
use crate::ui::Button;
use crate::ui::SkyeIcon;
use crate::ui::TextField;
use gpui::{
    AnyElement, Context, Div, FontWeight, IntoElement, ParentElement as _, SharedString, Window,
    div, prelude::*, px,
};
use gpui_component::{ActiveTheme as _, Icon, h_flex, v_flex};

pub(super) fn home_relative(p: &std::path::Path, home: Option<&std::path::Path>) -> SharedString {
    match home.and_then(|h| p.strip_prefix(h).ok()) {
        Some(rest) => format!("~/{}", rest.display()).into(),
        None => p.display().to_string().into(),
    }
}

/// A numbered step: its number in a small circle, the title, a muted line, then its body.
pub(super) fn step(
    n: u8,
    title: &'static str,
    about: &'static str,
    done: bool,
    body: impl IntoElement,
    cx: &gpui::App,
) -> Div {
    let theme = cx.theme();
    let badge = div()
        .size_6()
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .text_size(ui::META)
        .font_weight(FontWeight::SEMIBOLD)
        .map(|d| {
            if done {
                d.bg(theme.success.opacity(0.16))
                    .text_color(ui::ink(theme, theme.success))
                    .child(Icon::new(SkyeIcon::Check).size_3p5())
            } else {
                d.bg(theme.secondary)
                    .text_color(theme.foreground)
                    .child(n.to_string())
            }
        });
    v_flex()
        .gap_4()
        .child(
            h_flex().items_start().gap_3().child(badge).child(
                v_flex()
                    .gap_0p5()
                    .child(
                        div()
                            .text_size(ui::SECTION)
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(title),
                    )
                    .child(
                        div()
                            .text_size(ui::BODY)
                            .text_color(theme.muted_foreground)
                            .child(about),
                    ),
            ),
        )
        .child(div().pl_9().child(body))
}

/// A card's frame: solid for what is set up, dashed for a suggestion.
pub(super) fn card(
    id: impl Into<gpui::ElementId>,
    dashed: bool,
    cx: &gpui::App,
) -> gpui::Stateful<Div> {
    let theme = cx.theme();
    h_flex()
        .id(id)
        .gap_3()
        .p_3p5()
        .rounded(ui::RADIUS_LG)
        .border_1()
        .map(|d| {
            if dashed {
                d.border_dashed().border_color(theme.input)
            } else {
                d.border_color(theme.border).bg(theme.muted)
            }
        })
}

pub(super) fn name_and_folder(name: impl IntoElement, folder: SharedString, cx: &gpui::App) -> Div {
    v_flex()
        .flex_1()
        .min_w_0()
        .gap_0p5()
        .child(
            div()
                .truncate()
                .text_size(ui::BODY)
                .font_weight(FontWeight::MEDIUM)
                .child(name),
        )
        .child(
            div()
                .truncate()
                .text_size(ui::META)
                .text_color(cx.theme().muted_foreground)
                .child(folder),
        )
}

impl Onboarding {
    /// The agents found, as one compact line of chips.
    fn agents_strip(&self, cx: &Context<Self>) -> Div {
        let theme = cx.theme();
        h_flex()
            .flex_wrap()
            .gap_2()
            .children(self.agents.iter().map(|a| {
                let (dot, text) = match &a.version {
                    None => (theme.muted_foreground, "checking…".to_owned()),
                    Some(Ok(v)) => (theme.success, v.clone()),
                    Some(Err(_)) => (theme.warning, "not found in PATH".to_owned()),
                };
                h_flex()
                    .h_7()
                    .px_2p5()
                    .gap_2()
                    .rounded_full()
                    .border_1()
                    .border_color(theme.border)
                    .text_size(ui::META)
                    .child(ui::StatusDot::new(dot))
                    .child(div().font_weight(FontWeight::MEDIUM).child(a.name))
                    .child(div().text_color(theme.muted_foreground).child(text))
                    .when_some(a.update.clone(), |d, v| {
                        d.child(
                            div()
                                .debug_selector(|| "agent-update".into())
                                .text_color(ui::ink(theme, theme.info))
                                .child(format!("{v} available")),
                        )
                    })
            }))
    }

    /// An added account's sign-in: a pill, and Log in while it is signed out.
    fn login_badge(&self, a: &skyelab_core::Account, cx: &mut Context<Self>) -> Div {
        use super::super::login_status::Status;
        let status = self.login.read(cx).get(a).cloned();
        let theme = cx.theme();
        let (text, _, why) = Status::label(status.as_ref(), theme);
        let tint = match &status {
            Some(Status::SignedIn(_)) => Some(theme.success),
            Some(Status::SignedOut) => Some(theme.warning),
            _ => None,
        };
        let pill = ui::Pill::new(text);
        let pill = match tint {
            Some(c) => pill.color(c),
            None => pill,
        };
        let selector = format!("setup-login-{}", a.id);
        h_flex()
            .gap_2()
            .flex_shrink_0()
            .child(
                div()
                    .id(SharedString::from(selector.clone()))
                    .debug_selector(move || selector)
                    .child(pill)
                    .when_some(why, |d, why| d.tooltip(ui::Tooltip::text(why))),
            )
            .when(Status::signed_out(status.as_ref()), |d| {
                let id = a.id.clone();
                let selector = format!("setup-log-in-{id}");
                d.child(
                    div().debug_selector(move || selector).child(
                        Button::new(SharedString::from(format!("setup-log-in-{}", a.id)))
                            .label("Log in")
                            .small()
                            .primary()
                            .on_click(move |_, window, cx| {
                                window.dispatch_action(
                                    Box::new(super::super::LogInAccount { id: id.clone() }),
                                    cx,
                                )
                            }),
                    ),
                )
            })
    }

    fn accounts_step(&self, cx: &mut Context<Self>) -> Div {
        let home = self.home.clone();
        let mut cards: Vec<AnyElement> = Vec::new();
        for a in &self.config.accounts {
            let color = ui::account_color_in(cx, &a.id);
            let folder = format!(
                "{} · {}",
                a.provider.label(),
                home_relative(&a.config_dir, home.as_deref())
            );
            cards.push(
                card(
                    SharedString::from(format!("setup-acct-{}", a.id)),
                    false,
                    cx,
                )
                .child(crate::settings::provider_mark(a.provider, color))
                .child(name_and_folder(a.label.clone(), folder.into(), cx))
                .child(self.login_badge(a, cx))
                .into_any_element(),
            );
        }
        for (ix, f) in self.found.iter().enumerate() {
            let dir = f.found.config_dir.clone();
            let name: AnyElement = match &f.label {
                Some(input) => div()
                    .max_w(px(240.))
                    .child(TextField::new(input).small())
                    .into_any_element(),
                None => h_flex()
                    .gap_1()
                    .child(f.found.label.clone())
                    .child(
                        Button::new(("setup-rename", ix))
                            .ghost()
                            .xsmall()
                            .icon(Icon::new(SkyeIcon::PenLine))
                            .tooltip("Rename before adding")
                            .on_click(cx.listener({
                                let dir = dir.clone();
                                move |this, _, window, cx| this.rename(&dir, window, cx)
                            })),
                    )
                    .into_any_element(),
            };
            let folder = format!(
                "{} · {}",
                f.found.provider.label(),
                home_relative(&dir, home.as_deref())
            );
            let muted = cx.theme().muted_foreground;
            cards.push(
                card(("setup-found", ix), true, cx)
                    .child(crate::settings::provider_mark(f.found.provider, muted))
                    .child(name_and_folder(name, folder.into(), cx))
                    .child(
                        Button::new(("setup-add", ix))
                            .primary()
                            .small()
                            .icon(Icon::new(SkyeIcon::Plus))
                            .label("Add")
                            .on_click(
                                cx.listener(move |this, _, _, cx| this.add_account(&dir, cx)),
                            ),
                    )
                    .into_any_element(),
            );
        }
        if cards.is_empty() {
            cards.push(
                card("setup-none", true, cx)
                    .child(Icon::new(SkyeIcon::CircleUser).size_4().text_color(cx.theme().muted_foreground))
                    .child(name_and_folder(
                        "No config folders found",
                        "No Claude Code or Codex config folder in your home folder. Add one by hand.".into(),
                        cx,
                    ))
                    .child(
                        Button::new("setup-settings")
                            .outline()
                            .small()
                            .label("Add in Settings")
                            .on_click(|_, window, cx| window.dispatch_action(Box::new(super::super::OpenSettings), cx)),
                    )
                    .into_any_element(),
            );
        }
        step(
            1,
            "Accounts",
            "Each Claude or Codex login keeps its own config folder. Add the ones you use.",
            !self.config.accounts.is_empty(),
            v_flex().gap_2().children(cards),
            cx,
        )
    }

    /// The one idea, as a tiny picture: a folder, an arrow, an account.
    fn idea(&self, cx: &Context<Self>) -> Div {
        let theme = cx.theme();
        let chip = |icon: Icon, text: &'static str| {
            h_flex()
                .h_7()
                .px_2p5()
                .gap_1p5()
                .rounded(ui::RADIUS)
                .bg(theme.secondary)
                .text_size(ui::META)
                .font_weight(FontWeight::MEDIUM)
                .child(icon.size_3p5().text_color(theme.muted_foreground))
                .child(text)
        };
        h_flex()
            .gap_2()
            .child(chip(Icon::new(SkyeIcon::Folder), "~/work/api"))
            .child(
                Icon::new(SkyeIcon::ArrowRight)
                    .size_3p5()
                    .text_color(theme.muted_foreground),
            )
            .child(
                h_flex()
                    .h_7()
                    .px_2p5()
                    .gap_1p5()
                    .rounded(ui::RADIUS)
                    .bg(theme.primary.opacity(0.12))
                    .text_size(ui::META)
                    .font_weight(FontWeight::MEDIUM)
                    .child(ui::StatusDot::new(theme.primary))
                    .child("Work account"),
            )
    }
}

impl gpui::Render for Onboarding {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (agents, idea) = (self.agents_strip(cx), self.idea(cx));
        let steps = [self.accounts_step(cx), self.projects_step(cx)];
        let theme = cx.theme();
        let complete = self.complete();
        let error = self.error.clone().or_else(|| {
            self.load_error
                .clone()
                .map(|e| format!("accounts.toml could not be read, so nothing can be saved: {e}"))
        });
        let column = v_flex()
            .w_full()
            .max_w(px(720.))
            .mx_auto()
            .px_6()
            .pt_12()
            .pb_10()
            .gap_8()
            .child(
                v_flex()
                    .gap_4()
                    .child(
                        div()
                            .size_12()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(ui::RADIUS_XL)
                            .bg(theme.secondary)
                            .child(ui::grip_mark(px(30.), theme.foreground)),
                    )
                    .child(
                        v_flex()
                            .gap_1p5()
                            .child(div().text_size(px(26.)).font_weight(FontWeight::SEMIBOLD).child("Welcome to SkyeLab"))
                            .child(
                                div().text_size(ui::BODY).text_color(theme.muted_foreground).child(
                                    "Every folder runs on the account it is bound to, and nothing ever falls back to another.",
                                ),
                            ),
                    )
                    .child(idea)
                    .child(agents),
            )
            .child(div().h_px().bg(theme.border))
            .children(steps)
            .children(error.map(|e| div().text_size(ui::META).text_color(ui::ink(theme, theme.danger)).child(e)))
            .child(
                h_flex()
                    .justify_between()
                    .child(
                        Button::new("setup-skip")
                            .ghost()
                            .small()
                            .label("Skip for now")
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(OnboardingEvent::Close))),
                    )
                    .child(
                        h_flex()
                            .gap_3()
                            .when(!complete, |el| {
                                el.child(
                                    div()
                                        .text_size(ui::META)
                                        .text_color(theme.muted_foreground)
                                        .child("Add an account and a project to start."),
                                )
                            })
                            .child(
                                Button::new("setup-done")
                                    .primary()
                                    .label("Done")
                                    .disabled(!complete)
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
