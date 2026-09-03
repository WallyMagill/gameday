//! Input-mode state machine: every key enters here. Normal mode delegates to
//! the existing `App::on_key`; `:` and `/` open the Command/Filter prompts,
//! whose buffers live in `InputMode` so the footer can render them directly.

use crate::app::{App, Tab};
use crate::command::{self, Cmd};
use crate::config::Pin;
use crate::domain::League;
use crate::theme;
use crate::views::View;
use crossterm::event::{KeyCode, KeyModifiers};

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum InputMode {
    #[default]
    Normal,
    Command {
        buf: String,
    },
    Filter {
        buf: String,
    },
}

/// Tab-completion cursor: the stem the user actually typed plus which of its
/// completions the buffer currently shows. Cleared by any non-Tab key so a
/// fresh Tab always completes what's really in the buffer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompletionState {
    pub stem: String,
    pub idx: usize,
}

pub fn handle_key(app: &mut App, code: KeyCode, mods: KeyModifiers) {
    // Ctrl+C quits from any mode — raw mode swallows SIGINT.
    if mods.contains(KeyModifiers::CONTROL) && code == KeyCode::Char('c') {
        app.should_quit = true;
        return;
    }
    // Ctrl+J is Enter (its terminal meaning: LF). Piped input relies on it:
    // bytes queued before raw mode go through the pty's ICRNL (\r -> \n), and
    // crossterm in raw mode parses \n as Ctrl+J rather than Enter.
    let code = if mods.contains(KeyModifiers::CONTROL) && code == KeyCode::Char('j') {
        KeyCode::Enter
    } else {
        code
    };
    match &mut app.mode {
        InputMode::Normal => {
            // The help overlay and the theme picker are modal: while either
            // is open, ':' and '/' are as inert as every other non-overlay
            // key, so let on_key swallow them. (A command run from inside
            // the picker could pop it without reverting the preview.)
            let modal = app.help_open || app.view == View::ThemePicker;
            match code {
                KeyCode::Char(':') if !modal => {
                    app.status_line = None;
                    app.mode = InputMode::Command { buf: String::new() };
                }
                KeyCode::Char('/') if !modal => {
                    app.status_line = None;
                    app.mode = InputMode::Filter { buf: String::new() };
                }
                _ => {
                    app.status_line = None;
                    app.on_key(code, mods);
                }
            }
        }
        InputMode::Command { buf } => match code {
            KeyCode::Esc => {
                app.completion = None;
                app.mode = InputMode::Normal;
            }
            KeyCode::Enter => {
                let line = std::mem::take(buf);
                app.completion = None;
                app.mode = InputMode::Normal;
                match command::parse(&line) {
                    Ok(cmd) => apply(app, cmd),
                    Err(err) => app.status_line = Some(err),
                }
            }
            KeyCode::Tab => cycle_completion(app),
            KeyCode::Backspace => {
                app.completion = None;
                // Backspacing past the start leaves the prompt (vim habit).
                if buf.pop().is_none() {
                    app.mode = InputMode::Normal;
                }
            }
            KeyCode::Char(c) => {
                buf.push(c);
                app.completion = None;
            }
            _ => {}
        },
        InputMode::Filter { buf } => match code {
            // Esc abandons the prompt AND any committed filter — one gesture
            // back to the full board.
            KeyCode::Esc => {
                app.mode = InputMode::Normal;
                app.filter = None;
                app.filter_changed();
            }
            // Enter commits the buffer; an empty pattern clears the filter.
            KeyCode::Enter => {
                let pat = std::mem::take(buf);
                app.mode = InputMode::Normal;
                app.filter = (!pat.is_empty()).then_some(pat);
                app.filter_changed();
            }
            // The buffer filters incrementally, so every edit re-clamps the
            // selection against the narrowed list.
            KeyCode::Backspace => {
                if buf.pop().is_none() {
                    app.mode = InputMode::Normal;
                }
                app.filter_changed();
            }
            KeyCode::Char(c) => {
                buf.push(c);
                app.filter_changed();
            }
            _ => {}
        },
    }
}

/// Tab in Command mode: first press completes the typed stem, further presses
/// cycle the stem's matches in registry order, wrapping.
fn cycle_completion(app: &mut App) {
    let InputMode::Command { buf } = &app.mode else {
        return;
    };
    let stem = app
        .completion
        .as_ref()
        .map(|s| s.stem.clone())
        .unwrap_or_else(|| buf.clone());
    let matches = command::complete(&stem);
    if matches.is_empty() {
        return;
    }
    let idx = match &app.completion {
        Some(state) => (state.idx + 1) % matches.len(),
        None => 0,
    };
    if let InputMode::Command { buf } = &mut app.mode {
        buf.clear();
        buf.push_str(&matches[idx]);
    }
    app.completion = Some(CompletionState { stem, idx });
}

/// Apply a parsed command to the app.
fn apply(app: &mut App, cmd: Cmd) {
    match cmd {
        Cmd::GoLeague(league) => go_league(app, league),
        Cmd::GoHome => app.set_tab(Tab::Home),
        Cmd::Plays => {
            app.view = View::PlaysFeed;
            app.feed_scroll = 0;
        }
        // `:standings` with no league: the current tab's league, or NFL from
        // Home (the plan's Task 6 contract). The poll loop watches
        // `app.standings_target()` and fetches when this view opens.
        Cmd::Standings(league) => {
            let league = league.unwrap_or(match app.tab {
                Tab::League(l) => l,
                Tab::Home => League::Nfl,
            });
            app.view = View::Standings(league);
            app.standings_scroll = 0;
        }
        Cmd::ConfigView => {
            app.view = View::ConfigView;
            app.config_cursor = 0;
            app.config_edit = None;
        }
        // `:theme` alone opens the live-preview picker; `:theme <name>` (the
        // parser already resolved it to a loaded canonical name) applies and
        // persists at once.
        Cmd::Theme(None) => app.open_theme_picker(),
        Cmd::Theme(Some(name)) => match theme::set_current(&name) {
            Ok(canonical) => {
                app.config.theme = canonical;
                app.persist_config();
            }
            Err(err) => app.status_line = Some(err),
        },
        // `:sort` alone cycles; `:sort <key>` sets directly. Either way the
        // board must re-derive its order immediately (a sort-key change is
        // an event, not something the next score tick should gate).
        Cmd::Sort(key) => {
            let next = key.unwrap_or_else(|| app.config.sort.cycled());
            app.config.sort = next;
            app.force_reorder();
            // Same composition as `App::cycle_sort` ('s'): a config error
            // blocking the save must not be clobbered by a claimed success.
            app.status_line = None;
            app.persist_config();
            let save_error = app.status_line.take();
            app.status_line = Some(match (&app.config_error, save_error) {
                (Some(_), _) => format!(
                    "sort {} · not saving (config error)",
                    next.label().to_ascii_lowercase()
                ),
                (None, Some(err)) => err,
                (None, None) => format!("sort {}", next.label().to_ascii_lowercase()),
            });
        }
        // Both entry paths seed the shown game and clear any lock: a `:tv`
        // that only set the view reopened still locked on a game from the
        // last visit, with no event able to move the screen.
        Cmd::Tv => app.open_tv(),
        Cmd::Pin(abbr) => pin_team(app, &abbr),
        Cmd::Quit => app.should_quit = true,
    }
}

fn go_league(app: &mut App, league: League) {
    if !app.config.enabled_tabs.contains(&league) {
        let enabled = app
            .config
            .enabled_tabs
            .iter()
            .map(|l| l.slug())
            .collect::<Vec<_>>()
            .join("|");
        app.status_line = Some(format!(
            "league \"{}\" not enabled, enabled: {enabled}",
            league.slug()
        ));
        return;
    }
    app.set_tab(Tab::League(league));
}

/// `:pin kc` — find the team's current game on any enabled board and pin it.
fn pin_team(app: &mut App, abbr: &str) {
    let found = app.config.enabled_tabs.iter().find_map(|league| {
        app.boards.get(league).into_iter().flatten().find(|g| {
            g.away.abbr.eq_ignore_ascii_case(abbr) || g.home.abbr.eq_ignore_ascii_case(abbr)
        })
    });
    let Some(game) = found else {
        app.status_line = Some(format!(
            "no game found for {abbr:?} on enabled boards, try a team abbr like kc"
        ));
        return;
    };
    let (id, league) = (game.id.clone(), game.league);
    let label = format!("pinned {}@{}", game.away.abbr, game.home.abbr);
    if !app.pins.iter().any(|p| p.game_id == id) {
        app.pins.push(Pin {
            game_id: id,
            league,
            final_at: None,
        });
        app.persist_pins();
    }
    app.status_line = Some(label);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::domain::*;
    use crate::rank::SortKey;

    fn team(abbr: &str) -> Team {
        Team {
            id: abbr.into(),
            abbr: abbr.into(),
            name: abbr.into(),
            ..Default::default()
        }
    }

    fn g(id: &str, away: &str, home: &str) -> Game {
        Game {
            id: id.into(),
            league: League::Nfl,
            away: team(away),
            home: team(home),
            away_score: 7,
            home_score: 3,
            status: Status::Live,
            period: "Q2".into(),
            clock: "5:00".into(),
            situation: None,
            last_plays: vec![],
            meter: None,
            ..Game::default()
        }
    }

    fn mk() -> App {
        let dir = std::env::temp_dir().join(format!("gd-input-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let mut app = App::new(Config::default_all(), vec![], dir, time::UtcOffset::UTC);
        app.apply_boards(League::Nfl, vec![g("1", "KC", "TB")], false);
        app
    }

    fn type_line(app: &mut App, line: &str) {
        for c in line.chars() {
            handle_key(app, KeyCode::Char(c), KeyModifiers::NONE);
        }
    }

    #[test]
    fn colon_enters_command_mode_and_esc_leaves() {
        let mut app = mk();
        handle_key(&mut app, KeyCode::Char(':'), KeyModifiers::NONE);
        assert_eq!(app.mode, InputMode::Command { buf: String::new() });
        type_line(&mut app, "nf");
        assert_eq!(app.mode, InputMode::Command { buf: "nf".into() });
        handle_key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        assert_eq!(app.mode, InputMode::Normal);
        assert!(!app.should_quit, "Esc leaves the mode, nothing else");
    }

    #[test]
    fn slash_enters_filter_mode_and_backspace_past_start_leaves() {
        let mut app = mk();
        handle_key(&mut app, KeyCode::Char('/'), KeyModifiers::NONE);
        type_line(&mut app, "k");
        assert_eq!(app.mode, InputMode::Filter { buf: "k".into() });
        handle_key(&mut app, KeyCode::Backspace, KeyModifiers::NONE);
        assert_eq!(app.mode, InputMode::Filter { buf: String::new() });
        handle_key(&mut app, KeyCode::Backspace, KeyModifiers::NONE);
        assert_eq!(app.mode, InputMode::Normal);
    }

    #[test]
    fn enter_commits_the_filter_and_esc_clears_it() {
        let mut app = mk();
        handle_key(&mut app, KeyCode::Char('/'), KeyModifiers::NONE);
        type_line(&mut app, "kc");
        handle_key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(app.mode, InputMode::Normal);
        assert_eq!(app.filter.as_deref(), Some("kc"));
        // Esc inside a reopened prompt drops the committed filter too.
        handle_key(&mut app, KeyCode::Char('/'), KeyModifiers::NONE);
        handle_key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        assert_eq!(app.filter, None);
        assert_eq!(app.mode, InputMode::Normal);
    }

    #[test]
    fn committing_an_empty_pattern_clears_the_filter() {
        let mut app = mk();
        app.filter = Some("kc".into());
        handle_key(&mut app, KeyCode::Char('/'), KeyModifiers::NONE);
        handle_key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(app.filter, None, "empty pattern clears");
        assert_eq!(app.mode, InputMode::Normal);
    }

    #[test]
    fn enter_runs_the_command_league_jump() {
        let mut app = mk();
        handle_key(&mut app, KeyCode::Char(':'), KeyModifiers::NONE);
        type_line(&mut app, "nfl");
        handle_key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(app.mode, InputMode::Normal);
        assert_eq!(app.tab, Tab::League(League::Nfl));
        assert!(app.status_line.is_none());
        // :home returns.
        handle_key(&mut app, KeyCode::Char(':'), KeyModifiers::NONE);
        type_line(&mut app, "home");
        handle_key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(app.tab, Tab::Home);
    }

    #[test]
    fn unknown_command_sets_status_line_and_next_key_clears_it() {
        let mut app = mk();
        handle_key(&mut app, KeyCode::Char(':'), KeyModifiers::NONE);
        type_line(&mut app, "foo");
        handle_key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        let status = app.status_line.clone().expect("error status");
        assert!(status.contains("\"foo\"") && status.contains("nfl"), "{status}");
        // Any Normal-mode key dismisses the status.
        handle_key(&mut app, KeyCode::Char('j'), KeyModifiers::NONE);
        assert!(app.status_line.is_none());
    }

    #[test]
    fn theme_command_sets_and_persists() {
        theme::set_current("broadcast").unwrap();
        let dir = std::env::temp_dir().join(format!("gd-input-theme-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let mut app = App::new(Config::default_all(), vec![], dir, time::UtcOffset::UTC);
        handle_key(&mut app, KeyCode::Char(':'), KeyModifiers::NONE);
        type_line(&mut app, "theme gruvbox");
        handle_key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(theme::current_name(), "gruvbox");
        assert_eq!(app.config.theme, "gruvbox");
        let saved = Config::load_from(&app.config_dir).unwrap();
        assert_eq!(saved.theme, "gruvbox");
        // Case-insensitive, persisted canonical.
        handle_key(&mut app, KeyCode::Char(':'), KeyModifiers::NONE);
        type_line(&mut app, "theme StUdIo");
        handle_key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(theme::current_name(), "studio");
        assert_eq!(Config::load_from(&app.config_dir).unwrap().theme, "studio");
        theme::set_current("broadcast").unwrap();
    }

    #[test]
    fn bare_theme_command_opens_the_picker_and_esc_reverts_enter_keeps() {
        theme::set_current("broadcast").unwrap();
        let dir = std::env::temp_dir().join(format!("gd-input-picker-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);
        let mut app = App::new(Config::default_all(), vec![], dir, time::UtcOffset::UTC);
        handle_key(&mut app, KeyCode::Char(':'), KeyModifiers::NONE);
        type_line(&mut app, "theme");
        handle_key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(app.view, View::ThemePicker);
        assert_eq!(app.theme_cursor, 0, "cursor starts on the current theme");
        // j previews live: the current theme changes with the cursor.
        handle_key(&mut app, KeyCode::Char('j'), KeyModifiers::NONE);
        handle_key(&mut app, KeyCode::Char('j'), KeyModifiers::NONE);
        assert_eq!(theme::current_name(), "gruvbox", "j/j previews the third theme");
        assert_eq!(app.config.theme, "broadcast", "preview is not persisted");
        // Esc reverts to what was current when the picker opened.
        handle_key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        assert_eq!(app.view, View::Board);
        assert_eq!(theme::current_name(), "broadcast");
        assert!(!std::path::Path::new(&app.config_dir).join("config.toml").exists(), "nothing saved");
        // Enter commits + persists.
        handle_key(&mut app, KeyCode::Char(':'), KeyModifiers::NONE);
        type_line(&mut app, "theme");
        handle_key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        handle_key(&mut app, KeyCode::Char('k'), KeyModifiers::NONE);
        assert_eq!(theme::current_name(), "gruvbox", "k wraps to the last theme");
        handle_key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(app.view, View::Board);
        assert_eq!(app.config.theme, "gruvbox");
        assert_eq!(Config::load_from(&app.config_dir).unwrap().theme, "gruvbox");
        // Reopening starts on the now-current theme, and q reverts like Esc.
        handle_key(&mut app, KeyCode::Char(':'), KeyModifiers::NONE);
        type_line(&mut app, "theme");
        handle_key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(app.theme_cursor, theme::names().len() - 1);
        handle_key(&mut app, KeyCode::Char('j'), KeyModifiers::NONE);
        assert_eq!(theme::current_name(), "broadcast", "j wraps to the top");
        handle_key(&mut app, KeyCode::Char('q'), KeyModifiers::NONE);
        assert_eq!(theme::current_name(), "gruvbox");
        assert!(!app.should_quit);
        theme::set_current("broadcast").unwrap();
    }

    #[test]
    fn picker_is_modal_colon_and_slash_cannot_leak_a_preview() {
        // Regression: `:nfl` / `:plays` / `/` from inside the picker popped it
        // without reverting, leaving the previewed theme live but unsaved.
        // Esc and Enter (and Tab, which reverts) are the only exits, so the
        // prompts are inert while the picker is up — like the help overlay.
        theme::set_current("broadcast").unwrap();
        let dir = std::env::temp_dir().join(format!("gd-input-modal-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);
        let mut app = App::new(Config::default_all(), vec![], dir, time::UtcOffset::UTC);
        handle_key(&mut app, KeyCode::Char(':'), KeyModifiers::NONE);
        type_line(&mut app, "theme");
        handle_key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        handle_key(&mut app, KeyCode::Char('j'), KeyModifiers::NONE);
        assert_eq!(theme::current_name(), "studio", "j previews");
        for key in [':', '/'] {
            handle_key(&mut app, KeyCode::Char(key), KeyModifiers::NONE);
            assert_eq!(app.mode, InputMode::Normal, "{key:?} opens no prompt over the picker");
            assert_eq!(app.view, View::ThemePicker, "{key:?} does not pop the picker");
            assert_eq!(theme::current_name(), "studio", "{key:?} leaves the preview alone");
        }
        // Esc still reverts; nothing was persisted along the way.
        handle_key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        assert_eq!(app.view, View::Board);
        assert_eq!(theme::current_name(), "broadcast");
        assert_eq!(app.config.theme, "broadcast");
        assert!(!app.config_dir.join("config.toml").exists(), "nothing saved");
        theme::set_current("broadcast").unwrap();
    }

    #[test]
    fn sort_and_tv_commands_persist_and_navigate() {
        let mut app = mk();
        handle_key(&mut app, KeyCode::Char(':'), KeyModifiers::NONE);
        type_line(&mut app, "sort time");
        handle_key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(app.config.sort, SortKey::Time);
        let saved = Config::load_from(&app.config_dir).unwrap();
        assert_eq!(saved.sort, SortKey::Time);
        // Bare `:sort` cycles from the current key.
        handle_key(&mut app, KeyCode::Char(':'), KeyModifiers::NONE);
        type_line(&mut app, "sort");
        handle_key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(app.config.sort, SortKey::League);
        // `:tv` enters the TV view.
        handle_key(&mut app, KeyCode::Char(':'), KeyModifiers::NONE);
        type_line(&mut app, "tv");
        handle_key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(app.view, View::Tv);
    }

    #[test]
    fn sort_command_with_a_broken_config_says_not_saving_not_success() {
        let mut app = mk();
        app.set_config_error(Some("config.toml:7: unknown variant `NFLL`".into()));
        handle_key(&mut app, KeyCode::Char(':'), KeyModifiers::NONE);
        type_line(&mut app, "sort time");
        handle_key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(app.config.sort, SortKey::Time, "the in-memory sort still changes");
        let line = app.status_line.clone().unwrap_or_default();
        assert!(
            line.starts_with("sort time") && line.ends_with("· not saving (config error)"),
            "{line:?}"
        );
    }

    #[test]
    fn pin_finds_the_game_across_boards_and_misses_report() {
        let mut app = mk();
        handle_key(&mut app, KeyCode::Char(':'), KeyModifiers::NONE);
        type_line(&mut app, "pin tb");
        handle_key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(app.pins.len(), 1);
        assert_eq!(app.pins[0].game_id, "1");
        assert_eq!(app.status_line.as_deref(), Some("pinned KC@TB"));
        // Pinning again is idempotent.
        handle_key(&mut app, KeyCode::Char(':'), KeyModifiers::NONE);
        type_line(&mut app, "pin KC");
        handle_key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(app.pins.len(), 1, "same game never pinned twice");
        // A miss names the abbr.
        handle_key(&mut app, KeyCode::Char(':'), KeyModifiers::NONE);
        type_line(&mut app, "pin zzz");
        handle_key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        let status = app.status_line.clone().expect("miss status");
        assert!(status.contains("\"zzz\""), "{status}");
        assert_eq!(app.pins.len(), 1);
    }

    #[test]
    fn quit_command_quits() {
        let mut app = mk();
        handle_key(&mut app, KeyCode::Char(':'), KeyModifiers::NONE);
        type_line(&mut app, "q");
        handle_key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert!(app.should_quit);
    }

    #[test]
    fn tab_completes_and_cycles() {
        let mut app = mk();
        handle_key(&mut app, KeyCode::Char(':'), KeyModifiers::NONE);
        type_line(&mut app, "st");
        handle_key(&mut app, KeyCode::Tab, KeyModifiers::NONE);
        assert_eq!(app.mode, InputMode::Command { buf: "standings".into() });
        // A single match cycles back onto itself.
        handle_key(&mut app, KeyCode::Tab, KeyModifiers::NONE);
        assert_eq!(app.mode, InputMode::Command { buf: "standings".into() });
        // Multiple matches cycle in registry order against the typed stem.
        handle_key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        handle_key(&mut app, KeyCode::Char(':'), KeyModifiers::NONE);
        type_line(&mut app, "n");
        handle_key(&mut app, KeyCode::Tab, KeyModifiers::NONE);
        assert_eq!(app.mode, InputMode::Command { buf: "nfl".into() });
        handle_key(&mut app, KeyCode::Tab, KeyModifiers::NONE);
        assert_eq!(app.mode, InputMode::Command { buf: "nba".into() });
        handle_key(&mut app, KeyCode::Tab, KeyModifiers::NONE);
        assert_eq!(app.mode, InputMode::Command { buf: "nhl".into() });
        handle_key(&mut app, KeyCode::Tab, KeyModifiers::NONE);
        assert_eq!(app.mode, InputMode::Command { buf: "nfl".into() }, "wraps");
        // Typing clears the cycle; completion then works on the new buffer.
        type_line(&mut app, " n");
        handle_key(&mut app, KeyCode::Tab, KeyModifiers::NONE);
        assert!(matches!(&app.mode, InputMode::Command { buf } if buf == "nfl n"),
            "no league args for a league jump: buffer unchanged");
    }

    #[test]
    fn colon_and_slash_stay_inert_while_help_is_open() {
        let mut app = mk();
        handle_key(&mut app, KeyCode::Char('?'), KeyModifiers::NONE);
        assert!(app.help_open);
        handle_key(&mut app, KeyCode::Char(':'), KeyModifiers::NONE);
        assert_eq!(app.mode, InputMode::Normal, "help is modal");
        handle_key(&mut app, KeyCode::Char('/'), KeyModifiers::NONE);
        assert_eq!(app.mode, InputMode::Normal);
        assert!(app.help_open);
    }

    #[test]
    fn ctrl_j_acts_as_enter_in_a_prompt() {
        // Piped input: the pty turns \r into \n before raw mode, and \n in
        // raw mode reaches us as Ctrl+J. It must still run the command.
        let mut app = mk();
        handle_key(&mut app, KeyCode::Char(':'), KeyModifiers::NONE);
        type_line(&mut app, "nfl");
        handle_key(&mut app, KeyCode::Char('j'), KeyModifiers::CONTROL);
        assert_eq!(app.mode, InputMode::Normal);
        assert_eq!(app.tab, Tab::League(League::Nfl));
    }

    #[test]
    fn ctrl_c_quits_from_any_mode() {
        let mut app = mk();
        handle_key(&mut app, KeyCode::Char(':'), KeyModifiers::NONE);
        handle_key(&mut app, KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert!(app.should_quit);
    }

    #[test]
    fn disabled_league_jump_reports_instead_of_jumping() {
        let mut app = mk();
        app.config.enabled_tabs = vec![League::Nfl];
        handle_key(&mut app, KeyCode::Char(':'), KeyModifiers::NONE);
        type_line(&mut app, "nba");
        handle_key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(app.tab, Tab::Home, "tab unchanged");
        let status = app.status_line.clone().expect("status");
        assert!(status.contains("\"nba\"") && status.contains("nfl"), "{status}");
    }
}
