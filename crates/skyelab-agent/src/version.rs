//! `<agent> --version`, for the Welcome page's agent list.

use anyhow::{Context as _, Result, bail};
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

const DEADLINE: Duration = Duration::from_secs(5);

/// The first line `bin --version` prints, found the way launches find it and run with the agent env allowlist
/// only (no account's config dir: a version check needs no login). Killed after 5 s.
pub fn agent_version(bin: &Path) -> Result<String> {
    let bin = crate::launch::resolve_bin(bin)?;
    let mut cmd = Command::new(&bin);
    cmd.arg("--version")
        .current_dir(std::env::temp_dir()) // never an untrusted project folder
        .env_clear()
        .envs(skyelab_core::env::inherited_agent_env(false));
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
    let mut run = skyelab_core::process::Captured::spawn(&mut cmd, None)
        .with_context(|| format!("running {}", bin.display()))?;
    let start = Instant::now();
    let status = loop {
        if let Some(status) = run.child.try_wait()? {
            break status;
        }
        if start.elapsed() > DEADLINE {
            #[cfg(unix)]
            crate::launch::kill_group(run.child.id());
            let _ = run.child.kill();
            let _ = run.child.wait();
            bail!(
                "{} --version took longer than {}s",
                bin.display(),
                DEADLINE.as_secs()
            );
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let out = run.output(status)?;
    if !status.success() {
        bail!("{} --version failed ({status})", bin.display());
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let line: String = text
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or_default()
        .chars()
        .take(80)
        .collect();
    if line.is_empty() {
        bail!("{} --version printed nothing", bin.display());
    }
    Ok(line)
}
