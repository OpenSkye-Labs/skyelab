//! Onboarding's Import projects step (T3's project import): the folders each added account's agents ran in, read
//! from that account's own dir only, grouped by repository. Recent repos are ticked. Every row shows the account it
//! will be bound to: the one whose history has the most sessions there, changeable, or the binding it already has.

use super::render::home_relative;
use super::{Onboarding, OnboardingEvent};
use crate::ui;
use crate::ui::{Button, Checkbox, SkyeIcon};
use gpui::{Context, Div, FontWeight, ParentElement as _, SharedString, div, prelude::*, px};
use gpui_component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_component::{ActiveTheme as _, Icon, h_flex, v_flex};
use skyelab_core::project_import::{self, Candidate, Group};
use std::path::PathBuf;

/// One folder offered for import.
pub(crate) struct Row {
    pub cand: Candidate,
    pub checked: bool,
    /// The account it will run on.
    pub account: String,
    /// Already bound (here or by a parent folder): its account can't change from here.
    pub locked: bool,
}

#[derive(Default)]
pub(crate) struct Import {
    /// The scan is running.
    pub scanning: bool,
    /// The scan ran for this many accounts (a new account scans again).
    pub scanned_for: Option<Vec<String>>,
    pub rows: Vec<Row>,
    pub groups: Vec<Group>,
    pub other: Vec<usize>,
}

impl Import {
    /// The repository groups for the rows as they are now: groups hold row indexes, so redo them whenever rows go.
    pub(crate) fn regroup(&mut self) {
        let cands: Vec<Candidate> = self.rows.iter().map(|r| r.cand.clone()).collect();
        (self.groups, self.other) = project_import::group(&cands);
    }
}

/// Every added account's history folders that exist and aren't projects yet, with their git facts. Blocking: run it
/// off the UI thread.
pub(crate) fn scan(
    accounts: Vec<(String, skyelab_core::Provider, PathBuf)>,
    projects: Vec<PathBuf>,
) -> Vec<Candidate> {
    let per_account = accounts
        .into_iter()
        .map(|(id, provider, dir)| (id, skyelab_agent::import::project_folders(provider, &dir)))
        .collect();
    project_import::merge(per_account)
        .into_iter()
        .filter(|c| c.path.is_dir() && !projects.contains(&c.path))
        .map(|mut c| {
            c.git = skyelab_core::add_project::git_info(&c.path).is_some();
            c.repo =
                project_import::origin_url(&c.path).and_then(|u| project_import::repo_name(&u));
            c
        })
        .collect()
}

impl Onboarding {
    /// Scans the added accounts' histories when the step opens, or again when the accounts changed since.
    pub(super) fn start_import_scan(&mut self, cx: &mut Context<Self>) {
        let ids: Vec<String> = self.config.accounts.iter().map(|a| a.id.clone()).collect();
        if self.import.scanned_for.as_ref() == Some(&ids) || self.import.scanning {
            return;
        }
        self.import.scanning = true;
        let accounts = self
            .config
            .accounts
            .iter()
            .map(|a| (a.id.clone(), a.provider, a.config_dir.clone()))
            .collect();
        let projects = self.projects.clone();
        let task = cx.background_spawn(async move { scan(accounts, projects) });
        self._scan = Some(cx.spawn(async move |this, cx| {
            let found = task.await;
            let _ = this.update(cx, |this, cx| {
                let now = ui::now_ms();
                this.import.rows = found
                    .into_iter()
                    .map(|cand| {
                        let bound = this.config.resolve(&cand.path).map(|a| a.id.clone());
                        Row {
                            checked: cand.recent(now),
                            account: bound.clone().unwrap_or_else(|| cand.account.clone()),
                            locked: bound.is_some(),
                            cand,
                        }
                    })
                    .collect();
                this.import.regroup();
                this.import.scanning = false;
                this.import.scanned_for = Some(ids);
                cx.notify();
            });
        }));
    }

    pub(crate) fn picked(&self) -> Vec<(PathBuf, Option<String>)> {
        self.import
            .rows
            .iter()
            .filter(|r| r.checked)
            .map(|r| (r.cand.path.clone(), (!r.locked).then(|| r.account.clone())))
            .collect()
    }

    pub(super) fn import_picked(&mut self, cx: &mut Context<Self>) {
        let picked = self.picked();
        if picked.is_empty() {
            return self.go(super::Step::Done, cx);
        }
        cx.emit(OnboardingEvent::Import(picked));
    }

    pub(crate) fn toggle_row(&mut self, path: &std::path::Path, cx: &mut Context<Self>) {
        if let Some(r) = self.import.rows.iter_mut().find(|r| r.cand.path == path) {
            r.checked = !r.checked;
            cx.notify();
        }
    }

    /// Picks `account` for `path`; refused for a folder already bound.
    pub(crate) fn set_row_account(
        &mut self,
        path: &std::path::Path,
        account: &str,
        cx: &mut Context<Self>,
    ) {
        if let Some(r) = self
            .import
            .rows
            .iter_mut()
            .find(|r| r.cand.path == path && !r.locked)
        {
            r.account = account.to_owned();
            cx.notify();
        }
    }

    fn import_row(&self, ix: usize, cx: &mut Context<Self>) -> impl IntoElement {
        let r = &self.import.rows[ix];
        let home = self.home.clone();
        let theme = cx.theme();
        let now = ui::now_ms();
        let path = r.cand.path.clone();
        let name: SharedString = r
            .cand
            .path
            .file_name()
            .map_or_else(
                || path.display().to_string(),
                |n| n.to_string_lossy().into_owned(),
            )
            .into();
        let meta = format!(
            "{} · {} session{} · {}",
            home_relative(&path, home.as_deref()),
            r.cand.threads,
            if r.cand.threads == 1 { "" } else { "s" },
            ui::ago(now, r.cand.last_active_ms)
        );
        let account = self.config.accounts.iter().find(|a| a.id == r.account);
        let badge = account.map(|a| ui::AccountBadge::of(a).into_any_element());
        let picker = if r.locked {
            div()
                .flex_shrink_0()
                .id(SharedString::from(format!(
                    "import-bound-{}",
                    path.display()
                )))
                .tooltip(ui::Tooltip::text("Already bound in accounts.toml"))
                .child(
                    h_flex().gap_1().children(badge).child(
                        Icon::new(SkyeIcon::Lock)
                            .size_3()
                            .text_color(theme.muted_foreground),
                    ),
                )
                .into_any_element()
        } else {
            let view = cx.entity().downgrade();
            let options: Vec<(String, String)> = self
                .config
                .accounts
                .iter()
                .map(|a| (a.id.clone(), a.label.clone()))
                .collect();
            let current = r.account.clone();
            let label = account.map_or_else(|| r.account.clone(), |a| a.label.clone());
            let menu_path = path.clone();
            div()
                .flex_shrink_0()
                .id(SharedString::from(format!(
                    "import-picker-{}",
                    path.display()
                )))
                .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(|_, _, cx| cx.stop_propagation())
                .child(
                    Button::new(SharedString::from(format!(
                        "import-account-{}",
                        path.display()
                    )))
                    .ghost()
                    .xsmall()
                    .label(SharedString::from(label))
                    .dropdown_caret(true)
                    .dropdown_menu(move |mut menu, _, _| {
                        for (id, label) in options.clone() {
                            let (view, path, on) = (view.clone(), menu_path.clone(), id == current);
                            menu = menu.item(PopupMenuItem::new(label).checked(on).on_click(
                                move |_, _, cx| {
                                    let _ =
                                        view.update(cx, |o, cx| o.set_row_account(&path, &id, cx));
                                },
                            ));
                        }
                        menu
                    }),
                )
                .into_any_element()
        };
        let toggle_path = path.clone();
        let selector = format!("import-row-{}", name);
        h_flex()
            .relative()
            .id(SharedString::from(format!("import-{}", path.display())))
            .debug_selector(move || selector)
            .h(px(44.))
            .px_3()
            .gap_3()
            .rounded(ui::RADIUS)
            .cursor_pointer()
            .hover(|s| s.bg(theme.accent))
            .on_click(cx.listener(move |this, _, _, cx| this.toggle_row(&toggle_path, cx)))
            .child(
                Checkbox::new(SharedString::from(format!(
                    "import-check-{}",
                    path.display()
                )))
                .checked(r.checked),
            )
            .child(
                Icon::new(if r.cand.git {
                    SkyeIcon::FolderGit2
                } else {
                    SkyeIcon::Folder
                })
                .size_4()
                .text_color(theme.muted_foreground),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
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
                            .text_color(theme.muted_foreground)
                            .child(meta),
                    ),
            )
            .child(picker)
    }

    pub(super) fn import_page(&self, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let mut body = v_flex().gap_1();
        if self.config.accounts.is_empty() {
            body = body.child(super::render::note(
                "Add an account first: projects are found in each account's own agent history.",
                cx,
            ));
        } else if self.import.scanning {
            body = body.child(super::render::note(
                "Looking through your agents' history…",
                cx,
            ));
        } else if self.import.rows.is_empty() {
            body = body.child(super::render::note(
                "No agent history found for these accounts. Add projects any time from the palette's Add project.",
                cx,
            ));
        } else {
            let sections: Vec<(SharedString, Vec<usize>)> = self
                .import
                .groups
                .iter()
                .map(|g| (SharedString::from(g.label.clone()), g.rows.clone()))
                .chain(
                    (!self.import.other.is_empty())
                        .then(|| ("Other folders".into(), self.import.other.clone())),
                )
                .collect();
            for (label, rows) in sections {
                body = body.child(
                    div()
                        .pt_2()
                        .pb_1()
                        .px_3()
                        .text_size(ui::META)
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(muted)
                        .child(label),
                );
                for ix in rows {
                    body = body.child(self.import_row(ix, cx));
                }
            }
        }
        body
    }
}
