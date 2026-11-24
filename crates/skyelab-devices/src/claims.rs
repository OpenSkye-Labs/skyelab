//! Who drives a device: one agent thread at a time, or the user, who can always take it back.

use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Holder {
    Thread(i64),
    /// The user took it back: no agent may claim it until they give it back.
    User,
}

/// Device id -> holder. A device with no entry is free.
#[derive(Debug, Default, Clone)]
pub struct Claims {
    held: HashMap<String, Holder>,
}

impl Claims {
    pub fn holder(&self, device: &str) -> Option<Holder> {
        self.held.get(device).copied()
    }

    /// Claims `device` for `thread`. Claiming one it already holds is fine.
    pub fn claim(&mut self, device: &str, thread: i64) -> Result<(), String> {
        match self.holder(device) {
            None => {
                self.held.insert(device.to_owned(), Holder::Thread(thread));
                Ok(())
            }
            Some(Holder::Thread(t)) if t == thread => Ok(()),
            Some(Holder::Thread(_)) => Err(format!("device {device} is in use by another thread")),
            Some(Holder::User) => Err(format!(
                "the user took device {device} back; ask them before using it"
            )),
        }
    }

    /// Ok when `thread` holds `device`.
    pub fn check(&self, device: &str, thread: i64) -> Result<(), String> {
        match self.holder(device) {
            Some(Holder::Thread(t)) if t == thread => Ok(()),
            Some(Holder::User) => Err(format!("the user took device {device} back")),
            _ => Err(format!(
                "you have not opened device {device}; call device_open first"
            )),
        }
    }

    /// Releases `device` if `thread` holds it. Returns whether it did.
    pub fn release(&mut self, device: &str, thread: i64) -> bool {
        let held = self.holder(device) == Some(Holder::Thread(thread));
        if held {
            self.held.remove(device);
        }
        held
    }

    /// Frees every device `thread` holds (its thread closed).
    pub fn release_thread(&mut self, thread: i64) -> bool {
        let before = self.held.len();
        self.held.retain(|_, h| *h != Holder::Thread(thread));
        self.held.len() != before
    }

    pub fn take_back(&mut self, device: &str) {
        self.held.insert(device.to_owned(), Holder::User);
    }

    /// Lets agents claim a device the user took back.
    pub fn give_back(&mut self, device: &str) {
        if self.holder(device) == Some(Holder::User) {
            self.held.remove(device);
        }
    }

    /// Every device a thread holds, with its thread.
    pub fn threads(&self) -> impl Iterator<Item = (&str, i64)> {
        self.held.iter().filter_map(|(d, h)| match h {
            Holder::Thread(t) => Some((d.as_str(), *t)),
            Holder::User => None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_thread_per_device() {
        let mut c = Claims::default();
        c.claim("d", 1).unwrap();
        c.claim("d", 1).unwrap();
        let err = c.claim("d", 2).unwrap_err();
        assert!(err.contains("another thread"), "{err}");
        c.check("d", 1).unwrap();
        assert!(c.check("d", 2).unwrap_err().contains("device_open"));
        c.claim("e", 2).unwrap();
        assert_eq!(c.threads().count(), 2);
    }

    #[test]
    fn take_back_blocks_agents_until_given_back() {
        let mut c = Claims::default();
        c.claim("d", 1).unwrap();
        c.take_back("d");
        assert!(c.check("d", 1).unwrap_err().contains("took device d back"));
        assert!(c.claim("d", 2).unwrap_err().contains("took device d back"));
        assert!(
            !c.release("d", 1),
            "the user's hold is not the thread's to release"
        );
        c.give_back("d");
        c.claim("d", 2).unwrap();
    }

    #[test]
    fn release_only_by_holder() {
        let mut c = Claims::default();
        c.claim("d", 1).unwrap();
        assert!(!c.release("d", 2));
        c.give_back("d");
        assert_eq!(
            c.holder("d"),
            Some(Holder::Thread(1)),
            "give_back frees only user holds"
        );
        assert!(c.release("d", 1));
        assert_eq!(c.holder("d"), None);
    }

    #[test]
    fn release_thread_frees_its_devices_only() {
        let mut c = Claims::default();
        c.claim("a", 1).unwrap();
        c.claim("b", 1).unwrap();
        c.claim("c", 2).unwrap();
        c.take_back("u");
        assert!(c.release_thread(1));
        assert!(!c.release_thread(1));
        assert_eq!(c.holder("a"), None);
        assert_eq!(c.holder("c"), Some(Holder::Thread(2)));
        assert_eq!(c.holder("u"), Some(Holder::User));
    }
}
