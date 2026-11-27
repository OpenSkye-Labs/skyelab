//! Visual replies in the timeline (T3's `chat/HtmlRenderFrame.tsx`): a successful `html_render` call draws as a
//! framed card with the page's snapshot, and its expand button opens the live page in a dialog. The page is the
//! agent's, so it only ever runs in `skyelab_browser::page`'s locked webview. The snapshot is an image because a
//! native view can't be clipped by the scrolling timeline (docs/gpui/webview.md); the live page sits in a modal
//! dialog, where nothing GPUI draws over it.

use super::orchestrate::Reply;
use super::thread_extras::prefs;
use super::thread_items::ItemsView;
use super::thread_model::ToolItem;
use crate::ui;
use crate::ui::Button;
use crate::ui::SkyeIcon;
use gpui::{
    AnyElement, AnyWindowHandle, App, AppContext as _, Context, FontWeight, Global, Hsla, Image,
    ImageFormat, IntoElement, Render, SharedString, Window, div, img, prelude::*, px,
};
use gpui_component::{ActiveTheme as _, Icon, Theme, WindowExt as _, h_flex, v_flex};
use serde_json::{Value, json};
use skyelab_agent::mcp::HTML_MAX_CHARS;
use skyelab_browser::page::{self, MAX_SHOT_HEIGHT, Page, PageTheme};
use skyelab_core::HtmlWidth;
use std::collections::{HashMap, VecDeque};
use std::hash::{Hash as _, Hasher as _};
use std::sync::Arc;

/// The card's page, when `tool` is a successful `html_render`: `(title, html, height)`, borrowed.
pub(super) fn page_of(tool: &ToolItem) -> Option<(&str, &str, u32)> {
    if !skyelab_agent::mcp::is_rendered_page(tool) {
        return None;
    }
    let raw = &tool.raw;
    // The server checked these when it answered, but a remote machine's stream is checked again (by bytes: cheap).
    let height = raw["height"]
        .as_u64()?
        .clamp(80, u64::from(MAX_SHOT_HEIGHT));
    let html = raw["html"]
        .as_str()
        .filter(|h| h.len() <= 4 * HTML_MAX_CHARS)?;
    let title = raw["title"].as_str().filter(|t| t.len() <= 4 * 200)?;
    Some((title, html, height as u32))
}

/// T3's theme variables, from the app's theme in `dark` or light mode.
pub(super) fn page_theme(dark: bool, cx: &App) -> PageTheme {
    let current = Theme::global(cx);
    if current.is_dark() == dark {
        return theme_vars(current);
    }
    // The other mode (an agent's preview): its colors from the config the app keeps for it.
    let mut other = current.clone();
    let config = if dark {
        other.dark_theme.clone()
    } else {
        other.light_theme.clone()
    };
    other.apply_config(&config);
    theme_vars(&other)
}

fn theme_vars(t: &Theme) -> PageTheme {
    let dark = t.is_dark();
    let surface = |c: Hsla| hex(c.opacity(0.14));
    let fonts = |family: &SharedString, fallback: &str| format!("\"{family}\", {fallback}");
    let mut vars = vec![
        ("--background", hex(t.background)),
        ("--foreground", hex(t.foreground)),
        ("--muted", hex(t.muted)),
        ("--muted-foreground", hex(t.muted_foreground)),
        ("--card", hex(t.popover)),
        ("--card-foreground", hex(t.popover_foreground)),
        ("--popover", hex(t.popover)),
        ("--popover-foreground", hex(t.popover_foreground)),
        ("--secondary", hex(t.secondary)),
        ("--secondary-foreground", hex(t.secondary_foreground)),
        ("--border", hex(t.border)),
        ("--input", hex(t.input)),
        ("--ring", hex(t.ring)),
        ("--primary", hex(t.primary)),
        ("--primary-foreground", hex(t.primary_foreground)),
        // T3: --accent is the brand color, not a hover surface.
        ("--accent", hex(t.primary)),
        ("--accent-foreground", hex(t.primary_foreground)),
        ("--accent-surface", surface(t.primary)),
        ("--accent-surface-foreground", hex(t.foreground)),
        ("--destructive", hex(t.danger)),
        ("--destructive-foreground", hex(ui::ink(t, t.danger))),
        ("--destructive-surface", surface(t.danger)),
        ("--warning", hex(t.warning)),
        ("--warning-foreground", hex(ui::ink(t, t.warning))),
        ("--warning-surface", surface(t.warning)),
        ("--success", hex(t.success)),
        ("--success-foreground", hex(ui::ink(t, t.success))),
        ("--info", hex(t.info)),
        ("--info-foreground", hex(ui::ink(t, t.info))),
        ("--code-background", hex(t.muted)),
        ("--code-foreground", hex(t.foreground)),
        ("--chart-1", hex(t.chart_1)),
        ("--chart-2", hex(t.chart_2)),
        ("--chart-3", hex(t.chart_3)),
        ("--chart-4", hex(t.chart_4)),
        ("--chart-5", hex(t.chart_5)),
        ("--chart-6", hex(t.info)),
        ("--radius", "0.625rem".into()),
    ];
    vars.push((
        "--font-sans",
        fonts(&t.font_family, "-apple-system, system-ui, sans-serif"),
    ));
    vars.push((
        "--font-mono",
        fonts(&t.mono_font_family, "Menlo, Consolas, monospace"),
    ));
    PageTheme { dark, vars }
}

fn hex(c: Hsla) -> String {
    let c = gpui::Rgba::from(c);
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{:02x}{:02x}{:02x}{:02x}", b(c.r), b(c.g), b(c.b), b(c.a))
}

/// A snapshot's state.
#[derive(Clone)]
enum Shot {
    Pending,
    /// The image, the page's height in CSS pixels at the column width (the image is at the screen's scale), and the
    /// hosts its scripts, styles or fonts were refused from (checked, but named by the page: only offered).
    Ready(Arc<Image>, u32, Arc<[String]>),
    Failed(SharedString),
}

struct Job {
    key: (String, u64),
    html: String,
    theme: PageTheme,
    hosts: Vec<String>,
    width: u32,
    window: AnyWindowHandle,
}

/// Snapshots by tool call, theme, width and allowed hosts, rendered one at a time (each is a webview of its own).
#[derive(Default)]
struct Shots {
    done: HashMap<(String, u64), Shot>,
    queue: VecDeque<Job>,
    busy: bool,
}

impl Global for Shots {}

// shortcut: past this many the whole cache is dropped and visible cards render again; an LRU if that shows.
const MAX_SHOTS: usize = 64;

/// Resolves the CSS width a visual reply card renders at for `pref` in `window`.
pub(super) fn target_width(pref: HtmlWidth, window: &Window) -> u32 {
    let available = match pref {
        HtmlWidth::Full => {
            let vp = f32::from(window.viewport_size().width).round() as u32;
            // Quantize to 64 px steps so live window-resize drags don't re-shoot a webview on every pixel.
            (vp > 0).then(|| (vp.saturating_sub(48) / 64) * 64)
        }
        HtmlWidth::Standard | HtmlWidth::Wide => None,
    };
    pref.resolve_px(available)
}

/// Cache key for a rendered visual reply snapshot (includes theme, allowed hosts, width, and page HTML).
pub(super) fn shot_key(
    tool_id: &str,
    html: &str,
    theme: &PageTheme,
    hosts: &[String],
    width: u32,
) -> (String, u64) {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    theme.dark.hash(&mut h);
    theme.vars.hash(&mut h);
    hosts.hash(&mut h);
    width.hash(&mut h);
    html.hash(&mut h);
    (tool_id.to_owned(), h.finish())
}

/// The snapshot of `tool_id`'s page in `theme` at `width`, queuing it the first time it's asked for. The key holds
/// the page too: a tool id is only unique within its own thread (Codex), and a remote machine names its own.
fn shot(
    tool_id: &str,
    html: &str,
    theme: PageTheme,
    width: u32,
    window: &Window,
    cx: &mut App,
) -> Shot {
    let hosts = prefs::page_hosts(cx);
    let key = shot_key(tool_id, html, &theme, &hosts, width);
    if let Some(s) = cx.default_global::<Shots>().done.get(&key) {
        return s.clone();
    }
    if cx.default_global::<Shots>().done.len() >= MAX_SHOTS {
        let shots = cx.default_global::<Shots>();
        let dropped: Vec<Arc<Image>> = shots
            .done
            .extract_if(|_, s| !matches!(s, Shot::Pending))
            .filter_map(|(_, s)| match s {
                Shot::Ready(image, ..) => Some(image),
                _ => None,
            })
            .collect();
        // GPUI keeps every decoded image until told.
        for image in dropped {
            image.remove_asset(cx);
        }
    }
    let shots = cx.default_global::<Shots>();
    shots.done.insert(key.clone(), Shot::Pending);
    shots.queue.push_back(Job {
        key,
        html: html.to_owned(),
        theme,
        hosts,
        width,
        window: window.window_handle(),
    });
    // Never from inside a frame: the next one builds the webview.
    cx.defer(pump);
    Shot::Pending
}

fn pump(cx: &mut App) {
    let shots = cx.default_global::<Shots>();
    if shots.busy {
        return;
    }
    let Some(job) = shots.queue.pop_front() else {
        return;
    };
    shots.busy = true;
    let task = job.window.update(cx, |_, window, cx| {
        page::shoot(&job.html, &job.theme, &job.hosts, job.width, window, cx)
    });
    let background = cx.background_executor().clone();
    cx.spawn(async move |cx| {
        let result = match task {
            Ok(task) => match task.await {
                Ok(shot) => {
                    let height = shot.content_height.min(MAX_SHOT_HEIGHT);
                    let blocked: Arc<[String]> = shot.blocked.clone().into();
                    background
                        .spawn(async move { shot.to_png() })
                        .await
                        .map(|png| {
                            let image = Arc::new(Image::from_bytes(ImageFormat::Png, png));
                            Shot::Ready(image, height, blocked)
                        })
                }
                Err(e) => Err(e),
            },
            Err(e) => Err(e),
        };
        cx.update(|cx| {
            let shots = cx.default_global::<Shots>();
            shots.busy = false;
            match result {
                // The asking window closed before its turn: the next card drawn asks again from its own window.
                Err(_) if job.window.update(cx, |_, _, _| ()).is_err() => {
                    cx.default_global::<Shots>().done.remove(&job.key);
                }
                // shortcut: a failure (a timeout too) stays until the cache is cleared; retry timeouts if that bites.
                r => {
                    let shot = r.unwrap_or_else(|e| Shot::Failed(format!("{e:#}").into()));
                    cx.default_global::<Shots>().done.insert(job.key, shot);
                }
            }
            let _ = job.window.update(cx, |_, window, _| window.refresh());
            pump(cx);
        });
    })
    .detach();
}

/// The card: the title with an expand button, over the page's snapshot (or a placeholder at the agent's height).
pub(super) fn render<V: ItemsView>(
    tool: &ToolItem,
    (title, html, height): (&str, &str, u32),
    window: &mut Window,
    cx: &mut Context<V>,
) -> AnyElement {
    let dark = cx.theme().is_dark();
    let width = target_width(prefs::html_width(cx), window);
    let shot = shot(&tool.id, html, page_theme(dark, cx), width, window, cx);
    let id = tool.id.clone();
    // Looked up on click, so a frame never copies the page.
    let open = cx.listener(move |this: &mut V, _: &gpui::ClickEvent, window, cx| {
        let page = this.items().iter().find_map(|i| match i {
            super::thread_model::Item::Tool(t) if t.id == id => page_of(t),
            _ => None,
        });
        if let Some((title, html, _)) = page {
            open_page(title.to_owned(), html, window, cx);
        }
    });
    let open = std::rc::Rc::new(open);
    let theme = cx.theme();
    let title = title.to_owned();
    let note = |text: SharedString| {
        div()
            .h(px(height as f32))
            .flex()
            .items_center()
            .justify_center()
            .px_3()
            .text_size(ui::META)
            .text_color(theme.muted_foreground)
            .child(text)
    };
    let blocked = match &shot {
        Shot::Ready(_, _, blocked) if !blocked.is_empty() => {
            Some(allow::blocked_row(&tool.id, blocked, cx))
        }
        _ => None,
    };
    let body = match shot {
        // A box of the page's shape, at most its CSS width (a narrow column scales it down); left alone, img() takes its
        // height from the 2x pixels and letterboxes the page.
        Shot::Ready(image, h, _) => div()
            .debug_selector(|| "html-card-shot".into())
            .w_full()
            .flex()
            .justify_center()
            .child(
                div()
                    .w(px(width as f32))
                    .max_w_full()
                    .aspect_ratio(width as f32 / h.max(1) as f32)
                    .child(img(image).size_full()),
            )
            .into_any_element(),
        Shot::Pending => note("Rendering the page…".into()).into_any_element(),
        Shot::Failed(why) => {
            note(format!("No preview here ({why}). Expand to see the page.").into())
                .into_any_element()
        }
    };
    v_flex()
        .debug_selector(|| "html-card".into())
        .w_full()
        .my_1()
        .rounded(ui::RADIUS_LG)
        .border_1()
        .border_color(theme.border)
        .overflow_hidden()
        .child(
            h_flex()
                .h(px(32.))
                .gap_2()
                .pl_3()
                .pr_1()
                .border_b_1()
                .border_color(theme.border)
                .bg(theme.muted.opacity(0.5))
                .text_size(ui::META)
                .child(
                    Icon::new(SkyeIcon::ChartPie)
                        .size_3p5()
                        .text_color(theme.muted_foreground),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .font_weight(FontWeight::MEDIUM)
                        .child(title),
                )
                .child(
                    div().debug_selector(|| "html-card-expand".into()).child(
                        Button::new(SharedString::from(format!("html-expand-{}", tool.id)))
                            .ghost()
                            .xsmall()
                            .icon(Icon::new(SkyeIcon::Expand))
                            .tooltip("Open the page")
                            .on_click({
                                let open = open.clone();
                                move |e, w, cx| open(e, w, cx)
                            }),
                    ),
                ),
        )
        .child(
            div()
                .id(SharedString::from(format!("html-body-{}", tool.id)))
                .w_full()
                .bg(theme.background)
                .cursor_pointer()
                .on_click(move |e, w, cx| open(e, w, cx))
                .child(body),
        )
        .children(blocked)
        .into_any_element()
}

/// The live page, over the window. Escape (with GPUI focus), the close button or a click on the backdrop close it.
struct PageViewer {
    page: Option<Page>,
    error: Option<SharedString>,
    title: SharedString,
}

fn open_page(title: String, html: &str, window: &mut Window, cx: &mut App) {
    let dark = cx.theme().is_dark();
    let hosts = prefs::page_hosts(cx);
    let prepared = page::prepare(html, &page_theme(dark, cx), &hosts);
    let (page, error) = match Page::new(prepared, &hosts, window, cx) {
        Ok(page) => (Some(page), None),
        Err(e) => (None, Some(format!("{e:#}").into())),
    };
    let viewer = cx.new(|_| PageViewer {
        page,
        error,
        title: title.into(),
    });
    crate::ui::open_dialog(window, cx, move |dialog, window, _| {
        let viewer = viewer.clone();
        dialog
            .w(window.viewport_size().width * 0.92)
            .margin_top(px(24.))
            .p_2()
            .close_button(false)
            .content(move |content, _, _| content.child(viewer.clone()))
    });
}

impl Render for PageViewer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let _render = crate::debug_renders::count("PageViewer");
        // A closing dialog still draws while it fades: the native page must go at once, not over the backdrop.
        let shown = window.has_active_dialog(cx);
        if let Some(page) = &self.page {
            let _ = page.webview().read(cx).set_visible(shown);
        }
        let theme = cx.theme();
        let viewport = window.viewport_size();
        let (w, h) = (viewport.width * 0.92 - px(16.), viewport.height * 0.8);
        v_flex()
            .id("page-viewer")
            .debug_selector(|| "page-viewer".into())
            .gap_2()
            .child(
                h_flex()
                    .h(px(28.))
                    .px_1()
                    .gap_1()
                    .text_size(ui::META)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .font_weight(FontWeight::MEDIUM)
                            .child(self.title.clone()),
                    )
                    .child(
                        Button::new("page-close")
                            .ghost()
                            .xsmall()
                            .icon(Icon::new(SkyeIcon::Close))
                            .tooltip("Close")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    ),
            )
            .child(
                div()
                    .w(w)
                    .h(h)
                    .rounded(ui::RADIUS)
                    .overflow_hidden()
                    .bg(theme.background)
                    .children(self.page.as_ref().map(|p| p.webview().clone()))
                    .children(self.error.clone().map(|e| {
                        div()
                            .p_3()
                            .text_size(ui::META)
                            .text_color(theme.muted_foreground)
                            .child(e)
                    })),
            )
    }
}

/// `html_preview`: a PNG of the page at `width` for the agent, with its content height.
pub(super) fn preview(
    html: &str,
    width: u32,
    dark: bool,
    reply: Reply<Value>,
    window: &mut Window,
    cx: &mut App,
) {
    let hosts = prefs::page_hosts(cx);
    let task = page::shoot(html, &page_theme(dark, cx), &hosts, width, window, cx);
    let background = cx.background_executor().clone();
    cx.spawn(async move |_| {
        let work = async move {
            let shot = task.await?;
            let content_height = shot.content_height;
            let png = background
                .spawn(async move { skyelab_browser::encode_png(&shot.image) })
                .await?;
            use base64::Engine as _;
            anyhow::Ok(json!({
                "image": {
                    "data": base64::engine::general_purpose::STANDARD.encode(&png.png),
                    "mimeType": "image/png",
                },
                "width": width,
                "contentHeight": content_height,
                "capturedHeight": content_height.min(MAX_SHOT_HEIGHT),
                "bytes": png.png.len(),
            }))
        };
        let _ = reply.send(work.await.map_err(|e| format!("{e:#}")));
    })
    .detach();
}

#[path = "html_card_allow.rs"]
mod allow;

#[cfg(test)]
#[path = "html_card_test.rs"]
mod tests;
