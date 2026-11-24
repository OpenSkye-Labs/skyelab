//! The setup page's second step: the projects added, a folder to choose (or a git repo found in home), and the
//! account it runs on.

use super::Onboarding;
use super::render::{card, home_relative, name_and_folder, step};
use crate::ui;
use crate::ui::Button;
use crate::ui::SkyeIcon;
use gpui::{Context, Div, FontWeight, ParentElement as _, SharedString, div, prelude::*, px};
use gpui_component::{ActiveTheme as _, Icon, h_flex, v_flex};

impl Onboarding {
    pub(super) fn projects_step(&self, cx: &mut Context<Self>) -> Div {
        let home = self.home.clone();
        let mut body = v_flex().gap_2();
        if !self.projects.is_empty() {
            let names: Vec<_> = self
                .projects
                .iter()
                .filter_map(|p| p.file_name())
                .map(|n| n.to_string_lossy())
                .collect();
            body = body.child(
                card("setup-projects", false, cx)
                    .child(
                        Icon::new(SkyeIcon::Folder)
                            .size_4()
                            .text_color(cx.theme().muted_foreground),
                    )
                    .child(name_and_folder(names.join(", "), "Added".into(), cx)),
            );
        }
        let folder = self
            .folder
            .as_ref()
            .map(|d| home_relative(d, home.as_deref()));
        let chosen = folder.is_some();
        body = body.child(
            card("setup-pick", !chosen, cx)
                .child(
                    Icon::new(SkyeIcon::FolderGit2)
                        .size_4()
                        .text_color(cx.theme().muted_foreground),
                )
                .child(name_and_folder(
                    if self.projects.is_empty() {
                        "Add a project"
                    } else {
                        "Add another project"
                    },
                    folder.unwrap_or_else(|| {
                        "Pick a folder. It runs on the account you bind it to.".into()
                    }),
                    cx,
                ))
                .child(
                    Button::new("setup-choose")
                        .outline()
                        .small()
                        .icon(SkyeIcon::FolderOpen)
                        .label("Choose folder…")
                        .on_click(cx.listener(|this, _, _, cx| this.choose_folder(cx))),
                ),
        );
        let repos: Vec<_> = self
            .repos
            .iter()
            .filter(|r| !self.projects.contains(r))
            .cloned()
            .collect();
        if !chosen && !repos.is_empty() {
            let theme = cx.theme();
            let mut list = v_flex()
                .rounded(ui::RADIUS_LG)
                .border_1()
                .border_color(theme.border)
                .overflow_hidden()
                .child(
                    div()
                        .px_3p5()
                        .py_2()
                        .text_size(ui::META)
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.muted_foreground)
                        .bg(theme.muted)
                        .child("Git repos in your home folder"),
                );
            for (ix, repo) in repos.into_iter().enumerate() {
                let theme = cx.theme();
                let name: SharedString = repo
                    .file_name()
                    .map_or_else(
                        || repo.display().to_string(),
                        |n| n.to_string_lossy().into_owned(),
                    )
                    .into();
                let path = home_relative(&repo, home.as_deref());
                let hover = theme.accent;
                list = list.child(
                    h_flex()
                        .id(("setup-repo", ix))
                        .h(px(36.))
                        .px_3p5()
                        .gap_3()
                        .border_t_1()
                        .border_color(theme.border)
                        .cursor_pointer()
                        .hover(move |el| el.bg(hover))
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.set_folder(repo.clone(), cx)),
                        )
                        .child(
                            Icon::new(SkyeIcon::GitBranch)
                                .size_3p5()
                                .text_color(theme.muted_foreground),
                        )
                        .child(
                            div()
                                .text_size(ui::BODY)
                                .font_weight(FontWeight::MEDIUM)
                                .child(name),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_size(ui::META)
                                .text_color(theme.muted_foreground)
                                .child(path),
                        )
                        .child(
                            div()
                                .text_size(ui::META)
                                .text_color(theme.muted_foreground)
                                .child("Use"),
                        ),
                );
            }
            body = body.child(list);
        }
        if chosen {
            let theme = cx.theme();
            let hint = if self.config.accounts.is_empty() {
                "Add an account first."
            } else {
                "Runs on"
            };
            let muted = theme.muted_foreground;
            let mut choices = h_flex().gap_1p5().flex_wrap();
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
            body = body.child(
                h_flex()
                    .pt_1()
                    .gap_3()
                    .child(div().text_size(ui::META).text_color(muted).child(hint))
                    .child(choices)
                    .child(div().flex_1())
                    .child(
                        Button::new("setup-bind")
                            .primary()
                            .small()
                            .label("Bind and add project")
                            .disabled(self.account.is_none())
                            .on_click(cx.listener(|this, _, _, cx| this.add_project(cx))),
                    ),
            );
        }
        step(
            2,
            "First project",
            "Bind a folder to an account. Agents in it, and every folder under it, run on that account.",
            !self.projects.is_empty(),
            body,
            cx,
        )
    }
}
