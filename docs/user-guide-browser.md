# User guide: browser tabs

## Browser tabs

<kbd>⌥⌘B</kbd> (<kbd>Ctrl+Alt+B</kbd>) opens a browser tab in the right panel. Type a `localhost` address, any http or https address, or a
local file path in its address bar. The tab is named after the page's title (an agent's tab: **Agent:** and the
title), and so is its **Go to tab** entry in the palette. While the tab has the focus (its address bar, or right after you open or click
the tab):

- <kbd>⌘+</kbd> / <kbd>⌘−</kbd> zoom the page in and out, <kbd>⌘0</kbd> puts it back to its own size
  (<kbd>Ctrl</kbd> on Linux and Windows). The toolbar shows the zoom level when it isn't 100%; click it to reset.
- <kbd>⌘F</kbd> (<kbd>Ctrl+F</kbd>) opens a find bar under the toolbar. Typing selects the first match and scrolls to
  it, <kbd>Enter</kbd> or the arrows go to the next or previous one (wrapping round), and **No matches** shows when
  there is none. <kbd>Esc</kbd> closes it. The search runs inside the page, so the page's own scripts can see
  what you search for.
- The **⋯** button at the end of the toolbar has **Reload**, **Duplicate tab** (a new tab of yours on the same page,
  even when an agent drives this one; see below for an agent's page), **Copy URL**, **Open in system browser** (http
  and https pages only, never an agent's page), find and
  zoom, and, in development builds, **Open DevTools**, which opens the page's inspector in a window of its own.
  The page hides while that menu is open, because it would otherwise draw over the menu.

A page that doesn't load (nothing answering on the address, a name that doesn't resolve, a certificate that isn't
trusted) shows **This site can't be reached** with the address, the reason, the engine's error code and **Retry**. For
a `localhost` address it also says that no dev server seems to be running: start it, then press **Retry** (or ↻).
**←** goes back to the page you were on. A link the page engine can't open (`mailto:`, another app's link) leaves the
page as it was. The same error shows in an agent's tab, and the agent's `browser_navigate` answers with the reason
instead of waiting. On Linux and Windows the page engine shows its own error page instead.

Right-clicking inside the page shows the page engine's own menu, not SkyeLab's. After you click into the page, the
shortcuts may reach the page instead: click the address bar first, or use the **⋯** menu.

## Agent browser tabs

An agent can open a browser tab of its own with SkyeLab's `browser_open` tool, then `browser_navigate`, read
`browser_url`, run `browser_eval` (a JavaScript expression; the JSON result comes back, up to 64 KiB, within 10
seconds) and take a `browser_screenshot` (a PNG of the visible page, at most 1600 px wide and 1 MiB; untested so far on Linux). SkyeLab doesn't keep image bytes in its thread history: the tool row shows **Image (205 KB)**
while the thread is open, and **Image (205 KB) not kept** after a restart. The same goes for any image or PDF a tool
returns. Agents may only load http and https pages, including `localhost` dev servers, and never run scripts in a
local file page. Each thread has one tab, titled **Agent browser**, with a bar saying the agent controls it:

- **Take over** makes it your tab. The agent can't reach it any more; its next `browser_open` opens a new tab.
- **Close** closes the tab.

An agent can't see or drive the tabs you open with <kbd>⌥⌘B</kbd>, or another thread's tab. Its tab has private,
in-memory cookies and storage, so it is not signed in to anything you are, and nothing it sets reaches your tabs.
Take over reloads the page, which stops any script the agent left running. While the agent still has the tab,
don't sign in to anything in it: the agent can read that page. macOS and Linux only.

An agent's page, or any other private tab (a forwarded port's page), never moves into your own browser profile.
**Duplicate tab** on one (an agent's tab before or after Take over, or a copy of one) opens a private, throwaway tab:
its own in-memory cookies and storage, gone when it closes. **Reopen Closed** never brings these tabs back, and
**Open in system browser** is off for them. A private tab's page also stays on the web: it can't go to a
`file:` page or open another app through its link scheme, a download is refused, and a link or script that opens a
new window opens the page as another private tab instead (web pages only). To open the
page in your profile anyway, use **Copy URL** and paste it into a tab of yours. Your own tabs duplicate into your
profile as before.
