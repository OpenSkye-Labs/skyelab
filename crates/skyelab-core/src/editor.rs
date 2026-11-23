//! Which editor "Open in editor" runs. The caller spawns it without a shell, so the command is only split on
//! whitespace (no quoting): an editor whose path has spaces must be on PATH or symlinked.

/// The editor's argv: settings.toml's `editor`, else `$VISUAL`, else `$EDITOR`, else `code` or `zed` when `on_path`
/// finds them. Blank values count as unset. `None` when there is nothing to run.
pub fn editor_argv(
    setting: Option<&str>,
    var: impl Fn(&str) -> Option<String>,
    on_path: impl Fn(&str) -> bool,
) -> Option<Vec<String>> {
    let split = |s: &str| -> Option<Vec<String>> {
        let argv: Vec<String> = s.split_whitespace().map(str::to_owned).collect();
        (!argv.is_empty()).then_some(argv)
    };
    setting
        .and_then(split)
        .or_else(|| var("VISUAL").as_deref().and_then(split))
        .or_else(|| var("EDITOR").as_deref().and_then(split))
        .or_else(|| {
            ["code", "zed"]
                .into_iter()
                .find(|n| on_path(n))
                .map(|n| vec![n.to_owned()])
        })
}
