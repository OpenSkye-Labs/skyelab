//! Files for the next message: the paperclip's file dialog, drag-drop and paste, each read off the UI thread and
//! checked against the caps before it shows as a chip.

use super::composer_state::AttachFiles;
use super::thread::ThreadView;
use crate::ui;
use crate::ui::ActiveTheme as _;
use crate::ui::Button;
use crate::ui::SkyeIcon;
use crate::ui::{Icon, h_flex};
use gpui::{
    AnyElement, AppContext as _, ClipboardEntry, ClipboardItem, Context, SharedString, Window, div,
    prelude::*, px,
};
use skyelab_agent::AttachmentKind;
use skyelab_agent::{Attachment, MAX_ATTACHMENT_BYTES, MAX_ATTACHMENTS};
use std::path::PathBuf;

impl ThreadView {
    pub(in crate::workspace) fn attach_files(
        &mut self,
        _: &AttachFiles,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let paths = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("Attach".into()),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = paths.await {
                let _ = this.update(cx, |this, cx| this.attach_paths(paths, cx));
            }
        })
        .detach();
    }

    /// Reads each file off the UI thread and adds it, or says why not. Over the cap, nothing is read.
    pub(in crate::workspace) fn attach_paths(
        &mut self,
        paths: Vec<PathBuf>,
        cx: &mut Context<Self>,
    ) {
        if self.read_only {
            return; // a drop on a thread that can't send
        }
        let room = MAX_ATTACHMENTS.saturating_sub(self.composer.attachments.len());
        let task = cx.background_spawn(async move {
            let mut out = Vec::new();
            for (i, path) in paths.into_iter().enumerate() {
                if i >= room {
                    out.push(Err(format!(
                        "a message can carry up to {MAX_ATTACHMENTS} files"
                    )));
                    break;
                }
                out.push(read_attachment(&path));
            }
            out
        });
        cx.spawn(async move |this, cx| {
            let read = task.await;
            let _ = this.update(cx, |this, cx| {
                this.composer.notice = None;
                for r in read {
                    match r {
                        Ok(a) => this.add_attachment(a),
                        Err(e) => this.composer.notice = Some(e.into()),
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// A paste: images and copied files become attachments, and so does long text (T3's fold), with an offer to
    /// paste it as text after all. Other text goes into the prompt as usual (`false`).
    pub(in crate::workspace) fn attach_clipboard(
        &mut self,
        item: &ClipboardItem,
        cx: &mut Context<Self>,
    ) -> bool {
        let mut taken = false;
        for entry in item.entries() {
            match entry {
                ClipboardEntry::String(s)
                    if !taken && skyelab_core::prompt::is_long_paste(s.text()) =>
                {
                    let names = self.composer.attachments.iter().map(|a| a.name.as_str());
                    let name = skyelab_core::prompt::pasted_file_name(names);
                    let before = self.composer.attachments.len();
                    if let Ok(a) = Attachment::new(name.clone(), s.text().as_bytes().to_vec()) {
                        self.add_attachment(a);
                    }
                    // No chip (no room, too big, a kind the account can't take): it goes in as text, not nowhere.
                    taken = self.composer.attachments.len() > before;
                    if taken {
                        self.composer.pasted = Some((name, s.text().clone()));
                    }
                }
                ClipboardEntry::Image(img) => {
                    taken = true;
                    let n = self.composer.attachments.len() + 1;
                    let name = format!("pasted-{n}.{}", img.format.extension());
                    match Attachment::new(name, img.bytes.clone()) {
                        Ok(a) => self.add_attachment(a),
                        Err(e) => self.composer.notice = Some(format!("{e:#}").into()),
                    }
                }
                ClipboardEntry::ExternalPaths(paths) => {
                    taken = true;
                    self.attach_paths(paths.paths().to_vec(), cx);
                }
                _ => {}
            }
        }
        cx.notify();
        taken
    }

    /// Adds a read file, checked again here: reads finish in any order, and the account may not take its kind.
    fn add_attachment(&mut self, a: Attachment) {
        if self.composer.attachments.len() >= MAX_ATTACHMENTS {
            self.composer.notice =
                Some(format!("a message can carry up to {MAX_ATTACHMENTS} files").into());
            return;
        }
        let unfit = self
            .composer_account()
            .and_then(|acct| super::composer_state::unfit(&acct, std::slice::from_ref(&a)));
        match unfit {
            Some(why) => self.composer.notice = Some(why.into()),
            None => self.composer.attachments.push(a),
        }
    }

    pub(in crate::workspace) fn remove_attachment(&mut self, ix: usize, cx: &mut Context<Self>) {
        if ix < self.composer.attachments.len() {
            let gone = self.composer.attachments.remove(ix);
            if self
                .composer
                .pasted
                .as_ref()
                .is_some_and(|(n, _)| *n == gone.name)
            {
                self.composer.pasted = None;
            }
        }
        self.composer.notice = None;
        cx.notify();
    }

    /// "Paste as text": the last long paste leaves its chip and goes into the prompt at the caret.
    pub(in crate::workspace) fn paste_as_text(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((name, text)) = self.composer.pasted.take() else {
            return;
        };
        self.composer.attachments.retain(|a| a.name != name);
        self.prompt.update(cx, |p, cx| p.replace(text, window, cx));
        self.focus_prompt(window, cx);
        cx.notify();
    }
}

/// One file from disk as an attachment, refused before reading when it is over the cap.
fn read_attachment(path: &std::path::Path) -> Result<Attachment, String> {
    let name = path
        .file_name()
        .map_or_else(|| "file".to_owned(), |n| n.to_string_lossy().into_owned());
    let meta = std::fs::metadata(path).map_err(|e| format!("{name}: {e}"))?;
    if !meta.is_file() {
        return Err(format!("{name} isn't a file"));
    }
    if meta.len() > MAX_ATTACHMENT_BYTES as u64 {
        return Err(format!(
            "{name} is over {} MB, the most one attachment can be",
            MAX_ATTACHMENT_BYTES >> 20
        ));
    }
    // Read no more than the cap allows: the file may have grown since the check above.
    let mut data = Vec::new();
    std::fs::File::open(path)
        .and_then(|f| {
            use std::io::Read as _;
            f.take(MAX_ATTACHMENT_BYTES as u64 + 1)
                .read_to_end(&mut data)
        })
        .map_err(|e| format!("{name}: {e}"))?;
    Attachment::new(name, data).map_err(|e| format!("{e:#}"))
}

impl ThreadView {
    /// Removable chips for the attached files, above the prompt.
    pub(super) fn render_attachments(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.composer.attachments.is_empty() {
            return None;
        }
        let theme = cx.theme();
        Some(
            h_flex()
                .flex_wrap()
                .gap_1p5()
                .px_3()
                .pt_3()
                .children(self.composer.attachments.iter().enumerate().map(|(ix, a)| {
                    let icon = match a.kind {
                        AttachmentKind::Image(_) => SkyeIcon::Image,
                        AttachmentKind::Pdf | AttachmentKind::Text => SkyeIcon::File,
                    };
                    h_flex()
                        .id(SharedString::from(format!("attachment-{ix}")))
                        .h(px(28.))
                        .max_w(px(220.))
                        .pl_2()
                        .gap_1()
                        .rounded(ui::RADIUS)
                        .border_1()
                        .border_color(theme.border)
                        .bg(theme.muted)
                        .text_size(ui::META)
                        .child(Icon::new(icon).size_3().text_color(theme.muted_foreground))
                        .child(div().min_w_0().truncate().child(a.name.clone()))
                        .child(
                            Button::new(SharedString::from(format!("remove-attachment-{ix}")))
                                .ghost()
                                .xsmall()
                                .icon(Icon::new(SkyeIcon::X))
                                .tooltip("Remove")
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.remove_attachment(ix, cx);
                                    this.restore_draft(window, cx);
                                })),
                        )
                }))
                .into_any_element(),
        )
    }
}
