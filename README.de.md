<p align="center">
  <img src="docs/images/banner.png" alt="SkyeLab" width="720">
</p>

<p align="center">
  <a href="README.md">English</a> ·
  <a href="README.zh-CN.md">简体中文</a> ·
  <a href="README.zh-TW.md">繁體中文</a> ·
  <a href="README.ja.md">日本語</a> ·
  <a href="README.ko.md">한국어</a> ·
  <a href="README.es.md">Español</a> ·
  <a href="README.pt-BR.md">Português (Brasil)</a> ·
  <b>Deutsch</b> ·
  <a href="README.fr.md">Français</a> ·
  <a href="README.ru.md">Русский</a>
</p>

> Dies ist eine Übersetzung. Maßgeblich ist das [englische README](README.md); zuletzt damit abgeglichen am 2026-11-25.

<p align="center">
  <b>Eine native Desktop-App für Coding-Agents, in der jeder Ordner fest an ein Konto gebunden ist.</b>
</p>

<p align="center">
  <a href="https://github.com/OpenSkye-Labs/skyelab/actions/workflows/ci.yml"><img src="https://github.com/OpenSkye-Labs/skyelab/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="https://github.com/OpenSkye-Labs/skyelab/releases"><img src="https://img.shields.io/github/v/release/OpenSkye-Labs/skyelab?include_prereleases&sort=semver" alt="Release"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="Lizenz: MIT"></a>
</p>

SkyeLab ist eine agentische Entwicklungsumgebung (ADE), geschrieben in Rust mit [GPUI](https://www.gpui.rs/). Du
startest Claude Code- und Codex-Agents in Threads über deinen Projekten, prüfst ihre Änderungen und hast ein
Terminal, einen Browser und deine Simulatoren direkt daneben. Die Grundform übernimmt es von [T3 Code](https://github.com/pingdotgg/t3code).

> **Status: früh.** Rechne mit Ecken und Kanten und mit Breaking Changes. macOS ist die Hauptplattform.

## Warum es das gibt

Wer ein Arbeitskonto und ein privates Konto hat, erlebt früher oder später, dass ein Agent im falschen läuft.
SkyeLab macht das unmöglich: **Jeder Ordner ist an genau ein Konto (Claude oder Codex) gebunden, und diese Bindung
wird durchgesetzt.**

- Der Agent in deinem Arbeits-Repo läuft immer mit dem Arbeitskonto, der in deinem Nebenprojekt mit dem privaten.
- Die tiefste Bindung gewinnt, also kann `~/work` ein Konto sein und `~/work/oss-fork` ein anderes.
- Ein Ordner ohne Bindung fällt nie auf ein Standardkonto zurück. SkyeLab bittet dich, ihn zuerst zu binden.
- Zeigt ein gebundener Pfad später woandershin (ein ausgetauschter Symlink, ein verschobener Ordner), wird die
  Bindung *veraltet*, und dort startet kein Agent, bis du sie erneut bestätigst.
- GitHub funktioniert genauso: Jedes Konto kann seinen eigenen `gh`-Login haben, der für Commits, Pushes und Pull
  Requests verwendet wird.

## Funktionen

- **Threads** für Claude Code und Codex mit einer reichhaltigen Timeline: gestreamte Antworten und Denkschritte,
  Tool-Karten, Inline-Diffs, Shell-Ausgabe, Todo-Listen, Subagents, Fragen, Pläne und Tool-Freigaben, die du direkt
  an Ort und Stelle beantwortest.
- **Composer** mit Datei-, Thread- und Pull-Request-Erwähnungen als Inline-Chips, Bildern und Dateien, `/`-Befehlen,
  einer Auswahl für Modell und Aufwand, einem Berechtigungsmodus pro Thread, einer Warteschlange, während der Agent
  arbeitet, und Diktat unter macOS.
- **Prüfen und ausliefern**: ein Changes-Panel mit Zeilenkommentaren, Checkpoints pro Turn und Zurückspulen,
  Worktrees pro Thread, Branches, Commit, Push und Pull Requests mit dem jeweils eigenen GitHub-Login des Kontos.
- **Befehlspalette** (<kbd>⌘K</kbd>), um jede Aktion auszuführen, das Projekt zu wechseln oder einen Thread zu öffnen
  (auch über seine Nachrichten auffindbar); <kbd>⌘P</kbd> findet eine Datei. Jedes Tastenkürzel lässt sich neu belegen.
- **Werkzeuge daneben**: ein eingebettetes Terminal mit Splits, ein Browser-Bereich, den Agents steuern können, und
  ein Geräte-Hub für iOS-Simulatoren und Android-Emulatoren.
- **Viele Threads gleichzeitig**: eine Liste über alle Projekte, geplante Aufgaben, Nebenchats, ein Prompt an
  mehrere Modelle gleichzeitig und Agents, die andere Threads starten und steuern (nur auf demselben Konto).
- **Deine Rechner**: Teile einen Rechner über dein Tailscale-Tailnet und führe Threads darauf von einem anderen aus.
  Ein mobiler Client ist in Arbeit.
- **Onboarding, Themes und Updates**: eine Einführung beim ersten Start, die deine vorhandenen Agent-Ordner
  importiert, fünf Themes mit hellen und dunklen Varianten und In-App-Updates, die gegen die SHA-256-Liste des
  Releases geprüft werden.

Das [Benutzerhandbuch](docs/user-guide.md) beschreibt jede dieser Funktionen ausführlich.

## Screenshots

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/images/thread-dark.png">
  <img src="docs/images/thread-light.png" alt="Ein Thread mit reichhaltiger Timeline und seinem Changes-Panel">
</picture>

| | |
|---|---|
| <picture><source media="(prefers-color-scheme: dark)" srcset="docs/images/composer-dark.png"><img src="docs/images/composer-light.png" alt="Der Composer mit Erwähnungs-Chips und der Modellauswahl"></picture> | <picture><source media="(prefers-color-scheme: dark)" srcset="docs/images/palette-dark.png"><img src="docs/images/palette-light.png" alt="Die Befehlspalette"></picture> |
| Der Composer mit Erwähnungs-Chips und der Modellauswahl | Die Befehlspalette |
| <picture><source media="(prefers-color-scheme: dark)" srcset="docs/images/onboarding-dark.png"><img src="docs/images/onboarding-light.png" alt="Onboarding beim ersten Start"></picture> | <picture><source media="(prefers-color-scheme: dark)" srcset="docs/images/themes-dark.png"><img src="docs/images/themes-light.png" alt="Settings, Appearance"></picture> |
| Onboarding beim ersten Start | Settings → Appearance |

## Installation

Lade den neuesten Build von der [Releases-Seite](https://github.com/OpenSkye-Labs/skyelab/releases) herunter:

| Plattform | Datei |
|---|---|
| macOS 14+ (Apple Silicon) | `SkyeLab_<version>_aarch64.dmg`: öffnen und SkyeLab in den Programme-Ordner ziehen. |
| Windows (x64) | `skyelab_<version>_x64-setup.exe` |
| Linux (x86_64) | `skyelab_<version>_amd64.deb` (`sudo apt install ./skyelab_<version>_amd64.deb`) oder `skyelab_<version>_x86_64.AppImage` (`chmod +x`, dann ausführen) |

**Erster Start unter macOS.** Release-Builds sind noch nicht notarisiert, deshalb blockiert Gatekeeper den ersten
Start. Öffne SkyeLab einmal, geh dann zu Systemeinstellungen → Datenschutz & Sicherheit und klicke auf **Open Anyway**.
Auch Windows SmartScreen kann nachfragen (**More info** → **Run anyway**). Danach hält **Check for Updates** in der
App sie aktuell.

SkyeLab installiert die Agents nicht. Installiere [Claude Code](https://docs.claude.com/en/docs/claude-code) (`claude`),
die [Codex CLI](https://github.com/openai/codex) (`codex`), falls du Codex nutzt, und die
[GitHub CLI](https://cli.github.com) (`gh`) für Pushes und Pull Requests.

Beim ersten Start findet das Onboarding deine vorhandenen Ordner `~/.claude*` und `~/.codex*`, fügt sie als Konten
hinzu und importiert die Projekte, in denen ihre Agents gearbeitet haben, jeweils auf dem Konto, das du bestätigst.
[Getting started](docs/getting-started.md) führt durch den Rest und hilft, wenn du von T3 Code kommst.

## Aus dem Quellcode bauen

Du brauchst das neueste stabile Rust ([rustup](https://rustup.rs)); `rust-toolchain.toml` wählt es aus.

- **macOS 14+:** volles Xcode plus die Metal-Toolchain (`xcodebuild -downloadComponent MetalToolchain`). Ohne sie
  schlägt der Build mit "metal shader compilation failed" fehl.
- **Linux:** `libxkbcommon-x11-dev libwayland-dev libx11-xcb-dev libvulkan1 libfontconfig-dev clang cmake libwebkit2gtk-4.1-dev libgtk-3-dev`
- **Windows:** die MSVC-Toolchain.

```sh
cargo run -p skyelab-app                              # run a debug build
cargo install cargo-packager --locked                 # or build the macOS app and disk image:
cargo packager --release -p skyelab-app -f app,dmg    # writes target/release/SkyeLab.app and a .dmg
```

## Dokumentation

- [Getting started](docs/getting-started.md): Installation, Konten, Bindungen, Sitzungen importieren, der Arbeitsalltag.
- [Tägliche Einrichtung, von T3 Code kommend](docs/user-guide-daily-setup.md): eine kurze Checkliste für den Umstieg.
- [Benutzerhandbuch](docs/user-guide.md): jede Funktion, Seite für Seite.
- [Sicherheitsmodell](docs/security.md): was SkyeLab garantiert und wie.
- [Architektur](docs/architecture.md): Crates, der In-Process-Daemon, Entscheidungen und Roadmap.
- [Mobile](docs/mobile.md): der iOS- und Android-Client.
- [Changelog](CHANGELOG.md)

| Plattform | Status |
|---|---|
| macOS 14+ (Apple Silicon) | Hauptplattform. Täglich genutzt und getestet. |
| Windows, Linux | In der CI gebaut und getestet. Weniger von Hand getestet. |
| iOS, Android | Remote-Client in Arbeit unter [`mobile/`](mobile). |

## Mitwirken

Beiträge sind willkommen. [CONTRIBUTING.md](CONTRIBUTING.md) beschreibt das Setup, die Prüfungen, die jede Änderung
bestehen muss, wie du UI-Änderungen ohne deine echten Konten testest und [wie die übersetzten READMEs aktualisiert
werden](CONTRIBUTING.md#translated-readmes). Bitte halte dich an den [Code of Conduct](CODE_OF_CONDUCT.md). Fragen
gehören in die [Discussions](https://github.com/OpenSkye-Labs/skyelab/discussions).

## Sicherheit

Ein Sicherheitsproblem gefunden? Melde es vertraulich, wie in [SECURITY.md](SECURITY.md) beschrieben. Öffne kein
öffentliches Issue.

## Lizenz

[MIT](LICENSE). Dank an [T3 Code](https://github.com/pingdotgg/t3code) (MIT) für das Design, dem dieses Projekt folgt.
