# gameday v3.3 — Polish (sub-project 3)

Date: 2026-09-03. Status: draft for Walter's review.
Predecessor: v3.2 Identity (merged to `redzone-draw` at `1dfd4b6`; spec `2026-09-01-gameday-v3-2-identity-design.md`).
Successor (already scoped apart): sub-project 4 "Data truth" — `Play.kind` from the provider, per-league logo expansion, NHL power play from summaries, soccer red-card bonus, startup-finals headlines. Nothing provider-side lands here.

Sources: two independent visual reviews of the shipped v3.2 gallery (a cold-eye critique with no spec context, and a faithfulness review against the A′ reference frames), run 2026-09-03. Their findings converged; this spec encodes the union. Full reports: `.superpowers/sdd/2026-09-03-v3-polish-review/` (gitignored — findings are restated here in full; the spec is self-contained).

## §0 Goal and principles

The v3.2 board's bones are right. This sub-project makes every frame broadcast-grade: readable at every size, one visual language across every screen, and screenshot-worthy on the flagship frames.

Principles, binding on every task:
- **One grammar everywhere.** The board's visual language (lowercase legend, rule lines, role colors, shared column grid) is THE language. Any screen still speaking v3.1 gets converted, not exempted.
- **Render-gated taste.** Any change marked [RENDER-GATE] ships only after Walter picks from labeled, rendered PNG options (real frames from the dump pipeline, not mockups). Certain fixes (alignment, dead code, honest copy) need no gate.
- **Never regress the hard rules.** One score formatter (`hero::score_block`); logos never move a digit; the board never re-sorts without an event (R24); digits charged before garnish (R29). All v3.2 rulings stand.
- **Fallbacks stay honest.** Every glyph ladder ends in plain text; nothing renders blank or as tofu *by our choice* (portability work in §1 exists to remove tofu we currently emit).

## §1 Digit and glyph integrity

Two distinct problems, one spike then one fix wave.

**1a. Portability spike (first task of the plan).** The big digits (sextant `PixelSize` path) and all logo art emit Unicode Legacy Computing sextants (U+1FB00–1FB3B). Modern GPU terminals (Kitty, WezTerm, Ghostty, iTerm2 ≥3.5, Alacritty ≥0.13) synthesize these procedurally regardless of font; Terminal.app and font-dependent setups show tofu when the font lacks the block (JetBrains Mono and most un-patched monospace fonts lack it; Cascadia Code ≥2404.23 has it). The spike produces a **render matrix receipt**: the same frame captured in (at minimum) Terminal.app, iTerm2, and one GPU terminal, across 3 common fonts, screenshots attached. The spike's output decides 1b's default:
- If tofu is real on mainstream setups → quadrant/half-block (U+2580–259F, universally covered) becomes the **default** digit/logo range, sextants an opt-in `config` value for terminals that render them.
- If coverage is broadly fine → sextants stay default, quadrant becomes the fallback config, README documents the one-liner.
The decision is recorded in the plan ledger with the matrix as its receipt. No rewrite happens on the cold-eye reviewer's inference alone.

> **Deviated as built — see `## Decisions`.** The spike found real tofu, so quadrant became the default (1A); the "sextants an opt-in `config` value" clause did NOT ship — ruling R40 refused the toggle (quad is the design, not a fallback) and the sextant path was deleted outright.

**1b. Mid-size digit readability [RENDER-GATE].** At 80×24 the shipped hero digits do not resolve into readable numbers; the A′ reference's digits at the same width are unmistakable. Deliverable: at least two redesigned mid-size digit treatments (e.g. quadrant-block 4-row digits; bold double-width text digits) rendered at 80×24 and 100×30 beside the current one. Walter picks. The chosen form slots into the existing Full → mid → text ladder without changing bracket rules (R28/R32).

## §2 TV overhaul

The weakest frame. All items in one task:
- **Digits**: row-double the existing 8-row Full glyphs to a 16-row jumbotron size at tall heights (no custom font; each glyph row painted twice). Bracketed: 16-row when the body affords it, else today's Full. [RENDER-GATE: doubled vs current, at 120×40.]
- **Dead space**: the ~6 empty rows at 120×40 are absorbed by the taller digits + a vertically balanced stack; no frame ships with more than 2 rows of intentional air.
- **ALSO LIVE strip**: two columns of up to 5 rows at ≥100 cols (single column below), matching the A′ frame.
- **Linescore**: team-colored rows (away in away color, home in home color) — today both paint red. Team abbr labels appear at the hero nameplates (shipped TV has no labels at all).
- Footer stays the TV legend; `next cut:` caption stays on the strip rule (v3.2 review confirmed that matches the frame family).

## §3 Cut refinements

- **Band geometry**: today a firing band shoves the whole list down 2 rows and back — the only layout jump left in the app. Options: reserve the 2 rows always while live games are present (standing air, used for the section rule when quiet) vs keep the jump. [RENDER-GATE: both as short frame sequences; the no-jump principle argues for reserving, but the standing air is a real cost only frames can weigh.]
- **Band placement**: stays top-of-screen red (glanceability from across a room is the product), with the fill dropped one step so it reads as a banner, not an alert. The mockup's amber-inside-IN-PLAY variant is rendered once for the same gate so the option is real, not remembered. [RENDER-GATE]
- **Band row 2**: adopt the reference's affordance — `enter jump · clears in Ns` (countdown live). This adds one interaction: `enter` while a band is active zooms the band's game instead of the selection. Suppressed when a prompt is open. (Smallest behavior change in this sub-project; called out here so it's approved with the spec.)
- **Takeover completeness**: team abbr labels beside the score block; away score wears its team color (today it falls back to neutral amber when the away color is the lifted one); the spec §3 dimmed bottom strip and the clear-timer line ship (both were in v3.2's spec but not the shipped frame).

## §4 Grid discipline (the board)

- **Tier-1 score legibility**: tier-1 rows gain a plain, readable score on the SAME five-column grid as tier-2/FINAL rows (both reviewers independently: featured rows are the only rows whose score you can't read at a glance). The sextant mini-digits move from "the score" to "the garnish": glyphs stay if they fit, but the plain numerals are always present and aligned to the shared grid. *(Deviated as built — see `## Decisions`: sitting 1 pick 2A dropped the garnish entirely (R39). The plain numerals shipped; the glyphs did not stay, and dropping them is what aligned tier-1's clock to tier-2's x.)* Tier-1 keeps its identity via the accent bar, bold weight, and the indented fragment/play lines. [RENDER-GATE: current vs re-gridded, one frame each.]
- **Mark column**: the hot/nudge gutter aligns to one column across all tiers (today it reads ragged between tier-1 blocks and one-line rows).
- **2-char abbrs**: pad to the 3-char cell so the grid gap is constant (`KC ` not `KC`), matching the reference's optical rhythm.
- **Bare section headers**: a section with zero rows renders nothing — no `LATER ───` orphan. Applies to all four sections. (Certain fix, no gate.)
- **SCORES lane**: keep the counts-caption behavior as shipped; no change (the faithfulness reviewer preferred the ref's per-game list, but the lane's job at small sizes is the count — parked unless Walter reacts at the gate).

## §5 One language on every screen

Every non-board screen converts to the board's grammar in one wave:
- **Footer**: the lowercase legend everywhere; the v3.1 `NAV: [TAB]` caps grammar is deleted app-wide. Keymap stays the single source; per-view contexts already exist.
- **Layout**: no screen floats a narrow column in a half-empty frame. Standings: two-column conference/league layout at ≥100 cols. Plays feed: full-width rows with the board's stamp/text columns. Config: two-panel (sections | values) at width, single column below. Filter/theme-picker/help: content block vertically balanced, key bar anchored beneath content (`content_end + 1`), not glued to the terminal floor with a gulf.
- **Zoom**: symmetric logo flanks (both sides or neither — today one side can render alone); linescore digits in team colors (the white NYY digit was a role miss); zoom footer converts with the rest.
- Empty/error screens (offline, stale, config-error, home-live empty): re-checked against the new balance rule; strings unchanged.

## §6 Themes

- **studio** [RENDER-GATE]: rebuilt as press-box monochrome — grayscale roles + exactly one accent (red) for hot/scoring. If the render doesn't clearly earn a slot beside broadcast, studio is deleted rather than shipped as a near-duplicate (three names, three looks, or fewer names).
- **gruvbox**: ground corrected to real `#282828` family (shipped frame reads near-black); role mapping re-checked against gruvbox's published palette.
- **light theme** [RENDER-GATE]: one new built-in designed for light terminals ("daygame" working name). Enters BUILTIN_NAMES only if Walter approves the render; otherwise parked.
- Theme-picker rows already render roles (v3.2 fix) — they pick up all changes automatically; picker test updated.

## §7 Copy

- Chip: `TYING RUN 3RD` (13 chars — fits the field exactly), `GO-AHEAD 3RD` stays.
- The `!` is dropped family-wide from scoring words (`TOUCHDOWN`, `HOME RUN`, `RUN SCORES`, `GOAL`, `BUCKET` — the block letters ARE the exclamation).
- `RUN SCORES` (noun form) replaces `RUN SCORES!`.
- One pass over every user-visible string for the caps/lowercase rule: section rules and scoring words are the only ALL-CAPS survivors outside data (team abbrs, league tags).

## §8 The render gate protocol

- Every [RENDER-GATE] task produces labeled PNG options through the real dump pipeline (`out/gate/<task>-<option>.png`), captioned in the filename.
- The controller batches gates into at most TWO sittings for Walter: one mid-project (digits §1b, tier-1 grid §4, band §3), one at the end (TV §2, themes §6, anything reopened). Each sitting is a lettered menu per item.
- A rejected option comes back as new directions, not a refinement of the rejected one (Walter's standing rule).
- The final DoD sitting reviews the regenerated 22-stem gallery as a whole.

## §9 Product decision carried into this spec (Walter answers at spec review)

**TV cut scope.** v3.2 ships spec-faithful "every scoring play takes the screen while TV is on" — the live night showed that a busy slate makes TV mostly takeovers. Options:
- A. Keep as shipped (TV = never miss a score; `space` lock already suppresses switching, not cuts).
- B. Full takeover only for the SHOWN game (+ MY GAMES teams); other scores get the band over TV.
- C. As B, plus a `:tv all` toggle to restore A per-session.
Recommendation: **B** — TV's job is watching one game with awareness of the rest; the band preserves awareness without hijacking the jumbotron. C is B with a knob we can add later if the live feel demands it (YAGNI now).

Direction (Walter, 2026-09-03): **B** — full takeover in TV only for the shown game and MY GAMES teams; other scores band over TV. Reasoning: TV is for watching one game with awareness. Reopens if the live feel shows the band under-serves big moments.

## §10 Non-goals

- No provider/data-mapping changes (sub-project 4).
- No new leagues, views, or commands beyond `enter`-jump on the band (§3) and the possible `:tv all` (§9C, only if chosen).
- No changes to ranking, ordering, fingerprint gating, or layout brackets except where §1b's chosen digit form requires a bracket-internal row budget tweak (bracket BOUNDARIES stay).
- No logo redesign (sub-4 expands coverage; this project only fixes symmetry/placement bugs).

## §11 Definition of done

- Every §1–§7 item closed or explicitly parked with Walter's initials in the ledger.
- The render matrix receipt (§1a) committed under `docs/research/v3-polish/`.
- Gallery regenerated; the two-sitting gate record (which option won each gate) appended to this spec as `## Decisions`.
- Full suite green at default parallelism; clippy zero warnings; the v3.2 verification receipts still hold (spot-run: size sweep, hard-rule tests).
- CPU re-sampled once against the v3.2 number (1.19%) — row-doubling and reserved band rows must not move it beyond jitter.

## Decisions (the two sittings, Walter, 2026-09-03)

Sitting 1 — from rendered frames at 80×24/120×36:
- Mid-size digits: **quad** (1A) — quadrant 4-row glyphs; current sextants tofu on Terminal.app and the PNG pipeline; text carries no weight. Wired as the ladder's mid rung (Full → quad → text).
- Tier-1 grid: **re-grid kept, garnish dropped** (2A) — one amber score column through the whole board; dropping the garnish aligned tier-1's clock to tier-2's x. Sextant garnish deleted (R39), not toggled.
- Band: **reserved rows, red top** (3A) — quiet and fired frames cell-identical below the band; the amber-in-list variant was rendered and declined.
- Config default: quadrant glyphs everywhere by evidence (spike matrix); no `glyphs` toggle ships (R40 — quad is the design, not a fallback). §1a's opt-in clause is deviated by that ruling.
- Consequence beyond the menu: all 38 logo marks regenerated sextant-free (guard test pins the range); the scoring word dropped its sextant rung (R42).

Sitting 2 — from rendered frames at 120×36/40:
- TV: **confirmed** (1A) with axis-gated 2× digits (R43 — 32×16 blocks; 3-digit scores degrade to rows-only by arithmetic). Remaining deltas vs the mockup (1-row meter, no FINAL·LATER strip section) accepted.
- Studio: **ships** (2A) as press-box monochrome — grayscale roles, red the only chroma, team color on the hero only.
- daygame: **parked** (3A) — the light theme reads well but committed logo art is baked black-on-black-box; promotion rides sub-project 4's art work. Roles + contrast test stay in-tree.
- gruvbox: **true #282828** (4A) — canonical dark0; the warm dark0_soft variant was rendered and declined.
- Config screen: **centered** (5A) — Walter asked for the recommendation; the top-anchored variant was rendered and re-created the §5 dead-space defect, so centered was confirmed by frames.
- Carried out of the sittings: 40×12 takeover (bold word, no labels) shipped tested but uneyeballed — final review triages; `TeamColorScope::Never` is a dead value — meaning-or-delete at final review.
