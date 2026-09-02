//! The tier budget: spec §4's sizes ladder as one pure function. Height and
//! counts in, `TierPlan` out — nothing here reads a `Game` or renders a cell.

/// How many rows each piece of the board gets at a given size. Pure: height
/// and counts in, plan out — the sizes ladder (spec §4) lives here and only here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TierPlan {
    pub hero_rows: u16, // 0 = no hero fits (only < 12 total rows)
    pub hero_digits_full: bool, // 8-row PixelSize::Full vs 4x3 sextant
    pub tier1: usize,   // promoted 3-row rows (0..=3)
    pub tier2: usize,   // single-line live rows
    pub finals: usize,  // dim single lines (0 = collapse to count line)
    pub later: usize,
    pub scores_lane: bool, // the off-screen SCORES lane (only when truncated)
}

/// Row cost of one section rule/label line.
pub const RULE_ROWS: u16 = 1;

/// The sizes ladder (spec §4) as a top-down subtraction: hero first by the
/// width/height gates, then a flat 4-rule reservation (MY GAMES, hero,
/// FINAL, LATER), then rows run out bottom-up — later first, then finals,
/// then tier2 truncates (lane on), tier1 demotes, hero shrinks last.
pub fn plan(width: u16, height: u16, live: usize, finals: usize, later: usize, my_games: usize) -> TierPlan {
    // spec §4 last row: `need 40×12, have W×H` — no hero fits below the floor.
    let (hero_rows, hero_digits_full): (u16, bool) = if width < 40 || height < 12 {
        (0, false)
    } else if width < 60 {
        // spec §4 "< 60 cols": 2-line compact hero, no digits.
        (2, false)
    } else if width >= 100 && height >= 32 {
        // spec §4 "≥120×40" / "100-119 cols": 8-row Full digits, 10 rows.
        (10, true)
    } else {
        // spec §4 "80×24" / "60-79 cols": sextant digits, 6 rows total.
        (6, false)
    };

    // spec §4 tier 1 column: up to 3 at the reference frame, up to 2 at
    // 100-119 cols, 0 below 100 cols (all tier 2).
    let tier1_cap = if width >= 120 {
        3
    } else if width >= 100 {
        2
    } else {
        0
    };

    // spec §1 MY GAMES band: 1 line per pinned/favorite row, off the same
    // budget as everything below the hero — plus a flat 4-rule reservation
    // (MY GAMES, hero, FINAL, LATER) so the invariant holds without needing
    // to know which sections end up empty.
    let reserved = hero_rows
        .saturating_add(my_games as u16)
        .saturating_add(4 * RULE_ROWS);
    let mut remaining = height.saturating_sub(reserved);

    // spec §4 tier 1: scaled by what's left, capped by width class.
    let tier1 = ((remaining / 4) as usize).min(tier1_cap).min(live);
    remaining = remaining.saturating_sub(3 * tier1 as u16);

    // spec §1 tier 2: as many single lines as fit.
    let live_left = live.saturating_sub(tier1);
    let tier2 = live_left.min(remaining as usize);
    remaining = remaining.saturating_sub(tier2 as u16);

    // spec §1 tier 3 FINAL: at most 2 dim lines, whatever's left.
    let finals_target = finals.min(2);
    let finals_rows = finals_target.min(remaining as usize);
    remaining = remaining.saturating_sub(finals_rows as u16);

    // spec §1 tier 3 LATER: shrinks first when rows run out (§4).
    let later_rows = later.min(remaining as usize);

    // spec §1 Ticker: the lane appears exactly when something got cut.
    let scores_lane =
        tier2 < live_left || finals_rows < finals_target || later_rows < later;

    TierPlan {
        hero_rows,
        hero_digits_full,
        tier1,
        tier2,
        finals: finals_rows,
        later: later_rows,
        scores_lane,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sizes_ladder_matches_the_spec_table() {
        // 120x40: full hero, up to 3 promoted, no lane.
        let p = plan(120, 38, 8, 2, 4, 0);
        assert_eq!(p.hero_rows, 10);
        assert!(p.hero_digits_full);
        assert_eq!(p.tier1, 3);
        assert!(!p.scores_lane);
        // 80x24: sextant hero, all tier 2, lane when truncated.
        let p = plan(80, 22, 8, 2, 4, 0);
        assert!(p.hero_rows <= 6 && p.hero_rows >= 4);
        assert!(!p.hero_digits_full);
        assert_eq!(p.tier1, 0);
        assert!(p.scores_lane, "8 live don't fit 22 rows — lane on");
        // <60 cols: 2-row compact hero, still one list.
        let p = plan(55, 38, 3, 1, 2, 0);
        assert_eq!(p.hero_rows, 2);
        // Never negative / overlapping: sum of allocated rows <= height.
        for (w, h) in [(40u16, 12u16), (60, 20), (100, 30), (200, 60)] {
            let p = plan(w, h, 12, 5, 8, 2);
            let used = p.hero_rows
                + 3 * p.tier1 as u16
                + p.tier2 as u16
                + p.finals as u16
                + p.later as u16
                + 4 * RULE_ROWS
                + if p.scores_lane { 1 } else { 0 };
            assert!(used <= h, "{w}x{h}: used {used}");
        }
    }

    #[test]
    fn rows_run_out_bottom_up() {
        // Shrinking height drops LATER to a count line before touching live rows.
        let tall = plan(120, 38, 6, 3, 6, 0);
        let short = plan(120, 26, 6, 3, 6, 0);
        assert!(short.later < tall.later, "later shrinks first");
        assert!(
            short.tier2 + short.tier1 >= 6usize.min(tall.tier1 + tall.tier2),
            "live rows survive"
        );
        // Down further: finals go, then the lane appears.
        let tiny = plan(120, 16, 6, 3, 6, 0);
        assert_eq!(tiny.finals, 0);
        assert!(tiny.scores_lane);
    }

    #[test]
    fn my_games_rows_come_off_the_same_budget() {
        let without = plan(120, 30, 6, 2, 3, 0);
        let with = plan(120, 30, 6, 2, 3, 2);
        assert!(with.tier2 <= without.tier2, "band rows are not free");
    }
}
