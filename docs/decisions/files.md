# Decisions: Files

Part of [decisions.md](../decisions.md). The thread folder's files: Go to File, Search in Files and the Preview tab.
Each entry is dated; newest at the bottom.

- **The Preview tab edits (2026-11-25):** the Preview tab was read-only on purpose; it now edits text files after its
  Edit toggle, saved with mod+S, the way T3 Code's file panel edits. Why: fixing a typo or a line an agent wrote
  shouldn't need a trip to another editor, and T3 has it. Scope: only text previews (not images, binary files, files
  over 2 MB or the notes shown instead), and read-only until Edit is on, so a stray key in a preview never edits.
  - **Folder boundary:** a save resolves the real path (symlinks, `..`) with `files::inside` and refuses anything not
    an existing file inside the thread's folder, so a symlink pointing out is refused, never followed, and nothing new
    is created. The agent can still change the folder while a save runs (it knows SkyeLab's pid as `$PPID`), so on Unix
    the write itself never goes by path: every folder below the root is opened with `O_NOFOLLOW` (`openat`), the file
    is opened the same way, and the temp file is created next to it relative to that folder handle (`O_EXCL`, a random
    name, owner-only until it takes the file's permissions) and renamed over it with `renameat`. A link planted at the
    temp name, or a folder swapped for a link mid-save, makes the save fail; it never writes outside
    (`tests/save_tmp_symlink_escape.rs`). Limits: the root folder itself is opened by path; on Windows the save goes by
    path (a random, exclusive temp file, the folder check again right before the rename), so a folder swapped for a
    junction in that gap could still redirect it. A crash mid-save can leave a `.<name>.<random>.tmp` next to the file.
    The file is opened non-blocking and must be a regular file, so a FIFO the agent puts there fails the save (and
    the changed check) at once instead of hanging it; the compare reads at most one byte past the loaded text. A save
    keeps the file's mode and group; when the group can't be kept, its group permission bits are dropped.
  - **The agent edits the same file:** a save compares the file on disk with the text it loaded and writes nothing
    when they differ; the folder watch (the one the Changes tab uses) also re-reads the file while the tab is open.
    Either way the tab says the file changed on disk and offers Reload (drop the edits, show the disk) or Overwrite
    (write the edits anyway). Edits are never merged. One save runs at a time, so a quick second mod+S can't report
    its own first write as a change.
  - **Losing edits:** Esc, Edit off, opening another file in the tab, the tab's Close and closing the window ask Save,
    Discard or Cancel first (the tab's × hides while edits are unsaved), and the quit guard lists the file. Edits live
    only in the editor: there is no draft file, so a crash loses them.
