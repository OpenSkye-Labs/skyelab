//! How full a thread's context window is: the window each model has, and how a reading reads.

/// From this percent on, the meter warns and hints `/compact`.
pub const NEAR_FULL: u32 = 80;
/// From this percent on, the meter shows the danger color.
pub const FULL: u32 = 90;

/// The context window of `model` (a `--model` alias or a full id) in tokens, when SkyeLab knows it. Claude 5 models
/// (`claude-haiku-5-5`, and a bare alias, whose callers first resolve it through the init line's id) take 1M, as real
/// Claude reports them; older Claude models
/// 200k, or 1M with the `[1m]` suffix. Anything else is `None`: Codex reports its own window, and a reading the agent
/// sends always wins over this table.
pub fn window(model: &str) -> Option<u64> {
    let (base, wide) = match model.strip_suffix("[1m]") {
        Some(base) => (base, true),
        None => (model, false),
    };
    let alias = matches!(base, "opus" | "sonnet" | "haiku" | "fable");
    let claude = alias || base.starts_with("claude-");
    // `claude-<family>-<major>-…`; the old naming (`claude-3-5-sonnet`) starts with the version.
    let mut parts = base.split('-').skip(1);
    let family = parts
        .next()
        .is_some_and(|f| f.chars().all(|c| c.is_ascii_alphabetic()));
    let major: Option<u32> = parts.next().and_then(|m| m.parse().ok());
    let five = alias || (family && major.is_some_and(|m| m >= 5));
    claude.then_some(if wide || five { 1_000_000 } else { 200_000 })
}

/// How much of `window` `tokens` fill, rounded and capped at 100. `None` without a window.
pub fn percent(tokens: u64, window: u64) -> Option<u32> {
    if window == 0 {
        return None;
    }
    let p = (u128::from(tokens) * 100 + u128::from(window) / 2) / u128::from(window);
    Some(u32::try_from(p.min(100)).unwrap_or(100))
}

pub fn near_full(percent: u32) -> bool {
    percent >= NEAR_FULL
}

/// A token count the way T3 writes it: 950, 9.5k, 84k, 1.3m.
pub fn tokens_text(n: u64) -> String {
    let one_decimal = |tenths: u64, unit: &str| match tenths % 10 {
        0 => format!("{}{unit}", tenths / 10),
        d => format!("{}.{d}{unit}", tenths / 10),
    };
    match n {
        0..1_000 => n.to_string(),
        1_000..10_000 => one_decimal((n + 50) / 100, "k"),
        10_000..999_500 => format!("{}k", (n + 500) / 1_000),
        _ => one_decimal(n.saturating_add(50_000) / 100_000, "m"),
    }
}
