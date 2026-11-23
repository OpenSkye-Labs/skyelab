# Files

Shortcuts are shown for macOS. On Linux and Windows, use <kbd>Ctrl</kbd> for <kbd>⌘</kbd>.

Each of these works on the shown thread's folder (its worktree, if it has one), or the project's folder when no
thread is open. Nothing outside that folder is listed, searched or shown: paths are checked after resolving symlinks,
and the search never follows a symlink. They aren't available while the window shows another machine.

- **Go to File** (<kbd>⌘P</kbd>): type a few letters of a file's name or path. It lists the files git tracks plus new
  files that aren't ignored (a folder that isn't a git repo lists its files, skipping ignored ones), at most 200 rows.
  <kbd>↩</kbd> opens the file in the Preview tab; <kbd>⌘↩</kbd> opens it in your editor. In a terminal on Linux and
  Windows, <kbd>Ctrl+P</kbd> stays the shell's; use the palette there.
- **Search in Files** (<kbd>⇧⌘F</kbd>): a **Search** tab in the right panel. Type to search file contents as plain text
  (case-insensitive unless you type a capital). Matches are grouped by file with their line numbers, up to 500 lines
  ("500+" when there are more). Binary files, ignored files and files over 2 MB are skipped. Click a match to open the
  file at that line.
- **Preview**: a read-only tab with line numbers and syntax highlighting, scrolled to the line asked for. Images show
  as images; a binary file or a file over 2 MB (images: 16 MB) shows a note instead. The button at the top right opens
  the file in your editor. One Preview tab is reused for each file you open.

File paths in the agent's replies open in the Preview tab too; see [What the timeline shows](user-guide.md#what-the-timeline-shows).
