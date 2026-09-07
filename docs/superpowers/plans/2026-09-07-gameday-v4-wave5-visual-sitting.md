# gameday v4 Wave 5 — Visual Sitting Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** One render sitting. Every visual question the review left open (college marks, tinted row abbreviations, the play-text clause cap, the wide-terminal tier row, the zoom fill at 40 and 60 rows, the README GIF's cut) is built for real, rendered as labeled PNGs on the review-day slate, and put in front of Walter as a lettered menu. Winners ship; losers are reverted in the same session; every answer is recorded as a direction.

**Architecture:** The options are code behind design-time knobs that only `gameday frame --opt` can set (`App.design_opts`, carried to the row drawers through `RowCtx`), so one binary renders every variant and the contact sheet is one command; after the sitting the knobs are deleted, the winning arm becomes unconditional, the losing arm is removed. A `review-slate` frame scenario loads `fixtures/review-slate-2026-09-05.json` through the real mapper so the marks and tint questions are answered on a real Saturday, not the demo. The college-marks option is an asset commit on a side branch (`tools/gen-logos.sh` in a new all-FBS mode), rendered against the same scenario. The GIF is a `vhs` tape with two cuts; recording it needs `vhs`, which this machine does not have — an owner action.

**Tech Stack:** Rust 2021; `gameday frame` + `tools/contact-sheet.sh` (ImageMagick present); headless Chrome (present) for PNGs; `chafa` + `jq` (present) for marks; `vhs` (absent — Brewfile addition is Walter's).

**Spec:** `docs/superpowers/specs/2026-09-06-gameday-v4-ship-design.md` §7 (Wave 5), §1 V1–V4, L5, L7; `docs/design-loop.md` (the render gate).

## Global Constraints

- Branch `v4-wave5` from `main` at `9572275`; the marks option on `v4-wave5-marks` (from `v4-wave5` after Task 1). Commit after every task with the trailer; never push.
- Suite green after every task with the count reported (643 at the start; measured numbers win); `cargo clippy --all-targets -- -D warnings` and `cargo fmt --check` clean before every commit. No `#[allow(...)]`.
- The gallery does not change during the sitting: `gameday dump`'s 22 stems render exactly as before Task 1 until a winner ships (the knobs default off; the demo slate is untouched — the review-slate scenario is `frame`-only).
- Every option is rendered from the same commit with the same command shape, labeled by filename: `out/design/<item>-<option>.png`. No option is described in prose to Walter.
- Numbers carry receipts: `PLAY_X`'s field width, the clause cap's minimum, the marks set's size.
- Rulings made while planning:
  - **The demo slate stays as it is.** The wave 3 carry ("a CFB demo game with a probability and a demo favorite so the LEADS labels render") is met by the review-slate scenario (real CFB win probabilities, a scenario-only favorite `Cfb ORE`), not by changing `demo.rs` — a demo change would move every gallery capture and every dump test for a frame only the sitting needs.
  - **Knobs, not branches, for the row-level options.** V2/V3/L7 are one-line arms in `rows.rs`; a branch per option would mean three binaries for one contact sheet. The knobs are `frame`-only, live for exactly one wave, and Task 5 deletes them.
  - **The marks option is a branch**, because it is 300 asset files and an embed table, and "revert the loser" must be a branch delete, not a 300-file revert commit.
  - **The clause cap applies to the tier-1 last-play row only** (V3 names "tier rows"); the zoom feed and the plays feed keep full sentences — they have the room.
  - **The wide tier's play cell** starts at `PLAY_X = TEXT_X + FRAGMENT_W` with `FRAGMENT_W = 40`: the longest situation fragment on the review slate is 28 cells (`BAY 1ST & 10 AT BAY 25` shape), so 40 holds every fragment with air; `WIDE_TIER_MIN = 160` columns is the spec's number.
  - **The clause cap's minimum is 48 cells** (the spec's number): a clause shorter than that is not worth cutting.
  - **`vhs` is an owner action.** The tape files are written and committed in this wave; the recording and the GIF's checked-in copy wait for `brew install vhs` (and its `ffmpeg`/`ttyd` deps) in the Brewfile. The sitting menu asks.
- Commit trailer:
  ```
  Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01UyD3BUJGnLsYpUqmtLdqbj
  ```

---

## File map

| File | Responsibility after this wave |
|---|---|
| `src/frame.rs` | `Scenario::ReviewSlate`; `--opt <name>[,name]` → `DesignOpts`; the two new frame tests |
| `src/app/mod.rs` | `pub design_opts: DesignOpts` (deleted in Task 5) |
| `src/board/rows.rs` | `RowCtx.design`; the three option arms (Task 2); the winners unconditional, losers gone (Task 5) |
| `src/board/mod.rs` | passes `app.design_opts` into `RowCtx` |
| `src/main.rs` | `frame --opt` parsing and HELP |
| `tools/gen-logos.sh` | `COLLEGE=all` mode (FBS + eight CBB conferences) |
| `assets/logos{,-light}/ncaa/*.ans`, `src/board/logo_sources.rs`, `tests/logo.rs` | the marks option (side branch) |
| `docs/demo-a.tape`, `docs/demo-b.tape` (→ `docs/demo.tape` for the winner) | the GIF cuts |
| `docs/superpowers/specs/…` Directions + §9 | the sitting's decisions and receipts |

---

### Task 1: The review-slate scenario and the `--opt` knobs

**Files:**
- Modify: `src/frame.rs`, `src/main.rs`, `src/app/mod.rs`, `src/board/mod.rs`, `src/board/rows.rs` (`RowCtx.design` only), `src/dump.rs` (every `RowCtx` literal, if any, and the gallery's `RowCtx` construction in `board/mod.rs`)
- Test: `src/frame.rs` tests, `src/main.rs` tests

**Interfaces (produces):**
```rust
// src/board/rows.rs
/// Design-time switches for the wave 5 sitting. Default off; only
/// `gameday frame --opt` sets them; deleted when the sitting is decided.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DesignOpts { pub tint_rows: bool, pub clause_cap: bool, pub wide_tier: bool }
impl DesignOpts {
    pub const NAMES: [&'static str; 3] = ["tint-rows", "clause-cap", "wide-tier"];
    /// `"tint-rows,wide-tier"` → opts; an unknown name errors naming the valid set.
    pub fn parse(list: &str) -> Result<DesignOpts, String>;
}
pub struct RowCtx { …existing…, pub design: DesignOpts }
// src/app/mod.rs
pub design_opts: DesignOpts,   // App::new → DesignOpts::default()
// src/frame.rs
pub enum Scenario { …, ReviewSlate }   // "review-slate"
pub struct Spec { …, pub opts: DesignOpts }
```

- [ ] **Step 1: Failing tests.** `src/frame.rs` tests (the module has `text_of`; render through `run`'s inner closure — factor the closure into `pub(crate) fn render(spec: &Spec) -> std::io::Result<Buffer>` so tests render without Chrome):

```rust
    #[test]
    fn the_review_slate_scenario_is_the_saturday_board_with_a_favorite() {
        let spec = Spec {
            view: FrameView::Board,
            scenario: Scenario::ReviewSlate,
            theme: "broadcast".into(),
            cols: 120,
            rows: 40,
            tick: None,
            opts: DesignOpts::default(),
            out: PathBuf::from("unused.png"),
        };
        let s = text_of(&render(&spec).unwrap());
        assert!(s.contains("BOIS") && s.contains("ORE"), "Boise at Oregon is on the board:\n{s}");
        assert!(s.contains("MY GAMES") && s.contains("★"), "the scenario's favorite (ORE) sits in the band:\n{s}");
        assert!(s.contains("SAT SEP 5") || s.contains("SEP 5"), "the clock is the capture's afternoon:\n{s}");
        assert!(!s.contains("KC") || s.contains("KC "), "no demo game leaks in");
    }

    #[test]
    fn design_opts_parse_and_name_the_valid_set() {
        let o = DesignOpts::parse("tint-rows,wide-tier").unwrap();
        assert!(o.tint_rows && o.wide_tier && !o.clause_cap);
        let e = DesignOpts::parse("tint-rows,bogus").unwrap_err();
        assert!(e.contains("bogus") && e.contains("tint-rows|clause-cap|wide-tier"), "{e}");
        assert_eq!(DesignOpts::parse("").unwrap(), DesignOpts::default());
    }
```

`src/main.rs` tests: `parsed(&["gameday","frame","--opt","clause-cap","--out","x.png"]).frame.unwrap().opts.clause_cap`; `err(&["gameday","--opt","clause-cap"])` names `frame`.

- [ ] **Step 2: Implement.** `Scenario::ReviewSlate` (`"review-slate"`; doc: "the 2026-09-05 CFB afternoon that exposed R1 — 68 events, 18 live, real win probabilities; read from `fixtures/review-slate-2026-09-05.json` at run time (it is a megabyte; dev only), mapped through `provider::map::map_scoreboard`"). `apply`: `app.boards.clear(); app.pins.clear(); app.config.favorites = vec![Favorite { league: Cfb, team_abbr: "ORE" }]; app.config.enabled_tabs = vec![League::Cfb]; app.now_override = Some(datetime!(2026-09-05 16:52 -4)); let games = map_scoreboard(League::Cfb, &std::fs::read_to_string(path)?, offset -4)…; app.apply_boards(League::Cfb, games, false); app.tab = Tab::Home;` — `apply` returns `Result<(), String>` now (the file may be missing: `review-slate: fixtures/review-slate-2026-09-05.json not found — run from the repo root`). `default_tick` 0. Since `apply` runs after `demo_app`, the demo's order state and fingerprints hold demo ids; clear `app.order`/fingerprints by constructing the app fresh instead: for `ReviewSlate` build `App::new(demo_config-with-cfb, vec![], dir, -4)` directly rather than `demo_app` (say so in a comment). `Spec.opts` → `app.design_opts = spec.opts` before the view setup. `RowCtx.design` is filled in `board/mod.rs` from `app.design_opts` everywhere a `RowCtx` is built (grep `RowCtx {`; `once.rs` and tests use `DesignOpts::default()`). `main.rs`: `--opt <list>` collected with the other frame flags, validated only under `frame`; HELP gains `--opt O[,O]   design-time switches: tint-rows|clause-cap|wide-tier (wave 5 sitting only)`.

- [ ] **Step 3: Suite, clippy, fmt, commit** `feat(frame): the review-slate scenario and --opt design switches for the sitting`.

### Task 2: The three row-level options behind the knobs (V2, V3, L7)

**Files:**
- Modify: `src/board/rows.rs`
- Test: `src/board/rows.rs` tests

**Interfaces:** `pub(crate) fn clause_cap(text: &str, min_cells: usize) -> String`; `const CLAUSE_MIN: usize = 48`; `const FRAGMENT_W: u16 = 40`; `const PLAY_X: u16 = TEXT_X + FRAGMENT_W`; `pub const WIDE_TIER_MIN: u16 = 160`.

- [ ] **Step 1: Failing tests** (rows.rs test module; helpers `tier2_game`, `live_game`, `ctx`, `render`, `text_of`, `col_of`, `cells_with_fg`, `install_marks_theme`):

```rust
    /// V2 option: every tier row's abbrs in team color inside the theme's
    /// `team` scope, not only a pinned game's.
    #[test]
    fn tint_rows_colors_an_unpinned_abbr_only_under_hero_marks() {
        install_marks_theme();
        let game = tier2_game();
        let mut c = ctx();
        c.design.tint_rows = true;
        let term = render(120, 1, &game, &c, draw_tier2);
        let buf = term.backend().buffer();
        let x = col_of(buf, 0, "GB").unwrap();
        assert_eq!(buf[(x, 0)].fg, theme::current().art_color(game.away.color), "tinted under hero+marks");
        theme::set_current("broadcast").unwrap(); // scope hero: never tinted
        let term = render(120, 1, &game, &c, draw_tier2);
        let buf = term.backend().buffer();
        assert_eq!(buf[(x, 0)].fg, ink(&c), "hero scope stays ink");
        let mut off = ctx();
        off.design.tint_rows = false;
        install_marks_theme();
        let term = render(120, 1, &game, &off, draw_tier2);
        assert_eq!(term.backend().buffer()[(x, 0)].fg, ink(&off), "knob off: unpinned stays ink");
    }

    /// V3 option: the first clause after 48 cells, `…` closes.
    #[test]
    fn clause_cap_cuts_at_the_first_clause_after_the_minimum() {
        let long = "No Huddle-Shotgun #29 T.Reed Jr. rush right for 4 yards gain to the SEMO20 (#91 B.Hawkins; #13 K.Bilal-Jones), and the clock runs";
        let cut = clause_cap(long, CLAUSE_MIN);
        assert!(cut.ends_with('…'), "{cut}");
        assert!(cut.chars().count() > CLAUSE_MIN && cut.chars().count() < long.chars().count(), "{cut}");
        assert!(cut.starts_with("No Huddle-Shotgun #29 T.Reed Jr. rush right for 4 yards gain to the SEMO20 (#91 B.Hawkins; #13 K.Bilal-Jones)"), "cut at the `, ` after 48: {cut}");
        assert_eq!(clause_cap("Short play, no cut", CLAUSE_MIN), "Short play, no cut", "under the minimum: untouched");
        let no_clause = "a".repeat(120);
        assert_eq!(clause_cap(&no_clause, CLAUSE_MIN), no_clause, "no clause boundary: untouched (truncate still applies)");
        // Through the tier-1 row.
        let mut game = live_game("DAL", "PHI");
        game.last_plays[0].text = long.into();
        let mut c = ctx();
        c.design.clause_cap = true;
        let text = text_of(render(200, 3, &game, &c, draw_tier1).backend().buffer());
        assert!(text.contains("K.Bilal-Jones)…"), "{text}");
        assert!(!text.contains("and the clock runs"), "{text}");
    }

    /// L7 option: at ≥160 columns a tier-2 row carries its last play after the fragment.
    #[test]
    fn wide_tier_puts_the_last_play_on_the_tier2_row_past_160_columns() {
        let game = live_game("DAL", "PHI"); // has a fragment and a last play
        let mut c = ctx();
        c.design.wide_tier = true;
        let term = render(180, 1, &game, &c, draw_tier2);
        let buf = term.backend().buffer();
        let text = text_of(buf);
        assert_eq!(col_of(buf, 0, "▸ Hurts hit"), Some(PLAY_X), "play at PLAY_X\n{text}");
        assert_eq!(col_of(buf, 0, "PHI 3RD & 6"), Some(TEXT_X), "fragment stays\n{text}");
        let narrow = text_of(render(120, 1, &game, &c, draw_tier2).backend().buffer());
        assert!(!narrow.contains("▸ Hurts"), "under WIDE_TIER_MIN: the two-row form\n{narrow}");
        let mut off = ctx();
        off.design.wide_tier = false;
        let wide_off = text_of(render(180, 1, &game, &off, draw_tier2).backend().buffer());
        assert!(!wide_off.contains("▸ Hurts"), "knob off: current form\n{wide_off}");
    }
```

- [ ] **Step 2: Implement.** `abbr_span`: `let tinted = (game_pinned || ctx.design.tint_rows) && th.roles().team == TeamColorScope::HeroMarks;` (pass `ctx`, it already does). `clause_cap`: walk char indices; the first `, ` or `. ` whose start index (in chars) is ≥ `min_cells` → keep up to and including the char before the comma/period? Spec: "first clause (split at the first `, ` or `. ` after 48 cells, `…` closes)": keep the text before the separator and append `…` (the test above expects `K.Bilal-Jones)…` — the clause up to the `)`, then `…`; the `, ` is dropped). Tier-1's play row: `let shown = if ctx.design.clause_cap { clause_cap(&play.text, CLAUSE_MIN) } else { play.text.clone() };` then the existing truncate. Tier-2: after the fragment, `if ctx.design.wide_tier && area.width >= WIDE_TIER_MIN { if let Some(play) = game.last_plays.first() { col(frame, area, PLAY_X, area.width, 0, Left, "▸ " + truncate(text, width - PLAY_X - 2)) } }` and the fragment's own room becomes `FRAGMENT_W - 1` in that case so it cannot run into the play. Doc comments carry the receipts named in the rulings.

- [ ] **Step 3: Suite, clippy, fmt, commit** `feat(rows): the sitting's three options — tinted rows, the clause cap, the wide tier (behind --opt)`.

### Task 3: The marks option — every FBS school and the top eight CBB conferences (V1, side branch)

**Files:**
- Branch: `v4-wave5-marks` from `v4-wave5` (after Task 1)
- Modify: `tools/gen-logos.sh` (`COLLEGE=all`), `assets/logos/ncaa/*.ans`, `assets/logos-light/ncaa/*.ans`, `src/board/logo_sources.rs` (regenerated), `tests/logo.rs` (the ncaa range)
- Test: `tests/logo.rs`

- [ ] **Step 1:** `tools/gen-logos.sh`: a `COLLEGE` variable (`poll`, the default and today's behavior; `all`). Under `all`, `resolve_league` for `cfb` reads `$API/football/college-football/teams?groups=80&limit=1000` (FBS, ESPN group 80 — the same group the scoreboard uses) and for `cbb` reads `$API/basketball/mens-college-basketball/teams?groups=<g>&limit=1000` for each of the eight conference groups — verify the ids against `$API/basketball/mens-college-basketball/groups` before trusting them and print the conference names it resolved: ACC, Big East, Big Ten, Big 12, SEC, Atlantic 10, Mountain West, American. Output the same `key<TAB>href<TAB>on-white` rows keyed `ncaa/<id>`; the `seen` dedupe already merges the two buckets. Header comment: what `all` fetches, how many marks it makes, and the binary cost.
- [ ] **Step 2:** `COLLEGE=all LEAGUES="cfb cbb" tools/gen-logos.sh` (network: ESPN's public team lists and CDN art, a one-time dev fetch with the script's existing 150 ms spacing); report the mark count, `du -sh assets/logos assets/logos-light`, and the release binary's size before/after (`ls -l target/release/gameday`). `tests/logo.rs`'s ncaa assertion becomes a `match`-free range for this branch: `(150..=400).contains(&ncaa)` with a comment naming the set (FBS 136 + eight conferences, deduped); the light-set tests keep passing (their six keys are in the set). Run the suite; the render-time cost is `include_str!` — say what `cargo build --release` took before and after.
- [ ] **Step 3: Commit** `feat(marks): every FBS school and the top eight CBB conferences (sitting option V1-a)` on `v4-wave5-marks`. Build `target/release/gameday` from this branch into a copy `out/design/bin/gameday-marks-all` (and the base binary as `gameday-marks-ranked` from `v4-wave5`) so the sitting renders both without switching branches.

### Task 4: The sitting (controller; the render gate)

**Files:** `out/design/` (gitignored), `docs/demo-a.tape`, `docs/demo-b.tape`

- [ ] **Step 1: Render every option**, labeled by filename, all from `--scenario review-slate --theme broadcast` unless said:
  - `marks-ranked.png` (base binary, `--view board --size 120x40`) vs `marks-all.png` (the marks binary, same command); plus `marks-all-daygame.png` (`--theme daygame`, the light set).
  - `tint-b-current.png` vs `tint-a-rows.png` (`--opt tint-rows`), both at `--size 120x40`; a `tint-a-rows-studio.png` to show studio stays gray.
  - `clause-b-full.png` vs `clause-a-cap.png` (`--opt clause-cap`) at `--size 120x40` (the tier-1 rows carry the long CFB sentences).
  - `wide-b-current.png` vs `wide-a-play.png` (`--opt wide-tier`) at `--size 180x40`.
  - `zoom-40.png`, `zoom-60.png` (`--view zoom --scenario full-slate --size 120x40` / `120x60`) for the L5 confirmation.
  - The GIF: write `docs/demo-a.tape` (board → the red-zone beat → the cut → zoom) and `docs/demo-b.tape` (board → the red-zone beat → the cut → tv) as `vhs` tapes at 120x36, broadcast, ~20 s, `Output docs/demo.gif`; if `vhs` is on PATH render `gif-a.gif`/`gif-b.gif`, else the tapes ride the sitting as text and the menu item asks for the install.
  - `tools/contact-sheet.sh out/design out/design/contact-sheet.png 2`.
- [ ] **Step 2: Present the lettered menu** to Walter, one item per decision, each with a one-line recommendation, the contact sheet and the PNGs opened. Items: 1 marks (A all / B ranked), 2 tint (A rows / B current), 3 clause (A cap / B full), 4 wide tier (A play / B current), 5 zoom fill (confirm / reopen), 6 GIF cut (A zoom / B tv) plus the `vhs` install (A add to the Brewfile and install / B skip the GIF until wave 6 / C use another recorder). Wait for the letters. **This is the gate; nothing past it runs without them.**

### Task 5: After the letters — ship winners, revert losers, record directions

**Files:** `src/board/rows.rs`, `src/frame.rs`, `src/main.rs`, `src/app/mod.rs`, `src/board/mod.rs`, `tests/logo.rs` (if marks-all wins: merge `v4-wave5-marks`), `docs/demo.tape` (the winning cut; delete the loser), `docs/superpowers/specs/…` (Directions + §9), `CHANGELOG.md`

- [ ] **Step 1:** For each of V2/V3/L7: the winning arm becomes unconditional (or the option's code is deleted when B wins); `DesignOpts`, `RowCtx.design`, `App.design_opts`, `Spec.opts`, `--opt` are deleted entirely — no knob survives the sitting; the three tests from Task 2 become tests of the shipped behavior (or are deleted with the loser). `Scenario::ReviewSlate` stays (it is the marks and ranking frame from now on). Marks: merge or delete the side branch. GIF: the winning tape is `docs/demo.tape`; the GIF is checked in if `vhs` ran, else §9 says it waits for the install.
- [ ] **Step 2:** Spec Directions gain items 10–15 in the sitting's own words (what won, why, what reopens it); §9 `### Wave 5 — landed <date>`: the contact sheet's filename list, the letters, the marks set's size and binary cost, the zoom fill confirmation, the GIF status; CHANGELOG `### Changed` for each shipped winner. Run the gallery (`cargo run --release -- dump`) once and say which stems changed and why (a shipped winner changes them deliberately).
- [ ] **Step 3: Suite, clippy, fmt, commit** `feat(v4): wave 5 — the sitting's winners; directions recorded`.

## Self-review against spec §7

Item 1 (marks a/b on the review slate) → Tasks 1, 3, 4; item 2 (tinted abbrs) → Tasks 2, 4; item 3 (clause cap) → Tasks 2, 4; item 4 (wide tier at ≥160) → Tasks 2, 4; item 5 (zoom fill at 40/60) → Task 4; item 6 (GIF, two cuts) → Task 4 (tapes) and Task 5 (winner), gated on `vhs`; "the review-slate fixture drives the board frames" → Task 1; DoD ("letters recorded; losers reverted; the winning GIF checked in with its tape") → Task 5, with the GIF's checked-in copy honest about the install. Names: `DesignOpts`/`RowCtx.design`/`Spec.opts` (Task 1) consumed by Task 2 and deleted by Task 5; `Scenario::ReviewSlate` (Task 1) used by Tasks 3–4 and kept. Test-count ledger: 643 → 646 (T1) → 649 (T2) → 649 (T3, on the side branch) → Task 5 measured; measured numbers win.
