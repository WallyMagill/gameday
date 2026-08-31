# gameday v2.1 — themes as identities, inline meters, ticker directions

Date: 2026-08-31
Status: approved (Walter, in-session)
Addendum to `2026-08-30-gameday-v2-design.md` §4. Supersedes the style-lab
"calm level" pick: color discipline is a theme property, not a global choice.

## Decisions

- Calm level: **not a global pick.** Each theme carries its own discipline.
  `broadcast` stays loud (level 1); `studio` is the broadcast palette at
  level 3 ("both 1 and 3").
- Themes must be identities, not tints (Omarchy / oh-my-zsh model): full
  coordinated palettes, a live-preview picker, user-authorable files.
- Shipped set (**option B**): `broadcast` (default), `studio`, `ceefax`,
  `phosphor`, `gruvbox`, `tokyo-night`, `nord`, `catppuccin-mocha`,
  `rose-pine`, `everforest`, `dracula`. Community palettes are opt-in only —
  the default identity remains broadcast.
- Meter: **variant B** (inline gauge row under the identity block) for all
  four meters. Variant A is retired.
- Ticker: **no pick yet.** Three fresh directions get rendered; none may be a
  refinement of the boxed a/b/c.

## Theme = palette + discipline

One TOML format, used by built-ins (compiled in from `assets/themes/*.toml`)
and by user files in `~/.config/gameday/themes/*.toml` (user file wins on a
name clash):

```toml
name = "gruvbox"

[palette]
bg = "#282828"      fg = "#ebdbb2"     bright = "#fbf1c7"
muted = "#928374"   dim = "#3c3836"    border = "#504945"
live = "#fb4934"    green = "#b8bb26"  cyan = "#8ec07c"
magenta = "#d3869b" star = "#fabd2f"

[palette.league]   # one accent per league slug; missing slugs fall back to `star`
nfl = "#fb4934"  cfb = "#fe8019"  nba = "#83a598"  cbb = "#d3869b"
wnba = "#fabd2f" nhl = "#8ec07c"  mlb = "#b8bb26"  epl = "#b8bb26"  mls = "#8ec07c"

[discipline]        # what the chrome is allowed to color; scores, logos, LIVE,
chips = true        # and scoring words are always colored (the identity floor)
section_labels = false   # LAST PLAYS / TOP PLAYS / RECORDS headers
play_abbrs = false       # team color on play-line abbreviations
clocks = false           # cyan clocks vs muted
sidebar_headers = "single"   # "multi" | "single" | "muted"
```

- Parse errors name the file, the key, and the expected form; a bad user
  theme never crashes startup — it is skipped with a stderr line.
- `:theme` with no argument opens a **picker view**: j/k previews live on the
  board behind it, Enter commits + persists, Esc reverts to the prior theme.
  `:theme <name>` still applies directly; completion lists all loaded names.
- Known limitation, accepted: logo art was composited over black, so on
  tinted-background themes the mark edges carry a faint black fringe.

## Meter B — inline row, all sports

The right-hand gauge column is removed. One row sits under the identity
block, before the momentum row; plays get the full tile width.

| Meter | Row |
|---|---|
| RedZone | `RED ZONE  ━━━━━━━●────  G   3 TO GOAL` — track 20→G, marker in `live` |
| Lead | `LEAD  -15 ───────▮────── +15   DEN +7` — marker in the leading team's color |
| Diamond | `BASES  ◆◇◇   OUTS ●●○   COUNT 1-2` — occupied bases in `star` |
| Penalty | `PENALTY  DAL ▮▮▮▮▮▮░░░░ 0:42` — countdown bar of the 2:00 minor |
| none (soccer / no data) | row omitted; plays gain the line |

Must hold at 2×2 (120×36), narrow (80×24), and zoom. Labels use the theme's
discipline (section_labels) for their color.

## Ticker — three new directions (rendered only, decided by eye)

- **d — BottomLine two-lane**: lane 1 is a continuous compact score strip for
  every game in play (`NFL KC 27 TB 24 Q4 1:27 │ NBA DEN 88 BOS 81 Q3 4:38 │ …`),
  lane 2 is scoring alerts. No box; a thin rule above.
- **e — LED ribbon**: each event is a chip — team abbr on a team-colored
  block, then the scoring word and text — on a dark band; league chip as the
  separator. One row.
- **f — split-flap**: fixed-width cells (`│ 3:21 KC TD 27-24 │`) that flip
  when a new event arrives; the sim drives the flip, the dump captures one
  mid-flip frame plus a settled frame.

The style lab drops the calm-* and meter-* variants (decided) and renders
ticker-d/e/f. The winner is implemented in a later pass; the lab is deleted
then.
