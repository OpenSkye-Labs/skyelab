//! Settings → Keyboard shortcuts (T3's Keybindings page): every SkyeLab action with the keys that run it now,
//! searchable, and the button that opens keybindings.toml. Editing happens in the file; this page reads it back.

use super::{Settings, SettingsEvent, widgets};
use crate::ui;
use gpui::{AnyElement, App, AsKeystroke as _, Context, Keystroke, SharedString, div, prelude::*};
use gpui_component::{
    ActiveTheme as _, Sizable as _, button::Button, h_flex, input::Input, kbd::Kbd, v_flex,
};

/// One action's row.
pub(super) struct KeyRow {
    name: SharedString,
    pub(super) id: SharedString,
    pub(super) binds: Vec<Bind>,
    /// Name, id, keys and contexts, lowercased: what the search matches.
    haystack: String,
}

pub(super) struct Bind {
    keys: Vec<Keystroke>,
    pub(super) context: SharedString,
    pub(super) custom: bool,
}

/// Every listed action and its bindings, highest precedence first.
pub(super) fn rows(cx: &App) -> Vec<KeyRow> {
    let keymap = cx.key_bindings();
    let keymap = keymap.borrow();
    let mut rows: Vec<KeyRow> = cx
        .all_action_names()
        .iter()
        .filter(|n| crate::palette::listed(n))
        .filter_map(|&name| {
            let action = cx.build_action(name, None).ok()?;
            let binds: Vec<Bind> = keymap
                .bindings_for_action(action.as_ref())
                .rev()
                .map(|b| Bind {
                    keys: b
                        .keystrokes()
                        .iter()
                        .map(|k| k.as_keystroke().clone())
                        .collect(),
                    context: b
                        .predicate()
                        .map(|p| p.to_string())
                        .unwrap_or_default()
                        .into(),
                    custom: b.meta() == Some(crate::keymap::CUSTOM),
                })
                .collect();
            let label = crate::palette::humanize(name);
            let mut haystack = format!("{label} {name}");
            for b in &binds {
                for k in &b.keys {
                    haystack.push(' ');
                    haystack.push_str(&k.unparse(cx.platform_info().os));
                }
                haystack.push(' ');
                haystack.push_str(&b.context);
            }
            Some(KeyRow {
                name: label.into(),
                id: name.into(),
                binds,
                haystack: haystack.to_lowercase(),
            })
        })
        .collect();
    rows.sort_by(|a, b| a.name.cmp(&b.name));
    rows
}

/// Whether every word of `query` is in the row.
fn matches(row: &KeyRow, query: &str) -> bool {
    query
        .to_lowercase()
        .split_whitespace()
        .all(|w| row.haystack.contains(w))
}

impl Settings {
    /// Re-reads the keymap and the search; called when either changes, never from render.
    pub(super) fn refresh_keys(&mut self, cx: &mut Context<Self>) {
        let query = self.keys_search.read(cx).value().to_string();
        self.keys = rows(cx)
            .into_iter()
            .filter(|r| matches(r, &query))
            .collect();
        cx.notify();
    }

    fn open_keys_file(&mut self, cx: &mut Context<Self>) {
        match skyelab_core::keymap::default_path() {
            Some(path) => cx.emit(SettingsEvent::OpenKeysFile { path }),
            None => {
                self.error = Some("no config directory on this OS".into());
                cx.notify();
            }
        }
    }

    pub(super) fn render_keys(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let toolbar = widgets::row(true, cx)
            .flex_wrap()
            .child(
                div()
                    .flex_1()
                    .min_w(gpui::px(200.))
                    .child(Input::new(&self.keys_search).small().cleanable(true)),
            )
            .child(
                Button::new("open-keys-file")
                    .outline()
                    .small()
                    .label("Open keybindings file")
                    .on_click(cx.listener(|this, _, _, cx| this.open_keys_file(cx))),
            );
        let problems: Vec<AnyElement> = cx
            .try_global::<crate::keymap::Keymap>()
            .map(|k| k.problems.iter().map(|p| p.to_string()).collect::<Vec<_>>())
            .unwrap_or_default()
            .into_iter()
            .enumerate()
            .map(|(i, p)| {
                widgets::row(i == 0, cx)
                    .text_color(ui::ink(cx.theme(), cx.theme().danger))
                    .child(format!("keybindings.toml {p}"))
                    .into_any_element()
            })
            .collect();
        let theme = cx.theme();
        let rows: Vec<AnyElement> = self
            .keys
            .iter()
            .enumerate()
            .map(|(i, row)| {
                let keys = if row.binds.is_empty() {
                    div()
                        .text_size(ui::META)
                        .text_color(muted)
                        .child("Unbound")
                        .into_any_element()
                } else {
                    v_flex()
                        .items_end()
                        .gap_1()
                        .children(row.binds.iter().map(|b| {
                            h_flex()
                                .gap_1()
                                .when(b.custom, |el| {
                                    el.child(
                                        div()
                                            .px_1()
                                            .rounded(ui::RADIUS_SM)
                                            .bg(theme.primary.opacity(0.12))
                                            .text_size(ui::META)
                                            .text_color(ui::ink(theme, theme.primary))
                                            .child("Custom"),
                                    )
                                })
                                .when(b.context.as_ref() != "Workspace", |el| {
                                    el.child(ui::mono(b.context.clone(), theme).text_size(ui::META))
                                })
                                .children(b.keys.iter().map(|k| Kbd::new(k.clone())))
                        }))
                        .into_any_element()
                };
                widgets::row(i == 0, cx)
                    .justify_between()
                    .child(
                        v_flex()
                            .min_w_0()
                            .child(
                                div()
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .child(row.name.clone()),
                            )
                            .child(ui::mono(row.id.clone(), theme).text_size(ui::META)),
                    )
                    .child(keys)
                    .into_any_element()
            })
            .collect();
        let empty = rows.is_empty().then(|| {
            widgets::row(true, cx)
                .justify_center()
                .py_8()
                .text_color(muted)
                .child("No shortcuts match your search.")
                .into_any_element()
        });
        v_flex()
            .gap_8()
            .child(
                div()
                    .text_size(ui::BODY)
                    .text_color(muted)
                    .child("Rules in keybindings.toml go on top of these defaults and win. Saved changes apply at once."),
            )
            .child(widgets::section("Keyboard shortcuts", [toolbar.into_any_element()], cx))
            .when(!problems.is_empty(), |el| {
                el.child(widgets::section("Problems in your file", problems, cx))
            })
            .child(ui::group(theme).children(rows).children(empty))
            .into_any_element()
    }
}
