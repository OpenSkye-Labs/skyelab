//! Composer dictation on macOS: the microphone through `AVAudioEngine` into Apple's `SFSpeechRecognizer`, on the
//! device whenever the Mac can recognize its language there. Elsewhere [`SUPPORTED`] is false and nothing here runs.
//!
//! Callbacks arrive on queues of Apple's choosing, so results go to a `Send` sink; the [`Session`] itself (engine,
//! request, task) stays on the thread that started it.
#![allow(unsafe_code)]

pub use skyelab_core::dictation::Refusal;

/// What a running recognizer reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Heard { words: String, is_final: bool },
    Failed(String),
}

/// Whether this platform has dictation.
pub const SUPPORTED: bool = cfg!(target_os = "macos");

#[cfg(target_os = "macos")]
pub use mac::{Session, authorize, start};

/// No dictation off macOS: the composer hides its button, so these are never called.
#[cfg(not(target_os = "macos"))]
pub struct Session;

#[cfg(not(target_os = "macos"))]
impl Session {
    pub fn on_device(&self) -> bool {
        true
    }
    pub fn stop(&self) {}
}

#[cfg(not(target_os = "macos"))]
pub fn authorize(done: impl FnOnce(Result<(), Refusal>) + Send + 'static) {
    done(Err(Refusal::NoRecognizer))
}

#[cfg(not(target_os = "macos"))]
pub fn start(_sink: impl Fn(Event) + Send + Sync + 'static) -> Result<Session, String> {
    Err(Refusal::NoRecognizer.to_string())
}

#[cfg(target_os = "macos")]
mod mac {
    use super::{Event, Refusal};
    use block2::RcBlock;
    use objc2::AnyThread as _;
    use objc2::rc::Retained;
    use objc2::runtime::Bool;
    use objc2_avf_audio::{
        AVAudioApplication, AVAudioApplicationRecordPermission, AVAudioEngine, AVAudioPCMBuffer,
        AVAudioTime,
    };
    use objc2_foundation::{NSBundle, NSError, NSString};
    use objc2_speech::{
        SFSpeechAudioBufferRecognitionRequest, SFSpeechRecognitionResult, SFSpeechRecognitionTask,
        SFSpeechRecognizer, SFSpeechRecognizerAuthorizationStatus,
    };
    use std::ptr::NonNull;
    use std::sync::Mutex;

    /// A one-shot callback callable from an Objective-C block (an `Fn`) on any thread.
    fn once<T>(f: impl FnOnce(T) + Send + 'static) -> impl Fn(T) + Send + Sync + 'static {
        let f = Mutex::new(Some(f));
        move |v| {
            if let Some(f) = f.lock().ok().and_then(|mut f| f.take()) {
                f(v)
            }
        }
    }

    /// Both usage strings are in the running bundle's Info.plist (or the binary's embedded one). Without them, asking
    /// for either permission aborts the process.
    fn packaged() -> bool {
        let bundle = NSBundle::mainBundle();
        [
            "NSMicrophoneUsageDescription",
            "NSSpeechRecognitionUsageDescription",
        ]
        .iter()
        .all(|k| {
            bundle
                .objectForInfoDictionaryKey(&NSString::from_str(k))
                .is_some()
        })
    }

    /// Checks (asking the first time) speech recognition, then the microphone. `done` may run on any thread.
    pub fn authorize(done: impl FnOnce(Result<(), Refusal>) + Send + 'static) {
        if !packaged() {
            return done(Err(Refusal::NotPackaged));
        }
        speech(move |r| match r {
            Ok(()) => microphone(done),
            Err(e) => done(Err(e)),
        });
    }

    fn speech(next: impl FnOnce(Result<(), Refusal>) + Send + 'static) {
        fn verdict(s: SFSpeechRecognizerAuthorizationStatus) -> Result<(), Refusal> {
            match s {
                SFSpeechRecognizerAuthorizationStatus::Authorized => Ok(()),
                SFSpeechRecognizerAuthorizationStatus::Restricted => Err(Refusal::Restricted),
                _ => Err(Refusal::SpeechDenied),
            }
        }
        // SAFETY: a class-method read with no preconditions.
        let status = unsafe { SFSpeechRecognizer::authorizationStatus() };
        if status != SFSpeechRecognizerAuthorizationStatus::NotDetermined {
            return next(verdict(status));
        }
        let next = once(next);
        let block = RcBlock::new(move |s| next(verdict(s)));
        // SAFETY: `packaged()` checked the usage string, whose absence is this call's only abort; the block is
        // copied by the callee and its captures are Send + Sync, so running it on another queue is sound.
        unsafe { SFSpeechRecognizer::requestAuthorization(&block) };
    }

    fn microphone(next: impl FnOnce(Result<(), Refusal>) + Send + 'static) {
        // SAFETY: sharedInstance and recordPermission are plain reads (AVAudioApplication is macOS 14+, our minimum).
        let app = unsafe { AVAudioApplication::sharedInstance() };
        // SAFETY: as above.
        let status = unsafe { app.recordPermission() };
        if status == AVAudioApplicationRecordPermission::Granted {
            return next(Ok(()));
        }
        if status == AVAudioApplicationRecordPermission::Denied {
            return next(Err(Refusal::MicDenied));
        }
        let next = once(next);
        let block = RcBlock::new(move |granted: Bool| {
            next(if granted.as_bool() {
                Ok(())
            } else {
                Err(Refusal::MicDenied)
            })
        });
        // SAFETY: the usage string is present (`packaged()`); the block is copied by the callee and Send + Sync.
        unsafe { AVAudioApplication::requestRecordPermissionWithCompletionHandler(&block) };
    }

    /// A running dictation. Dropping it cancels the recognition and turns the microphone off.
    pub struct Session {
        // Kept for the task's life, as Apple's sample does: a released recognizer may stop delivering results.
        _recognizer: Retained<SFSpeechRecognizer>,
        engine: Retained<AVAudioEngine>,
        request: Retained<SFSpeechAudioBufferRecognitionRequest>,
        task: Retained<SFSpeechRecognitionTask>,
        on_device: bool,
    }

    impl Session {
        /// Audio stays on the Mac (false: Apple's servers recognize it).
        pub fn on_device(&self) -> bool {
            self.on_device
        }

        /// Stops listening; the recognizer then sends its final result.
        pub fn stop(&self) {
            self.mic_off();
            // SAFETY: ending the audio of a request we own; calling it twice is a no-op.
            unsafe { self.request.endAudio() };
        }

        fn mic_off(&self) {
            // SAFETY: stopping an engine we own and removing the tap we installed on bus 0 are both no-ops when done.
            unsafe {
                self.engine.stop();
                self.engine.inputNode().removeTapOnBus(0);
            }
        }
    }

    impl Drop for Session {
        fn drop(&mut self) {
            self.mic_off();
            // SAFETY: cancelling a task we own; its handler's last (cancelled) call is dropped by the caller's session check.
            unsafe { self.task.cancel() };
        }
    }

    /// Starts listening (after [`authorize`] said yes). Results go to `sink` on Apple's queue.
    pub fn start(sink: impl Fn(Event) + Send + Sync + 'static) -> Result<Session, String> {
        // SAFETY: init with the user's default language; nil (no recognizer for it) is handled.
        let Some(recognizer) = (unsafe { SFSpeechRecognizer::init(SFSpeechRecognizer::alloc()) })
        else {
            return Err(Refusal::NoRecognizer.to_string());
        };
        // SAFETY: a property read.
        if !unsafe { recognizer.isAvailable() } {
            return Err(Refusal::Unavailable.to_string());
        }
        // SAFETY: creating and configuring a request we own, before any task uses it.
        let (request, on_device) = unsafe {
            let request = SFSpeechAudioBufferRecognitionRequest::new();
            request.setShouldReportPartialResults(true);
            request.setAddsPunctuation(true);
            let on_device = recognizer.supportsOnDeviceRecognition();
            request.setRequiresOnDeviceRecognition(on_device);
            (request, on_device)
        };
        // SAFETY: creating an engine and reading its input node and that node's format.
        let (engine, input, format) = unsafe {
            let engine = AVAudioEngine::new();
            let input = engine.inputNode();
            let format = input.outputFormatForBus(0);
            (engine, input, format)
        };
        // SAFETY: a property read on a format we hold.
        if unsafe { format.sampleRate() } <= 0.0 || unsafe { format.channelCount() } == 0 {
            return Err("No microphone found.".into());
        }
        let feed = request.clone();
        let tap = RcBlock::new(
            move |buffer: NonNull<AVAudioPCMBuffer>, _: NonNull<AVAudioTime>| {
                // SAFETY: the engine passes a valid buffer for the call's duration; appending from the audio thread is
                // what Apple's recognizer is built for (its own sample does it).
                unsafe { feed.appendAudioPCMBuffer(buffer.as_ref()) }
            },
        );
        // SAFETY: bus 0 exists on the input node, the format is its own output format, and the node copies the block,
        // which captures only the request (kept alive by that copy).
        unsafe {
            input.installTapOnBus_bufferSize_format_block(
                0,
                1024,
                Some(&format),
                RcBlock::as_ptr(&tap),
            )
        };
        let handler = RcBlock::new(
            move |result: *mut SFSpeechRecognitionResult, error: *mut NSError| {
                // SAFETY: each pointer is nil or a valid object for the duration of the call.
                let (result, error) = unsafe { (result.as_ref(), error.as_ref()) };
                if let Some(result) = result {
                    // SAFETY: property reads on a valid result.
                    let (words, is_final) = unsafe {
                        (
                            result.bestTranscription().formattedString().to_string(),
                            result.isFinal(),
                        )
                    };
                    sink(Event::Heard { words, is_final });
                } else if let Some(error) = error {
                    sink(Event::Failed(error.localizedDescription().to_string()));
                }
            },
        );
        // SAFETY: the recognizer copies the handler, which captures only the Send + Sync sink.
        let task =
            unsafe { recognizer.recognitionTaskWithRequest_resultHandler(&request, &handler) };
        // SAFETY: preparing and starting an engine we own, with the tap installed above.
        let started = unsafe {
            engine.prepare();
            engine.startAndReturnError()
        };
        let session = Session {
            _recognizer: recognizer,
            engine,
            request,
            task,
            on_device,
        };
        if let Err(e) = started {
            drop(session);
            return Err(format!(
                "The microphone didn't start: {}",
                e.localizedDescription()
            ));
        }
        Ok(session)
    }
}
