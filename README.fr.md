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
  <a href="README.de.md">Deutsch</a> ·
  <b>Français</b> ·
  <a href="README.ru.md">Русский</a>
</p>

> Ceci est une traduction. Le [README anglais](README.md) fait foi ; dernière synchronisation avec lui le 2026-11-25.

<p align="center">
  <b>Une application de bureau native pour lancer des agents de code, où chaque dossier est verrouillé sur un seul compte.</b>
</p>

<p align="center">
  <a href="https://github.com/OpenSkye-Labs/skyelab/actions/workflows/ci.yml"><img src="https://github.com/OpenSkye-Labs/skyelab/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="https://github.com/OpenSkye-Labs/skyelab/releases"><img src="https://img.shields.io/github/v/release/OpenSkye-Labs/skyelab?include_prereleases&sort=semver" alt="Version"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="Licence : MIT"></a>
</p>

SkyeLab est un environnement de développement agentique (ADE) écrit en Rust avec [GPUI](https://www.gpui.rs/). Vous y
lancez des agents Claude Code et Codex dans des threads sur vos projets, vous relisez ce qu'ils modifient, et vous
gardez un terminal, un navigateur et vos simulateurs à côté. Sa forme générale s'inspire de [T3 Code](https://github.com/pingdotgg/t3code).

> **Statut : précoce.** Attendez-vous à des aspérités et à des changements incompatibles. macOS est la plateforme principale.

## Pourquoi il existe

Si vous avez un compte professionnel et un compte personnel, tôt ou tard un agent tourne sur le mauvais. SkyeLab
rend cela impossible : **chaque dossier est lié à exactement un compte (Claude ou Codex), et cette liaison est
imposée.**

- L'agent de votre dépôt professionnel tourne toujours sur le compte professionnel ; celui de votre projet perso, sur
  votre compte personnel.
- La liaison la plus profonde l'emporte : `~/work` peut utiliser un compte et `~/work/oss-fork` un autre.
- Un dossier sans liaison ne retombe jamais sur un compte par défaut. SkyeLab vous demande d'abord de le lier.
- Si un chemin lié pointe plus tard ailleurs (un lien symbolique remplacé, un dossier déplacé), la liaison devient
  *périmée* et aucun agent n'y démarre tant que vous ne l'avez pas confirmée à nouveau.
- GitHub fonctionne de la même façon : chaque compte peut avoir sa propre connexion `gh`, utilisée pour les commits,
  les pushes et les pull requests.

## Fonctionnalités

- **Threads** pour Claude Code et Codex, avec une timeline riche : réponses et réflexion en streaming, cartes
  d'outils, diffs en ligne, sortie du shell, listes de tâches, sous-agents, questions, plans et autorisations d'outils
  auxquels vous répondez sur place.
- **Composer** avec mentions de fichiers, de threads et de pull requests sous forme de puces en ligne, images et
  fichiers, commandes `/`, choix du modèle et de l'effort, mode de permission par thread, file d'attente pendant que
  l'agent travaille, et dictée sur macOS.
- **Relire et livrer** : un panneau Changes avec commentaires de ligne, points de contrôle par tour et retour en
  arrière, worktrees par thread, branches, commit, push et pull requests avec la connexion GitHub propre à chaque compte.
- **Palette de commandes** (<kbd>⌘K</kbd>) pour lancer n'importe quelle action, changer de projet ou ouvrir un thread
  (on le retrouve aussi par ses messages) ; <kbd>⌘P</kbd> trouve un fichier. Chaque raccourci peut être réaffecté.
- **Outils annexes** : un terminal intégré avec divisions, un panneau de navigateur que les agents peuvent piloter, et
  un hub d'appareils pour les simulateurs iOS et les émulateurs Android.
- **Plusieurs threads à la fois** : une seule liste pour tous les projets, des tâches planifiées, des discussions
  annexes, l'envoi d'un même prompt à plusieurs modèles, et des agents qui lancent et pilotent d'autres threads
  (uniquement sur le même compte).
- **Vos machines** : partagez une machine sur votre tailnet Tailscale et lancez-y des threads depuis une autre. Un
  client mobile est en cours de développement.
- **Prise en main, thèmes et mises à jour** : un guide au premier lancement qui importe vos dossiers d'agents
  existants, cinq thèmes avec variantes claire et sombre, et des mises à jour intégrées vérifiées par rapport à la
  liste SHA-256 de la version.

Le [guide utilisateur](docs/user-guide.md) détaille chacune de ces fonctionnalités.

## Captures d'écran

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/images/thread-dark.png">
  <img src="docs/images/thread-light.png" alt="Un thread avec une timeline riche et son panneau Changes">
</picture>

| | |
|---|---|
| <picture><source media="(prefers-color-scheme: dark)" srcset="docs/images/composer-dark.png"><img src="docs/images/composer-light.png" alt="Le composer avec les puces de mention et le choix du modèle"></picture> | <picture><source media="(prefers-color-scheme: dark)" srcset="docs/images/palette-dark.png"><img src="docs/images/palette-light.png" alt="La palette de commandes"></picture> |
| Le composer, avec les puces de mention et le choix du modèle | La palette de commandes |
| <picture><source media="(prefers-color-scheme: dark)" srcset="docs/images/onboarding-dark.png"><img src="docs/images/onboarding-light.png" alt="Prise en main au premier lancement"></picture> | <picture><source media="(prefers-color-scheme: dark)" srcset="docs/images/themes-dark.png"><img src="docs/images/themes-light.png" alt="Settings, Appearance"></picture> |
| Prise en main au premier lancement | Settings → Appearance |

## Installation

Téléchargez la dernière version depuis la [page des releases](https://github.com/OpenSkye-Labs/skyelab/releases) :

| Plateforme | Fichier |
|---|---|
| macOS 14+ (Apple silicon) | `SkyeLab_<version>_aarch64.dmg` : ouvrez-le et faites glisser SkyeLab dans Applications. |
| Windows (x64) | `skyelab_<version>_x64-setup.exe` |
| Linux (x86_64) | `skyelab_<version>_amd64.deb` (`sudo apt install ./skyelab_<version>_amd64.deb`) ou `skyelab_<version>_x86_64.AppImage` (`chmod +x`, puis lancez-le) |

**Premier lancement sur macOS.** Les versions publiées ne sont pas encore notarisées, donc Gatekeeper bloque le
premier lancement. Ouvrez SkyeLab une fois, puis allez dans Réglages Système → Confidentialité et sécurité et cliquez
sur **Open Anyway**. Windows SmartScreen peut aussi demander confirmation (**More info** → **Run anyway**). Ensuite,
**Check for Updates** dans l'application la maintient à jour.

SkyeLab n'installe pas les agents. Installez [Claude Code](https://docs.claude.com/en/docs/claude-code) (`claude`),
la [CLI Codex](https://github.com/openai/codex) (`codex`) si vous utilisez Codex, et la
[CLI GitHub](https://cli.github.com) (`gh`) pour les pushes et les pull requests.

Au premier démarrage, la prise en main trouve vos dossiers `~/.claude*` et `~/.codex*` existants, les ajoute comme
comptes et importe les projets dans lesquels leurs agents ont travaillé, chacun sur le compte que vous confirmez.
[Getting started](docs/getting-started.md) explique la suite, et vous aide si vous venez de T3 Code.

## Compiler depuis les sources

Il vous faut la dernière version stable de Rust ([rustup](https://rustup.rs)) ; `rust-toolchain.toml` la sélectionne.

- **macOS 14+ :** Xcode complet plus la toolchain Metal (`xcodebuild -downloadComponent MetalToolchain`). Sans elle,
  la compilation échoue avec "metal shader compilation failed".
- **Linux :** `libxkbcommon-x11-dev libwayland-dev libx11-xcb-dev libvulkan1 libfontconfig-dev clang cmake libwebkit2gtk-4.1-dev libgtk-3-dev`
- **Windows :** la toolchain MSVC.

```sh
cargo run -p skyelab-app                              # run a debug build
cargo install cargo-packager --locked                 # or build the macOS app and disk image:
cargo packager --release -p skyelab-app -f app,dmg    # writes target/release/SkyeLab.app and a .dmg
```

## Documentation

- [Getting started](docs/getting-started.md) : installation, comptes, liaisons, import de sessions, le travail au quotidien.
- [Guide utilisateur](docs/user-guide.md) : chaque fonctionnalité, page par page.
- [Modèle de sécurité](docs/security.md) : ce que SkyeLab garantit, et comment.
- [Architecture](docs/architecture.md) : les crates, le démon intégré au processus, les décisions et la feuille de route.
- [Mobile](docs/mobile.md) : le client iOS et Android.
- [Changelog](CHANGELOG.md)

| Plateforme | Statut |
|---|---|
| macOS 14+ (Apple silicon) | Principale. Utilisée et testée chaque jour. |
| Windows, Linux | Compilées et testées en CI. Moins testées à la main. |
| iOS, Android | Client distant en cours de développement dans [`mobile/`](mobile). |

## Contribuer

Les contributions sont les bienvenues. [CONTRIBUTING.md](CONTRIBUTING.md) couvre l'installation, les vérifications
que chaque changement doit passer, comment tester les changements d'interface sans vos vrais comptes, et [comment
mettre à jour les README traduits](CONTRIBUTING.md#translated-readmes). Merci de respecter le
[Code of Conduct](CODE_OF_CONDUCT.md). Les questions vont dans les
[Discussions](https://github.com/OpenSkye-Labs/skyelab/discussions).

## Sécurité

Vous avez trouvé un problème de sécurité ? Signalez-le en privé, comme décrit dans [SECURITY.md](SECURITY.md).
N'ouvrez pas d'issue publique.

## Licence

[MIT](LICENSE). Merci à [T3 Code](https://github.com/pingdotgg/t3code) (MIT) pour le design dont ce projet s'inspire.
