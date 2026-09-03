# gameday

Terminal sports board. One ranked list — the game most worth watching leads it, and the rest fall in behind. NFL first, college football when you turn the tab on.

No account. No API key.

## Run

```bash
cargo run --release
cargo run -- --demo    # scripted demo slate, no network
cargo run -- dump      # capture gallery into out/ (no network; --tick N picks the sim frame)
```

`dump` writes fixed names: the ranked board in each built-in theme
(`board-broadcast`, `board-studio`, `board-gruvbox`) and at two more sizes
(`board-narrow` 80x24, `board-sixty` 60x40); the surfaces `tv`, `cut-full`,
`cut-band`, `zoom`, `plays-feed`, `standings`, `config`, `filter`,
`theme-picker`, `help`; the four state captures `home-live` (first boot),
`offline` (no board, failed fetch), `stale` (board from cache) and
`config-error` (unparseable config.toml); and `nudge-seq-1/-2/-3`, three
frames around the scripted re-sort showing the `↑n` gutter appear and hold.
Each is written as `.html` + `.ansi`, plus `.png` when headless Chrome is
installed. The board draws its big scores and mark art from quadrant blocks
(`▀▄█`), which every mono font has; set
`GAMEDAY_DUMP_FONT=/path/to/CascadiaMono.ttf` if you want the PNGs' rules and
meter tracks to render as crisply as the terminal draws them.

Config: `~/.config/gameday/config.toml`

```toml
enabled_tabs = ["Nfl", "Cfb"]   # default: all nine leagues
theme = "broadcast"             # broadcast | studio | gruvbox, or a file in themes/
sort = "watch"                  # watch | time | league
```

## Themes

Three built-ins: **broadcast** (the default — amber scores, colored chrome),
**studio** (the press box: grayscale plus exactly one red, white scores),
and **gruvbox** (the one warm-ground community palette). A theme is a palette
read through *roles* — `ground`, `ink`, `dim`, `digits`, `hot`, `cool` and a
`team` scope saying where team color is allowed — so two themes can share
every hue and still be two looks.

Drop a `.toml` in `<config-dir>/themes/` and it loads at startup; a file that
names a built-in replaces it. The eight palettes that used to be built in —
`ceefax`, `phosphor`, `tokyo-night`, `nord`, `catppuccin-mocha`, `rose-pine`,
`everforest`, `dracula` — still ship in `assets/themes/` and still load as
user files, so `theme = "nord"` keeps working once you copy that file across.
A broken file is skipped with a line naming the file, the key and the
expected form; the board keeps running.

Old `layout` and `score_style` keys from before v3.2 are ignored if present — they
no longer do anything and are not written back.

Favorites and pins live beside that file (`pins.json`), along with `gameday.log` — where notes like a skipped malformed game go once the board owns the terminal. Pins drop 6 hours after a game goes final.
`$XDG_CONFIG_HOME` is honored, and `--config-dir <path>` overrides both.
Older installs on macOS: the app reads `~/Library/Application Support/gameday` until you move it.
A config.toml that doesn't parse is reported with its line and left alone — the board runs on defaults and saves nothing until you fix it.

## Keys

space pin/unpin · enter/z zoom · esc back · j/k move · [ ] date · / filter · : command · ? keys · t fav home team · s sort · v tv · c theme · tab/h/l tabs · r refresh · q quit

While a score is on screen — the two-row band or the full takeover — enter
jumps to *that* game instead of the selected one (the band says so itself:
`enter jump · clears in 3s`). It goes back to zooming the selection the moment
the cut clears.

## Data

Unofficial ESPN JSON (`site.web.api.espn.com`). Polling, not a websocket. Last good payload stays on disk; the board shows `stale` rather than going blank. Every row shows the scoreboard's last play; the full play-by-play and box score are fetched only for the game you zoom (`z`), which keeps polling to ~45 requests/min with nine leagues live.

Not affiliated with ESPN or the NFL.
