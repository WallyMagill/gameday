# gameday — design spec

Date: 2026-08-29  
Status: draft for review  
Binary / crate: `gameday`  
Stack: Rust, Ratatui, crossterm

A terminal sports board. You pin games, they tile. Home is your mix across sports. League tabs are full live boards for one sport. NFL data first; the shell is multi-sport from day one.

## Goal

Run in a terminal next to an editor. Watch live scores, last plays, and a sport-specific meter for games you chose. No account, no API key, no server.

Success for NFL Kickoff (2026-09-09): Home + NFL tab, live tiles, pin/favorite, last plays, ticker, 32 NFL logos.

## Non-goals (v1)

- Video, betting beyond showing spread if ESPN already sent it, fantasy, accounts
- Scraping HTML
- Fake CRT / teletext / NES skins as the default look
- Four live sports on day one (tabs exist in the shell; only NFL is wired)

## Surfaces

### Home (always on)

Mixed watch board. Tiles of games you pinned, any enabled sport. Coding companion. Ticker is mixed scoring plays from those games.

### League tabs (optional)

NFL, CFB, CBB, NBA, NHL, … — each a full board for that sport: **live tiles + compact slate** (upcoming / final). Ticker is that sport only. You can sit on NFL all Sunday without opening Home.

Enabled tabs are a setting. Turn NBA off and it leaves the chrome. NFL-only is config, not a fork of the app.

### Settings

Which league tabs exist, favorite teams, layout override (`auto` / `1` / `2` / `4` / `sidebar`).

### Pinning

- **Favorite team** — persistent. When that team has a game this week, it auto-appears on Home.
- **Pin** — this game on Home until unpinned, or until 6 hours after `final`.
- A live game on a league tab is just live. It reaches Home only via pin or favorite.

## Tiler

One **game tile** everywhere. The packer picks density. Sport-specific UI is a `meter` slot inside the tile, not a different window type.

### Densities

| Density | When | Tile shows |
|---|---|---|
| Full | 1 pane | Logo, score, clock/situation, last plays, meter, drive if football |
| Standard | 2 or 4 panes | Logo, score, clock/situation, last 3 plays, meter |
| Compact | sidebar / overflow | Logo or abbr, score, clock, LIVE |

### Home packer

| Open pins | Default layout |
|---|---|
| 1 | Full |
| 2 | Split (side by side; stacked if the terminal is narrow) |
| 3–4 | 2×2 |
| 5+ | 2×2 of the first four + pager (`n`/`p`) |

User can override density. Stored: pin list, enabled tabs, preferred density.

### League packer

Live games fill the mosaic (same 1 / 2 / 4 rules). Upcoming and final sit in a compact slate, not in live slots. Eight 1pm NFL games do not draw eight full tiles — four (or current layout) plus pager. Live always wins a slot over upcoming.

### Sidebar / coding column

Width under ~60 columns, or forced `sidebar`: stack compact tiles, one-line ticker, no slate.

### Resize

Reflow on terminal resize. No pixel layouts on disk.

## Tile contents

A tile reads a domain `Game`, never ESPN JSON.

- Two teams: abbr, display name, colors, logo key
- Status: `pre` / `live` / `final`
- Score, clock, situation (football: down/distance/possession/yard line; other sports fill the same fields in their language)
- Last plays (newest first)
- Optional `meter`: red zone (NFL), lead (NBA), diamond (MLB), penalty clock (NHL). Empty until that sport’s widget exists
- Broadcast / start time for `pre`

**Logos.** First-class. Cell spec: 8×5 bounding box, two colors on black, one glyph language. Missing logo → abbreviation in team color, never a hole. NFL 32 first, then CFB, then other leagues. Art lives in-repo; not fetched at runtime.

**Look.** Terminal-legal: one cell grid, box drawing, truecolor. Scores as bold `27 - 24` (not multi-row block digits). Team color on abbr and score. Cool/dim for structure. Black unused space is composition, not gray padding. No Catppuccin-as-identity. Broadcast density inspired by the user’s RedZone mosaic; optical depth (warm live vs cool structure) inspired by MEK.txt chromostereopsis — do not copy that series’ `#FF0000`/`#0000FF` as the brand.

## Data

### Provider

```text
trait SportsProvider {
  scoreboard(league) -> Vec<Game>
  summary(game_id)   -> Summary   // plays, drive, box
}
```

First impl: ESPN unofficial JSON.

- Base that works today: `https://site.web.api.espn.com/apis/site/v2/sports/{sport}/{league}/…`
- `site.api.espn.com` returned 403 (2026-08-29); do not use it as primary
- Scoreboard: `/scoreboard` (NFL: `football/nfl`; CFB: `football/college-football`)
- Game detail: `/summary?event={id}`
- Browser-like User-Agent
- No API key

Polling is “live.” No public WebSocket.

| Condition | Request | Interval |
|---|---|---|
| Anything visible is live | Scoreboard for leagues in play (Home pins’ leagues + current tab) | ~20s |
| Nothing live | Scoreboard | ~60s |
| Visible live tiles only | `summary` for last plays | ~15s |
| Upcoming / final | Scoreboard only | slow |

Do not summary-poll an entire NFL week. Rate-limit and backoff on 403/5xx. Last good payload stays on disk; tiles keep last scores; footer shows stale. No blank board.

### Persistence

`~/.config/gameday/` (or XDG equivalent):

- `config.toml` — enabled tabs, layout preference, favorite teams
- `pins.json` — Home pins (game id, league, expiry)
- `cache/` — last scoreboard/summary JSON per league

No account. No telemetry.

### Ship order (data)

1. NFL scoreboard + summary  
2. CFB (same adapter, other slug; conference filter later)  
3. NBA, NHL, MLB, CBB as more tabs; meter widgets per sport when that tab ships  

## Architecture

```text
config + pins
    ↓
app state (current tab, selection, pager, layout)
    ↓
provider (ESPN)  →  cache  →  Vec<Game>
    ↓
packer (Home vs league, density)
    ↓
tile widget (Ratatui)
```

Layers:

- **domain** — `Game`, `Team`, `Play`, `League`, `Meter`. No ESPN types leak here.
- **provider** — HTTP, map JSON → domain, cache, poll.
- **tiles** — packer + tile render + logos.
- **app** — tabs, keys, settings.

ESPN field names stay in `provider`. If the host changes, only that crate/module moves.

## Keys (v1)

| Key | Action |
|---|---|
| Tab / `h` `l` | League / Home tabs |
| `j` `k` | Move among tiles / slate |
| Space | Pin / unpin to Home |
| Enter | Full density / focus that game |
| `t` | Favorite teams |
| `n` `p` | Pager when >4 live/pins |
| `1` `2` `4` `s` | Force 1 / 2 / 4 / sidebar (optional; `auto` default) |
| `r` | Refresh now |
| `q` | Quit |

Footer always shows the relevant chords.

## Errors

- Network / 403: keep cache, mark stale, retry with backoff  
- Unknown logo: abbr fallback  
- Empty week: “next kickoff” / slate of upcoming, not a crash  
- Terminal too small: compact tiles, then a one-line “need more columns” if under a hard minimum (~40×12)

## Testing

- Fixture JSON (captured ESPN) → `Game` mapping  
- Packer: 1, 2, 4, 5+ pins, sidebar width, live-vs-upcoming priority  
- Ratatui `TestBackend` snapshots of a tile at each density  
- No network in CI  

## Implementation slices (after plan)

1. Domain + fixture `Game` + one tile at three densities (no network)  
2. Packer + Home with fake games (1/2/4/sidebar)  
3. ESPN NFL scoreboard + poll + cache  
4. Summary → last plays on visible live tiles; ticker  
5. Pins, favorites, config, league tab (NFL)  
6. NFL logos to spec  
7. CFB tab  
8. Homebrew / README GIF for kickoff  

## Risks

- ESPN may block or reshape JSON. Mitigation: provider trait, cache, User-Agent, documented fallback host.  
- Logo set is labor. Mitigation: spec first, abbr fallback, NFL 32 before CFB.  
- Chromostereopsis / high-contrast color can strain eyes. Mitigation: warm/cool as hierarchy, not #FF0000 on #0000FF as the theme; respect terminal contrast.

## Decisions locked

| Topic | Decision |
|---|---|
| Stack | Rust + Ratatui |
| Data | ESPN unofficial, `site.web.api.espn.com` |
| Home | Mixed pins, any sport |
| League tabs | Live tiles + slate; togglable |
| Tile | One widget; meter slot per sport |
| Logos | Yes; cell spec; NFL first |
| Layout | Auto 1/2/4 + sidebar; override stored |
| Auth | None |
| First sport | NFL, then CFB |
