# gameday v3.3 Polish Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make every gameday frame broadcast-grade — readable digits at every size, one visual grammar on every screen, TV/cut/tier rows matching the A′ family — with all taste calls decided by Walter from rendered frames in two sittings.

**Architecture:** No new subsystems. Polish waves over the v3.2 board (src/board/*), views (src/views/*), chrome (src/app/chrome.rs, src/keymap.rs), and themes (src/theme.rs). Render-gated items are built as real code plus labeled gate frames under `out/gate/`; two controller-run sittings collect Walter's picks and the apply steps land or revert them.

**Tech Stack:** Rust 2021, ratatui 0.29, crossterm 0.28, tui-big-text 0.7, the v3.2 dump/PNG pipeline (src/dump.rs + batched Chrome rendering).

**Spec:** docs/superpowers/specs/2026-09-03-gameday-v3-3-polish-design.md (binding; §9 decision B recorded).

## Global Constraints

- `cargo clippy --all-targets`: ZERO warnings, every task.
- Full `cargo test` green at DEFAULT parallelism, every task.
- TDD: failing test first; the red run is recorded in the task report.
- Cell-level assertions (v3.2 ruling R22): tests assert buffer cells (char + fg/bg/modifier), not `contains()` over a joined buffer, except where a string's presence is itself the property. Outlined test bodies naming exact cell properties are acceptable plan form (R22 precedent); an implementer writing weaker assertions is a review finding.
- Hard rules carried from v3.2, violating any is a Critical: `hero::score_block` is the ONLY score formatter (cut/TV/zoom call it); logos never move a digit; no reorder without a real event (R24 fingerprint gate); digits charged before garnish (R29); shrink keep-order fragment → meter → play (R30); hero bracket boundaries unchanged (R28/R32 — internal row budgets may shift only where §1b's chosen digit form demands).
- Numeric constants carry a receipt comment (measured source, or "guess because X").
- Copy rules (spec §7): scoring words lose the `!` family-wide; ALL-CAPS survivors are section rules, scoring words, team abbrs, league tags — nothing else.
- No provider/data-mapping changes (sub-project 4's territory).
- Render-gate frames go to `out/gate/<task>-<option>.png`, captioned by filename; gates are decided ONLY at the two sittings (Tasks 12 and 15) — implementers never self-certify a gated look.
- End every commit message with:
  `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>`

---

### Task 1: Glyph portability spike (spec §1a)

**Files:**
- Create: `docs/research/v3-polish/glyph-matrix.md` (+ captured screenshots in the same dir)
- Create: `scripts/glyph-probe.sh` (prints the probe block below)
- No src changes.

**Interfaces:** Produces a DECISION recorded in the ledger and in glyph-matrix.md: `sextant-default` or `quadrant-default`. Task 12's apply step consumes it.

- [ ] **Step 1: Build the probe.** `scripts/glyph-probe.sh` prints three labeled lines: (a) sextants `🬀🬁🬂🬃🬄🬅🬆🬇 🬐🬑🬒 🬭🬮🬯` (U+1FB00–1FB3B sample), (b) quadrants/half-blocks `▀▁▂▃▄▅▆▇█ ▖▗▘▙▚▛▜▝▞▟` (U+2580–259F), (c) a real captured digit row from `out/board-broadcast.txt` (grep a line containing U+1FB glyphs; the ANSI dumps are in out/). Also print the terminal's `$TERM_PROGRAM`/`$TERM`.
- [ ] **Step 2: Capture the matrix.** Run the probe and screenshot in every terminal reachable on this Mac — check `ls /Applications | grep -iE 'iterm|kitty|wezterm|alacritty|ghostty|warp'` plus Terminal.app (always present). For each terminal: default font, plus JetBrains Mono and SF Mono if installed (`fc-list | grep -iE 'jetbrains|sf mono'` or `ls ~/Library/Fonts /Library/Fonts`). Screenshot via `screencapture -l <windowid>` (get window id with `osascript -e 'tell app "<Terminal>" to id of window 1'`) or a plain `screencapture -i` note asking the controller. Tofu = boxes/blanks where line (a) should show mosaic blocks.
- [ ] **Step 3: Write `glyph-matrix.md`:** one row per (terminal, font): sextants OK?, quadrants OK?, screenshot filename. Then the decision per spec §1a: tofu on a mainstream default setup (Terminal.app default font counts as mainstream) → `quadrant-default`; clean everywhere → `sextant-default`. State the rule applied and the winner in one bolded line.
- [ ] **Step 4: Commit.**

```bash
git add docs/research/v3-polish scripts/glyph-probe.sh
git commit -m "docs(v3.3): glyph portability matrix — sextant vs quadrant coverage receipt"
```

---

### Task 2: Board certain fixes — bare headers, mark column, abbr padding (spec §4)

**Files:**
- Modify: `src/board/mod.rs` (section rendering), `src/board/rows.rs` (mark column, abbr cell)
- Test: `src/board/rows.rs` tests, `tests/draw.rs`

**Interfaces:** Consumes `rows::{draw_tier1,draw_tier2,draw_tier3,RowCtx,GUTTER}`, the board walk in `board::draw`. Produces no new symbols — behavior only.

- [ ] **Step 1: Failing tests.**

```rust
// tests/draw.rs
#[test] fn a_section_with_no_rows_renders_no_header() {
    // Board with live games but zero `later`: no cell row contains "LATER";
    // repeat for finals=[] → no "FINAL". Cell-scan the buffer rows.
}
// src/board/rows.rs tests
#[test] fn the_mark_column_is_column_zero_in_every_tier() {
    // Render tier1, tier2, tier3 for the same hot game into 3 buffers:
    // the ▌/· mark glyph sits at x==0 in all three; tier1's mark is not
    // shifted by its taller layout (assert char+fg at (0,0)).
}
#[test] fn two_char_abbrs_occupy_the_three_char_cell() {
    // Tier2 for KC (2 chars) and BUF (3 chars): the score digit column x
    // is identical for both rows (find the score cell by fg==roles.digits;
    // assert same x). KC's cell pads right with a space.
}
```

- [ ] **Step 2: Run to verify failure** — `cargo test --lib board::rows` + `cargo test --test draw`: the three fail (bare LATER renders today; mark/pad offsets differ).
- [ ] **Step 3: Implement.** In `board::draw`'s section walk: skip rule-line emission when the section's row list is empty (all four sections, one guard each with a `// spec v3.3 §4: no orphan headers` comment). In `rows.rs`: unify mark drawing into one `fn mark_cell(frame, area, ctx, th)` called by all three tiers at x=0; abbr formatting pads to 3 (`format!("{:<3}", abbr)`) in tier2/tier3 and the tier1 nameplate.
- [ ] **Step 4: Full suite + clippy.** Expect prior tests pinning bare headers to need honest updates (comment: `// spec v3.3 §4 retired orphan headers`).
- [ ] **Step 5: Commit.**

```bash
git add src/board tests/draw.rs
git commit -m "fix(v3.3): grid discipline — no orphan section headers, one mark column, padded abbr cells"
```

---

### Task 3: Cut completeness — takeover labels/colors/strip, band row 2 + enter-jump (spec §3)

**Files:**
- Modify: `src/board/cut.rs` (draw_takeover, draw_band), `src/app/mod.rs` + `src/input.rs` (enter-while-band), `src/keymap.rs` (help row only if a row exists for enter/zoom — reuse it)
- Test: `src/board/cut.rs` tests, `tests/draw.rs`

**Interfaces:** Consumes `hero::score_block`, `CutState::{fire,active}`, `Cut{game_id,play,full,until_tick}`, `BAND_TICKS`. Produces: `Cut::remaining_secs(&self, tick: u64) -> u64` (ceil of remaining ticks / LIVE_TICKS_PER_SEC; band row 2 and tests use it).

- [ ] **Step 1: Failing tests.**

```rust
// src/board/cut.rs
#[test] fn the_takeover_names_both_teams_in_their_colors() {
    // draw_takeover for KC/TB: "KC" cell fg == hero_pair away color,
    // "TB" cell fg == hero_pair home color, adjacent to score_block's rect.
}
#[test] fn the_takeover_has_a_dimmed_strip_and_timer() {
    // bottom strip rows use roles.dim on chars; a cell run reads "clears in 3s"
    // at until_tick-CUT_TICKS ticks elapsed 0 (remaining_secs receipt).
}
#[test] fn the_band_second_row_offers_the_jump_and_counts_down() {
    // draw_band row 1 contains "enter jump · clears in 2s" at a tick with
    // 1.5s remaining → "2s" (ceil); dim fg on the affordance text.
}
// tests/draw.rs
#[test] fn enter_during_a_band_zooms_the_bands_game_not_the_selection() {
    // Selection on game A; band active for game B; KeyCode::Enter →
    // app.zoomed == Some(B). No band → Enter zooms selection (existing path).
    // Prompt open (mode != Normal) → Enter does NOT jump.
}
```

- [ ] **Step 2: Run to verify failure.**
- [ ] **Step 3: Implement.** Takeover: team abbr labels flank `score_block`'s rect (computed AFTER the score rect — logos-never-move-a-digit discipline applies to labels too); away label takes `hero_pair`'s away color even when lifted (read `theme::hero_pair` — the bug was falling back to `roles.digits` for away; use the returned pair directly). Dimmed strip: bottom row(s) in `roles.dim` with the timer line right-aligned. `remaining_secs`: `((until_tick - tick) + LIVE_TICKS_PER_SEC - 1) / LIVE_TICKS_PER_SEC` with a receipt comment naming LIVE_TICKS_PER_SEC's value. Band row 2 replaces the repeated play text. Enter-jump in `input.rs`'s Normal-mode Enter arm: `if let Some(cut) = app.cuts.active(app.tick) { zoom cut.game_id } else { existing }`, guarded on `mode == Normal`.
- [ ] **Step 4: Full suite + clippy.** The v3.2 band tests asserting the old row 2 get honest updates with a spec comment.
- [ ] **Step 5: Commit.**

```bash
git add src/board/cut.rs src/app src/input.rs src/keymap.rs tests/draw.rs
git commit -m "feat(v3.3): cut completeness — team labels and colors, dimmed strip with timer, band jump affordance"
```

---

### Task 4: TV cut scope B (spec §9)

**Files:**
- Modify: `src/app/mod.rs` (the `full` computation at the fire sites)
- Test: `tests/draw.rs` (or src/app tests where the v3.2 cut-scope tests live — grep `every_scoring_play_takes_the_screen_in_tv`)

**Interfaces:** Consumes `CutState::fire`, `View::Tv`, `tv_shown`, the my-games membership helper used by `Derived` (grep `is_my_game`). Produces behavior only.

- [ ] **Step 1: Failing test.**

```rust
#[test] fn in_tv_only_the_shown_game_and_my_teams_take_the_screen() {
    // View::Tv showing game A. Delta on A → takeover (full). Delta on
    // pinned/favorited B (not shown) → takeover. Delta on unrelated C →
    // BAND over TV (cut active, full==false, TV body still drawn beneath:
    // assert a TV-only cell like the linescore header survives).
}
```

- [ ] **Step 2: Run to verify failure** — v3.2's `every_scoring_play_takes_the_screen_in_tv` pins the OLD behavior; it will be rewritten by this test (delete it in the same commit with a `// spec v3.3 §9 decision B` comment).
- [ ] **Step 3: Implement.** At the fire site(s): `full = pinned || favorited || (matches!(view, View::Tv) && Some(id) == tv_shown.as_deref())`. The band-over-TV render path: `board::cut::draw_band` is drawn above the TV strip — reuse the board's band slot logic; the TV view reserves its band rows the same way the board does after Task 11 lands (until then, band draws over the strip's top rows — acceptable interim, note it).
- [ ] **Step 4: Full suite + clippy.**
- [ ] **Step 5: Commit.**

```bash
git add src/app tests
git commit -m "feat(v3.3): tv cut scope B — takeover for the shown game and my teams, band for the rest"
```

---

### Task 5: Words and colors — copy pass + gruvbox ground (spec §7, §6)

**Files:**
- Modify: `src/theme.rs` (scoring words, gruvbox), `src/rank.rs` (chip strings), any file grep finds with `!"`-suffixed scoring words or stray caps
- Test: existing scoring-word tests + theme tests

**Interfaces:** Produces the final word set later tasks render: `TOUCHDOWN`, `FIELD GOAL`, `SAFETY`, `HOME RUN`, `RUN SCORES`, `GOAL`, `BUCKET` (no `!` anywhere); chips `TYING RUN 3RD`, `GO-AHEAD 3RD`.

- [ ] **Step 1: Failing tests.**

```rust
#[test] fn no_scoring_word_carries_an_exclamation() {
    // For every league + the sharpened MLB/NFL paths: scoring_word_for_play
    // output contains no '!'.
}
#[test] fn the_tying_chip_uses_the_full_13_char_form() {
    // The situation that produced TYING ON 3RD now yields "TYING RUN 3RD"
    // (exactly 13 chars — receipt: rows::T1_CHIP_W).
}
#[test] fn gruvbox_ground_is_the_published_282828() {
    // roles(gruvbox).ground == Color::Rgb(0x28,0x28,0x28).
}
```

- [ ] **Step 2: Run to verify failure.**
- [ ] **Step 3: Implement.** Strip `!` at the word definitions (not per-call); `RUN SCORES!` → `RUN SCORES`; chip string swap in rank.rs. Gruvbox: ground `#282828`, then re-check the other roles against gruvbox's published palette (fg `#ebdbb2`, yellow `#d79921`, red `#cc241d`, aqua `#689d6a`, gray `#928374`) — adjust any role that drifted, one receipt comment per hex naming the palette. Caps audit: `grep -nE '"[A-Z]{2,}[A-Z !]*"' src/` and demote anything that isn't a section rule, scoring word, abbr, or league tag (expect a handful in views/chrome; each change carries the §7 comment).
- [ ] **Step 4: Full suite + clippy** (word tests across cut/rank update honestly).
- [ ] **Step 5: Commit.**

```bash
git add src tests
git commit -m "fix(v3.3): copy pass — no exclamations, TYING RUN 3RD, gruvbox on the published palette"
```

---

### Task 6: One footer language (spec §5)

**Files:**
- Modify: `src/app/chrome.rs`, `src/keymap.rs`, every view emitting its own caps footer (grep `NAV:` and `[TAB]` in src/views/)
- Test: `tests/draw.rs`

**Interfaces:** Consumes the keymap per-view contexts (Board's lowercase legend from v3.2 Task 9). Produces: every view's footer rendered by ONE chrome function from keymap rows, lowercase grammar.

- [ ] **Step 1: Failing tests.**

```rust
#[test] fn every_view_speaks_the_lowercase_footer() {
    // For each of: plays feed, standings, config, zoom, help, theme picker —
    // render at 120x40; last content row contains no "NAV:" and no "[TAB]";
    // contains at least "q quit"; caps run-length in the footer row ≤ 3
    // (abbrs/league tags excepted by checking the specific cells).
}
#[test] fn footers_shed_in_order_and_help_quit_survive_at_40_cols() {
    // Narrow render of two non-board views: "? help" and "q quit" present.
}
```

- [ ] **Step 2: Run to verify failure.**
- [ ] **Step 3: Implement.** Extend the keymap context enum to cover every view (grep how Board/TV contexts render); delete the per-view caps footer code paths; one `chrome::draw_footer(view, ...)`. FOOTER_DROP_ORDER semantics carry over unchanged.
- [ ] **Step 4: Full suite + clippy** (v3.1-era footer tests update honestly, spec comment each).
- [ ] **Step 5: Commit.**

```bash
git add src tests
git commit -m "feat(v3.3): one footer grammar — every view on the lowercase keymap legend"
```

---

### Task 7: Screen layouts — standings, plays, config, balance (spec §5)

**Files:**
- Modify: `src/views/standings.rs`, `src/views/plays.rs` (or the feed's file — grep `plays-feed` stem for its draw fn), `src/views/config_view.rs`, `src/views/help.rs`, `src/views/theme_picker.rs`, filter prompt draw site
- Test: `tests/draw.rs`

**Interfaces:** Consumes `theme::roles`, footer from Task 6. Produces layout behavior only.

- [ ] **Step 1: Failing tests.**

```rust
#[test] fn standings_use_two_columns_at_width() {
    // 120x40: two conference/league tables side by side (two distinct
    // header cells on one row, x separated by ≥40); 80x24: one column.
}
#[test] fn the_plays_feed_fills_the_width() {
    // 120x40: a play row's text extends past x=80 (today's column ends ~60);
    // stamp column aligned at one x for all rows.
}
#[test] fn no_screen_floats_a_dead_column() {
    // config, help, theme-picker at 120x40: the key bar row is within 2 rows
    // of the last content row (anchored content_end+1, receipt), not at the
    // terminal floor with a >2-row gulf; content block's left margin centers
    // the block (|left_gap - right_gap| ≤ 2 cols).
}
```

- [ ] **Step 2: Run to verify failure.**
- [ ] **Step 3: Implement.** Standings: split by conference/group when the data has one, else halve the table list; ≥100 cols gate (receipt: two 48-col tables + gutter). Plays feed: adopt the board's stamp/text column pair full-width. Config: two-panel (section list | values) at ≥100 cols, single below. Help/picker/filter: measure content height, vertically center, key bar at content_end+1.
- [ ] **Step 4: Full suite + clippy + regenerate the four stems** (`cargo run --release -- dump`; eyeball out/standings.png, plays-feed.png, config.png, help.png — say in the report what changed).
- [ ] **Step 5: Commit.**

```bash
git add src/views tests
git commit -m "feat(v3.3): screens fill their frames — two-column standings, full-width feed, balanced blocks"
```

---

### Task 8: Zoom symmetry and linescore colors (spec §5)

**Files:**
- Modify: `src/views/zoom.rs`, `src/board/linescore.rs`
- Test: `tests/draw.rs`

**Interfaces:** Consumes `hero::draw_hero`, `linescore_lines`, `theme::hero_pair`. Produces: `linescore_lines(game, th) -> Vec<Line>` (signature gains the theme — TV consumes the same colored output in Task 13; update TV's call site in this task, compile-level only).

- [ ] **Step 1: Failing tests.**

```rust
#[test] fn zoom_logo_flanks_are_symmetric_or_absent() {
    // Game with only one committed mark: NO flank cells render on either
    // side (colored non-ground cells beside the digit rect absent).
    // Game with both marks at width ≥100: both flanks present.
}
#[test] fn the_linescore_wears_team_colors() {
    // Zoomed KC/TB: KC linescore row's abbr cell fg == away hero_pair color,
    // TB row == home color; totals column bold.
}
```

- [ ] **Step 2: Run to verify failure.**
- [ ] **Step 3: Implement.** Flank gate: `hero_mark(away).is_some() && hero_mark(home).is_some()` before drawing either (comment: `// spec v3.3 §5: both or neither`). Linescore rows take `hero_pair` colors; the white-digit role miss dies with it.
- [ ] **Step 4: Full suite + clippy; regen + eyeball out/zoom.png.**
- [ ] **Step 5: Commit.**

```bash
git add src/views/zoom.rs src/board/linescore.rs src/views/tv.rs tests
git commit -m "fix(v3.3): zoom — symmetric flanks or none, team-colored linescore"
```

---

### Task 9: Gate prep — mid-size digit candidates (spec §1b)

**Files:**
- Create: `src/tiles/quad_digits.rs` (`pub fn quad_digits(frame, rect, value: u32, color: Color) -> bool` — 4-row-tall digits from U+2580–259F quadrant/half blocks, self-contained glyph table for 0-9, returns fit like `digit_glyphs`)
- Modify: `src/tiles/mod.rs` (`pub mod quad_digits;`), `src/dump.rs` (three gate stems)
- Test: `src/tiles/quad_digits.rs` tests

**Interfaces:** Produces `tiles::quad_digits::quad_digits` (Task 12's apply wires the WINNER into `hero::score_block`'s ladder; nothing else calls it yet) and gate stems `gate-digits-current`, `gate-digits-quad`, `gate-digits-text` rendered at 80×24 (the dump gains a per-stem size override for gate stems — smallest mechanism, e.g. an entry-level `(cols, rows)` option defaulting to DUMP_COLS×DUMP_ROWS).

- [ ] **Step 1: Failing tests.**

```rust
#[test] fn quad_digits_draw_readable_numerals_in_four_rows() {
    // Value 87 into a 4-row rect: glyph cells occupy exactly 4 rows; the
    // per-digit column mask for '8' differs from '7' (cell-level: compare
    // two rendered buffers); fg == the passed color on every glyph cell.
}
#[test] fn quad_digits_refuse_too_small_and_report_it() {
    // 3-row rect → returns false, buffer untouched (all ground).
}
```

- [ ] **Step 2: Run to verify failure** — module absent.
- [ ] **Step 3: Implement.** Glyph table: 10 digits × 4 rows × 3 cols using `▀▄█▌▐▖▗▘▝▚▞ ` (design them; a digit must be unambiguous — 8 vs 0 vs 6 distinct). `gate-digits-text` = the existing bold text fallback styled double-spaced (`2 4`) — no new renderer, a dump-side variant flag into `score_block`'s text arm is NOT allowed (one formatter); instead render the text form by giving score_block a rect too small for glyphs (that IS the ladder's text arm — honest capture).
- [ ] **Step 4: Wire the three gate stems, `cargo run --release -- dump`, confirm out/gate/*.png render. Full suite + clippy.**
- [ ] **Step 5: Commit.**

```bash
git add src/tiles src/dump.rs tests
git commit -m "feat(v3.3): quadrant mid-size digits + gate frames at 80x24"
```

---

### Task 10: Gate prep — tier-1 re-grid (spec §4)

**Files:**
- Modify: `src/board/rows.rs` (draw_tier1)
- Modify: `src/dump.rs` (capture `gate-tier1-after`; `gate-tier1-before` is captured by the CONTROLLER from the pre-task commit before this task merges — note in report, don't fake it)
- Test: `src/board/rows.rs` tests

**Interfaces:** Consumes `RowCtx`, `digit_glyphs`, roles. Produces the re-gridded tier-1: plain bold numerals in `roles.digits` at the SAME score column x as tier-2 rows; sextant mini-digits become optional garnish right of the nameplate when width affords (never the only score).

- [ ] **Step 1: Failing tests.**

```rust
#[test] fn tier1_score_is_readable_text_on_the_shared_grid() {
    // Tier1 and tier2 for the same game: the plain numeral score cell x is
    // identical in both; tier1's numerals are bold roles.digits.
}
#[test] fn tier1_glyph_garnish_never_replaces_the_numerals() {
    // Narrow tier1 (glyphs don't fit): numerals still present; wide: both
    // present, numerals unchanged position.
}
```

- [ ] **Step 2: Run to verify failure.**
- [ ] **Step 3: Implement** (accent bar + bold + indented fragment/play lines keep tier-1's identity — spec §4 wording).
- [ ] **Step 4: Full suite + clippy; regen board-broadcast + gate-tier1-after.**
- [ ] **Step 5: Commit.**

```bash
git add src/board/rows.rs src/dump.rs tests
git commit -m "feat(v3.3): tier-1 rows on the shared grid — readable numerals, glyphs demoted to garnish"
```

---

### Task 11: Gate prep — band geometry (spec §3)

**Files:**
- Modify: `src/board/layout.rs` (reserve 2 band rows when live > 0), `src/board/mod.rs` (quiet-state use of the reserved rows), `src/views/tv.rs` (same reservation — closes Task 4's interim), `src/dump.rs` (stems `gate-band-reserved` quiet frame, `gate-band-fired` active frame, `gate-band-amber` the one-off amber-inside-IN-PLAY variant rendered via a dump-only draw that calls draw_band into the IN PLAY rule's rows with `roles.digits` fill — dump-side composition, no product code path)
- Test: `src/board/layout.rs` + `tests/draw.rs`

**Interfaces:** `TierPlan` gains `band_rows: u16` (0 or 2; consumed by board::draw and tv).

- [ ] **Step 1: Failing tests.**

```rust
#[test] fn the_board_never_jumps_when_a_band_fires() {
    // Live board: render quiet, render with an active band cut — the y of
    // the IN PLAY rule row is IDENTICAL in both buffers.
}
#[test] fn the_reserved_rows_earn_their_keep_when_quiet() {
    // Quiet: the 2 rows hold the top section rule + air, not blank+blank
    // (assert the rule row moved up into the reservation).
}
#[test] fn no_live_games_means_no_reservation() {
    // Finals-only board: band_rows == 0; total row budget unchanged from
    // the Task-5-era sum property (the 490-case sweep must stay green).
}
```

- [ ] **Step 2: Run to verify failure.**
- [ ] **Step 3: Implement.** `plan(...)` charges `band_rows = 2` when live > 0 (receipt: BAND draws 2 rows, spec §3), before tiers (same two-pass discipline as scores_lane). The property sweep and R28/R32 bracket tests update honestly.
- [ ] **Step 4: Full suite + clippy; regen; confirm the three gate stems.**
- [ ] **Step 5: Commit.**

```bash
git add src/board src/views/tv.rs src/dump.rs tests
git commit -m "feat(v3.3): band rows reserved — the board never jumps; gate frames for geometry and placement"
```

---

### Task 12: SITTING 1 — controller checkpoint (spec §8), then apply

**This is NOT a subagent task.** The controller:

- [ ] **Step 1:** Capture `gate-tier1-before` from the pre-Task-10 commit (`git stash`-free: `git worktree add /tmp-gate <pre-commit>` or re-render from that commit in a scratch worktree), place in out/gate/.
- [ ] **Step 2:** Present to Walter as lettered menus with the PNGs opened: (i) mid digits — current / quad / text (Task 9 frames); (ii) tier-1 grid — before / after; (iii) band — reserved / jump, and top-red / amber-in-list; plus Task 1's spike decision if it was `quadrant-default` (confirm the config default flip).
- [ ] **Step 3 (apply, one subagent after decisions):** wire the chosen mid-size form into `score_block`'s ladder (Full → CHOSEN → text) with bracket-internal budget adjustments only; add `glyphs = "sextant" | "quadrant"` config value defaulting per the spike decision (serde default, README line; if quadrant-default: the sextant path stays selectable, and logo art regenerates via the v3.2 chafa pipeline with quadrant symbols ONLY if the spike found logo tofu — bounded per spec §1a); revert Task 10/11 work Walter rejected (`git revert` the task commit, ledgered). Tests follow the decisions.
- [ ] **Step 4:** Record every pick in the ledger as Directions; commit the apply.

```bash
git commit -m "feat(v3.3): sitting-1 decisions applied — digit ladder, grid, band geometry"
```

---

### Task 13: TV overhaul (spec §2)

**Files:**
- Modify: `src/board/hero.rs` (`score_block` doubles glyph rows when the rect affords ≥16 rows — internal, signature unchanged, receipt comment; this serves TV and the takeover on tall frames through the one formatter), `src/views/tv.rs` (balanced stack, two-column strip, hero labels)
- Test: `src/board/hero.rs` + `tests/draw.rs`

**Interfaces:** Consumes Task 8's colored `linescore_lines`, Task 11's `band_rows`. Produces no new symbols.

- [ ] **Step 1: Failing tests.**

```rust
// src/board/hero.rs
#[test] fn the_score_block_doubles_at_jumbotron_heights() {
    // 16-row rect: each glyph row of the 8-row Full form paints two
    // identical buffer rows (compare row pairs cell-by-cell); 10-row rect:
    // single-height unchanged (regression pin).
}
// tests/draw.rs
#[test] fn tv_fills_its_frame() {
    // 120x40, 6 live: ≤2 fully-empty rows in the body (count rows whose
    // cells are all ground); strip is two columns (two "…" game cells on
    // one row with x separation ≥50); hero nameplates present (abbr cells
    // in team colors above the digits).
}
```

- [ ] **Step 2: Run to verify failure.**
- [ ] **Step 3: Implement.** Doubling inside `score_block`'s Full arm (`rect.height >= 16` gate — receipt: 8 glyph rows × 2). TV stack: labels / doubled digits / clock+chip / fragment / meter / linescore / three plays / strip — reuse draw_hero for the top (it already orders these; hand it the tall area) and verify the doubled digits flow through WITHOUT tv.rs touching digit code. Two-column strip at ≥100 cols (receipt), 5 rows max per column.
- [ ] **Step 4: Full suite + clippy; regen out/tv.png; report the empty-row count.**
- [ ] **Step 5: Commit.**

```bash
git add src/board/hero.rs src/views/tv.rs tests
git commit -m "feat(v3.3): tv fills the frame — doubled digits through the one formatter, two-column strip, hero labels"
```

---

### Task 14: Themes — studio press-box + daygame candidate (spec §6)

**Files:**
- Modify: `src/theme.rs` (studio roles rebuild; new `daygame` NOT in BUILTIN_NAMES yet — behind `#[cfg(test)]`-visible constructor plus a dump-only hook), `src/dump.rs` (stems `gate-studio`, `gate-daygame` boards)
- Test: `src/theme.rs` tests, picker test

**Interfaces:** Produces: studio = grayscale roles (ground near-black gray, ink light gray, dim mid gray, digits WHITE, cool gray) + exactly `hot = red` as the only chroma; `daygame` light role set (paper ground `#f5f2ea`-family, ink near-black, digits deep amber `#9a6a00`-family, hot red, cool slate — receipts naming contrast intent ≥ WCAG-ish 4.5:1 for ink-on-ground, checked by a test computing relative luminance).

- [ ] **Step 1: Failing tests.**

```rust
#[test] fn studio_is_monochrome_plus_one_red() {
    // Over studio's roles: every color except hot has saturation ≈ 0
    // (max(r,g,b)-min(r,g,b) ≤ 8); hot is red-dominant.
}
#[test] fn daygame_ink_contrast_clears_4_5_to_1() {
    // Relative-luminance contrast(ink, ground) ≥ 4.5 (helper in the test).
}
#[test] fn broadcast_studio_picker_rows_still_differ() { /* existing test extends to daygame when promoted */ }
```

- [ ] **Step 2: Run to verify failure.**
- [ ] **Step 3: Implement + render both gate stems.**
- [ ] **Step 4: Full suite + clippy.**
- [ ] **Step 5: Commit.**

```bash
git add src/theme.rs src/dump.rs tests
git commit -m "feat(v3.3): studio rebuilt press-box monochrome; daygame light candidate behind the gate"
```

---

### Task 15: SITTING 2 — controller checkpoint, then apply

**NOT a subagent task.** The controller:

- [ ] **Step 1:** Regenerate the full gallery; open for Walter: out/tv.png (confirm §2), gate-studio (earn-or-delete), gate-daygame (in-or-park), plus the whole 22-stem gallery as the family check, plus anything reopened from Sitting 1.
- [ ] **Step 2 (apply, one subagent):** per decisions — promote daygame into BUILTIN_NAMES + picker/README, or park it (delete the dump hook, keep the code path testable? NO — YAGNI: parked means the roles fn stays with its test, no stem, README silent); keep studio or delete it from BUILTIN_NAMES (retired name stays loadable as a user theme like the v3.2 retirements); land any small reopened fixes.
- [ ] **Step 3:** Append `## Decisions` to the spec (every gate: options shown, pick, one-line reasoning), commit.

```bash
git commit -m "docs(v3.3): sitting-2 decisions applied and recorded"
```

---

### Task 16: DoD sweep (spec §11)

**Files:** receipts appended to the spec; README; no new code beyond what receipts demand.

- [ ] **Step 1:** Full `cargo test` (default parallelism) + `cargo clippy --all-targets` — record both lines verbatim.
- [ ] **Step 2:** Spot-run the carried v3.2 receipts: the 42-combo size sweep test, the cut/hero hard-rule cell-identity tests, the R24 gate tests — name each test and its result.
- [ ] **Step 3:** CPU: `--demo` 30 s idle sampled the v3.2 way; compare against 1.19% — record numbers + method; investigate before shipping if above by more than jitter (band reservation + doubling are the suspects).
- [ ] **Step 4:** README: keys (enter-jump note), `glyphs` config value (if landed), themes paragraph matching BUILTIN_NAMES reality, stems list if it changed (gate-* stems are dev-only — exclude from the contract test's public list or fold behind a `--gate` dump flag, whichever the stem test already supports; state which).
- [ ] **Step 5:** Append `## Verification (date)` to the spec; commit.

```bash
git add docs README.md
git commit -m "docs(v3.3): verification receipts — tests, clippy, size/hard-rule spot-runs, CPU"
```

---

## Self-Review

**Spec coverage:** §1a→T1, §1b→T9+T12; §2→T13(+T15 confirm); §3→T3(row 2/jump/labels/strip)+T11(geometry/placement)+T12; §4→T2+T10+T12 (SCORES lane: explicit no-change); §5→T6+T7+T8; §6→T5(gruvbox)+T14+T15; §7→T5; §8→T12+T15 protocol; §9→T4 (decision B); §10 respected (no provider work; only enter-jump added); §11→T16. No gaps.

**Placeholder scan:** clean — every step names exact files, values, or the grep that finds them; test outlines follow the R22-precedent form with named cell properties.

**Type consistency:** `quad_digits(frame, rect, value: u32, color) -> bool` (T9) matches `digit_glyphs`'s shape for T12's ladder wiring; `linescore_lines(game, th)` change lands in T8 and is consumed in T13; `TierPlan.band_rows: u16` lands in T11, consumed T11/T13; `Cut::remaining_secs(&self, tick: u64) -> u64` lands and is consumed in T3. Sitting tasks (12, 15) are controller checkpoints — the subagent-driven executor must not dispatch them as implementer tasks.
