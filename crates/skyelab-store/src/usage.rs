//! Each account's newest usage readings, kept by the `usage_reading_insert` trigger as agent events arrive.

use anyhow::Result;

use crate::Store;

/// One stored usage reading: an agent `RateLimit` event's payload.
#[derive(Debug, Clone, PartialEq)]
pub struct UsageReading {
    /// `""` for the newest reading of any kind, else the window it measures (`five_hour`, `seven_day`, …).
    pub window: String,
    pub payload_json: String,
    pub created_at: i64,
}

impl Store {
    /// The newest reading per window from threads created under `account_id` (a primary-key lookup).
    pub fn usage_readings(&self, account_id: &str) -> Result<Vec<UsageReading>> {
        let mut stmt = self.conn.prepare(
            "SELECT window, payload_json, created_at FROM usage_readings WHERE account_id = ?1",
        )?;
        let rows = stmt.query_map([account_id], |r| {
            Ok(UsageReading {
                window: r.get(0)?,
                payload_json: r.get(1)?,
                created_at: r.get(2)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Stores readings the account's CLI gave when asked (the usage poll), like the `usage_reading_insert` trigger
    /// keeps a thread's: a reading with a window and a percentage becomes that window's newest. Only a summary (no
    /// window) replaces the `""` row, so a poll's plain readings never hide or lift a "Limited" a turn reported.
    pub fn record_usage(&self, account_id: &str, payloads: &[String], at: i64) -> Result<()> {
        for p in payloads {
            self.conn.execute(
                "INSERT OR REPLACE INTO usage_readings (account_id, window, payload_json, created_at)
                 SELECT ?1, CASE WHEN json_type(?2, '$.RateLimit.window') = 'text'
                                 THEN json_extract(?2, '$.RateLimit.window') ELSE '' END, ?2, ?3
                 WHERE json_valid(?2) AND json_type(?2, '$.RateLimit') = 'object'
                   AND (json_type(?2, '$.RateLimit.window') IS NULL
                        OR (json_type(?2, '$.RateLimit.window') = 'text'
                            AND json_type(?2, '$.RateLimit.used') IN ('integer', 'real')))",
                rusqlite::params![account_id, p, at],
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod record_tests {
    use crate::Store;

    #[test]
    fn a_poll_keeps_windows_and_only_a_summary_touches_limited() {
        let s = Store::open_in_memory().expect("store");
        let limited = r#"{"RateLimit":{"limited":true,"resets_at":5}}"#.to_owned();
        s.record_usage("a", std::slice::from_ref(&limited), 1)
            .expect("summary");
        let five =
            r#"{"RateLimit":{"limited":false,"resets_at":9,"window":"five_hour","used":0.4}}"#;
        let status_only = r#"{"RateLimit":{"limited":false,"window":"five_hour"}}"#;
        s.record_usage(
            "a",
            &[five.into(), status_only.into(), "not json".into()],
            2,
        )
        .expect("readings");
        let mut got = s.usage_readings("a").expect("read");
        got.sort_by(|x, y| x.window.cmp(&y.window));
        let got: Vec<_> = got
            .iter()
            .map(|r| (r.window.as_str(), r.payload_json.as_str(), r.created_at))
            .collect();
        assert_eq!(got, vec![("", limited.as_str(), 1), ("five_hour", five, 2)]);
        assert!(
            s.usage_readings("b").expect("read").is_empty(),
            "per account"
        );
    }
}

/// How many days the Usage page reads: today and the 29 before it.
pub const DAYS: i64 = 30;
const DAY_MS: i64 = 86_400_000;

/// Where the Usage page's window starts: `DAYS - 1` days before `today_start` (local midnight, ms).
/// shortcut: days are 24 h from that midnight, so across a DST change a turn near midnight can land a day off;
/// bucket by local dates in Rust if that ever matters.
pub fn since(today_start: i64) -> i64 {
    today_start - (DAYS - 1) * DAY_MS
}

/// One account's spend in one project on one day of the window.
#[derive(Debug, Clone, PartialEq)]
pub struct UsageDay {
    pub account_id: String,
    pub project_id: i64,
    /// 0 is the window's first day, `DAYS - 1` today.
    pub day: i64,
    pub cost_usd: f64,
    pub input: u64,
    pub output: u64,
    pub turns: u64,
}

/// Summed turns: what they cost (as the agent reported it), their tokens and how many there were.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Spend {
    pub cost_usd: f64,
    pub input: u64,
    pub output: u64,
    pub turns: u64,
}

impl Spend {
    pub fn tokens(&self) -> u64 {
        self.input + self.output
    }

    fn add(&mut self, r: &UsageDay) {
        self.cost_usd += r.cost_usd;
        self.input += r.input;
        self.output += r.output;
        self.turns += r.turns;
    }
}

/// The Usage page's ranges.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Range {
    Today,
    #[default]
    Week,
    Month,
}

impl Range {
    pub const ALL: [Range; 3] = [Range::Today, Range::Week, Range::Month];

    pub fn days(self) -> i64 {
        match self {
            Range::Today => 1,
            Range::Week => 7,
            Range::Month => DAYS,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Range::Today => "Today",
            Range::Week => "7 days",
            Range::Month => "30 days",
        }
    }

    fn holds(self, day: i64) -> bool {
        (DAYS - self.days()..DAYS).contains(&day)
    }
}

/// The rows in `range` that `keep` accepts, summed.
pub fn spend(rows: &[UsageDay], range: Range, keep: impl Fn(&UsageDay) -> bool) -> Spend {
    let mut s = Spend::default();
    for r in rows.iter().filter(|r| range.holds(r.day) && keep(r)) {
        s.add(r);
    }
    s
}

/// The rows `keep` accepts, summed per day of the window (`DAYS` entries, today last).
pub fn daily(rows: &[UsageDay], keep: impl Fn(&UsageDay) -> bool) -> Vec<Spend> {
    let mut out = vec![Spend::default(); DAYS as usize];
    for r in rows.iter().filter(|r| keep(r)) {
        if let Some(d) = usize::try_from(r.day).ok().and_then(|d| out.get_mut(d)) {
            d.add(r);
        }
    }
    out
}

impl Store {
    /// Every turn since `since` (ms), summed per account, project and day: at most accounts × projects × days rows,
    /// read through the time index.
    pub fn usage_days(&self, since: i64) -> Result<Vec<UsageDay>> {
        let mut stmt = self.conn.prepare(
            "SELECT account_id, project_id, (created_at - ?1) / ?2 AS day, SUM(cost_usd), SUM(input_tokens),
                    SUM(output_tokens), COUNT(*)
             FROM turn_usage WHERE created_at >= ?1 GROUP BY account_id, project_id, day",
        )?;
        let rows = stmt.query_map([since, DAY_MS], |r| {
            Ok(UsageDay {
                account_id: r.get(0)?,
                project_id: r.get(1)?,
                day: r.get(2)?,
                cost_usd: r.get(3)?,
                input: r.get::<_, i64>(4)?.max(0) as u64,
                output: r.get::<_, i64>(5)?.max(0) as u64,
                turns: r.get::<_, i64>(6)?.max(0) as u64,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
}
