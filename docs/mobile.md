# Mobile client

**Decision (2026-11-23): build the iOS and Android client in GPUI, as one Rust crate with two thin platform entry points.**
iOS runs on the GPUI we already pin. Android runs on longbridge's `gpui-mobile` platform crate, ported to our pin with a
small patch (version pins plus a three-line API fix). Both run the same `gpui-component` view, and gpui-component 0.10
compiles for both targets without changes.

The mobile app is a remote client only. It talks `skyelab-protocol` to a desktop environment over the tailnet and runs
no agents.

## What was tried

The spike lives in [`experiments/mobile-gpui`](../experiments/mobile-gpui). It is excluded from the workspace
(`[workspace] exclude = ["experiments"]`), so it never touches the desktop build or the verify gate.

It is one screen: a thread list decoded from a canned `skyelab-protocol` `Reply::Threads` message, tap to select, and
a `gpui_component` `Input` with a Send button. `src/app.rs` holds the view. `src/main.rs` is the iOS entry point and
`src/lib.rs` (`android_main`) is the Android one.

```sh
cd experiments/mobile-gpui
ios/run-simulator.sh <simulator-udid>    # cargo build → .app → simctl install + launch
android/run-emulator.sh emulator-5554    # cargo ndk → gradle APK → adb install + launch
```

### Candidates

| Candidate | Result |
|---|---|
| Upstream `gpui_ios` | **Works.** Our pinned `gpui-pre-platform =0.3.12` already depends on `gpui-pre-ios =0.3.12` for `target_os = "ios"`, so `gpui_platform::application()` just works on iOS. The old note that it was "not in our pinned snapshot" was wrong. |
| `gpui-mobile` 0.1.0 on crates.io (itsbalamurali) | **Not usable.** The published crate has an empty `[dependencies]` table and no GPUI dependency. It's a placeholder. |
| `gpui-pre-mobile` (longbridge, the `gpui-component` maintainers) | **Works on Android after a port.** crates.io 0.1.0 pins `gpui-pre =0.3.5`. Repo `main` (rev `7548ebc4b24f`) pins `=0.3.10`. Moving it to `=0.3.12` caused one compile error (`WgpuContext::new` gained a `&WgpuSurfaceConfig` argument). The fix is [`android/gpui-pre-mobile-0.3.12.patch`](../experiments/mobile-gpui/android/gpui-pre-mobile-0.3.12.patch). |
| uniffi Rust core + native UI per platform | **Not built.** Candidate 1 worked on both platforms, so per the spike's rules it was compared on paper only (see Trade-offs). |

### Evidence

| Check | iOS Simulator (iPhone 17, iOS 27.0) | Android Emulator (Pixel 8, API 36, arm64) |
|---|---|---|
| Builds | `cargo build --target aarch64-apple-ios-sim`, 4 min 11 s cold. gpui-component 0.10 compiles unchanged. | `cargo ndk -t arm64-v8a build --lib`, about 2 min cold. gpui-component 0.10 compiles unchanged. |
| Renders list + input | Yes (`ios-gpui.png`) | Yes (`android-gpui.png`) |
| Tap selects a row | Yes (`ios-gpui-interact.png`) | Yes |
| Tap focuses the input | Yes, the caret shows | Yes |
| Typing | **Not verified.** See "iOS has no accessibility tree" below. | Yes, `agent-device type` (`android-gpui-typed.png`) |
| Send appends the message | Not reached | Yes (`android-gpui-sent.png`) |
| `skyelab-protocol` decodes | Yes, linked into both apps | Yes |
| `tokio` + `tokio-tungstenite 0.30` + `skyelab-protocol` cross-compile | Yes, in a throwaway crate calling `connect_async` (not committed) | Yes, same crate via `cargo ndk` |
| Debug artifact size | 34 MB `.app` | 773 MB `.so`, 1.3 GB debug APK. Release size was not measured. |

### Problems found, with the exact errors

1. **Android debug builds crash on the emulator.** wgpu enables Vulkan debug labels under `debug_assertions`, and
   the emulator's llvmpipe driver segfaults in `vkSetDebugUtilsObjectNameEXT` (`create_bind_group_layout`). Workaround:
   `[profile.dev] debug-assertions = false`. `mobile/Cargo.toml` also disables overflow checks for `accesskit_android`
   0.7.5 in dev (underflow on first typed character; see `mobile/UPSTREAM.md`).
2. **iOS has no accessibility tree.** `gpui_ios` exposes only a 1-node tree, so VoiceOver and `agent-device type`
   can't drive inputs yet (typing on iOS is unproven; Android ships an AccessKit bridge).
3. **Safe areas** need platform insets (`gpui-pre-mobile` provides them on Android; `gpui_ios` uses `window.insets()`).
4. **Upstream drift.** `gpui-pre-mobile` tracks its own GPUI snapshot, so GPUI bumps require re-porting our patches.

## Trade-offs

| | GPUI on both (chosen) | uniffi Rust core + SwiftUI / Compose |
|---|---|---|
| UI code | One codebase, shared with desktop: same components, theme and patterns as `docs/gpui.md` | Two native UIs, written twice |
| Protocol and connection logic | Plain Rust, used directly | Rust behind uniffi, plus generated Swift and Kotlin bindings |
| Platform polish (keyboard, a11y, safe areas, gestures) | Rough: experimental platform layers, iOS a11y missing | Native, free |
| Upstream risk | `gpui_ios` is upstream Zed but experimental. Android depends on one community fork we must port on each bump. | Low: uniffi 0.32 is mature |
| Build tooling | cargo + a small Xcode plist / Gradle host | cargo + Xcode project + Gradle + bindgen step |
| App size | Large: GPUI and wgpu in every build | Small UI, Rust core only |

GPUI wins because the client is small (four calls and a live event stream) and the team writes GPUI every day. The
polish gaps matter less for an internal remote control than a second and third UI codebase would. If iOS accessibility
or text input turns out to be a blocker, the fallback stays cheap: the connection logic lives in a GPUI-free crate
(step 1 below) that uniffi can wrap later.

## Plan for the real client

Each step follows [CONTRIBUTING.md](../CONTRIBUTING.md).

1. **`skyelab-client` crate (no GPUI).** A WebSocket client for `skyelab-protocol`: `hello`, `list_projects`,
   `list_threads`, `thread_events_since`, `send_message`, `interrupt`, and pushed `Event`s, with gap fill by `seq`. It
   runs tokio on its own thread and talks to the UI through channels (the `docs/gpui.md` async rule). Test it against a
   real `skyelab-daemon` in-process. The desktop environment switcher uses it too.
   **Done:** `crates/skyelab-client`. Only watched threads get events (`Client::watch(thread, after_seq)`). A request
   is never re-sent after a dropped connection; it fails with `ConnectionLost`, because `send_message` may have run.
2. **Pin `gpui-pre-mobile`.** *Done (2026-11-23), without a fork for now:* `mobile/scripts/fetch-gpui-mobile.sh`
   fetches the pinned commit and applies our patches. The two upstream-ready ones are described in
   [`mobile/UPSTREAM.md`](../mobile/UPSTREAM.md), which also says why fetch + patch beats vendoring. Fork and pin by
   rev once our remote is live. The GPUI bump rule in `docs/gpui.md` now includes re-porting it.
3. **`mobile/` app crate**, outside the desktop workspace like the spike, with its own lockfile. It shares path deps
   (`skyelab-protocol`, `skyelab-client`) and the exact GPUI and gpui-component pins. CI builds it for
   `aarch64-apple-ios-sim` and `aarch64-linux-android` in its own job, so the desktop gate stays fast. *Started
   (2026-11-23):* `mobile/` ran the step 5 screens on a fake `ThreadSource` until `skyelab-client` landed (now only the `demo`
   feature and tests use it), and
   `.github/workflows/mobile.yml` builds both targets.
4. **Connect over the tailnet.** v1 relies on the official Tailscale app's VPN being on and dials
   `wss://<machine>.<tailnet>.ts.net:<port>`. The daemon's `whois` check already accepts the same user's untagged
   phone. The client must check the server too: if the VPN is off, the name resolves through whatever DNS the phone
   has, so `skyelab-client` refuses any address outside Tailscale's ranges (100.64.0.0/10, fd7a:115c:a1e0::/48) and
   speaks only `wss://`, checked against the node's `tailscale cert` certificate. A lost phone is revoked by removing it from the tailnet.
   An embedded `libtailscale` node comes later, only if needing the VPN app is a real problem.
   **Done (address check):** `skyelab-client` resolves the name, dials only the tailnet addresses it got back
   (`is_tailnet_ip`), and never re-resolves. Loopback is refused too; the exception exists only under `cargo test`.
   **Done (TLS):** the daemon serves only `wss://` with the node's `tailscale cert` certificate, and the client
   accepts only a chain to the Mozilla roots whose leaf names exactly the dialled MagicDNS name. That closes the
   carrier-grade NAT hole (100.64.0.0/10 is also CGNAT space, so a hostile DNS with the VPN off could point at
   another subscriber's 100.x host). The tailnet needs MagicDNS and HTTPS Certificates on (remote.md).
   **Done (mobile, 2026-11-23):** `mobile/src/remote.rs` is a `ThreadSource` over `skyelab-client`. The picker takes
   only `<machine>.<tailnet>.ts.net` names. The screens show Connecting, Connected, Reconnecting (with why), Refused
   (the daemon's gate closed the socket: not one of your own untagged devices) and a version mismatch. It calls
   `Client::connect(host, Options::default())` with only `approve_each_connection` set, so the transport the client defaults to (`wss://` once it lands) is
   what the phone uses.
   **Done (node pinning, 2026-11-23):** the phone pins the node StableID per MagicDNS name in `nodes.json` in its
   data dir, from the hello: the phone has no Tailscale CLI, and the official app's LocalAPI is sandboxed, so it
   can't ask its own Tailscale who it dialled as the desktop does (`tailscale whois`). The first id a name reports
   is pinned. Another id later shows "This isn't the machine you connected to before" with **Trust it** (which
   re-pins exactly the node shown, and only while that node is still connected). A pinned name whose hello has no id,
   an id that isn't a StableID (empty, over 64 characters, not ASCII alphanumeric), and a pin file that can't be read
   or parsed, are refused. A never-pinned name whose hello has no id (an old server) connects unpinned, as on the
   desktop. A certificate failure counts as an identity failure too. Until a connection passes, the client holds it
   (`approve_each_connection`): nothing is listed, watched or sent. A refusal empties the projects, threads and
   events and leaves the thread screen. **This is weaker than the desktop:** it trusts the server's report about
   itself, so it catches a reused device name or a replaced machine, but a node that took over the name and replays
   the old id passes. Upgrade path: an embedded `libtailscale`/tsnet node, or the Tailscale app's LocalAPI if it is
   ever exposed, then pass a `PeerVerifier` like the desktop's.
   Tests run it against an in-process daemon with a fake tailnet: the shared harness in
   `skyelab_daemon::testing` and `skyelab_client::testing::options`, both behind `test-support`, dev-dependencies only.
5. **Screens.** Picker (MagicDNS name, remembered) → threads → thread view (live events) with composer (send) and
   stop (interrupt), respecting safe areas. **Done (2026-11-23):** events stream into a `list` + `ListState` (visible
   rows only; 10k-event threads open/append in ~1 ms). Android reports keyboard insets (`mobile/upstream/0003`). Build
   with `SKYELAB_MOBILE_FEATURES=demo scripts/run-*.sh` for canned data.
6. **Packaging.** iOS: a generated Xcode project (or the spike's plist) with signing for device builds. Android: the
   Gradle host from gpui-mobile, cut down to `GpuiActivity` and text input, with a pinned
   `distributionSha256Sum`. `android_main` needs `#[unsafe(no_mangle)]`, so it goes in a tiny crate that allows
   `unsafe_code`, per the CLAUDE.md rule. Measure release sizes with
   `opt-level = "z"`, LTO and strip.
   **Done (2026-11-23):** both shells are ours, app id `dev.openskye.skyelab`, label SkyeLab, the placeholder icon
   (`scripts/icons.sh` renders both from `icon.svg`). `mobile/scripts/check-hosts.sh` enforces the rules below on the
   sources and on a built APK or .app, in CI too.
   - **Android** (`mobile/android-host/`): one activity, `SkyeLabActivity`, a `GpuiInputActivity` (NativeActivity
     plus gpui-mobile's keyboard bridge) that adds edge-to-edge keyboard insets. INTERNET is the only permission (no
     code reads network state), the launcher is the only component, no deep links. Gradle 9.9.2 with
     `distributionSha256Sum`, and the wrapper jar's sha256 checked against Gradle's published one. Every artifact
     Gradle resolves (AGP and everything under it) is checked against `android-host/gradle/verification-metadata.xml`;
     a mismatch or an artifact missing from it fails the build. AGP 9.5.2,
     compileSdk and targetSdk 37 (Android 17), minSdk 26 (what the cdylib links against, `cargo ndk -P 26`), arm64
     only. `scripts/build-android.sh release` signs only when all four `SKYELAB_ANDROID_KEYSTORE*` /
     `SKYELAB_ANDROID_KEY_*` variables are set, else writes `app-release-unsigned.apk`.
   - **iOS** (`mobile/ios/SkyeLab.xcodeproj`): checked in by hand rather than generated with xcodegen, so neither dev
     machines nor CI need another tool; it is one target with no sources, so it rarely changes. A "Build Rust" phase
     runs cargo (`ios/build-rust.sh`) and Xcode adds the asset catalog, Info.plist and signing. Deployment target
     iOS 15, what the binary has always been built for. No entitlements. `scripts/build-ios.sh release device`
     signs with `SKYELAB_IOS_TEAM_ID` (automatic signing); no team id is committed.
   - **Updating the Gradle checksums** after bumping AGP, Gradle or adding a dependency: in `mobile/android-host/`,
     `./gradlew --write-verification-metadata sha256 help assembleDebug assembleRelease` (it keeps existing entries
     and adds new ones). Then read the diff: every new component should be one the bump explains (an AGP version,
     a library it pulls in), from `google()` or `mavenCentral()`. aapt2 is one jar per OS and Gradle only records the
     one it ran on, so add the other two by hand from `https://dl.google.com/dl/android/maven2/com/android/tools/build/aapt2/<version>/aapt2-<version>-<osx|linux|windows>.jar`
     (`shasum -a 256`); `scripts/check-hosts.sh` fails until all three are there. Delete entries for versions you no
     longer use. Never set `verify-metadata` to false to get past a mismatch: a mismatch means the downloaded bytes
     changed.
   - **Emoji.** On Android 13+ the system emoji font is COLR v1, which the text system (swash) can't render, so
     gpui-mobile loads `assets/fonts/NotoColorEmoji.ttf` from the APK instead. We bundle Twemoji Mozilla 0.7.0
     (COLR v0, every emoji to Emoji 15, CC BY 4.0, license in `assets/licenses/`) under that name:
     `scripts/fetch-gpui-mobile.sh` downloads it by sha256 into `target/emoji/` and renames its PostScript name to
     `NotoColorEmoji`, the only name gpui-pre-wgpu draws in colour. It adds 0.7 MB to the APK (Noto's own CBDT
     font would add 10 MB). Below Android 13 the system font is CBDT and is used instead. Emoji therefore look like
     Twitter's set, not Android's. `mobile/android/tests/emoji_font.rs` checks a face and a flag rasterise in colour.
   - **Release sizes** (`opt-level = "z"`, fat LTO, one codegen unit, strip): Android `libskyelab_mobile_android.so`
     15.2 MiB, release APK 16.0 MiB (15.3 MiB before the emoji font; the .so is stored uncompressed so it can be
     mapped in place); iOS simulator
     `SkyeLab.app` 9.2 MiB (9.1 MiB executable). Clean release builds take about 3 min (Android) and 1.5 min (iOS)
     on an M-series Mac.
   - **Limits:** no splash screen; Android is arm64 only; iOS is iPhone only.
7. **Protocol parity (2026-11-24).** The phone renders protocol data with the desktop's shared models
   (`skyelab_agent::{timeline, thread_list}`, `skyelab_core::snooze`):
   - **Threads** is the first screen after connecting: every project's threads in Pinned, Working, Today, Yesterday,
     Previous 7 days and Older, newest first (plus collapsible Snoozed and Settled shelves below). Each row shows
     title + status dot, project · account · status, and a one-line plain preview.
   - **Timeline**: compact rows (`src/timeline_rows.rs`); finished turns fold under "Worked for …"; lines clipped
     (2000 chars, 500 lines); plain text.
   - **Answers** above the composer: AskUserQuestion (`answer_questions`, >= 1.4), ExitPlanMode (Approve / Keep
     planning), and tool permissions (Allow once / Allow for this thread / Deny), all gated on device + node approval.
   - **Usage**: thread header shows `account · status · Limited · 5h … · 7d …` from `RateLimit` events.
   - **Touch**: rows, turn/shelf fold headers, shelf/snooze actions, and buttons are >= 44 pt (`app::TAP`, verified by
     `tests/touch_targets.rs`). Theme follows system appearance (`SKYELAB_VERIFY_THEME=dark|light` in debug).
8. **Shell stream & thread shelving (protocol 1.6 / 1.7).**
   - **Live shell stream (`subscribe_shell`, >= 1.7):** `RemoteSource` subscribes via `Client::subscribe_shell` and
     reduces `ShellDelta` pushes with `skyelab_protocol::ShellSnapshot::apply`, keeping the thread list updated live
     without polling or Back+refresh; a dropped stream while connected resubscribes automatically. Older servers
     (< 1.7, e.g. 1.4–1.6) are never sent `subscribe_shell` and fall back to `list_projects` + `list_threads`. Any
     refusal, `IdentityMismatch`, `NodeChanged`, or `VersionMismatch` clears projects, threads, and events immediately
     and stops watching/subscribing.
   - **Thread shelving (`shelve_thread`, >= 1.6):** `thread_rows::partition` matches desktop precedence (working beats
     shelves; shelves beat pinned) with collapsible **Snoozed** (soonest-waking first, `Wakes in …` badge) and
     **Settled** (most-recently-settled first) shelves. Each row has >= 44 pt controls (`⋯` opens Settle + desktop
     snooze presets from `skyelab_core::snooze::presets`; shelved rows add direct Wake / Restore buttons). Servers
     older than 1.6 are never sent `shelve_thread` and show `"update SkyeLab on <machine>"`.
9. **Verify.** Logic is tested in `skyelab-client`; views use `#[gpui::test]`. Android uses `agent-device`; iOS typing
   is checked manually until `gpui_ios` exposes accessibility.
