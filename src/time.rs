//! Timestamps: RFC 3339, UTC.

use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

/// Now, to the millisecond, for entries.
pub fn now_ms() -> String {
    at_ms(OffsetDateTime::now_utc())
}

/// `secs` seconds ago, as `now_ms` writes it (so they compare as text).
pub fn ms_ago(secs: i64) -> String {
    at_ms(OffsetDateTime::now_utc() - time::Duration::seconds(secs))
}

/// A Unix time in milliseconds, as `now_ms` writes one.
pub fn from_unix_ms(ms: i64) -> Option<String> {
    OffsetDateTime::from_unix_timestamp_nanos(ms as i128 * 1_000_000).ok().map(at_ms)
}

fn at_ms(t: OffsetDateTime) -> String {
    let ms = t.millisecond();
    let t = t.replace_nanosecond(0).expect("zero is valid");
    let s = t.format(&Rfc3339).expect("format time");
    // 2026-11-17T10:00:00Z → 2026-11-17T10:00:00.123Z
    format!("{}.{ms:03}Z", s.trim_end_matches('Z'))
}

/// Now, to the second, for logs.
pub fn now() -> String {
    OffsetDateTime::now_utc().replace_nanosecond(0).expect("zero is valid").format(&Rfc3339).expect("format time")
}

pub fn parse(s: &str) -> Option<OffsetDateTime> {
    OffsetDateTime::parse(s, &Rfc3339).ok()
}

/// Seconds from `since` to now.
#[allow(dead_code)]
pub fn secs_since(since: &str) -> Option<i64> {
    Some((OffsetDateTime::now_utc() - parse(since)?).whole_seconds())
}

#[cfg(test)]
mod tests {
    #[test]
    fn ms_times_parse() {
        let t = super::now_ms();
        assert!(t.ends_with('Z') && t.contains('.'), "{t}");
        assert!(super::parse(&t).is_some(), "{t}");
    }
}
