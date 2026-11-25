//! Drawing the Pull requests and Issues pages in T3's list shape (`PullRequestListRow.tsx`, `IssueRow.tsx`): a
//! group per project under a header naming it and its account, then two-line rows: the state glyph, a mono `#N`, the
//! title and its signals, and below it the author, labels and the updated time. Render only reads.

use super::gh_pages::{Dated, Filter, GhEvent, GhPage, Group, Kind, Listing, Source, browser_url};
use super::pr_watch::{checks_look, review_look, state_look};
use super::usage_page_render::segments;
use crate::ui;
use crate::ui::SkyeIcon;
use crate::ui::{Button, Tooltip};
use gpui::{
    AnyElement, Context, Focusable as _, FontWeight, Hsla, IntoElement, Render, SharedString,
    Window, div, prelude::*, px,
};
use gpui_component::{ActiveTheme as _, Icon, Sizable as _, Theme, h_flex, v_flex};
use skyelab_git::{IssueFilter, IssueRow, PrFilter, PrRow, PrState};

const PR_FILTERS: [(Filter, &str); 3] = [
    (Filter::Pr(PrFilter::Mine), "Mine"),
    (Filter::Pr(PrFilter::ReviewRequested), "Review requested"),
    (Filter::Pr(PrFilter::All), "All"),
];
const ISSUE_FILTERS: [(Filter, &str); 2] = [
    (Filter::Issue(IssueFilter::Assigned), "Assigned to me"),
    (Filter::Issue(IssueFilter::All), "All"),
];
/// T3's `text-2xs`: the row's meta line.
const SMALL: gpui::Pixels = px(11.);

fn glyph(icon: SkyeIcon, color: Hsla, tip: &'static str) -> AnyElement {
    div()
        .id(tip)
        .flex_shrink_0()
        .child(Icon::new(icon).small().text_color(color))
        .when(!tip.is_empty(), |d| {
            d.tooltip(move |window, cx| Tooltip::new(tip).build(window, cx))
        })
        .into_any_element()
}

/// The row frame both kinds share: glyph column, two lines, a trailing control.
fn row(id: SharedString, theme: &Theme) -> gpui::Stateful<gpui::Div> {
    h_flex()
        .id(id)
        .w_full()
        .gap_2()
        .px_3()
        .py_2()
        .rounded(ui::RADIUS_MD)
        .cursor_pointer()
        .hover(|d| d.bg(theme.accent))
}

fn lines(
    number: u64,
    title: &str,
    signals: Vec<AnyElement>,
    meta: Vec<AnyElement>,
    updated: i64,
    theme: &Theme,
) -> AnyElement {
    v_flex()
        .flex_1()
        .min_w_0()
        .gap_0p5()
        .child(
            h_flex()
                .min_w_0()
                .gap_1p5()
                .child(
                    ui::mono(format!("#{number}"), theme)
                        .text_size(ui::META)
                        .flex_shrink_0(),
                )
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_size(ui::BODY)
                        .child(title.to_owned()),
                )
                .child(
                    h_flex()
                        .ml_auto()
                        .flex_shrink_0()
                        .gap_1p5()
                        .children(signals),
                ),
        )
        .child(
            h_flex()
                .min_w_0()
                .gap_1p5()
                .text_size(SMALL)
                .text_color(theme.muted_foreground)
                .children(meta)
                .child(div().ml_auto().flex_shrink_0().child(if updated > 0 {
                    ui::ago(ui::now_ms(), updated)
                } else {
                    String::new()
                })),
        )
        .into_any_element()
}

fn browser_button(id: SharedString, url: &str) -> AnyElement {
    let url = url.to_owned();
    Button::new(id)
        .ghost()
        .xsmall()
        .icon(SkyeIcon::ExternalLink)
        .tooltip("Open in browser")
        .on_click(move |_, _, cx| {
            cx.stop_propagation(); // not the row's own click
            if let Some(url) = browser_url(&url) {
                cx.open_url(url);
            }
        })
        .into_any_element()
}

impl GhPage {
    fn pr_row(&self, project: i64, r: &Dated<PrRow>, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let pr = &r.row;
        let (icon, color, label) = state_look(Some(pr.state), theme);
        let mut signals = Vec::new();
        // Read again after a merge: say so on the row, not only in the glyph's tooltip.
        if pr.state != PrState::Open {
            signals.push(ui::Pill::new(label).into_any_element());
        }
        signals.extend(
            [checks_look(pr.checks, theme), review_look(pr.review, theme)]
                .into_iter()
                .flatten()
                .map(|(i, c, tip)| glyph(i, c, tip)),
        );
        let meta = vec![div().child(pr.author.clone()).into_any_element()];
        let (number, url) = (pr.number, pr.url.clone());
        let done = pr.state.is_done();
        row(format!("pr-{project}-{number}").into(), theme)
            .child(glyph(icon, color, label))
            .child(lines(number, &pr.title, signals, meta, r.updated_ms, theme))
            .child(
                Button::new(SharedString::from(format!("pr-review-{project}-{number}")))
                    .ghost()
                    .xsmall()
                    .icon(SkyeIcon::Sparkles)
                    .label("Review with agent")
                    .tooltip("Start a thread that reviews this pull request")
                    .on_click(cx.listener(move |_, _, _, cx| {
                        cx.stop_propagation(); // not the row's open
                        cx.emit(GhEvent::Review {
                            project,
                            number,
                            url: url.clone(),
                        })
                    })),
            )
            // Nothing is left to merge on a merged or closed PR.
            .children((!done).then(|| {
                Button::new(SharedString::from(format!("pr-actions-{project}-{number}")))
                    .ghost()
                    .xsmall()
                    .icon(SkyeIcon::GitMerge)
                    .tooltip("Merge or review…")
                    .on_click(cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation(); // not the row's open
                        this.open_pr_actions(project, number, window, cx);
                    }))
            }))
            .child(browser_button(
                format!("pr-open-{project}-{number}").into(),
                &pr.url,
            ))
            .on_click(cx.listener(move |this, _, _, cx| this.open_pr(project, number, cx)))
            .into_any_element()
    }

    fn issue_row(
        &self,
        project: i64,
        account: &str,
        r: &Dated<IssueRow>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme();
        let issue = &r.row;
        let mut meta = vec![
            div()
                .flex_shrink_0()
                .child(issue.author.clone())
                .into_any_element(),
        ];
        meta.extend(issue.labels.iter().map(|l| {
            let label = SharedString::from(l.clone());
            let chosen = self.label.as_ref() == Some(&label);
            div()
                .id(SharedString::from(format!(
                    "label-{project}-{}-{l}",
                    issue.number
                )))
                .flex_shrink_0()
                .px_1()
                .rounded(ui::RADIUS_SM)
                .bg(if chosen {
                    theme.accent
                } else {
                    theme.secondary
                })
                .hover(|d| d.text_color(theme.foreground))
                .child(label.clone())
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.pick_label(Some(label.clone()), cx);
                }))
                .into_any_element()
        }));
        let signals = issue
            .assignees
            .iter()
            .take(3)
            .map(|a| {
                div()
                    .text_size(SMALL)
                    .text_color(theme.muted_foreground)
                    .child(format!("@{a}"))
                    .into_any_element()
            })
            .collect();
        let (account, row_data) = (account.to_owned(), issue.clone());
        let number = issue.number;
        let state = if issue.closed {
            glyph(SkyeIcon::CircleCheck, theme.muted_foreground, "Closed")
        } else {
            glyph(SkyeIcon::CircleDot, ui::ink(theme, theme.success), "Open")
        };
        row(format!("issue-{project}-{number}").into(), theme)
            .child(state)
            .child(lines(
                number,
                &issue.title,
                signals,
                meta,
                r.updated_ms,
                theme,
            ))
            .child(
                Button::new(SharedString::from(format!(
                    "issue-actions-{project}-{number}"
                )))
                .ghost()
                .xsmall()
                .icon(SkyeIcon::MessageCircle)
                .tooltip("Comment, close or reopen…")
                .on_click(cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation(); // not the row's thread
                    this.open_issue_actions(project, number, window, cx);
                })),
            )
            .child(browser_button(
                format!("issue-open-{project}-{number}").into(),
                &issue.url,
            ))
            .on_click(cx.listener(move |_, _, _, cx| {
                cx.emit(GhEvent::StartIssue {
                    project,
                    account: account.clone(),
                    row: row_data.clone(),
                })
            }))
            .into_any_element()
    }

    fn group(&self, source: &Source, group: &Group, cx: &mut Context<Self>) -> AnyElement {
        let noun = match self.kind {
            Kind::Prs => "pull requests",
            Kind::Issues => "issues",
        };
        let mut rows: Vec<AnyElement> = Vec::new();
        let mut count = None;
        let note = |title: String, hint: SharedString| {
            ui::EmptyState::new(title).hint(hint).into_any_element()
        };
        match &group.listing {
            Listing::Loading => rows.push(note("Loading…".into(), "".into())),
            Listing::Unbound => {}
            Listing::SignIn => rows.push(
                h_flex()
                    .gap_3()
                    .child(note(
                        "Sign in to GitHub".into(),
                        "This project's account has no GitHub login, so nothing was asked.".into(),
                    ))
                    .child(
                        Button::new(SharedString::from(format!("gh-sign-in-{}", source.id)))
                            .outline()
                            .small()
                            .label("Sign in to GitHub")
                            .on_click(|_, window, cx| {
                                window.dispatch_action(
                                    Box::new(super::OpenSettingsSection {
                                        section: crate::settings::Section::Accounts,
                                    }),
                                    cx,
                                )
                            }),
                    )
                    .into_any_element(),
            ),
            Listing::NoGitHub(why) => {
                use skyelab_git::NoGitHub;
                let (title, hint) = match why {
                    NoGitHub::NotARepo => (
                        "This project isn't a git repository",
                        format!("Its {noun} would come from a github.com remote named origin."),
                    ),
                    NoGitHub::NoOrigin => (
                        "This project has no GitHub remote",
                        format!(
                            "Add one named origin to list its {noun}: \
                             git remote add origin https://github.com/OWNER/REPO.git"
                        ),
                    ),
                    NoGitHub::OtherHost => (
                        "This project's remote isn't on GitHub",
                        format!(
                            "Its origin points outside github.com, so there are no {noun} to list here."
                        ),
                    ),
                };
                rows.push(note(title.into(), hint.into()))
            }
            Listing::Failed(why) => rows.push(note(format!("Couldn't list {noun}"), why.clone())),
            Listing::Prs(list) => {
                count = Some(list.len());
                rows.extend(list.iter().map(|r| self.pr_row(source.id, r, cx)));
            }
            Listing::Issues(list) => {
                let account = group
                    .account
                    .as_ref()
                    .map(|a| a.0.clone())
                    .unwrap_or_default();
                let shown = self.issues_shown(list);
                count = Some(shown.len());
                rows.extend(
                    shown
                        .into_iter()
                        .map(|r| self.issue_row(source.id, &account, r, cx)),
                );
            }
        }
        if count == Some(0) {
            rows.push(note(
                format!("No open {noun}"),
                "Nothing matches this filter.".into(),
            ));
        }
        let theme = cx.theme();
        let header = h_flex()
            .gap_2()
            .px_2()
            .child(
                div()
                    .text_size(ui::BODY)
                    .font_weight(FontWeight::MEDIUM)
                    .child(source.name.clone()),
            )
            .children(
                group
                    .account
                    .as_ref()
                    .map(|(id, label)| ui::AccountBadge::new(id.clone(), label.clone())),
            )
            .child(div().flex_1().h_px().bg(theme.border.opacity(0.5)))
            .children(count.map(|n| {
                div()
                    .text_size(ui::META)
                    .text_color(theme.muted_foreground)
                    .child(n.to_string())
            }));
        v_flex()
            .gap_1()
            .child(header)
            .children(rows)
            .into_any_element()
    }
}

impl Render for GhPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (title, filters): (&str, &[(Filter, &str)]) = match self.kind {
            Kind::Prs => ("Pull requests", &PR_FILTERS),
            Kind::Issues => ("Issues", &ISSUE_FILTERS),
        };
        let filter = segments(
            "gh-filter",
            filters,
            self.filter,
            |this: &mut Self, f, cx| this.set_filter(f, cx),
            cx,
        );
        let folded: Vec<(i64, SharedString)> = self
            .folded_off_github()
            .iter()
            .map(|s| (s.id, s.name.clone()))
            .collect();
        let mut groups: Vec<AnyElement> = self
            .shown()
            .into_iter()
            .filter(|(s, _)| !folded.iter().any(|f| f.0 == s.id))
            .map(|(s, g)| self.group(s, g, cx))
            .collect();
        if !folded.is_empty() {
            let theme = cx.theme();
            let projects = match folded.len() {
                1 => "1 project has".to_owned(),
                n => format!("{n} projects have"),
            };
            groups.push(
                div()
                    .id("gh-off-github")
                    .px_2()
                    .text_size(ui::META)
                    .text_color(theme.muted_foreground)
                    .child(format!(
                        "{projects} no GitHub remote named origin: {}",
                        folded
                            .iter()
                            .map(|f| f.1.as_ref())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ))
                    .into_any_element(),
            );
        }
        let theme = cx.theme();
        let label = self.label.clone().map(|l| {
            Button::new("gh-label-clear")
                .ghost()
                .xsmall()
                .icon(SkyeIcon::Close)
                .label(format!("Label: {l}"))
                .on_click(cx.listener(|this, _, _, cx| this.pick_label(None, cx)))
        });
        let header = super::layout::page_header(window, cx)
            .child(
                div()
                    .flex_1()
                    .text_size(ui::BODY)
                    .font_weight(FontWeight::MEDIUM)
                    .child(title),
            )
            .children(label)
            .child(filter)
            .child(
                Button::new("gh-refresh")
                    .ghost()
                    .small()
                    .icon(SkyeIcon::RefreshCw)
                    .tooltip("Refresh")
                    .on_click(cx.listener(|this, _, _, cx| this.refresh(cx))),
            )
            .child(super::layout::panel_toggles(window, cx));
        let body = if groups.is_empty() {
            div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    ui::EmptyState::new("No bound projects")
                        .hint(
                            "Add a project and bind its folder to an account with a GitHub login.",
                        )
                        .page(),
                )
                .into_any_element()
        } else {
            // shortcut: a plain scroll list; each project is capped at 50 rows, so only many projects make it long.
            div()
                .id("gh-scroll")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .child(
                    v_flex()
                        .w_full()
                        .max_w(ui::PAGE_MAX_W)
                        .mx_auto()
                        .px_5()
                        .py_4()
                        .gap_5()
                        .children(groups),
                )
                .into_any_element()
        };
        v_flex()
            .id("gh-page")
            .track_focus(&self.focus_handle(cx))
            .size_full()
            .bg(theme.background)
            .child(header)
            .child(body)
    }
}
