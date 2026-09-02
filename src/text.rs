//! Shared text helpers for draw code. Every right-edge cut in the UI goes
//! through [`truncate`] so nothing ever hard-clips mid-word without a visible
//! ellipsis.

use time::format_description::well_known::Rfc3339;
use time::{OffsetDateTime, UtcOffset};

/// Fit `s` into `width` cells: unchanged when it fits, otherwise cut one
/// short and finished with `…`. Char-based (the UI is single-width text).
pub fn truncate(s: &str, width: usize) -> String {
    if s.chars().count() <= width {
        s.to_string()
    } else if width > 1 {
        let cut: String = s.chars().take(width - 1).collect();
        format!("{cut}…")
    } else {
        // A 0/1-cell window has no room for text + ellipsis; keep what fits.
        s.chars().take(width).collect()
    }
}

/// Last word of the leading capitalized-name run ("Patrick Mahomes pass to
/// T. Kelce" -> "Mahomes"), keeping generational suffixes ("Jazz Chisholm
/// Jr. walks" -> "Chisholm Jr."). Play text leads with the player's name in
/// every feed we map; when it doesn't, the first word is the honest fallback.
pub fn leading_surname(text: &str) -> String {
    let words: Vec<&str> = text.split_whitespace().collect();
    let is_name = |w: &str| w.chars().next().is_some_and(char::is_uppercase);
    let run = words.iter().take_while(|w| is_name(w)).count();
    if run == 0 {
        return words.first().copied().unwrap_or_default().to_string();
    }
    let last = words[run - 1];
    if run >= 2 && matches!(last, "Jr." | "Sr." | "II" | "III" | "IV") {
        format!("{} {last}", words[run - 2])
    } else {
        last.to_string()
    }
}

/// ESPN's `date` fields are RFC 3339 with a `Z` and no seconds
/// (`2026-09-01T01:38Z`) on the scoreboard, and full `…:00.000+00:00` on some
/// summaries. `Rfc3339` needs seconds, so the short form is padded first.
pub fn local_time(iso: &str, offset: UtcOffset) -> Option<OffsetDateTime> {
    let iso = iso.trim();
    if iso.is_empty() {
        return None;
    }
    // "YYYY-MM-DDTHH:MMZ" -> "YYYY-MM-DDTHH:MM:00Z"
    let padded;
    // `is_char_boundary(16)` guards the slice below: every byte index here is
    // a byte index, not a char index, and a 17-byte string is not necessarily
    // 17 characters.
    let s = if iso.len() == 17
        && iso.ends_with('Z')
        && iso.as_bytes()[13] == b':'
        && iso.is_char_boundary(16)
    {
        padded = format!("{}:00Z", &iso[..16]);
        padded.as_str()
    } else {
        iso
    };
    OffsetDateTime::parse(s, &Rfc3339).ok().map(|t| t.to_offset(offset))
}

/// Start-time label relative to `now` (same offset as `start`):
/// today → `9:38 PM`; within the next six days → `THU 8:20 PM`; otherwise
/// `SEP 13 1:00 PM`. Six days keeps a bare weekday unambiguous.
pub fn fmt_start(start: OffsetDateTime, now: OffsetDateTime) -> String {
    let clock = fmt_hm12(start);
    let days = (start.date() - now.date()).whole_days();
    if days == 0 {
        clock
    } else if (1..=6).contains(&days) {
        format!("{} {clock}", &format!("{:?}", start.weekday()).to_uppercase()[..3])
    } else {
        format!(
            "{} {} {clock}",
            &format!("{:?}", start.month()).to_uppercase()[..3],
            start.day()
        )
    }
}

/// Header wall clock: `9:30:01 PM`.
pub fn fmt_clock12(t: OffsetDateTime) -> String {
    let (h, ampm) = h12(t.hour());
    format!("{h}:{:02}:{:02} {ampm}", t.minute(), t.second())
}

/// Clock without seconds: `9:41 PM`. The seconds in `fmt_clock12` are for a
/// header that ticks; a timestamp that doesn't tick shouldn't carry them.
pub fn fmt_hm12(t: OffsetDateTime) -> String {
    let (h, ampm) = h12(t.hour());
    format!("{h}:{:02} {ampm}", t.minute())
}

fn h12(hour: u8) -> (u8, &'static str) {
    match hour {
        0 => (12, "AM"),
        h if h < 12 => (h, "AM"),
        12 => (12, "PM"),
        h => (h - 12, "PM"),
    }
}

/// The local UTC offset, read ONCE on the main thread before any other thread
/// exists: `time` refuses to read the TZ database from a multi-threaded
/// process on Unix (it returns Err), and the old per-call
/// `now_local().unwrap_or_else(now_utc)` silently printed UTC in that case.
pub fn startup_offset() -> UtcOffset {
    match UtcOffset::current_local_offset() {
        Ok(off) => off,
        Err(e) => {
            eprintln!("gameday: local UTC offset unavailable ({e}); times will show in UTC — set TZ to fix");
            UtcOffset::UTC
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fits_untouched() {
        assert_eq!(truncate("SHORT", 10), "SHORT");
        assert_eq!(truncate("EXACT", 5), "EXACT");
        assert_eq!(truncate("", 0), "");
    }

    #[test]
    fn overflow_gets_an_ellipsis_inside_the_width() {
        assert_eq!(truncate("TOUCHDOWN", 6), "TOUCH…");
        assert_eq!(truncate("TOUCHDOWN", 6).chars().count(), 6);
        assert_eq!(truncate("AB", 1), "A", "width 1 keeps one char, no ellipsis");
        assert_eq!(truncate("AB", 0), "");
    }

    #[test]
    fn counts_chars_not_bytes() {
        // "90'+3' Bukayo Saka" style text with multibyte chars must not split
        // a codepoint or overshoot the cell width.
        assert_eq!(truncate("héllo wörld", 7), "héllo …");
        assert_eq!(truncate("héllo wörld", 7).chars().count(), 7);
        assert_eq!(truncate("◆◇◆◇", 4), "◆◇◆◇");
        assert_eq!(truncate("◆◇◆◇", 3), "◆◇…");
    }
}

#[cfg(test)]
mod surname_tests {
    use super::*;

    #[test]
    fn leading_surname_takes_the_last_word_of_the_leading_name_run() {
        assert_eq!(leading_surname("Patrick Mahomes pass to T. Kelce"), "Mahomes");
        assert_eq!(leading_surname("Mahomes pass to Kelce for 3 yards"), "Mahomes");
        assert_eq!(leading_surname("Nikola Jokic makes layup (28 PTS)"), "Jokic");
        assert_eq!(leading_surname("Leon Draisaitl snap shot GOAL (32)"), "Draisaitl");
    }

    #[test]
    fn leading_surname_keeps_generational_suffixes() {
        assert_eq!(leading_surname("Jazz Chisholm Jr. walks"), "Chisholm Jr.");
        assert_eq!(leading_surname("Vladimir Guerrero Jr. single to right"), "Guerrero Jr.");
    }

    #[test]
    fn leading_surname_falls_back_to_the_first_word() {
        assert_eq!(leading_surname("TOUCHDOWN"), "TOUCHDOWN");
        assert_eq!(leading_surname(""), "");
    }
}

#[cfg(test)]
mod time_tests {
    use super::*;
    use time::macros::datetime;
    use time::UtcOffset;

    #[test]
    fn local_time_parses_espn_iso_and_applies_offset() {
        let la = UtcOffset::from_hms(-7, 0, 0).unwrap();
        let t = local_time("2026-09-01T01:38Z", la).unwrap();
        // 01:38 UTC on Sep 1 is 6:38 PM on Aug 31 in Los Angeles.
        assert_eq!(t, datetime!(2026-08-31 18:38 -7));
        let london = UtcOffset::from_hms(1, 0, 0).unwrap();
        assert_eq!(local_time("2026-09-01T01:38Z", london).unwrap(), datetime!(2026-09-01 02:38 +1));
        // ESPN also sends full offsets and fractional seconds on some feeds.
        assert!(local_time("2026-09-13T17:00:00Z", la).is_some());
        assert!(local_time("2026-09-13T17:00:00.000+00:00", la).is_some());
        assert_eq!(local_time("not a date", la), None);
        assert_eq!(local_time("", la), None);
        // 17 BYTES, not 17 chars: the short-form branch slices at byte 16, so
        // a multi-byte char anywhere near the tail must decline, not panic.
        for s in ["2026-09-01T01:3é", "2é6-09-01T01:38Z"] {
            assert_eq!(s.len(), 17, "{s:?} must be 17 bytes to reach the branch");
            assert_eq!(local_time(s, la), None, "{s:?}");
        }
    }

    #[test]
    fn fmt_start_is_clock_today_and_day_clock_within_the_week() {
        let now = datetime!(2026-08-31 21:30 -4);
        assert_eq!(fmt_start(datetime!(2026-08-31 21:38 -4), now), "9:38 PM");
        assert_eq!(fmt_start(datetime!(2026-09-03 20:20 -4), now), "THU 8:20 PM");
        assert_eq!(fmt_start(datetime!(2026-09-06 13:00 -4), now), "SUN 1:00 PM");
        // A week or more out: the date, never a bare weekday that could mean two days.
        assert_eq!(fmt_start(datetime!(2026-09-13 13:00 -4), now), "SEP 13 1:00 PM");
        // Midnight and noon edges.
        assert_eq!(fmt_start(datetime!(2026-08-31 00:05 -4), now), "12:05 AM");
        assert_eq!(fmt_start(datetime!(2026-08-31 12:00 -4), now), "12:00 PM");
    }

    #[test]
    fn fmt_clock12_has_seconds_and_meridiem() {
        assert_eq!(fmt_clock12(datetime!(2026-08-31 21:30:01 -4)), "9:30:01 PM");
        assert_eq!(fmt_clock12(datetime!(2026-08-31 00:00:00 -4)), "12:00:00 AM");
    }

    #[test]
    fn fmt_hm12_drops_the_seconds() {
        assert_eq!(fmt_hm12(datetime!(2026-08-31 21:41:59 -4)), "9:41 PM");
        assert_eq!(fmt_hm12(datetime!(2026-08-31 00:05:00 -4)), "12:05 AM");
        assert_eq!(fmt_hm12(datetime!(2026-08-31 12:00:00 -4)), "12:00 PM");
    }
}
