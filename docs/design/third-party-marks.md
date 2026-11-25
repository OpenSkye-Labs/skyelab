# Third-party marks

SkyeLab shows two provider logos so a person can tell at a glance which provider an account runs. They live in
`crates/skyelab-app/icons/ui/` and are drawn through `SkyeIcon::Claude` and `SkyeIcon::Codex`.

| Icon | Mark | Source | Artwork license |
|------|------|--------|-----------------|
| `SkyeIcon::Claude` (`claude.svg`) | Claude, by Anthropic | [simple-icons](https://github.com/simple-icons/simple-icons) 16.38.1 | CC0-1.0 |
| `SkyeIcon::Codex` (`codex.svg`) | OpenAI | [lobe-icons](https://github.com/lobehub/lobe-icons) (`@lobehub/icons-static-svg` 1.106.0) | MIT, © LobeHub |

simple-icons has no OpenAI mark, so that one comes from lobe-icons.

Rules:

- Use a mark only to name its provider (account cards, the composer's model chip and picker, onboarding, the palette).
  Never as SkyeLab's own logo or decoration, never recolored into another brand, never combined with other art.
- The files are single-path monochrome SVGs filled with `currentColor`: they take the text color (or the account's
  color in an account tile), so they read in both themes.
- The trademarks stay with Anthropic PBC and OpenAI; the license above covers only the drawing. To update one, take it
  again from the same source and update this table and `icons/ui/THIRD_PARTY.md`.
- Don't copy any other app's logo or art (T3 Code's included).
