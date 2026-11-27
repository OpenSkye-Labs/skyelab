//! Drawing the Usage page (T3's `UsagePage.tsx`): a total and one line per account on the left, the daily chart on
//! the right, then the projects table. Values come precomputed in [`UsageView`]; render only reads.

use super::details_panel::{limit_text, usage_bar};
use super::timeline_text::count;
use super::usage_page::{Metric, UsagePage, UsageView};
use crate::ui;
use gpui::{
    AnyElement, Context, FontWeight, Hsla, IntoElement, Render, SharedString, Window, div,
    prelude::*, px,
};
use gpui_component::{ActiveTheme as _, Theme, h_flex, v_flex};
use skyelab_store::usage::{Range, Spend};

const CHART_H: f32 = 140.;

fn usd(v: f64) -> String {
    if v > 0.0 && v < 0.01 {
        "<$0.01".into()
    } else {
        format!("${v:.2}")
    }
}

fn value(metric: Metric, s: &Spend) -> String {
    match metric {
        Metric::Cost => usd(s.cost_usd),
        Metric::Tokens => count(s.tokens()),
    }
}

fn turns(n: u64) -> String {
    match n {
        1 => "1 turn".into(),
        n => format!("{n} turns"),
    }
}

/// A segmented toggle (T3's `ToggleGroup variant="segmented"`): thin typed adapter over [`ui::SegmentedControl`].
pub(super) fn segments<T: Copy + PartialEq + 'static, V: 'static>(
    id: &'static str,
    aria_label: &'static str,
    items: &[(T, &'static str)],
    on: T,
    pick: impl Fn(&mut V, T, &mut Context<V>) + Copy + 'static,
    cx: &mut Context<V>,
) -> AnyElement {
    let values: Vec<T> = items.iter().map(|&(v, _)| v).collect();
    let selected = values.iter().position(|&v| v == on);
    ui::SegmentedControl::new(id)
        .accessibility_label(aria_label)
        .items(items.iter().map(|&(_, label)| label))
        .selected(selected)
        .on_click(cx.listener(move |this, &ix: &usize, _, cx| {
            if let Some(&v) = values.get(ix) {
                pick(this, v, cx);
            }
        }))
        .into_any_element()
}

fn section_title(text: impl Into<SharedString>, theme: &Theme) -> gpui::Div {
    div()
        .text_size(ui::BODY)
        .font_weight(FontWeight::MEDIUM)
        .text_color(theme.foreground)
        .child(text.into())
}

/// One account: its color, name and provider; its value for the range and share; its limit readings.
fn account_line(
    line: &super::usage_page::AccountLine,
    metric: Metric,
    now: i64,
    theme: &Theme,
    color: Hsla,
) -> AnyElement {
    let a = &line.usage;
    let limit = limit_text(a.limited, a.limited_ms, now);
    let no_reading = a.windows.is_empty() && limit.is_none();
    let other = match metric {
        Metric::Cost => format!("{} tokens", count(line.spend.tokens())),
        Metric::Tokens => usd(line.spend.cost_usd),
    };
    v_flex()
        .id(SharedString::from(format!("usage-account-{}", a.id)))
        .gap_1()
        .child(
            h_flex()
                .gap_2()
                .items_center()
                .child(div().size_2().flex_shrink_0().rounded_full().bg(color))
                .child(
                    h_flex()
                        .flex_1()
                        .min_w_0()
                        .gap_1p5()
                        .items_baseline()
                        .text_size(ui::BODY)
                        .child(div().truncate().child(a.label.clone()))
                        .child(
                            div()
                                .flex_shrink_0()
                                .text_size(ui::MICRO)
                                .text_color(theme.muted_foreground)
                                .child(format!(
                                    "{} · {}",
                                    a.provider.label(),
                                    turns(line.spend.turns)
                                )),
                        ),
                )
                .child(
                    div()
                        .flex_shrink_0()
                        .text_size(ui::BODY)
                        .font_weight(FontWeight::MEDIUM)
                        .child(value(metric, &line.spend)),
                ),
        )
        .child(
            div()
                .h(px(4.))
                .w_full()
                .rounded_full()
                .bg(theme.foreground.opacity(0.08))
                .child(
                    div()
                        .h_full()
                        .rounded_full()
                        .bg(color)
                        .w(gpui::relative(line.share.clamp(0., 1.) as f32)),
                ),
        )
        .child(
            div()
                .text_size(ui::META)
                .text_color(theme.muted_foreground)
                .child(format!(
                    "{:.0}% of {} · {other}",
                    line.share * 100.,
                    match metric {
                        Metric::Cost => "cost",
                        Metric::Tokens => "tokens",
                    }
                )),
        )
        .children(limit.map(|t| {
            div()
                .text_size(ui::META)
                .font_weight(FontWeight::MEDIUM)
                .text_color(ui::ink(theme, theme.warning))
                .child(t)
        }))
        .when(!a.windows.is_empty(), |el| {
            el.child(
                h_flex()
                    .flex_wrap()
                    .gap_x_3()
                    .gap_y_1()
                    .text_size(ui::META)
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.foreground.opacity(0.8))
                    .children(a.windows.iter().map(|w| usage_bar(w, now, theme))),
            )
        })
        .children(a.refresh_error.as_ref().map(|e| {
            div()
                .id(SharedString::from(format!("usage-refresh-error-{}", a.id)))
                .text_size(ui::META)
                .text_color(theme.muted_foreground)
                .child(format!("Couldn't refresh limits: {e}"))
        }))
        .when(no_reading, |el| {
            el.child(
                div()
                    .text_size(ui::META)
                    .text_color(theme.muted_foreground)
                    .child("No limit reading yet"),
            )
        })
        .into_any_element()
}

/// One stacked column per day of the chosen range (today on the right), each account in its own color.
fn chart(view: &UsageView, colors: &[Hsla], theme: &Theme) -> AnyElement {
    let peak = view.peak;
    let bars =
        view.days.iter().enumerate().map(|(d, values)| {
            let total: f64 = values.iter().sum();
            let h = if peak > 0.0 { total / peak } else { 0.0 } as f32;
            v_flex()
                .id(("usage-day", d))
                .flex_1()
                .h_full()
                .justify_end()
                .child(
                    v_flex()
                        .w_full()
                        .h(gpui::relative(h))
                        .min_h(if total > 0.0 { px(2.) } else { px(0.) })
                        .rounded_t(px(2.))
                        .overflow_hidden()
                        .flex_col_reverse()
                        .children(values.iter().zip(colors).filter(|(v, _)| **v > 0.0).map(
                            |(v, c)| div().w_full().h(gpui::relative((v / total) as f32)).bg(*c),
                        )),
                )
        });
    v_flex()
        .gap_1()
        .child(
            h_flex()
                .h(px(CHART_H))
                .gap(px(3.))
                .items_end()
                .border_b_1()
                .border_color(theme.border)
                .children(bars),
        )
        .child(
            h_flex()
                .justify_between()
                .text_size(ui::MICRO)
                .text_color(theme.muted_foreground)
                .child(format!("{} days ago", view.days.len().saturating_sub(1)))
                .child("Today"),
        )
        .into_any_element()
}

impl Render for UsagePage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let _render = crate::debug_renders::count("UsagePage");
        let (range, metric) = (self.range, self.metric);
        // Wrapped under the heading, they start where its text does (`heading`'s 4 px inset).
        let toggles = h_flex()
            .pl_1()
            .gap_2()
            .child(segments(
                "usage-range",
                "Usage time range",
                &Range::ALL.map(|r| (r, r.label())),
                range,
                |this, r, cx| this.set_range(r, cx),
                cx,
            ))
            .child(segments(
                "usage-metric",
                "Usage metric",
                &[(Metric::Cost, "Cost"), (Metric::Tokens, "Tokens")],
                metric,
                |this, m, cx| this.set_metric(m, cx),
                cx,
            ));
        let theme = cx.theme();
        let colors: Vec<Hsla> = self
            .view
            .accounts
            .iter()
            .map(|a| ui::account_color_in(cx, &a.usage.id))
            .collect();
        let now = ui::now_ms();
        let header = h_flex()
            .justify_between()
            .gap_4()
            .flex_wrap()
            .child(
                crate::settings::widgets::heading(
                    "Usage",
                    "What this machine's agents reported, per account. Limits refresh from each account's own CLI while shown.",
                    cx,
                )
                // A definite width, so its line wraps and the toggles drop under it on a narrow dialog.
                .flex_1()
                .min_w(px(280.)),
            )
            .child(toggles);
        let Some(_) = &self.data else {
            return v_flex()
                .gap_8()
                .child(header)
                .child(ui::EmptyState::new("Reading usage…"))
                .into_any_element();
        };
        let view = &self.view;
        let left = v_flex()
            .w(px(300.))
            .flex_shrink_0()
            .gap_5()
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        div()
                            .id("usage-total")
                            .text_size(ui::DISPLAY)
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(value(metric, &view.total)),
                    )
                    .child(
                        div()
                            .text_size(ui::META)
                            .text_color(theme.muted_foreground)
                            .child(format!(
                                "{} · {} · {}",
                                range.label(),
                                turns(view.total.turns),
                                match metric {
                                    Metric::Cost => "as the agents reported it",
                                    Metric::Tokens => "input and output",
                                }
                            )),
                    ),
            )
            .children(
                view.accounts
                    .iter()
                    .zip(&colors)
                    .map(|(a, c)| account_line(a, metric, now, theme, *c)),
            );
        let right = v_flex()
            .flex_1()
            .min_w(px(280.))
            .gap_3()
            .child(section_title(
                format!(
                    "Daily {} · {} days",
                    match metric {
                        Metric::Cost => "cost",
                        Metric::Tokens => "tokens",
                    },
                    super::usage_page::chart_days(range)
                ),
                theme,
            ))
            .child(chart(view, &colors, theme));
        let projects = v_flex()
            .gap_3()
            .child(section_title(
                format!("Projects · {}", range.label()),
                theme,
            ))
            .child(projects_table(view, metric, theme, cx));
        v_flex()
            .id("usage-page")
            .gap_8()
            .child(header)
            .child(
                h_flex()
                    .gap_6()
                    .items_start()
                    .flex_wrap()
                    .child(left)
                    .child(right),
            )
            .child(projects)
            .into_any_element()
    }
}

fn projects_table(
    view: &UsageView,
    metric: Metric,
    theme: &Theme,
    cx: &Context<UsagePage>,
) -> AnyElement {
    let head = |t: &'static str| div().w(px(96.)).flex().justify_end().child(t);
    let header = h_flex()
        .py_2()
        .px_2()
        .border_b_1()
        .border_color(theme.border)
        .text_size(ui::META)
        .text_color(theme.muted_foreground)
        .child(div().flex_1().child("Project"))
        .child(div().w(px(180.)).child("Account"))
        .child(head(match metric {
            Metric::Cost => "Cost",
            Metric::Tokens => "Tokens",
        }))
        .child(head(match metric {
            Metric::Cost => "Tokens",
            Metric::Tokens => "Cost",
        }))
        .child(head("Turns"));
    let rows: Vec<AnyElement> = view
        .projects
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let account = view
                .accounts
                .iter()
                .find(|a| a.usage.id == p.account_id)
                .map_or_else(|| p.account_id.clone(), |a| a.usage.label.clone());
            let cell = |t: String| div().w(px(96.)).flex().justify_end().child(t);
            let other = match metric {
                Metric::Cost => count(p.spend.tokens()),
                Metric::Tokens => usd(p.spend.cost_usd),
            };
            h_flex()
                .id(("usage-project", i))
                .py_2()
                .px_2()
                .border_b_1()
                .border_color(theme.border.opacity(0.5))
                .text_size(ui::BODY)
                .text_color(theme.muted_foreground)
                .hover(|el| el.bg(theme.muted))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_color(theme.foreground)
                        .child(p.project.clone()),
                )
                .child(
                    h_flex()
                        .w(px(180.))
                        .gap_2()
                        .child(ui::StatusDot::new(ui::account_color_in(cx, &p.account_id)))
                        .child(div().truncate().child(account)),
                )
                .child(cell(value(metric, &p.spend)).text_color(theme.foreground))
                .child(cell(other))
                .child(cell(p.spend.turns.to_string()))
                .into_any_element()
        })
        .collect();
    let body = if rows.is_empty() {
        vec![
            div()
                .py_6()
                .flex()
                .justify_center()
                .text_size(ui::BODY)
                .text_color(theme.muted_foreground)
                .child("No turns in this range")
                .into_any_element(),
        ]
    } else {
        rows
    };
    v_flex().child(header).children(body).into_any_element()
}
