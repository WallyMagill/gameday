//! Input-mode state machine: every key enters here. Normal mode delegates to
//! the existing `App::on_key`; `:` and `/` open the Command/Filter prompts,
//! whose buffers live in `InputMode` so the footer can render them directly.

use crate::app::{App, Tab};
use crate::command::{self, Cmd};
use crate::config::{save_pins, Pin};
use crate::domain::League;
use crate::theme;
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
    match &mut app.mode {
        InputMode::Normal => {
            // The help overlay is modal: while it's open, ':' and '/' are as
            // inert as every other non-help key, so let on_key swallow them.
            match code {
                KeyCode::Char(':') if !app.help_open => {
                    app.status_line = None;
                    app.mode = InputMode::Command { buf: String::new() };
                }
                KeyCode::Char('/') if !app.help_open => {
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
            // Task 2 wires Enter to commit the filter; for now both leave.
            KeyCode::Esc | KeyCode::Enter => app.mode = InputMode::Normal,
            KeyCode::Backspace => {
                if buf.pop().is_none() {
                    app.mode = InputMode::Normal;
                }
            }
            KeyCode::Char(c) => buf.push(c),
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

/// Apply a parsed command to the app. View-opening commands whose views land
/// later in this plan report that honestly instead of silently no-opping.
fn apply(app: &mut App, cmd: Cmd) {
    match cmd {
        Cmd::GoLeague(league) => go_league(app, league),
        Cmd::GoHome => app.set_tab(Tab::Home),
        // Task 3 introduces the View enum these route into.
        Cmd::Plays => app.status_line = Some("plays view not built yet (coming in v2)".into()),
        Cmd::Standings(_) => {
            app.status_line = Some("standings view not built yet (coming in v2)".into())
        }
        Cmd::ConfigView => {
            app.status_line = Some("config view not built yet (coming in v2)".into())
        }
        Cmd::Theme(name) => {
            theme::set_current(name);
            app.config.theme = name.as_str().to_string();
            let _ = app.config.save_to(&app.config_dir);
        }
        Cmd::Score(style) => {
            app.config.score_style = style;
            let _ = app.config.save_to(&app.config_dir);
        }
        Cmd::Layout(pref) => {
            app.config.layout = pref;
            let _ = app.config.save_to(&app.config_dir);
        }
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
        let _ = save_pins(&app.config_dir, &app.pins);
    }
    app.status_line = Some(label);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::domain::*;
    use crate::tiles::packer::LayoutPref;
    use crate::tiles::ScoreStyle;

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
            start_time: None,
            broadcast: None,
        }
    }

    fn mk() -> App {
        let dir = std::env::temp_dir().join(format!("gd-input-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let mut app = App::new(Config::default_all(), vec![], dir);
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
        use crate::theme::ThemeName;
        theme::set_current(ThemeName::Broadcast);
        let dir = std::env::temp_dir().join(format!("gd-input-theme-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let mut app = App::new(Config::default_all(), vec![], dir);
        handle_key(&mut app, KeyCode::Char(':'), KeyModifiers::NONE);
        type_line(&mut app, "theme phosphor");
        handle_key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(theme::current_name(), ThemeName::Phosphor);
        assert_eq!(app.config.theme, "phosphor");
        let saved = Config::load_from(&app.config_dir).unwrap();
        assert_eq!(saved.theme, "phosphor");
        theme::set_current(ThemeName::Broadcast);
    }

    #[test]
    fn score_and_layout_commands_persist() {
        let mut app = mk();
        handle_key(&mut app, KeyCode::Char(':'), KeyModifiers::NONE);
        type_line(&mut app, "score compact");
        handle_key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(app.config.score_style, ScoreStyle::Compact);
        handle_key(&mut app, KeyCode::Char(':'), KeyModifiers::NONE);
        type_line(&mut app, "layout 2");
        handle_key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(app.config.layout, LayoutPref::Two);
        let saved = Config::load_from(&app.config_dir).unwrap();
        assert_eq!(saved.score_style, ScoreStyle::Compact);
        assert_eq!(saved.layout, LayoutPref::Two);
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
