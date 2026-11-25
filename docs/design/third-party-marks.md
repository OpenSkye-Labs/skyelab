# Third-party marks

SkyeLab shows two provider logos so a person can tell at a glance which provider an account runs. They are the
brands' own files, unedited, in `crates/skyelab-app/icons/brand/`, and are drawn only through
`ui::provider_logo` (and `settings::provider_mark`, which puts one on a tile).

| File | Mark | Official color | Source |
|------|------|----------------|--------|
| `claude.svg` | Claude Spark, by Anthropic | `#D97757` ("Clay"), one variant for both themes | Anthropic press kit, linked as "Download press kit" from https://www.anthropic.com/news (`/press-kit` → `Anthropic media resources/Anthropic logos/Claude logos/3 Claude Spark/SVG/Claude Spark - Clay.svg`) |
| `openai-black.svg` | OpenAI Blossom, black | `black` | OpenAI's "Download logos" on https://openai.com/brand/ (https://cdn.openai.com/brand/openai-logos.zip → `OpenAI-logos/SVGs/OAI_OpenAI-Blossom_Black.svg`) |
| `openai-white.svg` | OpenAI Blossom, white | `white` | same zip, `OAI_OpenAI-Blossom_White.svg` |

Which variant shows: the Claude Spark has one published color, so it shows in Clay on both themes. OpenAI publishes the
Blossom in black and in white; SkyeLab shows the black file on light themes and the white file on dark themes. Neither
brand page we could read states a light/dark rule in words (Anthropic's /brand page is gone); this pairing is the one
that keeps each official file legible.

Rules (from OpenAI's guidelines: "Use the logo exactly as provided", "DON'T add any colors to the Blossom", "Do not
feature our Marks more prominently than your own company's name or marks"; we hold Anthropic's mark to the same):

- Draw a logo only as an image (`gpui::img`) of its file. Never as a `SkyeIcon`/`svg()` mask, which takes the text
  color; never tinted, recolored, faded or given opacity. `ui::provider_logo`'s test checks each file keeps its own
  fill and has no `currentColor`, opacity, style, script or embedded image.
- Account identity never goes on the logo. `provider_mark` puts the logo on a neutral tile and the account's color in
  a separate dot on the tile's corner.
- Use a mark only to name its provider: account cards, the composer's model chip and picker, onboarding. Places that
  take a glyph (palette rows) use a generic icon instead.
- Small sizes: 14–16 px. Keep a little space around the mark; don't put it on a busy background.
- The marks are trademarks of Anthropic PBC and OpenAI. To update one, download it again from the source above, copy
  it unchanged, and update this table and `icons/brand/THIRD_PARTY.md`.
- Don't copy any other app's logo or art (T3 Code's included).
