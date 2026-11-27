# Release

How to cut a release, package locally, change the icon and set up signing. Linked from CLAUDE.md.

Packaging uses [cargo-packager](https://github.com/crabnebula-dev/cargo-packager); its config is
`[package.metadata.packager]` in `crates/skyelab-app/Cargo.toml`. Install it with `cargo install cargo-packager --locked`.

- **Cut a release:** bump `version` in the root `Cargo.toml`, commit, then tag `v<version>` and push the tag (the workflow fails if the two differ).
  **Nightly:** run `nightly.yml` by hand on main; it publishes `v<next>-nightly.<date>` as a pre-release ([docs/ci.md](ci.md)).
  Both attach `SHA256SUMS`; the updater refuses a download it doesn't list, so keep that step and never edit a published release's assets.
  Live-check the updater with `SKYELAB_UPDATE_FAKE=<tag>[,<tag>…]` (`none`: a repo with no release yet), `SKYELAB_UPDATE_FAKE_DIR` (assets plus their `SHA256SUMS`, served slowly) and `SKYELAB_UPDATE_FAKE_APP` (a stand-in `.app` or AppImage to replace), debug builds only.
  `release.yml` packages (`package.yml`: `.dmg` arm64, NSIS `-setup.exe`, `.deb` + `.AppImage`) into a **draft** release. Check it, then publish it.
- **Locally (macOS):** `cargo packager --release -p skyelab-app -f app,dmg` writes `target/release/SkyeLab.app` and the `.dmg`.
  Prefix `CI=true` when the terminal has no Automation permission for Finder; that skips only the dmg window layout.
  `test/acceptance/packaging.sh all` checks the icons, release profile, Info.plist, code signature and workflow.
- **Icon:** see [the brand guide](design/brand.md). Edit `crates/skyelab-app/icons/icon.svg`, then run `scripts/icons.sh` on macOS (needs `cargo install resvg`).
- **Re-running** a tag's workflow makes a second draft; delete the stale draft first.
- **Signing:** without these repository secrets the macOS app is signed ad hoc (`signing-identity = "-"`, so the bundle
  verifies and keeps its identifier; Gatekeeper still asks on first open), and Windows is unsigned. Never commit them. Each set is all-or-nothing.
  - macOS, sign + notarize: `APPLE_CERTIFICATE` (base64 Developer ID Application `.p12`), `APPLE_CERTIFICATE_PASSWORD`,
    `APPLE_SIGNING_IDENTITY` (e.g. `Developer ID Application: Name (TEAMID)`), `APPLE_ID`,
    `APPLE_PASSWORD` (an app-specific password), `APPLE_TEAM_ID`.
  - Windows, through [Azure Artifact Signing](https://learn.microsoft.com/azure/trusted-signing/) (public code-signing
    keys must live in an HSM, so a `.pfx` secret is not an option): `AZURE_CLIENT_ID`, `AZURE_CLIENT_SECRET`,
    `AZURE_TENANT_ID` (an app registration with the signer role), `AZURE_ARTIFACT_SIGNING_ENDPOINT`
    (e.g. `https://eus.codesigning.azure.net`), `AZURE_ARTIFACT_SIGNING_ACCOUNT`, `AZURE_ARTIFACT_SIGNING_CERTIFICATE_PROFILE`.
  - Linux packages are not signed.
