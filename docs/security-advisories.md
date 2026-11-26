# Known dependency advisories

Part of [security.md](security.md).


Open Dependabot alerts we can't clear with a version bump, why they don't reach SkyeLab, and what lifts them.
Recheck each when lb-wry or gpui-webview release.

- **`glib` 0.18.5, `VariantStrIter` unsound (patched in 0.20).** Linux only, through the browser pane:
  `skyelab-browser` → `lb-wry` 0.53.3 / `gpui-webview` 0.11.0 → `gtk` 0.18 and `webkit2gtk =2.0.1` → `glib` 0.18.
  lb-wry pins `webkit2gtk =2.0.1` (and our own `webkit2gtk` pin must match it), and even upstream `wry` 0.57 still
  uses `gtk` 0.18, so a newer glib can't be locked. No SkyeLab code calls `glib::VariantStrIter` (or any `glib`
  variant API); we use webkit2gtk and cairo only for page snapshots. Lifts when lb-wry moves to `gtk` 0.19+ /
  `webkit2gtk` 3.
- **`rand` 0.7.3, unsound with a custom logger (patched in 0.8.6).** Android target only, at build time:
  `lb-wry` 0.53.3 pins `kuchikiki =0.8.8-speedreader` for Android → `selectors` 0.24 → `phf_codegen` 0.8 (build
  dependency) → `phf_generator` 0.8 → `rand` 0.7.3. The desktop workspace never builds for Android (the phone app is
  the separate `mobile/` workspace, which locks no `rand` below 0.8.8), and the macOS, Windows and Linux builds don't
  compile it at all. Lifts when lb-wry unpins kuchikiki.
