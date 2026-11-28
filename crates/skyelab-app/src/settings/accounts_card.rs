//! Settings → Accounts: one account's card (provider mark, name, its sign-in, config folder and GitHub login).

use super::widgets::path_line;
use super::*;
use crate::ui;
use crate::ui::SkyeIcon;
use crate::ui::{Icon, h_flex, v_flex};

impl Settings {
    pub(super) fn render_card(&self, a: &Account, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = cx.theme();
        let color = ui::account_color_in(cx, &a.id);
        let (edit, remove) = (
            EditAccount { id: a.id.clone() },
            RemoveAccount { id: a.id.clone() },
        );
        let (login_id, login_cmd) = (
            a.id.clone(),
            match a.provider {
                Provider::Claude => "claude auth login with this account's CLAUDE_CONFIG_DIR",
                Provider::Codex => "codex login with this account's CODEX_HOME",
                Provider::Cursor => "cursor-agent login with this account's CURSOR_CONFIG_DIR",
                Provider::Antigravity => "agy acp --login with this account's GEMINI_HOME",
            },
        );
        let login_key = format!("{}-login-{}", a.provider.label().to_lowercase(), a.id);
        let login = self.login.read(cx).get(a);
        let signed_out = crate::workspace::login_status::Status::signed_out(login);
        let signed_in = matches!(
            login,
            Some(crate::workspace::login_status::Status::SignedIn(_))
        );
        let (status, status_color, why) =
            crate::workspace::login_status::Status::label(login, theme);
        let status_id = format!("login-status-{}", a.id);
        let gh = self.render_card_gh(a, cx);
        let selector = format!("provider-{}", a.id);
        v_flex()
            .id(SharedString::from(format!("account-{}", a.id)))
            .gap_2()
            .p_4()
            .rounded(ui::RADIUS_LG)
            .border_1()
            .border_color(theme.border)
            .bg(theme.muted)
            .child(
                h_flex()
                    .gap_3()
                    .child(provider_mark(a.provider, color, cx))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .id(SharedString::from(format!("name-{}", a.id)))
                                    .role(gpui::accesskit::Role::Button)
                                    .aria_label(format!("Edit account {}", a.label))
                                    .truncate()
                                    .text_size(ui::BODY)
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .cursor_pointer()
                                    .child(a.label.clone())
                                    .on_click(cx.listener({
                                        let edit = edit.clone();
                                        move |this, _, window, cx| {
                                            this.edit_account(&edit, window, cx)
                                        }
                                    })),
                            )
                            .child(
                                h_flex()
                                    .debug_selector(move || selector)
                                    // A narrow dialog moves the sign-in status to a second line instead of
                                    // running it under the buttons; truncation is the last resort.
                                    .min_w_0()
                                    .flex_wrap()
                                    .gap_x_1p5()
                                    .text_size(ui::META)
                                    .text_color(theme.muted_foreground)
                                    .child(ui::StatusDot::new(color))
                                    .child(a.provider.label())
                                    .child("·")
                                    .child(ui::mono(a.id.clone(), theme).text_size(ui::META))
                                    .child("·")
                                    .child(
                                        div()
                                            .id(SharedString::from(status_id.clone()))
                                            .debug_selector(move || status_id)
                                            .min_w_0()
                                            .truncate()
                                            .text_color(status_color)
                                            .child(status)
                                            .when_some(why, |d, why| {
                                                d.tooltip(ui::Tooltip::text(why))
                                            }),
                                    ),
                            ),
                    )
                    .child({
                        let selector = login_key.clone();
                        div().debug_selector(move || selector).child(
                            Button::new(SharedString::from(login_key))
                                // Signed in already: the button signs in again (another login), as it says.
                                .label(if signed_in { "Log in again" } else { "Log in" })
                                .tooltip(format!("Opens a terminal running {login_cmd}"))
                                .small()
                                // Signed in: a quiet ghost, since it isn't the row's next step.
                                .map(|b| match (signed_out, signed_in) {
                                    (true, _) => b.primary(),
                                    (_, true) => b.ghost(),
                                    _ => b.outline(),
                                })
                                .on_click(
                                    cx.listener(move |this, _, _, cx| this.log_in(&login_id, cx)),
                                ),
                        )
                    })
                    .child(
                        Button::new(SharedString::from(format!("edit-{}", a.id)))
                            .icon(SkyeIcon::PenLine)
                            .tooltip("Edit")
                            .small()
                            .ghost()
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.edit_account(&edit, window, cx)
                            })),
                    )
                    .child(
                        Button::new(SharedString::from(format!("remove-account-{}", a.id)))
                            .icon(SkyeIcon::Trash)
                            .tooltip("Remove account")
                            .small()
                            .ghost()
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.remove_account(&remove, window, cx)
                            })),
                    ),
            )
            .child(
                v_flex()
                    .pl(gpui::px(44.))
                    .gap_1()
                    .child(path_line(format!("dir-{}", a.id), &a.config_dir, cx))
                    .children(gh),
            )
            .into_any_element()
    }

    /// The account's GitHub login (Source control's check), when it has a GitHub folder.
    fn render_card_gh(&self, a: &Account, cx: &App) -> Option<gpui::Div> {
        a.gh_config_dir.as_ref()?;
        let theme = cx.theme();
        let (text, color) = match (self.gh_logins.get(&a.id), self.gh_errors.get(&a.id)) {
            (Some(Some(login)), _) => (
                format!("GitHub: signed in as {login}"),
                ui::ink(theme, theme.success),
            ),
            (Some(None), _) => (
                "GitHub: not signed in".to_owned(),
                ui::ink(theme, theme.warning),
            ),
            (None, Some(_)) => ("GitHub: couldn't check".to_owned(), theme.muted_foreground),
            (None, None) => ("GitHub: checking…".to_owned(), theme.muted_foreground),
        };
        Some(
            h_flex()
                .gap_1p5()
                .text_size(ui::META)
                .text_color(color)
                .child(Icon::new(SkyeIcon::Github).size_3())
                .child(text),
        )
    }
}
