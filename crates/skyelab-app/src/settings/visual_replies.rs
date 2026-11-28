//! Settings → Chat → Visual replies: the https hosts an agent's page (`html_render`) may load scripts, styles and
//! fonts from. Only the user edits it: here, or with a card's Allow. Images and media never load from the network.

use super::Settings;
use super::widgets::{row, section, setting};
use crate::ui::ActiveTheme as _;
use crate::ui::Icon;
use crate::ui::{self, Button, SkyeIcon, TextField};
use crate::workspace::thread_extras::prefs;
use gpui::{AnyElement, Context, SharedString, div, prelude::*};
use skyelab_core::page_hosts::parse_host;
use skyelab_core::{HtmlWidth, VisualReplySettings};

impl Settings {
    /// Saves `hosts` as the list; a failed save shows on the page.
    pub(super) fn set_page_hosts(&mut self, hosts: Vec<String>, cx: &mut Context<Self>) {
        let save = prefs::save_page_hosts(hosts, cx);
        self.report(save, "the visual reply hosts", cx);
        cx.notify();
    }

    /// Saves the preferred inline card width for visual replies (`html_render`).
    pub(super) fn set_html_width(&mut self, width: HtmlWidth, cx: &mut Context<Self>) {
        let save = prefs::save_html_width(width, cx);
        self.report(save, "the visual reply width", cx);
        cx.notify();
    }

    /// The host field was confirmed: add what it holds when the validator takes it, else say why.
    pub(super) fn add_page_host(&mut self, window: &mut gpui::Window, cx: &mut Context<Self>) {
        let typed = self.page_host_input.read(cx).value().to_string();
        if typed.trim().is_empty() {
            return;
        }
        match parse_host(&typed) {
            Ok(host) => {
                self.threads_error = None;
                let mut hosts = prefs::page_hosts(cx);
                if !hosts.contains(&host) {
                    hosts.push(host);
                    self.set_page_hosts(hosts, cx);
                }
                self.page_host_input
                    .update(cx, |i, cx| i.set_value("", window, cx));
            }
            Err(why) => {
                self.threads_error = Some(format!("\"{}\" can't be allowed. {why}", typed.trim()));
                cx.notify();
            }
        }
    }

    pub(super) fn render_visual_replies(&self, cx: &mut Context<Self>) -> AnyElement {
        // A hand-edited entry the validator refuses never reaches a page, so it isn't listed as allowed either.
        let hosts = skyelab_core::page_hosts::valid_hosts(&prefs::page_hosts(cx));
        let is_default = hosts == VisualReplySettings::default().allowed_hosts;
        let width = prefs::html_width(cx);
        let theme = cx.theme();
        let mut rows: Vec<AnyElement> = vec![setting(
            true,
            "Card width",
            "Layout width for inline visual reply pages. Standard (728 px) matches the reply column; \
             Wide (960 px) and Full (up to 1280 px or available width) give charts and diagrams more room.",
            ui::SegmentedControl::new("visual-reply-width")
                .with_size(ui::Size::Sm)
                .items(HtmlWidth::ALL.map(HtmlWidth::label))
                .selected(HtmlWidth::ALL.iter().position(|&w| w == width))
                .on_click(cx.listener(|this, ix: &usize, _, cx| {
                    if let Some(&picked) = HtmlWidth::ALL.get(*ix) {
                        this.set_html_width(picked, cx);
                    }
                })),
            cx,
        )];
        rows.extend(hosts.iter().enumerate().map(|(i, host)| {
            let gone = host.clone();
            row(false, cx)
                .debug_selector(|| format!("page-host-{i}"))
                .gap_3()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(crate::ui::BODY)
                        .font_family(theme.mono_font_family.clone())
                        .child(format!("https://{host}")),
                )
                .child(
                    Button::new(SharedString::from(format!("page-host-remove-{i}")))
                        .ghost()
                        .xsmall()
                        .icon(Icon::new(SkyeIcon::Trash))
                        .tooltip(format!("Remove {host}"))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let hosts = prefs::page_hosts(cx)
                                .into_iter()
                                .filter(|h| parse_host(h).ok().as_ref() != Some(&gone))
                                .collect();
                            this.set_page_hosts(hosts, cx);
                        })),
                )
                .into_any_element()
        }));
        if hosts.is_empty() {
            rows.push(
                row(false, cx)
                    .text_size(super::widgets::DESC)
                    .text_color(theme.muted_foreground)
                    .child("No hosts: pages use only their own inline scripts, styles and fonts.")
                    .into_any_element(),
            );
        }
        rows.push(
            row(false, cx)
                .gap_2()
                .child(
                    div()
                        .flex_1()
                        .debug_selector(|| "page-host-input".into())
                        .child(
                            TextField::new(&self.page_host_input)
                                .small()
                                .accessibility_label("Allowed CDN host"),
                        ),
                )
                .child(
                    Button::new("page-host-add")
                        .outline()
                        .small()
                        .icon(Icon::new(SkyeIcon::Plus))
                        .label("Add")
                        .on_click(
                            cx.listener(|this, _, window, cx| this.add_page_host(window, cx)),
                        ),
                )
                .child(
                    Button::new("page-host-reset")
                        .ghost()
                        .small()
                        .label("Reset to defaults")
                        .disabled(is_default)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.threads_error = None;
                            this.set_page_hosts(VisualReplySettings::default().allowed_hosts, cx);
                        })),
                )
                .into_any_element(),
        );
        section(
            "Visual replies",
            "Where an agent's page may load scripts, styles and fonts from (https only). Images and media are \
             never fetched. A page refused a host says so on its card, and only you can allow it.",
            rows,
            cx,
        )
        .into_any_element()
    }
}
