//! A running emulator this process started.

use skyelab_core::process::Owned;
use std::io;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// A running emulator started by [`DeviceHub::boot`]. Dropping it leaves the emulator running, and a thread reaps
/// it when it exits; call [`Emulator::stop`] to end it.
#[derive(Debug)]
pub struct Emulator {
    /// `None` only inside `drop`, which hands the child to the reaper thread.
    pub(crate) child: Option<Owned>,
}

const TAKEN_IN_DROP: &str = "only drop takes the child, and nothing runs after drop";

impl Drop for Emulator {
    fn drop(&mut self) {
        if let Some(child) = self.child.take() {
            child.detach();
        }
    }
}

impl Emulator {
    pub fn pid(&self) -> u32 {
        self.child.as_ref().expect(TAKEN_IN_DROP).id()
    }

    /// Asks the emulator to quit (SIGTERM on Unix, so it shuts the AVD down cleanly), waits up to 30 s, then kills it.
    pub fn stop(&mut self) -> io::Result<()> {
        let child = self.child.as_mut().expect(TAKEN_IN_DROP);
        if cfg!(unix) && child.try_wait()?.is_none() {
            // std can only SIGKILL. An absolute path, so a planted `kill` on PATH can't skip the clean shutdown.
            let termed = skyelab_core::process::spawn(
                Command::new("/bin/kill")
                    .args(["-TERM", &child.id().to_string()])
                    .stderr(Stdio::null()),
            )
            .and_then(|mut c| c.wait())
            .is_ok_and(|s| s.success());
            // No `/bin/kill` (NixOS): don't wait 30 s for a signal that was never sent.
            let wait = if termed {
                Duration::from_secs(30)
            } else {
                Duration::ZERO
            };
            let deadline = Instant::now() + wait;
            while Instant::now() < deadline {
                if child.try_wait()?.is_some() {
                    return Ok(());
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        }
        child.kill()?;
        child.wait().map(drop)
    }
}
