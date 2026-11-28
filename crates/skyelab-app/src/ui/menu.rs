//! SkyeLab's popup menu: a focused `Entity` holding its rows, the selected row and an open submenu, drawn with the
//! sizes gpui-component's menu had. Dropdowns (`Button::dropdown_menu`) and right-click menus build it.

use super::Icon;
use gpui::{
    Action, AnyElement, App, AppContext as _, Bounds, ClickEvent, Context, DismissEvent, Entity,
    EventEmitter, FocusHandle, Focusable, IntoElement, KeyBinding, Pixels, SharedString,
    WeakEntity, Window, actions,
};
use std::rc::Rc;

actions!(
    skye_menu,
    [
        SelectUp,
        SelectDown,
        SelectLeft,
        SelectRight,
        Confirm,
        Cancel
    ]
);

pub(super) const KEY_CONTEXT: &str = "SkyeMenu";
/// Popups paint above dialogs (50) and SkyeLab's own overlays.
pub(crate) const POPUP_PRIORITY: usize = 100;

pub(super) fn bind_keys(cx: &mut App) {
    let ctx = Some(KEY_CONTEXT);
    cx.bind_keys([
        KeyBinding::new("up", SelectUp, ctx),
        KeyBinding::new("down", SelectDown, ctx),
        KeyBinding::new("left", SelectLeft, ctx),
        KeyBinding::new("right", SelectRight, ctx),
        KeyBinding::new("enter", Confirm, ctx),
        KeyBinding::new("escape", Cancel, ctx),
    ]);
}

type Handler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;
type Render = Rc<dyn Fn(&mut Window, &mut App) -> AnyElement>;

pub(super) enum Kind {
    Row(SharedString),
    /// A row drawn by its builder, named for assistive technology by its own name.
    Element(SharedString, Render),
    Label(SharedString),
    Separator,
    Submenu(SharedString, Entity<PopupMenu>),
}

/// One row of a [`PopupMenu`].
pub struct PopupMenuItem {
    pub(super) kind: Kind,
    pub(super) icon: Option<Icon>,
    pub(super) action: Option<Box<dyn Action>>,
    pub(super) on_click: Option<Handler>,
    pub(super) checked: bool,
    pub(super) checkable: bool,
    pub(super) disabled: bool,
}

impl PopupMenuItem {
    fn of(kind: Kind) -> Self {
        Self {
            kind,
            icon: None,
            action: None,
            on_click: None,
            checked: false,
            checkable: false,
            disabled: false,
        }
    }

    pub fn new(label: impl Into<SharedString>) -> Self {
        Self::of(Kind::Row(label.into()))
    }

    /// A row drawn by `render`, named `name` for assistive technology.
    pub fn element<E: IntoElement>(
        name: impl Into<SharedString>,
        render: impl Fn(&mut Window, &mut App) -> E + 'static,
    ) -> Self {
        Self::of(Kind::Element(
            name.into(),
            Rc::new(move |window, cx| render(window, cx).into_any_element()),
        ))
    }

    pub fn icon(mut self, icon: impl Into<Icon>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = checked;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn action(mut self, action: Box<dyn Action>) -> Self {
        self.action = Some(action);
        self
    }

    pub fn on_click(mut self, f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Rc::new(f));
        self
    }

    pub(super) fn pickable(&self) -> bool {
        !self.disabled
            && matches!(
                self.kind,
                Kind::Row(_) | Kind::Element(..) | Kind::Submenu(..)
            )
    }

    pub(super) fn name(&self) -> Option<SharedString> {
        match &self.kind {
            Kind::Row(l) | Kind::Element(l, _) | Kind::Label(l) | Kind::Submenu(l, _) => {
                Some(l.clone())
            }
            Kind::Separator => None,
        }
    }

    pub(super) fn has_left_icon(&self, checks_right: bool) -> bool {
        match self.kind {
            Kind::Row(_) | Kind::Element(..) => {
                self.icon.is_some() || (!checks_right && self.checked)
            }
            Kind::Submenu(..) => self.icon.is_some(),
            _ => false,
        }
    }
}

pub struct PopupMenu {
    pub(super) focus: FocusHandle,
    pub(super) items: Vec<PopupMenuItem>,
    action_context: Option<FocusHandle>,
    restore_focus: Option<FocusHandle>,
    pub(super) selected: Option<usize>,
    /// The submenu row whose menu is showing.
    pub(super) open_submenu: Option<usize>,
    pub(super) min_w: Option<Pixels>,
    pub(super) checks_right: bool,
    parent: Option<WeakEntity<PopupMenu>>,
    pub(super) bounds: Bounds<Pixels>,
    pub(super) depth: usize,
}

impl EventEmitter<DismissEvent> for PopupMenu {}

impl Focusable for PopupMenu {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl PopupMenu {
    pub fn build(
        window: &mut Window,
        cx: &mut App,
        f: impl FnOnce(Self, &mut Window, &mut Context<Self>) -> Self,
    ) -> Entity<Self> {
        let restore_focus = window.focused(cx);
        cx.new(|cx| {
            let menu = Self {
                focus: cx.focus_handle(),
                items: Vec::new(),
                action_context: None,
                restore_focus,
                selected: None,
                open_submenu: None,
                min_w: None,
                checks_right: false,
                parent: None,
                bounds: Bounds::default(),
                depth: 0,
            };
            f(menu, window, cx)
        })
    }

    /// Where an action row's action is dispatched, and where focus goes after it.
    pub fn action_context(mut self, focus: FocusHandle) -> Self {
        self.action_context = Some(focus);
        self
    }

    pub fn min_w(mut self, w: impl Into<Pixels>) -> Self {
        self.min_w = Some(w.into());
        self
    }

    /// Draw checks at the end of the row instead of in the icon column.
    pub fn checks_right(mut self) -> Self {
        self.checks_right = true;
        self
    }

    pub fn item(mut self, item: PopupMenuItem) -> Self {
        self.items.push(item);
        self
    }

    /// A row that dispatches `action`.
    pub fn menu(self, label: impl Into<SharedString>, action: Box<dyn Action>) -> Self {
        self.item(PopupMenuItem::new(label).action(action))
    }

    pub fn menu_with_icon(
        self,
        label: impl Into<SharedString>,
        icon: impl Into<Icon>,
        action: Box<dyn Action>,
    ) -> Self {
        self.item(PopupMenuItem::new(label).icon(icon).action(action))
    }

    /// A row that dispatches `action` and reports `checked` either way.
    pub fn menu_with_check(
        self,
        label: impl Into<SharedString>,
        checked: bool,
        action: Box<dyn Action>,
    ) -> Self {
        let mut item = PopupMenuItem::new(label).checked(checked).action(action);
        item.checkable = true;
        self.item(item)
    }

    /// A heading row nobody can pick.
    pub fn label(self, label: impl Into<SharedString>) -> Self {
        self.item(PopupMenuItem::of(Kind::Label(label.into())))
    }

    /// A line between groups; none at the top or twice in a row.
    pub fn separator(self) -> Self {
        match self.items.last() {
            None
            | Some(PopupMenuItem {
                kind: Kind::Separator,
                ..
            }) => self,
            Some(_) => self.item(PopupMenuItem::of(Kind::Separator)),
        }
    }

    pub fn submenu(
        self,
        label: impl Into<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
        f: impl FnOnce(Self, &mut Window, &mut Context<Self>) -> Self,
    ) -> Self {
        let parent = cx.entity().downgrade();
        let depth = self.depth + 1;
        let menu = Self::build(window, cx, f);
        menu.update(cx, |m, _| {
            m.parent = Some(parent);
            m.depth = depth;
        });
        self.item(PopupMenuItem::of(Kind::Submenu(label.into(), menu)))
    }

    pub(super) fn submenu_entity(&self) -> Option<Entity<PopupMenu>> {
        match &self.items.get(self.open_submenu?)?.kind {
            Kind::Submenu(_, menu) => Some(menu.clone()),
            _ => None,
        }
    }

    fn resolved_action_context(&self, cx: &App) -> Option<FocusHandle> {
        self.action_context.clone().or_else(|| {
            let parent = self.parent.as_ref()?.upgrade()?;
            parent.read(cx).resolved_action_context(cx)
        })
    }

    /// Where a row's shortcut is looked up: the action context, the trigger, then the menu itself.
    pub(super) fn key_paths(&self, cx: &App) -> Vec<FocusHandle> {
        let restore = self.resolved_restore_focus(cx);
        [
            self.resolved_action_context(cx),
            restore,
            Some(self.focus.clone()),
        ]
        .into_iter()
        .flatten()
        .collect()
    }

    fn resolved_restore_focus(&self, cx: &App) -> Option<FocusHandle> {
        match self.parent.as_ref().and_then(|p| p.upgrade()) {
            Some(parent) => parent.read(cx).resolved_restore_focus(cx),
            None => self
                .restore_focus
                .clone()
                .or_else(|| self.action_context.clone()),
        }
    }

    pub(super) fn select(&mut self, ix: Option<usize>, cx: &mut Context<Self>) {
        if self.selected != ix {
            self.selected = ix;
            if self.open_submenu.is_some() && self.open_submenu != ix {
                self.open_submenu = None;
            }
            cx.notify();
        }
    }

    fn pickable(&self) -> impl DoubleEndedIterator<Item = usize> + '_ {
        (0..self.items.len()).filter(|&i| self.items[i].pickable())
    }

    pub(super) fn select_up(&mut self, _: &SelectUp, _: &mut Window, cx: &mut Context<Self>) {
        cx.stop_propagation();
        let cur = self.selected.unwrap_or(0);
        let ix = self
            .pickable()
            .rev()
            .find(|&i| i < cur)
            .or_else(|| self.pickable().last());
        self.select(ix.or(self.selected), cx);
    }

    pub(super) fn select_down(&mut self, _: &SelectDown, _: &mut Window, cx: &mut Context<Self>) {
        cx.stop_propagation();
        let ix = match self.selected {
            None => self.pickable().next(),
            Some(cur) => self
                .pickable()
                .find(|&i| i > cur)
                .or_else(|| self.pickable().next()),
        };
        self.select(ix.or(self.selected), cx);
    }

    pub(super) fn select_right(
        &mut self,
        _: &SelectRight,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.stop_propagation();
        if let Some(ix) = self.selected {
            self.open_submenu_at(ix, true, window, cx);
        }
    }

    pub(super) fn select_left(
        &mut self,
        _: &SelectLeft,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.stop_propagation();
        let Some(parent) = self.parent.as_ref().and_then(|p| p.upgrade()) else {
            return;
        };
        self.selected = None;
        parent.update(cx, |p, cx| {
            p.open_submenu = None;
            p.focus.focus(window, cx);
            cx.notify();
        });
    }

    /// Shows row `ix`'s submenu, if it has one; `focus` moves the keyboard into it on its first row.
    pub(super) fn open_submenu_at(
        &mut self,
        ix: usize,
        focus: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(PopupMenuItem {
            kind: Kind::Submenu(_, menu),
            disabled: false,
            ..
        }) = self.items.get(ix)
        else {
            return;
        };
        let menu = menu.clone();
        self.open_submenu = Some(ix);
        if focus {
            menu.update(cx, |m, cx| {
                let first = m.pickable().next();
                m.select(first, cx);
                m.focus.focus(window, cx);
            });
        }
        cx.notify();
    }

    pub(super) fn confirm(&mut self, _: &Confirm, window: &mut Window, cx: &mut Context<Self>) {
        cx.stop_propagation();
        if let Some(ix) = self.selected {
            self.activate(ix, &ClickEvent::default(), window, cx);
        }
    }

    pub(super) fn activate(
        &mut self,
        ix: usize,
        event: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(item) = self.items.get(ix).filter(|i| i.pickable()) else {
            return;
        };
        if let Kind::Submenu(..) = item.kind {
            self.open_submenu_at(ix, true, window, cx);
            return;
        }
        let (on_click, action) = (
            item.on_click.clone(),
            item.action.as_ref().map(|a| a.boxed_clone()),
        );
        if let Some(on_click) = on_click {
            on_click(event, window, cx);
        } else if let Some(action) = action {
            if let Some(context) = self.resolved_action_context(cx) {
                context.focus(window, cx);
            }
            window.dispatch_action(action, cx);
        }
        self.dismiss(&Cancel, window, cx);
    }

    /// Closes this menu and every menu above it, giving focus back unless a pick moved it elsewhere.
    pub(super) fn dismiss(&mut self, _: &Cancel, window: &mut Window, cx: &mut Context<Self>) {
        self.selected = None;
        self.open_submenu = None;
        let moved_away = window.focused(cx).is_some() && !self.chain_has_focus(window, cx);
        if !moved_away && let Some(focus) = self.resolved_restore_focus(cx) {
            focus.focus(window, cx);
        }
        cx.emit(DismissEvent);
        if let Some(parent) = self.parent.as_ref().and_then(|p| p.upgrade()) {
            parent.update(cx, |p, cx| p.dismiss(&Cancel, window, cx));
        }
    }

    fn chain_has_focus(&self, window: &Window, cx: &App) -> bool {
        if self.focus.contains_focused(window, cx) {
            return true;
        }
        let up = self.parent.as_ref().and_then(|p| p.upgrade());
        up.is_some_and(|p| p.read(cx).chain_has_focus(window, cx))
    }

    /// Whether `at` falls on this menu, an open submenu, or a parent menu.
    pub(super) fn chain_contains(&self, at: gpui::Point<Pixels>, cx: &App) -> bool {
        if self.bounds.contains(&at) {
            return true;
        }
        let sub = self.submenu_entity();
        if sub.is_some_and(|s| s.read(cx).contains_down(at, cx)) {
            return true;
        }
        let up = self.parent.as_ref().and_then(|p| p.upgrade());
        up.is_some_and(|p| p.read(cx).bounds.contains(&at))
    }

    fn contains_down(&self, at: gpui::Point<Pixels>, cx: &App) -> bool {
        self.bounds.contains(&at)
            || self
                .submenu_entity()
                .is_some_and(|s| s.read(cx).contains_down(at, cx))
    }
}
