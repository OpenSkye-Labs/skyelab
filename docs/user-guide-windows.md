# Windows and quitting

Part of the [user guide](user-guide.md).

On Windows, minimize, maximize and close sit in the top-right corner of the window's top row, where the page's
own title is; drag the empty part of that row (the SkyeLab name above the sidebar, the page title) to move the window,
and double-click it to maximize. Shortcut hints read Ctrl, Shift and Enter where macOS shows ⌘, ⇧ and ↩.

## Several windows

**New Window** (<kbd>⇧⌘N</kbd>, <kbd>Ctrl+Shift+N</kbd> on Linux and Windows, the palette, or the SkyeLab menu on
macOS) opens another window on the same projects, threads and accounts. Each window has its own page, project filter,
docks and terminals. A thread open in two windows is the same thread with one agent: both windows show it as it runs,
and its message input follows the window that opened it last, with its text and files (undo starts over there); the other window shows the draft, and clicking it brings the input there. A notification for it comes once. Saving accounts in
Settings applies to every window, and a window catches up with projects and threads changed elsewhere when you switch
to it. Each window scrolls a thread on its own. Every open window saves its frame and page, and the next launch opens all
of them again; a window you close leaves that list, except the last one, so a launch always has a window to reopen (quitting keeps every window).
Closing the last window quits on Linux and Windows (unless a thread is working, see below). On macOS SkyeLab keeps
running, agents included, until you quit (<kbd>⌘Q</kbd>, or Quit SkyeLab in the Dock icon's menu); click its Dock
icon to open the saved windows again. Delegated threads, scheduled tasks and notifications keep working with no
window open.

## Quitting

Quitting stops every agent and terminal command. So while a thread or side chat is working or waiting for you (an
approval, a question, a plan), a thread holds diff comments or context chips you haven't sent (they live in memory
only), or a terminal runs a command, Quit (<kbd>⌘Q</kbd>, the menu, or closing the last window on Linux and
Windows) first asks, listing them. **Quit** quits, **Cancel** keeps everything running, and pressing <kbd>⌘Q</kbd> again
while it asks quits too. Tick **Don't ask again** to quit at once from then on; **Quit confirmation** in Settings →
General turns the question back on. With nothing in flight it quits at once. Windows, drafts and thread state are saved
either way.

## Keep running in background

While a thread is working or waiting for you, the question also offers **Keep running in background** (unless a
Preview tab holds unsaved edits). It closes every window but keeps SkyeLab and its agents running: turns finish and
are saved, delegated threads and scheduled tasks start, and tailnet sharing stays on if it was on (a new device's
request waits until a window opens). An approval or question shows a system notification; clicking it opens a window
on that thread. Open SkyeLab again (its Dock icon on macOS, or launch it again anywhere) to get the saved windows
back with the threads as they are now. With **Don't ask again** ticked, Keep running becomes the answer every time;
**Always keep running in background** in Settings → General sets it too, and Restore defaults turns it off. On
Linux and Windows closing the last window asks the same way. Side chats and terminal commands stop when their window
closes.

In the background SkyeLab quits by itself once no thread has been working, starting or waiting for you for 30
seconds (a scheduled task due within the next 15 seconds keeps it up too). If you armed **Update when threads finish**,
that idle exit installs the staged update and relaunches SkyeLab on the new version instead.

## Resuming interrupted turns

If SkyeLab crashes, is killed, or restarts while a local thread was mid-turn, the next launch automatically resumes
that thread's session once with a short continuation message ("SkyeLab restarted and resumed this turn…"), using the
thread's saved session and bound account. Threads explicitly stopped by Stop or a closed tab, threads waiting on a
permission, question or plan approval, and turns that were already auto-resumed once without finishing are left as
interrupted. **Settings → General → Resume interrupted turns after restart** turns this off (on by default; **Restore
defaults** turns it back on).

## One SkyeLab at a time

Opening SkyeLab again while it runs brings the running one forward (or opens a window in it) instead of starting a
second copy, which would run scheduled tasks and resume threads twice. A build from source keeps its own data
(`skyelab-dev`), so it runs next to the installed app with its own threads.

<kbd>⌘W</kbd> (<kbd>Ctrl+W</kbd> on Linux and Windows) closes the right panel's active tab, as in T3 Code; in a focused terminal it closes that terminal instead (<kbd>Ctrl+Shift+W</kbd> off macOS). Right-clicking a right-panel tab offers **Close tab**, **Close other tabs**, and **Close all tabs**, leaving unclosable tabs open and prompting before closing a Preview tab with unsaved edits. It never closes the window: on macOS use File → Close Window or the window's close button.

On a narrow window the right panel gives up width so the thread keeps at least 360 pt. A window too narrow for both
puts the panel away when it opens or shrinks (its width is kept for later); opening the panel yourself then hides the
sidebar to make room.
