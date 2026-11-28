//! The Agents & accounts page, in Settings' card language: the agent CLIs found (version, or not found), then the
//! accounts. Added accounts are cards (provider mark in the account's color, name, folder in small type, sign-in);
//! found config folders are dashed suggestion cards with their own Add. Also the shared helpers of every page.

use super::Onboarding;
use crate::ui;
use crate::ui::ActiveTheme as _;
use crate::ui::Button;
use crate::ui::SkyeIcon;
use crate::ui::TextField;
use crate::ui::{Icon, h_flex, v_flex};
use gpui::{
    AnyElement, Context, Div, FontWeight, IntoElement, ParentElement as _, SharedString, div,
    prelude::*, px,
};

pub(super) fn home_relative(p: &std::path::Path, home: Option<&std::path::Path>) -> SharedString {
    match home.and_then(|h| p.strip_prefix(h).ok()) {
        Some(rest) => format!("~/{}", rest.display()).into(),
        None => p.display().to_string().into(),
    }
}

/// A muted line standing in for an empty list.
pub(super) fn note(text: &'static str, cx: &gpui::App) -> Div {
    div()
        .py_4()
        .px_3()
        .text_size(ui::BODY)
        .text_color(cx.theme().muted_foreground)
        .child(text)
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
                d.border_color(theme.border).bg(theme.background)
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
    /// The agent CLIs and gh found: one line each in one bordered list (logo, name, version, status dot), so all five
    /// fit a short window.
    fn agent_cards(&self, cx: &Context<Self>) -> Div {
        let theme = cx.theme();
        v_flex()
            .rounded(ui::RADIUS_LG)
            .border_1()
            .border_color(theme.border)
            .bg(theme.background)
            .children(self.agents.iter().enumerate().map(|(i, a)| {
                let (dot, text) = match &a.version {
                    None => (theme.muted_foreground, "checking…".to_owned()),
                    // Just the number: "2.1.0 (Claude Code)" would repeat the name beside it.
                    Some(Ok(v)) => (
                        theme.success,
                        skyelab_core::update::version_in(v).unwrap_or(v).to_owned(),
                    ),
                    Some(Err(_)) => (theme.warning, "not found in PATH".to_owned()),
                };
                // The providers' logos in their own colors; anything else a plain icon.
                let icon = match i {
                    0 => crate::ui::provider_logo(skyelab_core::Provider::Claude, px(16.), cx)
                        .into_any_element(),
                    1 => crate::ui::provider_logo(skyelab_core::Provider::Codex, px(16.), cx)
                        .into_any_element(),
                    2 => crate::ui::provider_logo(skyelab_core::Provider::Cursor, px(16.), cx)
                        .into_any_element(),
                    3 => crate::ui::provider_logo(skyelab_core::Provider::Antigravity, px(16.), cx)
                        .into_any_element(),
                    _ => Icon::new(SkyeIcon::Github)
                        .size_4()
                        .text_color(theme.foreground)
                        .into_any_element(),
                };
                let name = a.name;
                h_flex()
                    .id(("setup-agent", i))
                    .debug_selector(move || format!("setup-agent-{name}"))
                    .h(px(40.))
                    .px_3p5()
                    .gap_3()
                    .when(i > 0, |d| d.border_t_1().border_color(theme.border))
                    .child(
                        h_flex()
                            .size_4()
                            .flex_shrink_0()
                            .justify_center()
                            .child(icon),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_size(ui::BODY)
                            .font_weight(FontWeight::MEDIUM)
                            .child(a.name),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(ui::META)
                            .text_color(theme.muted_foreground)
                            .child(text),
                    )
                    .when_some(a.update.clone(), |d, v| {
                        d.child(
                            div()
                                .debug_selector(|| "agent-update".into())
                                .flex_shrink_0()
                                .text_size(ui::META)
                                .text_color(ui::ink(theme, theme.info))
                                .child(format!("{v} available")),
                        )
                    })
                    .child(ui::StatusDot::new(dot))
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
                                crate::workspace::dispatch(
                                    Box::new(super::super::LogInAccount { id: id.clone() }),
                                    window,
                                    cx,
                                )
                            }),
                    ),
                )
            })
    }

    fn account_cards(&self, cx: &mut Context<Self>) -> Div {
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
                .child(crate::settings::provider_mark(a.provider, color, cx))
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
                    .child(
                        TextField::new(input)
                            .small()
                            .accessibility_label("Account name"),
                    )
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
                    .child(crate::settings::provider_mark(f.found.provider, muted, cx))
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
                    // The why wraps: cut at "in your…" it would not say what to do.
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_0p5()
                            .child(div().text_size(ui::BODY).font_weight(FontWeight::MEDIUM).child("No config folders found"))
                            .child(
                                div()
                                    .text_size(ui::META)
                                    .text_color(cx.theme().muted_foreground)
                                    .child("No Claude Code, Codex, Cursor or Antigravity config folder in your home folder. Add one by hand."),
                            ),
                    )
                    .child(
                        Button::new("setup-settings")
                            .outline()
                            .small()
                            .label("Add in Settings")
                            .on_click(|_, window, cx| crate::workspace::dispatch(Box::new(super::super::OpenSettings), window, cx)),
                    )
                    .into_any_element(),
            );
        }
        let n = cards.len();
        v_flex().gap_2().children(cards).when(n > 0, |d| {
            d.child(
                h_flex().pt_1().child(
                    Button::new("setup-add-account")
                        .ghost()
                        .small()
                        .icon(Icon::new(SkyeIcon::Plus))
                        .label("Add account…")
                        .on_click(|_, window, cx| {
                            crate::workspace::dispatch(
                                Box::new(super::super::OpenSettingsSection {
                                    section: crate::settings::Section::Accounts,
                                }),
                                window,
                                cx,
                            )
                        }),
                ),
            )
        })
    }

    pub(super) fn agents_page(&self, cx: &mut Context<Self>) -> Div {
        let (agents, accounts) = (self.agent_cards(cx), self.account_cards(cx));
        let theme = cx.theme();
        let caption = |text: &'static str| {
            div()
                .pb_2()
                .text_size(ui::META)
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme.muted_foreground)
                .child(text)
        };
        v_flex()
            .child(caption("Agents"))
            .child(agents)
            .child(div().h_4())
            .child(caption("Accounts"))
            .child(accounts)
    }

    /// The one idea, as a tiny picture: a folder, an arrow, an account.
    pub(super) fn idea(&self, cx: &Context<Self>) -> Div {
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
