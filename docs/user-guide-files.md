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
  ("500+" when there are more). Dotfiles are searched (not the `.git` folder); binary files, ignored files and files over 2 MB are skipped. Click a match to open the
  file at that line.
- **Preview**: a tab with line numbers and syntax highlighting, scrolled to the line asked for. Images show
  as images (an SVG shows as its text, since drawing one can load files from elsewhere); a binary file or a file over
  2 MB (images: 16 MB) shows a note instead. The button at the top right opens the file in your editor. One Preview
  tab is reused for each file you open. Click an image to view it large (zoom, <kbd>Esc</kbd> closes).
  <kbd>⌘F</kbd> in a text preview opens the find bar over the file: it counts the matches (**2/7**, ignoring case),
  marks them in the text, and <kbd>↩</kbd> / <kbd>⇧↩</kbd> go to the next and previous one, wrapping around.
  <kbd>Esc</kbd> closes it.
- **Editing in the Preview**: a text file opens read-only. The pencil button in its header turns editing on;
  <kbd>⌘S</kbd> saves, and a dot after the file's name (in the header and on the tab) means there are unsaved edits.
  <kbd>Esc</kbd> or the pencil again stops editing. With unsaved edits, stopping, opening another file in the tab,
  closing the tab or closing the window first asks **Save**, **Discard** or **Cancel**, and quitting lists the file.
  Find (<kbd>⌘F</kbd>) works while editing. Saves only write files inside the thread's folder (a symlink pointing
  outside it is refused) and keep the file's permissions. If the file changed on disk since it opened (the agent or
  git wrote it), a save writes nothing and the tab says so: **Reload** shows the file as it is now and drops your
  edits, **Overwrite** saves your edits over it. Images, binary files and files over 2 MB can't be edited.

File paths in the agent's replies open in the Preview tab too; see [What the timeline shows](user-guide-using-threads.md#what-the-timeline-shows).
