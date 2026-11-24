# Adding projects

Part of the [user guide](user-guide.md).

**Add project** (the palette, the project filter, or New thread with no project) opens a palette page, as in T3:

- **Type a path** (`~/code/app`, `/srv/x`): the folder's subfolders list as you type, each with its git branch when it
  is a repo. ↑/↓ move, **Tab** completes the highlighted folder into the field, **Enter** on a folder opens it, and
  **Enter** on the top row (**Add ~/code/app**) adds the folder you typed.
- With nothing typed, **Recent** lists folders your accounts' agents worked in that aren't projects yet.
- **New project** takes a name (it goes next to your current project, else in `~/code`, else home) or a full path,
  then creates the folder and runs `git init`. A folder that already has files is refused.
- **Clone from URL** takes a plain `https://` URL (no ssh, no login in the URL). The clone runs with the chosen
  account's own GitHub login, or with no login at all (public repos only), never your default `gh` or keychain. A
  failed clone leaves nothing behind and binds nothing.
- **Browse…** is the system folder picker, for when you'd rather click.

Every way in ends on the same question, **Which account does <folder> run on?**: your accounts, **Add account…**
(Settings → Accounts) and **Leave unbound** (agents then ask before they start). A folder that already runs on an
account (through its own or a parent's binding) is added on that account without asking.
