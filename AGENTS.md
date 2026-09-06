# gameday

Terminal sports board in Rust (ratatui). One ranked list: the game most worth
watching leads it. Nine leagues from unofficial ESPN JSON, polled every 15s
while anything is live. No account, no key.

## Build, test, run

- `cargo build --release` → `target/release/gameday`
- `cargo test` (fast; no network, no sleeps) · `cargo clippy --all-targets -- -D warnings` · `cargo fmt --check`
- `gameday --demo` scripted slate, no network · `gameday dump` capture gallery into `out/` · `gameday frame …` one surface (see `docs/dev.md`)
- Config lives in `~/.config/gameday/` (`--config-dir` overrides). Tests never touch it: they use temp dirs.

## Standing rules (the reasons, not the ruling numbers)

- **Structure over parsing.** Read the field ESPN publishes; never derive state from prose. Presentation-only normalization is allowed and labeled.
- **The board reorders only on a real event** (a score, a status change, a chip flipping, a leverage band crossing). A clock that merely advanced never moves a row. New ranking inputs enter the fingerprint set deliberately.
- **One score formatter; logos never move a digit.** The hero and the takeover agree on every digit cell.
- **Quadrant blocks only** (`▀▄█`) for digits and marks: every mono font has them; sextants tofu.
- **Numbers carry receipts.** A timeout, cap, or weight is measured or says it is a guess and why.
- **Failures name the value, the expectation, and the knob.**
- **No new request load without a measured budget line.** Scoreboards every 15s live / 60s idle; summaries for the zoomed game and one-shot catch-ups only.

## Design work

Looks are decided from rendered PNGs, never in prose: `docs/design-loop.md`. Specs and plans live in `docs/superpowers/`; decisions are recorded there as directions (what won, why, what reopens it).

## Repo hygiene

- Branch from `main`; the push, merge, tag, and publish are Walter's.
- `git config blame.ignoreRevsFile .git-blame-ignore-revs` (the rustfmt commit).
- Before any `gh` command: `gh auth switch --user WallyMagill` (this is a personal repo; the machine's default `gh` account is the work one). Switch back afterwards.
- Release procedure: `docs/dev.md`.
