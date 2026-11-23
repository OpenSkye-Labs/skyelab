//! How a thread's items look, shared by local (`ThreadView`) and remote (`RemoteThreadView`) thread tabs.

use super::ask_cards::{self, QuestionDraft};
use super::thread_model::{Item, ToolItem};
use super::timeline_fold;
use super::timeline_text::{divider_text, limited_text, markdown};
use super::tool_cards;
use crate::ui;
use gpui::{
    AnyElement, App, Context, FontWeight, IntoElement, SharedString, Window, div, prelude::*, px,
    relative,
};
use gpui_component::{ActiveTheme as _, Icon, IconName, h_flex, v_flex};
use skyelab_agent::{Decision, DeltaKind};

/// Longest tool input or result shown when a row is expanded; the store keeps the whole text.
const TOOL_PREVIEW: usize = 4000;

pub(super) fn clip(s: &str) -> String {
    match s.char_indices().nth(TOOL_PREVIEW) {
        Some((at, _)) => format!("{}\n…", &s[..at]),
        None => s.to_owned(),
    }
}

#[cfg(test)]
thread_local! {
    static ROWS_BUILT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// How many rows `render_item` has built on this thread, so tests can check a frame builds only visible rows.
#[cfg(test)]
pub(super) fn rows_built() -> usize {
    ROWS_BUILT.with(|n| n.get())
}

/// What the item renderer needs from a thread view, local or remote, so both draw items the same way.
pub(super) trait ItemsView: Sized + 'static {
    fn items(&self) -> &[Item];
    fn is_expanded(&self, tool_id: &str) -> bool;
    fn toggle_tool(&mut self, tool_id: &str, cx: &mut Context<Self>);
    /// Whether an agent is there to answer a pending permission request.
    fn can_answer(&self, cx: &App) -> bool;
    fn answer(&mut self, id: &str, decision: Decision, window: &mut Window, cx: &mut Context<Self>);
    /// Stops the running turn instead of answering (T3 keeps Interrupt beside a pending ask).
    fn interrupt_turn(&mut self, window: &mut Window, cx: &mut Context<Self>);
    /// Shows `path` (a tool card's file) in the Changes tab. Remote tabs have no local files.
    fn open_path(&mut self, _path: &str, _cx: &mut Context<Self>) {}
    /// The local folder reply links resolve in; `None` (remote) links no paths and opens no file links.
    fn files_root(&self) -> Option<&std::path::Path> {
        None
    }
    /// The images a message with this shown text carried, while this tab still holds them.
    fn sent_images(&self, _text: &str) -> Option<&super::image_viewer::Sent> {
        None
    }
    /// Shows a file a reply links to in the Preview tab, at `line`.
    fn preview_path(&mut self, _path: &str, _line: Option<u32>, _cx: &mut Context<Self>) {}
    /// The checkpoint turn and changed files of the turn whose divider is row `done`.
    fn turn_files(&self, _done: usize) -> Option<(u32, &[super::turn_files::FileStat])> {
        None
    }
    /// Shows `path` (absolute) in the Changes tab, scoped to checkpoint turn `turn`.
    fn open_turn_file(&mut self, _turn: u32, _path: &std::path::Path, _cx: &mut Context<Self>) {}
    /// The find bar's tint for row `ix`: `Some(true)` holds the active match, `Some(false)` another one.
    fn find_mark(&self, _ix: usize) -> Option<bool> {
        None
    }
    /// "Fork from here" on reply row `ix` (local threads with a row only).
    fn fork(&self, _ix: usize) -> Option<super::thread_extras::fork::ForkThread> {
        None
    }
    /// Whether AskUserQuestion answers can be sent from here.
    fn can_answer_questions(&self, _cx: &App) -> bool {
        false
    }
    fn draft(&self, _id: &str) -> Option<&QuestionDraft> {
        None
    }
    /// Picks option `option` (or [`ask_cards::OTHER`]) of question `q`.
    fn pick(
        &mut self,
        _id: &str,
        _q: usize,
        _option: usize,
        _multi: bool,
        _cx: &mut Context<Self>,
    ) {
    }
    /// The text field of question `q`'s "Other" answer.
    fn other_input(
        &mut self,
        _id: &str,
        _q: usize,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<gpui::Entity<gpui_component::input::InputState>> {
        None
    }
    fn submit_answers(&mut self, _id: &str, _window: &mut Window, _cx: &mut Context<Self>) {}
    /// ExitPlanMode: approve the plan (the agent may start), or keep planning (it's told no).
    fn answer_plan(
        &mut self,
        id: &str,
        approve: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let d = if approve {
            Decision::AllowOnce
        } else {
            Decision::Deny
        };
        self.answer(id, d, window, cx)
    }
}

/// One row of a thread's item list, folded into its finished turn's header when the turn folds it.
pub(super) fn render_item<V: ItemsView>(
    this: &mut V,
    ix: usize,
    window: &mut Window,
    cx: &mut Context<V>,
) -> AnyElement {
    #[cfg(test)]
    ROWS_BUILT.with(|n| n.set(n.get() + 1));
    let mark = this.find_mark(ix);
    let row = timeline_fold::fold_row(this, ix, |this, cx| render_row(this, ix, window, cx), cx);
    match mark {
        None => row,
        // A find match: tinted, the current one stronger (T3 marks the text; markdown here can't).
        Some(active) => div()
            .debug_selector(move || if active { "find-active" } else { "find-match" }.into())
            .w_full()
            .bg(cx.theme().warning.opacity(if active { 0.22 } else { 0.08 }))
            .child(row)
            .into_any_element(),
    }
}

fn render_row<V: ItemsView>(
    this: &mut V,
    ix: usize,
    window: &mut Window,
    cx: &mut Context<V>,
) -> AnyElement {
    // The finished turn this row is the reply of: its time goes on the copy row; a folded turn's divider is its header.
    let reply_at = timeline_fold::reply_of(this.items(), ix).and_then(|d| match this.items()[d] {
        Item::Done { at_ms, .. } => at_ms,
        _ => None,
    });
    let divider_folded = timeline_fold::fold_head(this.items(), ix).is_some();
    let root = this.files_root().map(std::path::Path::to_path_buf);
    let files = super::turn_files::render(this, ix, cx);
    let theme = cx.theme();
    let muted = theme.muted_foreground;
    // T3's chat column: 736 wide, centered; rows 16 apart.
    let column = |child: AnyElement| {
        h_flex()
            .w_full()
            .justify_center()
            .px_6()
            .child(div().w_full().max_w(ui::CHAT_MAX_W).py_2().child(child))
            .into_any_element()
    };
    // Tool rows sit close together, like T3's work log.
    let tight = |child: AnyElement| {
        h_flex()
            .w_full()
            .justify_center()
            .px_6()
            .child(div().w_full().max_w(ui::CHAT_MAX_W).py_0p5().child(child))
            .into_any_element()
    };
    // The questions card is the one row that needs the view mutably (its answer fields), so only it is cloned.
    if let Some(Item::Approval(a)) = this.items().get(ix)
        && a.tool == skyelab_agent::QUESTION_TOOL
    {
        let a = a.clone();
        return column(ask_cards::render_questions(this, &a, window, cx));
    }
    let this: &V = this;
    let Some(item) = this.items().get(ix) else {
        return div().into_any_element();
    };
    match item {
        Item::Prompt(text) | Item::Steer(text) => column(
            v_flex()
                .w_full()
                .items_end()
                .gap_1p5()
                .children(
                    this.sent_images(text)
                        .map(|images| super::image_viewer::thumbnails(ix, images)),
                )
                .child(
                    div()
                        .max_w(relative(0.8))
                        .p_3()
                        .rounded(ui::RADIUS_XL)
                        .bg(theme.secondary)
                        .text_size(ui::BODY)
                        .child(text.clone()),
                )
                .into_any_element(),
        ),
        Item::Text(text) => column(
            v_flex()
                .px_1()
                .gap_1()
                .text_size(ui::BODY)
                .child(markdown(
                    ("md", ix),
                    text,
                    false,
                    root.as_deref(),
                    window,
                    cx,
                ))
                .child(
                    h_flex()
                        .debug_selector(|| "message-copy".into())
                        .gap_1()
                        .text_size(ui::META)
                        .text_color(muted)
                        .child(
                            div()
                                .opacity(0.6)
                                .child(tool_cards::copy(("copy-msg", ix), text.clone())),
                        )
                        .children(reply_at.map(ui::clock))
                        .children(this.fork(ix).map(super::thread_extras::fork::button)),
                )
                .into_any_element(),
        ),
        Item::Live {
            kind: DeltaKind::Text,
            text,
        } => column(
            div()
                .debug_selector(|| "streaming-text".into())
                .px_1()
                .text_size(ui::BODY)
                .child(markdown(
                    ("md", ix),
                    text,
                    true,
                    root.as_deref(),
                    window,
                    cx,
                ))
                .into_any_element(),
        ),
        Item::Live {
            kind: DeltaKind::Thinking,
            text,
        } => tight(tool_cards::render_thinking(
            this,
            &format!("thinking-{ix}"),
            text,
            true,
            cx,
        )),
        Item::Thinking { id, text } => {
            tight(tool_cards::render_thinking(this, id, text, false, cx))
        }
        Item::Todo(todos) => column(tool_cards::render_todo(todos, cx)),
        Item::Tool(tool) if tool_cards::is_shell(&tool.name) => {
            tight(super::shell_card::render_bash(this, tool, cx))
        }
        Item::Tool(tool)
            if tool_cards::is_edit(&tool.name) && tool.raw["file_path"].is_string() =>
        {
            tight(tool_cards::render_edit(tool, cx))
        }
        Item::Tool(tool) => tight(render_tool(this, tool, cx)),
        Item::Subagent(s) => tight(super::subagent::render(this, s, cx)),
        Item::Approval(a) if a.tool == skyelab_agent::PLAN_TOOL => {
            column(ask_cards::render_plan(this, a, window, cx))
        }
        Item::Approval(a) => column(ask_cards::render_approval(this, a, cx)),
        Item::Limit { resets_at } => column(
            h_flex()
                .debug_selector(|| "limit-row".into())
                .gap_1p5()
                .px_3()
                .py_2()
                .rounded(ui::RADIUS_LG)
                .bg(theme.warning.opacity(0.08))
                .border_1()
                .border_color(theme.warning.opacity(0.35))
                .text_size(ui::BODY)
                .child(
                    Icon::new(IconName::TriangleAlert)
                        .size_4()
                        .text_color(ui::ink(theme, theme.warning)),
                )
                .child(limited_text(*resets_at, ui::now_ms() / 1000))
                .into_any_element(),
        ),
        Item::Notice { text, .. } => column(
            h_flex()
                .debug_selector(|| "notice-row".into())
                .gap_1p5()
                .px_1()
                .text_size(ui::META)
                .text_color(theme.muted_foreground)
                .child(Icon::new(IconName::Info).size_3p5())
                .child(text.clone())
                .into_any_element(),
        ),
        Item::Interrupted => column(
            div()
                .debug_selector(|| "interrupted-note".into())
                .px_1()
                .text_size(ui::META)
                .text_color(theme.muted_foreground)
                .child("(interrupted)")
                .into_any_element(),
        ),
        Item::Output { id, text } => tight(tool_cards::render_output(this, id, text, cx)),
        // The turn's fold header says it already.
        Item::Done { .. } if divider_folded => match files {
            Some(files) => column(files),
            None => div().into_any_element(),
        },
        Item::Done {
            is_error,
            duration_ms,
            cost_usd,
            usage,
            ..
        } => {
            let verb = if *is_error {
                "Failed after"
            } else {
                "Worked for"
            };
            let secs = *duration_ms as f64 / 1000.;
            let color = if *is_error {
                ui::ink(theme, theme.danger)
            } else {
                theme.muted_foreground
            };
            let rule = || div().flex_1().h_px().bg(theme.border);
            column(
                v_flex()
                    .child(
                        h_flex()
                            .gap_3()
                            .py_1()
                            .text_size(ui::META)
                            .text_color(color)
                            .child(rule())
                            .child(divider_text(verb, secs, *usage, *cost_usd))
                            .child(rule()),
                    )
                    .children(files)
                    .into_any_element(),
            )
        }
    }
}

/// The first argument's value, as T3 shows it (`ls crates`, not `"command": "ls crates"`).
pub(super) fn tool_summary(tool: &ToolItem) -> String {
    tool.input
        .lines()
        .find_map(|l| l.split_once(':').map(|(_, v)| v))
        .unwrap_or_default()
        .trim()
        .trim_end_matches(',')
        .trim_matches('"')
        .chars()
        .take(80)
        .collect()
}

fn render_tool<V: ItemsView>(this: &V, tool: &ToolItem, cx: &mut Context<V>) -> AnyElement {
    let theme = cx.theme();
    let open = this.is_expanded(&tool.id);
    let summary = tool_summary(tool);
    let state = match (&tool.result, tool.is_error) {
        (None, _) => "running",
        (Some(_), true) => "error",
        (Some(_), false) => "",
    };
    let id = tool.id.clone();
    let (icon, icon_color) = match (&tool.result, tool.is_error) {
        (None, _) => (IconName::LoaderCircle, theme.info),
        (Some(_), true) => (IconName::CircleAlert, theme.danger),
        (Some(_), false) => (IconName::Check, theme.muted_foreground),
    };
    // T3's work-log row: 24 px icon slot, the tool's name, its argument in mono, a chevron that turns when open.
    let header = h_flex()
        .id(SharedString::from(format!("tool-{}", tool.id)))
        .min_h(px(28.))
        .gap_1p5()
        .px_1()
        .rounded(ui::RADIUS_MD)
        .cursor_pointer()
        .text_size(ui::BODY)
        .hover(|s| s.bg(theme.accent))
        .on_click(cx.listener(move |this, _, _, cx| this.toggle_tool(&id, cx)))
        .child(
            div()
                .w(px(24.))
                .flex()
                .justify_center()
                .flex_shrink_0()
                .child(Icon::new(icon).size_4().text_color(icon_color)),
        )
        .child(
            div()
                .flex_shrink_0()
                .font_weight(FontWeight::MEDIUM)
                .child(tool.name.clone()),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .font_family(theme.mono_font_family.clone())
                .text_size(ui::META)
                .text_color(theme.muted_foreground)
                .child(summary),
        )
        .when(!state.is_empty(), |this| {
            this.child(
                div()
                    .text_size(ui::META)
                    .text_color(icon_color)
                    .child(state),
            )
        })
        .child(
            Icon::new(if open {
                IconName::ChevronDown
            } else {
                IconName::ChevronRight
            })
            .size_3()
            .text_color(theme.muted_foreground.opacity(0.7)),
        );
    let block = |label: &'static str, text: String| {
        v_flex()
            .gap_1()
            .child(
                div()
                    .text_size(ui::META)
                    .text_color(theme.muted_foreground)
                    .child(label),
            )
            .child(
                div()
                    .px_3()
                    .py_2()
                    .rounded(ui::RADIUS)
                    .bg(theme.muted)
                    .border_1()
                    .border_color(theme.border)
                    .font_family(theme.mono_font_family.clone())
                    .text_size(ui::META)
                    .text_color(theme.muted_foreground)
                    .child(text),
            )
    };
    v_flex()
        .w_full()
        .child(header)
        .when(open, |this| {
            this.child(
                v_flex()
                    .gap_2()
                    .ml_7()
                    .pt_1()
                    .pb_2()
                    .child(block("Input", clip(&tool.input)))
                    .children(tool.result.as_deref().map(|r| block("Result", clip(r)))),
            )
        })
        .into_any_element()
}
