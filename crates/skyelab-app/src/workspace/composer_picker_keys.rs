//! The model picker's search, highlighted row and keys (T3's ModelPickerContent): typing narrows the list, ↑/↓ move,
//! Enter picks, Esc closes, and ⌘1–⌘9 (Ctrl elsewhere) pick the Nth row while the picker has the focus.

use super::thread::ThreadView;
use gpui::{
    Action, AnyWindowHandle, AppContext as _, Context, Entity, FocusHandle, Focusable as _,
    KeyBinding, Subscription, Window, actions,
};
use gpui_component::input::{InputEvent, InputState};
use skyelab_agent::CLAUDE_MODELS;
use skyelab_core::Provider;
use skyelab_store::ModelChoice;

actions!(skyelab, [PickerUp, PickerDown, PickerAccept, PickerClose]);

/// Picks the picker's row at `index` (0-based), as listed under the current search.
#[derive(Clone, PartialEq, Action)]
#[action(namespace = skyelab, no_json)]
pub struct PickerJump {
    pub index: usize,
}

/// Set on the open picker, so its keys reach it before the search field's own.
pub(in crate::workspace) const PICKER_CONTEXT: &str = "ModelPicker";

/// An open picker: its search field, the highlighted row, and where the focus goes back on close.
pub(crate) struct PickerState {
    pub search: Entity<InputState>,
    pub hi: usize,
    back: Option<(AnyWindowHandle, FocusHandle)>,
    _search: Subscription,
}

/// A model the picker lists: its id (`None`: the account's default) and label.
pub(in crate::workspace) type PickerRow = (Option<String>, String);

pub(in crate::workspace) fn bind_keys(cx: &mut gpui::App) {
    let input = Some("ModelPicker > Input");
    let mut keys = vec![
        KeyBinding::new("up", PickerUp, input),
        KeyBinding::new("down", PickerDown, input),
        KeyBinding::new("enter", PickerAccept, input),
        KeyBinding::new("escape", PickerClose, input),
    ];
    // Deeper than the workspace's thread jumps on the same keys, so they win only while the picker has the focus.
    let picker = gpui::KeyBindingContextPredicate::parse(PICKER_CONTEXT)
        .ok()
        .map(std::rc::Rc::new);
    for index in 0..9 {
        keys.push(
            KeyBinding::load(
                &format!("secondary-{}", index + 1),
                Box::new(PickerJump { index }),
                picker.clone(),
                false,
                None,
                cx,
            )
            .expect("static keystrokes are valid"),
        );
    }
    cx.bind_keys(keys);
}

impl ThreadView {
    pub(in crate::workspace) fn toggle_picker(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.composer.picker.is_some() {
            return self.close_picker(cx);
        }
        self.composer.menu = None;
        self.list_codex_models(cx);
        self.list_cursor_models(cx);
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search models…"));
        // Only a typed change: the field also notifies on focus and cursor blinks.
        let _search = cx.subscribe(&search, |this, _, e: &InputEvent, cx| {
            if matches!(e, InputEvent::Change)
                && let Some(p) = this.composer.picker.as_mut()
            {
                p.hi = 0; // the list changed under the highlight
                cx.notify();
            }
        });
        // Back to the prompt on close, as in T3: the chip click may have taken the focus, or nothing had it.
        let prompt = self.prompt.read(cx).focus_handle(cx);
        search.update(cx, |s, cx| s.focus(window, cx));
        self.composer.picker = Some(PickerState {
            search,
            hi: 0,
            back: Some((window.window_handle(), prompt)),
            _search,
        });
        self.highlight_current(cx);
        cx.notify();
    }

    /// Puts the highlight on the current pick, so Enter keeps it. Run again when Codex's list lands, as it may
    /// arrive after the picker opened.
    pub(in crate::workspace) fn highlight_current(&mut self, cx: &mut Context<Self>) {
        let current = self.composer.pick.model.clone();
        let hi = self
            .picker_rows(cx)
            .iter()
            .position(|(id, _)| *id == current);
        if let (Some(p), Some(hi)) = (self.composer.picker.as_mut(), hi) {
            p.hi = hi;
        }
    }

    /// Closes the picker; the focus its search held goes back where it was.
    pub(in crate::workspace) fn close_picker(&mut self, cx: &mut Context<Self>) {
        let Some(p) = self.composer.picker.take() else {
            return;
        };
        if let Some((win, to)) = p.back {
            let search = p.search;
            cx.defer(move |cx| {
                let _ = win.update(cx, |_, window, cx| {
                    if search.read(cx).focus_handle(cx).is_focused(window) {
                        to.focus(window, cx);
                    }
                });
            });
        }
        cx.notify();
    }

    /// The models the bound account's provider runs, narrowed by the search: the account's default first. Only
    /// this provider's models: a pick never moves the thread to another account or provider.
    pub(in crate::workspace) fn picker_rows(&self, cx: &gpui::App) -> Vec<PickerRow> {
        let needle = self
            .composer
            .picker
            .as_ref()
            .map(|p| p.search.read(cx).value().trim().to_lowercase())
            .unwrap_or_default();
        self.picker_models(&needle)
    }

    /// The rows whose label or id contains `needle` (lowercase; empty: all).
    pub(in crate::workspace) fn picker_models(&self, needle: &str) -> Vec<PickerRow> {
        let models: Vec<PickerRow> = match self.composer_account().map(|a| a.provider) {
            Some(Provider::Codex) => self
                .composer
                .codex_models
                .as_ref()
                .and_then(|l| l.as_ref().ok())
                .map(|l| {
                    l.iter()
                        .map(|m| (Some(m.id.clone()), m.label.clone()))
                        .collect()
                })
                .unwrap_or_default(),
            Some(Provider::Cursor) => self
                .composer
                .cursor_models
                .as_ref()
                .and_then(|l| l.as_ref().ok())
                .map(|l| {
                    l.iter()
                        .map(|m| (Some(m.id.clone()), m.label.clone()))
                        .collect()
                })
                .unwrap_or_default(),
            _ => CLAUDE_MODELS
                .iter()
                .map(|(id, label)| (Some((*id).to_owned()), (*label).to_owned()))
                .collect(),
        };
        let default = (
            None,
            format!("Account default ({})", self.default_model_name()),
        );
        std::iter::once(default)
            .chain(models)
            .filter(|(id, label)| {
                label.to_lowercase().contains(needle)
                    || id
                        .as_deref()
                        .is_some_and(|id| id.to_lowercase().contains(needle))
            })
            .collect()
    }

    /// The choice a model row makes: that model, with the current effort when the model takes it.
    pub(in crate::workspace) fn model_choice(&self, id: Option<String>) -> ModelChoice {
        let provider = self.composer_account().map(|a| a.provider);
        let takes = |e: &String| match (&id, provider) {
            (Some(id), Some(Provider::Codex)) => self
                .composer
                .codex_models
                .as_ref()
                .and_then(|l| l.as_ref().ok())
                .is_some_and(|l| l.iter().any(|m| m.id == *id && m.efforts.contains(e))),
            (None, Some(Provider::Codex)) | (_, Some(Provider::Cursor)) => false,
            (Some(_), _) => skyelab_agent::CLAUDE_EFFORTS.contains(&e.as_str()),
            // The account's default model: any Claude effort.
            (None, _) => true,
        };
        ModelChoice {
            effort: self.composer.pick.effort.clone().filter(takes),
            model: id,
        }
    }

    /// Sets the effort and keeps the model.
    pub(in crate::workspace) fn pick_effort(
        &mut self,
        effort: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let model = self.composer.pick.model.clone();
        self.pick_model(ModelChoice { model, effort }, cx);
    }

    fn move_picker(&mut self, by: isize, cx: &mut Context<Self>) {
        let n = self.picker_rows(cx).len() as isize;
        if let Some(p) = self.composer.picker.as_mut().filter(|_| n > 0) {
            p.hi = (p.hi as isize + by).rem_euclid(n) as usize;
            cx.notify();
        }
    }

    fn pick_row(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some((id, _)) = self.picker_rows(cx).into_iter().nth(index) {
            let choice = self.model_choice(id);
            self.pick_model(choice, cx);
        }
    }

    pub(in crate::workspace) fn picker_up(
        &mut self,
        _: &PickerUp,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.composer.compare.is_some() {
            return cx.propagate(); // ticking several models is by click; ⌘N jumps threads as usual
        }
        self.move_picker(-1, cx);
    }

    pub(in crate::workspace) fn picker_down(
        &mut self,
        _: &PickerDown,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.composer.compare.is_some() {
            return cx.propagate(); // ticking several models is by click; ⌘N jumps threads as usual
        }
        self.move_picker(1, cx);
    }

    pub(in crate::workspace) fn picker_accept(
        &mut self,
        _: &PickerAccept,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.composer.compare.is_some() {
            return cx.propagate(); // ticking several models is by click; ⌘N jumps threads as usual
        }
        if let Some(hi) = self.composer.picker.as_ref().map(|p| p.hi) {
            self.pick_row(hi, cx);
        }
    }

    pub(in crate::workspace) fn picker_close(
        &mut self,
        _: &PickerClose,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_picker(cx);
    }

    pub(in crate::workspace) fn picker_jump(
        &mut self,
        a: &PickerJump,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.composer.compare.is_some() {
            return cx.propagate(); // ticking several models is by click; ⌘N jumps threads as usual
        }
        self.pick_row(a.index, cx);
    }
}
