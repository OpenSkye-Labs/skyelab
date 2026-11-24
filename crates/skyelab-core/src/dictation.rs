//! Composer dictation as plain state: where the dictated words go in the draft, and which recognizer events still
//! count. The recognizer itself (Apple's Speech framework) is `skyelab-speech-sys`; the composer glue is
//! `composer_voice.rs`.

use std::ops::Range;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Phase {
    #[default]
    Idle,
    /// The microphone is on and partial results stream into the draft.
    Listening,
    /// Stopped by the user: waiting for the recognizer's final result.
    Finishing,
    /// Why dictation stopped or couldn't start, shown until dismissed or started again.
    Failed(String),
}

/// The draft to show after an event, with the cursor (a byte offset) after the dictated words.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Edit {
    pub text: String,
    pub cursor: usize,
}

/// Why dictation couldn't start, worded for the composer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    MicDenied,
    SpeechDenied,
    /// Device management (or parental controls) forbid it.
    Restricted,
    /// No recognizer for the system language.
    NoRecognizer,
    /// A recognizer exists but can't run right now (no network for a server-only language).
    Unavailable,
    /// The binary has no usage strings: asking for permission would abort the process.
    NotPackaged,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::MicDenied => {
                "SkyeLab can't use the microphone. Allow it in System Settings → Privacy & Security → Microphone."
            }
            Self::SpeechDenied => {
                "SkyeLab can't use speech recognition. Allow it in System Settings → Privacy & Security → Speech Recognition."
            }
            Self::Restricted => "Speech recognition is restricted on this Mac.",
            Self::NoRecognizer => "Speech recognition doesn't support your Mac's language.",
            Self::Unavailable => "Speech recognition isn't available right now. Try again in a moment.",
            Self::NotPackaged => {
                "Voice input needs SkyeLab.app (this build has no microphone permission text)."
            }
        })
    }
}

#[derive(Debug, Default)]
pub struct Dictation {
    phase: Phase,
    /// Counts starts, so events from a cancelled or ended recognizer are told apart from the current one's.
    session: u64,
    before: String,
    after: String,
    original: Edit,
    heard: String,
    /// The draft as dictation last wrote it, so a write from elsewhere can be told apart.
    shown: String,
    /// The recognizer sends audio to Apple's servers (the Mac can't recognize this language on the device).
    server: bool,
}

/// The largest char boundary in `s` at or below `at`.
fn floor(s: &str, at: usize) -> usize {
    let mut at = at.min(s.len());
    while !s.is_char_boundary(at) {
        at -= 1;
    }
    at
}

impl Dictation {
    pub fn phase(&self) -> &Phase {
        &self.phase
    }

    /// Listening or finishing: the draft belongs to dictation.
    pub fn active(&self) -> bool {
        matches!(self.phase, Phase::Listening | Phase::Finishing)
    }

    pub fn shown(&self) -> &str {
        &self.shown
    }

    pub fn server(&self) -> bool {
        self.active() && self.server
    }

    /// Starts over `text` with `selection` (what the words replace; empty: the cursor). `None` while one runs.
    pub fn start(&mut self, text: &str, selection: Range<usize>) -> Option<u64> {
        if self.active() {
            return None;
        }
        let end = floor(text, selection.end);
        let start = floor(text, selection.start.min(end));
        self.session += 1;
        self.phase = Phase::Listening;
        self.before = text[..start].to_owned();
        self.after = text[end..].to_owned();
        self.original = Edit {
            text: text.to_owned(),
            cursor: end,
        };
        self.heard.clear();
        self.shown = text.to_owned();
        self.server = false;
        Some(self.session)
    }

    fn current(&self, session: u64) -> bool {
        session == self.session && self.active()
    }

    /// The recognizer is running; `on_device` false means audio goes to Apple.
    pub fn started(&mut self, session: u64, on_device: bool) {
        if self.current(session) {
            self.server = !on_device;
        }
    }

    /// The words heard so far (each partial revises the whole span); a final result ends dictation.
    pub fn heard(&mut self, session: u64, words: &str, is_final: bool) -> Option<Edit> {
        if !self.current(session) {
            return None;
        }
        self.heard = words.trim().to_owned();
        if is_final {
            self.phase = Phase::Idle;
        }
        let edit = self.edit();
        self.shown.clone_from(&edit.text);
        Some(edit)
    }

    fn edit(&self) -> Edit {
        if self.heard.is_empty() {
            return self.original.clone();
        }
        // Scripts written without spaces between words (CJK and up) never get one.
        let spaced = |c: char| !c.is_whitespace() && (c as u32) < 0x2E80;
        let words = |c: Option<char>| c.is_some_and(spaced);
        let mut text = self.before.clone();
        let prev = self.before.chars().next_back();
        if words(prev)
            && words(self.heard.chars().next())
            && !prev.is_some_and(|c| "([{\"'".contains(c))
        {
            text.push(' ');
        }
        text.push_str(&self.heard);
        let cursor = text.len();
        let next = self.after.chars().next();
        if words(next)
            && words(self.heard.chars().next_back())
            && !next.is_some_and(|c| c.is_ascii_punctuation())
        {
            text.push(' ');
        }
        text.push_str(&self.after);
        Edit { text, cursor }
    }

    /// The recognizer failed. After the user stopped with words heard, that is just the end (it reports "no speech"
    /// for the silence after them). Returns whether anything changed.
    pub fn failed(&mut self, session: u64, why: String) -> bool {
        if !self.current(session) {
            return false;
        }
        self.phase = if self.phase == Phase::Finishing && !self.heard.is_empty() {
            Phase::Idle
        } else {
            Phase::Failed(why)
        };
        true
    }

    /// The user pressed stop: wait for the final result.
    pub fn stop(&mut self) -> bool {
        let listening = self.phase == Phase::Listening;
        if listening {
            self.phase = Phase::Finishing;
        }
        listening
    }

    /// Discards what was dictated: the draft as it was at start.
    pub fn cancel(&mut self) -> Option<Edit> {
        if !self.active() {
            return None;
        }
        self.phase = Phase::Idle;
        Some(self.original.clone())
    }

    /// Ends now, keeping what is in the draft (a send); later events are dropped.
    pub fn end(&mut self) -> bool {
        let was = self.active();
        if was {
            self.phase = Phase::Idle;
        }
        was
    }

    pub fn dismiss(&mut self) {
        if matches!(self.phase, Phase::Failed(_)) {
            self.phase = Phase::Idle;
        }
    }
}
