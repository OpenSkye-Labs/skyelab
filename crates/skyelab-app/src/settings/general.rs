//! Settings → General (the editor command, the update check) and Worktrees, the error banner every section shows,
//! and Restore defaults.

use super::widgets::{DESC, section, setting};
use super::*;
use crate::ui;
use crate::ui::SkyeIcon;
use crate::ui::Switch;
use crate::ui::TextField;
use crate::workspace::thread_extras::prefs;
use gpui_component::{h_flex, v_flex};
use skyelab_core::update::Channel;
use skyelab_core::{AppSettings, DiffSettings, ThreadSettings};

/// The "Open in editor" command saved in settings.toml (empty: the default lookup).
pub(super) fn editor(cx: &App) -> String {
    prefs::path(cx)
        .and_then(|p| AppSettings::load(&p).ok())
        .and_then(|s| s.editor)
        .unwrap_or_default()
}

impl Settings {
    /// Shows `save`'s failure in the section's error banner, as "Saving <what> failed: …".
    pub(super) fn report<T: 'static>(
        &self,
        save: gpui::Task<anyhow::Result<T>>,
        what: &'static str,
        cx: &mut Context<Self>,
    ) {
        cx.spawn(async move |this, cx| {
            if let Err(e) = save.await {
                let _ = this.update(cx, |this, cx| {
                    this.error = Some(format!("Saving {what} failed: {e:#}"));
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// The editor field was left or confirmed: save it (empty is the default lookup).
    pub(super) fn save_editor(&mut self, cx: &mut Context<Self>) {
        let typed = self.editor_input.read(cx).value().trim().to_owned();
        let Some(path) = prefs::path(cx) else { return };
        let editor = (!typed.is_empty()).then_some(typed);
        let save = cx.background_spawn(async move {
            AppSettings::update(&path, |s| {
                let changed = s.editor != editor;
                s.editor = editor;
                changed
            })
        });
        self.report(save, "the editor command", cx);
    }

    pub(super) fn render_general(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let enabled = cx
            .try_global::<crate::updates::Updates>()
            .is_none_or(|u| u.enabled);
        let channel = cx
            .try_global::<crate::updates::Updates>()
            .map_or(Channel::Stable, |u| u.channel);
        v_flex()
            .gap_8()
            .child(section(
                "Editor",
                "Where \"Open in editor\" sends a folder.",
                [setting(
                    true,
                    "Open in editor",
                    "The command \"Open in editor\" runs, with the folder added at the end. Empty tries $VISUAL, \
                     $EDITOR, then code or zed.",
                    div().w(gpui::px(220.)).child(TextField::new(&self.editor_input).small()),
                    cx,
                )],
                cx,
            ))
            .child(section(
                "Terminal",
                "The shells in the bottom panel.",
                [setting(
                    true,
                    "Terminal close confirmation",
                    "Ask before closing a terminal pane or tab whose shell is still running a command. Closing \
                     stops the process.",
                    Switch::new("terminal-confirm-close")
                        .checked(prefs::get_terminal(cx).confirm_close)
                        .accessibility_label("Terminal close confirmation")
                        .on_click(cx.listener(|_, on: &bool, _, cx| {
                            let mut t = prefs::get_terminal(cx);
                            t.confirm_close = *on;
                            let save = prefs::save_terminal(t, cx);
                            cx.spawn(async move |this, cx| {
                                if let Err(e) = save.await {
                                    let _ = this.update(cx, |this, cx| {
                                        this.error = Some(format!("Saving the terminal setting failed: {e:#}"));
                                        cx.notify();
                                    });
                                }
                            })
                            .detach();
                            cx.notify();
                        })),
                    cx,
                )],
                cx,
            ))
            .child(section(
                "Quitting",
                "What happens on Quit while work is in flight.",
                [
                    setting(
                    true,
                    "Quit confirmation",
                    "Ask before quitting while a thread is working or waiting for you, or a terminal is running a \
                     command. Quitting stops them.",
                    Switch::new("quit-confirm")
                        .checked(crate::workspace::quit_guard::asks(cx))
                        .accessibility_label("Quit confirmation")
                        .on_click(cx.listener(|this, on: &bool, _, cx| {
                            if let Err(e) = crate::workspace::quit_guard::set_asks(*on, cx) {
                                this.error = Some(format!("Saving the quit setting failed: {e:#}"));
                            }
                            cx.notify();
                        })),
                    cx,
                    ),
                    setting(
                        false,
                        "Always keep running in background",
                        "Quit while a thread is working or waiting for you closes the windows instead of asking; \
                         the threads go on. SkyeLab quits by itself once nothing has worked or waited for 30 \
                         seconds. Open SkyeLab again to get its windows back.",
                        Switch::new("quit-keep")
                            .checked(crate::workspace::quit_guard::keeps(cx))
                            .accessibility_label("Always keep running in background")
                            .on_click(cx.listener(|this, on: &bool, _, cx| {
                                if let Err(e) = crate::workspace::quit_guard::set_keeps(*on, cx) {
                                    this.error =
                                        Some(format!("Saving the quit setting failed: {e:#}"));
                                }
                                cx.notify();
                            })),
                        cx,
                    ),
                    setting(
                        false,
                        "Resume interrupted turns after restart",
                        "Automatically continue local threads that were running when SkyeLab restarted or crashed, \
                         unless they were waiting for an approval or already auto-resumed once.",
                        Switch::new("resume-after-restart")
                            .checked(crate::workspace::resume_after_restart::enabled(cx))
                            .accessibility_label("Resume interrupted turns after restart")
                            .on_click(cx.listener(|this, on: &bool, _, cx| {
                                if let Err(e) =
                                    crate::workspace::resume_after_restart::set_enabled(*on, cx)
                                {
                                    this.error =
                                        Some(format!("Saving the quit setting failed: {e:#}"));
                                }
                                cx.notify();
                            })),
                        cx,
                    ),
                ],
                cx,
            ))
            .child(section(
                "Advanced",
                "Tools for working on SkyeLab itself.",
                [setting(
                    true,
                    "Developer mode",
                    "Debug mode at every launch: the Debug chip in the status bar, the overlay with frame times and \
                     live counts, and a debug log of network calls and child processes (no prompts or secrets).",
                    Switch::new("developer-mode")
                        .checked(crate::debug_mode::developer_mode(cx))
                        .accessibility_label("Developer mode")
                        .on_click(cx.listener(|this, on: &bool, _, cx| {
                            let save = crate::debug_mode::set_developer_mode(*on, cx);
                            this.report(save, "Developer mode", cx);
                            cx.notify();
                        })),
                    cx,
                )],
                cx,
            ))
            .child(section(
                "Updates",
                "Nothing installs until you click Download and install, then Restart to update.",
                [setting(
                    true,
                    "Check for updates",
                    "On start, at most once a day, SkyeLab asks GitHub for its latest release and shows a notice \
                     when there is a newer one.",
                    Switch::new("about-updates")
                        .checked(enabled)
                        .accessibility_label("Check for updates")
                        .on_click(cx.listener(|this, on: &bool, _, cx| {
                            let save = crate::updates::set_enabled(*on, cx);
                            this.report(save, "the update check", cx);
                            cx.notify();
                        })),
                    cx,
                ),
                setting(
                    true,
                    "Update channel",
                    "Stable offers releases only. Nightly also offers the nightly builds of the next version, \
                     and a newer stable release when one is out.",
                    ui::SegmentedControl::new("update-channel")
                        .with_size(ui::Size::Sm)
                        .items(["Stable", "Nightly"])
                        .selected(Some(usize::from(channel == Channel::Nightly)))
                        .on_click(cx.listener(|this, ix: &usize, _, cx| {
                            let channel = if *ix == 1 { Channel::Nightly } else { Channel::Stable };
                            let save = crate::updates::set_channel(channel, cx);
                            this.report(save, "the update channel", cx);
                            cx.notify();
                        })),
                    cx,
                )],
                cx,
            ))
            .into_any_element()
    }

    pub(super) fn render_worktrees(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        section(
            "Worktrees",
            "A worktree gives a thread's agent its own branch and folder, so parallel threads don't collide.",
            [setting(
                true,
                "New threads start in a worktree",
                "Turns on \"New worktree\" under a new thread's composer.",
                self.worktree_control(cx),
                cx,
            )],
            cx,
        )
        .into_any_element()
    }

    /// The header's Restore defaults: this section's preferences back to how a fresh install has them.
    pub(crate) fn restore_defaults(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let fresh = ThreadSettings::default();
        if let Some(dir) = self.scope.clone()
            && matches!(self.section, Section::Chat | Section::Worktrees)
        {
            return self.restore_project(dir, window, cx);
        }
        match self.section {
            Section::Appearance => {
                let save = crate::ui::themes::set(Default::default(), cx);
                self.report(save, "Appearance", cx);
            }
            Section::Chat => {
                let save = prefs::clear_server_dictation(cx);
                self.report(save, "the dictation permissions", cx);
                let save = prefs::save_html_width(skyelab_core::HtmlWidth::default(), cx);
                self.report(save, "the visual reply width", cx);
                self.set_threads(
                    move |t| {
                        t.permission_mode = fresh.permission_mode.clone();
                        t.claude_model = fresh.claude_model.clone();
                        t.claude_effort = fresh.claude_effort.clone();
                        t.codex_model = fresh.codex_model.clone();
                        t.codex_effort = fresh.codex_effort.clone();
                        t.cursor_model = fresh.cursor_model.clone();
                        t.antigravity_model = fresh.antigravity_model.clone();
                        t.antigravity_effort = fresh.antigravity_effort.clone();
                        t.generate_titles = fresh.generate_titles;
                    },
                    cx,
                )
            }
            Section::Diffs => {
                let save = prefs::save_diff(DiffSettings::default(), cx);
                self.report(save, "Diffs", cx);
                self.set_threads(
                    move |t| {
                        t.fetch_minutes = fresh.fetch_minutes;
                        t.generate_git_text = fresh.generate_git_text;
                    },
                    cx,
                );
            }
            Section::Worktrees => {
                self.set_threads(move |t| t.new_worktree = fresh.new_worktree, cx)
            }
            Section::General => {
                self.editor_input
                    .update(cx, |i, cx| i.set_value("", window, cx));
                self.save_editor(cx);
                let save = crate::updates::set_enabled(true, cx);
                self.report(save, "the update check", cx);
                let save = crate::updates::restore_channel(cx);
                self.report(save, "the update channel", cx);
                let save = prefs::save_terminal(Default::default(), cx);
                self.report(save, "the terminal settings", cx);
                let quit = crate::workspace::quit_guard::set_asks(true, cx)
                    .and_then(|()| crate::workspace::quit_guard::set_keeps(false, cx))
                    .and_then(|()| crate::workspace::resume_after_restart::set_enabled(true, cx));
                if let Err(e) = quit {
                    self.error = Some(format!("Saving the quit settings failed: {e:#}"));
                }
            }
            _ => {}
        }
        if self.section == Section::Chat {
            self.codex_model_input
                .update(cx, |i, cx| i.set_value("", window, cx));
            self.cursor_model_input
                .update(cx, |i, cx| i.set_value("", window, cx));
            self.antigravity_model_input
                .update(cx, |i, cx| i.set_value("", window, cx));
        }
        cx.notify();
    }

    /// The section's error: a short human sentence first; the full text (paths, causes) behind Details, with copy.
    pub(super) fn render_error(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let full = self.error.clone().or_else(|| self.threads_error.clone())?;
        let theme = cx.theme();
        let (danger, ink) = (theme.danger, ui::ink(theme, theme.danger));
        let short = super::widgets::short_error(&full);
        let open = self.error_open;
        Some(
            v_flex()
                .id("settings-error")
                .gap_2()
                .px_4()
                .py_3()
                .rounded(ui::RADIUS_LG)
                .bg(danger.opacity(0.08))
                .border_1()
                .border_color(danger.opacity(0.3))
                .child(
                    h_flex()
                        .gap_3()
                        .child(
                            gpui_component::Icon::new(SkyeIcon::CircleAlert)
                                .size_4()
                                .text_color(ink),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_size(ui::BODY)
                                .text_color(ink)
                                .child(short),
                        )
                        .child(
                            Button::new("settings-error-details")
                                .ghost()
                                .xsmall()
                                .label(if open { "Hide details" } else { "Details" })
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.error_open = !this.error_open;
                                    cx.notify();
                                })),
                        ),
                )
                .when(open, |d| {
                    let copy = full.clone();
                    d.child(
                        h_flex()
                            .items_start()
                            .gap_2()
                            .pl_7()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .font_family(theme.mono_font_family.clone())
                                    .text_size(DESC)
                                    .text_color(theme.muted_foreground)
                                    .child(full),
                            )
                            .child(
                                Button::new("settings-error-copy")
                                    .ghost()
                                    .xsmall()
                                    .icon(SkyeIcon::Copy)
                                    .tooltip("Copy")
                                    .on_click(move |_, _, cx| {
                                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                                            copy.clone(),
                                        ))
                                    }),
                            ),
                    )
                })
                .into_any_element(),
        )
    }
}
