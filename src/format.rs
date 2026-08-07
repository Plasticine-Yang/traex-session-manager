//! Display formatting helpers for the session list (spec §5.2).

use std::sync::OnceLock;

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Format an epoch-seconds timestamp as `MM-DD HH:MM` in local time (spec §5.2).
///
/// Timezone conversion is kept out of the dependency set (std-only, spec §9.2):
/// the local UTC offset is discovered once via `date +%z` and applied here.
pub fn format_updated(epoch_secs: i64) -> String {
    format_updated_with_offset(epoch_secs, local_utc_offset_secs())
}

/// Pure core of [`format_updated`], with the offset injected for testability.
pub fn format_updated_with_offset(epoch_secs: i64, offset_secs: i64) -> String {
    let (_, month, day, hour, min) = civil_from_epoch(epoch_secs + offset_secs);
    format!("{month:02}-{day:02} {hour:02}:{min:02}")
}

/// Decompose an epoch-seconds value into `(year, month, day, hour, minute)`
/// using Howard Hinnant's days-from-civil algorithm (proleptic Gregorian).
fn civil_from_epoch(secs: i64) -> (i64, u32, u32, u32, u32) {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let hour = (rem / 3600) as u32;
    let min = ((rem % 3600) / 60) as u32;

    // days since 1970-01-01 -> civil date
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    let year = if month <= 2 { y + 1 } else { y };
    (year, month, day, hour, min)
}

/// Discover the local UTC offset in seconds, cached for the process. Falls back
/// to UTC (0) if `date +%z` is unavailable.
fn local_utc_offset_secs() -> i64 {
    static OFFSET: OnceLock<i64> = OnceLock::new();
    *OFFSET.get_or_init(|| {
        std::process::Command::new("date")
            .arg("+%z")
            .output()
            .ok()
            .filter(|o| o.status.success())
            .and_then(|o| parse_tz_offset(String::from_utf8_lossy(&o.stdout).trim()))
            .unwrap_or(0)
    })
}

/// Parse a `+HHMM` / `-HHMM` timezone offset (as `date +%z` prints) to seconds.
fn parse_tz_offset(s: &str) -> Option<i64> {
    let bytes = s.as_bytes();
    if bytes.len() < 5 {
        return None;
    }
    let sign = match bytes[0] {
        b'+' => 1,
        b'-' => -1,
        _ => return None,
    };
    let hh: i64 = s.get(1..3)?.parse().ok()?;
    let mm: i64 = s.get(3..5)?.parse().ok()?;
    Some(sign * (hh * 3600 + mm * 60))
}

/// Human-readable token count: `442k` / `4.3M` (spec §5.2).
pub fn format_tokens(tokens: i64) -> String {
    if tokens < 0 {
        return "0".to_string();
    }
    let t = tokens as f64;
    if tokens >= 1_000_000 {
        let m = t / 1_000_000.0;
        format!("{m:.1}M")
    } else if tokens >= 1_000 {
        let k = (t / 1_000.0).floor();
        format!("{k}k")
    } else {
        tokens.to_string()
    }
}

/// The list's `session` cell: `COALESCE(NULLIF(title,''), NULLIF(first_user_message,''), '(untitled)')`
/// (spec §5.2), with any embedded newlines/tabs collapsed to spaces.
pub fn session_display(title: &str, first_user_message: &str) -> String {
    let pick = if !title.trim().is_empty() {
        title
    } else if !first_user_message.trim().is_empty() {
        first_user_message
    } else {
        "(untitled)"
    };
    collapse_whitespace(pick)
}

/// Collapse newlines/tabs (and runs of whitespace) into single spaces so a cell
/// stays on one line.
fn collapse_whitespace(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_space = false;
    for ch in s.chars() {
        if ch.is_whitespace() {
            if !prev_space {
                out.push(' ');
                prev_space = true;
            }
        } else {
            out.push(ch);
            prev_space = false;
        }
    }
    out.trim().to_string()
}

/// Truncate `s` to at most `max_width` display columns, appending `…` when it
/// overflows, never splitting a wide (CJK) character (spec §5.2/§5.3).
pub fn truncate_display(s: &str, max_width: usize) -> String {
    if s.width() <= max_width {
        return s.to_string();
    }
    if max_width == 0 {
        return String::new();
    }
    // Reserve one column for the ellipsis.
    let budget = max_width.saturating_sub(1);
    let mut out = String::new();
    let mut used = 0usize;
    for ch in s.chars() {
        let w = ch.width().unwrap_or(0);
        if used + w > budget {
            break;
        }
        out.push(ch);
        used += w;
    }
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_small() {
        assert_eq!(format_tokens(0), "0");
        assert_eq!(format_tokens(999), "999");
    }

    #[test]
    fn tokens_thousands() {
        assert_eq!(format_tokens(1_000), "1k");
        assert_eq!(format_tokens(442_123), "442k");
        assert_eq!(format_tokens(999_999), "999k");
    }

    #[test]
    fn tokens_millions() {
        assert_eq!(format_tokens(1_000_000), "1.0M");
        assert_eq!(format_tokens(4_300_000), "4.3M");
    }

    #[test]
    fn session_prefers_title() {
        assert_eq!(session_display("My title", "first msg"), "My title");
    }

    #[test]
    fn session_falls_back_to_first_message() {
        assert_eq!(session_display("", "first msg"), "first msg");
        assert_eq!(session_display("   ", "first msg"), "first msg");
    }

    #[test]
    fn session_untitled_when_both_empty() {
        assert_eq!(session_display("", ""), "(untitled)");
    }

    #[test]
    fn session_collapses_newlines() {
        assert_eq!(session_display("line one\nline two", ""), "line one line two");
        assert_eq!(session_display("a\t\tb", ""), "a b");
    }

    #[test]
    fn truncate_leaves_short_strings() {
        assert_eq!(truncate_display("hello", 10), "hello");
        assert_eq!(truncate_display("hello", 5), "hello");
    }

    #[test]
    fn truncate_appends_ellipsis() {
        assert_eq!(truncate_display("hello world", 5), "hell…");
    }

    #[test]
    fn truncate_respects_cjk_width() {
        // Each CJK char is 2 columns wide. Budget 5 => ellipsis(1) + 4 cols => 2 chars.
        assert_eq!(truncate_display("你好世界", 5), "你好…");
        // Never split a wide char: budget 4 => ellipsis(1) + 3 cols fits 1 char.
        assert_eq!(truncate_display("你好世界", 4), "你…");
    }

    #[test]
    fn updated_formats_known_epoch_utc() {
        // 2021-01-02 03:04:00 UTC
        let secs = 1_609_556_640;
        assert_eq!(format_updated_with_offset(secs, 0), "01-02 03:04");
    }

    #[test]
    fn updated_applies_offset() {
        // Same instant, +08:00 -> 11:04 local.
        let secs = 1_609_556_640;
        assert_eq!(format_updated_with_offset(secs, 8 * 3600), "01-02 11:04");
    }

    #[test]
    fn parse_tz_offset_cases() {
        assert_eq!(parse_tz_offset("+0800"), Some(8 * 3600));
        assert_eq!(parse_tz_offset("-0530"), Some(-(5 * 3600 + 30 * 60)));
        assert_eq!(parse_tz_offset("+0000"), Some(0));
        assert_eq!(parse_tz_offset("garbage"), None);
        assert_eq!(parse_tz_offset(""), None);
    }
}
