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
Closing the last window quits on Linux and Windows. On macOS SkyeLab keeps running, agents included, until you quit
(<kbd>⌘Q</kbd>); click its Dock icon to open a window again.

## Quitting

Quitting stops every agent and terminal command. So while a thread or side chat is working or waiting for you (an
approval, a question, a plan), a thread holds diff comments or context chips you haven't sent (they live in memory
only), or a terminal runs a command, Quit (<kbd>⌘Q</kbd>, the menu, or closing the last window on Linux and
Windows) first asks, listing them. **Quit** quits, **Cancel** keeps everything running, and pressing <kbd>⌘Q</kbd> again
while it asks quits too. Tick **Don't ask again** to quit at once from then on; **Quit confirmation** in Settings →
General turns the question back on. With nothing in flight it quits at once. Windows, drafts and thread state are saved
either way.

## One SkyeLab at a time

Opening SkyeLab again while it runs brings the running one forward (or opens a window in it) instead of starting a
second copy, which would run scheduled tasks and resume threads twice. A build from source keeps its own data
(`skyelab-dev`), so it runs next to the installed app with its own threads.

<kbd>⌘W</kbd> (<kbd>Ctrl+W</kbd> on Linux and Windows) closes the right panel's active tab, as in T3 Code; in a focused terminal it closes that terminal instead (<kbd>Ctrl+Shift+W</kbd> off macOS). It never closes the window: on macOS use File → Close Window or the window's close button.
