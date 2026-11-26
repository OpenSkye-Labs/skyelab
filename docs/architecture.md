# Architecture

## What we're building

A native ADE: run Claude Code, OpenAI Codex, and Cursor agents in threads over your projects, with worktrees, diffs,
a terminal, a browser preview, a device hub and agent orchestration, the way T3 Code does
([pingdotgg/t3code](https://github.com/pingdotgg/t3code), MIT). It shares environments between your machines over Tailscale.

**What sets it apart from T3 Code:** T3 Code supports several Claude accounts (one `CLAUDE_CONFIG_DIR` per provider
instance), but you pick the account per thread, by hand. There's no project or folder binding
(see t3code discussion #32072). Here, **the folder decides the account**, so a work repo can't run on the
personal plan by mistake.

## Shape (borrowed from T3, where it works)

```
 clients (GPUI desktop, mobile)  ──RPC over WebSocket──▶  environment daemon (one per machine)
                                  local / tailnet            ├─ accounts + bindings (skyelab-core)
                                                             ├─ agent sessions (claude CLI / Agent SDK, per-account env)
                                                             ├─ worktrees, git, checkpoints
                                                             ├─ terminals (PTY), preview browser, devices
                                                             └─ SQLite (threads, events)
```

- **Execution and credentials never leave the environment that runs them.** A client on your laptop driving your
  desktop's environment sends commands. The desktop's daemon launches the agent with the desktop's account dirs.
- The desktop app embeds the daemon in-process for v1. It becomes a separate process only when remote access ships.
- Agents: launch the `claude` CLI in stream-json mode, `codex app-server`, or `cursor-agent -p` in stream-json mode,
  with `launch_env(dir)` applied. That is the only place `CLAUDE_CONFIG_DIR` / `CODEX_HOME` / `CURSOR_CONFIG_DIR` is
  set. The account's `provider` picks which.

## Crates (add each only when its code exists)

| Crate | Holds |
|---|---|
| `skyelab-core` | accounts/bindings, app settings, domain types; no GPUI, no async runtime |
| `skyelab-app` | GPUI desktop client (binary `skyelab`) |
| `skyelab-agent` | launching Claude Code, Codex, and Cursor sessions under the bound account; one event stream for all; resume; SkyeLab's MCP server for orchestration |
| `skyelab-store` | SQLite persistence: projects, threads, thread events (sync API, no GPUI) |
| `skyelab-git` | worktrees, status, structured diff (shells out to `git`) |
| `skyelab-terminal` | PTY + terminal grid model and GPUI view (alacritty_terminal) |
| `skyelab-browser` | embedded browser preview (gpui-webview); user vs agent navigation |
| `skyelab-devices` | device hub core: iOS Simulators and Android emulators (simctl/adb/emulator) |
| `skyelab-tailnet` | `Tailnet` trait over the Tailscale CLI: `self_node / peers / whois / cert` |
| `skyelab-protocol` | client↔environment RPC messages (JSON over WebSocket) |
| `skyelab-daemon` | environment server: tailnet-only `wss://`, whois gate, RPC dispatch; hosted in the app |
| `skyelab-client` | the client side of the protocol (desktop switcher, mobile app) |
| `skyelab-env` | every machine as an environment: `EnvironmentId`, `Shell` (projects + thread rows) and the `Source` trait (one method per protocol call), with `LocalSource` (the daemon's `Backend` in-process) and `RemoteSource` (over `skyelab-client`); no GPUI |

`mobile/` is a separate workspace (iOS/Android client); see [mobile.md](mobile.md).

## Tailscale plan

- **v1 (desktop):** use the OS-installed Tailscale. Read `status` for your devices and MagicDNS names. Listen only on
  the tailnet IP. For each incoming connection, call LocalAPI `whois`. Accept only if `UserProfile.ID == Self.UserID` and
  the node is untagged.
- **Mobile / no Tailscale app:** embed a userspace node (libtailscale via the `libtailscale` crate, which needs Go to build). It needs
  no VPN entitlement, but each install becomes its own tailnet device.
- **Later:** official `tailscale-rs` (pure Rust, currently v0.7, labelled unstable, no mobile or MagicDNS yet), once it hits 1.0.

## Remote

Trust model, what clients can do, ACLs and known limits: [remote.md](remote.md).

## Orchestration

MCP delegation, account rule and limits: [orchestration.md](orchestration.md).

## Platforms

| | Status |
|---|---|
| macOS, Windows, Linux | GPUI official. v1 targets. |
| iOS | GPUI via upstream `gpui_ios`, already in our pin (`gpui-pre-platform` 0.3.14). Experimental. See [mobile.md](mobile.md). |
| Android | GPUI via longbridge `gpui-pre-mobile`, ported to our pin. Community fork. See [mobile.md](mobile.md). |

Mobile is a **remote client only**: it controls a desktop environment over the tailnet and runs no agents itself.
That keeps it small whichever UI tech it ends up on.

## Roadmap

Each item follows [CONTRIBUTING.md](../CONTRIBUTING.md).

Items 1-8 are built (v0.1.0); the [user guide](user-guide.md) describes what each one does today. What's left is
tracked in GitHub issues.

1. **Accounts:** core resolver and config, the accounts/bindings settings UI, a binding indicator in every thread header. Built.
2. **Threads:** run `claude` (or `codex`) in a folder with the resolved account, stream output into a thread view, persist to SQLite. Built.
3. **Projects and worktrees:** project sidebar, a worktree per thread, diff view. Built.
4. **Terminal:** `alacritty_terminal` + a GPUI renderer. Built.
5. **Remote:** daemon split, RPC, Tailscale peers and whois, an environment switcher. Built.
6. **Browser preview:** `gpui-webview` plus agent automation. Built (screenshots untested on Linux).
7. **Device hub:** iOS Simulator / Android Emulator control. Built.
8. **Orchestration:** delegate tasks between threads and environments. Built.
9. **Mobile client:** GPUI on iOS and Android, plan in [mobile.md](mobile.md). In progress.

## Scheduled tasks and bots

One scheduler per app (`workspace/scheduled`): a `Global` whose 15 s tick runs through the hub window, deciding by
the wall clock. Rules in `skyelab_core::schedule` (croner for cron and times of day), tasks in the store (schema 21).
A run resolves the folder's account at run time and pauses the task on any mismatch. A bot is a task with GitHub
triggers (schema 28: `triggers`, `bot_runs` history, `bot_seen` keys): the same tick polls each (account, project)
every 5 min through that account's gh (`scheduled/gh_triggers.rs`, after the same binding check), queues new items as
`bot_runs` rows with their framed prompt, and starts them like scheduled runs. Rules (hits, framing, Plan/Ask cap,
rate limits) in `skyelab_core::bots`. A thread started from GitHub text launches without SkyeLab's MCP tools, on
resume too (`Store::thread_untrusted`). The Bots page (`bots_page.rs`) hosts the list. See
[user-guide-bots.md](user-guide-bots.md).

## CI

What each workflow runs, and the platform traps tests fall into on Linux, Windows and the mobile builds: [ci.md](ci.md).

## Debugging

Debug mode, traces, flame graphs, heap and leak hunting, the network log: [debugging.md](debugging.md). Its code is
`skyelab_core::debug` (switch, log, redaction, live counts) and the app's `debug_mode` and `workspace/debug_hud`.

## Decisions

Recorded decisions (agent env, browser access, minimum macOS, machine names, ...): [decisions.md](decisions.md).
