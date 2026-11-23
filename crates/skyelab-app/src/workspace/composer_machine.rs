//! The machine name the composer footer shows.

/// This machine's name as people know it (macOS's Computer Name), read once.
pub(in crate::workspace) fn machine_name() -> Option<String> {
    static NAME: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    NAME.get_or_init(|| {
        // Debug builds only: a generic name for screenshots, so no real machine name lands in the docs.
        if cfg!(debug_assertions)
            && let Ok(name) = std::env::var("SKYELAB_VERIFY_MACHINE")
        {
            return Some(name);
        }
        #[cfg(target_os = "macos")]
        {
            // Absolute path and an empty env: nothing of ours leaks into the child.
            let mut cmd = std::process::Command::new("/usr/sbin/scutil");
            cmd.args(["--get", "ComputerName"]).env_clear();
            let out = skyelab_core::process::output(&mut cmd, None).ok()?;
            let name = String::from_utf8_lossy(&out.stdout).trim().to_owned();
            (out.status.success() && !name.is_empty()).then_some(name)
        }
        #[cfg(windows)]
        {
            std::env::var("COMPUTERNAME").ok().filter(|n| !n.is_empty())
        }
        #[cfg(not(any(target_os = "macos", windows)))]
        {
            let name = std::fs::read_to_string("/proc/sys/kernel/hostname")
                .or_else(|_| std::fs::read_to_string("/etc/hostname"))
                .ok()?;
            Some(name.trim().to_owned()).filter(|n| !n.is_empty())
        }
    })
    .clone()
}
