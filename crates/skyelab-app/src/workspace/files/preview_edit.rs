//! Editing a text preview (docs/decisions/files.md): the Edit toggle, mod+S, the unsaved marker, the Save / Discard /
//! Cancel question before edits would be left, and "Changed on disk" (Reload / Overwrite) when the file no longer
//! holds what was loaded. Saves go through `skyelab_core::files::save_text`: inside the folder only, atomic, and
//! never over a file that changed since it was loaded.

use super::{Body, PreviewPanel};
use crate::ui::Button;
use gpui::{Context, SharedString, Subscription, Task, Window, prelude::*};
use gpui_component::input::{InputEvent, Rope};
use skyelab_core::files::Saved;
use std::cell::RefCell;
use std::rc::Rc;

gpui::actions!(skyelab, [SaveFile]);

pub(in crate::workspace) const SAVE_KEY: &str = if cfg!(target_os = "macos") {
    "cmd-s"
} else {
    "ctrl-s"
};

/// What runs once the edits are saved or dropped.
pub(super) type Then = Box<dyn FnOnce(&mut PreviewPanel, &mut Window, &mut Context<PreviewPanel>)>;

/// The line under the header.
#[derive(Clone, PartialEq)]
pub(super) enum Banner {
    /// The file on disk no longer holds what was loaded.
    Changed,
    Error(SharedString),
}

#[derive(Default)]
pub(super) struct Edit {
    /// The Edit toggle: the editor takes typing.
    pub on: bool,
    /// The file's text as loaded or last saved; `None` when the preview shows no text.
    pub base: Option<String>,
    /// The editor's text at `base` (a rope clone shares its chunks), for the unsaved check.
    saved: Rope,
    pub dirty: bool,
    pub banner: Option<Banner>,
    saving: Option<Task<()>>,
    checking: Option<Task<()>>,
    /// The Save / Discard / Cancel question's continuation while it is open.
    pending: Option<Rc<RefCell<Option<Then>>>>,
    pub(super) watch: Option<Task<()>>,
    pub(super) sub: Option<Subscription>,
}

/// The editor differs from what was saved. The length first, so most keystrokes in a big file compare nothing.
pub(super) fn differs(text: &Rope, saved: &Rope) -> bool {
    text.len() != saved.len() || text != saved
}

impl PreviewPanel {
    /// The editor reports an edit.
    pub(super) fn editor_event(&mut self, e: &InputEvent, cx: &mut Context<Self>) {
        if !matches!(e, InputEvent::Change) || self.edit.base.is_none() {
            return;
        }
        let dirty = differs(self.editor.read(cx).text(), &self.edit.saved);
        if dirty != self.edit.dirty {
            self.edit.dirty = dirty;
            cx.notify();
        }
    }

    /// A text file loaded (or reloaded): it is clean again.
    pub(super) fn text_loaded(&mut self, text: String, cx: &mut Context<Self>) {
        self.edit.saved = self.editor.read(cx).text().clone();
        self.edit.base = Some(text);
        self.edit.dirty = false;
        self.edit.banner = None;
        self.edit.saving = None;
    }

    /// Unsaved edits are in the editor (the quit guard and the tab's close ask about them).
    pub(in crate::workspace) fn unsaved(&self) -> bool {
        self.edit.dirty
    }

    pub(super) fn can_edit(&self) -> bool {
        matches!(self.body, Body::Text) && self.edit.base.is_some()
    }

    /// The header's Edit toggle: on focuses the editor; off asks first when there are edits.
    pub(in crate::workspace) fn toggle_edit(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.can_edit() {
            return;
        }
        if self.edit.on {
            return self.leave(Box::new(|this, _, cx| this.stop_editing(cx)), window, cx);
        }
        self.edit.on = true;
        self.editor.update(cx, |e, cx| e.focus(window, cx));
        cx.notify();
    }

    fn stop_editing(&mut self, cx: &mut Context<Self>) {
        self.edit.on = false;
        cx.notify();
    }

    /// Esc in the editor (nothing of its own to close): leaves editing, asking first when there are edits.
    pub(super) fn escape(
        &mut self,
        _: &gpui_component::input::Escape,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // The find bar's Esc closes the bar (CloseFind), not editing.
        if !self.edit.on || self.find.input.is_some() {
            return cx.propagate();
        }
        self.leave(Box::new(|this, _, cx| this.stop_editing(cx)), window, cx);
    }

    /// The tab's Close (its menu): asks first when there are edits, then closes. The × is hidden while dirty.
    pub(super) fn close_panel(
        &mut self,
        _: &crate::ui::dock::ClosePanel,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.edit.dirty {
            return cx.propagate();
        }
        self.leave(
            // CloseTab, not ClosePanel again: the dock is locked, and CloseTab unlocks it for the close.
            Box::new(|_, window, cx| {
                crate::workspace::dispatch(Box::new(crate::workspace::layout::CloseTab), window, cx)
            }),
            window,
            cx,
        );
    }

    /// mod+S.
    pub(super) fn save_file(&mut self, _: &SaveFile, window: &mut Window, cx: &mut Context<Self>) {
        if self.edit.on && self.edit.dirty {
            self.save(false, None, window, cx);
        }
    }

    /// Writes the editor's text; `overwrite` skips the changed-on-disk check. `then` runs once it is written.
    pub(super) fn save(
        &mut self,
        overwrite: bool,
        then: Option<Then>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(base) = self.edit.base.clone() else {
            return;
        };
        // One save at a time: a second would compare against the first's write and report a false change.
        if self.edit.saving.is_some() {
            return;
        }
        let rope = self.editor.read(cx).text().clone();
        let text = rope.to_string();
        let (root, path) = (self.root.clone(), self.path.clone());
        let write = text.clone();
        let job = cx.background_spawn(async move {
            let base = (!overwrite).then_some(base.as_str());
            skyelab_core::files::save_text(&root, &path, &write, base)
        });
        let (shown_root, shown_path) = (self.root.clone(), self.path.clone());
        self.edit.saving = Some(cx.spawn_in(window, async move |this, cx| {
            let result = job.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.edit.saving = None;
                if this.root != shown_root || this.path != shown_path {
                    return; // another file is shown now
                }
                match result {
                    Ok(Saved::Written) => {
                        this.edit.base = Some(text);
                        this.edit.saved = rope;
                        this.edit.banner = None;
                        // Typing during the save stays unsaved.
                        this.edit.dirty = differs(this.editor.read(cx).text(), &this.edit.saved);
                        if let Some(then) = then {
                            then(this, window, cx);
                        }
                    }
                    Ok(Saved::Changed) => this.edit.banner = Some(Banner::Changed),
                    Err(e) => this.edit.banner = Some(Banner::Error(format!("{e:#}").into())),
                }
                cx.notify();
            });
        }));
    }

    /// The banner's Reload: the file as it is on disk now, at the same line; edits are dropped.
    pub(super) fn reload(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let line = self.editor.read(cx).cursor_position().line + 1;
        self.edit.dirty = false;
        let (root, path) = (self.root.clone(), self.path.clone());
        self.show(root, path, Some(line), window, cx);
    }

    /// The folder changed on disk (its watch): flags the file when it no longer holds what was loaded.
    pub(super) fn disk_changed(&mut self, cx: &mut Context<Self>) {
        let Some(base) = self.edit.base.clone() else {
            return;
        };
        if self.edit.saving.is_some() || self.edit.checking.is_some() {
            return; // our own write, or a check already reading
        }
        let (root, path) = (self.root.clone(), self.path.clone());
        let read = cx.background_spawn(async move {
            skyelab_core::files::changed_on_disk(&root, &path, &base)
        });
        self.edit.checking = Some(cx.spawn(async move |this, cx| {
            let changed = read.await;
            let _ = this.update(cx, |this, cx| {
                this.edit.checking = None;
                let banner = changed.then_some(Banner::Changed);
                if matches!(this.edit.banner, None | Some(Banner::Changed))
                    && this.edit.banner != banner
                {
                    this.edit.banner = banner;
                    cx.notify();
                }
            });
        }));
    }

    /// Runs `then` now, or after Save or Discard when the editor holds unsaved edits (Cancel keeps them).
    pub(in crate::workspace) fn leave(
        &mut self,
        then: Then,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.edit.dirty {
            return then(self, window, cx);
        }
        let then = Rc::new(RefCell::new(Some(then)));
        self.edit.pending = Some(then.clone());
        let this = cx.entity().downgrade();
        let title: SharedString = format!("Save changes to {}?", self.name()).into();
        crate::ui::open_dialog(window, cx, move |dialog, _, _| {
            let (save_then, drop_then) = (then.clone(), then.clone());
            let (save_this, drop_this) = (this.clone(), this.clone());
            dialog
                .w(gpui::px(420.))
                .title(title.clone())
                .child("Your edits to this file aren't saved. Discarding them can't be undone.")
                .footer(
                    crate::ui::dialog_footer()
                        .child(crate::ui::dialog_cancel("preview-leave-cancel"))
                        .child(
                            Button::new("preview-leave-discard")
                                .danger()
                                .small()
                                .label("Discard")
                                .on_click(move |_, window, cx| {
                                    crate::ui::close_dialog(window, cx);
                                    let Some(then) = drop_then.borrow_mut().take() else {
                                        return;
                                    };
                                    let _ = drop_this.update(cx, |p, cx| {
                                        p.edit.pending = None;
                                        p.discard(window, cx);
                                        then(p, window, cx)
                                    });
                                }),
                        )
                        .child(
                            Button::new("preview-leave-save")
                                .primary()
                                .small()
                                .label("Save")
                                .on_click(move |_, window, cx| {
                                    crate::ui::close_dialog(window, cx);
                                    let Some(then) = save_then.borrow_mut().take() else {
                                        return;
                                    };
                                    let _ = save_this.update(cx, |p, cx| {
                                        p.edit.pending = None;
                                        p.save(false, Some(then), window, cx)
                                    });
                                }),
                        ),
                )
        });
    }

    /// The "Changed on disk" / save error line under the header.
    pub(super) fn banner(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        use crate::ui::ActiveTheme as _;
        use crate::ui::h_flex;
        let banner = self.edit.banner.clone()?;
        let theme = cx.theme();
        let row = h_flex()
            .id("preview-banner")
            .min_h(crate::ui::ROW_H)
            .flex_none()
            .px_3()
            .gap_2()
            .border_b_1()
            .border_color(theme.border)
            .bg(theme.warning.opacity(0.12))
            .text_size(crate::ui::META);
        let text = |t: SharedString| gpui::div().min_w_0().flex_1().child(t);
        Some(match banner {
            Banner::Error(e) => row.child(text(e)).into_any_element(),
            Banner::Changed => row
                .child(text(match self.edit.dirty {
                    true => {
                        "This file changed on disk. Reload drops your edits; Overwrite replaces it."
                            .into()
                    }
                    false => "This file changed on disk.".into(),
                }))
                .child(
                    Button::new("preview-reload")
                        .outline()
                        .xsmall()
                        .label("Reload")
                        .on_click(cx.listener(|this, _, window, cx| this.reload(window, cx))),
                )
                .when(self.edit.dirty, |row| {
                    row.child(
                        Button::new("preview-overwrite")
                            .danger()
                            .xsmall()
                            .label("Overwrite")
                            .on_click(
                                cx.listener(|this, _, window, cx| {
                                    this.save(true, None, window, cx)
                                }),
                            ),
                    )
                })
                .into_any_element(),
        })
    }

    /// The editor goes back to the text last loaded or saved, the caret where it was.
    pub(super) fn discard(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(base) = self.edit.base.clone() else {
            return;
        };
        self.editor.update(cx, |e, cx| {
            let at = e.cursor_position();
            e.set_value(base, window, cx);
            e.set_cursor_position(at, window, cx);
        });
        self.edit.saved = self.editor.read(cx).text().clone();
        self.edit.dirty = false;
        if self.edit.banner == Some(Banner::Changed) {
            self.edit.banner = None;
        }
        cx.notify();
    }
}

#[cfg(all(test, unix))] // its tests run the fake agent, a shell script
impl PreviewPanel {
    /// The question's answer, as its buttons give it (tests): `Some(true)` Save, `Some(false)` Discard, `None` Cancel.
    pub(in crate::workspace) fn answer_leave(
        &mut self,
        save: Option<bool>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let then = self.edit.pending.take().and_then(|t| t.borrow_mut().take());
        crate::ui::close_dialog(window, cx);
        match (save, then) {
            (Some(true), Some(then)) => self.save(false, Some(then), window, cx),
            (Some(false), Some(then)) => {
                self.discard(window, cx);
                then(self, window, cx)
            }
            _ => {}
        }
    }
}
