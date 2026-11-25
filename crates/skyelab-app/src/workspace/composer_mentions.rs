//! `@` file, `model:` model, `$` skill and `/` command mentions drawn as inline chips in the prompt, after T3's
//! (`ComposerPromptEditorTiptap.tsx`). A chip is one of gpui-base's atomic inline tokens over the mention's own text:
//! the text field steps the caret over it, deletes it whole, undoes it and copies its text, so the prompt's value
//! (what is sent, drafted, stashed and queued) stays the plain text. This file only finds mentions and draws them.

use crate::ui::{self, SkyeIcon};
use gpui::{AnyElement, App, Entity, FontWeight, IntoElement, Styled as _, div, prelude::*, px};
use gpui_component::{
    ActiveTheme as _, Icon, h_flex,
    input::{InlineToken, InlineTokenContext, InputContent, TextareaState},
};
use skyelab_core::Provider;
use std::ops::Range;

/// What a mention names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    /// `@path`
    File,
    /// `model:<provider>/<id>`
    Model,
    /// `$name` (a Codex skill)
    Skill,
    /// `/name` opening the prompt (a command, or a Claude skill)
    Command,
}

/// The mentions in `text`, in order: byte ranges of whole whitespace-separated words. Like T3, a typed mention
/// counts only once whitespace follows it, so a word being typed (and its open menu) is never a chip.
pub(crate) fn parse(text: &str) -> Vec<(Range<usize>, Kind)> {
    let mut out = Vec::new();
    let mut first = true;
    let mut start = None;
    for (i, c) in text.char_indices() {
        match (c.is_whitespace(), start) {
            (false, None) => start = Some(i),
            (true, Some(s)) => {
                if let Some(kind) = kind(&text[s..i], first) {
                    out.push((s..i, kind));
                }
                first = false;
                start = None;
            }
            _ => {}
        }
    }
    out
}

/// The kind of mention `word` is; `first`: it opens the prompt (only there is `/name` a command).
pub(crate) fn kind(word: &str, first: bool) -> Option<Kind> {
    let name = |s: &str| {
        s.starts_with(|c: char| c.is_ascii_alphabetic())
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, ':' | '_' | '-' | '.'))
    };
    if let Some(path) = word.strip_prefix('@') {
        return (!path.is_empty() && !path.contains(['@', '"'])).then_some(Kind::File);
    }
    if let Some((provider, id)) = word.strip_prefix("model:").and_then(|m| m.split_once('/')) {
        return (matches!(provider, "claude" | "codex") && !id.is_empty()).then_some(Kind::Model);
    }
    if let Some(n) = word.strip_prefix('$') {
        return name(n).then_some(Kind::Skill);
    }
    match word.strip_prefix('/') {
        Some(n) if first && name(n) => Some(Kind::Command),
        _ => None,
    }
}

/// The chip's label: a file's name, a model's display name (`model_name(provider, id)`), a skill or command's name.
fn label(word: &str, kind: Kind, model_name: &dyn Fn(&str, &str) -> String) -> String {
    match kind {
        Kind::File => word[1..]
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(word)
            .to_owned(),
        Kind::Model => word
            .strip_prefix("model:")
            .and_then(|m| m.split_once('/'))
            .map_or_else(|| word.to_owned(), |(p, id)| model_name(p, id)),
        Kind::Skill | Kind::Command => word[1..].to_owned(),
    }
}

/// Marks the mentions in `prompt`'s text as chips: the ones it holds stay (deleting the space after a chip keeps
/// it), and newly typed ones are added, except one the caret or selection sits inside. Call after every change
/// and every `set_value` of the composer's prompt. Does nothing during an IME composition (the next change after it
/// catches up).
pub(in crate::workspace) fn sync(
    prompt: &Entity<TextareaState>,
    model_name: &dyn Fn(&str, &str) -> String,
    cx: &mut App,
) {
    let (text, sel, held) = {
        let p = prompt.read(cx);
        (p.value(), p.selected_range(), p.tokens().to_vec())
    };
    let found = parse(&text);
    if found.is_empty() && held.is_empty() {
        return; // most prompts: no work on a keystroke
    }
    let inside = |r: &Range<usize>| {
        [sel.start, sel.end]
            .iter()
            .any(|&at| r.start < at && at < r.end)
    };
    let mut content = InputContent::new(text.clone());
    let spans = held.iter().map(|s| (s.range(), s.token().clone()));
    let new = found.into_iter().filter(|(r, _)| !inside(r)).map(|(r, k)| {
        let word = &text[r.clone()];
        let token = InlineToken::new(word.to_owned(), word.to_owned())
            .with_label(label(word, k, model_name));
        (r, token)
    });
    for (range, token) in spans.chain(new) {
        // A parsed mention a held chip already covers is refused as overlapping: the held one stays.
        content = content.clone().with_token(range, token).unwrap_or(content);
    }
    prompt.update(cx, |p, cx| {
        // Refused only mid-composition, which the next change after it retries.
        let _ = p.set_tokens(content, cx);
    });
}

/// The provider a `model:<provider>/<id>` mention names (an unknown one reads as Claude; [`kind`] admits only the two).
pub(in crate::workspace) fn provider_of(mention: &str) -> Provider {
    if mention.starts_with("model:codex/") {
        Provider::Codex
    } else {
        Provider::Claude
    }
}

/// A chip as T3's inline context chip draws it: no box, the kind's icon and a medium label in the kind's ink (the
/// accent for files, the provider's color and logo for models). Selected, it takes the selection's fill.
pub(in crate::workspace) fn chip(ctx: &InlineTokenContext, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let text = ctx.token().text();
    let size = px(14.);
    let icon = |i: SkyeIcon| Icon::new(i).size(size).flex_shrink_0().into_any_element();
    let (icon, color) = match kind(text, true) {
        Some(Kind::Model) => {
            let provider = provider_of(text);
            let ink = match provider {
                // Claude's orange, as T3 tints its model chips, mixed from the theme's own red and yellow.
                Provider::Claude => ui::ink(theme, theme.red.blend(theme.yellow.opacity(0.4))),
                Provider::Codex => theme.foreground,
            };
            (
                ui::provider_logo(provider, size, cx).into_any_element(),
                ink,
            )
        }
        Some(Kind::Skill) => (icon(SkyeIcon::Sparkles), ui::ink(theme, theme.info)),
        Some(Kind::Command) => (icon(SkyeIcon::SquareTerminal), ui::ink(theme, theme.info)),
        Some(Kind::File) | None => (icon(SkyeIcon::File), ui::ink(theme, theme.primary)),
    };
    h_flex()
        .h(ctx.line_height())
        .max_w(ctx.available_width())
        .gap(px(3.))
        .px(px(1.))
        .rounded(ui::RADIUS_SM)
        .when(ctx.is_selected(), |this| this.bg(theme.selection))
        .text_color(color)
        .font_weight(FontWeight::MEDIUM)
        .child(icon)
        .child(
            div()
                .min_w_0()
                .truncate()
                .child(ctx.token().label().clone()),
        )
        .into_any_element()
}

impl crate::workspace::thread::ThreadView {
    /// [`sync`] for this composer's prompt.
    pub(in crate::workspace) fn sync_chips(&mut self, cx: &mut gpui::Context<Self>) {
        let prompt = self.prompt.clone();
        let this = &*self;
        sync(&prompt, &|p, id| this.mention_model_name(p, id), cx);
    }

    /// Puts `text` in the prompt (its undo history goes), with its mentions as chips.
    pub(in crate::workspace) fn set_prompt(
        &mut self,
        text: impl Into<gpui::SharedString>,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) {
        let text = text.into();
        self.prompt
            .update(cx, |p, cx| p.set_value(text, window, cx));
        self.sync_chips(cx);
    }
}
