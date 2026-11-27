//! A linked machine that hasn't approved this device yet: its sidebar row's hover card and Settings → Machines show
//! this device's code, the one that machine's Sharing & access asks the user to compare, and the row opens Machines.

use super::env_harness::*;
use super::env_linked::ToggleLinkedEnvironment;
use super::*;
use crate::settings::Section;
use gpui::TestAppContext;
use skyelab_daemon::testing::NODE_NAME;
use skyelab_protocol::{device_code, device_key_hash, new_device_key};

#[gpui::test]
fn a_machine_waiting_for_approval_shows_this_devices_code(cx: &mut TestAppContext) {
    let remote = remote_side(FakeTailnet::default());
    // A key that machine never approved.
    let key = new_device_key().expect("key");
    let code = device_code(&device_key_hash(&key));
    let mut options = remote.options();
    options.device_key = Some(key.as_str().into());
    let (ws, _local, cx) = local_workspace(Arc::default(), None, options, cx);
    ws.update_in(cx, |_, window, cx| {
        let host = NODE_NAME.into();
        window.dispatch_action(Box::new(ToggleLinkedEnvironment { host }), cx)
    });
    wait_until(cx, "waiting for approval", |cx| {
        ws.read_with(cx, |ws, cx| {
            ws.linked_envs()
                .iter()
                .any(|e| matches!(e.read(cx).state, crate::environment::ConnState::NotApproved(_)))
        })
    });
    let id = format!("linked-state-{NODE_NAME}");
    let (note, opens) = ws.read_with(cx, |ws, cx| {
        let sidebar = ws.sidebar.read(cx);
        let item = sidebar.sections.iter().flat_map(|s| &s.items).find(|i| i.id == id);
        let note = sidebar
            .marks
            .get(id.as_str())
            .and_then(|m| m.card.as_ref())
            .and_then(|c| c.note.clone());
        let opens = item.and_then(|i| i.action.as_any().downcast_ref::<OpenSettingsSection>().cloned());
        (note.map(|n| n.to_string()), opens.map(|a| a.section))
    });
    let note = note.expect("the row's hover card");
    assert!(note.contains(&code), "{note}");
    assert!(note.contains("Settings → Sharing & access"), "{note}");
    assert_eq!(opens, Some(Section::Machines), "the row opens Settings → Machines");

    cx.dispatch_action(OpenSettingsSection {
        section: Section::Machines,
    });
    cx.run_until_parked();
    let page = ws.read_with(cx, |ws, cx| {
        ws.settings.as_ref().and_then(|s| s.read(cx).machines.clone()).expect("machines page")
    });
    assert_eq!(page.read_with(cx, |p, _| p.waiting.clone()), vec![(NODE_NAME.to_owned(), code.clone())]);
    let row: &'static str = Box::leak(format!("waiting-{NODE_NAME}").into_boxed_str());
    assert!(cx.debug_bounds(row).is_some(), "the code's row is drawn");
}
