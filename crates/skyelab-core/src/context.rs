//! How full a thread's context window is: the window each model has, and how a reading reads.

/// From this percent on, the meter warns and hints `/compact`.
pub const NEAR_FULL: u32 = 80;

/// The context window of `model` (a `--model` alias or a full id) in tokens, when SkyeLab knows it. Claude models
/// take 200k, or 1M with the `[1m]` suffix. Anything else is `None`: Codex reports its own window, and a reading the
/// agent sends always wins over this table.
pub fn window(model: &str) -> Option<u64> {
    let (base, wide) = match model.strip_suffix("[1m]") {
        Some(base) => (base, true),
        None => (model, false),
    };
    let claude =
        matches!(base, "opus" | "sonnet" | "haiku" | "fable") || base.starts_with("claude-");
    claude.then_some(if wide { 1_000_000 } else { 200_000 })
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
    // One decimal, dropped when it is 0: `tenths` is the count in tenths of the unit, rounded half up.
    let tenths = |tenths: u64, unit: &str| match tenths % 10 {
        0 => format!("{}{unit}", tenths / 10),
        d => format!("{}.{d}{unit}", tenths / 10),
    };
    match n {
        0..1_000 => n.to_string(),
        1_000..10_000 => tenths((n + 50) / 100, "k"),
        10_000..999_500 => format!("{}k", (n + 500) / 1_000),
        _ => tenths((n + 50_000) / 100_000, "m"),
    }
}
