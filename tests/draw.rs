use gameday::app::{App, Tab};
use gameday::config::Config;
use gameday::domain::*;
use ratatui::{backend::TestBackend, Terminal};

fn team(abbr: &str) -> Team {
    Team {
        id: abbr.into(),
        abbr: abbr.into(),
        name: abbr.into(),
        color: [200, 16, 46],
        alt_color: [255, 184, 28],
        logo_key: format!("nfl/{}", abbr.to_lowercase()),
        ..Default::default()
    }
}

fn g(id: &str, away: &str, home: &str, live: bool) -> Game {
    Game {
        id: id.into(),
        league: League::Nfl,
        away: team(away),
        home: team(home),
        away_score: 27,
        home_score: 24,
        status: if live { Status::Live } else { Status::Pre },
        period: "Q4".into(),
        clock: "1:27".into(),
        situation: Some(Situation {
            down_distance: "1st & Goal".into(),
            possession: Some(away.into()),
            ball_on: Some("TB 3".into()),
            ..Default::default()
        }),
        last_plays: vec![Play {
            clock: "1:27".into(),
            team: away.into(),
            text: "Mahomes pass to Kelce for 3 yards".into(),
            scoring: false,
        }],
        meter: None,
        start_time: Some("8:20 PM".into()),
        broadcast: Some("CBS".into()),
    }
}

fn buf_text(term: &Terminal<TestBackend>) -> String {
    let b = term.backend().buffer();
    let area = b.area();
    let mut s = String::new();
    for y in 0..area.height {
        for x in 0..area.width {
            s.push_str(b[(x, y)].symbol());
        }
        s.push('\n');
    }
    s
}

fn mk() -> App {
    let dir = std::env::temp_dir().join(format!("gd-draw-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    App::new(Config::default_all(), vec![], dir)
}

#[test]
fn header_and_tabs_render() {
    let mut app = mk();
    let mut t = Terminal::new(TestBackend::new(120, 24)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t);
    assert!(s.contains("GAMEDAY"), "{s}");
    assert!(s.contains("FILTER:"), "{s}");
    assert!(s.contains("[ALL]"), "{s}");
    assert!(s.contains("NFL"), "{s}");
}

#[test]
fn footer_shows_chords() {
    let mut app = mk();
    let mut t = Terminal::new(TestBackend::new(120, 24)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t);
    assert!(s.contains("NAV:"), "{s}");
    assert!(s.contains("[Q]"), "{s}");
    assert!(s.contains("[SPC]"), "{s}");
    assert!(s.contains("[?] HELP"), "{s}");
    // The whole chord list fits 120 cols: QUIT must not be clipped.
    assert!(s.contains("QUIT"), "{s}");
    // Theme moved out of the footer into help — the footer shows top chords only.
    assert!(!s.contains("[C] THEME"), "{s}");
}

#[test]
fn help_overlay_lists_every_group_and_the_hidden_chords() {
    let mut app = mk();
    app.help_open = true;
    let mut t = Terminal::new(TestBackend::new(120, 36)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t);
    for needle in [
        "KEYS", "NAVIGATION", "SELECTION", "VIEW", "APP",
        // Chords the footer omits must still be discoverable here.
        "THEME", "LAYOUT", "FAVORITE", "CTRL-C", "S-TAB",
    ] {
        assert!(s.contains(needle), "help overlay missing {needle:?}:\n{s}");
    }
}

#[test]
fn zoomed_footer_shows_back_and_the_zoomed_game() {
    use gameday::views::{View, ZoomTab};
    let mut app = mk();
    app.apply_boards(League::Nfl, vec![g("1", "KC", "TB", true)], false);
    app.tab = Tab::League(League::Nfl);
    app.view = View::Zoom { game_id: "1".into(), tab: ZoomTab::Overview };
    let mut t = Terminal::new(TestBackend::new(120, 24)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t);
    assert!(s.contains("[ESC] BACK"), "{s}");
    assert!(!s.contains("[Z] ZOOM"), "{s}");
    // 'q' pops here instead of quitting, so QUIT is not advertised.
    assert!(!s.contains("[Q] QUIT"), "{s}");
    assert!(s.contains("FOCUS KC@TB"), "{s}");
}

#[test]
fn footer_shows_freshness_age() {
    let mut app = mk();
    app.apply_boards(League::Nfl, vec![g("1", "KC", "TB", true)], false);
    app.tab = Tab::League(League::Nfl);
    let mut t = Terminal::new(TestBackend::new(120, 24)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t);
    assert!(s.contains("UPD 0s"), "{s}");
}

#[test]
fn league_tab_with_only_slate_games_fills_mosaic_and_highlights_selection() {
    let mut app = mk();
    app.apply_boards(
        League::Nfl,
        vec![g("1", "KC", "TB", false), g("2", "DAL", "PHI", false)],
        false,
    );
    app.tab = Tab::League(League::Nfl);
    // Tall enough for the slate strip: mosaic must NOT be blank above it.
    let mut t = Terminal::new(TestBackend::new(120, 36)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t);
    assert!(s.contains("SLATE"), "{s}");
    // Tiles render in the mosaic (tile chrome, not just slate rows).
    assert!(s.contains("KC"), "{s}");
    assert!(s.contains("DAL"), "{s}");
    // The selected slate row carries the accent marker.
    assert!(s.contains("▸"), "{s}");
}

#[test]
fn draw_reads_the_current_theme() {
    use gameday::theme::{self, Theme, ThemeName};
    use ratatui::style::Color;
    let bg_of = |name: ThemeName| -> Color {
        theme::set_current(name);
        let mut app = mk();
        let mut t = Terminal::new(TestBackend::new(120, 24)).unwrap();
        t.draw(|f| app.draw(f)).unwrap();
        t.backend().buffer()[(0, 0)].bg
    };
    assert_eq!(bg_of(ThemeName::Ceefax), Theme::ceefax().bg);
    assert_eq!(bg_of(ThemeName::Phosphor), Theme::phosphor().bg);
    assert_eq!(bg_of(ThemeName::Broadcast), Color::Rgb(0, 0, 0));
}

#[test]
fn sidebar_top_plays_right_align_the_clock() {
    let mut app = mk();
    let mut game = g("1", "KC", "TB", true);
    game.last_plays = vec![Play {
        clock: "1:27".into(),
        team: "KC".into(),
        text: "Mahomes pass to Kelce, 12 yd TOUCHDOWN".into(),
        scoring: true,
    }];
    app.apply_boards(League::Nfl, vec![game], false);
    app.tab = Tab::League(League::Nfl);
    let mut t = Terminal::new(TestBackend::new(120, 24)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t);
    let row = s
        .lines()
        .find(|l| l.contains("★"))
        .expect("TOP PLAYS row with a starred play");
    // The clock hugs the sidebar's right border; the long play text is
    // ellipsis-truncated, never hard-cut into it.
    assert!(row.trim_end().ends_with("1:27│"), "clock not right-aligned: {row:?}");
    assert!(row.contains("…"), "long play text should truncate with ellipsis: {row:?}");
}

#[test]
fn empty_home_prompt() {
    let mut app = mk();
    let mut t = Terminal::new(TestBackend::new(120, 24)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t).to_lowercase();
    assert!(s.contains("pin a game"), "{s}");
}

#[test]
fn too_small_message() {
    let mut app = mk();
    let mut t = Terminal::new(TestBackend::new(30, 10)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t).to_lowercase();
    assert!(s.contains("need more columns"), "{s}");
}

#[test]
fn nfl_tab_draws_live_score() {
    let mut app = mk();
    app.config.score_style = gameday::tiles::ScoreStyle::Compact;
    app.apply_boards(League::Nfl, vec![g("1", "KC", "TB", true)], false);
    app.tab = Tab::League(League::Nfl);
    let mut t = Terminal::new(TestBackend::new(120, 24)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t);
    assert!(s.contains("27"), "{s}");
    assert!(s.contains("KC"), "{s}");
    assert!(s.contains("LIVE"), "{s}");
}

#[test]
fn stale_flag_in_header() {
    let mut app = mk();
    app.apply_boards(League::Nfl, vec![g("1", "KC", "TB", true)], true);
    let mut t = Terminal::new(TestBackend::new(120, 24)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t).to_lowercase();
    assert!(s.contains("stale"), "{s}");
}

#[test]
fn zoomed_pre_game_on_nfl_tab_fills_the_body() {
    use gameday::views::{View, ZoomTab};
    let mut app = mk();
    app.config.score_style = gameday::tiles::ScoreStyle::Compact;
    app.apply_boards(League::Nfl, vec![g("1", "KC", "TB", false)], false);
    app.tab = Tab::League(League::Nfl);
    app.view = View::Zoom { game_id: "1".into(), tab: ZoomTab::Overview };
    let mut t = Terminal::new(TestBackend::new(120, 24)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t);
    assert!(s.contains("KC"), "{s}");
    assert!(s.contains("27"), "{s}");
}

#[test]
fn command_prompt_renders_in_the_footer_row() {
    use gameday::input::InputMode;
    let mut app = mk();
    app.mode = InputMode::Command { buf: "nf".into() };
    let mut t = Terminal::new(TestBackend::new(120, 24)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t);
    let footer = s.lines().nth(23).expect("footer row");
    assert!(footer.contains(":nf"), "prompt missing: {footer:?}");
    assert!(!footer.contains("NAV:"), "chords must yield to the prompt: {footer:?}");
}

#[test]
fn filter_prompt_renders_in_the_footer_row() {
    use gameday::input::InputMode;
    let mut app = mk();
    app.mode = InputMode::Filter { buf: "kc".into() };
    let mut t = Terminal::new(TestBackend::new(120, 24)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let footer = buf_text(&t).lines().nth(23).unwrap().to_string();
    assert!(footer.contains("/kc"), "filter prompt missing: {footer:?}");
}

#[test]
fn status_line_renders_verbatim_in_the_footer_row() {
    let mut app = mk();
    app.status_line = Some("unknown command \"foo\", valid: nfl|standings".into());
    let mut t = Terminal::new(TestBackend::new(120, 24)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let footer = buf_text(&t).lines().nth(23).unwrap().to_string();
    assert!(
        footer.contains("unknown command \"foo\", valid: nfl|standings"),
        "status line missing: {footer:?}"
    );
}

#[test]
fn committed_filter_narrows_the_board_and_shows_in_the_footer() {
    let mut app = mk();
    app.apply_boards(
        League::Nfl,
        vec![g("1", "KC", "TB", true), g("2", "DAL", "PHI", true)],
        false,
    );
    app.tab = Tab::League(League::Nfl);
    app.filter = Some("kc".into());
    let mut t = Terminal::new(TestBackend::new(120, 24)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t);
    assert!(s.contains("KC"), "filtered-in game missing: {s}");
    assert!(!s.contains("PHI"), "filtered-out game still drawn: {s}");
    assert!(s.contains("/kc"), "active filter missing from footer: {s}");
}

#[test]
fn filter_mode_typing_filters_incrementally() {
    use gameday::input::InputMode;
    let mut app = mk();
    app.apply_boards(
        League::Nfl,
        vec![g("1", "KC", "TB", true), g("2", "DAL", "PHI", true)],
        false,
    );
    app.tab = Tab::League(League::Nfl);
    // No commit yet: the open prompt's buffer already narrows the board.
    app.mode = InputMode::Filter { buf: "phi".into() };
    let mut t = Terminal::new(TestBackend::new(120, 24)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t);
    assert!(s.contains("PHI"), "{s}");
    assert!(!s.contains("KC"), "buffer should filter while typing: {s}");
}

#[test]
fn filter_matching_nothing_names_the_pattern() {
    let mut app = mk();
    app.apply_boards(League::Nfl, vec![g("1", "KC", "TB", true)], false);
    app.tab = Tab::League(League::Nfl);
    app.filter = Some("zzz".into());
    let mut t = Terminal::new(TestBackend::new(120, 24)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t);
    assert!(
        s.contains("no games match \"zzz\""),
        "empty filter result must name the pattern: {s}"
    );
}

#[test]
fn z_zooms_the_selected_game_and_shows_the_tab_bar() {
    use gameday::views::{View, ZoomTab};
    use ratatui::style::Style;
    let mut app = mk();
    app.apply_boards(League::Nfl, vec![g("1", "KC", "TB", true)], false);
    app.tab = Tab::League(League::Nfl);
    gameday::input::handle_key(
        &mut app,
        crossterm::event::KeyCode::Char('z'),
        crossterm::event::KeyModifiers::NONE,
    );
    assert_eq!(
        app.view,
        View::Zoom { game_id: "1".into(), tab: ZoomTab::Overview }
    );
    let mut t = Terminal::new(TestBackend::new(120, 24)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t);
    assert!(s.contains("OVERVIEW │ PLAYS │ STATS"), "tab bar missing:\n{s}");
    // OVERVIEW is highlighted: its cells are styled unlike the idle PLAYS tab.
    let b = t.backend().buffer();
    let (mut over_style, mut plays_style) = (None::<Style>, None::<Style>);
    let area = b.area();
    for y in 0..area.height {
        let row: String = (0..area.width).map(|x| b[(x, y)].symbol()).collect::<Vec<_>>().join("");
        if let Some(ox) = row.find("OVERVIEW") {
            let px = row.find("PLAYS").expect("PLAYS on the same row");
            over_style = Some(b[(ox as u16, y)].style());
            plays_style = Some(b[(px as u16, y)].style());
            break;
        }
    }
    assert_ne!(
        over_style.expect("OVERVIEW cell"),
        plays_style.expect("PLAYS cell"),
        "active tab must be visually highlighted"
    );
}

#[test]
fn l_cycles_to_the_plays_tab_and_jk_move_the_highlight() {
    use crossterm::event::{KeyCode, KeyModifiers};
    use gameday::views::{View, ZoomTab};
    let mut app = mk();
    let mut game = g("1", "KC", "TB", true);
    game.last_plays = vec![
        Play {
            clock: "1:27".into(),
            team: "KC".into(),
            text: "Mahomes pass to Kelce, 12 yd TOUCHDOWN".into(),
            scoring: true,
        },
        Play {
            clock: "2:05".into(),
            team: "TB".into(),
            text: "Evans 8 yard reception".into(),
            scoring: false,
        },
    ];
    app.apply_boards(League::Nfl, vec![game], false);
    app.tab = Tab::League(League::Nfl);
    gameday::input::handle_key(&mut app, KeyCode::Char('z'), KeyModifiers::NONE);
    gameday::input::handle_key(&mut app, KeyCode::Char('l'), KeyModifiers::NONE);
    assert_eq!(
        app.view,
        View::Zoom { game_id: "1".into(), tab: ZoomTab::Plays }
    );
    let mut t = Terminal::new(TestBackend::new(120, 24)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t);
    // The full feed reuses the game's last_plays.
    assert!(s.contains("Mahomes pass to Kelce"), "{s}");
    assert!(s.contains("Evans 8 yard reception"), "{s}");
    assert!(s.contains("▸"), "highlight marker missing: {s}");
    assert_eq!(app.zoom_scroll, 0);
    gameday::input::handle_key(&mut app, KeyCode::Char('j'), KeyModifiers::NONE);
    assert_eq!(app.zoom_scroll, 1, "j moves the highlight");
    gameday::input::handle_key(&mut app, KeyCode::Char('k'), KeyModifiers::NONE);
    assert_eq!(app.zoom_scroll, 0, "k moves it back");
    // ']' cycles tabs too: Plays -> Stats.
    gameday::input::handle_key(&mut app, KeyCode::Char(']'), KeyModifiers::NONE);
    assert_eq!(
        app.view,
        View::Zoom { game_id: "1".into(), tab: ZoomTab::Stats }
    );
}

#[test]
fn esc_pops_zoom_back_to_board() {
    use crossterm::event::{KeyCode, KeyModifiers};
    use gameday::views::View;
    let mut app = mk();
    app.apply_boards(League::Nfl, vec![g("1", "KC", "TB", true)], false);
    app.tab = Tab::League(League::Nfl);
    gameday::input::handle_key(&mut app, KeyCode::Char('z'), KeyModifiers::NONE);
    assert!(matches!(app.view, View::Zoom { .. }));
    gameday::input::handle_key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    assert_eq!(app.view, View::Board);
}

#[test]
fn q_in_zoom_pops_instead_of_quitting() {
    use crossterm::event::{KeyCode, KeyModifiers};
    use gameday::views::View;
    let mut app = mk();
    app.apply_boards(League::Nfl, vec![g("1", "KC", "TB", true)], false);
    app.tab = Tab::League(League::Nfl);
    gameday::input::handle_key(&mut app, KeyCode::Char('z'), KeyModifiers::NONE);
    gameday::input::handle_key(&mut app, KeyCode::Char('q'), KeyModifiers::NONE);
    assert_eq!(app.view, View::Board, "q pops the zoom");
    assert!(!app.should_quit, "q must not quit outside the Board view");
    gameday::input::handle_key(&mut app, KeyCode::Char('q'), KeyModifiers::NONE);
    assert!(app.should_quit, "q on the Board quits");
}

#[test]
fn footer_advertises_command_and_filter_chords() {
    let mut app = mk();
    let mut t = Terminal::new(TestBackend::new(120, 24)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t);
    assert!(s.contains("[:] CMD"), "{s}");
    assert!(s.contains("[/] FILTER"), "{s}");
}

#[test]
fn narrow_footer_sheds_low_value_chords_but_keeps_help_and_quit() {
    // 80 cols can't hold the whole chord list; MOVE/PAGE go first, the way
    // out and the full keymap never get clipped.
    let mut app = mk();
    let mut t = Terminal::new(TestBackend::new(80, 24)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t);
    let footer = s.lines().find(|l| l.contains("NAV:")).expect("footer row");
    assert!(footer.contains("[?] HELP"), "HELP clipped: {footer:?}");
    assert!(footer.contains("[Q] QUIT"), "QUIT clipped: {footer:?}");
    assert!(!footer.contains("MOVE"), "MOVE should be shed first: {footer:?}");
}

#[test]
fn stats_tab_renders_rows_and_leaders() {
    use gameday::domain::{GameStats, Leader, StatRow};
    use gameday::views::{View, ZoomTab};
    let mut app = mk();
    app.apply_boards(League::Nfl, vec![g("1", "KC", "TB", true)], false);
    app.tab = Tab::League(League::Nfl);
    app.view = View::Zoom { game_id: "1".into(), tab: ZoomTab::Stats };
    app.merge_stats(
        "1",
        GameStats {
            rows: vec![
                StatRow { label: "Total Yards".into(), away: "251".into(), home: "277".into() },
                StatRow { label: "Turnovers".into(), away: "1".into(), home: "0".into() },
            ],
            leaders: vec![Leader {
                team: "KC".into(),
                label: "Passing Yards".into(),
                text: "D. Lock 12/14, 103 YDS, 1 TD".into(),
            }],
        },
    );
    let mut t = Terminal::new(TestBackend::new(120, 24)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t);
    assert!(s.contains("Total Yards"), "{s}");
    assert!(s.contains("251") && s.contains("277"), "{s}");
    assert!(s.contains("LEADERS"), "{s}");
    assert!(s.contains("D. Lock"), "{s}");
}

#[test]
fn stats_tab_without_data_says_no_stats_yet() {
    use gameday::views::{View, ZoomTab};
    let mut app = mk();
    app.apply_boards(League::Nfl, vec![g("1", "KC", "TB", true)], false);
    app.tab = Tab::League(League::Nfl);
    app.view = View::Zoom { game_id: "1".into(), tab: ZoomTab::Stats };
    let mut t = Terminal::new(TestBackend::new(120, 24)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t);
    assert!(s.contains("no stats yet"), "{s}");
}

/// Live NBA game with one scoring play, for cross-league feed tests.
fn nba_game(id: &str, away: &str, home: &str) -> Game {
    let t = |abbr: &str| Team {
        id: abbr.into(),
        abbr: abbr.into(),
        name: abbr.into(),
        color: [0, 122, 51],
        alt_color: [186, 150, 83],
        logo_key: format!("nba/{}", abbr.to_lowercase()),
        ..Default::default()
    };
    Game {
        id: id.into(),
        league: League::Nba,
        away: t(away),
        home: t(home),
        away_score: 88,
        home_score: 81,
        status: Status::Live,
        period: "Q3".into(),
        clock: "4:12".into(),
        situation: None,
        last_plays: vec![Play {
            clock: "4:12".into(),
            team: away.into(),
            text: "Tatum pull-up three".into(),
            scoring: true,
        }],
        meter: None,
        start_time: None,
        broadcast: None,
    }
}

#[test]
fn plays_feed_lists_scoring_plays_across_leagues_with_a_marker() {
    use gameday::views::View;
    let mut app = mk();
    let mut nfl = g("1", "KC", "TB", true);
    nfl.last_plays = vec![Play {
        clock: "1:27".into(),
        team: "KC".into(),
        text: "Mahomes to Kelce, 12 yd".into(),
        scoring: true,
    }];
    app.apply_boards(League::Nfl, vec![nfl], false);
    app.apply_boards(League::Nba, vec![nba_game("2", "BOS", "LAL")], false);
    app.view = View::PlaysFeed;
    let mut t = Terminal::new(TestBackend::new(120, 24)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t);
    let lines: Vec<&str> = s.lines().collect();
    // Both boards' scoring plays are rows, each tagged with its league chip
    // and scoring word, and carrying the matchup score for orientation.
    let nfl_row = lines
        .iter()
        .find(|l| l.contains("Mahomes to Kelce"))
        .unwrap_or_else(|| panic!("NFL scoring play missing from feed:\n{s}"));
    assert!(nfl_row.contains("NFL") && nfl_row.contains("TOUCHDOWN!"), "{nfl_row}");
    assert!(nfl_row.contains("KC@TB") && nfl_row.contains("27-24"), "{nfl_row}");
    let nba_row = lines
        .iter()
        .find(|l| l.contains("Tatum pull-up three"))
        .unwrap_or_else(|| panic!("NBA scoring play missing from feed:\n{s}"));
    assert!(nba_row.contains("NBA") && nba_row.contains("BUCKET!"), "{nba_row}");
    // Row 0 (the NFL play — enabled-tab order) carries the ▸ marker.
    assert!(nfl_row.contains("▸"), "marker must start on row 0: {nfl_row}");
    assert!(!nba_row.contains("▸"), "only one row is marked: {nba_row}");
}

#[test]
fn plays_feed_j_and_k_move_the_marker_and_clamp() {
    use crossterm::event::{KeyCode, KeyModifiers};
    use gameday::views::View;
    let mut app = mk();
    let mut nfl = g("1", "KC", "TB", true);
    nfl.last_plays = vec![Play {
        clock: "1:27".into(),
        team: "KC".into(),
        text: "Mahomes to Kelce, 12 yd".into(),
        scoring: true,
    }];
    app.apply_boards(League::Nfl, vec![nfl], false);
    app.apply_boards(League::Nba, vec![nba_game("2", "BOS", "LAL")], false);
    app.view = View::PlaysFeed;
    assert_eq!(app.feed_scroll, 0);
    gameday::input::handle_key(&mut app, KeyCode::Char('j'), KeyModifiers::NONE);
    assert_eq!(app.feed_scroll, 1, "j moves the marker down");
    let mut t = Terminal::new(TestBackend::new(120, 24)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t);
    let marked = s
        .lines()
        .find(|l| l.contains("▸"))
        .unwrap_or_else(|| panic!("no marked row:\n{s}"));
    assert!(marked.contains("Tatum pull-up three"), "marker follows j: {marked}");
    // Clamped at the last row; k walks back; PgUp clamps at the top.
    gameday::input::handle_key(&mut app, KeyCode::Char('j'), KeyModifiers::NONE);
    assert_eq!(app.feed_scroll, 1, "clamped at the bottom (2 rows)");
    gameday::input::handle_key(&mut app, KeyCode::Char('k'), KeyModifiers::NONE);
    assert_eq!(app.feed_scroll, 0);
    gameday::input::handle_key(&mut app, KeyCode::PageUp, KeyModifiers::NONE);
    assert_eq!(app.feed_scroll, 0, "PgUp clamps at the top");
    // Esc pops back to the board.
    gameday::input::handle_key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    assert_eq!(app.view, View::Board);
}

/// A tiny two-group standings table for view tests: enough rows to check
/// group headers, column alignment, and team-color lookup.
fn standings_table() -> gameday::domain::StandingsTable {
    use gameday::domain::{StandingRow, StandingsGroup, StandingsTable};
    let row = |abbr: &str, name: &str, w: u32, l: u32, t: u32| StandingRow {
        abbr: abbr.into(),
        name: name.into(),
        wins: w,
        losses: l,
        third: Some(t),
        third_label: "T",
    };
    StandingsTable {
        league: League::Nfl,
        groups: vec![
            StandingsGroup {
                name: "American Football Conference".into(),
                rows: vec![row("KC", "Chiefs", 11, 6, 0), row("BUF", "Bills", 10, 7, 1)],
            },
            StandingsGroup {
                name: "National Football Conference".into(),
                rows: vec![row("PHI", "Eagles", 12, 5, 0), row("DAL", "Cowboys", 9, 8, 0)],
            },
        ],
    }
}

#[test]
fn standings_view_renders_groups_and_columns() {
    use gameday::views::View;
    let mut app = mk();
    app.apply_boards(League::Nfl, vec![g("1", "KC", "TB", true)], false);
    app.view = View::Standings(League::Nfl);
    app.merge_standings(standings_table());
    let mut t = Terminal::new(TestBackend::new(120, 36)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t);
    assert!(s.contains("STANDINGS"), "{s}");
    assert!(s.contains("AMERICAN FOOTBALL CONFERENCE"), "{s}");
    assert!(s.contains("NATIONAL FOOTBALL CONFERENCE"), "{s}");
    // Every row renders abbr + name, and the W/L/T values line up under the
    // column headers.
    for needle in ["KC", "BILLS", "EAGLES", "COWBOYS"] {
        assert!(s.contains(needle), "missing {needle}:\n{s}");
    }
    let header = s
        .lines()
        .find(|l| l.contains("W") && l.contains("L") && l.contains("T") && l.contains("TEAM"))
        .unwrap_or_else(|| panic!("no W/L/T column header:\n{s}"));
    let kc_row = s.lines().find(|l| l.contains("CHIEFS")).unwrap();
    // "11" (wins) ends in the same column the header's "W" occupies.
    let w_col = header.find(" W").expect("W header") + 1;
    let wins_end = kc_row.find("11").expect("KC wins") + 1;
    assert_eq!(wins_end, w_col, "wins not aligned under W:\nheader: {header:?}\nrow:    {kc_row:?}");
}

#[test]
fn standings_view_without_data_says_so_and_esc_pops() {
    use crossterm::event::{KeyCode, KeyModifiers};
    use gameday::views::View;
    let mut app = mk();
    app.view = View::Standings(League::Nba);
    let mut t = Terminal::new(TestBackend::new(120, 24)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t);
    assert!(s.contains("no standings yet"), "{s}");
    gameday::input::handle_key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    assert_eq!(app.view, View::Board);
    assert!(!app.should_quit);
}

#[test]
fn standings_view_scrolls_with_j_and_clamps() {
    use crossterm::event::{KeyCode, KeyModifiers};
    use gameday::views::View;
    let mut app = mk();
    app.view = View::Standings(League::Nfl);
    app.merge_standings(standings_table());
    assert_eq!(app.standings_scroll, 0);
    gameday::input::handle_key(&mut app, KeyCode::Char('j'), KeyModifiers::NONE);
    assert_eq!(app.standings_scroll, 1, "j scrolls down");
    // Small terminal: the second group starts below the fold until scrolled.
    let mut t = Terminal::new(TestBackend::new(120, 24)).unwrap();
    for _ in 0..50 {
        gameday::input::handle_key(&mut app, KeyCode::Char('j'), KeyModifiers::NONE);
    }
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t);
    assert!(
        s.contains("NATIONAL FOOTBALL CONFERENCE"),
        "scrolled view must reach the last group:\n{s}"
    );
    gameday::input::handle_key(&mut app, KeyCode::PageUp, KeyModifiers::NONE);
    gameday::input::handle_key(&mut app, KeyCode::PageUp, KeyModifiers::NONE);
    assert_eq!(app.standings_scroll, 0, "PgUp clamps at the top");
    // q pops instead of quitting (q quits only on the Board).
    gameday::input::handle_key(&mut app, KeyCode::Char('q'), KeyModifiers::NONE);
    assert_eq!(app.view, View::Board);
    assert!(!app.should_quit);
}

#[test]
fn standings_command_opens_the_view_and_sets_the_poll_target() {
    use crossterm::event::{KeyCode, KeyModifiers};
    use gameday::views::View;
    let mut app = mk();
    assert_eq!(app.standings_target(), None);
    for c in ":standings nba".chars() {
        gameday::input::handle_key(&mut app, KeyCode::Char(c), KeyModifiers::NONE);
    }
    gameday::input::handle_key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(app.view, View::Standings(League::Nba));
    assert_eq!(app.standings_target(), Some(League::Nba));
}

#[test]
fn plays_feed_without_scoring_plays_says_so() {
    use gameday::views::View;
    let mut app = mk();
    app.apply_boards(League::Nfl, vec![g("1", "KC", "TB", true)], false);
    app.view = View::PlaysFeed;
    let mut t = Terminal::new(TestBackend::new(120, 24)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t);
    assert!(s.contains("no scoring plays yet"), "{s}");
}
