use skyelab_agent::{Launch, Session, start_with_bin};
use skyelab_core::AccountsConfig;
use std::path::Path;

/// Starts `bin` under the account `cfg` resolves for `dir`, like the app does.
pub fn start_in(
    bin: &Path,
    cfg: &AccountsConfig,
    dir: &Path,
    prompt: &str,
) -> anyhow::Result<Session> {
    let account = cfg
        .resolve(dir)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("test setup: {} is not bound", dir.display()))?;
    let launch = Launch {
        account: &account,
        dir,
        resume: None,
        prompt,
        mcp_config: None,
        model: None,
        effort: None,
        attachments: &[],
    };
    start_with_bin(bin, cfg, &launch)
}
