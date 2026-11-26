//! Acceptance: dictation in the composer with a fake recognizer. The mic key starts and stops it, partial results
//! stream into the prompt at the cursor, Esc puts the draft back, a refused permission says so, a recognizer that sends
//! audio to Apple says that, and Send stops dictation and sends what is in the composer.

use super::composer_voice::{Listening, Recognizer, use_recognizer};
use super::thread::ThreadView;
use super::threads_test::{fixture, open_workspace, project_thread, setup, wait_until};
use crate::workspace::thread_extras::prefs;
use crate::workspace::thread_model::Item;
use gpui::{Entity, TestAppContext, VisualTestContext};
use gpui_component::WindowExt as _;
use skyelab_speech_sys::{Capability, Event, Refusal};
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

#[path = "composer_voice_consent_test.rs"]
mod consent;

const MIC_KEY: &str = "cmd-shift-m";

type Sink = Box<dyn Fn(Event) + Send + Sync>;

type Done = Box<dyn FnOnce(Result<(), Refusal>) + Send>;

#[derive(Default)]
struct Fake {
    sink: Arc<Mutex<Option<Sink>>>,
    /// Set: the permission answer waits here until the test gives it (a prompt still up).
    held: Option<Arc<Mutex<Option<Done>>>>,
    refuse: Option<Refusal>,
    server: bool,
    /// What the probe reports: the language, and (unset) on-device unless `server`.
    locale: Arc<Mutex<String>>,
    probe_on_device: Option<bool>,
    starts: Arc<AtomicUsize>,
    /// Running sessions: a start adds one, a drop takes it away.
    live: Arc<AtomicUsize>,
    stops: Arc<AtomicUsize>,
}

struct FakeSession {
    on_device: bool,
    live: Arc<AtomicUsize>,
    stops: Arc<AtomicUsize>,
}

impl Listening for FakeSession {
    fn on_device(&self) -> bool {
        self.on_device
    }
    fn stop(&self) {
        self.stops.fetch_add(1, Ordering::SeqCst);
    }
}

impl Drop for FakeSession {
    fn drop(&mut self) {
        self.live.fetch_sub(1, Ordering::SeqCst);
    }
}

impl Recognizer for Fake {
    fn authorize(&self, done: Done) {
        if let Some(held) = &self.held {
            *held.lock().expect("held") = Some(done);
            return;
        }
        let refuse = self.refuse;
        // From another thread, as Apple's callbacks come.
        std::thread::spawn(move || done(refuse.map_or(Ok(()), Err)));
    }
    fn probe(&self) -> Result<Capability, Refusal> {
        Ok(Capability {
            locale: self.locale.lock().expect("locale").clone(),
            on_device: self.probe_on_device.unwrap_or(!self.server),
        })
    }
    fn start(&self, sink: Sink) -> Result<Box<dyn Listening>, String> {
        self.starts.fetch_add(1, Ordering::SeqCst);
        *self.sink.lock().expect("sink") = Some(sink);
        self.live.fetch_add(1, Ordering::SeqCst);
        Ok(Box::new(FakeSession {
            on_device: !self.server,
            live: self.live.clone(),
            stops: self.stops.clone(),
        }))
    }
}

struct Handles {
    sink: Arc<Mutex<Option<Sink>>>,
    locale: Arc<Mutex<String>>,
    starts: Arc<AtomicUsize>,
    live: Arc<AtomicUsize>,
    stops: Arc<AtomicUsize>,
}

impl Handles {
    fn say(&self, words: &str, is_final: bool, cx: &mut VisualTestContext) {
        let sink = self.sink.lock().expect("sink");
        (sink.as_ref().expect("started"))(Event::Heard {
            words: words.into(),
            is_final,
        });
        drop(sink);
        cx.run_until_parked();
    }
    fn live(&self) -> usize {
        self.live.load(Ordering::SeqCst)
    }
}

fn install(fake: Fake, cx: &mut VisualTestContext) -> Handles {
    let h = Handles {
        sink: fake.sink.clone(),
        locale: fake.locale.clone(),
        starts: fake.starts.clone(),
        live: fake.live.clone(),
        stops: fake.stops.clone(),
    };
    cx.update(|_, cx| use_recognizer(Rc::new(fake), cx));
    h
}

fn prompt(thread: &Entity<ThreadView>, cx: &mut VisualTestContext) -> (String, usize) {
    thread.read_with(cx, |t, cx| {
        let p = t.prompt.read(cx);
        (p.value().to_string(), p.cursor())
    })
}

/// The prompt holds `text` with the cursor at `at`, focused.
fn type_at(thread: &Entity<ThreadView>, text: &str, at: usize, cx: &mut VisualTestContext) {
    thread.update_in(cx, |t, window, cx| {
        t.prompt.update(cx, |p, cx| {
            p.set_value(text, window, cx);
            p.set_selected_range(at..at, cx);
        });
        t.focus_prompt(window, cx);
    });
}

fn listening(thread: &Entity<ThreadView>, cx: &mut VisualTestContext) -> bool {
    thread.read_with(cx, |t, _| t.composer.dictation.active())
}

#[gpui::test]
fn partials_stream_in_at_the_cursor_and_esc_restores(cx: &mut TestAppContext) {
    setup(cx);
    let f = fixture(true);
    let (ws, cx) = open_workspace(&f, cx);
    let thread = project_thread(&f, &ws, cx);
    let h = install(Fake::default(), cx);

    type_at(&thread, "fix the bug", 4, cx);
    cx.simulate_keystrokes(MIC_KEY);
    wait_until(cx, "the recognizer started", |_| h.live() == 1);
    assert!(listening(&thread, cx), "MIC_KEY_DIDNT_START");
    cx.simulate_keystrokes("x");
    assert_eq!(
        prompt(&thread, cx).0,
        "fix the bug",
        "TYPED_WHILE_DICTATING"
    );
    h.say("only", false, cx);
    assert_eq!(
        prompt(&thread, cx),
        ("fix only the bug".into(), 8),
        "PARTIAL_NOT_AT_CURSOR"
    );
    h.say("only this", false, cx);
    assert_eq!(
        prompt(&thread, cx).0,
        "fix only this the bug",
        "PARTIAL_APPENDED"
    );

    // The mic key again stops; the final result commits and the microphone goes off.
    cx.simulate_keystrokes(MIC_KEY);
    assert_eq!(h.stops.load(Ordering::SeqCst), 1, "STOP_NOT_SENT");
    h.say("only this", true, cx);
    assert!(!listening(&thread, cx));
    assert_eq!(h.live(), 0, "MIC_LEFT_ON");
    assert_eq!(prompt(&thread, cx).0, "fix only this the bug");

    // Esc while listening puts the draft back as it was.
    cx.simulate_keystrokes(MIC_KEY);
    wait_until(cx, "started again", |_| h.live() == 1);
    h.say("throw away", false, cx);
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(!listening(&thread, cx), "ESC_DIDNT_CANCEL");
    assert_eq!(
        prompt(&thread, cx).0,
        "fix only this the bug",
        "ESC_KEPT_WORDS"
    );
    assert_eq!(h.live(), 0);
    // A late result from the cancelled run changes nothing.
    h.say("late", true, cx);
    assert_eq!(
        prompt(&thread, cx).0,
        "fix only this the bug",
        "LATE_EVENT_APPLIED"
    );
}

#[gpui::test]
fn a_refused_permission_says_what_to_do(cx: &mut TestAppContext) {
    setup(cx);
    let f = fixture(true);
    let (ws, cx) = open_workspace(&f, cx);
    let thread = project_thread(&f, &ws, cx);
    let h = install(
        Fake {
            refuse: Some(Refusal::MicDenied),
            ..Fake::default()
        },
        cx,
    );
    type_at(&thread, "draft", 5, cx);
    cx.simulate_keystrokes(MIC_KEY);
    wait_until(cx, "the refusal", |cx| {
        thread.read_with(cx, |t, _| t.composer.notice.is_some())
    });
    let notice = thread.read_with(cx, |t, _| t.composer.notice.clone().expect("notice"));
    assert!(
        notice.contains("Privacy & Security → Microphone"),
        "{notice}"
    );
    assert!(!listening(&thread, cx));
    assert_eq!(h.live(), 0, "STARTED_DESPITE_REFUSAL");
    cx.simulate_keystrokes("!");
    assert_eq!(prompt(&thread, cx).0, "draft!", "PROMPT_LEFT_READ_ONLY");
}

#[gpui::test]
fn a_server_recognizer_says_audio_goes_to_apple(cx: &mut TestAppContext) {
    setup(cx);
    let f = fixture(true);
    let (ws, cx) = open_workspace(&f, cx);
    let thread = project_thread(&f, &ws, cx);
    let h = install(
        Fake {
            server: true,
            ..Fake::default()
        },
        cx,
    );
    *h.locale.lock().expect("locale") = "fr-FR".into();
    cx.update(|_, cx| prefs::allow_server_dictation("fr_FR".into(), cx))
        .detach();
    cx.run_until_parked();
    type_at(&thread, "", 0, cx);
    cx.simulate_keystrokes(MIC_KEY);
    wait_until(cx, "started", |_| h.live() == 1);
    cx.run_until_parked();
    assert!(
        cx.debug_bounds("voice-server-notice").is_some(),
        "SERVER_NOTICE_MISSING"
    );
    assert!(cx.debug_bounds("voice-pill").is_some());
}

#[cfg(unix)] // the fake agent is a shell script
#[gpui::test]
fn send_stops_dictation_and_sends_the_words(cx: &mut TestAppContext) {
    setup(cx);
    let f = fixture(true);
    let (ws, cx) = open_workspace(&f, cx);
    let thread = project_thread(&f, &ws, cx);
    let h = install(Fake::default(), cx);
    type_at(&thread, "", 0, cx);
    cx.simulate_keystrokes(MIC_KEY);
    wait_until(cx, "started", |_| h.live() == 1);
    h.say("hello there", false, cx);
    cx.simulate_keystrokes("cmd-enter");
    cx.run_until_parked();
    assert!(!listening(&thread, cx), "SEND_DIDNT_STOP");
    assert_eq!(h.live(), 0, "MIC_LEFT_ON_AFTER_SEND");
    wait_until(cx, "the prompt", |cx| {
        thread.read_with(cx, |t, _| {
            t.items
                .iter()
                .any(|i| matches!(i, Item::Prompt(p) if p == "hello there"))
        })
    });
    // The recognizer's final after the send lands nowhere.
    h.say("hello there friend", true, cx);
    assert_eq!(prompt(&thread, cx).0, "", "FINAL_AFTER_SEND_APPLIED");
}

#[gpui::test]
fn a_prompt_written_elsewhere_ends_dictation_without_losing_it(cx: &mut TestAppContext) {
    setup(cx);
    let f = fixture(true);
    let (ws, cx) = open_workspace(&f, cx);
    let thread = project_thread(&f, &ws, cx);
    let h = install(Fake::default(), cx);
    type_at(&thread, "", 0, cx);
    cx.simulate_keystrokes(MIC_KEY);
    wait_until(cx, "started", |_| h.live() == 1);
    h.say("first", false, cx);
    // A stash restore (or Paste as text) writes the prompt meanwhile.
    thread.update_in(cx, |t, window, cx| {
        t.prompt.update(cx, |p, cx| {
            p.set_value("restored from the stash", window, cx)
        });
    });
    h.say("first second", false, cx);
    assert_eq!(
        prompt(&thread, cx).0,
        "restored from the stash",
        "OUTSIDE_WRITE_OVERWRITTEN"
    );
    assert!(!listening(&thread, cx));
    assert_eq!(h.live(), 0, "MIC_ON_AFTER_OUTSIDE_WRITE");
}

#[gpui::test]
fn leaving_the_tab_turns_the_mic_off(cx: &mut TestAppContext) {
    setup(cx);
    let f = fixture(true);
    let (ws, cx) = open_workspace(&f, cx);
    let thread = project_thread(&f, &ws, cx);
    let h = install(Fake::default(), cx);
    cx.update(|window, _| window.activate_window()); // focus-out fires only in the active window
    type_at(&thread, "keep", 4, cx);
    cx.simulate_keystrokes(MIC_KEY);
    wait_until(cx, "started", |_| h.live() == 1);
    h.say("this", false, cx);
    // Clicking the pill's check mark is inside the thread: it finishes, the focus-out hook doesn't end it.
    let pill = cx.debug_bounds("voice-pill").expect("pill drawn");
    let check = gpui::point(pill.right() - gpui::px(14.), pill.center().y);
    cx.simulate_mouse_down(check, gpui::MouseButton::Left, gpui::Modifiers::none());
    cx.simulate_mouse_up(check, gpui::MouseButton::Left, gpui::Modifiers::none());
    cx.run_until_parked();
    assert_eq!(
        h.stops.load(Ordering::SeqCst),
        1,
        "CHECK_CLICK_DIDNT_FINISH"
    );
    assert!(listening(&thread, cx), "CLICK_IN_THREAD_ENDED_IT");
    cx.dispatch_action(crate::workspace::NewThread);
    cx.run_until_parked();
    assert_eq!(h.live(), 0, "MIC_ON_IN_HIDDEN_TAB");
    assert!(!listening(&thread, cx));
    assert_eq!(prompt(&thread, cx).0, "keep this", "words heard are kept");
}

#[gpui::test]
fn stop_during_the_permission_prompt_starts_nothing(cx: &mut TestAppContext) {
    setup(cx);
    let f = fixture(true);
    let (ws, cx) = open_workspace(&f, cx);
    let thread = project_thread(&f, &ws, cx);
    let held = Arc::new(Mutex::new(None));
    let h = install(
        Fake {
            held: Some(held.clone()),
            ..Fake::default()
        },
        cx,
    );
    type_at(&thread, "draft", 5, cx);
    cx.simulate_keystrokes(MIC_KEY);
    cx.run_until_parked();
    assert!(
        cx.debug_bounds("voice-pill").is_some(),
        "the pill shows while asking"
    );
    cx.simulate_keystrokes(MIC_KEY);
    let done = held.lock().expect("held").take().expect("asked");
    done(Ok(()));
    cx.run_until_parked();
    assert_eq!(h.live(), 0, "MIC_STARTED_AFTER_STOP");
    assert!(!listening(&thread, cx));
    assert!(
        thread.read_with(cx, |t, _| t.composer.notice.is_none()),
        "NOTICE_AFTER_PLAIN_STOP"
    );
    assert_eq!(prompt(&thread, cx).0, "draft");
}
