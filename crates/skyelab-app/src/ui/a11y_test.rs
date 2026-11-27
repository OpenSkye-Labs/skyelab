//! Accessibility tests for SkyeLab's UI primitives and pages: AccessKit roles, accessible names, descriptions,
//! toggled/selected/expanded states, dropdown expanded transitions, decorative checkboxes, and smoke renders across the
//! UI Gallery and every Settings section.

use super::{
    Button, Checkbox, EmptyState, SectionHeader, SegmentedControl, Switch, Tabs, TextField,
    icon_button, page_title, row, row_surface, section_title,
    toast::{self, Toast},
};
use crate::ui::ActiveTheme as _;
use crate::ui::SkyeIcon;
use gpui::{
    AppContext as _, Context, Element as _, Entity, IntoElement, ParentElement as _, Render,
    RenderOnce as _, StatefulInteractiveElement as _, Styled as _, TestAppContext, Window,
    accesskit, canvas, div,
};
use gpui_base::test_support::snapshots;
use gpui_component::{
    input::InputState,
    menu::{DropdownMenu as _, PopupMenuItem},
    v_flex,
};
use std::{
    cell::RefCell,
    rc::Rc,
    sync::{Arc, Mutex},
};

fn setup(cx: &mut TestAppContext) {
    super::button_test::setup(cx);
}

#[gpui::test]
fn button_toggle_choice_and_decorative_checkbox_snapshots(cx: &mut TestAppContext) {
    struct Harness {
        field: Entity<InputState>,
    }

    impl Render for Harness {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            v_flex()
                .child(toast::card(
                    9,
                    &Toast::info("Synced"),
                    Rc::new(|_, _| {}),
                    cx,
                ))
                .child(
                    Button::new("btn-save")
                        .label("Save")
                        .accessibility_id("save-id")
                        .accessibility_description("Writes config")
                        .accessibility_selected(true)
                        .toggled(true)
                        .on_click(|_, _, _| {}),
                )
                .child(
                    icon_button("btn-copy", SkyeIcon::Copy)
                        .tooltip("Copy")
                        .on_click(|_, _, _| {}),
                )
                .child(
                    Button::new("btn-disabled")
                        .label("Locked")
                        .disabled(true)
                        .on_click(|_, _, _| {}),
                )
                .child(
                    Switch::new("sw-dark")
                        .checked(true)
                        .accessibility_label("Dark mode")
                        .accessibility_id("dark-mode-switch")
                        .on_click(|_, _, _| {}),
                )
                .child(
                    Checkbox::new("cb-remember")
                        .checked(false)
                        .label("Remember choice")
                        .accessibility_id("remember-checkbox")
                        .on_click(|_, _, _| {}),
                )
                .child(
                    // Decorative checkboxes have no label and must emit zero interactive AX nodes.
                    Checkbox::new("cb-decorative").checked(true).decorative(),
                )
                .child(
                    SegmentedControl::new("seg-theme")
                        .accessibility_label("Theme mode")
                        .accessibility_id("theme-seg")
                        .items(["System", "Light", "Dark"])
                        .selected(Some(2))
                        .on_click(|_, _, _| {}),
                )
                .child(
                    Tabs::new("tabs-view")
                        .accessibility_label("View tabs")
                        .accessibility_id("view-tabs")
                        .items(["Diff", "Checks"])
                        .selected(Some(0))
                        .on_click(|_, _, _| {}),
                )
                .child(
                    TextField::new(&self.field)
                        .accessibility_label("Branch name")
                        .accessibility_id("branch-input"),
                )
        }
    }

    setup(cx);
    let (_, cx) = cx.add_window_view(|window, cx| Harness {
        field: cx.new(|cx| InputState::new(window, cx).placeholder("main")),
    });
    cx.run_until_parked();

    let snaps = cx.update(|window, _| snapshots(window));
    let find = |label: &str| {
        snaps
            .iter()
            .find(|s| s.label() == Some(label))
            .unwrap_or_else(|| {
                let labels: Vec<_> = snaps.iter().filter_map(|s| s.label()).collect();
                panic!("missing snapshot for `{label}`; available labels: {labels:?}")
            })
    };

    let save = find("Save");
    assert_eq!(save.role(), Some(accesskit::Role::Button));
    assert_eq!(save.description(), Some("Writes config"));
    assert_eq!(save.selected(), Some(true));
    assert_eq!(save.checked(), Some(true));
    assert_ne!(save.disabled(), Some(true));

    let copy = find("Copy");
    assert_eq!(copy.role(), Some(accesskit::Role::Button));
    assert_ne!(copy.disabled(), Some(true));

    let disabled = find("Locked");
    assert_eq!(disabled.disabled(), Some(true));

    let dark_mode = find("Dark mode");
    assert_eq!(dark_mode.role(), Some(accesskit::Role::Switch));
    assert_eq!(dark_mode.checked(), Some(true));

    let checkboxes: Vec<_> = snaps
        .iter()
        .filter(|s| s.role() == Some(accesskit::Role::CheckBox))
        .collect();
    assert_eq!(
        checkboxes.len(),
        1,
        "only non-decorative Checkbox must appear in snapshots; got {checkboxes:?}"
    );
    assert_eq!(checkboxes[0].label(), Some("Remember choice"));
    assert_eq!(checkboxes[0].checked(), Some(false));

    // Radio buttons report their state as checked, not selected.
    assert_eq!(find("System").checked(), Some(false));
    assert_eq!(find("Dark").checked(), Some(true));
    assert_eq!(find("Diff").selected(), Some(true));
    assert_eq!(find("Checks").selected(), Some(false));

    // The window's own close button is "Close"; a control with that name would be ambiguous to agent-desktop.
    assert_eq!(
        find("Dismiss notification").role(),
        Some(accesskit::Role::Button)
    );
    assert!(snaps.iter().all(|s| s.label() != Some("Close")));
}

#[gpui::test]
fn dropdown_trigger_exposes_and_toggles_expanded_state(cx: &mut TestAppContext) {
    struct MenuHarness {
        picked: Rc<RefCell<bool>>,
    }

    impl Render for MenuHarness {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let picked = self.picked.clone();
            v_flex().p_4().child(
                Button::new("projects-menu")
                    .label("Projects")
                    .dropdown_menu(move |menu, _, _| {
                        let picked = picked.clone();
                        menu.item(PopupMenuItem::new("All projects").on_click(move |_, _, _| {
                            *picked.borrow_mut() = true;
                        }))
                    }),
            )
        }
    }

    setup(cx);
    let picked = Rc::new(RefCell::new(false));
    let (_, menu_cx) = cx.add_window_view(move |_, _| MenuHarness { picked });
    menu_cx.run_until_parked();

    let closed_snap = menu_cx.update(|window, _| {
        snapshots(window)
            .into_iter()
            .find(|s| s.label() == Some("Projects"))
            .expect("trigger snapshot")
    });
    assert_eq!(closed_snap.expanded(), Some(false));

    menu_cx.simulate_click(closed_snap.bounds().center(), gpui::Modifiers::none());
    menu_cx.run_until_parked();

    let open_snap = menu_cx.update(|window, _| {
        snapshots(window)
            .into_iter()
            .find(|s| s.label() == Some("Projects"))
            .expect("open trigger snapshot")
    });
    assert_eq!(open_snap.expanded(), Some(true));
}

#[gpui::test]
fn row_empty_state_dialog_title_and_toast_a11y_nodes(cx: &mut TestAppContext) {
    #[derive(Default)]
    struct Nodes {
        row_unselected: Option<accesskit::Node>,
        row_selected: Option<accesskit::Node>,
        empty: Option<accesskit::Node>,
        dialog_heading: Option<accesskit::Node>,
        toast_error: Option<accesskit::Node>,
        toast_info: Option<accesskit::Node>,
        toast_clickable: Option<accesskit::Node>,
    }

    fn probe_div(mut any: gpui::AnyElement) -> accesskit::Node {
        if let Some(el) = any.downcast_mut::<gpui::Stateful<gpui::Div>>() {
            let role = el.a11y_role().expect("Stateful<Div> a11y_role");
            let mut node = accesskit::Node::new(role);
            el.write_a11y_info(&mut node);
            return node;
        }
        if let Some(el) = any.downcast_mut::<gpui::Div>() {
            let role = el.a11y_role().expect("Div a11y_role");
            let mut node = accesskit::Node::new(role);
            el.write_a11y_info(&mut node);
            return node;
        }
        panic!("expected Div or Stateful<Div> concrete element")
    }

    struct Probe {
        out: Arc<Mutex<Nodes>>,
    }

    impl Render for Probe {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let out = self.out.clone();
            canvas(
                move |_, window, cx| {
                    let theme = cx.theme().clone();
                    let dismiss: toast::Handler = Rc::new(|_, _| {});
                    let mut n = Nodes::default();

                    let r1 = row("row-1", false, &theme)
                        .aria_label("Fix bug")
                        .on_click(|_, _, _| {})
                        .into_element();
                    let role1 = r1.a11y_role().expect("row role");
                    let mut node1 = accesskit::Node::new(role1);
                    r1.write_a11y_info(&mut node1);
                    n.row_unselected = Some(node1);

                    let r2 = row_surface(div(), "row-2", true, &theme)
                        .aria_label("Selected thread")
                        .into_element();
                    let role2 = r2.a11y_role().expect("row_surface role");
                    let mut node2 = accesskit::Node::new(role2);
                    r2.write_a11y_info(&mut node2);
                    n.row_selected = Some(node2);

                    n.empty = Some(probe_div(
                        EmptyState::new("Nothing here yet")
                            .render(window, cx)
                            .into_any_element(),
                    ));
                    n.dialog_heading = Some(probe_div(
                        super::dialog::heading("Delete thread".into()).into_any_element(),
                    ));
                    n.toast_error = Some(probe_div(
                        toast::card(
                            1,
                            &Toast::error("Disk full").title("Save failed"),
                            dismiss.clone(),
                            cx,
                        )
                        .into_any_element(),
                    ));
                    n.toast_info = Some(probe_div(
                        toast::card(2, &Toast::info("Synced"), dismiss.clone(), cx)
                            .into_any_element(),
                    ));
                    n.toast_clickable = Some(probe_div(
                        toast::card(
                            3,
                            &Toast::new("Thread completed").on_click(|_, _| {}),
                            dismiss,
                            cx,
                        )
                        .into_any_element(),
                    ));

                    // Smoke render heading wrappers too (their inner Heading carries Role::Heading).
                    let _ = page_title("Settings", &theme).into_any_element();
                    let _ = section_title("General", &theme).into_any_element();
                    let _ = SectionHeader::new("Appearance")
                        .render(window, cx)
                        .into_any_element();

                    *out.lock().expect("lock") = n;
                },
                |_, _, _, _| {},
            )
        }
    }

    setup(cx);
    let out = Arc::new(Mutex::new(Nodes::default()));
    let res = out.clone();
    let (_, vcx) = cx.add_window_view(move |_, _| Probe { out });
    vcx.update(|window, cx| {
        window.draw(cx).clear(cx);
    });

    let nodes = res.lock().expect("lock");
    let r1 = nodes.row_unselected.as_ref().expect("r1");
    assert_eq!(r1.role(), accesskit::Role::Button);
    assert_eq!(r1.is_selected(), Some(false));
    assert_eq!(r1.label(), Some("Fix bug"));
    // What agent-desktop presses and asks for before pressing something scrolled away.
    assert!(r1.supports_action(accesskit::Action::Click));
    assert!(r1.supports_action(accesskit::Action::ScrollIntoView));

    let r2 = nodes.row_selected.as_ref().expect("r2");
    assert_eq!(r2.role(), accesskit::Role::Button);
    assert_eq!(r2.is_selected(), Some(true));
    assert_eq!(r2.label(), Some("Selected thread"));

    let empty = nodes.empty.as_ref().expect("empty");
    assert_eq!(empty.role(), accesskit::Role::Status);
    assert_eq!(empty.label(), Some("Nothing here yet"));

    let dlg = nodes.dialog_heading.as_ref().expect("dialog_title");
    assert_eq!(dlg.role(), accesskit::Role::Heading);
    assert_eq!(dlg.label(), Some("Delete thread"));

    let err = nodes.toast_error.as_ref().expect("toast error");
    assert_eq!(err.role(), accesskit::Role::Alert);
    let err_label = err.label().expect("toast error label");
    assert!(err_label.contains("Save failed"), "{err_label}");
    assert!(err_label.contains("Disk full"), "{err_label}");

    let info = nodes.toast_info.as_ref().expect("toast info");
    assert_eq!(info.role(), accesskit::Role::Status);
    assert_eq!(info.label(), Some("Synced"));

    let click_toast = nodes.toast_clickable.as_ref().expect("toast clickable");
    assert_eq!(click_toast.role(), accesskit::Role::Button);
    // The click itself is added by the toast list on this same element (`ToastList::render`).
}

#[gpui::test]
fn gallery_and_all_settings_sections_have_named_controls(cx: &mut TestAppContext) {
    fn assert_all_controls_named(snaps: Vec<gpui_base::test_support::ElementSnapshot>, ctx: &str) {
        assert!(!snaps.is_empty(), "{ctx}: expected rendered UI controls");
        // A field whose editor wasn't painted this frame (scrolled or clipped out) has no role and no native node yet.
        for snap in snaps.into_iter().filter(|s| s.role().is_some()) {
            let unnamed = snap.label().is_none_or(|label| label.trim().is_empty());
            assert!(
                !unnamed,
                "{ctx}: unnamed control in snapshot: path={:?}, role={:?}",
                snap.path(),
                snap.role()
            );
        }
    }

    setup(cx);
    let gallery_win = cx
        .update(super::gallery::open)
        .expect("gallery opens cleanly");
    cx.run_until_parked();
    let gallery_snaps = gallery_win
        .update(cx, |_, window, _| snapshots(window))
        .expect("gallery alive");
    assert_all_controls_named(gallery_snaps, "UiGallery");

    let (settings_view, settings_cx) = cx.add_window_view(|window, cx| {
        crate::settings::Settings::new(
            None,
            skyelab_core::AccountsConfig::default(),
            None,
            window,
            cx,
        )
    });
    for section in crate::settings::Section::ALL {
        settings_view.update_in(settings_cx, |view, window, _| {
            view.section = section;
            window.refresh();
        });
        settings_cx.run_until_parked();
        let snaps = settings_cx.update(|window, _| snapshots(window));
        assert_all_controls_named(snaps, &format!("Settings::{section:?}"));
    }
}
