//! Mermaid diagrams in agent replies: a closed ```` ```mermaid ```` fence draws as a diagram, with its source a click
//! away (see `mermaid_block` for the card). Pure Rust: `mermaid-rs-renderer` lays the diagram out as SVG and resvg
//! rasterizes it to a PNG. No web view, no network, and every image href is refused: the renderer writes `<image>`
//! for some shapes, and GPUI's own SVG path would load an href from any file.

use std::sync::{Arc, LazyLock};
use std::time::Duration;

use futures::future::{Either, select};
use gpui::{App, Hsla, SharedString};
use gpui_component::text::{MarkdownNode, MarkdownParseContext, markdown_ast::Node};
use resvg::{tiny_skia, usvg};

#[cfg(test)]
#[path = "mermaid_pool_test.rs"]
mod pool_test;

/// The custom markdown node's name.
pub(super) const NODE: &str = "mermaid";
/// Longer sources are shown as code: they are slow to lay out and no reader follows them as a picture.
pub(super) const MAX_SOURCE: usize = 16 * 1024;
/// The PNG's largest side in pixels; a diagram that only fits at under half scale is shown as code.
pub(super) const MAX_SIDE: f32 = 4096.0;
/// Pixels per logical pixel, so the diagram stays sharp on a Retina screen.
const SCALE: f32 = 2.0;
/// How long a render may take before the block falls back to code.
pub(super) const RENDER_TIMEOUT: Duration = Duration::from_secs(5);

/// At most this many diagrams draw at once, each on one of this many worker threads: a reply full of them queues,
/// not a thread and a 64 MB pixmap each. A render past its timeout keeps its worker to its end, so runaways can't
/// pile up either.
const MAX_RENDERS: usize = 2;
static RENDERS: LazyLock<Pool> = LazyLock::new(|| Pool::new(MAX_RENDERS));

type Job = Box<dyn FnOnce() + Send>;

/// A fixed set of worker threads that run queued jobs in order. A panicking job is caught, so its worker stays.
pub(super) struct Pool(flume::Sender<Job>);

impl Pool {
    pub(super) fn new(threads: usize) -> Self {
        let (tx, rx) = flume::unbounded::<Job>();
        for _ in 0..threads {
            let rx = rx.clone();
            // A worker that can't start leaves the rest; with none, a render fails at once ("the renderer stopped").
            let _ = std::thread::Builder::new()
                .name("mermaid".into())
                .spawn(move || {
                    while let Ok(job) = rx.recv() {
                        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(job));
                    }
                });
        }
        Self(tx)
    }

    /// Queues `job`. The workers hold the receiver for the app's life, so the send can't fail while they run.
    pub(super) fn run(&self, job: impl FnOnce() + Send + 'static) {
        let _ = self.0.send(Box::new(job));
    }
}

/// A mermaid fence: its diagram source, and whether its closing fence has arrived (a streamed reply ends mid-fence).
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Block {
    pub source: SharedString,
    pub closed: bool,
}

/// The markdown block parser: a code node whose language is `mermaid` becomes a [`NODE`] node.
pub(super) fn parse(node: &Node, cx: &MarkdownParseContext<'_>) -> Option<MarkdownNode> {
    let block = block(node, cx.node_source(node).unwrap_or_default())?;
    let raw = cx.node_source(node).unwrap_or_default().to_owned();
    Some(
        MarkdownNode::new(NODE, block.clone())
            .text(block.source)
            .markdown(raw),
    )
}

/// The [`Block`] for `node`, whose markdown source is `raw`, when it is a mermaid fence.
pub(super) fn block(node: &Node, raw: &str) -> Option<Block> {
    let Node::Code(code) = node else { return None };
    let lang = code.lang.as_deref()?;
    if !lang.eq_ignore_ascii_case("mermaid") {
        return None;
    }
    Some(Block {
        source: code.value.clone().into(),
        closed: closed(raw),
    })
}

/// Whether a fenced block's source ends with its closing fence: a line of at least as many of the same fence
/// character as it opened with. Quote and list prefixes (`> `, indentation) are skipped.
fn closed(raw: &str) -> bool {
    let fence = |line: &str| {
        let line = line.trim_start_matches(|c: char| c == '>' || c.is_whitespace());
        let ch = line.chars().next().filter(|c| *c == '`' || *c == '~')?;
        let n = line.chars().take_while(|c| *c == ch).count();
        (n >= 3).then_some((ch, n, line[n * ch.len_utf8()..].trim().is_empty()))
    };
    let mut lines = raw.trim_end().lines();
    let (Some(open), Some(last)) = (lines.next(), lines.next_back()) else {
        return false;
    };
    match (fence(open), fence(last)) {
        (Some((ch, n, _)), Some((c, m, bare))) => c == ch && m >= n && bare,
        _ => false,
    }
}

/// The diagram's colors, from the app theme, as RGBA. Part of the render's cache key, so a theme change draws again.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub(super) struct Palette {
    /// The card the diagram sits on, opaque (edge labels are painted with it).
    pub card: u32,
    /// Node fills.
    pub node: u32,
    /// Secondary fills (notes, clusters, activations).
    pub fill: u32,
    pub text: u32,
    pub line: u32,
    pub border: u32,
}

impl Palette {
    pub fn new(card: Hsla, node: Hsla, fill: Hsla, text: Hsla, line: Hsla, border: Hsla) -> Self {
        let c = |h: Hsla| u32::from(h.to_rgb());
        Self {
            card: c(card),
            node: c(node),
            fill: c(fill),
            text: c(text),
            line: c(line),
            border: c(border),
        }
    }

    fn theme(self) -> mermaid_rs_renderer::Theme {
        let css = |c: u32| {
            let [r, g, b, a] = c.to_be_bytes();
            format!("rgba({r},{g},{b},{:.3})", f32::from(a) / 255.0)
        };
        let mut t = mermaid_rs_renderer::Theme::modern();
        // Transparent, so the card shows through once (a translucent theme color drawn twice reads darker).
        t.background = "transparent".into();
        t.edge_label_background = css(self.card);
        t.primary_color = css(self.node);
        t.tertiary_color = css(self.node);
        t.sequence_actor_fill = css(self.node);
        t.secondary_color = css(self.fill);
        t.cluster_background = css(self.fill);
        t.sequence_note_fill = css(self.fill);
        t.sequence_activation_fill = css(self.fill);
        for text in [
            &mut t.primary_text_color,
            &mut t.text_color,
            &mut t.pie_title_text_color,
            &mut t.pie_section_text_color,
            &mut t.pie_legend_text_color,
        ] {
            *text = css(self.text);
        }
        t.line_color = css(self.line);
        t.sequence_actor_line = css(self.line);
        for border in [
            &mut t.primary_border_color,
            &mut t.cluster_border,
            &mut t.sequence_actor_border,
            &mut t.sequence_note_border,
            &mut t.sequence_activation_border,
            &mut t.pie_outer_stroke_color,
        ] {
            *border = css(self.border);
        }
        t
    }
}

/// A drawn diagram: PNG bytes at [`SCALE`], and its size in logical pixels.
#[derive(Debug)]
pub(super) struct Png {
    pub bytes: Vec<u8>,
    pub width: f32,
    pub height: f32,
}

/// Lays `source` out and rasterizes it, or says why it can't be drawn. Blocking: run it off the main thread.
pub(super) fn render_png(source: &str, palette: Palette) -> Result<Png, String> {
    if source.len() > MAX_SOURCE {
        return Err(format!("too large to draw (over {} KB)", MAX_SOURCE / 1024));
    }
    let options = mermaid_rs_renderer::RenderOptions {
        theme: palette.theme(),
        layout: Default::default(),
    };
    let svg =
        mermaid_rs_renderer::render_with_options(source, options).map_err(|e| e.to_string())?;
    rasterize(&svg)
}

/// System fonts, loaded once: the first diagram pays for the scan.
static FONTS: LazyLock<Arc<usvg::fontdb::Database>> = LazyLock::new(|| {
    let mut db = usvg::fontdb::Database::new();
    db.load_system_fonts();
    Arc::new(db)
});

/// Draws `svg` at [`SCALE`] (less for a big diagram, refused below half), with no external or embedded image loaded.
pub(super) fn rasterize(svg: &str) -> Result<Png, String> {
    let options = usvg::Options {
        fontdb: FONTS.clone(),
        image_href_resolver: usvg::ImageHrefResolver {
            resolve_data: Box::new(|_, _, _| None),
            resolve_string: Box::new(|_, _| None),
        },
        ..Default::default()
    };
    let tree = usvg::Tree::from_str(svg, &options).map_err(|e| e.to_string())?;
    let (width, height) = (tree.size().width(), tree.size().height());
    let scale = SCALE.min(MAX_SIDE / width).min(MAX_SIDE / height);
    if scale < SCALE / 4.0 {
        return Err("too large to draw".into());
    }
    let mut pixmap = tiny_skia::Pixmap::new(
        (width * scale).ceil() as u32,
        (height * scale).ceil() as u32,
    )
    .ok_or("the diagram is empty")?;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    let bytes = pixmap.encode_png().map_err(|e| e.to_string())?;
    // Shown at the logical size a full-scale render would have, so a capped one is just less sharp.
    Ok(Png {
        bytes,
        width,
        height,
    })
}

/// A drawn diagram as GPUI shows it.
pub(super) struct Diagram {
    pub image: Arc<gpui::Image>,
    pub width: f32,
    pub height: f32,
}

/// The render as a GPUI asset: `window.use_asset` starts it once per (source, palette), keeps the result for the
/// session and redraws the view when it lands, so a frame never renders a diagram.
pub(super) struct Render;

impl gpui::Asset for Render {
    type Source = (SharedString, Palette);
    type Output = Result<Arc<Diagram>, SharedString>;

    fn load(
        (source, palette): Self::Source,
        cx: &mut App,
    ) -> impl Future<Output = Self::Output> + Send + 'static {
        let timeout = cx.background_executor().timer(RENDER_TIMEOUT);
        let (tx, rx) = futures::channel::oneshot::channel();
        // Its own workers, not the executor's: a render past the timeout runs on to its end, and must not hold an
        // executor thread while it does.
        RENDERS.run(move || {
            // Timed out while queued: the block already shows code.
            if !tx.is_canceled() {
                _ = tx.send(render_png(&source, palette));
            }
        });
        async move {
            let png = match select(rx, timeout).await {
                Either::Left((Ok(png), _)) => png?,
                Either::Left((Err(_), _)) => return Err("the renderer stopped".into()),
                Either::Right(_) => {
                    return Err(format!("took over {}s to draw", RENDER_TIMEOUT.as_secs()).into());
                }
            };
            Ok(Arc::new(Diagram {
                image: Arc::new(gpui::Image::from_bytes(gpui::ImageFormat::Png, png.bytes)),
                width: png.width,
                height: png.height,
            }))
        }
    }
}
