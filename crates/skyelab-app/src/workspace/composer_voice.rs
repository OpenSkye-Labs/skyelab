//! Dictation in the composer (macOS), T3's microphone and recording pill (T3's docs/user/voice-input.md): the mic button
//! or ⌘⇧M starts it, partial results stream into the prompt at the cursor, ⌘⇧M or the check mark finishes, Esc or the
//! cross puts the draft back, and Send ends it and sends what is there. The words come from Apple's Speech framework
//! (`skyelab-speech-sys`); where they go is `skyelab_core::dictation`.

use super::thread::{self, ThreadView};
use crate::ui;
use crate::ui::ActiveTheme as _;
use crate::ui::Button;
use crate::ui::SkyeIcon;
use futures::StreamExt as _;
use futures::channel::mpsc::UnboundedSender;
use gpui::{
    AnyElement, App, Context, Global, InteractiveElement as _, KeyBinding, Task, Window, actions,
    div, prelude::*, px,
};
use gpui_component::{Icon, h_flex};
use skyelab_core::dictation::{Edit, Phase};
use skyelab_speech_sys::{Capability, Event, Refusal};
use std::rc::Rc;

#[path = "composer_voice_consent.rs"]
mod consent;

actions!(skyelab, [ToggleDictation, CancelDictation]);

/// Set on the prompt's wrapper while dictating, so Esc reaches [`CancelDictation`] before the text field.
pub(in crate::workspace) const VOICE_CONTEXT: &str = "ComposerDictation";
/// T3 uses mod+shift+d; here ⌘⇧D opens Devices.
const MIC_KEY: &str = "cmd-shift-m";

pub(in crate::workspace) fn bind_keys(cx: &mut App) {
    if skyelab_speech_sys::SUPPORTED {
        cx.bind_keys([
            KeyBinding::new(MIC_KEY, ToggleDictation, Some(thread::CONTEXT)),
            KeyBinding::new("escape", CancelDictation, Some("ComposerDictation > Input")),
            // A read-only text field drops ⌘↩ instead of reporting it, so Send is bound here while dictating.
            KeyBinding::new(
                "cmd-enter",
                thread::SendPrompt,
                Some("ComposerDictation > Input"),
            ),
        ]);
    }
}

/// A running recognizer; dropping it turns the microphone off.
pub(in crate::workspace) trait Listening {
    /// Audio stays on the Mac (false: Apple's servers recognize it).
    fn on_device(&self) -> bool;
    /// Stop listening; the final result follows.
    fn stop(&self);
}

/// Where the words come from: Apple's recognizer, or a test's fake.
pub(in crate::workspace) trait Recognizer {
    /// Checks (asking the first time) the permissions; `done` may run on any thread.
    fn authorize(&self, done: Box<dyn FnOnce(Result<(), Refusal>) + Send>);
    /// The language and whether it is recognized on the Mac, read without turning the microphone on.
    fn probe(&self) -> Result<Capability, Refusal>;
    fn start(&self, sink: Box<dyn Fn(Event) + Send + Sync>) -> Result<Box<dyn Listening>, String>;
}

struct Apple;

impl Recognizer for Apple {
    fn authorize(&self, done: Box<dyn FnOnce(Result<(), Refusal>) + Send>) {
        skyelab_speech_sys::authorize(done)
    }
    fn probe(&self) -> Result<Capability, Refusal> {
        skyelab_speech_sys::probe()
    }
    fn start(&self, sink: Box<dyn Fn(Event) + Send + Sync>) -> Result<Box<dyn Listening>, String> {
        Ok(Box::new(skyelab_speech_sys::start(sink)?))
    }
}

impl Listening for skyelab_speech_sys::Session {
    fn on_device(&self) -> bool {
        skyelab_speech_sys::Session::on_device(self)
    }
    fn stop(&self) {
        skyelab_speech_sys::Session::stop(self)
    }
}

struct Voice(Rc<dyn Recognizer>);
impl Global for Voice {}

/// Replaces Apple's recognizer (tests).
#[cfg(all(test, target_os = "macos"))]
pub(in crate::workspace) fn use_recognizer(r: Rc<dyn Recognizer>, cx: &mut App) {
    cx.set_global(Voice(r));
}

fn recognizer(cx: &App) -> Rc<dyn Recognizer> {
    match cx.try_global::<Voice>() {
        Some(v) => v.0.clone(),
        None => Rc::new(Apple),
    }
}

enum Msg {
    Allowed(Result<(), Refusal>),
    Event(Event),
}

/// The recognizer and the task applying its events. Both go when dictation ends; their channel goes with them, so a
/// late callback from Apple lands nowhere.
#[derive(Default)]
pub(crate) struct VoiceRun {
    session: Option<Box<dyn Listening>>,
    _events: Option<Task<()>>,
    /// Ends dictation when the focus leaves the thread (another tab, the sidebar): no hidden composer keeps listening.
    _focus_out: Option<gpui::Subscription>,
}

impl ThreadView {
    pub(in crate::workspace) fn toggle_dictation(
        &mut self,
        _: &ToggleDictation,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.composer.dictation.active() {
            self.stop_dictation(cx);
        } else {
            self.start_dictation(window, cx);
        }
    }

    pub(in crate::workspace) fn start_dictation(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !skyelab_speech_sys::SUPPORTED && cx.try_global::<Voice>().is_none() {
            return;
        }
        if self.read_only || !self.owned_by(window) {
            return;
        }
        let (text, selection) = {
            let p = self.prompt.read(cx);
            (p.value().to_string(), p.selected_range())
        };
        let Some(id) = self.composer.dictation.start(&text, selection) else {
            return;
        };
        self.composer.notice = None;
        self.composer.menu = None;
        self.composer.stash_open = None; // its keys would lose to Esc's dictation binding
        let (tx, mut rx) = futures::channel::mpsc::unbounded();
        let allowed = tx.clone();
        recognizer(cx).authorize(Box::new(move |r| {
            let _ = allowed.unbounded_send(Msg::Allowed(r));
        }));
        let events = cx.spawn_in(window, async move |this, cx| {
            while let Some(msg) = rx.next().await {
                let applied = this.update_in(cx, |this, window, cx| {
                    this.voice_message(id, msg, &tx, window, cx)
                });
                if applied.is_err() {
                    // The window closed: end here, or the microphone would stay on with no one to stop it.
                    let _ = this.update(cx, |this, cx| this.end_dictation(cx));
                    break;
                }
            }
        });
        // Going inactive (another app) is reported as a focus-out too: dictating while reading elsewhere goes on.
        let focus_out = cx.on_focus_out(&self.focus, window, |this, _, window, cx| {
            if window.is_window_active() {
                this.end_dictation(cx);
            }
        });
        self.composer.voice = VoiceRun {
            session: None,
            _events: Some(events),
            _focus_out: Some(focus_out),
        };
        cx.notify();
    }

    fn voice_message(
        &mut self,
        id: u64,
        msg: Msg,
        tx: &UnboundedSender<Msg>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match msg {
            Msg::Allowed(Err(refusal)) => self.voice_failed(id, refusal.to_string(), cx),
            Msg::Allowed(Ok(())) => {
                // Sent or cancelled while the permission prompt was up.
                if !self.composer.dictation.active() || self.composer.voice.session.is_some() {
                    return;
                }
                // Stopped meanwhile: nothing was heard, so end without turning the microphone on.
                if self.composer.dictation.phase() == &Phase::Finishing {
                    return self.end_dictation(cx);
                }
                let recognizer = recognizer(cx);
                let cap = match recognizer.probe() {
                    Ok(cap) => cap,
                    Err(refusal) => return self.voice_failed(id, refusal.to_string(), cx),
                };
                let allowed = |cx: &App| {
                    crate::workspace::thread_extras::prefs::allows_server_dictation(&cap.locale, cx)
                };
                // Audio for a language the Mac can't recognize goes to Apple: ask first, with the microphone off.
                if !cap.on_device && !allowed(cx) {
                    self.end_dictation(cx);
                    return consent::ask(cap.locale, window, cx);
                }
                let tx = tx.clone();
                let sink = Box::new(move |e| {
                    let _ = tx.unbounded_send(Msg::Event(e));
                });
                match recognizer.start(sink) {
                    // The probe said on-device but this session isn't, and nobody allowed that: drop it at once and ask.
                    Ok(session) if !session.on_device() && !allowed(cx) => {
                        drop(session);
                        self.end_dictation(cx);
                        consent::ask(cap.locale, window, cx)
                    }
                    Ok(session) => {
                        self.composer.dictation.started(id, session.on_device());
                        self.composer.voice.session = Some(session);
                        cx.notify();
                    }
                    Err(why) => self.voice_failed(id, why, cx),
                }
            }
            Msg::Event(Event::Heard { words, is_final }) => {
                if self.written_elsewhere(cx) {
                    return self.end_dictation(cx);
                }
                if let Some(edit) = self.composer.dictation.heard(id, &words, is_final) {
                    self.show_dictated(edit, window, cx);
                    if is_final {
                        self.voice_done(cx);
                    }
                }
            }
            Msg::Event(Event::Failed(why)) => self.voice_failed(id, why, cx),
        }
    }

    /// Something else wrote the prompt (a stash restore, Paste as text): dictation must not overwrite it.
    fn written_elsewhere(&self, cx: &App) -> bool {
        *self.prompt.read(cx).value() != *self.composer.dictation.shown()
    }

    fn show_dictated(&mut self, edit: Edit, window: &mut Window, cx: &mut Context<Self>) {
        self.prompt.update(cx, |p, cx| {
            p.set_value(edit.text, window, cx);
            p.set_selected_range(edit.cursor..edit.cursor, cx);
        });
        self.sync_chips(cx);
        cx.notify(); // the draft saves on notify; Send's look follows the text
    }

    fn voice_failed(&mut self, id: u64, why: String, cx: &mut Context<Self>) {
        if !self.composer.dictation.failed(id, why) {
            return;
        }
        if let Phase::Failed(why) = self.composer.dictation.phase() {
            self.composer.notice = Some(why.clone().into());
        }
        self.composer.dictation.dismiss();
        self.voice_done(cx);
    }

    /// Dictation ended: the microphone goes off (the prompt's read-only follows `active()` in render).
    fn voice_done(&mut self, cx: &mut Context<Self>) {
        self.composer.voice = VoiceRun::default();
        cx.notify();
    }

    fn stop_dictation(&mut self, cx: &mut Context<Self>) {
        if !self.composer.dictation.stop() {
            return;
        }
        // Without a session it is still asking for permission: the answer sees Finishing and ends it.
        if let Some(s) = &self.composer.voice.session {
            s.stop();
        }
        cx.notify();
    }

    pub(in crate::workspace) fn cancel_dictation(
        &mut self,
        _: &CancelDictation,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.composer.dictation.active() && self.written_elsewhere(cx) {
            return self.end_dictation(cx); // putting the old draft back would undo that write
        }
        match self.composer.dictation.cancel() {
            Some(edit) => {
                self.show_dictated(edit, window, cx);
                self.voice_done(cx);
            }
            None => cx.propagate(),
        }
    }

    /// A send ends dictation where it is: the words already in the prompt go, later results don't.
    pub(in crate::workspace) fn end_dictation(&mut self, cx: &mut Context<Self>) {
        if self.composer.dictation.end() {
            self.voice_done(cx);
        }
    }

    /// The mic button, or while dictating T3's pill: cancel, a red dot and the state, and finish.
    pub(in crate::workspace) fn render_voice(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        // While dictating the pill stays, even if the thread turned read-only meanwhile, so it can still be stopped.
        let dictating = self.composer.dictation.active();
        if !skyelab_speech_sys::SUPPORTED && cx.try_global::<Voice>().is_none()
            || self.read_only && !dictating
        {
            return None;
        }
        let dictation = &self.composer.dictation;
        if !dictation.active() {
            return Some(
                Button::new("voice-start")
                    .ghost()
                    .small()
                    .icon(Icon::new(SkyeIcon::Mic))
                    .tooltip_with_action(
                        "Start voice input",
                        &ToggleDictation,
                        Some(thread::CONTEXT),
                    )
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.toggle_dictation(&ToggleDictation, window, cx)
                    }))
                    .into_any_element(),
            );
        }
        let theme = cx.theme();
        let finishing = dictation.phase() == &Phase::Finishing;
        let status = if finishing {
            "Finishing…"
        } else if self.composer.voice.session.is_none() {
            "Starting…"
        } else {
            "Listening"
        };
        let server = dictation.server().then(|| {
            div()
                .id("voice-server")
                .debug_selector(|| "voice-server-notice".into())
                .text_color(ui::ink(theme, theme.warning))
                .child("Sent to Apple")
                .tooltip(|window, cx| {
                    crate::ui::Tooltip::new(
                        "This Mac can't recognize your language on the device, so macOS sends the audio to Apple.",
                    )
                    .build(window, cx)
                })
        });
        Some(
            h_flex()
                .id("voice-pill")
                .debug_selector(|| "voice-pill".into())
                .h(px(28.))
                .pr_0p5()
                .gap_1p5()
                .rounded_full()
                .border_1()
                .border_color(theme.border)
                .bg(theme.muted)
                .text_size(ui::META)
                .child(
                    Button::new("voice-cancel")
                        .ghost()
                        .xsmall()
                        .icon(Icon::new(SkyeIcon::X))
                        .tooltip_with_action(
                            "Cancel voice input",
                            &CancelDictation,
                            Some(VOICE_CONTEXT),
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.cancel_dictation(&CancelDictation, window, cx)
                        })),
                )
                .child(
                    div()
                        .size(px(8.))
                        .flex_shrink_0()
                        .rounded_full()
                        .bg(if finishing {
                            theme.muted_foreground.opacity(0.4)
                        } else {
                            theme.danger
                        }),
                )
                .child(div().text_color(theme.muted_foreground).child(status))
                .children(server)
                .child(
                    Button::new("voice-finish")
                        .primary()
                        .xsmall()
                        .rounded(ui::RADIUS_LG)
                        .icon(Icon::new(SkyeIcon::Check))
                        .disabled(finishing)
                        .tooltip_with_action(
                            "Finish voice input",
                            &ToggleDictation,
                            Some(thread::CONTEXT),
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.toggle_dictation(&ToggleDictation, window, cx)
                        })),
                )
                .into_any_element(),
        )
    }
}
