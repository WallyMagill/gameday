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

/// Mirror a fixture's scoring rows into the model's scoring feed
/// (`Game.scoring_plays`, oldest-first) — what the app itself does from a
/// score delta or a summary. Fixtures author plays newest-first.
fn with_scoring(mut game: Game) -> Game {
    game.scoring_plays = game
        .last_plays
        .iter()
        .filter(|p| p.scoring)
        .rev()
        .cloned()
        .collect();
    game
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
    // v3.2 spec §1: only an enabled league with a game today earns a chip —
    // NFL and MLS both need boards to show up here at all.
    let mut app = mk();
    app.apply_boards(League::Nfl, vec![g("1", "KC", "TB", true)], false);
    let mut mls_game = g("2", "LAFC", "SEA", true);
    mls_game.league = League::Mls;
    app.apply_boards(League::Mls, vec![mls_game], false);
    // Wide enough for the whole header: the chips' brackets are the first
    // thing the shed ladder gives up (spec §1: no `FILTER:` label at all).
    let mut wide = Terminal::new(TestBackend::new(180, 24)).unwrap();
    wide.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&wide);
    assert!(s.contains("GAMEDAY"), "{s}");
    assert!(!s.contains("FILTER:"), "the FILTER: label is gone: {s}");
    assert!(s.contains("[ALL]"), "{s}");
    assert!(s.contains("[ NFL ]"), "{s}");
    // At 120 with the enabled chips the brackets shed instead of the clock
    // (R12), but the wordmark and every chip with a game still render.
    let mut t = Terminal::new(TestBackend::new(120, 24)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let row = buf_text(&t).lines().next().unwrap().to_string();
    assert!(row.contains("GAMEDAY"), "{row}");
    assert!(row.contains("ALL"), "{row}");
    assert!(row.contains("NFL") && row.contains("MLS"), "{row}");
    assert!(!row.contains("FILTER:"), "label sheds before the clock: {row}");
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
    // v3.2 spec §1: the Board footer is the lowercase A′ legend — no
    // brackets, no "NAV:" label.
    let mut app = mk();
    let mut t = Terminal::new(TestBackend::new(120, 24)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t);
    assert!(!s.contains("NAV:"), "{s}");
    assert!(s.contains("q quit"), "{s}");
    assert!(s.contains("space pin"), "{s}");
    assert!(s.contains("? help"), "{s}");
    // Theme never had a footer slot; still true of the new legend.
    assert!(!s.contains("THEME"), "{s}");
}

#[test]
fn footer_advertises_sort_and_tv_not_pages() {
    // v3.2 spec §1: the Board legend names `s sort` and `v tv`; the old caps
    // "PAGE"/"PIN [SPC]" style is gone.
    let mut app = mk();
    let mut t = Terminal::new(TestBackend::new(120, 24)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t);
    assert!(s.contains("s sort"), "{s}");
    assert!(s.contains("v tv"), "{s}");
    assert!(!s.contains("PAGE"), "{s}");
    assert!(!s.contains("PIN [SPC]"), "{s}");
}

#[test]
fn help_overlay_lists_every_group_and_the_hidden_chords() {
    // spec v3.3 §5: the "?" overlay speaks the same lowercase grammar as
    // every footer now — group titles, chords and labels are all lowercase,
    // no hand-written caps.
    let mut app = mk();
    app.help_open = true;
    let mut t = Terminal::new(TestBackend::new(120, 36)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t);
    for needle in [
        "keys", "navigation", "selection", "view", "app",
        // Chords the footer omits must still be discoverable here.
        "theme", "sort", "tv", "favorite", "ctrl-c", "s-tab",
    ] {
        assert!(s.contains(needle), "help overlay missing {needle:?}:\n{s}");
    }
}

#[test]
fn help_panel_speaks_the_lowercase_grammar_not_just_the_footer() {
    // spec v3.3 §5: the overlay's own KEYS panel — not just the footer strip
    // underneath it — must be lowercase. Round 1 fix: `App::draw_help` used
    // to render `NAVIGATION`, `SPC`, `ESC/?/Q CLOSES` verbatim, a second caps
    // grammar the footer-scoped tests never saw because they only inspect
    // the buffer's last row.
    let mut app = mk();
    app.help_open = true;
    let mut t = Terminal::new(TestBackend::new(120, 40)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t);
    // The panel's own new rows, lowercase.
    for needle in ["navigation", "space pin", "esc  q", "esc/?/q closes", " keys "] {
        assert!(s.contains(needle), "panel missing {needle:?}:\n{s}");
    }
    // No leftover v3.1 caps-token grammar (the old NAVIGATION/SPC/ESC-CLOSES
    // style, and the footer's own dead NAV: label).
    for caps in ["NAVIGATION", "SELECTION", " SPC", "ESC/?/Q CLOSES", "NAV:"] {
        assert!(!s.contains(caps), "leftover caps token {caps:?}:\n{s}");
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
    // spec v3.3 §5: every non-board footer speaks the lowercase legend now —
    // no "[ESC] BACK" bracket-caps, no "NAV:".
    assert!(s.contains("esc back"), "{s}");
    assert!(!s.contains("NAV:"), "{s}");
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
fn league_tab_with_only_scheduled_games_lists_them_under_later() {
    let mut app = mk();
    app.apply_boards(
        League::Nfl,
        vec![g("1", "KC", "TB", false), g("2", "DAL", "PHI", false)],
        false,
    );
    app.tab = Tab::League(League::Nfl);
    let mut t = Terminal::new(TestBackend::new(120, 36)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t);
    // v3.2 §7: no mosaic and no boxed SLATE — scheduled games are the board's
    // own LATER section, and the board is never blank above it.
    assert!(s.contains("LATER"), "{s}");
    assert!(s.contains("KC"), "{s}");
    assert!(s.contains("DAL"), "{s}");
    // The selected row still carries the caret.
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
    assert_eq!(bg_of("gruvbox"), Color::Rgb(0x28, 0x28, 0x28));
    assert_eq!(bg_of("studio"), theme::builtin("studio").bg);
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
    app.apply_boards(League::Nfl, vec![with_scoring(game)], false);
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
    // v3.2 §6/§7: the sidebar and the tile chrome are gone. What carries the
    // theme's discipline now is the board itself — section rules in `cool`,
    // and the identity floor on the hero's digits.
    assert_eq!(fg_at("IN PLAY"), studio.roles().cool);
    let team = studio.art_color([200, 16, 46]);
    let colored = (0..area.height)
        .flat_map(|y| (0..area.width).map(move |x| (x, y)))
        .filter(|&(x, y)| b[(x, y)].fg == team)
        .count();
    assert!(colored > 0, "the hero's digits keep team color:\n{}", buf_text(&t));
    theme::set_current("broadcast").unwrap();
}

// v3.2 §7: the GLOBAL ALERTS / TOP PLAYS / RECORDS sidebar is deleted, and
// with it `sidebar_top_plays_are_abbr_surname_clock` and
// `records_rail_shows_the_abbr_instead_of_a_clipped_name`. The scoring feed
// they rendered still exists (`Derived::scoring`) and is covered by the plays
// feed tests.

#[test]
fn empty_home_with_no_boards_points_at_config() {
    let mut app = mk();
    // A board actually arrived and carried nothing — before the first fetch
    // lands the board says NO DATA YET instead, which is a different claim.
    app.apply_boards(League::Nfl, vec![], false);
    let mut t = Terminal::new(TestBackend::new(120, 24)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t).to_lowercase();
    assert!(s.contains("nothing live on the enabled boards"), "{s}");
}

#[test]
fn home_with_nothing_live_still_lists_the_day() {
    let mut app = mk();
    // v3.2 §1: Home is ONE list of the day, so a board with nothing live is
    // not an empty board — it is a LATER section. (The "nothing live · next:"
    // message stays for a board with no games at all; the empty-Home strings
    // themselves are pinned by `empty_home_with_no_boards_points_at_config`.)
    app.apply_boards(League::Nfl, vec![g("1", "KC", "TB", false)], false);
    let mut t = Terminal::new(TestBackend::new(120, 24)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t);
    assert!(s.contains("LATER"), "{s}");
    assert!(s.contains("KC") && s.contains("TB"), "{s}");
    assert!(!s.contains("nothing live"), "a scheduled game is not an empty board:\n{s}");
}

#[test]
fn too_small_message() {
    let mut app = mk();
    let mut t = Terminal::new(TestBackend::new(30, 10)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t);
    assert!(s.contains("need 40×12, have 30×10"), "{s}");
}

#[test]
fn the_minimum_size_message_names_both_numbers() {
    // Walter's rule: a limit someone can hit must name the actual and the
    // expected value — 39x11 must say both 40x12 (the floor) and 39x11 (what
    // they actually have), not a bare "need more columns".
    let mut app = mk();
    let mut t = Terminal::new(TestBackend::new(39, 11)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t);
    assert!(s.contains("need 40×12, have 39×11"), "{s}");
}

#[test]
fn nfl_tab_draws_live_score() {
    let mut app = mk();
    app.apply_boards(League::Nfl, vec![g("1", "KC", "TB", true)], false);
    app.tab = Tab::League(League::Nfl);
    let mut t = Terminal::new(TestBackend::new(120, 24)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t);
    // v3.2 §1: the only live game is the hero — its score is digit glyphs and
    // the tile's "[NFL] LIVE" chip is gone with the tile.
    assert!(s.contains("KC") && s.contains("TB"), "{s}");
    assert!(s.contains('█') || s.contains("27"), "the score renders in some form:\n{s}");
    assert!(s.contains("IN PLAY"), "{s}");
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
        s.contains("no games match \"zzz\" on NFL"),
        "empty filter result must name the pattern and the scope it searched: {s}"
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
    app.apply_boards(League::Nfl, vec![with_scoring(game)], false);
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
fn footer_advertises_filter_chord_but_not_cmd_on_the_board() {
    // v3.2 spec §1: `/ filter` is in the fixed Board legend; `:` earns no
    // footer slot there (CMD is still reachable via `:` and the `?`
    // overlay) — the old bracket-caps "[:] CMD"/"[/] FILTER" style is gone.
    let mut app = mk();
    let mut t = Terminal::new(TestBackend::new(120, 24)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t);
    assert!(s.contains("/ filter"), "{s}");
    assert!(!s.contains("CMD"), "{s}");
    // spec v3.3 §5: the Zoomed footer keeps the generic list, CMD included —
    // just lowercase now, same as every other non-board view.
    use gameday::views::{View, ZoomTab};
    app.apply_boards(League::Nfl, vec![g("1", "KC", "TB", true)], false);
    app.tab = Tab::League(League::Nfl);
    app.view = View::Zoom { game_id: "1".into(), tab: ZoomTab::Overview };
    let mut tz = Terminal::new(TestBackend::new(120, 24)).unwrap();
    tz.draw(|f| app.draw(f)).unwrap();
    let sz = buf_text(&tz);
    assert!(sz.contains(": cmd"), "{sz}");
    assert!(sz.contains("/ filter"), "{sz}");
    assert!(!sz.contains("NAV:"), "{sz}");
}

#[test]
fn narrow_footer_sheds_low_value_chords_but_keeps_help_and_quit() {
    // Narrow enough that the whole legend can't fit; move/pin go first, the
    // way out and help never get clipped (spec §1's shed order discipline).
    let mut app = mk();
    let mut t = Terminal::new(TestBackend::new(45, 24)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let footer = buf_text(&t).lines().last().unwrap().to_string();
    assert!(footer.contains("? help"), "help clipped: {footer:?}");
    assert!(footer.contains("q quit"), "quit clipped: {footer:?}");
    assert!(!footer.contains("move"), "move should be shed first: {footer:?}");
}

/// Longest run of consecutive ASCII-uppercase letters in `s`, excluding the
/// footer's known status readouts (`FOCUS`, `UPD`, `GAME`) — those are
/// clock-shaped status text, not the "NAV:" chord grammar spec v3.3 §5
/// deletes. Team abbreviations (`KC`, `TB`) are 2-3 letters and never trip
/// the ≤3 budget on their own.
fn max_caps_run_excluding_status(s: &str) -> usize {
    let stripped = s.replace("FOCUS", "").replace("UPD", "").replace("GAME", "");
    let mut max = 0;
    let mut run = 0;
    for c in stripped.chars() {
        if c.is_ascii_uppercase() {
            run += 1;
            max = max.max(run);
        } else {
            run = 0;
        }
    }
    max
}

#[test]
fn every_view_speaks_the_lowercase_footer() {
    // spec v3.3 §5: one footer grammar everywhere — plays feed, standings,
    // config, zoom, help (over the board) and the theme picker all render
    // their footer from the keymap, lowercase, no "NAV:", no "[TAB]"
    // bracket-caps.
    use gameday::views::{View, ZoomTab};
    type Setup = (&'static str, Box<dyn Fn(&mut App)>);
    let cases: Vec<Setup> = vec![
        (
            "plays feed",
            Box::new(|app: &mut App| {
                app.config.enabled_tabs = vec![League::Nfl];
                app.apply_boards(League::Nfl, vec![g("1", "KC", "TB", true)], false);
                app.view = View::PlaysFeed;
            }),
        ),
        (
            "standings",
            Box::new(|app: &mut App| {
                app.config.enabled_tabs = vec![League::Nfl];
                app.apply_boards(League::Nfl, vec![g("1", "KC", "TB", true)], false);
                app.view = View::Standings(League::Nfl);
            }),
        ),
        (
            "config",
            Box::new(|app: &mut App| {
                app.view = View::ConfigView;
            }),
        ),
        (
            "zoom",
            Box::new(|app: &mut App| {
                app.apply_boards(League::Nfl, vec![g("1", "KC", "TB", true)], false);
                app.tab = Tab::League(League::Nfl);
                app.view = View::Zoom { game_id: "1".into(), tab: ZoomTab::Overview };
            }),
        ),
        (
            "help",
            Box::new(|app: &mut App| {
                app.help_open = true;
            }),
        ),
        (
            "theme picker",
            Box::new(|app: &mut App| {
                app.view = View::ThemePicker;
            }),
        ),
    ];
    for (name, setup) in cases {
        let mut app = mk();
        setup(&mut app);
        let mut t = Terminal::new(TestBackend::new(120, 40)).unwrap();
        t.draw(|f| app.draw(f)).unwrap();
        let s = buf_text(&t);
        // spec v3.3 §5: the key bar is no longer always the last row — the
        // config editor anchors it under its block — so find the legend
        // rather than assuming the terminal floor.
        let footer = s
            .lines()
            .find(|l| l.contains("? help"))
            .unwrap_or_else(|| panic!("{name}: no key bar:\n{s}"));
        assert!(!footer.contains("NAV:"), "{name}: {footer:?}");
        assert!(!footer.contains("[TAB]"), "{name}: {footer:?}");
        assert!(footer.contains("? help"), "{name}: {footer:?}");
        assert!(
            max_caps_run_excluding_status(footer) <= 3,
            "{name}: caps run too long: {footer:?}"
        );
    }
}

#[test]
fn footers_shed_in_order_and_help_quit_survive_at_40_cols() {
    // spec v3.3 §5: FOOTER_DROP_ORDER's shed discipline carries over to
    // every view — HELP and the way back (BACK, ESC/Q, since 'q' pops
    // rather than quits off the Board) are never the ones clipped.
    use gameday::views::View;
    for view in [View::PlaysFeed, View::Standings(League::Nfl)] {
        let mut app = mk();
        app.config.enabled_tabs = vec![League::Nfl];
        app.apply_boards(League::Nfl, vec![g("1", "KC", "TB", true)], false);
        app.view = view.clone();
        let mut t = Terminal::new(TestBackend::new(40, 24)).unwrap();
        t.draw(|f| app.draw(f)).unwrap();
        // spec v3.3 §5: a feed that fits its pane pulls the key bar up under
        // its last row, so find the legend instead of taking the floor row.
        let s = buf_text(&t);
        let footer = s
            .lines()
            .find(|l| l.contains("? help"))
            .unwrap_or_else(|| panic!("{view:?}: no key bar:\n{s}"))
            .to_string();
        assert!(footer.contains("? help"), "{view:?}: help clipped: {footer:?}");
        assert!(footer.contains("esc back"), "{view:?}: back clipped: {footer:?}");
        assert!(!footer.contains("NAV:"), "{view:?}: {footer:?}");
    }
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
    with_scoring(Game {
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
    })
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
    app.apply_boards(League::Nfl, vec![with_scoring(nfl)], false);
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
    assert!(nfl_row.contains("NFL") && nfl_row.contains("TOUCHDOWN"), "{nfl_row}"); // spec v3.3 §7
    assert!(nfl_row.contains("KC@TB") && nfl_row.contains("27-24"), "{nfl_row}");
    let nba_row = lines
        .iter()
        .find(|l| l.contains("Tatum pull-up three"))
        .unwrap_or_else(|| panic!("NBA scoring play missing from feed:\n{s}"));
    assert!(nba_row.contains("NBA") && nba_row.contains("BUCKET"), "{nba_row}"); // spec v3.3 §7
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
    app.apply_boards(League::Nfl, vec![with_scoring(nfl)], false);
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
        season: None,
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
        fetched_at: None,
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

/// With no CFB table in hand the view points at the per-conference command
/// rather than saying "no standings yet", which reads as a fetch still in
/// flight — and never claims ESPN has no FBS table, because it does.
#[test]
fn standings_view_for_cfb_names_the_per_conference_workaround() {
    use gameday::views::View;
    let mut app = mk();
    app.view = View::Standings(League::Cfb);
    let mut t = Terminal::new(TestBackend::new(120, 24)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t);
    assert!(s.contains("no FBS standings right now"), "{s}");
    assert!(s.contains(":standings <conf>"), "the workaround must be named:\n{s}");
    assert!(!s.contains("no standings yet"), "{s}");
}

/// The header dates the table: the feed's own season when it sent one, and
/// otherwise when we took the snapshot — never nothing, which reads as live.
#[test]
fn standings_header_carries_the_season_else_when_it_was_fetched() {
    use gameday::views::View;
    let mut app = mk();
    app.view = View::Standings(League::Nfl);
    app.merge_standings(standings_table());
    let mut t = Terminal::new(TestBackend::new(120, 36)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t);
    let header = s.lines().find(|l| l.contains("STANDINGS")).unwrap();
    assert!(header.contains("·  updated "), "no updated label:\n{header:?}");
    assert!(!header.contains("2025-26"), "{header:?}");

    let mut table = standings_table();
    table.season = Some("2025-26".into());
    app.merge_standings(table);
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t);
    let header = s.lines().find(|l| l.contains("STANDINGS")).unwrap();
    assert!(header.contains("·  2025-26"), "no season label:\n{header:?}");
    assert!(!header.contains("updated"), "season wins over the age:\n{header:?}");
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
        season: None,
        groups: vec![group("American Football Conference", "A"), group("National Football Conference", "N")],
        fetched_at: None,
    }
}

#[test]
fn standings_scroll_clamps_to_the_pane_so_k_moves_back_at_once() {
    use crossterm::event::{KeyCode, KeyModifiers};
    use gameday::views::View;
    let mut app = mk();
    app.view = View::Standings(League::Nfl);
    app.merge_standings(tall_standings_table());
    // spec v3.3 §5: 80 cols, where the table is one column and 41 lines
    // genuinely overflow a 24-row pane — at 120 the two conferences sit side
    // by side and this table fits without scrolling at all.
    let mut t = Terminal::new(TestBackend::new(80, 24)).unwrap();
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
    // spec v3.3 §5: 80 cols — the one-column width, where a 41-line table is
    // actually clipped by a 24-row pane.
    let mut t = Terminal::new(TestBackend::new(80, 24)).unwrap();
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
        // spec v3.3 §5: the key bar follows the content when the view fits,
        // so it is not always the floor row.
        let footer = s
            .lines()
            .find(|l| l.contains("? help"))
            .unwrap_or_else(|| panic!("{view:?}: no key bar:\n{s}"));
        // spec v3.3 §5: lowercase, no "NAV:", no bracket-caps.
        assert!(!footer.contains("NAV:"), "{view:?}: {footer}");
        assert!(!footer.contains("tabs"), "{view:?}: zoom's tab cycle is a no-op here: {footer}");
        assert!(footer.contains("back"), "{view:?}: {footer}");
        // "tab league" is advertised, so Tab must actually switch tabs.
        assert!(footer.contains("league"), "{view:?}: {footer}");
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
    app.apply_boards(League::Nfl, vec![with_scoring(nfl)], false);
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

    // A merged dated board replaces the league's visible games. The date has
    // to be the one the APP is viewing: `mk()` builds it at `UtcOffset::UTC`,
    // so a fixture keyed off `now_local()` missed the board by a day in every
    // wall-clock window where the local date and the UTC date differ (this
    // test failed every evening west of UTC). Ask the app.
    let date = app
        .viewed_date(League::Nfl)
        .expect("one step back is a traveled date");
    let mut final_game = g("d1", "DAL", "PHI", false);
    final_game.status = Status::Final;
    app.merge_dated_board(League::Nfl, date, vec![final_game]);
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t);
    assert!(s.contains("DAL") && s.contains("PHI"), "{s}");
    // v3.2 §1: no ticker under the board, so a traveled board shows the
    // traveled slate and nothing of today.
    assert!(!s.contains("KC"), "today's board is hidden while traveling:\n{s}");

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
fn a_later_row_shows_its_odds() {
    let mut app = mk();
    let mut game = g("1", "KC", "TB", false); // Pre
    game.last_plays.clear(); // a pre-game has no plays
    game.situation = None;
    game.odds = Some("KC -3.5  O/U 47.5".into());
    app.apply_boards(League::Nfl, vec![with_scoring(game)], false);
    app.tab = Tab::League(League::Nfl);
    let mut t = Terminal::new(TestBackend::new(120, 36)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t);
    // v3.2 §1: once, on the LATER row (the mosaic tile that repeated it is
    // deleted).
    assert!(s.contains("LATER"), "{s}");
    assert_eq!(s.matches("O/U 47.5").count(), 1, "{s}");
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
fn clicking_a_row_selects_it() {
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
    // v3.2 §7: tiles are gone — every board row is a Hit::Row zone.
    let zone = zone_for(&app, Hit::Row(1));
    click(&mut app, zone.x + zone.width / 2, zone.y + zone.height / 2);
    assert_eq!(app.selected, 1, "click on the second row selects it");
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
fn clicking_a_later_row_selects_it() {
    use gameday::keymap::Hit;
    let mut app = mk();
    app.apply_boards(
        League::Nfl,
        vec![g("1", "KC", "TB", true), g("2", "DAL", "PHI", false)],
        false,
    );
    app.tab = Tab::League(League::Nfl);
    // v3.2 §7: the SLATE strip is gone; a LATER row is a board row like any
    // other, at its `Derived::selection` index (1 live + this one).
    let mut t = Terminal::new(TestBackend::new(120, 36)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let zone = zone_for(&app, Hit::Row(1));
    click(&mut app, zone.x + 2, zone.y);
    assert_eq!(app.selected, 1, "the LATER row is selection index 1");
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
    app.apply_boards(League::Nfl, vec![with_scoring(nfl)], false);
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
        "CONFIG", "TABS", "[x] NFL", "FAVORITES", "★ NFL KC", "ADD FAVORITE", "THEME", "SORT",
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

// Task 10 (spec §9): the config editor's SCORE/LAYOUT rows are gone; DISPLAY
// is THEME + SORT now.
#[test]
fn config_h_l_cycle_sort_and_persist() {
    use crossterm::event::KeyCode;
    use gameday::rank::SortKey;
    use gameday::views::View;
    let dir = config_dir("cycle");
    let mut app = App::new(Config::default_all(), vec![], dir.clone(), time::UtcOffset::UTC);
    app.view = View::ConfigView;
    // Rows: 9 tabs, ADD FAVORITE, THEME, SORT.
    for _ in 0..League::ALL.len() + 2 {
        key(&mut app, KeyCode::Char('j'));
    }
    key(&mut app, KeyCode::Char('l'));
    assert_eq!(app.config.sort, SortKey::Time, "l cycles sort forward");
    key(&mut app, KeyCode::Char('h'));
    assert_eq!(app.config.sort, SortKey::Watch, "h cycles sort back");
    key(&mut app, KeyCode::Char('l'));
    let saved = Config::load_from(&dir).unwrap();
    assert_eq!(saved.sort, SortKey::Time);
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

#[test]
fn pregame_tile_and_slate_show_local_start_never_iso() {
    let mut app = App::new(
        Config::default_all(),
        vec![],
        std::env::temp_dir(),
        time::UtcOffset::from_hms(-4, 0, 0).unwrap(),
    );
    app.now_override = Some(time::macros::datetime!(2026-09-10 12:00 -4));
    let mut pre = g("1", "NE", "SEA", false);
    pre.start = Some(time::macros::datetime!(2026-09-10 20:20 -4));
    app.apply_boards(League::Nfl, vec![pre], false);
    app.tab = Tab::League(League::Nfl);
    let mut term = Terminal::new(TestBackend::new(120, 40)).unwrap();
    term.draw(|f| app.draw(f)).unwrap();
    let text = buf_text(&term);
    assert!(text.contains("8:20 PM"), "{text}");
    assert!(!text.contains("2026-"), "raw ISO leaked: {text}");
}

#[test]
fn pinned_and_favorited_tiles_carry_a_glyph_in_the_title() {
    let mut app = App::new(
        Config::default_all(),
        vec![],
        std::env::temp_dir(),
        time::UtcOffset::UTC,
    );
    app.apply_boards(League::Nfl, vec![g("1", "KC", "TB", true)], false);
    app.tab = Tab::League(League::Nfl);
    app.on_key(
        crossterm::event::KeyCode::Char(' '),
        crossterm::event::KeyModifiers::NONE,
    );
    let mut term = Terminal::new(TestBackend::new(120, 40)).unwrap();
    term.draw(|f| app.draw(f)).unwrap();
    let text = buf_text(&term);
    assert!(text.contains("⚑"), "pin glyph missing: {text}");
    assert!(text.contains("pinned KC@TB"), "toast missing: {text}");
    app.on_key(
        crossterm::event::KeyCode::Char('t'),
        crossterm::event::KeyModifiers::NONE,
    );
    term.draw(|f| app.draw(f)).unwrap();
    let text = buf_text(&term);
    assert!(text.contains("★"), "favorite glyph missing: {text}");
}

#[test]
fn baseball_play_rows_show_the_inning_not_a_dash_clock() {
    let mut app = App::new(
        Config::default_all(),
        vec![],
        std::env::temp_dir(),
        time::UtcOffset::UTC,
    );
    let mut mlb = g("1", "SEA", "BOS", true);
    mlb.league = League::Mlb;
    mlb.period = "BOT 9TH".into();
    mlb.clock = String::new();
    mlb.last_plays = vec![Play {
        period: "B9".into(),
        team: "SEA".into(),
        text: "Rodríguez singles".into(),
        ..Default::default()
    }];
    app.apply_boards(League::Mlb, vec![mlb], false);
    app.tab = Tab::League(League::Mlb);
    let mut term = Terminal::new(TestBackend::new(120, 40)).unwrap();
    term.draw(|f| app.draw(f)).unwrap();
    let text = buf_text(&term);
    // v3.2 §1: the tile's `[B9]` play stamp went with the tile; the hero
    // prints the play itself and the clock column says the inning.
    assert!(text.contains("BOT 9TH"), "{text}");
    assert!(text.contains("Rodríguez singles"), "{text}");
    assert!(!text.contains("-:--"), "{text}");
}

#[test]
fn long_names_keep_their_record_as_the_abbr_form() {
    let mut app = App::new(
        Config::default_all(),
        vec![],
        std::env::temp_dir(),
        time::UtcOffset::UTC,
    );
    let games: Vec<Game> = (0..4)
        .map(|i| {
            let mut game = g(&format!("{i}"), "SEA", "BOS", true);
            game.away.name = "Mariners".into();
            game.away.record = "64-73".into();
            game.home.name = "Red Sox".into();
            game.home.record = "74-63".into();
            game
        })
        .collect();
    app.apply_boards(League::Nfl, games, false); // 2x2 => narrow tiles
    app.tab = Tab::League(League::Nfl);
    let mut term = Terminal::new(TestBackend::new(120, 40)).unwrap();
    term.draw(|f| app.draw(f)).unwrap();
    let text = buf_text(&term);
    assert!(
        text.contains("64-73") && text.contains("74-63"),
        "records dropped: {text}"
    );
}

#[test]
fn zoom_overview_carries_the_linescore_with_hits_and_errors() {
    use gameday::views::{View, ZoomTab};
    let mut app = App::new(
        Config::default_all(),
        vec![],
        std::env::temp_dir(),
        time::UtcOffset::UTC,
    );
    let mut mlb = g("1", "SEA", "BOS", true);
    mlb.league = League::Mlb;
    mlb.away_score = 3;
    mlb.home_score = 2;
    mlb.linescore = vec![(1, 0), (0, 2), (2, 0)];
    mlb.extras = Extras::Baseball {
        hits: Some((8, 5)),
        errors: Some((0, 1)),
    };
    app.apply_boards(League::Mlb, vec![mlb], false);
    app.tab = Tab::League(League::Mlb);
    app.view = View::Zoom {
        game_id: "1".into(),
        tab: ZoomTab::Overview,
    };
    let mut term = Terminal::new(TestBackend::new(120, 40)).unwrap();
    term.draw(|f| app.draw(f)).unwrap();
    let text = buf_text(&term);
    let lines: Vec<&str> = text.lines().collect();
    let i = lines
        .iter()
        .position(|l| l.contains("  1  2  3"))
        .unwrap_or_else(|| panic!("period header missing:\n{text}"));
    let head = lines[i];
    assert!(head.contains('R') && head.contains('H') && head.contains('E'), "{head:?}");
    // Away row: per-inning runs, then R H E — R is the game score, not a sum.
    let away = lines[i + 1];
    assert!(away.trim_start().starts_with("SEA"), "away row: {away:?}");
    assert!(away.contains("  1  0  2   3  8  0"), "away R H E: {away:?}");
    assert!(lines[i + 2].contains("  0  2  0   2  5  1"), "home R H E: {:?}", lines[i + 2]);
    // A short pane keeps the tile whole instead of a headless strip.
    let mut short = Terminal::new(TestBackend::new(120, 19)).unwrap();
    short.draw(|f| app.draw(f)).unwrap();
    assert!(
        !buf_text(&short).contains("  1  2  3"),
        "linescore should be skipped under 20 rows"
    );
}

#[test]
fn header_chip_is_its_own_cell_and_offline_names_the_error() {
    let mut app = mk();
    let mut term = Terminal::new(TestBackend::new(120, 40)).unwrap();
    term.draw(|f| app.draw(f)).unwrap();
    let text = buf_text(&term);
    assert!(text.contains("NO DATA YET"), "{text}");
    app.note_failure(
        League::Nfl,
        "ESPN 403 nfl scoreboard".into(),
        Some(std::time::Duration::from_secs(40)),
    );
    term.draw(|f| app.draw(f)).unwrap();
    let text = buf_text(&term);
    assert!(text.contains("OFFLINE"), "{text}");
    assert!(
        text.contains("ESPN 403 nfl scoreboard"),
        "board area names the error: {text}"
    );
    assert!(text.contains("retry 40s"), "{text}");
    assert!(
        !text.contains("OFFLINEMON") && !text.contains("STALEMON"),
        "chip glued to the date: {text}"
    );
    // Wide enough for the whole header: chip, date and clock all fit, and
    // the chip's padding keeps it off the date.
    let mut wide = Terminal::new(TestBackend::new(180, 40)).unwrap();
    wide.draw(|f| app.draw(f)).unwrap();
    let row: String = {
        let b = wide.backend().buffer();
        (0..b.area().width).map(|x| b[(x, 0)].symbol().to_string()).collect()
    };
    assert!(row.contains("OFFLINE · retry 40s  "), "chip padded: {row:?}");
    let chip_end = row.find("retry 40s").unwrap() + "retry 40s".len();
    assert!(
        row[chip_end..].trim_start().len() > 8,
        "date and clock still render after the chip: {row:?}"
    );
}

#[test]
fn a_narrow_header_shortens_the_chip_instead_of_chopping_it() {
    // Every league enabled at 50 cols: even with the left side shed all the
    // way down (no FILTER: label, no brackets, one tab, "GD"), the padded
    // "OFFLINE · retry 40s" cannot fit, so the chip degrades to its bare
    // state word. A chopped "OFFLINE · retry 4" would be a lie about the
    // retry.
    let mut app = mk();
    app.note_failure(
        League::Nfl,
        "ESPN 403 nfl scoreboard".into(),
        Some(std::time::Duration::from_secs(40)),
    );
    let mut t = Terminal::new(TestBackend::new(50, 40)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let b = t.backend().buffer();
    let row: String = (0..b.area().width).map(|x| b[(x, 0)].symbol().to_string()).collect();
    assert!(row.contains("OFFLINE"), "chip survives a narrow header: {row:?}");
    assert!(!row.contains("retry 4"), "chopped retry tail: {row:?}");
}

#[test]
fn header_shows_sort_key_and_only_leagues_with_games() {
    let mut app = mk();
    app.apply_boards(League::Nfl, vec![g("1", "KC", "TB", true)], false);
    // NBA is enabled but has no board applied at all — no chip for it.
    let mut t = Terminal::new(TestBackend::new(180, 40)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let first = buf_text(&t).lines().next().unwrap().to_string();
    assert!(first.contains("s SORT: WATCH"), "{first}");
    assert!(first.contains("NFL"), "{first}");
    assert!(!first.contains("NBA"), "a league with no games gets no chip: {first}");

    // The sort chip is Board-only — it disappears in Zoom.
    use gameday::views::{View, ZoomTab};
    app.view = View::Zoom { game_id: "1".into(), tab: ZoomTab::Overview };
    let mut tz = Terminal::new(TestBackend::new(180, 40)).unwrap();
    tz.draw(|f| app.draw(f)).unwrap();
    let zoomed_first = buf_text(&tz).lines().next().unwrap().to_string();
    assert!(!zoomed_first.contains("SORT:"), "{zoomed_first}");

    // Clock survives the sort chip across the same width sweep R12 already
    // guarantees for the net chip and the tab ladder.
    for width in 40u16..=180 {
        let mut app = mk();
        app.now_override = Some(time::macros::datetime!(2026-08-31 21:37:05 +0));
        app.apply_boards(League::Nfl, vec![g("1", "KC", "TB", true)], false);
        let mut t = Terminal::new(TestBackend::new(width, 40)).unwrap();
        t.draw(|f| app.draw(f)).unwrap();
        let row = buf_text(&t).lines().next().unwrap().to_string();
        assert!(row.contains("9:37:05 PM"), "clock clipped at {width} with the sort chip: {row:?}");
    }
}

/// Every one of the 9 leagues carries a live game today, so every enabled
/// league's chip actually earns its place (spec §1: only a league with a
/// game today earns a chip). Ten chips total with `ALL` — what
/// `header_keeps_the_clock_with_ten_chips_at_120_columns` and the width
/// sweep below were named for, before the chip-gating change made a
/// boardless fixture render 0-1 chips regardless of the name (task-9 review
/// carry-forward #1).
fn app_with_every_league_live() -> App {
    let mut app = App::new(Config::default_all(), vec![], std::env::temp_dir(), time::UtcOffset::UTC);
    for league in League::ALL {
        let mut game = g("1", "AAA", "BBB", true);
        game.league = league;
        app.apply_boards(league, vec![game], false);
    }
    app
}

#[test]
fn header_keeps_the_clock_with_ten_chips_at_120_columns() {
    let mut app = app_with_every_league_live();
    app.now_override = Some(time::macros::datetime!(2026-08-31 21:37:05 +0));
    let mut term = Terminal::new(TestBackend::new(120, 40)).unwrap();
    term.draw(|f| app.draw(f)).unwrap();
    let first = buf_text(&term).lines().next().unwrap().to_string();
    assert!(first.contains("9:37:05 PM"), "clock clipped: {first}");
    // All 9 league chips actually earned a slot (the fixture's whole point);
    // the rule under test is that the clock survives ten real chips, not
    // whatever the shed ladder left of an empty header.
    for league in League::ALL {
        assert!(
            first.contains(&league.slug().to_uppercase()),
            "{} chip missing at 120 cols with every league live: {first}",
            league.slug()
        );
    }
}

#[test]
fn header_keeps_the_clock_and_the_selected_tab_at_eighty_columns() {
    // Ten chips at 80 cols: the shed ladder runs out of league tabs long
    // before it touches the right side. The selected tab is the one chip
    // that never sheds, and the clock is never clipped.
    //
    // Task-16 carry (task-9 review carry-forward #1, finished): this used a
    // boardless fixture, which under chip-gating renders 0-1 chips — the shed
    // ladder was never asked to shed anything. On the nine-league fixture the
    // header genuinely overflows 80 columns, so shedding is what the test
    // exercises: something must go, and it is never ALL, never NFL, never the
    // clock.
    let mut app = app_with_every_league_live();
    app.now_override = Some(time::macros::datetime!(2026-08-31 21:37:05 +0));
    app.tab = Tab::League(League::Nfl);
    let mut term = Terminal::new(TestBackend::new(80, 40)).unwrap();
    term.draw(|f| app.draw(f)).unwrap();
    let first = buf_text(&term).lines().next().unwrap().to_string();
    assert!(first.contains("9:37:05 PM"), "clock clipped: {first}");
    assert!(first.contains("NFL"), "selected tab shed: {first}");
    assert!(first.contains("ALL"), "ALL chip shed: {first}");
    // The ladder DID descend: ten bracketed chips cannot fit 80 columns, so
    // the brackets are the rung that got given up. Every chip still reads —
    // decoration sheds before content, and content sheds before the clock.
    assert!(
        !first.contains("[ NFL ]") && !first.contains("[ALL]"),
        "brackets survived 80 cols with all nine leagues live — the ladder never descended: {first}"
    );
    for league in League::ALL {
        assert!(
            first.contains(&league.slug().to_uppercase()),
            "{} chip shed at 80 cols before the brackets did: {first}",
            league.slug()
        );
    }
}

#[test]
fn the_clock_survives_every_width_the_board_will_draw_at() {
    // R12's rule swept: from the narrowest board the app will render (40)
    // up, the clock is always whole and the status chip is never glued to
    // whatever follows it. Caught a clipped "9:37:05" at 40 and an
    // "OFFLINE9:37:05 PM" at 60.
    //
    // Task-9 review carry-forward #1: a boardless fixture renders 0-1 chips
    // under the new chip-gating (a league only earns a chip with a game
    // today), so this sweep was never actually exercising "many chips" — the
    // never-clip guarantee needs every league fighting for space to mean
    // anything.
    for width in 40u16..=180 {
        for tab in [Tab::Home, Tab::League(League::Mls)] {
            let mut app = app_with_every_league_live();
            app.now_override = Some(time::macros::datetime!(2026-08-31 21:37:05 +0));
            app.tab = tab;
            let mut t = Terminal::new(TestBackend::new(width, 40)).unwrap();
            t.draw(|f| app.draw(f)).unwrap();
            let b = t.backend().buffer();
            let row: String = (0..b.area().width).map(|x| b[(x, 0)].symbol().to_string()).collect();
            assert!(row.contains("9:37:05 PM"), "clock clipped at {width} tab={tab:?}: {row:?}");
            assert!(!row.contains("YET9") && !row.contains("YETMON"), "chip glued at {width} tab={tab:?}: {row:?}");
        }
    }
}

#[test]
fn command_completion_shows_the_candidates_in_the_footer() {
    let mut app = App::new(Config::default_all(), vec![], std::env::temp_dir(), time::UtcOffset::UTC);
    for c in [':', 'n'] { gameday::input::handle_key(&mut app, crossterm::event::KeyCode::Char(c), crossterm::event::KeyModifiers::NONE); }
    gameday::input::handle_key(&mut app, crossterm::event::KeyCode::Tab, crossterm::event::KeyModifiers::NONE);
    let mut term = Terminal::new(TestBackend::new(120, 40)).unwrap();
    term.draw(|f| app.draw(f)).unwrap();
    let last = buf_text(&term).lines().last().unwrap().to_string();
    assert!(last.contains(":nfl") && last.contains("nba") && last.contains("nhl"), "{last}");
}

// ---------------------------------------------------------------------------
// v3.2 §1: the ranked board — one list, sections, band, selection, lane.

/// `live` live games, then `finals`, then `later`, all NFL, distinct abbrs so
/// a row can be found by text. Scores differ per game so the ranked order is
/// observable.
fn board_games(live: usize, finals: usize, later: usize) -> Vec<Game> {
    const PAIRS: [(&str, &str); 12] = [
        ("KC", "TB"), ("DAL", "PHI"), ("GB", "CHI"), ("SF", "SEA"),
        ("BUF", "MIA"), ("NYJ", "NE"), ("DEN", "LV"), ("ATL", "NO"),
        ("CIN", "BAL"), ("PIT", "CLE"), ("HOU", "IND"), ("MIN", "DET"),
    ];
    let mut out = Vec::new();
    let mut next = 0usize;
    for (tag, n, status) in [
        ("l", live, Status::Live),
        ("f", finals, Status::Final),
        ("p", later, Status::Pre),
    ] {
        for i in 0..n {
            let (away, home) = PAIRS[next % PAIRS.len()];
            next += 1;
            let mut game = g(&format!("{tag}{i}"), away, home, status == Status::Live);
            game.status = status;
            game.away_score = 10 + i as u16;
            game.home_score = 7 + i as u16;
            out.push(game);
        }
    }
    out
}

fn board_app(live: usize, finals: usize, later: usize) -> App {
    let mut app = mk();
    app.apply_boards(League::Nfl, board_games(live, finals, later), false);
    app
}

fn render(app: &mut App, w: u16, h: u16) -> Terminal<TestBackend> {
    let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
    term.draw(|f| app.draw(f)).unwrap();
    term
}

#[test]
fn the_board_is_one_ranked_list_with_sections() {
    let mut app = board_app(6, 2, 2);
    let term = render(&mut app, 120, 40);
    let s = buf_text(&term);
    assert!(!s.contains("MY GAMES"), "no pins, no band:\n{s}");
    assert!(s.contains("IN PLAY"), "the live section names itself:\n{s}");
    assert!(s.contains("SORTED BY WATCHABILITY"), "the rule names the sort:\n{s}");
    assert!(s.contains("FINAL"), "FINAL section:\n{s}");
    assert!(s.contains("LATER"), "LATER section:\n{s}");
    // The hero's digits are drawn as glyph cells, not as "27 - 24" text — and
    // cell-level (R22): the glyphs sit above the IN PLAY rule, in the away
    // team's hero color, not merely present somewhere on the board.
    let buf = term.backend().buffer();
    let area = *buf.area();
    let in_play_y = s
        .lines()
        .position(|l| l.contains("IN PLAY"))
        .expect("IN PLAY rule") as u16;
    let th = gameday::theme::current();
    let (away_color, ..) = gameday::theme::hero_pair(&th, [200, 16, 46], [200, 16, 46]);
    let glyph_above_rule = (0..in_play_y).any(|y| {
        (0..area.width / 3).any(|x| buf[(x, y)].symbol() == "█" && buf[(x, y)].fg == away_color)
    });
    assert!(
        glyph_above_rule,
        "hero digit glyphs must render above IN PLAY, in the away team's hero color:\n{s}"
    );
    // Spec §7: the tile grammar is gone — no borders, no MOMENTUM rail, no
    // SLATE strip, no GLOBAL ALERTS sidebar.
    for dead in ['┌', '┐', '└', '┘'] {
        assert!(!s.contains(dead), "no box-drawing on the board ({dead}):\n{s}");
    }
    for dead in ["MOMENTUM", "SLATE", "GLOBAL ALERTS", "TOP PLAYS", "RECORDS"] {
        assert!(!s.contains(dead), "{dead} is deleted:\n{s}");
    }
}

#[test]
fn a_section_with_no_rows_renders_no_header() {
    // spec v3.3 §4: an empty section prints no rule at all — no orphan
    // "FINAL ───" or "LATER ───" over nothing. Cell-scan every row (not a
    // whole-buffer string search) so a header hiding off the visible window
    // would not falsely pass.

    // Live games, zero later: no LATER header anywhere in the buffer.
    let mut app = board_app(6, 2, 0);
    let term = render(&mut app, 120, 40);
    assert!(!row_contains(&term, "LATER ─"), "no later games, no LATER header");
    assert!(row_contains(&term, "FINAL ─"), "the FINAL section still renders");

    // Live games, zero finals: no FINAL header anywhere in the buffer.
    let mut app = board_app(6, 0, 2);
    let term = render(&mut app, 120, 40);
    assert!(!row_contains(&term, "FINAL ─"), "no final games, no FINAL header");
    assert!(row_contains(&term, "LATER ─"), "the LATER section still renders");
}

/// True when `needle` appears whole inside some row of the buffer — a
/// cell-level scan, not a whole-buffer string search, so a header that
/// wrapped across a row boundary (it never does; Paragraph doesn't wrap
/// mid-word) couldn't slip past.
fn row_contains(term: &Terminal<TestBackend>, needle: &str) -> bool {
    let buf = term.backend().buffer();
    let area = *buf.area();
    (0..area.height).any(|y| {
        let row: String = (0..area.width).map(|x| buf[(x, y)].symbol()).collect();
        row.contains(needle)
    })
}

#[test]
fn a_truncated_section_renders_no_orphan_rule() {
    // spec v3.3 §4, fix round 1: the empty-list guard (above) only catches a
    // section with zero games. A *non-empty* section whose row budget the
    // window cuts to zero must not draw its rule either — the reviewer's
    // exact reproductions at 60x13 and 60x16.

    // LATER orphan: 8 finals + 8 later at 60x13 — the window runs out right
    // after FINAL's rows, so LATER's rule used to draw with nothing under it.
    let mut app = board_app(0, 8, 8);
    let term = render(&mut app, 60, 13);
    let s = buf_text(&term);
    assert!(!row_contains(&term, "LATER ─"), "no orphan LATER rule at 60x13:\n{s}");
    assert!(s.contains("SCORES"), "the lane still fires for the truncated games:\n{s}");
    assert!(s.contains("8 LATER"), "the lane still counts every later game:\n{s}");

    // FINAL orphan: 6 live + 6 final + 6 later at 60x16, selection at the
    // top — IN PLAY's rows eat the window, FINAL's rule used to draw bare
    // directly above the SCORES lane.
    let mut app = board_app(6, 6, 6);
    app.selected = 0;
    let term = render(&mut app, 60, 16);
    let s = buf_text(&term);
    assert!(!row_contains(&term, "FINAL ─"), "no orphan FINAL rule at 60x16:\n{s}");
    assert!(!row_contains(&term, "LATER ─"), "no orphan LATER rule either at 60x16:\n{s}");
    assert!(s.contains("SCORES"), "the lane still fires for the truncated games:\n{s}");
    assert!(s.contains("6 FINAL"), "the lane still counts every final game:\n{s}");
    assert!(s.contains("6 LATER"), "the lane still counts every later game:\n{s}");
}

#[test]
fn a_section_granted_rows_still_shows_its_rule() {
    // Regression pin alongside the truncation-path fix: a non-empty section
    // that DOES get visible rows still gets its header — the fix must not
    // over-suppress rules that fit along with real content.
    let mut app = board_app(2, 2, 2);
    let term = render(&mut app, 120, 40);
    let s = buf_text(&term);
    assert!(row_contains(&term, "FINAL ─"), "FINAL still renders when it has room:\n{s}");
    assert!(row_contains(&term, "LATER ─"), "LATER still renders when it has room:\n{s}");
    assert!(!s.contains("SCORES"), "everything fits, no lane needed:\n{s}");
}

#[test]
fn pinned_games_sit_in_a_band_that_never_resorts() {
    // Two pinned LATER games: neither can be the hero (the hero is the top of
    // MY GAMES only when it is live), so both show as band rows in pin order.
    let games = board_games(4, 0, 2);
    let pin = |id: &str| gameday::config::Pin {
        game_id: id.into(),
        league: League::Nfl,
        final_at: None,
    };
    let band_rows = |pins: Vec<gameday::config::Pin>| -> Vec<String> {
        let mut app = mk();
        app.pins = pins;
        app.apply_boards(League::Nfl, games.clone(), false);
        let term = render(&mut app, 120, 40);
        let s = buf_text(&term);
        assert!(
            s.contains("2 PINNED · NEVER RE-SORTS"),
            "the band says what it is:\n{s}"
        );
        let lines: Vec<&str> = s.lines().collect();
        let at = lines
            .iter()
            .position(|l| l.contains("MY GAMES"))
            .expect("MY GAMES rule");
        lines[at + 1..at + 3].iter().map(|l| l.trim().to_string()).collect()
    };
    let forward = band_rows(vec![pin("p0"), pin("p1")]);
    let backward = band_rows(vec![pin("p1"), pin("p0")]);
    assert!(forward[0].contains("BUF"), "p0 first: {forward:?}");
    assert!(forward[1].contains("NYJ"), "p1 second: {forward:?}");
    assert!(backward[0].contains("NYJ"), "pin order wins: {backward:?}");
    assert!(backward[1].contains("BUF"), "pin order wins: {backward:?}");
}

#[test]
fn selection_walks_the_whole_list_and_scrolls() {
    // 24 games (12 live + 6 final + 6 later) at 120x24: the body holds the
    // 6-row hero, the IN PLAY rule and a handful of rows, so index 20 — the
    // third LATER game — cannot be in the first window. That is the point:
    // the window has to MOVE, not just highlight.
    let mut app = board_app(12, 6, 6);
    let opening = buf_text(&render(&mut app, 120, 24));
    assert!(opening.contains("IN PLAY"), "the board opens at the top:\n{opening}");
    assert!(!opening.contains("LATER"), "LATER starts off-screen:\n{opening}");

    for _ in 0..20 {
        key(&mut app, crossterm::event::KeyCode::Char('j'));
    }
    assert_eq!(app.selected, 20, "j walks the whole list — 24 games, no wrap");

    let term = render(&mut app, 120, 24);
    let buf = term.backend().buffer();
    let s = buf_text(&term);
    // The window moved: what was at the top is gone, what was off the bottom
    // is here.
    assert!(!s.contains("IN PLAY"), "the top of the list scrolled away:\n{s}");
    assert!(s.contains("LATER"), "the window followed the selection:\n{s}");

    // The selected row is on screen, cell-level: a `bright` caret in the
    // nudge gutter, on the row that carries the selected game.
    let mut caret = None;
    for y in 0..buf.area().height {
        for x in 0..buf.area().width {
            if buf[(x, y)].symbol() == "▸" && buf[(x, y)].fg == gameday::theme::current().bright {
                caret = Some((x, y));
            }
        }
    }
    let (_, y) = caret.unwrap_or_else(|| panic!("the selected row scrolled into view:\n{s}"));
    // selection = in_play(12) ++ finals(6) ++ later(6); index 20 is later[2],
    // and LATER keeps board order, so it is the 21st pair of `board_games`.
    let row = s.lines().nth(y as usize).unwrap();
    assert!(
        row.contains("CIN") && row.contains("BAL"),
        "the caret sits on the selected game: {row:?}\n{s}"
    );
}

#[test]
fn the_window_math_holds_at_odd_heights() {
    // Task-16 carry (Task 14 review): the size sweep walks even heights and
    // the dynamic window (`board::first_visible` + the lane's row) had no
    // regression test at an ODD height, where `height - lane` and the row
    // costs cannot divide evenly and an off-by-one lands on the footer.
    //
    // At every odd height the board draws at, with the selection deep enough
    // that the window must have moved: the selected row is on screen, and
    // nothing the walk drew overflows into the lane or the footer.
    for h in [15u16, 25, 27, 33, 39] {
        let mut app = board_app(12, 6, 6);
        for _ in 0..20 {
            key(&mut app, crossterm::event::KeyCode::Char('j'));
        }
        assert_eq!(app.selected, 20, "j walks the whole list at h={h}");
        let term = render(&mut app, 120, h);
        let buf = term.backend().buffer();
        let s = buf_text(&term);
        let lines: Vec<&str> = s.lines().collect();
        assert_eq!(lines.len(), h as usize, "the frame is exactly h={h} rows:\n{s}");

        // The selected row is visible: the bright caret in the nudge gutter.
        let bright = gameday::theme::current().bright;
        let caret_y = (0..buf.area().height).find(|&y| {
            (0..buf.area().width).any(|x| buf[(x, y)].symbol() == "▸" && buf[(x, y)].fg == bright)
        });
        let caret_y = caret_y
            .unwrap_or_else(|| panic!("selected row must be on screen at h={h}:\n{s}"));

        // The footer is the last row and the lane, when it exists, is the row
        // above it. Neither may be overwritten by a board row, and the caret
        // may never land on either — that is what an off-by-one in the window
        // would look like.
        let footer_y = h - 1;
        assert!(
            lines[footer_y as usize].contains("quit"),
            "footer keeps the last row at h={h}:\n{s}"
        );
        let lane_y = footer_y - 1;
        let lane = lines[lane_y as usize];
        assert!(
            lane.contains("SCORES"),
            "24 games cannot fit h={h} — the lane must be the row above the footer:\n{s}"
        );
        assert!(
            caret_y < lane_y,
            "the selected row was drawn into the lane/footer at h={h}: caret_y={caret_y}, lane_y={lane_y}\n{s}"
        );
        // The lane counts every game the walk did not draw: exactly the 24
        // games minus the ones carrying a caret-able row on screen. It is
        // never zero here, and never more than the whole board.
        let off: usize = lane
            .split_whitespace()
            .find_map(|t| t.parse::<usize>().ok())
            .unwrap_or_else(|| panic!("lane names a count at h={h}: {lane:?}"));
        assert!(
            (1..24).contains(&off),
            "lane count out of range at h={h}: off={off}, board=24\n{s}"
        );
    }
}

#[test]
fn the_ticker_is_gone_at_40_rows_and_the_lane_appears_when_truncated() {
    let mut app = board_app(4, 0, 0);
    let s = buf_text(&render(&mut app, 120, 40));
    assert!(!s.contains("SCORES"), "everything fits — no lane:\n{s}");
    assert!(!s.contains("ALERTS"), "the v3.1 ticker is gone from the board:\n{s}");

    let mut app = board_app(10, 2, 4);
    let term = render(&mut app, 80, 24);
    let s = buf_text(&term);
    let lines: Vec<&str> = s.lines().collect();
    let lane_y = (lines.len() - 2) as u16;
    let lane = lines[lane_y as usize];
    assert!(lane.contains("SCORES"), "one lane above the footer:\n{s}");
    // Cell-level (R22): the lane's label starts at column 0 of that exact
    // row, bold in the section-rule `cool` role — not merely text that
    // happens to say SCORES somewhere on the board.
    let buf = term.backend().buffer();
    let r = gameday::theme::current().roles();
    assert_eq!(buf[(0, lane_y)].fg, r.cool, "SCORES label wears the rule color:\n{s}");
    assert!(
        buf[(0, lane_y)].modifier.contains(ratatui::style::Modifier::BOLD),
        "SCORES label is bold:\n{s}"
    );
    // The lane accounts for exactly what didn't fit. Here every live game is
    // on screen and it is LATER that ran out of rows (Task 5's note), so the
    // lane degrades to the counts rather than naming a live game twice.
    let drawn = lines[..lines.len() - 2].join("\n");
    // 16 games; the 22-row body holds the hero (6 rows), the 9 other live
    // rows, both finals and one LATER row — so three LATER games are off.
    assert!(
        lane.contains("3 OFF-SCREEN") && lane.contains("3 LATER"),
        "the lane counts exactly what is not drawn:\n{s}"
    );
    assert_eq!(drawn.matches("LATER").count(), 1, "one LATER section:\n{s}");
}

#[test]
fn scores_lane_lists_off_screen_games_only() {
    // Task 9: every other view gets the SAME off-screen SCORES lane the
    // Board would show at this size — gated by the identical
    // `layout::plan(...).scores_lane` truncation check (spec §1), never a
    // second grammar, and never allocated when the Board itself wouldn't
    // truncate.
    use gameday::views::{View, ZoomTab};
    let mut app = board_app(10, 2, 4);
    app.tab = Tab::League(League::Nfl);
    app.view = View::Zoom { game_id: "l0".into(), tab: ZoomTab::Overview };
    let small_term = render(&mut app, 80, 24);
    let small = buf_text(&small_term);
    assert!(
        small.contains("SCORES"),
        "the board would truncate at 80x24 too, so Zoom gets the lane:\n{small}"
    );
    // Cell-level (R22): the lane sits on its own row — the bottom of the
    // frame's body, not the footer row itself — and its label is styled like
    // every other view's lane the same way the Board's own is.
    let lane_y = small
        .lines()
        .position(|l| l.contains("SCORES"))
        .expect("SCORES line") as u16;
    let footer_y = small.lines().count() as u16 - 1;
    assert!(lane_y < footer_y, "the lane is above the footer, not on it:\n{small}");
    let buf = small_term.backend().buffer();
    let x = small.lines().nth(lane_y as usize).unwrap().find("SCORES").unwrap() as u16;
    // This is `ticker::draw_lane` (every other view's lane), not
    // `board::mod::draw_lane` — its own grammar, `th.muted` gutter, not the
    // Board's section-rule `cool`.
    let th = gameday::theme::current();
    assert_eq!(buf[(x, lane_y)].fg, th.muted, "the lane label wears the ticker's gutter color:\n{small}");

    let mut wide_app = board_app(4, 0, 0);
    wide_app.tab = Tab::League(League::Nfl);
    wide_app.view = View::Zoom { game_id: "l0".into(), tab: ZoomTab::Overview };
    let wide = buf_text(&render(&mut wide_app, 120, 40));
    assert!(
        !wide.contains("SCORES"),
        "everything fits on the board at 120x40 — no lane here either:\n{wide}"
    );

    // The Board itself never allocates these rows a second time: exactly one
    // SCORES lane, its own inline one.
    let mut board_view = board_app(10, 2, 4);
    let board_s = buf_text(&render(&mut board_view, 80, 24));
    assert_eq!(board_s.matches("SCORES").count(), 1, "one lane, one owner:\n{board_s}");
}

// ---- Task 14: the size sweep -----------------------------------------------

/// The brief's 14-game fixture: 8 live (one hot — RED ZONE), 3 final, 3
/// later, 2 pinned. All NFL, distinct abbrs so a row is identifiable.
fn sweep_games() -> Vec<Game> {
    const PAIRS: [(&str, &str); 14] = [
        ("KC", "TB"), ("DAL", "PHI"), ("GB", "CHI"), ("SF", "SEA"),
        ("BUF", "MIA"), ("NYJ", "NE"), ("DEN", "LV"), ("ATL", "NO"),
        ("CIN", "BAL"), ("PIT", "CLE"), ("HOU", "IND"), ("MIN", "DET"),
        ("LAR", "ARI"), ("NYG", "WSH"),
    ];
    let mut out = Vec::new();
    let mut next = 0usize;
    for (tag, n, status) in [("l", 8, Status::Live), ("f", 3, Status::Final), ("p", 3, Status::Pre)] {
        for i in 0..n {
            let (away, home) = PAIRS[next % PAIRS.len()];
            next += 1;
            let mut game = g(&format!("{tag}{i}"), away, home, status == Status::Live);
            game.status = status;
            game.away_score = 10 + i as u16;
            game.home_score = 7 + i as u16;
            out.push(game);
        }
    }
    // Make the first live game unambiguously hot: red zone.
    out[0].meter = Some(gameday::domain::Meter::RedZone { yards_to_goal: 3 });
    out
}

fn sweep_app() -> App {
    let mut app = mk();
    let games = sweep_games();
    // Two pins, per the brief — the last two later games (never the hero,
    // which only ever comes from a live game — R26).
    app.pins = vec![
        gameday::config::Pin { game_id: "p1".into(), league: League::Nfl, final_at: None },
        gameday::config::Pin { game_id: "p2".into(), league: League::Nfl, final_at: None },
    ];
    app.apply_boards(League::Nfl, games, false);
    app.tab = Tab::League(League::Nfl);
    app
}

#[test]
fn the_board_survives_every_size_the_app_will_draw_at() {
    // The v3.1 clock sweep pattern, board edition (spec §4's sizes ladder,
    // `src/board/layout.rs`'s own `the_budget_never_over_allocates` sweep,
    // and `tests/draw.rs`'s `the_clock_survives_every_width_the_board_will_draw_at`).
    let widths = [40u16, 55, 60, 80, 100, 120, 180];
    let heights = [12u16, 16, 24, 30, 40, 60];
    for &w in &widths {
        for &h in &heights {
            let mut app = sweep_app();
            let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
            // No panic — the whole point of the sweep.
            t.draw(|f| app.draw(f)).unwrap();
            let s = buf_text(&t);
            let lines: Vec<&str> = s.lines().collect();

            // The key legend is the last row.
            let footer = lines.last().unwrap_or(&"");
            assert!(
                footer.contains("quit") || footer.contains("q "),
                "{w}x{h}: footer legend must be the last row: {footer:?}\n{s}"
            );

            // The selected row is visible after 10 j presses.
            for _ in 0..10 {
                key(&mut app, crossterm::event::KeyCode::Char('j'));
            }
            let mut t2 = Terminal::new(TestBackend::new(w, h)).unwrap();
            t2.draw(|f| app.draw(f)).unwrap();
            let buf2 = t2.backend().buffer();
            let has_caret = (0..buf2.area().height).any(|y| {
                (0..buf2.area().width).any(|x| {
                    let c = &buf2[(x, y)];
                    c.symbol() == "▸" && c.fg == gameday::theme::current().bright
                })
            });
            assert!(has_caret, "{w}x{h}: selected row must be visible after 10 j presses:\n{}", buf_text(&t2));

            // If h>=12 the hero (or compact hero) exists: its game's away
            // abbr appears above the IN PLAY rule (or above MY GAMES, when
            // the pinned band sits over it).
            // The Board's body is the terminal minus header+footer (1 row
            // each; the Board view never gets a ticker row), so the hero
            // bracket's own >=12-row floor (`layout::plan`) applies to that
            // body height, not the raw terminal height — a 40x12 terminal
            // legitimately has no hero (its 10-row body is under the floor)
            // and that is not a bug.
            if h.saturating_sub(2) >= 12 {
                let in_play_at = lines.iter().position(|l| l.contains("IN PLAY"));
                if let Some(at) = in_play_at {
                    let above = lines[..at].join("\n");
                    // The hero is the hottest live game — l0's pair (KC/TB,
                    // the red-zone game) — and its away abbr must be visible
                    // above the rule, not just "something" rendered there.
                    assert!(
                        above.contains("KC"),
                        "{w}x{h}: hero's away abbr (KC) must render above IN PLAY:\n{s}"
                    );
                }
            }

            // No hard clip mid-word: TestBackend already guarantees no line
            // exceeds `w`. Where content genuinely didn't fit (the SCORES
            // lane), it degrades through `text::truncate`, which always
            // closes with `…` rather than stopping mid-word — so a truncated
            // lane must end at `…`, with nothing glued on after it but
            // trailing air.
            if let Some(line) = lines.iter().find(|l| l.contains("SCORES")) {
                if let Some(cut) = line.find('…') {
                    let after: String = line.chars().skip(line[..cut].chars().count() + 1).collect();
                    assert!(
                        after.trim().is_empty(),
                        "{w}x{h}: SCORES lane has content glued after its ellipsis: {line:?}"
                    );
                }
            }
        }
    }
}

/// The abbr pair (away, home) on the row carrying the bright selection caret
/// (`▸` in `theme::current().bright`) — the hero's outside-edge mark or a
/// tier row's gutter mark, whichever is on screen. `App::derived()` is
/// crate-private, so identity here is read the same way a viewer reads it:
/// off the screen.
fn caret_pair(term: &Terminal<TestBackend>) -> Option<(String, String)> {
    let buf = term.backend().buffer();
    let area = *buf.area();
    let bright = gameday::theme::current().bright;
    let y = (0..area.height)
        .find(|&y| (0..area.width).any(|x| buf[(x, y)].symbol() == "▸" && buf[(x, y)].fg == bright))?;
    let row: String = (0..area.width).map(|x| buf[(x, y)].symbol().to_string()).collect();
    for (away, home) in SWEEP_PAIRS {
        if row.contains(away) && row.contains(home) {
            return Some((away.to_string(), home.to_string()));
        }
    }
    None
}

const SWEEP_PAIRS: [(&str, &str); 14] = [
    ("KC", "TB"), ("DAL", "PHI"), ("GB", "CHI"), ("SF", "SEA"),
    ("BUF", "MIA"), ("NYJ", "NE"), ("DEN", "LV"), ("ATL", "NO"),
    ("CIN", "BAL"), ("PIT", "CLE"), ("HOU", "IND"), ("MIN", "DET"),
    ("LAR", "ARI"), ("NYG", "WSH"),
];

#[test]
fn resize_relayouts_from_the_same_list() {
    let mut app = sweep_app();
    for _ in 0..3 {
        key(&mut app, crossterm::event::KeyCode::Char('j'));
    }
    let mut wide = Terminal::new(TestBackend::new(120, 40)).unwrap();
    wide.draw(|f| app.draw(f)).unwrap();
    let wide_pair = caret_pair(&wide).expect("a selected row is on screen at 120x40");

    // Same App, drawn at a smaller size: selection is preserved by id (the
    // selection index doesn't move, and the board is rebuilt fresh from the
    // same underlying list on every draw — no stale scroll offset carried
    // across the resize).
    let mut narrow = Terminal::new(TestBackend::new(80, 24)).unwrap();
    narrow.draw(|f| app.draw(f)).unwrap();
    let narrow_pair = caret_pair(&narrow).expect("a selected row is on screen at 80x24");
    assert_eq!(wide_pair, narrow_pair, "selection survives a resize, by identity");

    // The hero may demote (drop its digit form, or lose the flanks) but the
    // same game keeps being the hero: its abbr pair still appears above the
    // IN PLAY rule (or the MY GAMES rule, when the pinned band sits over it)
    // at both sizes.
    let hero_line = |term: &Terminal<TestBackend>| -> String {
        let s = buf_text(term);
        let lines: Vec<&str> = s.lines().collect();
        let rule = lines
            .iter()
            .position(|l| l.contains("IN PLAY") || l.contains("MY GAMES"))
            .unwrap_or(lines.len());
        lines[..rule].join("\n")
    };
    let wide_hero = hero_line(&wide);
    let narrow_hero = hero_line(&narrow);
    for (away, home) in SWEEP_PAIRS {
        let in_wide = wide_hero.contains(away) && wide_hero.contains(home);
        let in_narrow = narrow_hero.contains(away) && narrow_hero.contains(home);
        assert_eq!(
            in_wide, in_narrow,
            "hero identity ({away}/{home}) must agree across the resize\nwide:\n{wide_hero}\nnarrow:\n{narrow_hero}"
        );
    }
}

#[test]
fn paging_keys_are_dead_and_not_advertised() {
    let mut app = board_app(10, 2, 2);
    let before = app.selected;
    key(&mut app, crossterm::event::KeyCode::Char('n'));
    key(&mut app, crossterm::event::KeyCode::PageDown);
    assert_eq!(app.selected, before, "n/PgDn are dead keys on the board");
    let s = buf_text(&render(&mut app, 120, 40));
    assert!(!s.contains("PAGE"), "no PAGE in the footer:\n{s}");
    assert!(
        !gameday::keymap::KEYMAP.iter().any(|b| b.label == "PAGE"),
        "the PAGE binding is deleted"
    );
}

// ---------------------------------------------------------------- the cut

/// The scoring play a cut fires on, and a two-team game that isn't a
/// same-color pair (so the digits really are two different colors).
fn cut_game(id: &str) -> Game {
    let mut game = g(id, "KC", "BUF", true);
    game.away.name = "Chiefs".into();
    game.home.name = "Bills".into();
    game.home.color = [0, 51, 141];
    game.away_score = 24;
    game.home_score = 21;
    game
}

fn scoring_play() -> Play {
    Play {
        clock: "1:52".into(),
        period: "Q4".into(),
        team: "KC".into(),
        text: "Mahomes 12 Yd pass to Kelce".into(),
        scoring: true,
    }
}

/// Seed the board (first sighting never fires), then land a score delta whose
/// `lastPlay` is the scoring play — exactly the path `apply_boards` captures.
fn land_a_score(app: &mut App, pinned: bool) {
    let mut before = cut_game("1");
    before.away_score = 17;
    before.last_plays = vec![Play {
        text: "Mahomes pass short right to Kelce for 6 yards".into(),
        ..Default::default()
    }];
    app.apply_boards(League::Nfl, vec![before, g("2", "DAL", "PHI", true)], false);
    if pinned {
        app.pins.push(gameday::config::Pin {
            game_id: "1".into(),
            league: League::Nfl,
            final_at: None,
        });
    }
    let mut after = cut_game("1");
    after.last_plays = vec![scoring_play()];
    app.apply_boards(League::Nfl, vec![after, g("2", "DAL", "PHI", true)], false);
}

#[test]
fn the_takeover_and_the_hero_agree_on_every_digit_cell() {
    // Spec §1's hard rule: there is ONE score formatter. The takeover asks
    // `hero::score_block` for its digits, so the same game rendered both ways
    // must be identical cell for cell inside the score's rect — a takeover
    // that re-implemented the glyphs would drift here immediately.
    use ratatui::layout::Rect;
    let game = cut_game("1");
    let play = scoring_play();
    let area = Rect::new(0, 1, 120, 39); // everything under the header row
    let (slot, full) = gameday::board::cut::score_slot(area, &game, &play);
    assert!(slot.width > 0 && slot.height > 0, "the takeover must reserve a score band");

    let mut cut = Terminal::new(TestBackend::new(120, 40)).unwrap();
    let fired = gameday::board::cut::Cut {
        game_id: game.id.clone(),
        play: play.clone(),
        full: true,
        until_tick: gameday::board::cut::CUT_TICKS,
    };
    cut.draw(|f| gameday::board::cut::draw_takeover(f, area, &game, &fired, 0))
        .unwrap();
    let mut hero = Terminal::new(TestBackend::new(120, 40)).unwrap();
    hero.draw(|f| gameday::board::hero::score_block(f, slot, &game, full))
        .unwrap();

    let (a, b) = (cut.backend().buffer(), hero.backend().buffer());
    let mut painted = 0;
    for y in slot.y..slot.bottom() {
        for x in slot.x..slot.right() {
            assert_eq!(
                a[(x, y)].symbol(),
                b[(x, y)].symbol(),
                "digit cell ({x},{y}) differs between the cut and the hero"
            );
            // The takeover paints its own ground across the whole area, so
            // an empty cell's fg differs by construction; the digits are the
            // claim, and every painted cell must match in color too.
            if a[(x, y)].symbol() != " " {
                assert_eq!(
                    a[(x, y)].fg,
                    b[(x, y)].fg,
                    "digit color at ({x},{y}) differs between the cut and the hero"
                );
                painted += 1;
            }
        }
    }
    assert!(painted >= 32, "the score has to actually be drawn: {painted} cells");
}

#[test]
fn a_pinned_score_takes_the_screen_and_an_unpinned_one_is_a_band() {
    let r = gameday::theme::current().roles();

    // Pinned: the screen becomes the score. The header row survives; the
    // board underneath does not.
    let mut app = mk();
    app.tick = 400; // past the 30 s startup suppression
    land_a_score(&mut app, true);
    let mut t = Terminal::new(TestBackend::new(120, 40)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t);
    let rows: Vec<&str> = s.lines().collect();
    assert!(rows[0].contains("GAMEDAY"), "the header row survives the cut:\n{s}");
    assert!(!s.contains("IN PLAY"), "the board is not drawn behind the takeover:\n{s}");
    // The word is block letters, so it is cells in the hot role, not text.
    let b = t.backend().buffer();
    let hot = (1..40u16)
        .map(|y| (0..120u16).filter(|&x| b[(x, y)].fg == r.hot && b[(x, y)].symbol() != " ").count())
        .sum::<usize>();
    assert!(hot >= 40, "TOUCHDOWN must be painted in block letters: {hot} hot cells\n{s}"); // spec v3.3 §7
    assert!(s.contains("CHIEFS AT BILLS"), "the dim strip names the game:\n{s}");
    assert!(s.contains("MAHOMES"), "the detail line comes from the play:\n{s}");
    // Spec §3's chip, verbatim, on the takeover's one filled element.
    assert!(rows[1].contains("▲ SCORING PLAY · KC"), "the takeover chip is the spec's:\n{s}");
    let chip_x = rows[1].chars().position(|c| c == '▲').unwrap() as u16;
    assert_eq!(
        t.backend().buffer()[(chip_x, 1)].bg,
        r.hot,
        "the chip is filled hot, like the hero's state chip"
    );

    // Unpinned: two quiet rows above the list, and the board stays.
    let mut app = mk();
    app.tick = 400;
    land_a_score(&mut app, false);
    let mut t = Terminal::new(TestBackend::new(120, 40)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t);
    let rows: Vec<&str> = s.lines().collect();
    // Spec §3 / ruling R33: `▲ HOME RUN · TEX Seager (32) · ATH 0 TEX 5` on a
    // hot ground, two rows, above an intact list.
    assert!(rows[1].starts_with("▲ TOUCHDOWN · KC MAHOMES · KC 24 BUF 21"), "band headline:\n{s}"); // spec v3.3 §7
    // spec v3.3 §3: row two stopped repeating the play and became the
    // affordance — what enter does, and how long the band has left.
    assert!(
        rows[2].starts_with("enter jump · clears in"),
        "the band's second row is the jump affordance:\n{s}"
    );
    assert!(!rows[2].contains("KELCE"), "the play is not said twice:\n{s}");
    assert!(s.contains("IN PLAY"), "the board is still there under the band:\n{s}");
    // Cell level: the mark, and the hot fill across both rows including the
    // empty tail — the band is a bar of alert color, not a bare line.
    let b = t.backend().buffer();
    let r = gameday::theme::current().roles();
    assert_eq!(b[(0, 1)].symbol(), "▲", "the mark is the band's first cell");
    assert_eq!(b[(0, 1)].fg, r.ground, "band ink is the ground role on hot");
    for y in 1..=2u16 {
        for x in 0..120u16 {
            assert_eq!(b[(x, y)].bg, r.hot, "the whole band row {y} is hot at ({x},{y})");
        }
    }
    assert_ne!(b[(0, 3)].bg, r.hot, "the fill stops at the band: row 3 is the board");
}

#[test]
fn enter_during_a_band_zooms_the_bands_game_not_the_selection() {
    // spec v3.3 §3: while a band is up, enter is the jump to the game that
    // just scored — the one interaction the band adds.
    use gameday::input::{handle_key, InputMode};
    use gameday::views::View;
    use ratatui::crossterm::event::{KeyCode, KeyModifiers};

    let zoomed = |app: &App| match &app.view {
        View::Zoom { game_id, .. } => Some(game_id.clone()),
        _ => None,
    };

    // No band: enter zooms the selection, exactly as before.
    let mut app = mk();
    app.tick = 400;
    app.apply_boards(League::Nfl, vec![cut_game("1"), g("2", "DAL", "PHI", true)], false);
    app.selected = 0;
    handle_key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(zoomed(&app), Some("1".into()), "with no band, enter still zooms the selection");

    // A band for game 2 while game 1 is selected: enter follows the band.
    let mut app = mk();
    app.tick = 400;
    let mut before = g("2", "DAL", "PHI", true);
    before.away_score = 3;
    app.apply_boards(League::Nfl, vec![cut_game("1"), before.clone()], false);
    app.selected = 0;
    let mut after = before.clone();
    after.away_score = 10;
    after.last_plays = vec![scoring_play()];
    app.apply_boards(League::Nfl, vec![cut_game("1"), after], false);
    let cut = app.cuts.active(app.tick).expect("the unpinned score fires a band");
    assert!(!cut.full && cut.game_id == "2");
    handle_key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(zoomed(&app), Some("2".into()), "enter jumped to the band's game, not the selection");

    // A prompt is open: enter belongs to the prompt, and nothing jumps.
    let mut app = mk();
    app.tick = 400;
    land_a_score(&mut app, false);
    assert!(app.cuts.active(app.tick).is_some(), "the band is up");
    app.mode = InputMode::Filter { buf: "kc".into() };
    handle_key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(zoomed(&app), None, "a prompt's enter never jumps");
}

#[test]
fn cuts_are_suppressed_during_prompts_and_startup() {
    // A prompt is open: the cut would eat the keystroke the user is typing.
    let mut app = mk();
    app.tick = 400;
    app.mode = gameday::input::InputMode::Filter { buf: "kc".into() };
    land_a_score(&mut app, true);
    assert!(app.cuts.active(app.tick).is_none(), "no cut while a prompt is open");

    // The config view's favorite-abbr editor: a text prompt in everything but
    // the enum. A takeover here blanks the editor while keystrokes keep
    // landing in the buffer the user can no longer see.
    let mut app = mk();
    app.tick = 400;
    app.config_edit = Some("K".into());
    land_a_score(&mut app, true);
    assert!(app.cuts.active(app.tick).is_none(), "no cut while the abbr editor is open");

    // The theme picker is modal and IS a live preview.
    let mut app = mk();
    app.tick = 400;
    app.view = gameday::views::View::ThemePicker;
    land_a_score(&mut app, true);
    assert!(app.cuts.active(app.tick).is_none(), "no cut over the theme picker");

    // Startup: the first boards arrive carrying a whole day of scores.
    let mut app = mk();
    app.tick = 12;
    land_a_score(&mut app, true);
    assert!(app.cuts.active(app.tick).is_none(), "no cut in the first 30 s");

    // Same delta once the session is warm and no prompt is open: it fires.
    let mut app = mk();
    app.tick = 400;
    land_a_score(&mut app, true);
    let cut = app.cuts.active(app.tick).expect("a warm, unblocked delta fires");
    assert!(cut.full, "a pinned game takes the screen");
    assert!(app.bell_pending, "a takeover rings the bell");
}

// ------------------------------------------------------------------- :tv

/// Six live games with distinct abbrs — TV shows one and strips the rest.
fn tv_slate() -> Vec<Game> {
    [
        ("KC", "TB"),
        ("DAL", "PHI"),
        ("GB", "CHI"),
        ("SF", "LAR"),
        ("NYJ", "MIA"),
        ("CIN", "BAL"),
    ]
    .into_iter()
    .enumerate()
    .map(|(i, (away, home))| {
        // 24-21, the reference frame's score: both digit pairs paint all
        // eight rows of a `PixelSize::Full` glyph, which is what the band
        // height is measured off below.
        let mut game = g(&format!("{}", i + 1), away, home, true);
        game.away_score = 24;
        game.home_score = 21;
        game
    })
    .collect()
}

#[test]
fn tv_fills_the_screen_with_the_hero_and_strips_the_rest() {
    use crossterm::event::{KeyCode, KeyModifiers};
    let mut app = mk();
    app.apply_boards(League::Nfl, tv_slate(), false);
    app.tab = Tab::League(League::Nfl);
    app.on_key(KeyCode::Char('v'), KeyModifiers::NONE);
    let mut term = Terminal::new(TestBackend::new(120, 40)).unwrap();
    term.draw(|f| app.draw(f)).unwrap();
    let text = buf_text(&term);

    // The jumbotron: `PixelSize::Full` digits are 8×8 cells, so the away
    // score paints eight contiguous rows of the left third in KC's hero
    // color. A sextant fallback would paint three, the text form one.
    let th = gameday::theme::current();
    let (away_color, ..) = gameday::theme::hero_pair(&th, team("KC").color, team("TB").color);
    let buf = term.backend().buffer();
    let digit_rows: Vec<u16> = (0..40u16)
        .filter(|y| (0..40u16).filter(|x| buf[(*x, *y)].fg == away_color).count() >= 8)
        .collect();
    assert_eq!(digit_rows.len(), 8, "TV draws 8-row Full digits:\n{text}");
    assert_eq!(
        digit_rows[7] - digit_rows[0],
        7,
        "the digit band is contiguous: {digit_rows:?}\n{text}"
    );
    // The shown game is the ranking's top; both its abbrs are on the hero.
    assert!(text.contains("KC") && text.contains("TB"), "{text}");

    // Spec §0: TV stays logo-free, even at the ≥100 columns where the board
    // flanks its hero — the margin outside the digits is untouched ground.
    // (KC and TB both have committed art, so this would paint otherwise.)
    for y in digit_rows[0]..=digit_rows[7] {
        for x in 0..20u16 {
            assert_eq!(
                buf[(x, y)].symbol(),
                " ",
                "no hero mark in TV's margin at ({x},{y}):\n{text}"
            );
        }
    }

    // Everything else rides the strip, one row each.
    assert!(text.contains("ALSO LIVE"), "the strip names itself:\n{text}");
    assert!(text.contains("5 GAMES"), "the strip counts the rest:\n{text}");
    for abbr in ["DAL", "PHI", "GB", "CHI", "SF", "LAR", "NYJ", "MIA", "CIN", "BAL"] {
        assert!(text.contains(abbr), "strip is missing {abbr}:\n{text}");
    }
    // TV is not the board: no section rules, no off-screen lane.
    assert!(!text.contains("IN PLAY"), "no board rules in TV:\n{text}");
    assert!(!text.contains("OFF-SCREEN"), "no lane in TV:\n{text}");
}

#[test]
fn a_locked_game_going_final_never_leaves_tv_saying_nothing_is_live() {
    // Ruling R36: one slate. The lock and the shown id are both validated
    // against the LIVE slate the strip draws from — a game that has gone
    // final can't stay "shown" while five games are live.
    use crossterm::event::{KeyCode, KeyModifiers};
    let mut app = mk();
    app.apply_boards(League::Nfl, tv_slate(), false);
    app.tab = Tab::League(League::Nfl);
    app.on_key(KeyCode::Char('v'), KeyModifiers::NONE);
    app.on_key(KeyCode::Char(' '), KeyModifiers::NONE);
    assert_eq!(app.tv_lock.as_deref(), Some("1"), "space locks the shown game");

    let mut games = tv_slate();
    games[0].status = Status::Final;
    app.apply_boards(League::Nfl, games, false);
    assert_eq!(app.tv_lock, None, "the lock released with its game");

    let mut term = Terminal::new(TestBackend::new(120, 40)).unwrap();
    term.draw(|f| app.draw(f)).unwrap();
    let text = buf_text(&term);
    assert!(!text.contains("nothing is live"), "five games are live:\n{text}");
    assert!(text.contains("DAL") && text.contains("PHI"), "the next live game leads:\n{text}");
    assert!(text.contains("4 GAMES"), "the strip counts the remaining live games:\n{text}");
}

#[test]
fn tv_never_panics_and_never_blanks_the_score() {
    // The v3.1 clamping discipline: every slot in TV is derived from the
    // area, so no size may panic (a debug build catches the underflows) —
    // and none may leave the jumbotron without a score.
    use crossterm::event::{KeyCode, KeyModifiers};
    let mut app = mk();
    app.apply_boards(League::Nfl, tv_slate(), false);
    app.tab = Tab::League(League::Nfl);
    app.on_key(KeyCode::Char('v'), KeyModifiers::NONE);
    for (w, h) in [(40, 12), (41, 13), (60, 20), (80, 24), (100, 30), (120, 40), (200, 60)] {
        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        term.draw(|f| app.draw(f)).unwrap();
        let text = buf_text(&term);
        assert!(
            text.contains("24 - 21") || text.contains("████") || text.contains('🬂'),
            "{w}x{h} must still show a score in some form:\n{text}"
        );
    }
}

#[test]
fn in_tv_only_the_shown_game_and_my_teams_take_the_screen() {
    // spec v3.3 §9 decision B: TV mode no longer promotes every scoring play
    // to a takeover. Only the game currently shown and MY GAMES (pinned or
    // favorited) teams earn the whole screen; everything else is the quiet
    // band, drawn over TV the same way it's drawn over the board.
    let mut app = mk();
    app.tick = 400;

    let mut cur1 = cut_game("1"); // KC vs BUF — TV's shown game
    cur1.away_score = 17;
    let mut cur2 = g("2", "DAL", "PHI", true); // pinned, not shown
    let mut cur3 = g("3", "SF", "NYG", true); // unrelated, not shown

    app.apply_boards(
        League::Nfl,
        vec![cur1.clone(), cur2.clone(), cur3.clone()],
        false,
    );
    app.pins.push(gameday::config::Pin {
        game_id: "2".into(),
        league: League::Nfl,
        final_at: None,
    });
    app.view = gameday::views::View::Tv;
    app.tv_shown = Some("1".into());

    // Delta on the shown game: takeover.
    cur1 = cut_game("1");
    cur1.last_plays = vec![scoring_play()];
    app.apply_boards(
        League::Nfl,
        vec![cur1.clone(), cur2.clone(), cur3.clone()],
        false,
    );
    let cut = app.cuts.active(app.tick).expect("a delta fires a cut");
    assert!(cut.full, "the shown game takes the whole screen");
    assert_eq!(cut.game_id, "1");

    app.tick += 31; // clear the takeover (CUT_TICKS is 30) before the next fire

    // Delta on the pinned game, not shown: still a takeover — MY GAMES.
    cur2.away_score += 3;
    cur2.last_plays = vec![Play {
        team: "DAL".into(),
        text: "Prescott 9 Yd pass — TOUCHDOWN".into(),
        scoring: true,
        ..Default::default()
    }];
    app.apply_boards(
        League::Nfl,
        vec![cur1.clone(), cur2.clone(), cur3.clone()],
        false,
    );
    let cut = app.cuts.active(app.tick).expect("a delta fires a cut");
    assert!(cut.full, "a pinned/favorited game takes the whole screen even unshown");
    assert_eq!(cut.game_id, "2");

    app.tick += 31;

    // Delta on an unrelated game: the quiet band, not a takeover.
    cur3.away_score += 3;
    cur3.last_plays = vec![Play {
        team: "SF".into(),
        text: "Purdy 5 Yd pass — TOUCHDOWN".into(),
        scoring: true,
        ..Default::default()
    }];
    app.apply_boards(League::Nfl, vec![cur1, cur2, cur3], false);
    let cut = app.cuts.active(app.tick).expect("a delta fires a cut").clone();
    assert!(!cut.full, "an unrelated game's cut is the quiet band, not a takeover");
    assert_eq!(cut.game_id, "3");

    // The band draws over TV's top rows; TV's own body still renders
    // beneath it — the shown game's nameplate (a TV-only cell) survives.
    let term = render(&mut app, 120, 40);
    let s = buf_text(&term);
    assert!(
        s.contains("KC") && s.contains("BUF"),
        "TV's shown game survives under the band:\n{s}"
    );
}

#[test]
fn a_locked_shown_game_keeps_the_takeover_even_when_a_better_game_scores() {
    // spec v3.3 §9 decision B named edge: TV can be locked (space) onto a
    // game that is NOT the ranking's top. `tv_follow` respects the lock and
    // never re-anchors `tv_shown` while it holds, so the locked/shown game
    // stays the one takeover-eligible id — a score from the ranking's
    // actual top (unshown, not MY GAMES) is still just the quiet band.
    let mut app = mk();
    app.tick = 400;

    let mut cur1 = cut_game("1"); // shown & locked
    cur1.away_score = 17;
    // A hotter, closer game than "1" — the one the ranking would otherwise
    // promote TV to on the next event, if TV weren't locked off it.
    let mut cur2 = g("2", "SF", "NYG", true);

    app.apply_boards(League::Nfl, vec![cur1.clone(), cur2.clone()], false);
    app.view = gameday::views::View::Tv;
    app.tv_shown = Some("1".into());
    app.tv_lock = Some("1".into());

    // Delta on the locked/shown game: takeover.
    cur1 = cut_game("1");
    cur1.last_plays = vec![scoring_play()];
    app.apply_boards(League::Nfl, vec![cur1.clone(), cur2.clone()], false);
    let cut = app.cuts.active(app.tick).expect("a delta fires a cut");
    assert!(cut.full, "the locked/shown game takes the whole screen");
    assert_eq!(cut.game_id, "1");
    assert_eq!(app.tv_shown.as_deref(), Some("1"), "the lock held tv_shown on 1");

    app.tick += 31; // clear the takeover before the next fire

    // Delta on the OTHER game (the would-be ranking top, still unshown
    // thanks to the lock, not pinned or favorited): the quiet band.
    cur2.away_score += 3;
    cur2.last_plays = vec![Play {
        team: "SF".into(),
        text: "Purdy 5 Yd pass — TOUCHDOWN".into(),
        scoring: true,
        ..Default::default()
    }];
    app.apply_boards(League::Nfl, vec![cur1, cur2], false);
    let cut = app.cuts.active(app.tick).expect("a delta fires a cut").clone();
    assert!(
        !cut.full,
        "an unshown game's cut is the quiet band even when it outranks the locked game"
    );
    assert_eq!(cut.game_id, "2");
}

// ---------------------------------------------------------------- zoom (§5)
// The Overview tab is hero + linescore + a per-sport matchup line, which is
// where sub-project 1's mapped-but-never-drawn fields (situation.pitcher /
// batter / due_up, Game.timeouts, Extras::Soccer.events) finally render.

fn zoom_team(league: &str, abbr: &str, color: [u8; 3]) -> Team {
    Team {
        id: abbr.into(),
        abbr: abbr.into(),
        name: abbr.into(),
        color,
        alt_color: [255, 255, 255],
        // No committed art under these keys, so the hero draws no flanks and
        // the only `█` cells in the frame are the score glyphs.
        logo_key: format!("{league}/{}", abbr.to_lowercase()),
        ..Default::default()
    }
}

fn mlb_zoom_game() -> Game {
    Game {
        id: "m1".into(),
        league: League::Mlb,
        away: zoom_team("mlb", "SEA", [0, 92, 92]),
        home: zoom_team("mlb", "BOS", [189, 48, 57]),
        away_score: 4,
        home_score: 3,
        status: Status::Live,
        period: "B7".into(),
        clock: String::new(),
        situation: Some(Situation {
            down_distance: "2 OUT · 1-2".into(),
            balls: Some(1),
            strikes: Some(2),
            outs: Some(2),
            on_base: Some([true, false, true]),
            pitcher: Some("G. Kirby".into()),
            batter: Some("R. Devers".into()),
            due_up: vec![
                "A. Riley (2-3, HR)".into(),
                "J. Duran (1-4)".into(),
                "T. Story (0-3)".into(),
            ],
            ..Default::default()
        }),
        meter: Some(Meter::Diamond { occupied: [true, false, true] }),
        last_plays: vec![Play {
            period: "B7".into(),
            team: "BOS".into(),
            text: "Devers singles to right field".into(),
            ..Default::default()
        }],
        linescore: vec![(0, 1), (2, 0), (0, 0), (1, 1), (0, 0), (1, 0), (0, 1)],
        extras: Extras::Baseball { hits: Some((8, 7)), errors: Some((0, 1)) },
        ..Game::default()
    }
}

/// The score glyphs of a rendered frame, cropped to their bounding box:
/// every `█` cell with its fg. Position-independent, so the same score drawn
/// at two different y offsets compares equal.
fn digit_grid(term: &Terminal<TestBackend>) -> Vec<Vec<(String, ratatui::style::Color)>> {
    let b = term.backend().buffer();
    let area = b.area();
    let mut cells = Vec::new();
    for y in 0..area.height {
        for x in 0..area.width {
            if b[(x, y)].symbol() == "█" {
                cells.push((x, y));
            }
        }
    }
    assert!(!cells.is_empty(), "no score glyphs were drawn at all");
    let (x0, x1) = (
        cells.iter().map(|c| c.0).min().unwrap(),
        cells.iter().map(|c| c.0).max().unwrap(),
    );
    let (y0, y1) = (
        cells.iter().map(|c| c.1).min().unwrap(),
        cells.iter().map(|c| c.1).max().unwrap(),
    );
    (y0..=y1)
        .map(|y| {
            (x0..=x1)
                .map(|x| (b[(x, y)].symbol().to_string(), b[(x, y)].fg))
                .collect()
        })
        .collect()
}

fn zoomed(app: &mut App, game: &Game) {
    use gameday::views::{View, ZoomTab};
    app.view = View::Zoom { game_id: game.id.clone(), tab: ZoomTab::Overview };
}

#[test]
fn zoom_overview_reuses_the_hero_and_shows_the_matchup_line() {
    let game = mlb_zoom_game();
    let mut app = mk();
    app.apply_boards(League::Mlb, vec![game.clone()], false);
    app.tab = Tab::League(League::Mlb);

    // The board's hero for this game.
    let mut board = Terminal::new(TestBackend::new(120, 40)).unwrap();
    board.draw(|f| app.draw(f)).unwrap();
    let board_digits = digit_grid(&board);

    zoomed(&mut app, &game);
    let mut zoom = Terminal::new(TestBackend::new(120, 40)).unwrap();
    zoom.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&zoom);

    // Spec §5: the matchup line, from fields the mapper has filled since v3.1.
    assert!(s.contains("P: G. Kirby"), "pitcher missing:\n{s}");
    assert!(s.contains("AB: R. Devers"), "batter missing:\n{s}");
    assert!(s.contains("DUE UP"), "due up missing:\n{s}");
    assert!(s.contains("A. Riley"), "the first due-up hitter missing:\n{s}");
    // …and the linescore table.
    assert!(
        s.lines().any(|l| l.contains("SEA") && l.trim_end().ends_with(" 0")),
        "away linescore row (R H E ending in E=0) missing:\n{s}"
    );
    assert!(s.contains("LAST PLAYS"), "the feed survived the rebuild:\n{s}");

    // Spec §1's hard rule: one score formatter. The zoom hero IS the board
    // hero, so the digits match cell for cell — chars and colors.
    assert_eq!(
        digit_grid(&zoom),
        board_digits,
        "the zoom hero's digits differ from the board hero's for the same game"
    );
}

#[test]
fn football_zoom_shows_timeouts_and_possession() {
    let mut game = g("1", "KC", "TB", true);
    game.timeouts = Some((2, 3));
    let mut app = mk();
    app.apply_boards(League::Nfl, vec![game.clone()], false);
    app.tab = Tab::League(League::Nfl);
    zoomed(&mut app, &game);
    let mut t = Terminal::new(TestBackend::new(120, 40)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t);
    assert!(s.contains("TIMEOUTS"), "timeouts label missing:\n{s}");
    assert!(
        s.contains("●●○ │ ●●●"),
        "2-of-3 away / 3-of-3 home pips missing:\n{s}"
    );
    // The hero's fragment line owns `KC BALL` on football; the matchup line
    // must not print a second copy of it three rows down.
    assert_eq!(s.matches("KC BALL").count(), 1, "possession said twice:\n{s}");
}

#[test]
fn soccer_zoom_lists_match_events_with_minute_and_letter() {
    let mut game = g("s1", "NFO", "NEW", true);
    game.league = League::Epl;
    game.away = zoom_team("epl", "NFO", [221, 0, 0]);
    game.home = zoom_team("epl", "NEW", [45, 41, 38]);
    game.away_score = 1;
    game.home_score = 2;
    game.period = "70'".into();
    game.clock = String::new();
    game.situation = None;
    game.meter = None;
    game.extras = Extras::Soccer {
        events: vec![
            MatchEvent { minute: "12'".into(), kind: EventKind::Yellow, team: "NEW".into(), player: "B. Burn".into() },
            MatchEvent { minute: "24'".into(), kind: EventKind::Goal, team: "NEW".into(), player: "D. Ndoye".into() },
            MatchEvent { minute: "61'".into(), kind: EventKind::Yellow, team: "NFO".into(), player: "O. Aina".into() },
            MatchEvent { minute: "70'".into(), kind: EventKind::Penalty, team: "NFO".into(), player: "M. Gibbs-White".into() },
        ],
    };
    let mut app = mk();
    app.apply_boards(League::Epl, vec![game.clone()], false);
    app.tab = Tab::League(League::Epl);
    zoomed(&mut app, &game);
    let mut t = Terminal::new(TestBackend::new(120, 40)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t);
    // The last three events, newest last, letters not emoji (terminal-legal).
    assert!(s.contains("24' G D. Ndoye"), "goal event missing:\n{s}");
    assert!(s.contains("61' Y O. Aina"), "card event missing:\n{s}");
    assert!(s.contains("70' PEN M. Gibbs-White"), "penalty event missing:\n{s}");
    assert!(!s.contains("12' Y B. Burn"), "only the last three events:\n{s}");
    for emoji in ['⚽', '🟨', '🟥'] {
        assert!(!s.contains(emoji), "emoji {emoji} in a terminal frame:\n{s}");
    }
}

// ── spec v3.3 §5: screen layouts — no screen floats a narrow column in a
// half-empty frame ─────────────────────────────────────────────────────────

/// Live WNBA game with one scoring play — the six-column league chip
/// (`[WNBA]`), which is what pushes the feed's stamp column out of line when
/// the chip isn't padded to a fixed width.
fn wnba_game(id: &str, away: &str, home: &str) -> Game {
    let mut game = nba_game(id, away, home);
    game.id = id.into();
    game.league = League::Wnba;
    game.last_plays = vec![Play {
        clock: "2:08".into(),
        team: away.into(),
        text: "Wilson turnaround jumper".into(),
        scoring: true,
        ..Default::default()
    }];
    with_scoring(game)
}

#[test]
fn standings_use_two_columns_at_width() {
    // spec v3.3 §5: at 120 cols the two conference tables sit side by side
    // (both group headers on one row, ≥40 cols apart; receipt: two 48-col
    // tables + a 4-col gutter = 100 is the gate). At 80 they stack.
    use gameday::views::View;
    let mut app = mk();
    app.view = View::Standings(League::Nfl);
    app.merge_standings(standings_table());
    let mut wide = Terminal::new(TestBackend::new(120, 40)).unwrap();
    wide.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&wide);
    let row = s
        .lines()
        .find(|l| {
            l.contains("AMERICAN FOOTBALL CONFERENCE") && l.contains("NATIONAL FOOTBALL CONFERENCE")
        })
        .unwrap_or_else(|| panic!("120 cols must put both conferences on one row:\n{s}"));
    let afc = row.find("AMERICAN").unwrap();
    let nfc = row.find("NATIONAL").unwrap();
    assert!(nfc - afc >= 40, "columns must be a real split, {afc} vs {nfc}: {row:?}");
    // Both tables keep their own rows under their own header.
    assert!(s.contains("CHIEFS") && s.contains("EAGLES"), "{s}");

    let mut narrow = Terminal::new(TestBackend::new(80, 24)).unwrap();
    narrow.draw(|f| app.draw(f)).unwrap();
    let n = buf_text(&narrow);
    assert!(
        !n.lines().any(|l| {
            l.contains("AMERICAN FOOTBALL CONFERENCE") && l.contains("NATIONAL FOOTBALL CONFERENCE")
        }),
        "below 100 cols the table is one column:\n{n}"
    );
    assert!(n.contains("NATIONAL FOOTBALL CONFERENCE"), "stacked, still both groups:\n{n}");
}

#[test]
fn the_plays_feed_fills_the_width() {
    // spec v3.3 §5: the feed is a full-width row, not a 60-col column in a
    // 120-col frame — the matchup score rides the right edge — and the stamp
    // column lands at one x for every league, four-letter chips included.
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
    app.config.enabled_tabs = vec![League::Nfl, League::Wnba];
    app.apply_boards(League::Nfl, vec![with_scoring(nfl)], false);
    app.apply_boards(League::Wnba, vec![wnba_game("2", "LV", "SEA")], false);
    app.view = View::PlaysFeed;
    let mut t = Terminal::new(TestBackend::new(120, 40)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t);
    let rows: Vec<&str> = s
        .lines()
        .filter(|l| l.contains("Mahomes to Kelce") || l.contains("Wilson turnaround"))
        .collect();
    assert_eq!(rows.len(), 2, "both leagues' plays are rows:\n{s}");
    // Everything left of the play text is fixed width, so the scoring word —
    // the last of those columns — starts at one x on every row.
    let word_col = |row: &str| {
        let at = row
            .find("TOUCHDOWN")
            .or_else(|| row.find("BUCKET"))
            .unwrap_or_else(|| panic!("no scoring word: {row:?}"));
        // Columns, not bytes — the ▸ marker is three bytes wide.
        row[..at].chars().count()
    };
    for row in &rows {
        let end = row.trim_end().chars().count();
        assert!(end > 80, "row stops short of the frame at {end}: {row:?}");
        assert_eq!(word_col(row), word_col(rows[0]), "stamp column drifts: {row:?}");
    }
}

#[test]
fn the_re_laid_out_screens_survive_every_size() {
    // spec v3.3 §5 does width arithmetic on three more screens (two-column
    // standings, the full-width feed, the two-panel editor), so they take the
    // board's own sweep: no panic anywhere on the ladder, and each still says
    // what it is.
    use gameday::views::View;
    for (name, view, needle) in [
        ("standings", View::Standings(League::Nfl), "STANDINGS"),
        ("plays feed", View::PlaysFeed, "PLAYS"),
        ("config", View::ConfigView, "CONFIG"),
    ] {
        for w in [40u16, 55, 60, 80, 99, 100, 120, 180] {
            for h in [12u16, 16, 24, 30, 40, 60] {
                let mut app = mk();
                app.config.enabled_tabs = vec![League::Nfl];
                app.apply_boards(League::Nfl, vec![with_scoring(g("1", "KC", "TB", true))], false);
                app.merge_standings(tall_standings_table());
                app.view = view.clone();
                let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
                t.draw(|f| app.draw(f)).unwrap();
                let s = buf_text(&t);
                assert!(s.contains(needle), "{name} at {w}x{h} lost its header:\n{s}");
            }
        }
    }
}

#[test]
fn no_screen_floats_a_dead_column() {
    // spec v3.3 §5: at 120x40 the key bar sits with the content it describes
    // (content_end+1), not stranded on the terminal floor under a gulf of
    // blank rows, and the block is centered rather than pinned left.
    use gameday::views::View;
    let mut app = App::new(
        Config::default_all(),
        vec![],
        config_dir("dead-column"),
        time::UtcOffset::UTC,
    );
    app.config.favorites.push(gameday::config::Favorite {
        league: League::Nfl,
        team_abbr: "KC".into(),
    });
    app.view = View::ConfigView;
    let mut t = Terminal::new(TestBackend::new(120, 40)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t);
    let lines: Vec<&str> = s.lines().collect();
    let bar = lines
        .iter()
        .position(|l| l.contains("? help"))
        .unwrap_or_else(|| panic!("config has no key bar:\n{s}"));
    // The CONFIG chip is chrome, like the top bar — the block under it is
    // what has to be centered.
    let top = lines.iter().position(|l| l.contains("CONFIG")).unwrap() + 1;
    let content: Vec<usize> = (top..bar).filter(|&y| !lines[y].trim().is_empty()).collect();
    let last = *content.last().unwrap_or_else(|| panic!("config has no content:\n{s}"));
    assert!(bar - last <= 2, "key bar floats {} rows under the content:\n{s}", bar - last);
    // The block is centered: its left and right margins match.
    let left = content
        .iter()
        .map(|&y| lines[y].chars().count() - lines[y].trim_start().chars().count())
        .min()
        .unwrap();
    let right = 120
        - content
            .iter()
            .map(|&y| lines[y].trim_end().chars().count())
            .max()
            .unwrap();
    assert!(
        left.abs_diff(right) <= 2,
        "config block is not centered: left {left}, right {right}\n{s}"
    );

    // The two overlays carry their own key bar inside the panel, one row
    // under the last content row, and the panel is centered in the frame.
    for (name, open) in [
        ("help", Box::new(|a: &mut App| a.help_open = true) as Box<dyn Fn(&mut App)>),
        ("theme picker", Box::new(|a: &mut App| a.view = View::ThemePicker)),
    ] {
        let mut app = mk();
        app.apply_boards(League::Nfl, vec![g("1", "KC", "TB", true)], false);
        open(&mut app);
        let mut t = Terminal::new(TestBackend::new(120, 40)).unwrap();
        t.draw(|f| app.draw(f)).unwrap();
        let s = buf_text(&t);
        let lines: Vec<&str> = s.lines().collect();
        let bottom = lines
            .iter()
            .position(|l| l.contains('└'))
            .unwrap_or_else(|| panic!("{name}: no panel:\n{s}"));
        let key_row = lines[..bottom]
            .iter()
            .rposition(|l| l.contains("closes") || l.contains("REVERT"))
            .unwrap_or_else(|| panic!("{name}: panel has no key line:\n{s}"));
        assert!(
            bottom - key_row <= 2,
            "{name}: key line {} rows off the panel floor:\n{s}",
            bottom - key_row
        );
        let border = lines[bottom];
        let l = border.chars().position(|c| c == '└').unwrap();
        let r = border.chars().rev().position(|c| c == '┘').unwrap();
        assert!(l.abs_diff(r) <= 2, "{name}: panel off center: left {l}, right {r}\n{s}");
    }
}
