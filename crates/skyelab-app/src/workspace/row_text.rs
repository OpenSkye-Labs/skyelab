//! A pull request's short label for a thread row (the reply preview is `skyelab_agent::thread_list::preview_text`).

/// "#24" for a pull request URL ending in its number, else "PR".
pub(in crate::workspace) fn pr_label(url: &str) -> String {
    match url.trim_end_matches('/').rsplit_once('/') {
        Some((_, n)) if !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) => format!("#{n}"),
        _ => "PR".into(),
    }
}
