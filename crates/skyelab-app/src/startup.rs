//! Startup I/O helpers for `lib.rs`: window options, binary overrides, store, accounts, single-instance lock, and
//! orchestration setup.

use crate::{ui, workspace};
use anyhow::Context as _;
use gpui::{App, Bounds, WindowBounds, WindowKind, WindowOptions, px, size};
use skyelab_core::AccountsConfig;
use skyelab_core::instance::APP_DIR;
use skyelab_store::Store;

/// `skyelab --mcp-proxy`: claude runs this as SkyeLab's MCP server (see skyelab_agent::mcp).
pub(crate) const MCP_PROXY_FLAG: &str = "--mcp-proxy";

/// Debug builds only: `SKYELAB_VERIFY_TOPMOST=1` keeps windows above others so
/// `/loop-verify-ui` can capture them without moving the user's windows.
pub(crate) fn window_options(bounds: Bounds<gpui::Pixels>) -> WindowOptions {
    let topmost = cfg!(debug_assertions) && std::env::var_os("SKYELAB_VERIFY_TOPMOST").is_some();
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        kind: if topmost {
            WindowKind::PopUp
        } else {
            WindowKind::Normal
        },
        titlebar: Some(ui::titlebar_options()),
        window_min_size: Some(size(px(800.), px(560.))),
        ..Default::default()
    }
}

/// An agent binary: `name`, or in debug builds the env override (`SKYELAB_CLAUDE_BIN`,
/// `SKYELAB_CODEX_BIN`, `SKYELAB_CURSOR_BIN`: a fake, for verification).
pub(crate) fn agent_bin(
    env: Option<std::ffi::OsString>,
    debug: bool,
    name: &str,
) -> std::path::PathBuf {
    match env {
        Some(bin) if debug => bin.into(),
        _ => name.into(),
    }
}

/// This install's device key for connecting to other machines, which approve it. Release builds on macOS keep it
/// in the keychain, where another program (an agent) can't read it without the user's yes. Debug builds and other
/// OSes use a 0600 file in the config dir, so a throwaway `HOME` never touches the login keychain. Read through
/// [`skyelab_client::LazyDeviceKey`] on the first connection to another machine, never at startup: the keychain
/// may prompt, and a user who never connects anywhere should never see that.
pub(crate) fn device_key() -> Result<String, String> {
    #[cfg(target_os = "macos")]
    if !cfg!(debug_assertions) {
        return skyelab_client::keychain_device_key()
            .map_err(|e| format!("the keychain item dev.openskye.skyelab couldn't be read ({e})"));
    }
    let path = dirs::config_dir()
        .ok_or("no config folder")?
        .join(APP_DIR)
        .join("device-key");
    skyelab_client::load_or_create_device_key(&path)
        .map_err(|e| format!("{} couldn't be read ({e})", path.display()))
}

/// The OS Tailscale, keeping this node's TLS certificate in a private folder next to the store.
pub(crate) fn tailnet() -> skyelab_tailnet::TailscaleCli {
    let cli = match std::env::var_os("SKYELAB_TAILSCALE_BIN") {
        // Debug builds only: a fake tailnet for live checks (tests/fixtures/fake-tailscale.sh).
        Some(bin) if cfg!(debug_assertions) => {
            skyelab_tailnet::TailscaleCli::with_binaries(vec![bin.into()])
        }
        _ => skyelab_tailnet::TailscaleCli::default(),
    };
    // Local, not roaming, app data: a private key must not sync to a Windows roaming profile.
    match dirs::data_local_dir() {
        Some(d) => cli.with_cert_dir(d.join(APP_DIR).join("tls")),
        None => cli,
    }
}

/// Where threads and projects persist: the platform data dir.
pub(crate) fn store_path() -> Option<std::path::PathBuf> {
    dirs::data_dir().map(|d| d.join(APP_DIR).join("skyelab.db"))
}

/// Opens the store, or an in-memory one (with the reason) so the app still runs.
pub(crate) fn open_store() -> (Store, Option<String>) {
    let opened = store_path()
        .ok_or_else(|| anyhow::anyhow!("no platform data dir"))
        .and_then(|path| {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
                // The store holds prompts and tool output, which can include secrets.
                #[cfg(unix)]
                std::fs::set_permissions(dir, std::os::unix::fs::PermissionsExt::from_mode(0o700))?;
            }
            Store::open(&path)
        });
    match opened {
        Ok(store) => (store, None),
        Err(e) => (
            Store::open_in_memory().expect("an in-memory SQLite database always opens"),
            Some(format!("{e:#}. Nothing is saved this run.")),
        ),
    }
}

/// The app's MCP server for agent orchestration: grants in `<data dir>/skyelab/run/<pid>`. `None` (with the reason
/// on stderr) means threads run without them.
pub(crate) fn orchestration(
    cx: &mut App,
    store: workspace::SharedStore,
    config: AccountsConfig,
) -> Option<flume::Receiver<workspace::orchestrate::OrchCall>> {
    let installed = (|| {
        // Local, not roaming: on Windows a roaming profile can live on a file share, and grant files hold tokens.
        let runs = dirs::data_local_dir()
            .context("no platform data dir")?
            .join(APP_DIR)
            .join("run");
        skyelab_agent::mcp::remove_stale_runs(&runs);
        let run = runs.join(std::process::id().to_string());
        let proxy = skyelab_agent::mcp::Proxy {
            program: std::env::current_exe().context("finding the skyelab binary")?,
            args: vec![MCP_PROXY_FLAG.into()],
        };
        workspace::orchestrate::install(cx, &run, store, config, proxy)
    })();
    installed
        .map_err(|e| eprintln!("skyelab: agent orchestration is off: {e:#}"))
        .ok()
}

/// accounts.toml, or an empty config and the reason (a broken file binds nothing).
pub(crate) fn load_accounts(path: Option<&std::path::Path>) -> (AccountsConfig, Option<String>) {
    match path.map(AccountsConfig::load) {
        Some(Ok(c)) => (c, None),
        Some(Err(e)) => (AccountsConfig::default(), Some(format!("{e:#}"))),
        None => (AccountsConfig::default(), None),
    }
}

/// One SkyeLab per data dir (`skyelab_core::instance`). `None` (logged): the lock file couldn't be opened or locked
/// at all (not held by another copy), and the app opens anyway.
pub(crate) fn claim_instance() -> Option<skyelab_core::instance::Claim> {
    let dir = dirs::data_dir()?.join(APP_DIR);
    skyelab_core::instance::claim(&dir)
        .map_err(|e| eprintln!("skyelab: no single-instance lock: {e:#}"))
        .ok()
}

/// Debug builds keep their own config dir: the first one starts from a copy of the installed app's accounts (not its
/// store or settings), so bound folders keep their accounts.
pub(crate) fn seed_dev_accounts(dev: Option<&std::path::Path>) {
    let Some(dev) = dev.filter(|p| cfg!(debug_assertions) && !p.exists()) else {
        return;
    };
    let Some(installed) = dirs::config_dir().map(|d| d.join("skyelab").join("accounts.toml"))
    else {
        return;
    };
    if !installed.is_file() {
        return;
    }
    let copied = dev
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| std::fs::copy(&installed, dev));
    if let Err(e) = copied {
        eprintln!(
            "skyelab: couldn't copy {} for the dev build: {e}",
            installed.display()
        );
    }
}
