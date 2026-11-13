# Security

## Reporting a vulnerability

Report it privately through GitHub: open the Security tab of this repository and use the "Report a vulnerability" button. Please do not open a public issue for a security problem.

gameday has one maintainer. There is no bug bounty and no guaranteed response time; I will reply as soon as I can.

## Supported versions

Only the latest release, as published on crates.io and the Homebrew tap (`WallyMagill/tap/gameday`). Fixes ship as a new patch release.

## What gameday touches

- Network: HTTPS GET requests to one host, `site.web.api.espn.com` (scoreboards, game summaries, standings). It follows HTTPS redirects from that host, up to 10 in a row (the default of its HTTP library, ureq), and never follows one to plain HTTP. No account, no API key, no cookies.
- Proxy: it reads these environment variables in this order and uses the first one that holds a proxy URL it can parse: `ALL_PROXY`, `all_proxy`, `HTTPS_PROXY`, `https_proxy`, `HTTP_PROXY`, `http_proxy`. That proxy carries every request, HTTPS included, so `HTTP_PROXY` alone routes gameday's HTTPS traffic. Hosts listed in `NO_PROXY` (or `no_proxy` when `NO_PROXY` is unset) skip it.
- Files: it writes only inside its config directory (`~/.config/gameday`, or `$XDG_CONFIG_HOME/gameday`, or the path given to `--config-dir`): `config.toml`, `pins.json`, `gameday.log`, and `cache/`, which holds the ESPN payloads and their ETags. Every file but `gameday.log` is replaced by writing a temp file beside it (`.<name>.<pid>-<n>.tmp`) and renaming it over the original; when the file is a symlink, both land beside the file it points to, which can be outside the config directory. It reads theme files from `themes/` in the same directory. Older macOS installs are read from `~/Library/Application Support/gameday`.
- Exceptions: `gameday --demo` keeps its state in a scratch directory under the system temp directory unless `--config-dir` is given. The developer commands `dump`, `frame` and `probe` write to `out/` in the current directory, to the path given to `--out`, or to scratch directories under the system temp directory, and `dump` starts headless Google Chrome if it is installed on macOS.
- Processes: to show a desktop notification it starts `osascript` on macOS or `notify-send` on Linux. Notification text is passed as an AppleScript string literal (escaped) or as command arguments, never through a shell.
- Terminal: it draws text taken from ESPN payloads (team names, play descriptions, headlines) in the terminal, and prints it to stdout with `--once`.
