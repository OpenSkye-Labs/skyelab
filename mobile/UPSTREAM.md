# gpui-mobile: what we pin and what we send upstream

Android runs on longbridge's [gpui-mobile](https://github.com/longbridge/gpui-mobile) (crate `gpui-pre-mobile`).
We pin commit `7548ebc4b24f00494af9976af8852c7163ded9ce`, which targets `gpui-pre =0.3.10`. Our GPUI is
`=0.3.12`, so [`scripts/fetch-gpui-mobile.sh`](scripts/fetch-gpui-mobile.sh) fetches that exact commit into
`target/gpui-mobile` and applies five patches in order. Every cargo command in `mobile/` needs it first, because
cargo resolves the Android-only path dependency on every target.

We take gpui-mobile under **Apache-2.0**, one of its three licenses (GPL-3.0+ OR AGPL-3.0+ OR Apache-2.0). None of its
code is in our git: the Android host (`android-host/`) copies the one Java class it needs, `GpuiInputActivity`, from the
fetched checkout at build time, and ships gpui-mobile's `LICENSE-APACHE` in the APK as
`assets/licenses/gpui-mobile-LICENSE-APACHE.txt`.

## Why fetch and patch, not vendor

- **Reproducible.** The fetch names a full commit sha, which is content-addressed, and `git apply` fails loudly if a
  patch stops fitting. Every build gets the same bytes.
- **Small.** gpui-mobile is 51k lines of Rust. Vendoring it would add 2 MB of someone else's code to every review and
  every GPUI bump. The patches are about 20 KB.
- **The risk** is that the GitHub repo or commit goes away. If it does, or once our own remote is live, fork it and
  pin the fork by rev (docs/mobile.md step 2).

## The patches

| File | Send upstream? | What it does |
|---|---|---|
| `upstream/0001-port-to-gpui-pre-0.3.12.patch` | Yes | Raises the `gpui-pre` / `gpui-pre-wgpu` pins to `=0.3.12`. Passes the new `&WgpuSurfaceConfig` argument to `WgpuContext::new` on Android (`src/android/window.rs`) and iOS (`src/ios/window.rs`). Implements `PlatformWindow::insets` on Android from the safe area gpui-mobile already tracks, so `window.insets()` works there as it does on `gpui_ios`. |
| `upstream/0002-refresh-vendored-gpui-pre-wgpu-0.3.12.patch` | Yes | Rebases the vendored `gpui-pre-wgpu` (the `offscreen` feature) from 0.3.10 onto 0.3.12. Its own changes apply to 0.3.12 without edits. |
| `upstream/0003-android-report-ime-insets.patch` | Yes | On every content-rect change (keyboard shown or hidden included), reads the decor view's root `WindowInsets` over JNI (API 30+): `systemBars() \| displayCutout()` becomes the safe area and `ime()` the keyboard inset, so `window.insets().ime` is real on Android and the safe area no longer carries the keyboard. Below API 30, or before the view is attached, the content-rect estimate stays. |
| `upstream/0004-android-skip-system-flags-font.patch` | Yes | Stops loading `/system/fonts/NotoColorEmojiFlags.ttf`. gpui-pre-wgpu draws colour only for the PostScript name `NotoColorEmoji`, so flags that fell back to the flags font were not drawn in colour. Stock Android before 13 keeps flags in its one CBDT emoji font, and the bundled fallback has them too; an OEM image that keeps a CBDT font on 13+ and splits flags out would lose them (not seen). |
| `upstream/local-no-offscreen.patch` | No | Ours only. A `[patch]` in a dependency is ignored, so as a path dependency gpui-mobile gets crates.io `gpui-pre-wgpu`, which has no `offscreen` feature. This drops that feature reference and the unused `[patch]`. We don't use `offscreen`. |

0001 + 0002 were checked in gpui-mobile's own workspace with `--features offscreen`:
`cargo ndk -t arm64-v8a -P 26 check --lib` and `cargo check --lib --target aarch64-apple-ios-sim` both pass.

## Sending it

Plain `git diff` output, no author line. From a clone at the pinned commit:

```sh
git apply ../skyelab/mobile/upstream/0001-port-to-gpui-pre-0.3.12.patch
git commit -am "Port to gpui-pre 0.3.12; implement PlatformWindow::insets on Android"
git apply ../skyelab/mobile/upstream/0002-refresh-vendored-gpui-pre-wgpu-0.3.12.patch
git commit -am "Refresh vendored gpui-pre-wgpu to 0.3.12"
git apply ../skyelab/mobile/upstream/0003-android-report-ime-insets.patch
git commit -am "Android: report system-bar and IME insets from WindowInsets"
git apply ../skyelab/mobile/upstream/0004-android-skip-system-flags-font.patch
git commit -am "Android: don't load the system flags font, which renders without colour"
```

0003 and 0004 apply on 0001 + 0002 without the local patch (checked with `git apply` on a fresh clone).

Then open a PR against longbridge/gpui-mobile with this description:

> Ports gpui-mobile to `gpui-pre` 0.3.12, the version `gpui-component` 0.10 pins.
> `WgpuContext::new` gained a `&WgpuSurfaceConfig` argument in 0.3.12; the Android and iOS windows now pass theirs.
> The vendored `gpui-pre-wgpu` is rebased onto 0.3.12 with the `offscreen` changes unchanged.
> Android's `PlatformWindow` now implements `insets()` from the content-rect safe area, so apps can read
> `window.insets().safe_area` on both platforms.
> Android also reads the root `WindowInsets` on each content-rect change, so `insets().ime` reports the keyboard
> and `safe_area` the system bars plus cutout (API 30+; the content rect stays the fallback). Below target SDK 35 the
> content rect already shrinks for the keyboard, but edge-to-edge (target SDK 35+) stops that.
> Checked: `cargo ndk -t arm64-v8a check --lib --features offscreen` and
> `cargo check --lib --target aarch64-apple-ios-sim --features offscreen`.

When upstream merges it, move the pin to their merge commit and delete 0001 to 0004.

## On every GPUI bump

The pins in `mobile/Cargo.toml` follow the desktop's. Port gpui-mobile to the new `gpui-pre` (update or regenerate
these patches against a new upstream rev), then build both targets (`scripts/run-ios.sh`, `scripts/run-android.sh`).

## Also worth reporting: accesskit_android 0.7.5

Typing the first character into an empty field panicked in debug builds: `accesskit_android::event` computes
`old_u16.len() - 1` on an empty string (`panic_const_sub_overflow` in `QueuedEvents::raise`, called from
gpui-mobile's TalkBack adapter). Release wraps it to `-1` as a `jint`, which is what the algorithm expects, so
`mobile/Cargo.toml` turns off overflow checks for that one crate in dev. Check whether a newer accesskit_android
(0.9.x at the time of writing) fixes it when gpui-mobile moves to it; if not, report it to AccessKit.

## Known gaps in what we borrow

- **Android keyboard.** Closed by 0003. On the Pixel 8 API 36 emulator (example host, target SDK 34) the content rect
  already shrank for the soft keyboard, so the composer was never covered there; what was missing was `ime` (zero)
  and a safe area that carried the keyboard height. With 0003 logcat shows `bars=[0, 132, 0, 63] ime_bottom=883`
  with the keyboard up. No keyboard animation tracking: the layout jumps once the insets settle.
  Edge-to-edge (target SDK 35+): there the content rect stops changing for the keyboard, so 0003 alone never
  re-reads the insets. Our host's `SkyeLabActivity` pads its content view by the IME inset from an
  `OnApplyWindowInsetsListener`; the view resizes, NativeActivity reports a content-rect change, and 0003 reads the
  real `WindowInsets`. No Rust change. Not yet seen on a device (docs/mobile.md step 6). Worth upstreaming into
  gpui-mobile's example host once it is.
- **The Gradle host** is ours since docs/mobile.md step 6 (`android-host/`). gpui-mobile's example host asked for
  camera, microphone, contacts, calendar, media and notification permissions, exported a `gpui://` deep link and had no
  `distributionSha256Sum`; ours has none of that (`scripts/check-hosts.sh`). What we dropped from it: the splash
  screen, the media session, deep links, the pickers and plugin classes, and the bundled 10 MB `NotoColorEmoji.ttf`.
  In its place we bundle Twemoji Mozilla (1.5 MB, 0.7 MB in the APK) under that asset name (docs/mobile.md step 6,
  "Emoji").
- **Emoji on Android 13+.** The system `NotoColorEmoji.ttf` is COLR v1 there, and swash (what gpui-pre-wgpu
  rasterises with) renders only CBDT and COLR v0, so gpui-mobile falls back to the font in the APK. Worth raising
  upstream: COLR v1 support in the text system (skrifa can paint it) would let every app use the system font and drop
  the bundled one; and gpui-pre-wgpu's `check_is_known_emoji_font` matching only `NotoColorEmoji` forces our rename.
- **Our host relies on two upstream names.** The JNI symbols bind to `dev.gpui.mobile.GpuiInputActivity` (its
  `nativeIme`, and the `gpuiShowKeyboard`/`gpuiHideKeyboard`/`gpuiResetComposition` methods Rust calls on the activity),
  and the Gradle copy task reads `example/android/gradle/app/src/main/java/` in the checkout. On a pin bump, check both
  still exist; the build fails loudly if the file moved.
