#!/usr/bin/env python3
"""Writes the generated theme families in crates/skyelab-app/themes/ from the palettes below.

Each variant names a handful of source colors (scripts/theme_palettes.py); every other key gpui-component reads is derived from them, hex only
(gpui-component silently drops color names it can't parse). Colors that miss SkyeLab's contrast floor are pulled
toward black (light) or white (dark) until they pass, and the script prints what it moved.
Sources and licenses: .agent/research/themes/README.md and the THEMES section of NOTICE.md.

Run: python3 scripts/gen-themes.py   (then cargo test -p skyelab-app contrast)
"""

import json
import pathlib
import sys

from theme_palettes import FAMILIES

OUT = pathlib.Path(__file__).resolve().parent.parent / "crates/skyelab-app/themes"


def rgb(h):
    h = h.lstrip("#")
    return tuple(int(h[i : i + 2], 16) / 255 for i in (0, 2, 4))


def hexa(c):
    return "#" + "".join(f"{round(max(0, min(1, v)) * 255):02x}" for v in c)


def lum(h):
    f = lambda v: v / 12.92 if v <= 0.03928 else ((v + 0.055) / 1.055) ** 2.4
    r, g, b = map(f, rgb(h))
    return 0.2126 * r + 0.7152 * g + 0.0722 * b


def ratio(a, b):
    x, y = lum(a), lum(b)
    return (max(x, y) + 0.05) / (min(x, y) + 0.05)


def mix(a, b, t):
    """a blended toward b by t (0..1)."""
    return hexa(tuple(x + (y - x) * t for x, y in zip(rgb(a), rgb(b))))


def oklab(h):
    lin = lambda v: v / 12.92 if v <= 0.04045 else ((v + 0.055) / 1.055) ** 2.4
    r, g, b = map(lin, rgb(h))
    l = (0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b) ** (1 / 3)
    m = (0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b) ** (1 / 3)
    s = (0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b) ** (1 / 3)
    return (0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s,
            1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s,
            0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s)


def from_oklab(L, a, b):
    enc = lambda v: 12.92 * v if v <= 0.0031308 else 1.055 * v ** (1 / 2.4) - 0.055
    l = (L + 0.3963377774 * a + 0.2158037573 * b) ** 3
    m = (L - 0.1055613458 * a - 0.0638541728 * b) ** 3
    s = (L - 0.0894841775 * a - 1.2914855480 * b) ** 3
    return hexa(tuple(enc(max(0, v)) for v in (
        4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
        -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
        -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s)))


def ink(c, fg, dark):
    """A status color as text, as `ui::ink` draws it: as is in dark themes, 55% of it and 45% foreground (Oklab) in light."""
    if dark:
        return c
    x, y = oklab(c), oklab(fg)
    return from_oklab(*(p * 0.55 + q * 0.45 for p, q in zip(x, y)))


MOVED = []

# Machado et al. (2009) full-severity simulation matrices, in linear RGB.
CVD = {
    "deuteranopia": [[0.367322, 0.860646, -0.227968], [0.280085, 0.672501, 0.047413], [-0.011820, 0.042940, 0.968881]],
    "protanopia": [[0.152286, 1.052583, -0.204868], [0.114503, 0.786281, 0.099216], [-0.003882, -0.048116, 1.051998]],
}


def cvd(h, m):
    lin = lambda v: v / 12.92 if v <= 0.04045 else ((v + 0.055) / 1.055) ** 2.4
    enc = lambda v: 12.92 * v if v <= 0.0031308 else 1.055 * v ** (1 / 2.4) - 0.055
    r = [lin(x) for x in rgb(h)]
    return hexa(tuple(enc(max(0, min(1, sum(m[i][j] * r[j] for j in range(3))))) for i in range(3)))


def dist(a, b):
    """RGB distance, 0..441."""
    return sum((x - y) ** 2 for x, y in zip(rgb(a), rgb(b))) ** 0.5 * 255


def fit(color, against, target, dark, what):
    """`color`, pulled toward white (dark themes) or black (light) until it reaches `target` against `against`."""
    out, pole = color, ("#ffffff" if dark else "#000000")
    t = 0.0
    while ratio(out, against) < target and t < 1:
        t += 0.02
        out = mix(color, pole, t)
    if out != color:
        MOVED.append(f"{what}: {color} -> {out} ({ratio(out, against):.2f})")
    return out


def variant(name, dark, p):
    bg, fg = p["bg"], p["fg"]
    side, card, border = p["sidebar"], p["card"], p["border"]
    w = name
    muted = p["muted"]
    # The canvases plus the tinted fills muted text sits on (secondary, accent and muted backgrounds).
    for s in (bg, side, card, mix(bg, fg, 0.06), mix(bg, fg, 0.03)):
        muted = fit(muted, s, 4.5, dark, f"{w} muted")
    accent, on_accent = p["accent"], p.get("on_accent")
    if on_accent is None:
        on_accent = "#ffffff" if ratio("#ffffff", accent) >= ratio(bg, accent) else bg
    # The fill moves away from its text: darker under white text, lighter under dark text.
    accent = fit(accent, on_accent, 4.5, lum(on_accent) < 0.2, f"{w} accent fill")
    link = p.get("link", accent)
    for s in (bg, side, card):
        link = fit(link, s, 4.5, dark, f"{w} link")
    status = {}
    for k in ("red", "green", "yellow", "blue"):
        c = p[k]
        for s in (bg, side, card):
            c = fit(c, s, 3.0, dark, f"{w} {k}")
        status[k] = c
    # Diff add and remove must stay apart for deuteranopes and protanopes too: when the palette's green and red
    # collapse, success steps toward the palette's teal (the usual color-blind-safe pair is teal and red), then
    # away from the red in lightness.
    def apart(c):
        return min(dist(cvd(c, m), cvd(status["red"], m)) for m in CVD.values())
    best = status["green"]
    pole = "#ffffff" if dark else "#000000"
    steps = [(p["cyan"], t) for t in (0.25, 0.5, 0.75, 1.0)] + [(pole, t) for t in (0.2, 0.35, 0.5)]
    for toward, t in steps:
        if apart(best) >= 45:
            break
        c = mix(p["green"], toward, t)
        for s in (bg, side, card):
            c = fit(c, s, 3.0, dark, f"{w} green (toward teal)")
        if apart(c) > apart(best):
            best = c
    status["green"] = best
    on = lambda c: "#ffffff" if ratio("#ffffff", c) >= ratio("#111111", c) else "#111111"

    # Status colors are also text (`ui::ink`) on every canvas and on the warning callout's tint, and button fills
    # under white or near-black text: all 4.5:1 (4.6 here: gpui's Hsla round trip costs a few hundredths). Step away
    # from the canvas until all of them hold.
    def readable(k, c):
        surfaces = [bg, side, card, mix(bg, c, 0.16 if dark else 0.08)]
        ok = lambda x: (min(ratio(ink(x, fg, dark), s) for s in surfaces) >= 4.6
                        and ratio(on(x), x) >= 4.6)
        out, t = c, 0.0
        while not ok(out) and t < 1:
            t += 0.02
            out = mix(c, pole, t)
            surfaces[3] = mix(bg, out, 0.16 if dark else 0.08)
        if out != c:
            MOVED.append(f"{w} {k} as text: {c} -> {out}")
        return out

    status = {k: readable(k, c) for k, c in status.items()}
    tint = lambda t: mix(bg, fg, t)
    hover = (lambda c, t: mix(c, "#ffffff" if dark else "#000000", t))
    a = lambda c, alpha: c + alpha
    colors = {
        "background": bg,
        "overlay": a(bg, "bf"),
        "foreground": fg,
        "border": border,
        "input.border": mix(border, fg, 0.12),
        "ring": accent,
        "caret": fg,
        "selection.background": a(accent, "4d" if dark else "33"),
        "muted.background": mix(bg, fg, 0.03),
        "muted.foreground": muted,
        "accent.background": tint(0.06),
        "accent.foreground": fg,
        "secondary.background": tint(0.06),
        "secondary.hover.background": tint(0.10),
        "secondary.active.background": tint(0.14),
        "secondary.foreground": fg,
        "primary.background": accent,
        "primary.hover.background": hover(accent, 0.08),
        "primary.active.background": mix(accent, "#000000", 0.1),
        "primary.foreground": on_accent,
        "popover.background": card,
        "popover.foreground": fg,
        "list.background": bg,
        "list.hover.background": tint(0.05),
        "list.active.background": a(accent, "26" if dark else "1a"),
        "list.active.border": accent,
        "list.even.background": bg,
        "list.head.background": side,
        "sidebar.background": side,
        "sidebar.foreground": fg,
        "sidebar.border": border,
        "sidebar.accent.background": mix(side, fg, 0.08) if dark else p.get("sidebar_pick", card),
        "sidebar.accent.foreground": fg,
        "sidebar.primary.background": accent,
        "sidebar.primary.foreground": on_accent,
        "tab_bar.background": side,
        "tab.background": side,
        "tab.foreground": muted,
        "tab.active.background": bg,
        "tab.active.foreground": fg,
        "title_bar.background": side,
        "title_bar.border": border,
        "status_bar.background": side,
        "status_bar.border": border,
        "danger.background": status["red"],
        "danger.foreground": on(status["red"]),
        "success.background": status["green"],
        "success.foreground": on(status["green"]),
        "warning.background": status["yellow"],
        "warning.foreground": on(status["yellow"]),
        "info.background": status["blue"],
        "info.foreground": on(status["blue"]),
        "link": link,
        "scrollbar.thumb.background": a(fg, "40" if dark else "33"),
        "scrollbar.thumb.hover.background": a(fg, "66" if dark else "59"),
        "base.blue": status["blue"],
        "base.cyan": p["cyan"],
        "base.green": status["green"],
        "base.magenta": p["magenta"],
        "base.yellow": status["yellow"],
        "base.red": status["red"],
    }
    assert len(p["ansi"]) == 16, name
    # 3:1 on the canvas, except the canvas's own side (black on dark, white on light), as terminal_colors_test checks.
    ansi = [c if i in ((0,) if dark else (7, 15)) else fit(c, bg, 3.0, dark, f"{w} ansi {i}")
            for i, c in enumerate(p["ansi"])]
    return {
        "name": name,
        "terminal.ansi": [c.lower() for c in ansi],
        "mode": "dark" if dark else "light",
        "font.size": 14,
        "mono_font.size": 13,
        "radius": 8,
        "radius.lg": 12,
        "shadow": False,
        "colors": {k: v.lower() for k, v in colors.items()},
    }


def main():
    for key, (label, source, sides) in FAMILIES.items():
        doc = {
            "name": label,
            "author": "SkyeLab",
            "_generated": "scripts/gen-themes.py; edit the palette there, not this file",
            "_source": source,
            "themes": [
                variant(f"{label} Light", False, sides["light"]),
                variant(f"{label} Dark", True, sides["dark"]),
            ],
        }
        (OUT / f"{key}.json").write_text(json.dumps(doc, indent=2, ensure_ascii=False) + "\n")
    print("\n".join(MOVED) or "nothing moved", file=sys.stderr)


if __name__ == "__main__":
    main()
