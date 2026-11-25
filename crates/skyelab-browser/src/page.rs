//! An agent's visual reply (`html_render`): an untrusted page shown in a locked, private, throwaway webview.
//!
//! - The page loads from a string (`loadHTMLString` with no base URL): an opaque `about:blank` origin with no file
//!   access. Every later navigation, new window and download is refused, there is no IPC handler and no custom
//!   scheme, and each page gets its own in-memory web data store (on Windows a fresh temp folder).
//! - [`prepare`] puts a doctype and a Content-Security-Policy meta first in the document, so it governs everything after it:
//!   inline scripts and styles, scripts, styles and fonts from the user's allowed hosts only (Settings → Chat →
//!   Visual replies, `skyelab_core::page_hosts`), images and media from `data:`/`blob:` only; no fetch, XHR,
//!   WebSocket, frames, workers, plugins, forms or `<base>`. A script of ours after it records what the policy
//!   refused, so the card can offer to allow a host; the user decides, never the agent.
//! - On Windows, `skyelab_webview2_sys::lock` enforces the same again under the CSP before the page loads: WebView2
//!   answers every other request itself (a 403) and refuses new windows on the UI thread.
//!   Its own browser process also fails to resolve local names and addresses, so preconnects and hints that
//!   no lock or CSP sees open no socket to this machine or its networks.
//! - Then T3's theme variables and base sheet, which the page may override.
//! - [`shoot`] lays a page out hidden at a width, measures it and snapshots it; the timeline shows that image, so
//!   no native view ever sits inside a scrolling list. The live page is only for the expand dialog.

use std::time::Duration;

use anyhow::{Context as _, anyhow};
use futures::{FutureExt as _, channel::oneshot};
use gpui::{App, AppContext as _, Entity, Task, Window};
use gpui_webview::WebView;

use skyelab_core::page_hosts::{parse_host, valid_hosts};

use crate::{Engine as _, engine::Wry};

/// The policy a page runs under, with `hosts` (the setting's list; entries `page_hosts` refuses are dropped) as the
/// only places scripts, styles and fonts may load from. A meta CSP can't loosen one that is already there, and
/// nothing in the page comes before it.
pub fn csp(hosts: &[String]) -> String {
    let from: String = valid_hosts(hosts)
        .iter()
        .map(|h| format!(" https://{h}"))
        .collect();
    format!(
        "default-src 'none'; script-src 'unsafe-inline' 'unsafe-eval'{from}; style-src 'unsafe-inline'{from}; \
         font-src data:{from}; img-src data: blob:; media-src data: blob:; connect-src 'none'; frame-src 'none'; \
         child-src 'none'; worker-src 'none'; object-src 'none'; manifest-src 'none'; form-action 'none'; \
         base-uri 'none'"
    )
}

/// Ours, right after the policy, so it runs before anything of the page's: records the address of each script,
/// style or font the policy refused, for [`blocked_hosts`]. The page can lie to it; what it says is only a hint.
/// It uses only functions it took before the page ran and keeps printable ASCII (at most 20 addresses of 190
/// characters), so whatever the page replaces, the recorded string stays one the engine's bridge can encode.
const WATCH_BLOCKED: &str = "<script>(function(){var ap=Reflect.apply,cc=String.prototype.charCodeAt,\
    fc=String.fromCharCode,io=String.prototype.indexOf,s='',n=0;function clean(v){var u=String(v),o='';\
    for(var i=0;i<u.length&&i<190;i++){var c=ap(cc,u,[i]);if(c>32&&c<127)o+=ap(fc,null,[c])}return o}\
    document.addEventListener('securitypolicyviolation',function(e){try{var d=clean(e.effectiveDirective);\
    if(n<20&&(ap(io,d,['script-src'])===0||ap(io,d,['style-src'])===0||ap(io,d,['font-src'])===0))\
    {s+=' '+clean(e.blockedURI);n++}}catch(x){}},true);\
    Object.defineProperty(window,'__skyelabBlocked',{value:function(){return s}})})()</script>";

/// At most this many blocked hosts are offered on a card.
const MAX_BLOCKED: usize = 5;

/// The hosts in `reported` (what the page's [`WATCH_BLOCKED`] says it was refused: untrusted) worth offering: https
/// addresses whose host `page_hosts` accepts and `allowed` doesn't hold yet, each once, at most [`MAX_BLOCKED`].
pub fn blocked_hosts(reported: &str, allowed: &[String]) -> Vec<String> {
    let allowed = valid_hosts(allowed);
    let mut out: Vec<String> = Vec::new();
    for word in reported.split_whitespace().take(20) {
        let Ok(url) = url::Url::parse(word) else {
            continue;
        };
        let Some(host) = url
            .host_str()
            .filter(|_| url.scheme() == "https" && url.port().is_none())
            .and_then(|h| parse_host(h).ok())
            .filter(|h| !h.starts_with('*'))
        else {
            continue;
        };
        if !allowed.contains(&host) && !out.contains(&host) && out.len() < MAX_BLOCKED {
            out.push(host);
        }
    }
    out
}

/// The page's WebView2 browser arguments: wry's defaults, plus every name and address on this machine or its networks
/// failing to resolve. Chromium opens sockets the lock never sees (`<link rel=preconnect>` is outside CSP, and its
/// speculative preconnects), and a bare socket is enough to probe a local port; a failed resolve opens none,
/// whatever asked for it, IP literals included. Glob patterns, not CIDRs: `100.*` and `172.*` cover the tailnet's
/// 100.64/10 and 172.16/12 with some public addresses beside them. Names that resolve to a local address in DNS
/// still get through. The page's environment has a data folder of its own, so the user's tabs keep their own.
#[cfg(windows)]
const PAGE_BROWSER_ARGS: &str = "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection \
    --host-resolver-rules=\"MAP localhost ^NOTFOUND, MAP *.localhost ^NOTFOUND, MAP *.local ^NOTFOUND, \
    MAP *.internal ^NOTFOUND, MAP *.home.arpa ^NOTFOUND, MAP *.ts.net ^NOTFOUND, MAP 0.* ^NOTFOUND, \
    MAP 10.* ^NOTFOUND, MAP 100.* ^NOTFOUND, MAP 127.* ^NOTFOUND, MAP 169.254.* ^NOTFOUND, MAP 172.* ^NOTFOUND, \
    MAP 192.168.* ^NOTFOUND, MAP :: ^NOTFOUND, MAP ::1 ^NOTFOUND, MAP ::ffff:* ^NOTFOUND, MAP fc*:* ^NOTFOUND, \
    MAP fd*:* ^NOTFOUND, MAP fe8*:* ^NOTFOUND, MAP fe9*:* ^NOTFOUND, MAP fea*:* ^NOTFOUND, MAP feb*:* ^NOTFOUND\"";

/// T3's base sheet: an 8px gutter, theme background, text and font, focus rings inside the element.
const BASE_CSS: &str = "html{box-sizing:border-box;padding:8px;background:var(--background);\
    color:var(--foreground);font-family:var(--font-sans);font-size:14px;line-height:1.5;\
    -webkit-font-smoothing:antialiased;-webkit-text-size-adjust:100%}body{margin:0}\
    code,kbd,pre,samp{font-family:var(--font-mono)}:where(:focus-visible){outline:2px solid var(--ring)}";

/// The tallest snapshot, in CSS pixels (T3's largest frame); the expand dialog shows the rest.
pub const MAX_SHOT_HEIGHT: u32 = 2000;
/// Smallest frame, and the height a page is first laid out at, so a short page measures as its content.
pub const MIN_SHOT_HEIGHT: u32 = 80;
/// A shot gives up after this.
pub const SHOT_TIMEOUT: Duration = Duration::from_secs(10);
/// After the load, before measuring: lets scripts draw and a chart's entry animation mostly finish.
const SETTLE: Duration = Duration::from_millis(600);

/// The theme a page gets: `--name` → CSS value pairs, T3's names (`--background`, `--chart-1`, ...).
#[derive(Debug, Clone, PartialEq)]
pub struct PageTheme {
    pub dark: bool,
    pub vars: Vec<(&'static str, String)>,
}

/// The document the webview loads: `html` with the CSP (from `hosts`), our watch and the theme put in front of it.
pub fn prepare(html: &str, theme: &PageTheme, hosts: &[String]) -> String {
    let mut root = format!(
        ":root{{color-scheme:{};",
        if theme.dark { "dark" } else { "light" }
    );
    for (name, value) in &theme.vars {
        // Our own values, but a font name comes from settings: nothing that ends the rule or the tag.
        let value: String = value
            .chars()
            .filter(|c| !matches!(c, ';' | '{' | '}' | '<' | '>'))
            .collect();
        root.push_str(&format!("{name}:{value};"));
    }
    root.push('}');
    let head = format!(
        "<meta http-equiv=\"Content-Security-Policy\" content=\"{}\">{WATCH_BLOCKED}\
         <style id=\"skyelab-theme\">{root}{BASE_CSS}</style>",
        csp(hosts)
    );
    // Ours always comes first: anything of the page's before the meta (even a character the HTML parser doesn't
    // count as space, like U+000B) would open <body>, and a CSP meta outside <head> is ignored. The page's own
    // doctype, after ours, is ignored too.
    format!("<!doctype html>{head}{html}")
}

/// A page in its own locked webview, hidden until the host shows it.
pub struct Page {
    webview: Entity<WebView>,
    loaded: flume::Receiver<()>,
    _data_dir: Option<tempfile::TempDir>,
}

impl Page {
    /// Builds the webview on `html` (already [`prepare`]d with `hosts`) in `window`, hidden. `hosts` is the same
    /// list: on Windows the lock follows it too.
    pub fn new(
        prepared: String,
        hosts: &[String],
        window: &mut Window,
        cx: &mut App,
    ) -> anyhow::Result<Self> {
        let override_env = std::env::var_os("WEBVIEW2_USER_DATA_FOLDER");
        let data_dir = crate::webview_data_dir(cfg!(windows), true, override_env.as_deref())?;
        let mut context = data_dir
            .as_ref()
            .map(|d| wry::WebContext::new(Some(d.path().to_owned())));
        let builder = match context.as_mut() {
            Some(context) => wry::WebViewBuilder::new_with_web_context(context),
            None => wry::WebViewBuilder::new(),
        };
        let (loaded_tx, loaded) = flume::unbounded();
        // On Windows the page loads only once the WebView2 lock is on (below), so its first request is covered.
        #[cfg(not(windows))]
        let builder = builder
            .with_html(prepared)
            .with_new_window_req_handler(|_, _| wry::NewWindowResponse::Deny);
        #[cfg(windows)]
        let builder = {
            use wry::WebViewBuilderExtWindows as _;
            builder.with_additional_browser_args(PAGE_BROWSER_ARGS)
        };
        let builder = builder
            .with_incognito(true)
            .with_devtools(false)
            // Never the keyboard: on Windows a page being shot is shown (off the window) while it loads.
            .with_focused(false)
            .with_visible(false)
            // The string load is the one navigation; a link, a script or a meta refresh goes nowhere. It is the first
            // one, started before the page has any script, and WebView2 names it `data:text/html…`, not `about:blank`.
            .with_navigation_handler({
                let first = std::sync::atomic::AtomicBool::new(true);
                move |url| {
                    first.swap(false, std::sync::atomic::Ordering::SeqCst) || url == "about:blank"
                }
            })
            .with_download_started_handler(|_, _| false)
            .with_on_page_load_handler(move |event, _| {
                if matches!(event, wry::PageLoadEvent::Finished) {
                    let _ = loaded_tx.send(());
                }
            });
        let webview = WebView::build(builder, window, cx).context("create the page's webview")?;
        // wry refuses new windows from a thread of its own, where WebView2 ignores the answer (the default popup
        // opens); without a handler it refuses on the spot, and the lock refuses too and blocks every request the
        // CSP wouldn't allow.
        #[cfg(windows)]
        {
            use wry::WebViewExtWindows as _;
            skyelab_webview2_sys::lock(&webview.raw().webview(), valid_hosts(hosts))
                .context("lock the page")?;
            webview
                .raw()
                .load_html(&prepared)
                .context("load the page")?;
        }
        let _ = webview.set_visible(false);
        #[cfg(not(windows))]
        let _ = hosts;
        Ok(Self {
            webview: cx.new(|_| webview),
            loaded,
            _data_dir: data_dir,
        })
    }

    /// The webview, to draw (the expand dialog) or show and hide.
    pub fn webview(&self) -> &Entity<WebView> {
        &self.webview
    }
}

/// A page's snapshot: the engine's image (TIFF on macOS, PNG on Linux and Windows), the page's full height in CSS
/// pixels, and the hosts its scripts, styles or fonts were refused from ([`blocked_hosts`]).
pub struct PageShot {
    pub image: Vec<u8>,
    pub content_height: u32,
    pub blocked: Vec<String>,
}

impl PageShot {
    /// The image as a PNG at full size (the card draws it at the screen's scale). Slow: off the UI thread.
    pub fn to_png(&self) -> anyhow::Result<Vec<u8>> {
        let image = image::load_from_memory(&self.image).context("decode the snapshot")?;
        let mut png = Vec::new();
        image::DynamicImage::ImageRgba8(image.to_rgba8())
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .context("encode the PNG")?;
        Ok(png)
    }
}

/// Lays `html` out hidden at `width` CSS pixels and snapshots it, at most [`MAX_SHOT_HEIGHT`] tall. The webview is
/// dropped when the task ends. On Windows the page is shown but parked outside the window (see [`SHOT_X`]).
pub fn shoot(
    html: &str,
    theme: &PageTheme,
    hosts: &[String],
    width: u32,
    window: &mut Window,
    cx: &mut App,
) -> Task<anyhow::Result<PageShot>> {
    let allowed = hosts.to_vec();
    let page = match Page::new(prepare(html, theme, hosts), hosts, window, cx) {
        Ok(page) => page,
        Err(e) => return Task::ready(Err(e)),
    };
    if cfg!(windows)
        && let Err(e) = page.webview.read(cx).set_visible(true)
    {
        return Task::ready(Err(e).context("show the page off the window"));
    }
    let executor = cx.background_executor().clone();
    let timeout = executor.timer(SHOT_TIMEOUT);
    cx.spawn(async move |cx| {
        let work = async {
            set_size(&page, width, MIN_SHOT_HEIGHT, cx)?;
            page.loaded
                .recv_async()
                .await
                .map_err(|_| anyhow!("the page never finished loading"))?;
            executor.timer(SETTLE).await;
            let content_height = measure(&page, cx).await?;
            let blocked = blocked_hosts(&eval(&page, READ_BLOCKED, cx).await?, &allowed);
            set_size(
                &page,
                width,
                content_height.clamp(MIN_SHOT_HEIGHT, MAX_SHOT_HEIGHT),
                cx,
            )?;
            executor.timer(Duration::from_millis(200)).await;
            let (tx, rx) = oneshot::channel();
            cx.update(|cx| {
                Wry(page.webview.clone()).snapshot(Box::new(move |r| _ = tx.send(r)), cx)
            })?;
            let image = rx
                .await
                .context("the webview dropped the snapshot")?
                .map_err(|e| anyhow!("snapshot failed: {e}"))?;
            anyhow::Ok(PageShot {
                image,
                content_height,
                blocked,
            })
        };
        futures::select! {
            r = work.fuse() => r,
            () = timeout.fuse() => Err(anyhow!("the page did not render in {}s", SHOT_TIMEOUT.as_secs())),
        }
    })
}

/// Where a page being shot sits. WebView2 paints nothing while hidden, so on Windows the page is shown, far left of
/// the window (a child view outside its parent is clipped away). A hidden WKWebView still renders; WebKitGTK is untried.
const SHOT_X: f64 = if cfg!(windows) { -30_000.0 } else { 0.0 };

fn set_size(page: &Page, width: u32, height: u32, cx: &mut gpui::AsyncApp) -> anyhow::Result<()> {
    let rect = wry::Rect {
        position: wry::dpi::LogicalPosition::new(SHOT_X, 0.0).into(),
        size: wry::dpi::LogicalSize::new(f64::from(width), f64::from(height)).into(),
    };
    cx.read_entity(&page.webview, |w, _| w.set_bounds(rect))
        .context("size the page")
}

/// The page's height. The script returns a number it made itself: wry's macOS bridge aborts the app on a result
/// it can't encode (NaN), and the page owns every getter it reads.
async fn measure(page: &Page, cx: &mut gpui::AsyncApp) -> anyhow::Result<u32> {
    const SCRIPT: &str = "(function(){try{var h=document.documentElement.scrollHeight|0;return h>0?h:0}\
        catch(e){return 0}})()";
    let raw = eval(page, SCRIPT, cx).await?;
    Ok(raw.trim().parse::<u32>().unwrap_or(0).max(MIN_SHOT_HEIGHT))
}

/// What [`WATCH_BLOCKED`] recorded, as a JSON string. Only a primitive string of bounded length gets out (`typeof`
/// and a primitive's `length` are the page's to read but not to change), else an empty one.
const READ_BLOCKED: &str = "(function(){try{var s=window.__skyelabBlocked();\
    return typeof s==='string'&&s.length<4000?s:''}catch(e){return ''}})()";

/// `script`'s raw result: the engine's JSON for it. A string result is decoded; anything else comes back as is.
async fn eval(
    page: &Page,
    script: &'static str,
    cx: &mut gpui::AsyncApp,
) -> anyhow::Result<String> {
    let (tx, rx) = oneshot::channel();
    let tx = std::sync::Mutex::new(Some(tx));
    cx.read_entity(&page.webview, |w, _| {
        w.raw().evaluate_script_with_callback(script, move |raw| {
            if let Some(tx) = tx.lock().ok().and_then(|mut t| t.take()) {
                let _ = tx.send(raw);
            }
        })
    })
    .context("run a script in the page")?;
    let raw = rx
        .await
        .context("the webview dropped the script's result")?;
    Ok(serde_json::from_str::<String>(&raw).unwrap_or(raw))
}

#[cfg(test)]
mod tests;
