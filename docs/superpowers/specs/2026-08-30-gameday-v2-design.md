# gameday v2 — interaction, depth, calm

Date: 2026-08-30
Status: approved (Walter, in-session)
Builds on: `2026-08-29-gameday-design.md` and the shipped RedZone draw layer
(branch `redzone-draw`). This spec changes how the app is *used*; it does not
change stack, data provider, or the board's visual identity.

Decisions locked in brainstorm:
- Command grammar: k9s/vim hybrid (`:` commands, `/` filter, `z` zoom, single
  keys for frequent actions). No tmux prefix key. No numbered-panel model.
- Depth: per-game Zoom tabs (Overview | Plays | Stats) AND a global `:plays`
  feed. League leaders screens are out of v2 except `:standings`.
- Feature cut: ALL of — score alerts, standings screen, slate time-travel,
  odds on pre-games, mouse support, config screen. (Recommendation was to
  defer standings + config; Walter overruled — both are in.)
- Style decisions are made by eye from rendered PNG variants, never prose.

## Non-goals

- No new backend, accounts, betting placement (odds display only), video.
- No daemon/client split, no framework change (named and rejected).
- No league-leaders screens beyond standings.
- The board's RedZone identity stays; the style pass calms it, not replaces it.

## 1. Interaction layer

Input modes on `App`: `Normal`, `Command`, `Filter`. Esc always leaves a mode
before it pops a view.

**Command mode (`:`)** — one-line prompt at the footer, with completion
(Tab cycles matches) against a static registry:

| Command | Action |
|---|---|
| `:nfl` `:nba` … (every league slug) | jump to that league tab |
| `:home` / `:all` | Home tab |
| `:plays` | global plays feed view |
| `:standings [league]` | standings view (default: current tab's league) |
| `:config` | config view |
| `:theme <name>` | set theme (completes names) |
| `:score big\|compact` | score style |
| `:layout 1\|2\|4\|s\|auto` | layout override |
| `:pin <abbr>` | pin that team's current game (error line if none found) |
| `:q` | quit |

Unknown command → footer error naming the valid set (agent-readable failure
rule applies: show what was typed and what's legal).

**Filter mode (`/`)** — incremental text filter on the current tab's games
(mosaic + slate) matching abbr/city/name, case-insensitive. Filter persists
until Esc or cleared with an empty pattern; active filter shows in the footer.

**Zoom (`z`)** — zooms the selected tile (tmux zoom steal). Enter stays as an
alias. `z`/Esc restores the board.

**Keymap** — one table (`src/keymap.rs`) is the single source for dispatch,
footer chords, and the help overlay. Arrow/vim parity everywhere.

**Mouse** — crossterm mouse capture: click selects a tile / tab chip / slate
row, click on a zoomed view's tab bar switches tabs, wheel scrolls any
scrollable feed. No hover states, no drag.

## 2. Views

`App.view: View` replaces the `focused_id` mechanism:

```rust
enum View {
  Board,
  Zoom { game_id: String, tab: ZoomTab },   // Overview | Plays | Stats
  PlaysFeed,
  Standings(League),
  Config,
}
```

- Each view renders from its own module under `src/views/`; `app.rs` shrinks
  to state + dispatch + shared chrome (header/ticker/footer stay global).
- Esc pops: mode → Zoom tab view → Board. `q` quits only from Board (footer
  says so); elsewhere it pops like Esc.
- **Zoom/Overview** — the current focus view (LED digits, field bar, plays,
  scoring timeline).
- **Zoom/Plays** — full play-by-play feed for that game, newest first,
  j/k + wheel scrolling, scoring plays highlighted.
- **Zoom/Stats** — box score from the summary endpoint: team stat rows
  (yards, TOP, shooting %, hits/errors — per sport) and leader lines
  (e.g. `PASS Mahomes 18/24 214 2TD`). Tabs cycle with `h/l` or `[`/`]`.
- **PlaysFeed (`:plays`)** — the same feed widget, but across all boards'
  scoring plays, tagged with league chip + matchup, scrollable.
- **Standings (`:standings`)** — one league's table: division/conference
  groups, W-L(-T/OTL), streak if present. Read-only.
- **Config (`:config`)** — toggle enabled tabs, add/remove favorites (typed
  abbr), theme, score style, layout. Writes through the existing
  `Config::save_to`. j/k + space/enter interaction; no free-text except abbr.

## 3. Data (all additive, all fixture-tested offline)

| Need | Source | Domain | Poll |
|---|---|---|---|
| Box score | summary `boxscore.teams[].statistics`, `leaders` | new `GameStats` | zoomed game only, ~30s |
| Standings | `…/standings` per league | new `Standings` | on demand, cache 10 min |
| Slate time-travel | scoreboard `?dates=YYYYMMDD` | existing `Game` | on demand per date; only today live-polls |
| Odds | scoreboard `competitions[].odds[0]` (`details`, `overUnder`) | `Game.odds: Option<String>` | with scoreboard |

- `[`/`]` on a league tab steps the viewed date (yesterday ↔ today ↔
  tomorrow, ±7 max); header shows the viewed date when it isn't today.
- **Alerts**: after each board apply, favorited teams' score deltas ring the
  terminal bell (`\a`) and flash a header banner (`★ KC TOUCHDOWN 27-24`),
  any tab, 30s per-game cooldown. Uses the existing `last_scores` map.
- NHL penalty / shots stay deferred until live NHL exists to verify field
  names (carried from v1).

## 4. Style pass — by eye

Policy baked into every theme: chrome is gray/white; red is earned by
LIVE/scoring only; team colors appear only on scores, abbrs, logos; league
accents shrink to the chip; sidebar headers use one accent family.

Process: build first, then render variant sets as PNGs —
(a) three calmness levels of the policy, (b) 2–3 meter-column redesigns,
(c) 2–3 ticker redesigns. Walter picks each by eye; losers are deleted.

## 5. Verification

- Unit: command parser (valid/invalid/completion), view routing + Esc-pop,
  filter matching, alert cooldown, date stepping.
- Fixtures: boxscore, standings, dated scoreboard, odds — captured once,
  committed, no network in tests.
- Dump gallery gains: `zoom-stats`, `plays-feed`, `standings`, `config`,
  `filter` captures (fixed names, deterministic tick).
- Pty smoke drives: `:nfl<CR>`, `/kc<CR>`, `z`, tab cycling, `:q`.
- Execution: implementation plan → workflow, same gates as v1 (tests green +
  implementer looks at its own PNGs before committing).
