# gameday

Terminal sports board. Pin games, they tile. NFL first, college football when you turn the tab on.

No account. No API key.

## Run

```bash
cargo run --release
cargo run -- --demo    # fake NFL slate, no network
```

Config: `~/.config/gameday/config.toml`

```toml
enabled_tabs = ["Nfl", "Cfb"]   # default: ["Nfl"]
layout = "Auto"                 # Auto | One | Two | Four | Sidebar
```

Favorites and pins live beside that file. Pins drop 6 hours after a game goes final.

## Keys

space pin/unpin · enter focus · esc unfocus · j/k move · n/p page · t fav home team · 1/2/4/s layout · tab/h/l tabs · r refresh · q quit

## Data

Unofficial ESPN JSON (`site.web.api.espn.com`). Polling, not a websocket. Last good payload stays on disk; the board shows `stale` rather than going blank.

Not affiliated with ESPN or the NFL.
