# Decisions: Remote

Part of [decisions.md](../decisions.md). Reaching this machine from other devices. Each entry is dated; newest at
the bottom.

- **Remote is TLS-only, with the node's `tailscale cert` certificate.** The address guard can't tell a tailnet peer
  from a CGNAT neighbour, so the client verifies the Let's Encrypt chain and the exact MagicDNS name. No certificate
  means no sharing, never plaintext. rustls with the `ring` provider (no OpenSSL, no aws-lc build tools), TLS 1.3
  only, webpki roots rather than the platform verifier so mobile needs no JNI setup. (2026-11-23)
- **Remote devices are approved one by one.** Whois can't tell the SkyeLab app from another process of the same
  user, so the daemon also requires an approved per-install device key (keychain on macOS release builds). A key
  over a TLS client cert: the same storage question, and one less TLS mode to get right. (2026-11-23)
- **Remote calls reuse the thread view.** The daemon's agent calls are forwarded to the UI thread and run through
  `ThreadView::send_prompt`, so there is one launch path and one place the account is resolved. Sharing is off by
  default and only an explicit toggle turns it on. (2026-11-23)
- **Remote machine names:** clients (mobile and desktop switcher) accept only MagicDNS names (`<machine>.<tailnet>.ts.net`).
  TLS certs from `tailscale cert` exist only for those names, and the client checks the cert against the exact name.
  Raw IPs, `localhost` and short names are rejected at input. (2026-11-23)
- **Remote errors carry no local paths:** the daemon replaces every word with a `/` or `\` in an error message
  with `<local path>` just before it sends it, so no backend refusal can tell a client this machine's folders.
  Errors the app knows may quote a folder (a refused resume) are written without it, since a path with spaces
  keeps its slash-free words. Project listings still send project paths by design. (2026-11-23)
