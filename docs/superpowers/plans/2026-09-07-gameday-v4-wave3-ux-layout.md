# gameday v4 Wave 3 — UX and Layout Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The board is navigable at 189 games and honest at zero: paging keys that move, a filter that means what it says, a help overlay ordered by the mode you are in, toasts that never hide the chords, an empty day that names the next game, and a row grid where nothing glues to its neighbor.

**Architecture:** Every item is a change inside an existing surface, tested through the real renderer (`tests/draw.rs` drives `App::draw` into a `TestBackend`; `src/board/rows.rs` tests drive one row). The row grid becomes a derivation from four widths instead of eight literals; the keymap grows a paging group and regroups by mode; the footer's status becomes a right-aligned, expiring toast; `src/filter.rs` is the one new module.

**Tech Stack:** Rust 2021, ratatui 0.30, crossterm 0.29, the `time` crate; no new dependencies.

**Spec:** `docs/superpowers/specs/2026-09-06-gameday-v4-ship-design.md` §5 (Wave 3), §1 U1–U9 and L1–L6, Direction 7. L7 (wide terminals) is wave 5's.

## Global Constraints

- Branch `v4-wave3` from `main` at `24c406b`. Commit after every task with the trailer; never push.
- Suite green after every task with the count reported (587 at the start; measured numbers win over the ledger below); `cargo clippy --all-targets -- -D warnings` and `cargo fmt --check` clean before every commit. No `#[allow(...)]`.
- Numbers carry receipts: every new constant says what it holds and why in its doc comment. `rows.rs` keeps no literal column offsets below its width constants — every `_X` is derived.
- Failures name the value, the expectation, and the knob.
- Pinned strings (tests assert them verbatim): empty-day lines `No NFL games MON SEP 7 · next TUE 7:10 PM NYY @ BOS` and `No MLB games MON SEP 7 · nothing scheduled`; footer filter `/ore · 3 games` and `/kc · 1 game`; help legend `▸ selected · ⚑ pinned · ★ favorite · ▌ hot · ↑n moved up`; help section titles `board`, `zoom`, `tv`, `config`, `standings & feed`, `paging`, `everywhere`.
- Rulings made while planning (the spec is the authority; these settle what it left open):
  - **U9, the `▌` on the hero record line:** it is `hero.rs`'s lookalike-color swatch (a one-cell block beside the home abbr when the home side "fell" to the fallback color), not a possession mark. The spec says a swatch is removed; it is removed, and the legend's `▌` entry names the row gutter's hot mark (`▌ hot`), the only `▌` left.
  - **Toast expiry is wall clock, not ticks:** `TOAST_SECS = 3` measured with `App::now()`. The idle loop ticks once a second, so "3 s of ticks" (30 ticks) would be thirty seconds on a quiet evening, which is exactly when pins happen. Tests move `now_override`.
  - **Paging binds in the zoom's PLAYS/STATS tabs too**, not only board/feed/standings: those tabs scroll with j/k, and a PgDn that is live in three scrolling views and dead in the fourth is the U1 finding again.
  - **`cycle_theme` is deleted**, not kept: once `c` opens the picker nothing calls it, and dead code fails clippy. `:theme <name>` never used it.
  - **`ABBR_W = 5` is a column pitch** ("four cells of abbr plus one of air") shared by the row grid, the STATS leaders column and the standings table; inside the row grid the abbr text is right/left-aligned in `ABBR_TEXT_W = 4` cells and the fifth cell is the air. The resulting grid: `AWAY_ABBR_X 5 · AWAY_SCORE_X 10 · HOME_ABBR_X 14 · HOME_SCORE_X 19 · CLOCK_X 23 · LEAGUE_X 38 · TEXT_X 43 · ODDS_X 51` (was 4/9/13/18/22/33/38/45).
  - **Config content starts one row under the CONFIG header** (`pane.y + 1`), not centered.
  - **Prompt errors are sticky** (`unknown command …`, `no game found for …`): they name a fix the user has to read; the next keypress clears them as before. Pin/favorite/sort confirmations are toasts.
- Commit trailer:
  ```
  Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01UyD3BUJGnLsYpUqmtLdqbj
  ```

---

## File map

| File | Responsibility after this wave |
|---|---|
| `src/board/rows.rs` | the grid derived from `GUTTER`, `ABBR_W`, `SCORE_W`, `CLOCK_W`, `LEAGUE_W`, `BCAST_W`, `GAP`; four grid teeth |
| `src/views/zoom.rs` | `feed_split` proportional fill; leaders column on `ABBR_W`; records `page_rows` |
| `src/views/standings.rs` | abbr column from `rows::ABBR_W`; records `page_rows` |
| `src/views/config_view.rs` | top-aligned block |
| `src/views/plays_feed.rs` | records `page_rows` |
| `src/board/mod.rs` | IN PLAY rule only over rows; empty league line; records `page_rows` |
| `src/board/hero.rs` | no color swatch on the nameplate |
| `src/app/keys/board.rs`, `keys/theme.rs` | `c` opens the picker; `cycle_theme` gone |
| `src/filter.rs` (new), `src/lib.rs`, `src/app/derive.rs`, `src/app/chrome.rs` | `Query`; footer `/ore · 3 games` |
| `src/app/mod.rs`, `src/app/persist.rs`, `src/app/merge.rs`, `src/app/keys/*.rs`, `src/input.rs`, `src/main.rs` | `toast` / `sticky_status` / `report_save`; `page_rows`; paging dispatch |
| `src/keymap.rs` | `Group` by mode + `Paging`; `help_order`; two paging bindings |
| `src/command.rs`, `src/input.rs` | `:help` |
| `tests/draw.rs`, `src/board/rows.rs` tests, `src/filter.rs` tests, `src/views/zoom.rs` tests | the teeth |

---

### Task 1: The grid — nothing glues (L1, L2, L3, L4)

**Files:**
- Modify: `src/board/rows.rs` (the constants block at lines 80–126 and every `col(...)` call that uses them)
- Modify: `src/views/zoom.rs` (the leaders line, `format!("  {:<4}", leader.team)`)
- Modify: `src/views/standings.rs` (`const ABBR_W: usize = 5;`)
- Test: `src/board/rows.rs` test module (three teeth), `tests/draw.rs` (the STATS tooth)

**Interfaces:**
- Produces: `pub const ABBR_W: u16 = 5` in `rows.rs`, consumed by `zoom.rs` and `standings.rs`.

- [ ] **Step 1: Write the failing tests** — in `src/board/rows.rs`'s `mod tests` (helpers `live_game`, `tier2_game`, `ctx`, `render`, `text_of`, `col_of` exist there):

```rust
    /// L3: `↑9TNST`. The nudge field is three cells so a two-glyph climb
    /// keeps a cell of air before a four-letter code.
    #[test]
    fn a_four_letter_code_with_a_nudge_keeps_its_gap() {
        let mut game = tier2_game();
        game.away.abbr = "TNST".into();
        game.home.abbr = "UGA".into();
        let mut c = ctx();
        c.nudge = Some(12); // prints ↑9: "rose 9 or more"
        let term = render(120, 1, &game, &c, draw_tier2);
        let buf = term.backend().buffer();
        let text = text_of(buf);
        assert_eq!(col_of(buf, 0, "↑9"), Some(NUDGE_X), "{text}");
        assert_eq!(col_of(buf, 0, "TNST"), Some(AWAY_ABBR_X), "{text}");
        assert!(!text.contains("↑9TNST"), "no air after the nudge\n{text}");
        assert_eq!(buf[(AWAY_ABBR_X + ABBR_TEXT_W, 0)].symbol(), " ", "air before the score\n{text}");
        assert_eq!(col_of(buf, 0, "13"), Some(AWAY_SCORE_X), "{text}");
    }

    /// L1 + L2: a start more than six days out prints whole at every width,
    /// and `NETFLIX` is followed by air before the odds.
    #[test]
    fn a_later_row_a_week_out_prints_its_whole_date_at_80_and_200_columns() {
        let mut game = live_game("TB", "ATL");
        game.status = Status::Pre;
        game.situation = None;
        game.last_plays.clear();
        // Eight days past ctx().now (2026-09-13): the month-day form.
        game.start = Some(datetime!(2026-09-21 20:20 -4));
        game.broadcast = Some("NETFLIX".into());
        game.odds = Some("LAR -3.5  O/U 44.5".into());
        for w in [80u16, 200] {
            let term = render(w, 1, &game, &ctx(), draw_tier3);
            let buf = term.backend().buffer();
            let text = text_of(buf);
            assert_eq!(col_of(buf, 0, "SEP 21 8:20 PM"), Some(CLOCK_X), "whole start at {w} columns\n{text}");
            assert_eq!(buf[(CLOCK_X + CLOCK_W, 0)].symbol(), " ", "air before the tag at {w}\n{text}");
            assert_eq!(col_of(buf, 0, "NFL"), Some(LEAGUE_X), "{text}");
            assert_eq!(col_of(buf, 0, "NETFLIX"), Some(TEXT_X), "{text}");
            assert_eq!(buf[(TEXT_X + BCAST_W, 0)].symbol(), " ", "air after NETFLIX at {w}\n{text}");
            assert_eq!(col_of(buf, 0, "LAR -3.5"), Some(ODDS_X), "{text}");
        }
    }

    /// The grid is a derivation, and this is the receipt for its numbers.
    #[test]
    fn the_grid_derives_from_its_widths() {
        assert_eq!(GUTTER, MARK_W + NUDGE_W);
        assert_eq!(AWAY_SCORE_X, AWAY_ABBR_X + ABBR_W);
        assert_eq!(HOME_ABBR_X, AWAY_SCORE_X + SCORE_W + GAP);
        assert_eq!(HOME_SCORE_X, HOME_ABBR_X + ABBR_W);
        assert_eq!(CLOCK_X, HOME_SCORE_X + SCORE_W + GAP);
        assert_eq!(LEAGUE_X, CLOCK_X + CLOCK_W + GAP);
        assert_eq!(TEXT_X, LEAGUE_X + LEAGUE_W + GAP);
        assert_eq!(ODDS_X, TEXT_X + BCAST_W + GAP);
        assert_eq!(CLOCK_W as usize, "SEP 21 8:20 PM".chars().count(), "CLOCK_W holds the longest state");
        assert_eq!(BCAST_W as usize, "NETFLIX".len());
    }
```

And in `tests/draw.rs` (add a `col_of` helper next to `buf_text`):

```rust
/// The column `needle` starts at on row `y` of the last frame, or None.
fn col_of(term: &Terminal<TestBackend>, y: u16, needle: &str) -> Option<u16> {
    let b = term.backend().buffer();
    let row: String = (0..b.area().width).map(|x| b[(x, y)].symbol()).collect::<Vec<_>>().concat();
    row.find(needle).map(|byte| row[..byte].chars().count() as u16)
}

/// L4: `BOISPASSING YARDS`. The leaders column is the row grid's abbr pitch.
#[test]
fn stats_leaders_keep_a_gap_after_a_four_letter_code() {
    use gameday::views::{View, ZoomTab};
    let mut app = mk();
    app.apply_boards(League::Nfl, vec![g("1", "BOIS", "ORE", true)], false);
    app.tab = Tab::League(League::Nfl);
    app.stats.insert(
        "1".into(),
        GameStats {
            rows: vec![StatRow { label: "Total yards".into(), away: "412".into(), home: "388".into() }],
            leaders: vec![Leader { team: "BOIS".into(), label: "Passing yards".into(), text: "M. Madsen 24/31, 288".into() }],
        },
    );
    app.view = View::Zoom { game_id: "1".into(), tab: ZoomTab::Stats };
    let term = render(&mut app, 120, 40);
    let s = buf_text(&term);
    let y = s.lines().position(|l| l.contains("PASSING YARDS")).expect("leader row") as u16;
    let abbr = col_of(&term, y, "BOIS").expect("abbr");
    assert_eq!(col_of(&term, y, "PASSING YARDS"), Some(abbr + gameday::board::rows::ABBR_W), "{s}");
    assert!(!s.contains("BOISPASSING"), "{s}");
}
```

- [ ] **Step 2: Run them; expect** the grid test to fail on `MARK_W`/`NUDGE_W`/`ODDS_X` not existing, the nudge test on `↑9TNST`, the later-row test on `SEP 21 8:20 P`, the stats test on `BOISPASSING`.

- [ ] **Step 3: Implement the derivation** — replace the constants block in `rows.rs`:

```rust
/// Column pitch of a team abbreviation: four cells hold every abbr the mapper
/// emits (`WSH`, `MTL`, `TNST`, soccer's four-letter clubs) and the fifth is
/// air. One number for the board rows, the STATS leaders column and the
/// standings table, so a four-letter code can never glue itself to what
/// follows (the review's `BOISPASSING YARDS`).
pub const ABBR_W: u16 = 5;
/// The cells an abbr's text may occupy inside its column; the last is air.
const ABBR_TEXT_W: u16 = ABBR_W - 1;
/// Score field width: three cells for a college basketball 100+.
const SCORE_W: u16 = 3;
/// Air between two fields whose pitch does not already carry it.
const GAP: u16 = 1;
/// The hot mark and its air.
const MARK_W: u16 = 2;
/// The nudge: `↑9` plus one cell of air before the abbr — `↑9TNST` was the
/// review's L3.
const NUDGE_W: u16 = 3;
/// The two fixed gutters every tier-1/2 row reserves.
pub const GUTTER: u16 = MARK_W + NUDGE_W;
const NUDGE_X: u16 = MARK_W;
/// Largest climb two glyphs can say truthfully: `↑9` means "rose 9 or more".
const NUDGE_MAX: usize = 9;

const AWAY_ABBR_X: u16 = GUTTER;
const AWAY_SCORE_X: u16 = AWAY_ABBR_X + ABBR_W;
const HOME_ABBR_X: u16 = AWAY_SCORE_X + SCORE_W + GAP;
const HOME_SCORE_X: u16 = HOME_ABBR_X + ABBR_W;
const CLOCK_X: u16 = HOME_SCORE_X + SCORE_W + GAP;
/// The longest state a row prints is a start more than six days out,
/// `SEP 21 8:20 PM`: fourteen cells (the review's L1 clipped it at eleven).
const CLOCK_W: u16 = 14;
const LEAGUE_X: u16 = CLOCK_X + CLOCK_W + GAP;
const LEAGUE_W: u16 = 4;
/// Where a row's prose starts: the headline, the fragment, the broadcast.
const TEXT_X: u16 = LEAGUE_X + LEAGUE_W + GAP;
/// Broadcast field on a LATER row: `NETFLIX` is the longest at seven.
const BCAST_W: u16 = 7;
/// The odds after the broadcast, with air between (L2: `NETFLIXLAR -3.5`).
const ODDS_X: u16 = TEXT_X + BCAST_W + GAP;
const T1_CHIP_W: u16 = TEXT_X - CLOCK_X;
```

Then: `state_text` truncates to `CLOCK_W as usize` (not `- 1`; the air is `GAP` now). Every `col(...)` that drew an abbr with width `ABBR_W` draws it with `ABBR_TEXT_W` (away right-aligned, home left-aligned, tier-1's league tag under the away abbr right-aligned). The nudge draws with width `NUDGE_W` at `NUDGE_X`. The odds draw at `ODDS_X` (`let x = ODDS_X; let room = area.width.saturating_sub(x)`). The module doc's "Columns 2–3 are the nudge" becomes "Columns 2–4". In `zoom.rs` the leaders line becomes `format!("  {:<w$}", leader.team, w = crate::board::rows::ABBR_W as usize)`; in `standings.rs`, `const ABBR_W: usize = crate::board::rows::ABBR_W as usize;` with its comment saying where the number lives.

- [ ] **Step 4: Update the existing position tests to the derived constants.** `tier3_later_shows_local_time_never_iso` asserts `TB` at `AWAY_ABBR_X + ABBR_W - 3` → `AWAY_ABBR_X + ABBR_TEXT_W - 3`, `@` at `AWAY_SCORE_X + 1`, odds at `TEXT_X + BCAST_W` → `ODDS_X`. Run `cargo test --release` and fix every other assertion the move breaks (the dump gallery's `gutter_nudges` and any `tests/draw.rs` literal column) by naming the constant, never by pasting the new number. List each in the report.

- [ ] **Step 5: Suite, clippy, fmt, commit** `fix(rows): the grid derives from its widths — nothing glues to a four-letter code`.

### Task 2: Zoom fill and the config block (L5, L6)

**Files:**
- Modify: `src/views/zoom.rs` (`draw_feed`, lines ~389–470), `src/views/config_view.rs` (line ~150, `let y = …`)
- Test: `src/views/zoom.rs` test module (new), `tests/draw.rs`

**Interfaces:** `pub(crate) fn feed_split(avail: usize, plays: usize, scoring: usize) -> (usize, usize)` in `zoom.rs`.

- [ ] **Step 1: Failing unit test** in `zoom.rs` (add `#[cfg(test)] mod tests` if absent):

```rust
    #[test]
    fn feed_split_shares_the_pane_proportionally_with_a_floor_of_four() {
        assert_eq!(feed_split(20, 6, 6), (6, 6), "fits: nothing to share");
        assert_eq!(feed_split(20, 40, 10), (16, 4), "proportional, scoring at its floor");
        assert_eq!(feed_split(20, 40, 2), (18, 2), "a section never gets more than it has");
        assert_eq!(feed_split(20, 3, 40), (3, 17), "the other section takes the leftover");
        assert_eq!(feed_split(8, 40, 40), (4, 4), "both at the floor");
        assert_eq!(feed_split(3, 40, 40), (2, 1), "under two floors: proportional, scoring last");
    }
```

- [ ] **Step 2: Failing draw test** in `tests/draw.rs`:

```rust
/// L5: five to eight blank rows under SCORING at 40 rows. The two feeds
/// share the body in proportion, each floored at four, so a game with the
/// plays to fill the pane fills it.
#[test]
fn zoom_overview_fills_the_pane_at_30_40_and_60_rows() {
    use gameday::views::{View, ZoomTab};
    let mut game = g("1", "KC", "TB", true);
    game.last_plays = (0..40u16)
        .map(|i| Play {
            clock: format!("{}:{:02}", 14 - i / 4, 59 - i),
            period: "Q1".into(),
            team: "KC".into(),
            text: format!("play {i}"),
            scoring: i % 4 == 0,
            ..Default::default()
        })
        .collect();
    let game = with_scoring(game);
    let mut app = mk();
    app.apply_boards(League::Nfl, vec![game], false);
    app.tab = Tab::League(League::Nfl);
    app.view = View::Zoom { game_id: "1".into(), tab: ZoomTab::Overview };
    for h in [30u16, 40, 60] {
        let term = render(&mut app, 120, h);
        let s = buf_text(&term);
        let lines: Vec<&str> = s.lines().collect();
        let plays_y = lines.iter().position(|l| l.contains("LAST PLAYS")).expect("caption");
        let scoring_y = lines.iter().position(|l| l.contains("SCORING")).expect("caption");
        let last_body = h as usize - 2; // row h-1 is the footer
        assert!(!lines[last_body].trim().is_empty(), "blank band at {h} rows:\n{s}");
        assert!(scoring_y - plays_y - 1 >= 4, "LAST PLAYS keeps four rows at {h}:\n{s}");
        assert!(last_body - scoring_y >= 4, "SCORING keeps four rows at {h}:\n{s}");
    }
}

/// L6: the config editor started at row 15 of 40. It sits under its header.
#[test]
fn config_view_top_aligns_under_its_header() {
    use gameday::views::View;
    let mut app = mk();
    app.view = View::ConfigView;
    let term = render(&mut app, 120, 40);
    let s = buf_text(&term);
    let tabs_y = s.lines().position(|l| l.contains("TABS")).expect("TABS section");
    assert_eq!(tabs_y, 3, "header, CONFIG chip, one row of air, then TABS:\n{s}");
}
```

- [ ] **Step 3: Implement.** In `zoom.rs`:

```rust
/// Rows a feed section keeps even when the other is long: a caption over
/// fewer than four rows is a header over a void (the old `FEED_MIN` said
/// the same about the whole feed).
const FEED_FLOOR: usize = 4;

/// Split `avail` body rows (the two rules and two captions already taken)
/// between LAST PLAYS and SCORING: in proportion to what each has, each
/// floored at [`FEED_FLOOR`] when it has that many, and rows one section
/// cannot use go to the other — the pane fills whenever the game has the
/// plays to fill it (L5 was five to eight blank rows under SCORING).
pub(crate) fn feed_split(avail: usize, plays: usize, scoring: usize) -> (usize, usize) {
    if plays + scoring <= avail {
        return (plays, scoring);
    }
    let floor_p = FEED_FLOOR.min(plays);
    let floor_s = FEED_FLOOR.min(scoring);
    let mut p = (avail * plays / (plays + scoring)).max(floor_p).min(plays);
    let mut s = avail.saturating_sub(p).min(scoring);
    if s < floor_s {
        s = floor_s.min(avail);
        p = avail.saturating_sub(s).min(plays);
    }
    if p + s < avail {
        p = avail.saturating_sub(s).min(plays);
    }
    if p + s < avail {
        s = avail.saturating_sub(p).min(scoring);
    }
    (p, s)
}
```

`draw_feed` computes `let avail = (area.height as usize).saturating_sub(4); let (p_rows, s_rows) = feed_split(avail, game.last_plays.len().max(1), game.scoring_plays.len().max(1));` (an empty section still prints its one-line message, so it counts one), takes `p_rows` last plays and `s_rows` scoring plays (newest first, as now), and keeps `lines.truncate(area.height as usize)`. `draw_overview` is unchanged: the linescore is already a fixed block. In `config_view.rs`: `let y = pane.y + 1;` with a comment ("one row of air under the CONFIG header; L6 centered a short editor at row 15 of 40").

- [ ] **Step 4: Suite, clippy, fmt, commit** `fix(zoom,config): the overview fills its pane; the editor sits under its header`.

### Task 3: No orphan rule, no swatch, `c` opens the picker (U8, U9, U4)

**Files:**
- Modify: `src/board/mod.rs` (the `IN PLAY` rule push, ~line 175, and the window comment at ~268), `src/board/hero.rs` (`nameplate`, ~line 440), `src/app/keys/board.rs` (line 34), `src/app/keys/theme.rs` (delete `cycle_theme`)
- Test: `tests/draw.rs`

- [ ] **Step 1: Failing tests**

```rust
/// U8: the IN PLAY rule drew over nothing when the hero absorbed the only
/// live game. A section rule needs a row under it.
#[test]
fn the_in_play_rule_needs_a_row_under_it() {
    let mut app = board_app(1, 1, 1);
    let s = buf_text(&render(&mut app, 120, 40));
    assert!(!s.contains("IN PLAY"), "one live game is the hero, not a section:\n{s}");
    assert!(s.contains("FINAL") && s.contains("LATER"), "{s}");
    let mut app = board_app(2, 0, 0);
    let s = buf_text(&render(&mut app, 120, 40));
    assert!(s.contains("IN PLAY"), "a second live game is a section:\n{s}");
}

/// U9: `0-0 ▌ISU` — the `▌` on the hero record line was the lookalike-color
/// swatch, not a possession mark. Gone; the record meets the abbr.
#[test]
fn the_hero_nameplate_carries_no_color_swatch() {
    let mut game = g("1", "KC", "TB", true);
    game.home.color = game.away.color; // the lookalike rule "fells" the home color
    game.home.record = "1-0".into();
    game.away.record = "1-0".into();
    let mut app = mk();
    app.apply_boards(League::Nfl, vec![game], false);
    app.tab = Tab::League(League::Nfl);
    let s = buf_text(&render(&mut app, 120, 40));
    assert!(!s.contains("▌TB"), "swatch beside the home abbr:\n{s}");
    assert!(s.contains("1-0 TB"), "record then abbr, one space:\n{s}");
}

/// U4: `c` cycled themes silently while `:theme` opened a picker.
#[test]
fn c_opens_the_theme_picker() {
    use gameday::views::View;
    let mut app = mk();
    app.apply_boards(League::Nfl, vec![g("1", "KC", "TB", true)], false);
    app.tab = Tab::League(League::Nfl);
    key(&mut app, crossterm::event::KeyCode::Char('c'));
    assert_eq!(app.view, View::ThemePicker);
    let s = buf_text(&render(&mut app, 120, 40));
    assert!(s.contains("broadcast") && s.contains("studio"), "picker over the board:\n{s}");
    assert!(app.status_line.is_none(), "no 'theme X' toast: nothing changed yet");
}
```

- [ ] **Step 2: Implement.** `board/mod.rs`: `if d.in_play.len() > usize::from(in_play_hero.is_some()) { blocks.push(Block::Rule("IN PLAY", …)); }` and rewrite the window comment that says a hero-only IN PLAY rule "stands on its own by design" (it no longer exists). `hero.rs`: delete `block` and its `spans.extend(block)`; bind `hero_pair`'s third element as `_` if nothing else reads it. `keys/board.rs`: `KeyCode::Char('c') => self.open_theme_picker(),`; delete `cycle_theme` from `keys/theme.rs` and its module doc's mention. If `theme::next_name` is then only used by `keys/config.rs`, it stays (it is).

- [ ] **Step 3: Suite, clippy, fmt, commit** `fix(board,hero,keys): a rule needs a row; the nameplate loses its swatch; c opens the picker`.

### Task 4: The filter means what it says (U2)

**Files:**
- Create: `src/filter.rs`
- Modify: `src/lib.rs` (`pub mod filter;`), `src/app/derive.rs` (`game_matches`, `visible_games`, `ticker_live`), `src/app/chrome.rs` (the ` /{f}` span and `filter_width`)
- Test: `src/filter.rs` tests, `tests/draw.rs`

**Interfaces:** `pub struct Query`; `Query::parse(&str) -> Query`; `Query::matches(&self, &Game) -> bool`; `Query::is_empty(&self) -> bool`.

- [ ] **Step 1: Failing unit tests** (in `src/filter.rs`, written together with the module):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Team;

    fn game(league: League, away: (&str, &str, &str), home: (&str, &str, &str)) -> Game {
        let t = |(abbr, location, name): (&str, &str, &str)| Team {
            abbr: abbr.into(),
            location: location.into(),
            name: name.into(),
            ..Default::default()
        };
        Game { league, away: t(away), home: t(home), ..Default::default() }
    }

    #[test]
    fn a_token_starts_a_word_and_never_sits_inside_one() {
        let q = Query::parse("ore");
        assert!(q.matches(&game(League::Cfb, ("ORST", "Oregon State", "Beavers"), ("BOIS", "Boise State", "Broncos"))));
        assert!(!q.matches(&game(League::Nfl, ("BAL", "Baltimore", "Ravens"), ("KC", "Kansas City", "Chiefs"))), "ore inside baltimORE");
        assert!(!q.matches(&game(League::Cfb, ("WAKE", "Wake Forest", "Demon Deacons"), ("VAN", "Vanderbilt", "Commodores"))));
    }

    #[test]
    fn a_league_slug_scopes_and_the_rest_must_all_match() {
        let q = Query::parse("nfl kc");
        assert!(q.matches(&game(League::Nfl, ("KC", "Kansas City", "Chiefs"), ("TB", "Tampa Bay", "Buccaneers"))));
        assert!(!q.matches(&game(League::Cbb, ("KC", "Kansas City", "Roos"), ("UNI", "Northern Iowa", "Panthers"))), "wrong league");
        let both = Query::parse("kc tb");
        assert!(both.matches(&game(League::Nfl, ("KC", "Kansas City", "Chiefs"), ("TB", "Tampa Bay", "Buccaneers"))));
        assert!(!both.matches(&game(League::Nfl, ("KC", "Kansas City", "Chiefs"), ("BUF", "Buffalo", "Bills"))), "every token must land");
    }

    #[test]
    fn an_empty_query_matches_everything_and_case_is_ignored() {
        assert!(Query::parse("").is_empty());
        assert!(Query::parse("  ").matches(&game(League::Mlb, ("NYY", "New York", "Yankees"), ("BOS", "Boston", "Red Sox"))));
        assert!(Query::parse("RED").matches(&game(League::Mlb, ("NYY", "New York", "Yankees"), ("BOS", "Boston", "Red Sox"))));
    }
}
```

- [ ] **Step 2: The module**

```rust
//! The `/` filter: prefix tokens over team words, not a substring over every
//! field. `/ore` once matched Baltimore, Vanderbilt, Eastern Shore and a
//! "Forest" headline (U2); a token now has to start the abbr or a word of
//! the location or name, and every token has to land on one of the two
//! teams. A token that is a league slug scopes the query to that league.

use crate::domain::{Game, League, Team};

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Query {
    /// Lowercased tokens; each must prefix-match a word of either team.
    tokens: Vec<String>,
    /// Set by the first token that is a league slug; a second slug stays a
    /// plain token (and matches nothing), which is the honest reading.
    league: Option<League>,
}

impl Query {
    pub fn parse(s: &str) -> Query {
        let mut q = Query::default();
        for tok in s.split_whitespace() {
            let tok = tok.to_lowercase();
            match (q.league, League::from_slug(&tok)) {
                (None, Some(l)) => q.league = Some(l),
                _ => q.tokens.push(tok),
            }
        }
        q
    }

    pub fn is_empty(&self) -> bool {
        self.tokens.is_empty() && self.league.is_none()
    }

    pub fn matches(&self, game: &Game) -> bool {
        if self.league.is_some_and(|l| l != game.league) {
            return false;
        }
        self.tokens
            .iter()
            .all(|t| [&game.away, &game.home].into_iter().any(|team| starts_a_word(team, t)))
    }
}

fn starts_a_word(team: &Team, token: &str) -> bool {
    team.abbr.to_lowercase().starts_with(token)
        || team
            .location
            .split_whitespace()
            .chain(team.name.split_whitespace())
            .any(|w| w.to_lowercase().starts_with(token))
}
```

`derive.rs`: delete `game_matches`; `visible_games` and `ticker_live` build `let q = self.active_filter().map(crate::filter::Query::parse);` once and filter with `q.as_ref().is_none_or(|q| q.matches(g))`. `chrome.rs`: the committed-filter span becomes ` /{f} · {n} game{s}` with `n = self.derived().selection.len()` (`1 game`, `3 games`), and `filter_width` counts the whole string.

- [ ] **Step 3: Failing draw test**

```rust
/// The footer says how many games the filter left: `/tb · 2 games`.
#[test]
fn the_filter_footer_counts_what_it_kept() {
    let mut app = mk();
    app.apply_boards(League::Nfl, vec![g("1", "KC", "TB", true), g("2", "DAL", "TB", true), g("3", "GB", "CHI", true)], false);
    app.tab = Tab::League(League::Nfl);
    app.filter = Some("tb".into());
    let s = buf_text(&render(&mut app, 120, 24));
    assert!(s.contains("/tb · 2 games"), "{s}");
    app.filter = Some("kc".into());
    let s = buf_text(&render(&mut app, 120, 24));
    assert!(s.contains("/kc · 1 game"), "{s}");
    assert!(!s.contains("1 games"), "{s}");
}
```

- [ ] **Step 4: Suite, clippy, fmt, commit** `feat(filter): prefix tokens over team words, a league slug scopes, the footer counts`.

### Task 5: Toasts keep the chords (U7)

**Files:**
- Modify: `src/app/mod.rs` (`TOAST_SECS`, `status_toasted_at`, `toast`, `sticky_status`, `report_save`, `advance_tick`), `src/app/chrome.rs` (`draw_footer`), and every writer of `status_line`: `src/app/persist.rs`, `src/app/merge.rs`, `src/app/keys/board.rs`, `src/app/keys/config.rs`, `src/input.rs`, `src/main.rs`
- Test: `tests/draw.rs`, `src/app/tests.rs`

**Interfaces:** `pub const TOAST_SECS: i64 = 3`; `App::toast(&mut self, text: impl Into<String>)`; `App::sticky_status(&mut self, text: impl Into<String>)`; `App::report_save(&mut self, ok_text: String)`. `status_line: Option<String>` stays public for readers.

- [ ] **Step 1: Failing tests**

```rust
/// U7: a toast replaced the chords for as long as it lived. Now the chords
/// stay left, the toast sits right, and it is gone three seconds later.
#[test]
fn a_toast_sits_right_of_the_chords_and_expires() {
    let mut app = mk();
    app.now_override = Some(time::macros::datetime!(2026-09-07 20:00 UTC));
    app.apply_boards(League::Nfl, vec![g("1", "KC", "TB", true)], false);
    app.tab = Tab::League(League::Nfl);
    key(&mut app, crossterm::event::KeyCode::Char(' '));
    let footer = buf_text(&render(&mut app, 120, 24)).lines().last().unwrap().to_string();
    assert!(footer.contains("q quit"), "chords stay: {footer:?}");
    assert!(footer.trim_end().ends_with("pinned KC@TB"), "toast right-aligned: {footer:?}");
    assert!(!footer.contains("UPD"), "the toast takes the right side while it lives: {footer:?}");
    app.now_override = Some(time::macros::datetime!(2026-09-07 20:00:02 UTC));
    app.advance_tick();
    assert!(app.status_line.is_some(), "two seconds: still up");
    app.now_override = Some(time::macros::datetime!(2026-09-07 20:00:03 UTC));
    app.advance_tick();
    assert!(app.status_line.is_none(), "TOAST_SECS reached");
    let footer = buf_text(&render(&mut app, 120, 24)).lines().last().unwrap().to_string();
    assert!(footer.contains("UPD"), "the right side is back: {footer:?}");
}

#[test]
fn an_error_status_never_expires_on_its_own() {
    let mut app = mk();
    app.now_override = Some(time::macros::datetime!(2026-09-07 20:00 UTC));
    app.sticky_status("not saving: config.toml line 3: expected `=`");
    app.now_override = Some(time::macros::datetime!(2026-09-07 20:05 UTC));
    app.advance_tick();
    assert!(app.status_line.is_some(), "sticky");
    // A toast after it expires as usual.
    app.toast("pinned KC@TB");
    app.now_override = Some(time::macros::datetime!(2026-09-07 20:05:03 UTC));
    app.advance_tick();
    assert!(app.status_line.is_none());
}
```

- [ ] **Step 2: Implement** in `src/app/mod.rs`:

```rust
/// How long a footer toast (`pinned KC@TB`, `sort time`) keeps the right
/// side before the position readout has it back. Wall clock, not ticks:
/// the idle loop ticks once a second, so thirty ticks would be thirty
/// seconds on a quiet evening, which is exactly when pins happen.
pub const TOAST_SECS: i64 = 3;
```

fields: `pub status_line: Option<String>` (unchanged) and `status_toasted_at: Option<OffsetDateTime>` (doc: "when `status_line` was last set by `toast`; `None` while it is sticky or empty"). Methods:

```rust
    /// A confirmation that expires after [`TOAST_SECS`].
    pub fn toast(&mut self, text: impl Into<String>) {
        self.status_line = Some(text.into());
        self.status_toasted_at = Some(self.now());
    }

    /// A status that stays until the next keypress clears it: config parse
    /// errors, save refusals, a theme that was not found, a prompt error.
    pub fn sticky_status(&mut self, text: impl Into<String>) {
        self.status_line = Some(text.into());
        self.status_toasted_at = None;
    }

    /// The one line a persisting keypress leaves. `persist_*` writes its own
    /// refusal into `status_line` when it cannot save; that refusal wins
    /// (sticky — it names a broken config), else `ok_text` is a toast.
    pub(crate) fn report_save(&mut self, ok_text: String) {
        let save_error = self.status_line.take();
        match (&self.config_error, save_error) {
            (Some(_), _) => self.sticky_status(format!("{ok_text} · not saving (config error)")),
            (None, Some(err)) => self.sticky_status(err),
            (None, None) => self.toast(ok_text),
        }
    }
```

`advance_tick`: `if let Some(at) = self.status_toasted_at { if self.now() - at >= time::Duration::seconds(TOAST_SECS) { self.status_line = None; self.status_toasted_at = None; } }`.

Writers: `persist.rs` (all four) and `merge.rs`'s offline line → `sticky_status`; `main.rs` line 321 → `if let Some(note) = theme_note { app.sticky_status(note); }`; `keys/board.rs` pin/unpin/favorited/unfavorited → `toast`, and `cycle_sort`'s five-line dance → `self.status_line = None; self.persist_config(); self.report_save(format!("sort {}", …));` (the same replacement in `input.rs`'s `Cmd::Sort`); `keys/config.rs` confirmations → `toast`, its "not found"/"already" messages → `sticky_status`; `input.rs` command parse errors, `Cmd::Theme(Some)` error, `go_league`'s not-enabled line and `pin_team`'s miss → `sticky_status`; `pin_team`'s `pinned …` → `toast`. Every `= None` clear stays. Receipt for the report: `grep -rn 'status_line = Some' src` returns only the two setters.

Footer (`chrome.rs`): delete the early-return `status` branch. The status's width joins the shed budget the way `filter_width` does (`status_width = status.map_or(0, |s| s.chars().count() + 2)`), for both the Board legend and the chord list. After the left side is built: if `status_line` is `Some`, `right = vec![status]` (it replaces LEADS/GAME/UPD while it lives); else the existing list. If `left_len + status_len + 2 > width` even after shedding, render the status alone on the row (the old behavior — a long error stays readable on a narrow terminal). The status span keeps its current color.

- [ ] **Step 3: Suite** (the existing `status_line_renders_verbatim_in_the_footer_row`, `the_theme_note_is_in_the_footer_at_startup`, `footer_status_takes_the_clocks_discipline`, `pinned_and_favorited_tiles_carry_a_glyph_in_the_title`, and `src/input.rs`/`src/app/tests.rs`/`tests/config.rs` status assertions must still pass; they read `status_line`, which is unchanged), clippy, fmt, commit `feat(footer): toasts sit right of the chords and expire; errors stay`.

### Task 6: An empty day names the next game (U6)

**Files:**
- Modify: `src/board/mod.rs` (`draw_empty_state`'s `Tab::League(_) if empty` arm), `src/app/derive.rs` (new `empty_league_line`)
- Test: `tests/draw.rs`

**Interfaces:** `pub(crate) fn App::empty_league_line(&self, league: League) -> String`.

- [ ] **Step 1: Failing test**

```rust
/// U6: a traveled day with no games was a blank screen and a lone "next
/// kickoff". It names the day and the next game the app knows about.
#[test]
fn an_empty_day_names_the_next_game_or_says_nothing_is_scheduled() {
    use crossterm::event::KeyCode;
    let mut app = mk();
    let now = time::macros::datetime!(2026-09-07 12:00 UTC); // a Monday
    app.now_override = Some(now);
    let mut next = g("n1", "NYY", "BOS", false);
    next.league = League::Mlb;
    next.start = Some(time::macros::datetime!(2026-09-08 19:10 UTC));
    app.apply_boards(League::Mlb, vec![next], false);
    app.tab = Tab::League(League::Mlb);
    key(&mut app, KeyCode::Char('['));
    let date = app.viewed_date(League::Mlb).expect("traveled");
    app.merge_dated_board(League::Mlb, date, vec![]);
    let s = buf_text(&render(&mut app, 120, 24));
    assert!(s.contains("No MLB games SUN SEP 6 · next TUE 7:10 PM NYY @ BOS"), "{s}");
    // Centered: the line sits in the body's vertical middle, not at the top.
    let y = s.lines().position(|l| l.contains("No MLB games")).unwrap();
    assert!((8..=14).contains(&y), "centered in a 24-row frame, got row {y}:\n{s}");

    let mut app = mk();
    app.now_override = Some(now);
    app.apply_boards(League::Mlb, vec![], false);
    app.tab = Tab::League(League::Mlb);
    let s = buf_text(&render(&mut app, 120, 24));
    assert!(s.contains("No MLB games MON SEP 7 · nothing scheduled"), "{s}");
    assert!(!s.contains("next kickoff"), "{s}");
}
```

- [ ] **Step 2: Implement** in `derive.rs`:

```rust
    /// The empty board's one line for a league tab: the day it is showing
    /// and the first game after that day the app knows about — today's
    /// board plus every slate `[`/`]` fetched — or `nothing scheduled`
    /// when none is loaded (the claim covers the loaded window; `]` fetches
    /// the next day on demand).
    pub(crate) fn empty_league_line(&self, league: League) -> String {
        let now = self.now();
        let date = self.viewed_date(league).unwrap_or(now.date());
        let next = self
            .boards
            .get(&league)
            .into_iter()
            .flatten()
            .chain(
                self.dated_boards
                    .iter()
                    .filter(|((l, _), _)| *l == league)
                    .flat_map(|(_, games)| games.iter()),
            )
            .filter(|g| g.status == Status::Pre)
            .filter_map(|g| g.start.map(|s| (s.to_offset(now.offset()), g)))
            .filter(|(s, _)| s.date() > date)
            .min_by_key(|(s, _)| *s);
        let head = format!("No {} games {}", league.slug().to_uppercase(), super::date_label(date));
        match next {
            Some((start, g)) => format!(
                "{head} · next {} {} @ {}",
                crate::text::fmt_start(start, now),
                g.away.abbr,
                g.home.abbr
            ),
            None => format!("{head} · nothing scheduled"),
        }
    }
```

`board/mod.rs`: the arm becomes `Tab::League(league) if empty => { frame.render_widget(Paragraph::new(app.empty_league_line(league)).style(…muted…).alignment(Alignment::Center), center_one_line(area)); true }` with `center_one_line` beside `center_two_lines` (height 1 at the vertical middle). `fmt_start` lives in `text.rs` and is already `pub`; `date_label` is `pub` in `app/mod.rs`.

- [ ] **Step 3: Suite, clippy, fmt, commit** `feat(board): an empty day names the next game`.

### Task 7: Paging (U1)

**Files:**
- Modify: `src/keymap.rs` (`Group::Paging`, two bindings), `src/app/mod.rs` (`page_rows`, `PAGE_ROWS_FALLBACK`), `src/app/keys/mod.rs` (paging dispatch), `src/app/keys/board.rs` (`page_selected`), `src/app/keys/feed.rs` + `keys/standings.rs` (drop their PgDn/PgUp arms and `FEED_PAGE_JUMP`), `src/board/mod.rs` (record `page_rows`), `src/views/plays_feed.rs` (`&mut App`, record), `src/views/standings.rs` (record), `src/views/zoom.rs` (record on PLAYS/STATS)
- Test: `tests/draw.rs` (replace `paging_keys_are_dead_and_not_advertised`)

**Interfaces:** `pub page_rows: Option<usize>` on `App`; `pub const PAGE_ROWS_FALLBACK: usize = 20`; `enum Paging { By(isize) }` private to `keys/mod.rs` with `const FAR: isize = isize::MAX / 4`; `App::page_selected(&mut self, delta: isize)` (clamps, never wraps); `Group::Paging` with title `paging`.

- [ ] **Step 1: Failing test** (delete `paging_keys_are_dead_and_not_advertised`):

```rust
/// U1: `G`, `g`, PgDn did nothing across 189 games. Half a page is half
/// of what the last frame showed; the ends are the ends; nothing wraps.
#[test]
fn paging_keys_move_half_a_page_and_are_in_the_overlay() {
    use crossterm::event::{KeyCode, KeyModifiers};
    let mut app = board_app(40, 0, 0);
    app.tab = Tab::League(League::Nfl);
    let _ = render(&mut app, 120, 40);
    let shown = app.page_rows.expect("the draw records what it showed");
    assert!((8..40).contains(&shown), "a 40-row frame shows part of 40 games: {shown}");
    key(&mut app, KeyCode::PageDown);
    assert_eq!(app.selected, shown / 2, "half of the last frame");
    app.on_key(KeyCode::Char('d'), KeyModifiers::CONTROL);
    assert_eq!(app.selected, shown / 2 * 2, "ctrl-d is the same half page");
    key(&mut app, KeyCode::Char('G'));
    assert_eq!(app.selected, 39, "G is the end");
    key(&mut app, KeyCode::PageDown);
    assert_eq!(app.selected, 39, "no wrap at the end");
    let s = buf_text(&render(&mut app, 120, 40));
    assert!(s.contains("GAME 40/40"), "the window followed the caret:\n{s}");
    key(&mut app, KeyCode::Char('g'));
    assert_eq!(app.selected, 0, "g is the top");
    key(&mut app, KeyCode::PageUp);
    assert_eq!(app.selected, 0, "no wrap at the top");
    key(&mut app, KeyCode::End);
    assert_eq!(app.selected, 39);
    key(&mut app, KeyCode::Home);
    assert_eq!(app.selected, 0);

    // The feed pages by the rows its pane showed.
    let mut app = mk();
    let games: Vec<Game> = (0..30).map(|i| with_scoring({ let mut x = g(&format!("s{i}"), "KC", "TB", true); x.last_plays[0].scoring = true; x })).collect();
    app.apply_boards(League::Nfl, games, false);
    app.view = gameday::views::View::PlaysFeed;
    let _ = render(&mut app, 120, 20);
    let shown = app.page_rows.expect("feed records its pane");
    key(&mut app, KeyCode::PageDown);
    assert_eq!(app.feed_scroll, shown / 2);
    key(&mut app, KeyCode::Char('G'));
    assert_eq!(app.feed_scroll, 29);

    // Advertised.
    let mut app = mk();
    app.help_open = true;
    let s = buf_text(&render(&mut app, 120, 40));
    for needle in ["paging", "pgdn/pgup", "ctrl-d/ctrl-u", "half page", "g/shift-g", "home/end", "top/bottom"] {
        assert!(s.contains(needle), "help overlay missing {needle:?}:\n{s}");
    }
}
```

- [ ] **Step 2: Implement.** `keymap.rs`: add `Paging` to `Group` and `ALL` (title `"PAGING"`), and

```rust
    Binding {
        // Half of what the last frame showed, in every scrolling view.
        keys: &["PGDN/PGUP", "CTRL-D/CTRL-U"],
        label: "HALF PAGE",
        group: Group::Paging,
        footer: FooterSlot::Never,
    },
    Binding {
        keys: &["G/SHIFT-G", "HOME/END"],
        label: "TOP/BOTTOM",
        group: Group::Paging,
        footer: FooterSlot::Never,
    },
```

`app/mod.rs`:

```rust
/// Rows of the scrolling list the last frame showed — games on the board,
/// lines in a feed, the standings pane — recorded by whichever view drew,
/// like hit zones. Half of it is a page. `None` before the first draw.
pub page_rows: Option<usize>,

/// A page before any frame has been drawn: key handling cannot see the
/// pane, so the first PgDn after launch guesses the 120x36 default's body.
pub const PAGE_ROWS_FALLBACK: usize = 20;
```

`keys/mod.rs`, after the help-modal check and before the view match:

```rust
        // Paging is one gesture in every scrolling view: half of what the
        // last frame showed, never a wrap; the ends are the ends. Ctrl-d/u
        // are the vim spellings. The config editor is not a list (and its
        // favorite prompt types g's), TV and the picker have nothing to page.
        let pageable = matches!(
            self.view,
            View::Board | View::Zoom { .. } | View::PlaysFeed | View::Standings(_)
        );
        if pageable {
            let ctrl = mods.contains(KeyModifiers::CONTROL);
            let half = (self.page_rows.unwrap_or(PAGE_ROWS_FALLBACK).max(2) / 2) as isize;
            let delta = match (code, ctrl) {
                (KeyCode::PageDown, _) | (KeyCode::Char('d'), true) => Some(half),
                (KeyCode::PageUp, _) | (KeyCode::Char('u'), true) => Some(-half),
                (KeyCode::Home, _) | (KeyCode::Char('g'), false) => Some(-FAR),
                (KeyCode::End, _) | (KeyCode::Char('G'), false) => Some(FAR),
                _ => None,
            };
            if let Some(delta) = delta {
                match self.view {
                    View::Board => self.page_selected(delta),
                    View::Zoom { .. } => self.move_zoom_scroll(delta),
                    View::PlaysFeed => self.move_feed_scroll(delta),
                    View::Standings(_) => self.move_standings_scroll(delta),
                    _ => {}
                }
                return;
            }
        }
```

with `const FAR: isize = isize::MAX / 4;` ("far enough that every mover clamps to its end, small enough that `scroll as isize + FAR` cannot overflow"). `keys/board.rs`: `pub(in crate::app) fn page_selected(&mut self, delta: isize) { let n = self.selection_len(); self.selected = if n == 0 { 0 } else { (self.selected as isize + delta).clamp(0, n as isize - 1) as usize }; }`. Delete the `PageDown`/`PageUp` arms and `FEED_PAGE_JUMP` from `keys/feed.rs` and `keys/standings.rs`. Recording: `board::draw` sets `app.page_rows = Some(shown)` where `board_walk` also returns `drawn.len()`; `plays_feed::draw` takes `&mut App` and sets `Some(visible)` after its `derived()` borrow ends; `standings::draw` sets the pane's row count beside `standings_max_scroll`; `zoom::draw` sets the list's visible rows on the PLAYS and STATS tabs and leaves `page_rows` alone on OVERVIEW. The existing `every_handled_board_key_is_a_binding` test passes because `G` is in `"G/SHIFT-G"`.

- [ ] **Step 3: Suite, clippy, fmt, commit** `feat(keys): paging — half a page, the ends, in every scrolling view`.

### Task 8: Help by mode, the legend, `:help` (U3, U5, U9)

**Files:**
- Modify: `src/keymap.rs` (`Group`, `help_order`), `src/app/chrome.rs` (`draw_help`), `src/command.rs` (`help`), `src/input.rs` (`Cmd::Help`)
- Test: `tests/draw.rs` (the two overlay tests updated + one new), `src/command.rs` tests

**Interfaces:** `pub enum Group { Board, Zoom, Tv, Config, Feed, Paging, Everywhere }` with `ALL` in that order and titles `board`/`zoom`/`tv`/`config`/`standings & feed`/`paging`/`everywhere` (caps in `title()`, lowered at render like today); `pub fn help_order(view: &View) -> Vec<Group>`; `Cmd::Help`.

- [ ] **Step 1: Failing tests.** Update `help_overlay_lists_every_group_and_the_hidden_chords` and `help_panel_speaks_the_lowercase_grammar_not_just_the_footer` to the new titles (`board`, `zoom`, `tv`, `config`, `standings & feed`, `paging`, `everywhere`; the leftover-caps list gains `BOARD`, `EVERYWHERE`). Add:

```rust
/// U3: six modes in one column, `h/l` twice with two meanings. The overlay
/// is sectioned by mode, the current mode first, and closes with the glyphs.
#[test]
fn help_leads_with_the_current_mode_and_closes_with_the_legend() {
    use gameday::views::{View, ZoomTab};
    let mut app = mk();
    app.apply_boards(League::Nfl, vec![g("1", "KC", "TB", true)], false);
    app.tab = Tab::League(League::Nfl);
    app.view = View::Zoom { game_id: "1".into(), tab: ZoomTab::Overview };
    app.help_open = true;
    let s = buf_text(&render(&mut app, 120, 50));
    let at = |needle: &str| s.lines().position(|l| l.trim_start().starts_with(needle)).unwrap_or_else(|| panic!("{needle:?} missing:\n{s}"));
    assert!(at("zoom") < at("board"), "the current mode leads:\n{s}");
    assert!(at("board") < at("everywhere") && at("paging") < at("everywhere"), "{s}");
    // tabs (h/l) is a zoom row; cycle (h/l) is a config row; league is everywhere.
    let row = |label: &str| s.lines().position(|l| l.contains(label)).unwrap_or_else(|| panic!("{label:?} missing:\n{s}"));
    assert!((at("zoom")..at("board")).contains(&row("tabs")), "{s}");
    assert!(row("cycle") > at("config") && row("cycle") < at("standings & feed"), "{s}");
    assert!(s.contains("▸ selected · ⚑ pinned · ★ favorite · ▌ hot · ↑n moved up"), "legend:\n{s}");
    // The board leads from the board.
    app.view = View::Board;
    let s = buf_text(&render(&mut app, 120, 50));
    assert!(at_in(&s, "board") < at_in(&s, "zoom"), "{s}");
}

fn at_in(s: &str, needle: &str) -> usize {
    s.lines().position(|l| l.trim_start().starts_with(needle)).unwrap_or_else(|| panic!("{needle:?} missing:\n{s}"))
}

/// U5: `:help` is a command.
#[test]
fn colon_help_opens_the_overlay() {
    use crossterm::event::KeyCode;
    let mut app = mk();
    key(&mut app, KeyCode::Char(':'));
    type_text(&mut app, "help");
    key(&mut app, KeyCode::Enter);
    assert!(app.help_open);
    assert_eq!(gameday::command::parse("help").unwrap(), gameday::command::Cmd::Help);
}
```

- [ ] **Step 2: Implement.** Regroup every binding: `DATE`, `PIN`, `FAVORITE`, `ZOOM`, `SORT`, `TV`, `THEME`, `FILTER` → `Board`; `TABS` → `Zoom`; `TOGGLE`, `EDIT`, `CYCLE` → `Config`; `TV LOCK`, `TV NEXT` → `Tv`; `LEAGUE`, `MOVE`, `BACK`, `CMD`, `REFRESH`, `HELP`, `QUIT` → `Everywhere`; the two paging rows stay `Paging`. `Feed` holds no binding of its own (its keys are the everywhere set plus paging); `help_order` still lists it so the overlay says so:

```rust
/// The overlay's section order for a view: the mode you are in first, the
/// other modes in [`Group::ALL`] order, then paging and the everywhere set.
pub fn help_order(view: &crate::views::View) -> Vec<Group> {
    use crate::views::View;
    let current = match view {
        View::Board | View::ThemePicker => Group::Board,
        View::Zoom { .. } => Group::Zoom,
        View::Tv => Group::Tv,
        View::ConfigView => Group::Config,
        View::PlaysFeed | View::Standings(_) => Group::Feed,
    };
    let mut order = vec![current];
    order.extend(Group::ALL.iter().copied().filter(|g| *g != current));
    order
}
```

`draw_help` iterates `keymap::help_order(&self.view)`; a group with no rows prints its title and one dim row `  (the everywhere keys, plus paging)` so `standings & feed` is not an empty heading; after the groups, a blank line and the legend `▸ selected · ⚑ pinned · ★ favorite · ▌ hot · ↑n moved up` in `th.muted`, then the existing close line. Panel width becomes `lines.iter().map(|l| l.width()).max().unwrap_or(0) as u16 + 4`, capped at `area.width - 4`; height as today. `command.rs`: `("help", ArgSpec::None)` in `REGISTRY` before `q`; `"help" => Cmd::Help` in the `ArgSpec::None` arm; `Cmd::Help` variant. `input.rs`: `Cmd::Help => app.help_open = true`. `every_binding_reaches_the_help_overlay` keeps passing (it sums over `Group::ALL`).

- [ ] **Step 3: Suite, clippy, fmt, commit** `feat(help): sections by mode, the current one first, a glyph legend, :help`.

### Task 9: Receipts

**Files:** `docs/superpowers/specs/2026-09-06-gameday-v4-ship-design.md` (§9), `CHANGELOG.md`

- [ ] **Step 1:** Append under `## §9 Verification`, after the wave 2 block, `### Wave 3 — landed <date>` with: suite 587 → n; the grid's derived numbers (`AWAY_ABBR_X 5 … TEXT_X 43 · ODDS_X 51`, was `4 … 38`) and the four teeth by test name; the zoom split rule and the three heights it was proven at; the config row; the U9 answer (the `▌` was the lookalike swatch, removed; the legend names the hot mark); the toast rule (wall clock, `TOAST_SECS = 3`, which statuses are sticky); the empty-day line's two forms; paging (`page_rows` recorded by the view that drew; `PAGE_ROWS_FALLBACK = 20` before the first frame; zoom's tabs included); the help order; `:help`; the filter grammar; and the paging capture receipt the controller hands over (the live slate's game count, the tmux captures at top/middle/end). `CHANGELOG.md`: `### Added` — paging keys, `:help`, the filter's grammar and count, the empty-day line; `### Changed` — toasts sit right of the chords and expire, `c` opens the theme picker, the help overlay is sectioned by mode; `### Fixed` — L1–L6 and U8/U9 in user words (a start a week out is no longer clipped; `NETFLIX` and the odds; a nudge and a four-letter code; the STATS leaders column; the zoom overview's blank band; the config editor's placement; a section rule over nothing; the stray block on the hero record line).

- [ ] **Step 2: Commit** `docs(v4): wave 3 receipts`.

## Self-review against spec §5

§5.1 paging → Task 7; §5.2 filter → Task 4; §5.3 help sections, legend, `:help`, the `▌` answer → Tasks 8 and 3; §5.4 `c` → Task 3; §5.5 empty dated line → Task 6, toasts → Task 5, orphan rule → Task 3; §5.6 grid → Task 1, zoom fill → Task 2, config → Task 2; §5.7 tests → every task carries its draw test; the DoD's paged slate → the controller's tmux receipt in Task 9. Names: `ABBR_W` (Task 1) consumed by Tasks 1's stats/standings edits; `page_rows` (Task 7) recorded by the views Task 7 edits; `Group::Paging` (Task 7) reordered by `help_order` (Task 8); `toast`/`sticky_status` (Task 5) used by nothing in later tasks. Test-count ledger: 587 → 591 (T1) → 594 (T2) → 597 (T3) → 601 (T4) → 603 (T5) → 604 (T6) → 604 (T7 replaces one) → 606 (T8); measured numbers win.
