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
            ..Default::default()
        }],
        meter: None,
        start: Some(time::macros::datetime!(2026-09-13 20:20 -4)),
        broadcast: Some("CBS".into()),
        ..Game::default()
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
    App::new(Config::default_all(), vec![], dir, time::UtcOffset::UTC)
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
fn active_alert_banner_renders_in_header_in_live_color() {
    let mut app = mk();
    app.active_alert = Some(gameday::alerts::Alert {
        text: "★ KC SCORES  27-24".into(),
        until_tick: 999,
    });
    let mut t = Terminal::new(TestBackend::new(120, 24)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t);
    assert!(s.contains("★ KC SCORES  27-24"), "{s}");
    // The star sits on the header row (row 0) styled in the theme's live role.
    let b = t.backend().buffer();
    let star_x = (0..b.area().width)
        .find(|&x| b[(x, 0)].symbol() == "★")
        .expect("banner star on the header row");
    assert_eq!(
        b[(star_x, 0)].style().fg,
        Some(gameday::theme::current().live),
        "banner must use the live color role"
    );
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
    use gameday::theme;
    use ratatui::style::Color;
    let bg_of = |name: &str| -> Color {
        theme::set_current(name).unwrap();
        let mut app = mk();
        let mut t = Terminal::new(TestBackend::new(120, 24)).unwrap();
        t.draw(|f| app.draw(f)).unwrap();
        t.backend().buffer()[(0, 0)].bg
    };
    assert_eq!(bg_of("ceefax"), theme::builtin("ceefax").bg);
    assert_eq!(bg_of("phosphor"), theme::builtin("phosphor").bg);
    assert_eq!(bg_of("gruvbox"), Color::Rgb(0x28, 0x28, 0x28));
    assert_eq!(bg_of("broadcast"), Color::Rgb(0, 0, 0));
}

#[test]
fn footer_status_takes_the_clocks_discipline() {
    // "GAME 1/4  UPD 3s" is status, clock-shaped: cyan where the theme grants
    // clocks (broadcast), muted where it doesn't (studio) — never raw cyan.
    use gameday::theme;
    let fg_of_upd = |name: &str| {
        theme::set_current(name).unwrap();
        let mut app = mk();
        app.apply_boards(League::Nfl, vec![g("1", "KC", "TB", true)], false);
        app.last_update = Some(std::time::Instant::now());
        let mut t = Terminal::new(TestBackend::new(120, 24)).unwrap();
        t.draw(|f| app.draw(f)).unwrap();
        let b = t.backend().buffer();
        let y = b.area().height - 1;
        let row: String = (0..b.area().width).map(|x| b[(x, y)].symbol().to_string()).collect();
        let x = row.find("UPD").unwrap_or_else(|| panic!("no UPD in footer: {row:?}")) as u16;
        b[(x, y)].fg
    };
    assert_eq!(fg_of_upd("broadcast"), theme::builtin("broadcast").cyan);
    assert_eq!(fg_of_upd("studio"), theme::builtin("studio").muted);
    theme::set_current("broadcast").unwrap();
}

#[test]
fn theme_picker_renders_every_loaded_name_over_the_board() {
    use gameday::theme;
    use gameday::views::View;
    theme::set_current("broadcast").unwrap();
    let mut app = mk();
    app.apply_boards(League::Nfl, vec![g("1", "KC", "TB", true)], false);
    app.tab = Tab::League(League::Nfl);
    app.open_theme_picker();
    assert_eq!(app.view, View::ThemePicker);
    let mut t = Terminal::new(TestBackend::new(120, 36)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t);
    assert!(s.contains(" THEMES "), "picker panel title missing:\n{s}");
    for name in theme::BUILTIN_NAMES {
        assert!(s.contains(name), "picker missing {name}:\n{s}");
    }
    assert!(s.contains("ESC REVERT"), "picker hint missing:\n{s}");
    // The board is still drawn underneath — the panel is the preview's frame.
    assert!(s.contains("[NFL]"), "board must render behind the picker:\n{s}");
    let marked = s.lines().find(|l| l.contains("▸ broadcast")).unwrap_or_else(|| panic!("{s}"));
    assert!(marked.contains("default"), "{marked}");
}

#[test]
fn studio_theme_grays_the_chrome_but_keeps_scores_and_live_colored() {
    use gameday::theme;
    theme::set_current("studio").unwrap();
    let studio = theme::builtin("studio");
    let mut app = mk();
    let mut game = g("1", "KC", "TB", true);
    game.last_plays = vec![Play {
        clock: "1:27".into(),
        team: "KC".into(),
        text: "Mahomes pass to Kelce, 12 yd TOUCHDOWN".into(),
        scoring: true,
        ..Default::default()
    }];
    app.config.score_style = gameday::tiles::ScoreStyle::Compact;
    app.apply_boards(League::Nfl, vec![game], false);
    app.tab = Tab::League(League::Nfl);
    let mut t = Terminal::new(TestBackend::new(120, 36)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let b = t.backend().buffer();
    let area = *b.area();
    let fg_at = |needle: &str| -> ratatui::style::Color {
        for y in 0..area.height {
            let row: String = (0..area.width).map(|x| b[(x, y)].symbol()).collect::<Vec<_>>().join("");
            if let Some(pos) = row.find(needle) {
                return b[(row[..pos].chars().count() as u16, y)].fg;
            }
        }
        panic!("{needle:?} not on the board:\n{}", buf_text(&t));
    };
    // Chrome disciplined: sidebar headers + clocks + play abbrs go gray.
    assert_eq!(fg_at("TOP PLAYS"), studio.muted);
    assert_eq!(fg_at("RECORDS"), studio.muted);
    assert_eq!(fg_at("GLOBAL ALERTS"), studio.muted);
    assert_eq!(fg_at("LAST PLAYS"), studio.muted);
    // Identity floor: chip, LIVE, scoring word and scores stay colored. (The
    // tile's chip reads "[NFL] LIVE"; the bare "[NFL]" is the header's
    // inverted tab chip.)
    assert_eq!(fg_at("[NFL] LIVE"), studio.league_accent(League::Nfl));
    assert_eq!(fg_at("LIVE"), studio.live);
    assert_eq!(fg_at("TOUCHDOWN!"), studio.live);
    assert_eq!(fg_at("27 - 24"), gameday::theme::rgb([200, 16, 46]), "score digits keep team color");
    theme::set_current("broadcast").unwrap();
}

#[test]
fn sidebar_top_plays_are_abbr_surname_clock() {
    let mut app = mk();
    let mut game = g("1", "KC", "TB", true);
    game.last_plays = vec![Play {
        clock: "1:27".into(),
        team: "KC".into(),
        text: "Mahomes pass to Kelce, 12 yd TOUCHDOWN".into(),
        scoring: true,
        ..Default::default()
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
    // A leaderboard line — abbr + surname, never the truncated sentence —
    // with the clock hugging the sidebar's right border.
    assert!(row.contains("★ KC  Mahomes"), "abbr + surname: {row:?}");
    assert!(!row.contains("pass"), "the sentence stays out of the rail: {row:?}");
    assert!(row.trim_end().ends_with("1:27│"), "clock not right-aligned: {row:?}");
}

#[test]
fn empty_home_with_no_boards_points_at_config() {
    let mut app = mk();
    let mut t = Terminal::new(TestBackend::new(120, 24)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t).to_lowercase();
    assert!(s.contains("nothing live on the enabled boards"), "{s}");
}

#[test]
fn empty_home_names_the_next_start() {
    let mut app = mk();
    // One scheduled game, nothing live: Home names when the slate opens.
    app.apply_boards(League::Nfl, vec![g("1", "KC", "TB", false)], false);
    let mut t = Terminal::new(TestBackend::new(120, 24)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t).to_lowercase();
    assert!(s.contains("nothing live · next: kc @ tb"), "{s}");
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
            ..Default::default()
        },
        Play {
            clock: "2:05".into(),
            team: "TB".into(),
            text: "Evans 8 yard reception".into(),
            scoring: false,
            ..Default::default()
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
            ..Default::default()
        }],
        meter: None,
        ..Game::default()
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
        ..Default::default()
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
        ..Default::default()
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

/// A 40-line standings table (two 18-team groups) that never fits a 24-row
/// terminal, for the scroll-clamp and scroll-affordance tests.
fn tall_standings_table() -> gameday::domain::StandingsTable {
    use gameday::domain::{StandingRow, StandingsGroup, StandingsTable};
    let group = |name: &str, prefix: &str| StandingsGroup {
        name: name.into(),
        rows: (0..18)
            .map(|i| StandingRow {
                abbr: format!("{prefix}{i:02}"),
                name: format!("{prefix}team{i:02}"),
                wins: 10,
                losses: i,
                third: None,
                third_label: "",
            })
            .collect(),
    };
    StandingsTable {
        league: League::Nfl,
        groups: vec![group("American Football Conference", "A"), group("National Football Conference", "N")],
    }
}

#[test]
fn standings_scroll_clamps_to_the_pane_so_k_moves_back_at_once() {
    use crossterm::event::{KeyCode, KeyModifiers};
    use gameday::views::View;
    let mut app = mk();
    app.view = View::Standings(League::Nfl);
    app.merge_standings(tall_standings_table());
    let mut t = Terminal::new(TestBackend::new(120, 24)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let lines = gameday::views::standings::line_count(&app.standings[&League::Nfl]);
    assert_eq!(lines, 41, "18+2 rows per group, one blank between");
    for _ in 0..60 {
        gameday::input::handle_key(&mut app, KeyCode::Char('j'), KeyModifiers::NONE);
    }
    t.draw(|f| app.draw(f)).unwrap();
    let bottom = app.standings_scroll;
    // The stored offset stops where the renderer stops (last line on the
    // last pane row) instead of running on to line_count - 1.
    assert!(bottom < lines - 1, "offset must clamp against the pane: {bottom} of {lines}");
    let s = buf_text(&t);
    assert!(s.contains("NTEAM17"), "bottom of the table is on screen:\n{s}");
    // One k visibly scrolls back up — no dead presses.
    gameday::input::handle_key(&mut app, KeyCode::Char('k'), KeyModifiers::NONE);
    assert_eq!(app.standings_scroll, bottom - 1);
    t.draw(|f| app.draw(f)).unwrap();
    let s2 = buf_text(&t);
    assert_ne!(s, s2, "a single k after the bottom must move the table");
}

#[test]
fn standings_shows_a_more_marker_when_the_table_is_clipped() {
    use crossterm::event::{KeyCode, KeyModifiers};
    use gameday::views::View;
    let mut app = mk();
    app.view = View::Standings(League::Nfl);
    app.merge_standings(tall_standings_table());
    let mut t = Terminal::new(TestBackend::new(120, 24)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t);
    let marker = s
        .lines()
        .find(|l| l.contains('▼'))
        .unwrap_or_else(|| panic!("clipped table needs a ▼ more marker:\n{s}"));
    assert!(marker.contains("BELOW"), "marker counts what's hidden: {marker}");
    assert!(!marker.contains('▲'), "nothing above at the top: {marker}");
    for _ in 0..60 {
        gameday::input::handle_key(&mut app, KeyCode::Char('j'), KeyModifiers::NONE);
    }
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t);
    let marker = s
        .lines()
        .find(|l| l.contains('▲'))
        .unwrap_or_else(|| panic!("scrolled table needs a ▲ marker:\n{s}"));
    assert!(marker.contains("ABOVE") && !marker.contains('▼'), "{marker}");
    // A table that fits shows no marker at all.
    let mut small = mk();
    small.view = View::Standings(League::Nfl);
    small.merge_standings(standings_table());
    let mut t = Terminal::new(TestBackend::new(120, 36)).unwrap();
    t.draw(|f| small.draw(f)).unwrap();
    let s = buf_text(&t);
    assert!(!s.contains('▼') && !s.contains('▲'), "no marker when it fits:\n{s}");
}

#[test]
fn feed_and_standings_footers_advertise_only_keys_that_work_there() {
    use crossterm::event::{KeyCode, KeyModifiers};
    use gameday::views::View;
    for view in [View::PlaysFeed, View::Standings(League::Nfl)] {
        let mut app = mk();
        app.config.enabled_tabs = vec![League::Nfl];
        app.apply_boards(League::Nfl, vec![g("1", "KC", "TB", true)], false);
        app.view = view.clone();
        let mut t = Terminal::new(TestBackend::new(120, 24)).unwrap();
        t.draw(|f| app.draw(f)).unwrap();
        let s = buf_text(&t);
        let footer = s.lines().last().unwrap();
        assert!(!footer.contains("TABS"), "{view:?}: zoom's tab cycle is a no-op here: {footer}");
        assert!(footer.contains("BACK"), "{view:?}: {footer}");
        // [TAB] LEAGUE is advertised, so Tab must actually switch tabs.
        assert!(footer.contains("LEAGUE"), "{view:?}: {footer}");
        assert_eq!(app.tab, Tab::Home);
        gameday::input::handle_key(&mut app, KeyCode::Tab, KeyModifiers::NONE);
        assert_eq!(app.tab, Tab::League(League::Nfl), "{view:?}: Tab switches league");
        assert_eq!(app.view, View::Board, "{view:?}: a tab switch lands on the board");
    }
}

#[test]
fn plays_feed_marks_its_end_when_the_pane_has_room() {
    use gameday::views::View;
    let mut app = mk();
    let mut nfl = g("1", "KC", "TB", true);
    nfl.last_plays = vec![Play {
        clock: "1:27".into(),
        team: "KC".into(),
        text: "Mahomes to Kelce, 12 yd".into(),
        scoring: true,
        ..Default::default()
    }];
    app.apply_boards(League::Nfl, vec![nfl], false);
    app.view = View::PlaysFeed;
    let mut t = Terminal::new(TestBackend::new(120, 24)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t);
    let lines: Vec<&str> = s.lines().collect();
    let row = lines.iter().position(|l| l.contains("Mahomes to Kelce")).unwrap();
    assert!(
        lines[row + 1].contains("END OF FEED"),
        "the row after the last play closes the feed:\n{s}"
    );
}

#[test]
fn records_rail_shows_the_abbr_instead_of_a_clipped_name() {
    // filter.png: "2. Buccanee… 11 6" — a name past the 9-cell column falls
    // back to the abbr rather than an ellipsized fragment.
    let mut app = mk();
    let mut game = g("1", "KC", "TB", true);
    game.away.name = "Chiefs".into();
    game.away.record = "11-6".into();
    game.home.name = "Buccaneers".into();
    game.home.record = "11-6".into();
    app.apply_boards(League::Nfl, vec![game], false);
    app.tab = Tab::League(League::Nfl);
    let mut t = Terminal::new(TestBackend::new(120, 36)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t);
    assert!(s.contains("RECORDS"), "sidebar rail present:\n{s}");
    assert!(!s.contains("Buccanee…"), "clipped name in the rail:\n{s}");
    let rail_rows: Vec<&str> = s.lines().filter(|l| l.contains(". ") && l.contains(" 11  6")).collect();
    assert!(
        rail_rows.iter().any(|l| l.contains("Chiefs")) && rail_rows.iter().any(|l| l.contains("TB ")),
        "rail rows: {rail_rows:?}\n{s}"
    );
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

#[test]
fn brackets_step_the_viewed_date_and_header_marks_it() {
    use crossterm::event::{KeyCode, KeyModifiers};
    // The pure label first: known date, known weekday.
    let d = time::Date::from_calendar_date(2000, time::Month::January, 1).unwrap();
    assert_eq!(gameday::app::date_label(d), "SAT JAN 1");

    let mut app = mk();
    app.apply_boards(League::Nfl, vec![g("1", "KC", "TB", true)], false);
    app.tab = Tab::League(League::Nfl);
    let mut t = Terminal::new(TestBackend::new(120, 36)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    assert!(!buf_text(&t).contains('‹'), "no travel marker on today");

    app.on_key(KeyCode::Char('['), KeyModifiers::NONE);
    assert_eq!(app.viewed_date_offset.get(&League::Nfl), Some(&-1));
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t);
    assert!(s.contains('‹') && s.contains('›'), "header shows the viewed date:\n{s}");

    // A merged dated board replaces the league's visible games.
    let date = time::OffsetDateTime::now_local()
        .unwrap_or_else(|_| time::OffsetDateTime::now_utc())
        .date()
        .previous_day()
        .unwrap();
    let mut final_game = g("d1", "DAL", "PHI", false);
    final_game.status = Status::Final;
    app.merge_dated_board(League::Nfl, date, vec![final_game]);
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t);
    assert!(s.contains("DAL") && s.contains("PHI"), "{s}");
    // The ticker keeps running today's live scores while you travel (its
    // SCORES lane is every live game), so only the board must hide KC.
    let (board, ticker) = s.split_once(" SCORES ").expect("ticker lane below the board");
    assert!(!board.contains("KC"), "today's board is hidden while traveling:\n{s}");
    assert!(ticker.contains("NFL KC 27 TB 24"), "live score still in the ticker:\n{s}");

    // Clamped at ±7.
    for _ in 0..20 {
        app.on_key(KeyCode::Char('['), KeyModifiers::NONE);
    }
    assert_eq!(app.viewed_date_offset.get(&League::Nfl), Some(&-7));
    // ']' steps forward; back at 0 the marker (and dated board) go away.
    for _ in 0..7 {
        app.on_key(KeyCode::Char(']'), KeyModifiers::NONE);
    }
    assert_eq!(app.viewed_date_offset.get(&League::Nfl).copied().unwrap_or(0), 0);
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t);
    assert!(!s.contains('‹'), "{s}");
    assert!(s.contains("KC"), "live board is back:\n{s}");
}

#[test]
fn pre_tile_and_slate_show_odds() {
    let mut app = mk();
    let mut game = g("1", "KC", "TB", false); // Pre
    game.last_plays.clear(); // a pre-game has no plays
    game.situation = None;
    game.odds = Some("KC -3.5  O/U 47.5".into());
    app.apply_boards(League::Nfl, vec![game], false);
    app.tab = Tab::League(League::Nfl);
    let mut t = Terminal::new(TestBackend::new(120, 36)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t);
    // Once on the mosaic tile, once on the slate row.
    assert!(s.matches("O/U 47.5").count() >= 2, "{s}");
}

// ---- Task 9: mouse support -------------------------------------------------

/// Synthetic left click at (x, y), routed through the real mouse handler.
fn click(app: &mut App, x: u16, y: u16) {
    use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
    gameday::keymap::on_mouse(
        app,
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: x,
            row: y,
            modifiers: KeyModifiers::NONE,
        },
    );
}

/// Synthetic wheel event (up = toward row 0).
fn wheel(app: &mut App, up: bool) {
    use crossterm::event::{KeyModifiers, MouseEvent, MouseEventKind};
    gameday::keymap::on_mouse(
        app,
        MouseEvent {
            kind: if up {
                MouseEventKind::ScrollUp
            } else {
                MouseEventKind::ScrollDown
            },
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        },
    );
}

/// The registered zone for `hit`, from the last draw.
fn zone_for(app: &App, hit: gameday::keymap::Hit) -> ratatui::layout::Rect {
    app.hit_zones
        .iter()
        .find(|(_, h)| *h == hit)
        .map(|(r, _)| *r)
        .unwrap_or_else(|| panic!("no zone registered for {hit:?}: {:?}", app.hit_zones))
}

#[test]
fn clicking_a_tile_selects_it() {
    use gameday::keymap::Hit;
    let mut app = mk();
    app.apply_boards(
        League::Nfl,
        vec![g("1", "KC", "TB", true), g("2", "DAL", "PHI", true)],
        false,
    );
    app.tab = Tab::League(League::Nfl);
    let mut t = Terminal::new(TestBackend::new(120, 24)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    assert_eq!(app.selected, 0);
    let zone = zone_for(&app, Hit::Tile(1));
    click(&mut app, zone.x + zone.width / 2, zone.y + zone.height / 2);
    assert_eq!(app.selected, 1, "click on the second tile selects it");
    // A click outside every zone (the footer row) changes nothing.
    click(&mut app, 0, 23);
    assert_eq!(app.selected, 1);
}

#[test]
fn clicking_a_header_tab_chip_switches_tabs() {
    use gameday::keymap::Hit;
    let mut app = mk();
    app.apply_boards(League::Nfl, vec![g("1", "KC", "TB", true)], false);
    assert_eq!(app.tab, Tab::Home);
    let mut t = Terminal::new(TestBackend::new(120, 24)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let zone = zone_for(&app, Hit::TabChip(Tab::League(League::Nfl)));
    // While help is open the board is modal: clicks are inert.
    app.help_open = true;
    click(&mut app, zone.x, zone.y);
    assert_eq!(app.tab, Tab::Home, "clicks are inert under the help overlay");
    app.help_open = false;
    click(&mut app, zone.x, zone.y);
    assert_eq!(app.tab, Tab::League(League::Nfl));
}

#[test]
fn clicking_a_slate_row_selects_it() {
    use gameday::keymap::Hit;
    let mut app = mk();
    app.apply_boards(
        League::Nfl,
        vec![g("1", "KC", "TB", true), g("2", "DAL", "PHI", false)],
        false,
    );
    app.tab = Tab::League(League::Nfl);
    // 120x36 keeps the slate strip visible (it needs >= 24 body rows).
    let mut t = Terminal::new(TestBackend::new(120, 36)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let zone = zone_for(&app, Hit::SlateRow(0));
    click(&mut app, zone.x + 2, zone.y);
    // Selection list = live tiles first, then slate rows: 1 live + row 0.
    assert_eq!(app.selected, 1, "slate row 0 is selection index 1");
}

#[test]
fn clicking_a_zoom_tab_switches_it() {
    use gameday::keymap::Hit;
    use gameday::views::{View, ZoomTab};
    let mut app = mk();
    app.apply_boards(League::Nfl, vec![g("1", "KC", "TB", true)], false);
    app.view = View::Zoom {
        game_id: "1".into(),
        tab: ZoomTab::Overview,
    };
    app.zoom_scroll = 3;
    let mut t = Terminal::new(TestBackend::new(120, 24)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let zone = zone_for(&app, Hit::ZoomTab(ZoomTab::Stats));
    click(&mut app, zone.x, zone.y);
    assert_eq!(
        app.view,
        View::Zoom {
            game_id: "1".into(),
            tab: ZoomTab::Stats,
        }
    );
    assert_eq!(app.zoom_scroll, 0, "switching tabs resets the scroll");
}

#[test]
fn wheel_scrolls_the_plays_feed_and_clamps() {
    use gameday::views::View;
    let mut app = mk();
    let mut nfl = g("1", "KC", "TB", true);
    nfl.last_plays = vec![Play {
        clock: "1:27".into(),
        team: "KC".into(),
        text: "Mahomes to Kelce, 12 yd".into(),
        scoring: true,
        ..Default::default()
    }];
    app.apply_boards(League::Nfl, vec![nfl], false);
    app.apply_boards(League::Nba, vec![nba_game("2", "BOS", "LAL")], false);
    app.view = View::PlaysFeed;
    assert_eq!(app.feed_scroll, 0);
    wheel(&mut app, false);
    assert_eq!(app.feed_scroll, 1, "wheel down moves the marker down");
    wheel(&mut app, false);
    assert_eq!(app.feed_scroll, 1, "clamped at the bottom (2 rows)");
    wheel(&mut app, true);
    assert_eq!(app.feed_scroll, 0, "wheel up moves it back");
    wheel(&mut app, true);
    assert_eq!(app.feed_scroll, 0, "clamped at the top");
}

#[test]
fn wheel_on_the_board_moves_the_selection() {
    let mut app = mk();
    app.apply_boards(
        League::Nfl,
        vec![g("1", "KC", "TB", true), g("2", "DAL", "PHI", true)],
        false,
    );
    app.tab = Tab::League(League::Nfl);
    wheel(&mut app, false);
    assert_eq!(app.selected, 1);
    wheel(&mut app, true);
    assert_eq!(app.selected, 0);
}

// ---- Config view (Task 10) ----

/// A fresh, empty config dir per test so round-trip assertions can't see
/// another test's config.toml.
fn config_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("gd-cfgview-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn key(app: &mut App, code: crossterm::event::KeyCode) {
    gameday::input::handle_key(app, code, crossterm::event::KeyModifiers::NONE);
}

fn type_text(app: &mut App, text: &str) {
    for c in text.chars() {
        key(app, crossterm::event::KeyCode::Char(c));
    }
}

#[test]
fn config_view_renders_every_section() {
    use gameday::views::View;
    let mut app = App::new(Config::default_all(), vec![], config_dir("sections"), time::UtcOffset::UTC);
    app.config.favorites.push(gameday::config::Favorite {
        league: League::Nfl,
        team_abbr: "KC".into(),
    });
    app.view = View::ConfigView;
    let mut t = Terminal::new(TestBackend::new(120, 36)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t);
    for needle in [
        "CONFIG", "TABS", "[x] NFL", "FAVORITES", "★ NFL KC", "ADD FAVORITE", "THEME", "SCORE",
        "LAYOUT",
    ] {
        assert!(s.contains(needle), "missing {needle:?} in config view:\n{s}");
    }
}

#[test]
fn config_space_toggles_a_tab_and_round_trips_config_toml() {
    use crossterm::event::KeyCode;
    use gameday::views::View;
    let dir = config_dir("toggle");
    let mut app = App::new(Config::default_all(), vec![], dir.clone(), time::UtcOffset::UTC);
    app.view = View::ConfigView;
    // Cursor starts on the first row: the NFL tab toggle.
    key(&mut app, KeyCode::Char(' '));
    assert!(!app.config.enabled_tabs.contains(&League::Nfl), "space disables NFL");
    let saved = Config::load_from(&dir).unwrap();
    assert!(!saved.enabled_tabs.contains(&League::Nfl), "written through immediately");
    key(&mut app, KeyCode::Char(' '));
    assert!(app.config.enabled_tabs.contains(&League::Nfl), "space re-enables");
    let saved = Config::load_from(&dir).unwrap();
    assert!(saved.enabled_tabs.contains(&League::Nfl));
}

#[test]
fn config_enter_adds_a_typed_favorite_and_enter_removes_it() {
    use crossterm::event::KeyCode;
    use gameday::views::View;
    let dir = config_dir("fav");
    let mut app = App::new(Config::default_all(), vec![], dir.clone(), time::UtcOffset::UTC);
    app.apply_boards(League::Nfl, vec![g("1", "KC", "TB", true)], false);
    app.view = View::ConfigView;
    // 9 league rows (League::ALL), then the ADD FAVORITE row.
    for _ in 0..League::ALL.len() {
        key(&mut app, KeyCode::Char('j'));
    }
    key(&mut app, KeyCode::Enter);
    type_text(&mut app, "kc");
    key(&mut app, KeyCode::Enter);
    assert_eq!(
        app.config.favorites,
        vec![gameday::config::Favorite { league: League::Nfl, team_abbr: "KC".into() }],
        "abbr resolves its league from the boards"
    );
    let saved = Config::load_from(&dir).unwrap();
    assert_eq!(saved.favorites, app.config.favorites, "written through immediately");
    // The new favorite row took this index; Enter on it removes the favorite.
    key(&mut app, KeyCode::Enter);
    assert!(app.config.favorites.is_empty(), "enter on a favorite row removes it");
    let saved = Config::load_from(&dir).unwrap();
    assert!(saved.favorites.is_empty());
}

#[test]
fn config_favorite_miss_names_the_abbr_and_the_league_form() {
    use crossterm::event::KeyCode;
    use gameday::views::View;
    let mut app = App::new(Config::default_all(), vec![], config_dir("favmiss"), time::UtcOffset::UTC);
    app.view = View::ConfigView;
    for _ in 0..League::ALL.len() {
        key(&mut app, KeyCode::Char('j'));
    }
    key(&mut app, KeyCode::Enter);
    type_text(&mut app, "zzz");
    key(&mut app, KeyCode::Enter);
    assert!(app.config.favorites.is_empty());
    let status = app.status_line.clone().expect("miss status");
    assert!(status.contains("\"zzz\""), "{status}");
    // The two-token form works without the team being on a board.
    key(&mut app, KeyCode::Enter);
    type_text(&mut app, "nhl edm");
    key(&mut app, KeyCode::Enter);
    assert_eq!(
        app.config.favorites,
        vec![gameday::config::Favorite { league: League::Nhl, team_abbr: "EDM".into() }]
    );
}

#[test]
fn config_h_l_cycle_score_and_layout_and_persist() {
    use crossterm::event::KeyCode;
    use gameday::tiles::packer::LayoutPref;
    use gameday::tiles::ScoreStyle;
    use gameday::views::View;
    let dir = config_dir("cycle");
    let mut app = App::new(Config::default_all(), vec![], dir.clone(), time::UtcOffset::UTC);
    app.view = View::ConfigView;
    // Rows: 9 tabs, ADD FAVORITE, THEME, SCORE, LAYOUT.
    for _ in 0..League::ALL.len() + 2 {
        key(&mut app, KeyCode::Char('j'));
    }
    key(&mut app, KeyCode::Char('l'));
    assert_eq!(app.config.score_style, ScoreStyle::Compact, "l cycles score style");
    key(&mut app, KeyCode::Char('j'));
    key(&mut app, KeyCode::Char('l'));
    assert_eq!(app.config.layout, LayoutPref::One, "l cycles layout forward");
    key(&mut app, KeyCode::Char('h'));
    assert_eq!(app.config.layout, LayoutPref::Auto, "h cycles layout back");
    let saved = Config::load_from(&dir).unwrap();
    assert_eq!(saved.score_style, ScoreStyle::Compact);
    assert_eq!(saved.layout, LayoutPref::Auto);
}

#[test]
fn config_esc_pops_but_cancels_an_open_edit_first() {
    use crossterm::event::KeyCode;
    use gameday::views::View;
    let mut app = App::new(Config::default_all(), vec![], config_dir("escpop"), time::UtcOffset::UTC);
    app.view = View::ConfigView;
    for _ in 0..League::ALL.len() {
        key(&mut app, KeyCode::Char('j'));
    }
    key(&mut app, KeyCode::Enter);
    type_text(&mut app, "kc");
    key(&mut app, KeyCode::Esc);
    assert_eq!(app.view, View::ConfigView, "esc cancels the edit, not the view");
    assert!(app.config.favorites.is_empty(), "cancelled edit commits nothing");
    key(&mut app, KeyCode::Esc);
    assert_eq!(app.view, View::Board, "second esc pops to the board");
    // q pops too (it quits only from the Board).
    app.view = View::ConfigView;
    key(&mut app, KeyCode::Char('q'));
    assert_eq!(app.view, View::Board);
    assert!(!app.should_quit);
}
