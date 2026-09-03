//! The tier budget: spec §4's sizes ladder as one pure function. Height and
//! counts in, `TierPlan` out — nothing here reads a `Game` or renders a cell.
//!
//! Hero height is a pure function of the (width, height) bracket, never of
//! game counts — a hero that resized as games came and went would violate
//! the frozen-board principle (ruling R28). §4's "hero shrinks last" governs
//! only the runout cascade *below* the bracket (tier1 → tier2 → finals →
//! later), not the bracket itself.
//!
//! The scoring band (spec §3) is reserved, never inserted: `band_rows` is 2
//! on any board with a live game, charged before the tiers, so the board a
//! firing band lands on is the board that was already drawn.
//!
//! FINAL/LATER truncation legitimately fires `scores_lane` on its own, with
//! every live game still fully shown — spec §1's `2 OFF-SCREEN · 2 FINAL ·
//! 4 LATER` lane example is exactly that case.

/// How many rows each piece of the board gets at a given size. Pure: height
/// and counts in, plan out — the sizes ladder (spec §4) lives here and only here.
///
/// `tier2`, `finals`, and `later` counts used to live here too, but no
/// reader anywhere consumed them (task-9 review carry-forward #2, M3):
/// `board::board_walk` builds its block list from the FULL live/finals/later
/// counts and works out what actually fits with its own dynamic
/// `total > area.height` window/scroll math (`board/mod.rs`), never by
/// asking this plan how many finals or later rows it budgeted — so the three
/// fields were dead weight on every caller. Deleted; the per-section budget
/// math that used to feed them is still computed (and still tested, in
/// `plan_detailed` below) because it decides `scores_lane`, which
/// `app/mod.rs` DOES read (to size the ticker lane on every non-Board view).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TierPlan {
    pub hero_rows: u16, // 0 = no hero fits (only < 12 total rows)
    pub hero_digits_full: bool, // 8-row PixelSize::Full vs 4x3 sextant
    pub tier1: usize,   // promoted 3-row rows (0..=3)
    /// The scoring band's rows (spec §3), reserved at the TOP of the body:
    /// [`crate::board::cut::BAND_ROWS`] whenever something is live, 0 when
    /// nothing is. Reserved, not inserted on fire — a band that appeared out
    /// of nowhere shoved the whole board down two rows and back up again
    /// three seconds later, which was the last layout jump in the app. The
    /// rows are charged before the tiers (like the lane's row) so the board
    /// under a firing band is the board that was already there.
    pub band_rows: u16,
    /// The off-screen SCORES lane (spec §1) — on exactly when tier2, finals,
    /// or later got cut short of what the raw counts asked for.
    pub scores_lane: bool,
}

/// Row cost of one section rule/label line.
pub const RULE_ROWS: u16 = 1;

/// A section (MY GAMES / FINAL / LATER) costs its rule row only if it ends
/// up showing at least one content row — an empty section folds into the
/// lane instead of drawing a label over nothing. Returns `(rows_shown, cost)`.
fn section_alloc(requested: usize, budget: u16) -> (usize, u16) {
    if requested == 0 || budget == 0 {
        return (0, 0);
    }
    let content = requested.min(budget.saturating_sub(RULE_ROWS) as usize);
    if content == 0 {
        (0, 0)
    } else {
        (content, content as u16 + RULE_ROWS)
    }
}

/// The live list (tier1 + tier2) shares one "in-play" rule, charged only
/// when it actually shows a row — same fold-into-the-lane rule as above.
/// Returns `(tier1, tier2, cost)`.
fn live_alloc(live: usize, tier1_cap: usize, budget: u16) -> (usize, usize, u16) {
    if live == 0 || budget == 0 {
        return (0, 0, 0);
    }
    let avail = budget.saturating_sub(RULE_ROWS);
    // spec §4 tier 1: scaled by what's left, capped by width class.
    let tier1 = ((avail / 4) as usize).min(tier1_cap).min(live);
    let after_tier1 = avail.saturating_sub(3 * tier1 as u16);
    let live_left = live - tier1;
    // spec §1 tier 2: as many single lines as fit.
    let tier2 = live_left.min(after_tier1 as usize);
    let content = 3 * tier1 as u16 + tier2 as u16;
    if content == 0 {
        (0, 0, 0)
    } else {
        (tier1, tier2, content + RULE_ROWS)
    }
}

/// The sizes ladder (spec §4) as a top-down subtraction: hero first by the
/// width/height gates (never by `live`/`finals`/`later`/`my_games` — see the
/// module doc), then MY GAMES, then rows run out bottom-up — later first,
/// then finals, then tier2 truncates (lane on, paying for its own row),
/// tier1 demotes, hero shrinks last. The lane is decided by a first pass
/// with no row reserved for it; if that pass would have truncated anything,
/// the whole cascade below MY GAMES reruns with one row set aside for the
/// lane before tier1/tier2/finals/later fill again.
///
/// `my_games` is the MY GAMES band's row count, one line per pinned/favorite
/// game — **excluding** the hero, even when the hero is itself a MY GAMES
/// game (pins win the hero, spec §1). Layout only counts rows; deciding which
/// game is the hero and whether to fold it out of the band's count is the
/// caller's job — double-charging a pinned hero here is a caller bug, not a
/// layout one.
pub fn plan(width: u16, height: u16, live: usize, finals: usize, later: usize, my_games: usize) -> TierPlan {
    let full = plan_detailed(width, height, live, finals, later, my_games);
    TierPlan {
        hero_rows: full.hero_rows,
        hero_digits_full: full.hero_digits_full,
        tier1: full.tier1,
        band_rows: full.band_rows,
        scores_lane: full.scores_lane,
    }
}

/// Every row count the cascade computes, `tier2`/`finals`/`later` included —
/// kept private (and test-only in practice) now that nothing outside this
/// module's own budget-invariant tests reads the three fields `TierPlan`
/// dropped. `plan` above is the real, minimal public contract.
#[derive(Clone, Debug, PartialEq, Eq)]
struct FullPlan {
    hero_rows: u16,
    hero_digits_full: bool,
    tier1: usize,
    tier2: usize,
    finals: usize,
    later: usize,
    band_rows: u16,
    scores_lane: bool,
}

fn plan_detailed(width: u16, height: u16, live: usize, finals: usize, later: usize, my_games: usize) -> FullPlan {
    // spec §4 last row: `need 40×12, have W×H` — no hero fits below the floor.
    let (hero_rows, hero_digits_full): (u16, bool) = if width < 40 || height < 12 {
        (0, false)
    } else if width < 60 {
        // spec §4 "< 60 cols": 2-line compact hero, no digits.
        (2, false)
    } else if width >= 100 && height >= 32 {
        // spec §4 "≥120×40" / "100-119 cols": 8-row Full digits.
        //
        // Ruling R32: 12 rows, not 10. The band charges the digits first
        // (R29), so 10 rows are nameplate 1 + digits 8 + ONE spare, which the
        // keep order (R30) spends on the fragment — leaving the flagship hero
        // with neither the meter bar nor the last-play line that the 6-row
        // 80×24 hero does draw. 12 = 1 nameplate + 8 digits + fragment +
        // meter + play, which is the A′ frame's hero exactly (11 buys the
        // meter only). Costs 1–2 tier rows at 40 rows tall.
        (12, true)
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

    // spec §3: the scoring band's two rows, reserved up front whenever a band
    // could fire at all. Receipt for the 2: `cut::draw_band` draws exactly
    // `BAND_ROWS` rows — a headline and an affordance — and nothing else in
    // the app may decide that number. Charged here, before the tiers, for the
    // same reason the lane's row is: a row spent later is a row the board
    // already drew in, i.e. a jump. The `>` (not `>=`) keeps at least one
    // content row under the reservation — two rows of band over an empty body
    // is a takeover with extra steps, and at that size there is no board to
    // protect from jumping anyway.
    let band_rows = if live > 0 && height.saturating_sub(hero_rows) > crate::board::cut::BAND_ROWS {
        crate::board::cut::BAND_ROWS
    } else {
        0
    };

    // spec §1 MY GAMES band: 1 line per pinned/favorite row, off the same
    // budget as everything below the hero.
    let budget = height.saturating_sub(hero_rows).saturating_sub(band_rows);
    let (_my_games_rows, my_games_cost) = section_alloc(my_games, budget);
    let budget = budget.saturating_sub(my_games_cost);

    // First pass: nothing reserved for the lane yet.
    let (tier1_0, tier2_0, live_cost_0) = live_alloc(live, tier1_cap, budget);
    let live_left = live.saturating_sub(tier1_0);
    let remaining_0 = budget.saturating_sub(live_cost_0);
    let finals_target = finals.min(2);
    let (finals_0, finals_cost_0) = section_alloc(finals_target, remaining_0);
    let remaining_1 = remaining_0.saturating_sub(finals_cost_0);
    let (later_0, _) = section_alloc(later, remaining_1);

    // spec §1 Ticker: the lane comes on exactly when the un-laned pass would
    // have cut something short of what it asked for.
    let would_truncate =
        tier2_0 < live_left || finals_0 < finals_target || later_0 < later;

    let (tier1, tier2, finals_rows, later_rows) = if would_truncate {
        // The lane pays for its own row before the cascade fills again.
        let budget = budget.saturating_sub(1);
        let (tier1, tier2, live_cost) = live_alloc(live, tier1_cap, budget);
        let remaining = budget.saturating_sub(live_cost);
        let (finals_rows, finals_cost) = section_alloc(finals_target, remaining);
        let remaining = remaining.saturating_sub(finals_cost);
        let (later_rows, _) = section_alloc(later, remaining);
        (tier1, tier2, finals_rows, later_rows)
    } else {
        (tier1_0, tier2_0, finals_0, later_0)
    };

    FullPlan {
        hero_rows,
        hero_digits_full,
        tier1,
        tier2,
        finals: finals_rows,
        later: later_rows,
        band_rows,
        scores_lane: would_truncate,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sizes_ladder_matches_the_spec_table() {
        // 120x40: full hero, up to 3 promoted, no lane. Ruling R32: the Full
        // bracket is 12 rows — nameplate + 8 digit rows + fragment + meter +
        // play — so the flagship hero is never poorer than the 80×24 one.
        let p = plan(120, 38, 8, 2, 4, 0);
        assert_eq!(p.hero_rows, 12);
        assert!(p.hero_digits_full);
        assert_eq!(p.tier1, 3);
        assert!(!p.scores_lane);
        // 80x24: sextant hero, all tier 2, lane when truncated.
        let p = plan(80, 22, 8, 2, 4, 0);
        assert!(p.hero_rows <= 6 && p.hero_rows >= 4);
        assert!(!p.hero_digits_full);
        assert_eq!(p.tier1, 0);
        // All 8 live games fit; it's LATER that truncates and trips the lane.
        assert!(p.scores_lane, "LATER doesn't fit 22 rows — lane on");
        // <60 cols: 2-row compact hero, still one list.
        let p = plan(55, 38, 3, 1, 2, 0);
        assert_eq!(p.hero_rows, 2);
        // Never negative / overlapping: sum of allocated rows <= height.
        for (w, h) in [(40u16, 12u16), (60, 20), (100, 30), (200, 60)] {
            let p = plan_detailed(w, h, 12, 5, 8, 2);
            let used = p.hero_rows
                + p.band_rows
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
        let tall = plan_detailed(120, 38, 6, 3, 6, 0);
        let short = plan_detailed(120, 26, 6, 3, 6, 0);
        assert!(short.later < tall.later, "later shrinks first");
        assert!(
            short.tier2 + short.tier1 >= 6usize.min(tall.tier1 + tall.tier2),
            "live rows survive"
        );
        // Down further: finals go, then the lane appears.
        let tiny = plan_detailed(120, 16, 6, 3, 6, 0);
        assert_eq!(tiny.finals, 0);
        assert!(tiny.scores_lane);
        // The public `plan` agrees with `plan_detailed` on every field it
        // still carries — it's a strict projection, not a second cascade.
        let public = plan(120, 26, 6, 3, 6, 0);
        assert_eq!(public.hero_rows, short.hero_rows);
        assert_eq!(public.hero_digits_full, short.hero_digits_full);
        assert_eq!(public.tier1, short.tier1);
        assert_eq!(public.scores_lane, short.scores_lane);
    }

    /// spec §3: the scoring band is TWO rows (`cut::BAND_ROWS`, and
    /// `draw_band` draws exactly that many), and they are *reserved* whenever
    /// a band could fire — i.e. whenever something is live. A board with no
    /// live game cannot fire one, so it reserves nothing and its budget is
    /// exactly the one Task 5 gave it.
    #[test]
    fn no_live_games_means_no_reservation() {
        let finals_only = plan(120, 36, 0, 5, 4, 0);
        assert_eq!(finals_only.band_rows, 0, "no live game can fire a band");
        let live = plan(120, 36, 6, 5, 4, 0);
        assert_eq!(live.band_rows, crate::board::cut::BAND_ROWS);
        // The reservation is charged before the tiers, like the lane's row:
        // a live board at H allocates the content a finals-only board would
        // have allocated at H - BAND_ROWS, never more.
        let reserved = plan_detailed(120, 30, 6, 2, 3, 0);
        let shorter = plan_detailed(120, 30 - crate::board::cut::BAND_ROWS, 6, 2, 3, 0);
        assert_eq!(reserved.tier1, shorter.tier1);
        assert_eq!(reserved.tier2, shorter.tier2);
        assert_eq!(reserved.finals, shorter.finals);
        assert_eq!(reserved.later, shorter.later);
    }

    #[test]
    fn my_games_rows_come_off_the_same_budget() {
        let without = plan_detailed(120, 30, 6, 2, 3, 0);
        let with = plan_detailed(120, 30, 6, 2, 3, 2);
        assert!(with.tier2 <= without.tier2, "band rows are not free");
    }

    /// Full accounting invariant, swept: hero + 3·tier1 + tier2 + finals +
    /// later + my_games + one RULE_ROW per section actually rendering
    /// content + the lane's own row must never exceed the height it was
    /// given. A section's rule is charged only when it shows content — an
    /// empty section folds into the lane rather than drawing a label over
    /// nothing (see `section_alloc`/`live_alloc`).
    #[test]
    fn the_budget_never_over_allocates() {
        let widths = [40u16, 55, 60, 80, 100, 120, 180];
        let heights = [12u16, 13, 15, 16, 22, 24, 26, 30, 40, 60];
        let counts = [
            (0usize, 0usize, 0usize, 0usize),
            (3, 1, 2, 0),
            (6, 3, 6, 0),
            (8, 2, 4, 0),
            (12, 5, 8, 2),
            (20, 0, 0, 0),
            (15, 3, 3, 0),
        ];
        for &w in &widths {
            for &h in &heights {
                for &(live, finals, later, my_games) in &counts {
                    let p = plan_detailed(w, h, live, finals, later, my_games);
                    let my_games_rows = my_games.min(h as usize); // matches section_alloc's cap
                    let sections_rendering = [
                        my_games_rows > 0,
                        p.tier1 + p.tier2 > 0,
                        p.finals > 0,
                        p.later > 0,
                    ]
                    .into_iter()
                    .filter(|&present| present)
                    .count() as u16;
                    let used = p.hero_rows
                        + p.band_rows
                        + 3 * p.tier1 as u16
                        + p.tier2 as u16
                        + p.finals as u16
                        + p.later as u16
                        + my_games_rows as u16
                        + sections_rendering * RULE_ROWS
                        + if p.scores_lane { 1 } else { 0 };
                    assert!(
                        used <= h,
                        "{w}x{h} live={live} finals={finals} later={later} my_games={my_games}: used {used} > {h} ({p:?})"
                    );
                }
            }
        }
    }
}
