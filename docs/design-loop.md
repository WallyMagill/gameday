# The design loop

How visual decisions get made in this repo. Written from the loop v3.2 and
v3.3 actually ran (see the `## Decisions` sections of the specs in
`docs/superpowers/specs/`) — this file is that loop, made cheap enough to run
without editing code.

Walter's standing rule is the whole reason for it: **never make him choose a
visual in words.** Build the real thing, render it, and let him look.

## The render gate

A task is a RENDER GATE when its outcome is a look rather than a behavior:
digit shapes, a band's geometry, a theme's ground, a screen's balance. A gate
is not decided in prose. It produces labeled PNGs, and Walter picks a letter.

1. **Build every option for real.** An option is either code on a branch or a
   `gameday frame` invocation — never a description. The option Walter can't
   see is a decision you made for him silently.
2. **Label by filename.** `out/design/<task>-<option>.png`:
   `tv-digits-a.png`, `tv-digits-b.png`. The filename IS the option's name in
   the menu, in the contact sheet's caption, and in the decision record.
3. **Batch to at most two sittings.** One mid-project, one at the end. A
   sitting is a lettered menu per item, with the PNGs opened. Interrupting
   for one frame at a time is the failure mode this protocol exists to stop.
4. **Present a lettered menu.** One item per decision, each with a short
   recommendation. Walter answers "A", or "remix A and C".
5. **A reject comes back as new directions**, not a refinement of the thing
   he rejected — and the rejected code is reverted, not left commented out.
6. **Record the decision as a direction**, in the spec's `## Decisions`:
   what won, why, and what would reopen it. Never as a ruling.

## Rendering an option

`gameday dump` writes the fixed 22-stem gallery — that is the regression
gallery, not the design surface. Don't add stems to it for a gate.

`gameday frame` renders exactly one surface, parameterized:

```
cargo run -- frame --view tv --theme studio --size 80x24 \
                   --scenario redzone --tick 40 \
                   --out out/design/tv-studio-80.png
```

| flag | values |
|---|---|
| `--view` | `board tv zoom cut-full cut-band standings plays config help theme-picker filter` |
| `--theme` | any loaded theme name, **or a path to a theme `.toml`** — a candidate palette renders without joining `BUILTIN_NAMES` |
| `--size` | `WxH`, default `120x36`, range `40x12`–`400x200` |
| `--scenario` | `full-slate redzone thin-slate finals-only empty nudge-resort` |
| `--tick` | sim tick; default is the scenario's own beat |
| `--out` | the PNG path; `.ansi` and `.html` land beside it |

Everything is deterministic — frozen clock, pure sim ticks, no timestamps in
names — so the same command always produces the same bytes, and two options
differ only by what you changed.

Both `dump` and `frame` run the *same* setup functions (`dump::setup`), so a
frame is never a lookalike of the surface: it is the surface.

## Presenting a sitting

Render the options, then montage them:

```
tools/contact-sheet.sh out/design            # → out/design/contact-sheet.png
tools/contact-sheet.sh out/design sheet.png 2
```

Open the sheet and the individual PNGs, then give the lettered menu. One
image beats eight file paths.

## After the sitting

- Ship the winner; revert the losers in the same session.
- Append the decisions to the spec, in the sitting's own words.
- Sweep `out/` — it is gitignored, and stale frames from a decided gate are
  the fastest way to review the wrong thing next time.
