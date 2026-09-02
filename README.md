# gameday

Terminal sports board. Pin games, they tile. NFL first, college football when you turn the tab on.

No account. No API key.

## Run

```bash
cargo run --release
cargo run -- --demo    # scripted demo slate, no network
cargo run -- dump      # capture gallery into out/ (no network; --tick N picks the sim frame)
```

`dump` writes fixed names: `board-<theme>` for every built-in theme
(`board-broadcast`, `board-studio`, `board-ceefax`, `board-phosphor`,
`board-gruvbox`, `board-tokyo-night`, `board-nord`, `board-catppuccin-mocha`,
`board-rose-pine`, `board-everforest`, `board-dracula`), `board-compact`,
`tab-nfl`, `focus`, `help`, `narrow`
(80x24), `zoom-stats`, `plays-feed`, `standings`, `config`, `filter`,
`theme-picker`, and the four state captures `home-live` (first boot),
`offline` (no board, failed fetch), `stale` (board from cache), and
`config-error` (unparseable config.toml) — each as `.html` + `.ansi`, plus
`.png` when headless Chrome is installed. Set `GAMEDAY_DUMP_FONT=/path/to/CascadiaMono.ttf` so PNGs carry the
sextant glyphs.

Config: `~/.config/gameday/config.toml`

```toml
enabled_tabs = ["Nfl", "Cfb"]   # default: all nine leagues
theme = "broadcast"             # any loaded theme: the 11 built-ins or a file in themes/
sort = "watch"                  # watch | time | league
```

Old `layout` and `score_style` keys from before v3.2 are ignored if present — they
no longer do anything and are not written back.

Favorites and pins live beside that file (`pins.json`), along with `gameday.log` — where notes like a skipped malformed game go once the board owns the terminal. Pins drop 6 hours after a game goes final.
`$XDG_CONFIG_HOME` is honored, and `--config-dir <path>` overrides both.
Older installs on macOS: the app reads `~/Library/Application Support/gameday` until you move it.
A config.toml that doesn't parse is reported with its line and left alone — the board runs on defaults and saves nothing until you fix it.

## Keys

space pin/unpin · enter/z zoom · esc back · j/k move · [ ] date · / filter · : command · ? keys · t fav home team · s sort · v tv · c theme · tab/h/l tabs · r refresh · q quit

## Data

Unofficial ESPN JSON (`site.web.api.espn.com`). Polling, not a websocket. Last good payload stays on disk; the board shows `stale` rather than going blank. Every tile shows the scoreboard's last play; the full play-by-play and box score are fetched only for the game you zoom (`z`), which keeps polling to ~45 requests/min with nine leagues live.

Not affiliated with ESPN or the NFL.
