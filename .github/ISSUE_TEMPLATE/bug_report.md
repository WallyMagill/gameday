---
name: Bug report
about: Something on the board is wrong, crashes, or does not draw correctly
labels: bug
---

**Version:** (output of `gameday --version`)

**OS and terminal:** (for example: macOS 15, Ghostty; say whether you run it inside tmux)

**Installed with:** brew / cargo / shell installer / binary from Releases

**What you saw:**

**What you expected:**

**League and game (for a data problem):** (for example: NFL, KC at BUF, and the date)

**Last lines of `gameday.log`:** it is in the config directory (`~/.config/gameday`, or the path you gave `--config-dir`). Run `tail -n 30 ~/.config/gameday/gameday.log` and paste the output.

```
```

Most data problems reproduce without the TUI: `gameday --once --league <league>` (for example `--league nfl`). If that output shows the problem too, paste it here.
