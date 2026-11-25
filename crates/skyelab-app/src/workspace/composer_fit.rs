//! Which attachments and model picks each provider's agent can take.

use skyelab_agent::{Attachment, AttachmentKind, CLAUDE_MODELS};
use skyelab_core::{Account, Provider};

/// Why `account`'s agent can't take these files, if it can't: Codex and Antigravity read images and text, not PDFs;
/// Cursor CLI reads text attachments only.
pub(in crate::workspace) fn unfit(account: &Account, attachments: &[Attachment]) -> Option<String> {
    match account.provider {
        Provider::Claude => None,
        Provider::Codex | Provider::Antigravity => {
            let pdf = attachments.iter().find(|a| a.kind == AttachmentKind::Pdf)?;
            Some(format!(
                "{} can't read PDFs ({}). Remove it, or attach an image or a text file.",
                account.provider.label(),
                pdf.name
            ))
        }
        Provider::Cursor => {
            let bad = attachments
                .iter()
                .find(|a| matches!(a.kind, AttachmentKind::Pdf | AttachmentKind::Image(_)))?;
            let what = match bad.kind {
                AttachmentKind::Pdf => "PDFs",
                AttachmentKind::Image(_) => "images",
                AttachmentKind::Text => return None,
            };
            Some(format!(
                "Cursor can't read {what} ({}). Remove it, or attach a text file.",
                bad.name
            ))
        }
    }
}

/// A model pick `provider`'s CLI can take: Claude's aliases and `claude-…` ids for Claude, non-Claude ids for Codex,
/// any non-empty non-Claude-alias model id for Cursor (which runs full model ids like `claude-4-sonnet`, `composer-1`,
/// `auto`), and anything except Claude Code bare aliases (`opus`, `sonnet`, `haiku`, `fable`) for Antigravity (which
/// hosts both Gemini and Claude-named models). A project default saved under another provider is dropped, not sent.
pub(in crate::workspace) fn fits(provider: Provider, model: &str) -> bool {
    let claude_alias = CLAUDE_MODELS.iter().any(|(alias, _)| *alias == model);
    let claude = claude_alias || model.starts_with("claude");
    match provider {
        Provider::Claude => claude,
        Provider::Codex => !claude,
        Provider::Cursor => !model.is_empty() && !claude_alias,
        Provider::Antigravity => !claude_alias,
    }
}
