# User guide: pull requests and issues

Back to the [user guide](user-guide.md).

**Open Pull Requests** and **Open Issues** in the command palette show one page each. Every project whose folder is
bound to an account is listed under its name and account, read with that account's own `gh` login and never
another's; the repo is the project's `origin` (a github.com https or ssh URL). A project with no `origin` says
**This project has no GitHub remote** (with the `git remote add origin …` line to fix it), one whose `origin` is on
another host (GitLab, a self-hosted server) says **This project's remote isn't on GitHub**, and a folder that isn't a
git repository says so; none of them asks `gh` anything. When another project is listed too, these fold into one
line under the lists (**2 projects have no GitHub remote named origin: …**) so they don't bury it. An account with no GitHub login, or whose `gh` login has
lapsed, shows **Sign in to GitHub** and nothing is asked. Up to 50 open items per project are read, once per filter (**Mine**,
**Review requested**, **All** for PRs; **Assigned to me**, **All** for issues) until you press refresh.

- Click a PR to open it: its diff opens in a tab of the right dock (see **View diff** below). **Review with agent** on
  the row starts a thread in its project with a review prompt instead; the PR is linked to the thread when you send it.
  The arrow icon opens the PR in your browser, and the merge icon opens **Pull request actions** (below). A merged or
  closed PR has no merge icon.
- Click an issue to start a thread with the issue (title, link, labels, the start of its text) as a context chip.
  Click a label to show only that label's issues; click it again (or the **Label** button) to show all. The speech
  bubble icon opens **Issue actions** (below).

## Issue actions

The speech bubble icon on an issue row opens a small dialog that acts on GitHub with the account the project's folder
is bound to (checked again when you click), never your own `gh` login:

- **Comment** posts what you wrote.
- **Close issue** closes it as **Completed** or **Not planned** (pick one first). With text in the box the button reads
  **Close with comment**: the comment is posted, then the issue is closed. If closing then fails, the error says the
  comment was already posted, so a retry doesn't post it twice by surprise.
- A closed issue offers **Reopen issue** (or **Reopen with comment**) instead.

After an action only that issue is read again, so a closed issue stays on the list, marked closed, until you press
refresh; reopen it from there if you closed it by mistake. GitHub's errors (no triage access, signed out) show in the
dialog. The issue's text is shown as plain text only.

## Pull request actions

The merge icon on a PR row (here, or on the thread details card's pull request row while the PR is open) opens a small
dialog that acts on GitHub with the account the project's folder is bound to, never your own `gh` login:

- **Merge**: pick **Squash**, **Merge commit** or **Rebase** (only the ones the repository allows), tick **Delete the
  branch on GitHub** if you want it gone (your local checkout is never touched), then click the merge button and click
  it again to confirm. While the PR's checks are failing (or weren't read yet) it asks a third time, **Merge anyway**. The merge is pinned to
  the commit whose checks you saw: if someone pushed since, GitHub refuses it, and if that commit wasn't read the
  dialog asks you to refresh first. GitHub's own branch rules still apply: SkyeLab never merges as an admin.
- **Review**: **Approve**, or write a comment and **Comment** or **Request changes** (both need text, unless line
  comments from the diff go along).
- **View diff** opens the PR's diff in a tab of the right dock, with the same file tree, split/unified and find as the
  Changes tab. It is read with `gh pr diff` under the same account, and the PR's head commit is read before and after,
  so a push landing meanwhile shows an error instead of a mixed diff; **Refresh** reads it again. The diff is shown as
  plain text only. Click a line number (or drag across several) to comment on those lines; comments stay pending on the
  tab until **Submit review…** opens this dialog with them, and the verdict you pick there sends them all as one
  GitHub review, placed on the commit the diff was read at. A review with line comments needs no summary. A line
  GitHub doesn't count as part of the diff makes it refuse the whole review; its message shows in the dialog and the
  comments stay pending.

While the merge runs its button reads **Merging…**. If GitHub refuses, its message shows in red and the dialog stays
open. After an action the dialog closes, a notice says **Merged pull request #12** (or **Review sent on pull request
#12**), and only that PR is read again: the details card shows its new state, and on the Pull requests page its row
shows **Merged** (it stays listed until you press refresh). A tooltip that was showing when the dialog opened is
hidden while the dialog is up.
