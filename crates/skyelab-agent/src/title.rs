//! A short thread title, written by the thread's own account's agent in one cheap call with no tools.

use anyhow::{Context as _, Result, bail};
use skyelab_core::{Account, AccountsConfig, Provider};
use std::path::Path;
use std::time::{Duration, Instant};

/// How much of the first prompt and of the first reply the title prompt carries.
const MAX_INPUT: usize = 2000;
/// A title call that takes longer is killed; the thread keeps its first-line title.
const DEADLINE: Duration = Duration::from_secs(90);
/// The most of the agent's answer read (only its first line is used).
const MAX_OUTPUT: usize = 64 << 10;
/// The longest title kept.
const MAX_WORDS: usize = 6;
const MAX_CHARS: usize = 80;

/// The prompt that asks for a title: only this thread's first prompt and first reply, each cut to [`MAX_INPUT`].
pub fn title_prompt(first_prompt: &str, reply: &str) -> String {
    format!(
        "Write a title for this conversation: at most {MAX_WORDS} words, plain text, no quotes, no punctuation at \
         the end. Reply with the title only.\n\n<user_message>\n{}\n</user_message>\n\n<assistant_reply>\n{}\n\
         </assistant_reply>",
        cut(first_prompt.trim()),
        cut(reply.trim())
    )
}

fn cut(s: &str) -> String {
    let mut t: String = s.chars().take(MAX_INPUT).collect();
    if s.chars().count() > MAX_INPUT {
        t.push('…');
    }
    t
}

/// The title in an agent's answer: its first non-empty line, quotes and markdown stripped, cut to six words and 80
/// characters. `None` when nothing is left.
pub fn clean_title(raw: &str) -> Option<String> {
    // Bidi controls and zero-width characters could make a title read as something else.
    let raw: String = raw
        .chars()
        .filter(|c| !matches!(c, '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2060}'..='\u{206F}' | '\u{FEFF}'))
        .collect();
    let line = raw.lines().map(str::trim).find(|l| !l.is_empty())?;
    let line = line
        .trim_start_matches(['#', '*', '-', '>', ' '])
        .trim_start_matches("Title:")
        .trim_matches(|c: char| c.is_whitespace() || "\"'`*_“”‘’".contains(c));
    let words: Vec<&str> = line.split_whitespace().take(MAX_WORDS).collect();
    let mut t: String = words.join(" ").chars().take(MAX_CHARS).collect();
    while t.ends_with(['.', ':', ';', ',']) {
        t.pop();
    }
    let t = t.trim().to_owned();
    (!t.is_empty() && !t.chars().any(char::is_control)).then_some(t)
}

/// Asks `account`'s agent for a title for `prompt` (from [`title_prompt`]), in `dir`, which must still resolve to
/// `account`. Claude: `claude -p` on its smallest model with no tools, no MCP and no saved session. Codex: an
/// ephemeral read-only `codex exec` with no MCP servers or hooks (its shell can still read files). The env is the agent allowlist plus the account's own, as for every agent.
pub fn generate_title(
    bin: &Path,
    config: &AccountsConfig,
    account: &Account,
    dir: &Path,
    prompt: &str,
) -> Result<String> {
    let (mut cmd, _, dir) = crate::launch::agent_command(bin, config, account, dir)?;
    let out_file = tempfile::NamedTempFile::new().context("creating the title's output file")?;
    match account.provider {
        Provider::Claude => {
            cmd.args([
                "-p",
                "--output-format",
                "text",
                "--model",
                "haiku",
                "--tools",
                "",
                "--setting-sources",
                "user",
                "--strict-mcp-config",
                "--disable-slash-commands",
                "--no-session-persistence",
                "--permission-mode",
                "dontAsk",
            ]);
        }
        Provider::Codex => {
            cmd.args([
                "exec",
                "--ephemeral",
                "--skip-git-repo-check",
                "-s",
                "read-only",
                "--config",
                "model_reasoning_effort=\"low\"",
                // No MCP servers or hooks: the prompt holds the agent's reply, which may quote untrusted text.
                "--config",
                "mcp_servers={}",
                "--config",
                "features.hooks=false",
                "--output-last-message",
            ])
            .arg(out_file.path())
            .arg("-");
        }
    }
    cmd.current_dir(&dir);
    let mut run = skyelab_core::process::Captured::spawn(&mut cmd, Some(prompt.as_bytes()))
        .context("starting the title call")?;
    let start = Instant::now();
    let status = loop {
        if let Some(status) = run.child.try_wait().context("waiting for the title call")? {
            break status;
        }
        if start.elapsed() > DEADLINE {
            #[cfg(unix)]
            crate::launch::kill_group(run.child.id());
            let _ = run.child.kill();
            let _ = run.child.wait();
            bail!("the title call took longer than {}s", DEADLINE.as_secs());
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let output = run.output(status).context("reading the title call")?;
    if !status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        bail!("the title call failed ({status}): {}", err.trim());
    }
    let mut text = match account.provider {
        Provider::Claude => output.stdout,
        Provider::Codex => std::fs::read(out_file.path()).context("reading Codex's answer")?,
    };
    text.truncate(MAX_OUTPUT);
    clean_title(&String::from_utf8_lossy(&text)).context("the agent's answer held no title")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_are_cleaned_and_cut() {
        assert_eq!(
            clean_title("\n  \"Fix the login bug.\"\nmore").as_deref(),
            Some("Fix the login bug")
        );
        assert_eq!(
            clean_title("# Title: Add a parser").as_deref(),
            Some("Add a parser")
        );
        assert_eq!(
            clean_title("one two three four five six seven eight").as_deref(),
            Some("one two three four five six")
        );
        assert_eq!(clean_title("  \n\"\"\n"), None);
        assert_eq!(
            clean_title("Fix\u{202E}gol bug").as_deref(),
            Some("Fixgol bug")
        );
    }

    #[test]
    fn the_prompt_holds_only_the_two_inputs_cut() {
        let long = "x".repeat(10_000);
        let p = title_prompt("fix login", &long);
        assert!(p.contains("fix login"));
        assert!(p.len() < 3 * MAX_INPUT, "bounded: {}", p.len());
    }
}
