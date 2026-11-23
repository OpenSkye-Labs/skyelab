//! The flat thread list's text, shared by the desktop's sidebar and the phone: the date sections under Pinned and
//! Working, and a reply cut to one plain preview line.

/// The date sections under Pinned and Working, T3's and Synara's: indexes [`day_bucket`] returns.
pub const DAYS: [&str; 4] = ["Today", "Yesterday", "Previous 7 days", "Older"];

/// Which of [`DAYS`] a thread last changed in, given where today, yesterday and the 7-day window start (local
/// midnights, ms since the epoch; see [`day_starts`]).
pub fn day_bucket(starts: [i64; 3], updated_ms: i64) -> usize {
    starts
        .iter()
        .position(|&start| updated_ms >= start)
        .unwrap_or(3)
}

/// Local midnight today, yesterday and six days before today, in ms since the epoch: each its own local midnight, so
/// a daylight-saving change doesn't shift a section by an hour.
pub fn day_starts() -> [i64; 3] {
    use chrono::{Days, Local, TimeZone as _};
    let today = Local::now().date_naive();
    [0, 1, 6].map(|back| {
        today
            .checked_sub_days(Days::new(back))
            .and_then(|d| d.and_hms_opt(0, 0, 0))
            .and_then(|m| Local.from_local_datetime(&m).earliest())
            .map_or(i64::MIN, |m| m.timestamp_millis())
    })
}

/// A reply as one plain line for the sidebar: markdown marks dropped, whitespace collapsed.
pub fn preview_text(markdown: &str) -> String {
    let mut out = String::with_capacity(markdown.len());
    let mut in_fence = false;
    for line in markdown.lines() {
        let line = line.trim();
        // Fenced code (a diagram's source, a snippet) and rules carry no words; headings, quotes and bullets keep theirs.
        if line.starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence
            || line
                .chars()
                .all(|c| matches!(c, '-' | '*' | '_' | '|' | ':' | ' '))
        {
            continue;
        }
        // A heading's marks ("## "), a quote's ("> "); "#24 merged" keeps its number.
        let line = line.trim_start_matches(['>', ' ']);
        let line = match line.trim_start_matches('#') {
            rest if rest.starts_with(' ') => rest.trim_start(),
            _ => line,
        };
        let line = line
            .strip_prefix("- ")
            .or_else(|| line.strip_prefix("* "))
            .unwrap_or(line);
        if !out.is_empty() {
            out.push(' ');
        }
        out.extend(
            link_text(line)
                .chars()
                .filter(|c| !matches!(c, '*' | '`' | '|')),
        );
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `[text](url)` and `![alt](url)` as just their text; a `[` that isn't a link stays.
fn link_text(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(open) = rest.find('[') {
        let after = &rest[open + 1..];
        let link = after.find("](").and_then(|close| {
            let url = &after[close + 2..];
            url.find(')').map(|end| (&after[..close], &url[end + 1..]))
        });
        match link {
            Some((text, tail)) => {
                out.push_str(rest[..open].strip_suffix('!').unwrap_or(&rest[..open]));
                out.push_str(text);
                rest = tail;
            }
            None => {
                out.push_str(&rest[..=open]);
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}
