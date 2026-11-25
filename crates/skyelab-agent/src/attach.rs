//! Files attached to a prompt: images, PDFs and text files, sent as content blocks (Claude) or input items (Codex).

use anyhow::{Result, bail};
use base64::Engine as _;
use serde_json::{Value, json};

/// The biggest file one attachment may be. Claude's API takes images up to 5 MB.
pub const MAX_ATTACHMENT_BYTES: usize = 5 << 20;
/// The most files one prompt may carry.
pub const MAX_ATTACHMENTS: usize = 10;

/// What an attachment is, which decides the block it becomes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// `image/png`, `image/jpeg`, `image/gif` or `image/webp`.
    Image(&'static str),
    Pdf,
    /// UTF-8 text, sent as its text.
    Text,
}

/// One attached file, read into memory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attachment {
    /// The file name shown on its chip and given to the agent.
    pub name: String,
    pub kind: Kind,
    pub data: Vec<u8>,
}

impl Attachment {
    /// Checks `data` (the cap, then its kind by content) and keeps it. Other binary files are refused.
    pub fn new(name: impl Into<String>, data: Vec<u8>) -> Result<Self> {
        let name = name.into();
        if data.len() > MAX_ATTACHMENT_BYTES {
            bail!(
                "{name} is {} MB; attachments can be up to {} MB",
                data.len().div_ceil(1 << 20),
                MAX_ATTACHMENT_BYTES >> 20
            );
        }
        let kind = match sniff(&data) {
            Some(kind) => kind,
            None if std::str::from_utf8(&data).is_ok() => Kind::Text,
            None => bail!("{name} isn't an image, a PDF or a text file"),
        };
        Ok(Self { name, kind, data })
    }

    fn base64(&self) -> String {
        base64::engine::general_purpose::STANDARD.encode(&self.data)
    }

    fn text(&self) -> &str {
        std::str::from_utf8(&self.data).unwrap_or_default() // Kind::Text was checked to be UTF-8
    }

    /// The Claude stream-json content block.
    pub(crate) fn claude_block(&self) -> Value {
        match self.kind {
            Kind::Image(media_type) => json!({
                "type": "image",
                "source": { "type": "base64", "media_type": media_type, "data": self.base64() },
            }),
            Kind::Pdf => json!({
                "type": "document",
                "title": self.name,
                "source": { "type": "base64", "media_type": "application/pdf", "data": self.base64() },
            }),
            Kind::Text => json!({
                "type": "document",
                "title": self.name,
                "source": { "type": "text", "media_type": "text/plain", "data": self.text() },
            }),
        }
    }

    /// The Codex `turn/start` input item. Codex reads images and text, not PDFs.
    pub(crate) fn codex_item(&self) -> Result<Value> {
        Ok(match self.kind {
            Kind::Image(media_type) => json!({
                "type": "image",
                "url": format!("data:{media_type};base64,{}", self.base64()),
            }),
            Kind::Text => json!({
                "type": "text",
                "text": format!("<attached_file name={}>\n{}\n</attached_file>", json!(self.name), self.text()),
            }),
            Kind::Pdf => bail!(
                "Codex can't read PDFs ({}); attach an image or a text file",
                self.name
            ),
        })
    }
}

/// An image or PDF, by its magic bytes (never by the name, which the user or a repo chose).
fn sniff(data: &[u8]) -> Option<Kind> {
    Some(match data {
        [0x89, b'P', b'N', b'G', ..] => Kind::Image("image/png"),
        [0xFF, 0xD8, 0xFF, ..] => Kind::Image("image/jpeg"),
        [b'G', b'I', b'F', b'8', ..] => Kind::Image("image/gif"),
        [
            b'R',
            b'I',
            b'F',
            b'F',
            _,
            _,
            _,
            _,
            b'W',
            b'E',
            b'B',
            b'P',
            ..,
        ] => Kind::Image("image/webp"),
        [b'%', b'P', b'D', b'F', b'-', ..] => Kind::Pdf,
        _ => return None,
    })
}

/// Claude's `message.content`: the plain text alone, or the attachments' blocks followed by the text.
pub(crate) fn claude_content(text: &str, attachments: &[Attachment]) -> Value {
    if attachments.is_empty() {
        return json!(text);
    }
    let mut blocks: Vec<Value> = attachments.iter().map(Attachment::claude_block).collect();
    if !text.is_empty() {
        blocks.push(json!({ "type": "text", "text": text }));
    }
    Value::Array(blocks)
}

/// Codex's `input`: the text, then each attachment.
pub(crate) fn codex_input(text: &str, attachments: &[Attachment]) -> Result<Value> {
    let mut items = vec![json!({ "type": "text", "text": text })];
    for a in attachments {
        items.push(a.codex_item()?);
    }
    Ok(Value::Array(items))
}

/// Cursor's stdin prompt: plain text plus any attached text files. Cursor CLI does not take inline images or PDFs.
pub(crate) fn cursor_prompt(text: &str, attachments: &[Attachment]) -> Result<String> {
    let mut parts = Vec::new();
    if !text.is_empty() {
        parts.push(text.to_owned());
    }
    for a in attachments {
        match a.kind {
            Kind::Text => parts.push(format!(
                "<attached_file name={}>\n{}\n</attached_file>",
                json!(a.name),
                a.text()
            )),
            Kind::Image(_) => bail!("Cursor can't read images ({}); attach a text file", a.name),
            Kind::Pdf => bail!("Cursor can't read PDFs ({}); attach a text file", a.name),
        }
    }
    Ok(parts.join("\n\n"))
}

/// Refuses a prompt with more than [`MAX_ATTACHMENTS`] files.
pub(crate) fn check_count(attachments: &[Attachment]) -> Result<()> {
    if attachments.len() > MAX_ATTACHMENTS {
        bail!("a message can carry up to {MAX_ATTACHMENTS} files");
    }
    Ok(())
}
