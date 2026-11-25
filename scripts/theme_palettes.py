"""The source palettes scripts/gen-themes.py builds each theme family from: per side, a few colors and the terminal's
16 ANSI colors. Sources and licenses: .agent/research/themes/README.md and the THEMES section of NOTICE.md.
"""

FAMILIES = {
    # SkyeLab's own default: porcelain paper and ink by day, the launcher's slate-midnight by night, one calm blue.
    "skyelab": ("SkyeLab", "SkyeLab's own palette, from the launcher's porcelain and midnight slate", {
        "light": dict(
            bg="#fbfaf8", sidebar="#f4f3f0", card="#ffffff", fg="#16181d", muted="#5c616b", border="#e4e2dd",
            accent="#3358d4", sidebar_pick="#ffffff",
            red="#c9372c", green="#1f7a4d", yellow="#a86400", blue="#2f62c9", cyan="#127a8a", magenta="#a3449c",
            ansi=["#16181d", "#c9372c", "#1f7a4d", "#a86400", "#3358d4", "#a3449c", "#127a8a", "#9a9ea6",
                  "#5c616b", "#a52c23", "#17633e", "#8a5200", "#2847b0", "#86377f", "#0e6370", "#d4d6db"],
        ),
        "dark": dict(
            bg="#111318", sidebar="#0c0e12", card="#181b21", fg="#e6e8ec", muted="#9aa0ab", border="#252932",
            accent="#4c6fe0", link="#8aa4f8",
            red="#f0706a", green="#4fc68a", yellow="#e8b04b", blue="#6f97f5", cyan="#4cc3cf", magenta="#d68ad0",
            ansi=["#2a2e37", "#f0706a", "#4fc68a", "#e8b04b", "#6f97f5", "#d68ad0", "#4cc3cf", "#c9cdd5",
                  "#5d6370", "#ff8f89", "#79dba6", "#f5c873", "#93b1ff", "#e8a8e2", "#7ad8e1", "#f4f5f7"],
        ),
    }),
    "catppuccin": ("Catppuccin", "Catppuccin Latte and Mocha (MIT)", {
        "light": dict(
            bg="#eff1f5", sidebar="#e6e9ef", card="#ffffff", fg="#4c4f69", muted="#5c5f77", border="#ccd0da",
            accent="#8839ef", red="#d20f39", green="#40a02b", yellow="#df8e1d", blue="#1e66f5", cyan="#179299",
            magenta="#ea76cb",
            ansi=["#5c5f77", "#d20f39", "#40a02b", "#df8e1d", "#1e66f5", "#ea76cb", "#179299", "#acb0be",
                  "#6c6f85", "#d20f39", "#40a02b", "#df8e1d", "#1e66f5", "#ea76cb", "#179299", "#bcc0cc"],
        ),
        "dark": dict(
            bg="#1e1e2e", sidebar="#181825", card="#24273a", fg="#cdd6f4", muted="#a6adc8", border="#313244",
            accent="#cba6f7", on_accent="#11111b", red="#f38ba8", green="#a6e3a1", yellow="#f9e2af",
            blue="#89b4fa", cyan="#94e2d5", magenta="#f5c2e7",
            ansi=["#45475a", "#f38ba8", "#a6e3a1", "#f9e2af", "#89b4fa", "#f5c2e7", "#94e2d5", "#bac2de",
                  "#585b70", "#f38ba8", "#a6e3a1", "#f9e2af", "#89b4fa", "#f5c2e7", "#94e2d5", "#a6adc8"],
        ),
    }),
    "rose-pine": ("Rosé Pine", "Rosé Pine Dawn and main (MIT)", {
        "light": dict(
            bg="#faf4ed", sidebar="#f2e9e1", card="#fffaf3", fg="#575279", muted="#797593", border="#dfdad9",
            accent="#286983", red="#b4637a", green="#286983", yellow="#ea9d34", blue="#56949f", cyan="#56949f",
            magenta="#907aa9", sidebar_pick="#fffaf3",
            ansi=["#575279", "#b4637a", "#286983", "#ea9d34", "#56949f", "#907aa9", "#d7827e", "#f2e9e1",
                  "#9893a5", "#b4637a", "#286983", "#ea9d34", "#56949f", "#907aa9", "#d7827e", "#fffaf3"],
        ),
        "dark": dict(
            bg="#191724", sidebar="#1f1d2e", card="#26233a", fg="#e0def4", muted="#908caa", border="#403d52",
            accent="#c4a7e7", on_accent="#191724", link="#c4a7e7", red="#eb6f92", green="#9ccfd8",
            yellow="#f6c177", blue="#31748f", cyan="#9ccfd8", magenta="#c4a7e7",
            ansi=["#26233a", "#eb6f92", "#31748f", "#f6c177", "#9ccfd8", "#c4a7e7", "#ebbcba", "#e0def4",
                  "#6e6a86", "#eb6f92", "#31748f", "#f6c177", "#9ccfd8", "#c4a7e7", "#ebbcba", "#e0def4"],
        ),
    }),
    "tokyo-night": ("Tokyo Night", "folke/tokyonight.nvim Day and Night (Apache-2.0)", {
        "light": dict(
            bg="#e1e2e7", sidebar="#e9e9ed", card="#f2f2f5", fg="#3760bf", muted="#6172b0", border="#c4c8da",
            accent="#2e7de9", red="#f52a65", green="#587539", yellow="#8c6c3e", blue="#2e7de9", cyan="#007197",
            magenta="#9854f1", sidebar_pick="#f2f2f5",
            ansi=["#3760bf", "#f52a65", "#587539", "#8c6c3e", "#2e7de9", "#9854f1", "#007197", "#e9e9ed",
                  "#a1a6c5", "#f52a65", "#587539", "#8c6c3e", "#2e7de9", "#9854f1", "#007197", "#ffffff"],
        ),
        "dark": dict(
            bg="#1a1b26", sidebar="#16161e", card="#1f2335", fg="#c0caf5", muted="#a9b1d6", border="#292e42",
            accent="#7aa2f7", on_accent="#16161e", red="#f7768e", green="#9ece6a", yellow="#e0af68",
            blue="#7aa2f7", cyan="#7dcfff", magenta="#bb9af7",
            ansi=["#15161e", "#f7768e", "#9ece6a", "#e0af68", "#7aa2f7", "#bb9af7", "#7dcfff", "#a9b1d6",
                  "#414868", "#f7768e", "#9ece6a", "#e0af68", "#7aa2f7", "#bb9af7", "#7dcfff", "#c0caf5"],
        ),
    }),
    "kanagawa": ("Kanagawa", "rebelot/kanagawa.nvim Lotus and Wave (MIT)", {
        "light": dict(
            bg="#f2ecbc", sidebar="#e7dba0", card="#f6f1cf", fg="#545464", muted="#716e61", border="#d5cea3",
            accent="#4d699b", red="#c84053", green="#6f894e", yellow="#77713f", blue="#4d699b", cyan="#597b75",
            magenta="#b35b79", sidebar_pick="#f6f1cf",
            ansi=["#1f1f28", "#c84053", "#6f894e", "#77713f", "#4d699b", "#b35b79", "#597b75", "#545464",
                  "#8a8980", "#d7474b", "#6e915f", "#836f4a", "#6693bf", "#624c83", "#5e857a", "#43436c"],
        ),
        "dark": dict(
            bg="#1f1f28", sidebar="#16161d", card="#2a2a37", fg="#dcd7ba", muted="#a6a69c", border="#363646",
            accent="#7e9cd8", on_accent="#16161d", red="#e46876", green="#98bb6c", yellow="#e6c384",
            blue="#7e9cd8", cyan="#7aa89f", magenta="#957fb8",
            ansi=["#16161d", "#c34043", "#76946a", "#c0a36e", "#7e9cd8", "#957fb8", "#6a9589", "#c8c093",
                  "#727169", "#e82424", "#98bb6c", "#e6c384", "#7fb4ca", "#938aa9", "#7aa89f", "#dcd7ba"],
        ),
    }),
    "everforest": ("Everforest", "sainnhe/everforest medium light and dark (MIT)", {
        "light": dict(
            bg="#fdf6e3", sidebar="#f4f0d9", card="#fffbef", fg="#5c6a72", muted="#829181", border="#e6e2cc",
            accent="#8da101", red="#f85552", green="#8da101", yellow="#dfa000", blue="#3a94c5", cyan="#35a77c",
            magenta="#df69ba", sidebar_pick="#fffbef",
            ansi=["#5c6a72", "#f85552", "#8da101", "#dfa000", "#3a94c5", "#df69ba", "#35a77c", "#dfddc8",
                  "#829181", "#f85552", "#8da101", "#dfa000", "#3a94c5", "#df69ba", "#35a77c", "#e6e2cc"],
        ),
        "dark": dict(
            bg="#2d353b", sidebar="#232a2e", card="#343f44", fg="#d3c6aa", muted="#9da9a0", border="#475258",
            accent="#a7c080", on_accent="#232a2e", red="#e67e80", green="#a7c080", yellow="#dbbc7f",
            blue="#7fbbb3", cyan="#83c092", magenta="#d699b6",
            ansi=["#343f44", "#e67e80", "#a7c080", "#dbbc7f", "#7fbbb3", "#d699b6", "#83c092", "#d3c6aa",
                  "#5c6a72", "#e67e80", "#a7c080", "#dbbc7f", "#7fbbb3", "#d699b6", "#83c092", "#d3c6aa"],
        ),
    }),
    "gruvbox": ("Gruvbox", "morhetz/gruvbox light and dark (MIT/X11)", {
        "light": dict(
            bg="#fbf1c7", sidebar="#f2e5bc", card="#f9f5d7", fg="#3c3836", muted="#7c6f64", border="#d5c4a1",
            accent="#af3a03", red="#9d0006", green="#79740e", yellow="#b57614", blue="#076678", cyan="#427b58",
            magenta="#8f3f71", sidebar_pick="#f9f5d7",
            ansi=["#3c3836", "#cc241d", "#98971a", "#d79921", "#458588", "#b16286", "#689d6a", "#fbf1c7",
                  "#928374", "#9d0006", "#79740e", "#b57614", "#076678", "#8f3f71", "#427b58", "#f9f5d7"],
        ),
        "dark": dict(
            bg="#282828", sidebar="#1d2021", card="#32302f", fg="#ebdbb2", muted="#a89984", border="#3c3836",
            accent="#fabd2f", on_accent="#282828", link="#83a598", red="#fb4934", green="#b8bb26",
            yellow="#fabd2f", blue="#83a598", cyan="#8ec07c", magenta="#d3869b",
            ansi=["#282828", "#cc241d", "#98971a", "#d79921", "#458588", "#b16286", "#689d6a", "#a89984",
                  "#928374", "#fb4934", "#b8bb26", "#fabd2f", "#83a598", "#d3869b", "#8ec07c", "#ebdbb2"],
        ),
    }),
    "nord": ("Nord", "nordtheme/nord (MIT); the light side is SkyeLab's, from Nord's Snow Storm and Frost", {
        "light": dict(
            bg="#eceff4", sidebar="#e5e9f0", card="#f8f9fb", fg="#2e3440", muted="#4c566a", border="#d8dee9",
            accent="#5e81ac", red="#bf616a", green="#a3be8c", yellow="#d08770", blue="#5e81ac", cyan="#88c0d0",
            magenta="#b48ead", sidebar_pick="#f8f9fb",
            ansi=["#3b4252", "#a5444f", "#5d7a45", "#9a6a1e", "#4c6b93", "#86607f", "#3e7f8d", "#d8dee9",
                  "#4c566a", "#bf616a", "#6f8f55", "#b07d2a", "#5e81ac", "#9a7394", "#4e95a3", "#e5e9f0"],
        ),
        "dark": dict(
            bg="#2e3440", sidebar="#272c36", card="#3b4252", fg="#eceff4", muted="#b4bccb", border="#434c5e",
            accent="#88c0d0", on_accent="#2e3440", red="#bf616a", green="#a3be8c", yellow="#ebcb8b",
            blue="#81a1c1", cyan="#8fbcbb", magenta="#b48ead",
            ansi=["#3b4252", "#bf616a", "#a3be8c", "#ebcb8b", "#81a1c1", "#b48ead", "#88c0d0", "#e5e9f0",
                  "#4c566a", "#bf616a", "#a3be8c", "#ebcb8b", "#81a1c1", "#b48ead", "#8fbcbb", "#eceff4"],
        ),
    }),
    "flexoki": ("Flexoki", "kepano/flexoki light and dark (MIT)", {
        "light": dict(
            bg="#fffcf0", sidebar="#f2f0e5", card="#ffffff", fg="#100f0f", muted="#6f6e69", border="#e6e4d9",
            accent="#205ea6", red="#af3029", green="#66800b", yellow="#ad8301", blue="#205ea6", cyan="#24837b",
            magenta="#a02f6f", sidebar_pick="#fffcf0",
            ansi=["#100f0f", "#af3029", "#66800b", "#ad8301", "#205ea6", "#a02f6f", "#24837b", "#878580",
                  "#6f6e69", "#d14d41", "#879a39", "#d0a215", "#4385be", "#ce5d97", "#3aa99f", "#cecdc3"],
        ),
        "dark": dict(
            bg="#100f0f", sidebar="#1c1b1a", card="#1c1b1a", fg="#cecdc3", muted="#878580", border="#343331",
            accent="#3aa99f", on_accent="#100f0f", red="#d14d41", green="#879a39", yellow="#d0a215",
            blue="#4385be", cyan="#3aa99f", magenta="#ce5d97",
            ansi=["#282726", "#d14d41", "#879a39", "#d0a215", "#4385be", "#ce5d97", "#3aa99f", "#cecdc3",
                  "#575653", "#af3029", "#66800b", "#ad8301", "#205ea6", "#a02f6f", "#24837b", "#fffcf0"],
        ),
    }),
}
