# Security policy

SkyeLab runs coding agents on your machine with your accounts, so we take security reports seriously.

## Supported versions

SkyeLab is pre-1.0. Only the latest release gets security fixes. Fixes ship in a new release, and the in-app updater
offers it.

| Version | Supported |
|---|---|
| Latest release | Yes |
| Older releases | No |
| `main` | Fixed there first |

## Report a vulnerability

**Don't open a public issue.** Use GitHub's private vulnerability reporting:

1. Go to the [Security tab](https://github.com/OpenSkye-Labs/skyelab/security) of this repository.
2. Click **Report a vulnerability**.
3. Describe the problem, the version or commit, your OS, and the steps to reproduce it. A proof of concept helps.

Only the maintainers see the report. If you can't use GitHub, open an issue asking for a private contact, with no
details in it.

## What to expect

- We confirm we got the report within 7 days.
- We tell you whether we accept it and how we plan to fix it within 30 days.
- We credit you in the advisory and the changelog, unless you'd rather stay anonymous.
- Please give us time to ship a fix before you disclose it publicly. We aim for 90 days at most and will agree a date
  with you.

## In scope

The security model is in [docs/security.md](docs/security.md). Reports that break any of it are in scope, especially:

- **Account isolation:** an agent, terminal, git or gh process running under an account its folder isn't bound to; a
  fallback to a default account; credentials or env leaking from one account to another.
- **Agent sandboxing choices:** a repo's own settings, hooks, MCP config or git config running code or changing
  credentials when SkyeLab says they don't load; approvals being bypassed or remembered beyond what the UI says.
- **Remote and tailnet sharing:** connecting without approval, from another user's or a tagged device, or reaching
  more than a connected device should (terminals, accounts, bindings, ports outside the allowed range).
- **The updater:** installing an asset that isn't the release's, doesn't match `SHA256SUMS`, or fails the signature
  checks.
- **Browser isolation:** an agent's tab reaching your own tabs, cookies or storage, or `file:` and other schemes;
  another thread's tab.
- **Delegation:** an agent starting or reading threads outside its own account.
- **Logs:** prompts, tokens or env values ending up in the debug log.

## Out of scope

- Bugs in Claude Code, Codex, `gh`, Tailscale or the OS themselves. Report those upstream.
- Anything that needs an attacker who already runs code as your user outside SkyeLab.
- What an agent does with the permissions you granted it (for example, in Full access mode).
- Development-only switches (`SKYELAB_*_BIN`, `SKYELAB_VERIFY_*`) that only exist in debug builds.
