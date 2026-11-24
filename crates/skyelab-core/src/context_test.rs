use super::context::*;

#[test]
fn claude_5_models_have_a_1m_window_and_older_ones_200k() {
    // Real Claude Code 2.1.331 reports `contextWindow` 1000000 for claude-haiku-5-5 and claude-opus-5-5.
    for m in [
        "opus",
        "sonnet",
        "haiku",
        "fable",
        "claude-opus-5-5",
        "claude-haiku-5-5",
        "claude-fable-5-2",
    ] {
        assert_eq!(window(m), Some(1_000_000), "{m}");
    }
    for m in [
        "claude-sonnet-4-5-20250929",
        "claude-sonnet-4-6",
        "claude-3-5-sonnet-20241022",
    ] {
        assert_eq!(window(m), Some(200_000), "{m}");
    }
}

#[test]
fn a_1m_suffix_means_a_million() {
    assert_eq!(window("opus[1m]"), Some(1_000_000));
    assert_eq!(window("claude-sonnet-5-5[1m]"), Some(1_000_000));
}

#[test]
fn unknown_models_have_no_window() {
    for m in ["", "gpt-5.5", "o3", "mystery[1m]", "claudex"] {
        assert_eq!(window(m), None, "{m}");
    }
}

#[test]
fn percent_rounds_and_clamps() {
    assert_eq!(percent(0, 200_000), Some(0));
    assert_eq!(percent(84_000, 200_000), Some(42));
    assert_eq!(percent(84_999, 200_000), Some(42));
    assert_eq!(percent(85_000, 200_000), Some(43));
    assert_eq!(percent(250_000, 200_000), Some(100));
    assert_eq!(percent(5, 0), None, "no window, no percent");
}

#[test]
fn near_full_starts_at_80_percent() {
    assert!(!near_full(79));
    assert!(near_full(80));
    assert!(near_full(100));
}

#[test]
fn token_counts_read_like_t3() {
    assert_eq!(tokens_text(950), "950");
    assert_eq!(tokens_text(9_500), "9.5k");
    assert_eq!(tokens_text(9_000), "9k");
    assert_eq!(tokens_text(84_400), "84k");
    assert_eq!(tokens_text(200_000), "200k");
    assert_eq!(tokens_text(1_000_000), "1m");
    assert_eq!(tokens_text(1_250_000), "1.3m");
    assert_eq!(tokens_text(u64::MAX), "18446744073709.5m");
}
