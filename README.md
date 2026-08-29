# gameday

Terminal sports board. Pin games, they tile. NFL first, college football when you turn the tab on.

No account. No API key.

## Run

```bash
cargo run --release
cargo run -- --demo    # scripted demo slate, no network
cargo run -- dump      # capture gallery into out/ (no network; --tick N picks the sim frame)
```

`dump` writes fixed names: `board-broadcast` / `board-ceefax` / `board-phosphor`
(home board per theme), `board-compact`, `tab-nfl`, `focus`, `help`, `narrow`
(80x24) — each as `.html` + `.ansi`, plus `.png` when headless Chrome is
installed. Set `GAMEDAY_DUMP_FONT=/path/to/CascadiaMono.ttf` so PNGs carry the
sextant glyphs.

Config: `~/.config/gameday/config.toml`

```toml
enabled_tabs = ["Nfl", "Cfb"]   # default: all nine leagues
layout = "Auto"                 # Auto | One | Two | Four | Sidebar
theme = "broadcast"             # broadcast | ceefax | phosphor
score_style = "big"             # big (sextant digits) | compact (single row)
```

Favorites and pins live beside that file. Pins drop 6 hours after a game goes final.

## Keys

space pin/unpin · enter focus · esc unfocus · j/k move · n/p page · t fav home team · 1/2/4/s layout · c theme · tab/h/l tabs · r refresh · q quit

## Data

Unofficial ESPN JSON (`site.web.api.espn.com`). Polling, not a websocket. Last good payload stays on disk; the board shows `stale` rather than going blank.

Not affiliated with ESPN or the NFL.
