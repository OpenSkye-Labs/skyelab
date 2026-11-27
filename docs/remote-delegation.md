# Delegation to other machines

An agent on machine A can list projects on, start threads on, and read, wait on, message and stop the threads it
started on the user's own tailnet machine B, through SkyeLab's MCP tools. B decides and enforces everything about its
own side: the account (its own folder binding), the caps, and which callers it accepts. When either side is unsure, it
refuses. The wire call is `delegate` (protocol 1.8); the local tools are in [orchestration.md](orchestration.md).

## Turning it on: two opt-ins, one per machine

- **On A**, B must already be a linked machine (its threads show in A's list). Settings → Machines → Delegation has a
  **Let my agents delegate here** switch per linked machine, off by default (`delegation.targets` in A's
  `settings.toml`).
- **On B**, the first call from A is refused and listed in Settings → Sharing & access → **Delegation requests**:
  "Agents on `a-mac` under `work` (me@acme.com · org) want to start threads in folders bound to *Work* here". **Allow**
  saves a link once per (A's node, A's device key, A's account) → B's account (`delegation.links` in B's
  `settings.toml`); **Deny** refuses that caller, unlisted, until sharing restarts. **Allowed delegation** lists the
  links, each with **Revoke**. A request whose login matches no Claude account on B says so and has no Allow. Device
  approval never creates a link, and revoking the device refuses every delegated call from it (its hello fails).

## The same account, proven twice on every call

Account ids are local names, so `work` on A means nothing on B. B accepts a call only when:

1. **B's user linked** A's (node, device-key hash, account X) to B's account Y, and
2. **the logins match now:** A sends X's login `{provider, email, org_id}` as A's own `claude auth status` reports it,
   B reads Y's the same way, and both must equal each other and the login saved in the link. A login that can't be
   read, has no org id (an API key) or isn't Claude refuses. Both sides cache a read login for up to 10 minutes.

**B's binding decides.** A start runs under the account B's `accounts.toml` binds the folder to, through B's normal
launch (re-read before spawn); if that isn't Y, it is refused. There is never a default account. Codex and Cursor
accounts are refused across machines: their status names no org.

B takes A's node and device-key hash from the connection itself (whois and the approved hello), never from the
message. The caller's thread id, title, account, login and depth are A's word, trusted because A's device key was
approved; the link adds the user's intent and the login check catches a rebind on either side.

## Limits

- **Depth:** A sends its caller's depth; B refuses at 2 and stores the child's depth, so a B child counts the whole
  chain when it delegates again (locally, back to A, or elsewhere).
- **Children:** on A, a parent's remote children count with its local ones against 4, for every start, resume and
  follow-up, local or remote. A keeps each remote child's last known status (read again before a remote start); one
  whose status can't be read (B offline) counts as busy, one B no longer has doesn't. On B, at most 4 busy children per (A node, A store, A parent) and 8 per A node, claimed
  under B's store lock at every claim site (a start, a resume, a restart-resume, a follow-up to an idle child).
- **Sizes**, checked by B's daemon before anything else: prompt and text 100 000 chars, title 200, ids 128, email 320,
  org id 64, one wait 20 s. Each call takes a `max_in_flight` permit. Tunnel connections can't call it.
- **Reach:** a caller reaches only threads it started there: B checks the child's `delegated_from` (same A node, A
  store id, parent thread and account), A checks its own `remote_children` before anything leaves. Each store has a
  random `store_id` (schema v29), so a fresh data dir on A reaches nothing old.
- A remote child is a delegated thread everywhere on B: no worktree setup script, never Full access, and its idle agent
  isn't released.
- The parent title is cleaned of control and bidi-override characters before B stores or draws it, and no refusal
  names a local path.

## Tools

`list_machines` lists linked machines with the switch on: `ready`, `offline`, `needs_update` (older than 1.8: nothing
is sent) or `not_linked_here` (B refused for want of a link). The six delegation tools take an optional `machine`
(a MagicDNS name); without it they are local, with it thread ids are B's. A start always carries a request id (the
agent's, else one A makes), so A's retry after a dropped connection returns the same thread; B keeps the request id
with the child (per caller and account, written in one transaction with the thread row), so this holds across a
restart. A start with B not connected fails at once; one whose connection drops
is retried for 30 s, then answers with the request id to retry. A send that may have landed says so instead of
retrying. A wait runs 20 s rounds on B until the agent's timeout, and answers `unreachable` at the deadline if B stays
down. If A can't record a started child, it stops it on B (a child still starting is stopped as soon as it is up). A
call about a known child goes only to the node it was started on: if another node answers to the name, nothing is
sent.

B reads its link, accounts.toml and both logins on every call, and checks the link, the account, the call's
connection and sharing again after the login read, so a Revoke, rebind or Disconnect during it refuses the call.

## UI

B's child header: "Delegated by <parent title> on <A>". A's parent lists remote children in Lineage with "on <B>",
opened through the linked machine. B's child shows the same label on A's remote thread page
(`ThreadRow.delegated_from`).

## Known limits

- The live check can't use two real machines (the daemon needs a real tailnet), so cross-machine behaviour is proven
  by in-process daemon tests; the live check covers B's drawn UI.
- A linked machine added while Settings is open shows in Machines → Delegation the next time Settings opens.
- A login read is cached for up to 10 minutes on each side, so a sign-out or account switch can take that long to
  refuse a call (an Allow reads it fresh).
- Two remote starts from one parent at the same moment can both pass A's own count; B's per-parent cap still holds.
- Out of scope: delegation from mobile; browser, device and html tools on another machine; Codex and Cursor accounts.
