<p align="center">
  <img src="docs/images/banner.png" alt="SkyeLab" width="720">
</p>

<p align="center">
  <a href="README.md">English</a> ·
  <a href="README.zh-CN.md">简体中文</a> ·
  <a href="README.zh-TW.md">繁體中文</a> ·
  <a href="README.ja.md">日本語</a> ·
  <a href="README.ko.md">한국어</a> ·
  <b>Español</b> ·
  <a href="README.pt-BR.md">Português (Brasil)</a> ·
  <a href="README.de.md">Deutsch</a> ·
  <a href="README.fr.md">Français</a> ·
  <a href="README.ru.md">Русский</a>
</p>

> Esta es una traducción. El [README en inglés](README.md) es la fuente de verdad; última sincronización: 2026-11-25.

<p align="center">
  <b>Una app de escritorio nativa para ejecutar agentes de programación, donde cada carpeta queda fijada a una sola cuenta.</b>
</p>

<p align="center">
  <a href="https://github.com/OpenSkye-Labs/skyelab/actions/workflows/ci.yml"><img src="https://github.com/OpenSkye-Labs/skyelab/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="https://github.com/OpenSkye-Labs/skyelab/releases"><img src="https://img.shields.io/github/v/release/OpenSkye-Labs/skyelab?include_prereleases&sort=semver" alt="Versión"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="Licencia: MIT"></a>
</p>

SkyeLab es un entorno de desarrollo agéntico (ADE) escrito en Rust con [GPUI](https://www.gpui.rs/). Ejecutas
agentes de Claude Code y Codex en hilos sobre tus proyectos, revisas lo que cambian y tienes una terminal, un navegador
y tus simuladores junto a ellos. Su forma general viene de [T3 Code](https://github.com/pingdotgg/t3code).

> **Estado: temprano.** Espera asperezas y cambios incompatibles. macOS es la plataforma principal.

## Por qué existe

Si tienes una cuenta de trabajo y una personal, tarde o temprano un agente se ejecuta en la equivocada. SkyeLab lo
hace imposible: **cada carpeta está vinculada a exactamente una cuenta (Claude o Codex), y ese vínculo se hace cumplir.**

- El agente de tu repositorio de trabajo siempre usa la cuenta de trabajo; el de tu proyecto personal, tu cuenta personal.
- Gana el vínculo más profundo, así que `~/work` puede ser una cuenta y `~/work/oss-fork` otra.
- Una carpeta sin vínculo nunca recurre a una cuenta por defecto. SkyeLab te pide vincularla primero.
- Si una ruta vinculada pasa a apuntar a otro lugar (un enlace simbólico cambiado, una carpeta movida), el vínculo queda
  *stale* (obsoleto) y ningún agente se inicia allí hasta que lo confirmes de nuevo.
- GitHub funciona igual: cada cuenta puede tener su propio inicio de sesión de `gh`, que se usa para commits, pushes y pull requests.

## Funciones

- **Hilos** para Claude Code y Codex, con una línea de tiempo completa: respuestas y razonamiento en streaming, tarjetas
  de herramientas, diffs en línea, salida del shell, listas de tareas, subagentes, preguntas, planes y aprobaciones de
  herramientas que respondes en el mismo lugar.
- **Compositor** con menciones de archivos, hilos y pull requests como chips en línea, imágenes y archivos, comandos `/`,
  selector de modelo y de esfuerzo, un modo de permisos por hilo, una cola mientras el agente trabaja y dictado en macOS.
- **Revisar y publicar**: un panel Changes con comentarios por línea, checkpoints por turno y rebobinado, worktrees por
  hilo, ramas, commit, push y pull requests con el inicio de sesión de GitHub propio de cada cuenta.
- **Paleta de comandos** (<kbd>⌘K</kbd>) para ejecutar cualquier acción, cambiar de proyecto o abrir un hilo (también
  por sus mensajes); <kbd>⌘P</kbd> busca un archivo. Todos los atajos se pueden reasignar.
- **Herramientas laterales**: una terminal integrada con divisiones, un panel de navegador que los agentes pueden
  controlar y un centro de dispositivos para simuladores de iOS y emuladores de Android.
- **Muchos hilos a la vez**: una sola lista para todos los proyectos, tareas programadas, chats laterales, enviar un
  prompt a varios modelos y agentes que inician y controlan otros hilos (solo en la misma cuenta).
- **Tus máquinas**: comparte una máquina en tu tailnet de Tailscale y ejecuta hilos en ella desde otra. Hay un cliente
  móvil en desarrollo.
- **Bienvenida, temas y actualizaciones**: una guía de primer inicio que importa tus carpetas de agentes existentes,
  cinco temas con variante clara y oscura, y actualizaciones dentro de la app verificadas con la lista SHA-256 de la versión.

La [guía de usuario](docs/user-guide.md) cubre cada una en detalle.

## Capturas de pantalla

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/images/thread-dark.png">
  <img src="docs/images/thread-light.png" alt="Un hilo con su línea de tiempo y el panel Changes">
</picture>

| | |
|---|---|
| <picture><source media="(prefers-color-scheme: dark)" srcset="docs/images/composer-dark.png"><img src="docs/images/composer-light.png" alt="El compositor con chips de mención y el selector de modelo"></picture> | <picture><source media="(prefers-color-scheme: dark)" srcset="docs/images/palette-dark.png"><img src="docs/images/palette-light.png" alt="La paleta de comandos"></picture> |
| El compositor, con chips de mención y el selector de modelo | La paleta de comandos |
| <picture><source media="(prefers-color-scheme: dark)" srcset="docs/images/onboarding-dark.png"><img src="docs/images/onboarding-light.png" alt="Bienvenida del primer inicio"></picture> | <picture><source media="(prefers-color-scheme: dark)" srcset="docs/images/themes-dark.png"><img src="docs/images/themes-light.png" alt="Settings, Appearance (ajustes de apariencia)"></picture> |
| Bienvenida del primer inicio | Settings → Appearance |

## Instalación

Descarga la última versión desde la [página de versiones](https://github.com/OpenSkye-Labs/skyelab/releases):

| Plataforma | Archivo |
|---|---|
| macOS 14+ (Apple silicon) | `SkyeLab_0.1.1_aarch64.dmg`: ábrelo y arrastra SkyeLab a Applications. |
| Windows (x64) | `skyelab_0.1.1_x64-setup.exe` |
| Linux (x86_64) | `skyelab_0.1.1_amd64.deb` (`sudo apt install ./skyelab_0.1.1_amd64.deb`) o `skyelab_0.1.1_x86_64.AppImage` (`chmod +x` y luego ejecútalo) |

**Primera apertura en macOS.** Las versiones aún no están notarizadas, así que Gatekeeper bloquea la primera apertura.
Abre SkyeLab una vez, luego ve a System Settings → Privacy & Security y haz clic en **Open Anyway**. Windows SmartScreen
también puede preguntar (**More info** → **Run anyway**). Después de eso, **Check for Updates** en la app la mantiene al día.

SkyeLab no instala los agentes. Instala [Claude Code](https://docs.claude.com/en/docs/claude-code) (`claude`),
la [CLI de Codex](https://github.com/openai/codex) (`codex`) si usas Codex, y la
[CLI de GitHub](https://cli.github.com) (`gh`) para pushes y pull requests.

En el primer inicio, la bienvenida encuentra tus carpetas `~/.claude*` y `~/.codex*` existentes, las agrega como cuentas
e importa los proyectos en los que trabajaron sus agentes, cada uno en la cuenta que confirmes.
[Primeros pasos](docs/getting-started.md) explica el resto y te ayuda si vienes de T3 Code.

## Compilar desde el código fuente

Necesitas la última versión estable de Rust ([rustup](https://rustup.rs)); `rust-toolchain.toml` la selecciona.

- **macOS 14+:** Xcode completo más el toolchain de Metal (`xcodebuild -downloadComponent MetalToolchain`). Sin él,
  la compilación falla con "metal shader compilation failed".
- **Linux:** `libxkbcommon-x11-dev libwayland-dev libx11-xcb-dev libvulkan1 libfontconfig-dev clang cmake libwebkit2gtk-4.1-dev libgtk-3-dev`
- **Windows:** el toolchain de MSVC.

```sh
cargo run -p skyelab-app                              # run a debug build
cargo install cargo-packager --locked                 # or build the macOS app and disk image:
cargo packager --release -p skyelab-app -f app,dmg    # writes target/release/SkyeLab.app and a .dmg
```

## Documentación

- [Primeros pasos](docs/getting-started.md): instalación, cuentas, vínculos, importar sesiones, el flujo diario.
- [Guía de usuario](docs/user-guide.md): cada función, página por página.
- [Modelo de seguridad](docs/security.md): qué garantiza SkyeLab y cómo.
- [Arquitectura](docs/architecture.md): crates, el daemon dentro del proceso, decisiones y hoja de ruta.
- [Móvil](docs/mobile.md): el cliente para iOS y Android.
- [Registro de cambios](CHANGELOG.md)

| Plataforma | Estado |
|---|---|
| macOS 14+ (Apple silicon) | Principal. Se usa y prueba a diario. |
| Windows, Linux | Se compila y prueba en CI. Menos probado a mano. |
| iOS, Android | Cliente remoto en desarrollo en [`mobile/`](mobile). |

## Contribuir

Las contribuciones son bienvenidas. [CONTRIBUTING.md](CONTRIBUTING.md) explica la configuración, las comprobaciones que
todo cambio debe pasar, cómo probar cambios de UI sin tus cuentas reales y [cómo actualizar los README traducidos](CONTRIBUTING.md#translated-readmes).
Sigue el [Código de conducta](CODE_OF_CONDUCT.md). Las preguntas van en
[Discussions](https://github.com/OpenSkye-Labs/skyelab/discussions).

## Seguridad

¿Encontraste un problema de seguridad? Repórtalo en privado, como describe [SECURITY.md](SECURITY.md). No abras un issue público.

## Licencia

[MIT](LICENSE). Crédito a [T3 Code](https://github.com/pingdotgg/t3code) (MIT) por el diseño que sigue este proyecto.
