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

/// The column `needle` starts at on row `y` of the last frame, or None.
fn col_of(term: &Terminal<TestBackend>, y: u16, needle: &str) -> Option<u16> {
    let b = term.backend().buffer();
    let row: String = (0..b.area().width)
        .map(|x| b[(x, y)].symbol())
        .collect::<Vec<_>>()
        .concat();
    row.find(needle)
        .map(|byte| row[..byte].chars().count() as u16)
}

fn mk() -> App {
    let dir = std::env::temp_dir().join(format!("gd-draw-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    App::new(Config::default_all(), vec![], dir, time::UtcOffset::UTC)
}

#[test]
fn header_and_tabs_render() {
    // Only an enabled league with a game today earns a chip —
    // NFL and MLS both need boards to show up here at all.
    let mut app = mk();
    app.apply_boards(League::Nfl, vec![g("1", "KC", "TB", true)], false);
    let mut mls_game = g("2", "LAFC", "SEA", true);
    mls_game.league = League::Mls;
    app.apply_boards(League::Mls, vec![mls_game], false);
    // Wide enough for the whole header: the chips' brackets are the first
    // thing the shed ladder gives up (there is no `FILTER:` label at all).
    let mut wide = Terminal::new(TestBackend::new(180, 24)).unwrap();
    wide.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&wide);
    assert!(s.contains("GAMEDAY"), "{s}");
    assert!(!s.contains("FILTER:"), "the FILTER: label is gone: {s}");
    assert!(s.contains("[ALL]"), "{s}");
    assert!(s.contains("[ NFL ]"), "{s}");
    // At 120 with the enabled chips the brackets shed instead of the
    // clock, but the wordmark and every chip with a game still render.
    let mut t = Terminal::new(TestBackend::new(120, 24)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let row = buf_text(&t).lines().next().unwrap().to_string();
    assert!(row.contains("GAMEDAY"), "{row}");
    assert!(row.contains("ALL"), "{row}");
    assert!(row.contains("NFL") && row.contains("MLS"), "{row}");
    assert!(
        !row.contains("FILTER:"),
        "label sheds before the clock: {row}"
    );
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
    // The Board footer is the lowercase A′ legend — no
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
    // The Board legend names `s sort` and `v tv`; the old caps
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
    // The "?" overlay speaks the same lowercase grammar as
    // every footer now — group titles, chords and labels are all lowercase,
    // no hand-written caps.
    let mut app = mk();
    app.help_open = true;
    let mut t = Terminal::new(TestBackend::new(120, 36)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t);
    for needle in [
        "keys",
        "board",
        "zoom",
        "tv",
        "config",
        "standings & feed",
        "paging",
        "everywhere",
        // Chords the footer omits must still be discoverable here.
        "theme",
        "sort",
        "favorite",
        "ctrl-c",
        "s-tab",
    ] {
        assert!(s.contains(needle), "help overlay missing {needle:?}:\n{s}");
    }
}

#[test]
fn help_panel_speaks_the_lowercase_grammar_not_just_the_footer() {
    // The overlay's own KEYS panel — not just the footer strip
    // underneath it — must be lowercase. `App::draw_help` used
    // to render `NAVIGATION`, `SPC`, `ESC/?/Q CLOSES` verbatim, a second caps
    // grammar the footer-scoped tests never saw because they only inspect
    // the buffer's last row.
    let mut app = mk();
    app.help_open = true;
    let mut t = Terminal::new(TestBackend::new(120, 40)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t);
    // The panel's own new rows, lowercase.
    for needle in ["board", "space pin", "esc  q", "esc/?/q closes", " keys "] {
        assert!(s.contains(needle), "panel missing {needle:?}:\n{s}");
    }
    // No leftover caps-token grammar (the old NAVIGATION/SELECTION style,
    // the new BOARD/EVERYWHERE titles, and the footer's own dead NAV:
    // label) — every one of those is spoken lowercase instead.
    for caps in [
        "NAVIGATION",
        "SELECTION",
        "BOARD",
        "EVERYWHERE",
        " SPC",
        "ESC/?/Q CLOSES",
        "NAV:",
    ] {
        assert!(!s.contains(caps), "leftover caps token {caps:?}:\n{s}");
    }
}

/// U3: six modes in one column, `h/l` twice with two meanings. The overlay
/// is sectioned by mode, the current mode first, and closes with the glyphs.
#[test]
fn help_leads_with_the_current_mode_and_closes_with_the_legend() {
    use gameday::views::{View, ZoomTab};
    let mut app = mk();
    app.apply_boards(League::Nfl, vec![g("1", "KC", "TB", true)], false);
    app.tab = Tab::League(League::Nfl);
    app.view = View::Zoom {
        game_id: "1".into(),
        tab: ZoomTab::Overview,
    };
    app.help_open = true;
    let s = buf_text(&render(&mut app, 120, 50));
    let at = |needle: &str| {
        s.lines()
            .position(|l| l.trim_start().starts_with(needle))
            .unwrap_or_else(|| panic!("{needle:?} missing:\n{s}"))
    };
    assert!(at("zoom") < at("board"), "the current mode leads:\n{s}");
    assert!(
        at("board") < at("everywhere") && at("paging") < at("everywhere"),
        "{s}"
    );
    // tabs (h/l) is a zoom row; cycle (h/l) is a config row; league is everywhere.
    let row = |label: &str| {
        s.lines()
            .position(|l| l.contains(label))
            .unwrap_or_else(|| panic!("{label:?} missing:\n{s}"))
    };
    assert!((at("zoom")..at("board")).contains(&row("tabs")), "{s}");
    assert!(
        row("cycle") > at("config") && row("cycle") < at("standings & feed"),
        "{s}"
    );
    assert!(
        s.contains("▸ selected · ⚑ pinned · ★ favorite · ▌ hot · ↑n moved up"),
        "legend:\n{s}"
    );
    // The board leads from the board.
    app.view = View::Board;
    let s = buf_text(&render(&mut app, 120, 50));
    assert!(at_in(&s, "board") < at_in(&s, "zoom"), "{s}");
}

fn at_in(s: &str, needle: &str) -> usize {
    s.lines()
        .position(|l| l.trim_start().starts_with(needle))
        .unwrap_or_else(|| panic!("{needle:?} missing:\n{s}"))
}

/// U5: `:help` is a command.
#[test]
fn colon_help_opens_the_overlay() {
    use crossterm::event::KeyCode;
    let mut app = mk();
    key(&mut app, KeyCode::Char(':'));
    type_text(&mut app, "help");
    key(&mut app, KeyCode::Enter);
    assert!(app.help_open);
    assert_eq!(
        gameday::command::parse("help").unwrap(),
        gameday::command::Cmd::Help
    );
}

#[test]
fn zoomed_footer_shows_back_and_the_zoomed_game() {
    use gameday::views::{View, ZoomTab};
    let mut app = mk();
    app.apply_boards(League::Nfl, vec![g("1", "KC", "TB", true)], false);
    app.tab = Tab::League(League::Nfl);
    app.view = View::Zoom {
        game_id: "1".into(),
        tab: ZoomTab::Overview,
    };
    let mut t = Terminal::new(TestBackend::new(120, 24)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t);
    // Every non-board footer speaks the lowercase legend now —
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
fn the_footer_names_the_selected_games_leading_term() {
    let mut app = mk();
    let mut rz = g("1", "KC", "TB", true);
    // Early and tied: lateness*closeness stays small (~8), so the red zone
    // bonus (40, unscaled by margin) is the largest single term — a late,
    // close game would have the base outrank it instead (spec §4.3).
    rz.period = "Q1".into();
    rz.clock = "10:00".into();
    rz.away_score = 7;
    rz.home_score = 7;
    rz.situation = Some(Situation {
        is_red_zone: Some(true),
        possession: Some("KC".into()),
        ..Default::default()
    });
    // Also early, so IN PLAY's watch-sort doesn't outrank `rz` and steal the
    // selection at index 0 — this game carries no bonus, so its own low
    // lateness*closeness base keeps it below the red zone score.
    let mut other = g("2", "GB", "CHI", true);
    other.period = "Q1".into();
    other.clock = "14:00".into();
    app.apply_boards(League::Nfl, vec![rz, other], false);
    let mut t = Terminal::new(TestBackend::new(120, 36)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t);
    let footer = s.lines().last().unwrap();
    // At 120 columns the footer holds both LEADS and GAME.
    assert!(footer.contains("LEADS: RED ZONE"), "{footer}");
    assert!(footer.contains("GAME"), "{footer}");
    // A pre-game selection has no lead.
    let mut app2 = mk();
    app2.apply_boards(
        League::Nfl,
        vec![g("3", "NE", "SEA", false), g("4", "NO", "DET", false)],
        false,
    );
    let mut t2 = Terminal::new(TestBackend::new(120, 36)).unwrap();
    t2.draw(|f| app2.draw(f)).unwrap();
    assert!(!buf_text(&t2).lines().last().unwrap().contains("LEADS"));
    // At 100 columns LEADS is shed before GAME: the footer keeps GAME x/y
    // and drops LEADS.
    let mut t3 = Terminal::new(TestBackend::new(100, 36)).unwrap();
    t3.draw(|f| app.draw(f)).unwrap();
    let f3 = buf_text(&t3).lines().last().unwrap().to_string();
    assert!(f3.contains("GAME"), "{f3}");
    assert!(!f3.contains("LEADS"), "{f3}");
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
    // No mosaic and no boxed SLATE — scheduled games are the board's
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
        let row: String = (0..b.area().width)
            .map(|x| b[(x, y)].symbol().to_string())
            .collect();
        let x = row
            .find("UPD")
            .unwrap_or_else(|| panic!("no UPD in footer: {row:?}")) as u16;
        b[(x, y)].fg
    };
    assert_eq!(fg_of_upd("broadcast"), theme::builtin("broadcast").cyan);
    assert_eq!(fg_of_upd("studio"), theme::builtin("studio").muted);
    theme::set_current("broadcast").unwrap();
}

/// U7: a toast replaced the chords for as long as it lived. Now the chords
/// stay left, the toast sits right, and it is gone three seconds later.
#[test]
fn a_toast_sits_right_of_the_chords_and_expires() {
    let mut app = mk();
    app.now_override = Some(time::macros::datetime!(2026-09-07 20:00 UTC));
    app.apply_boards(League::Nfl, vec![g("1", "KC", "TB", true)], false);
    app.tab = Tab::League(League::Nfl);
    key(&mut app, crossterm::event::KeyCode::Char(' '));
    let footer = buf_text(&render(&mut app, 120, 24))
        .lines()
        .last()
        .unwrap()
        .to_string();
    assert!(footer.contains("q quit"), "chords stay: {footer:?}");
    assert!(
        footer.trim_end().ends_with("pinned KC@TB"),
        "toast right-aligned: {footer:?}"
    );
    assert!(
        !footer.contains("UPD"),
        "the toast takes the right side while it lives: {footer:?}"
    );
    app.now_override = Some(time::macros::datetime!(2026-09-07 20:00:02 UTC));
    app.advance_tick();
    assert!(app.status_line.is_some(), "two seconds: still up");
    app.now_override = Some(time::macros::datetime!(2026-09-07 20:00:03 UTC));
    app.advance_tick();
    assert!(app.status_line.is_none(), "TOAST_SECS reached");
    let footer = buf_text(&render(&mut app, 120, 24))
        .lines()
        .last()
        .unwrap()
        .to_string();
    assert!(footer.contains("UPD"), "the right side is back: {footer:?}");
}

#[test]
fn an_error_status_never_expires_on_its_own() {
    let mut app = mk();
    app.now_override = Some(time::macros::datetime!(2026-09-07 20:00 UTC));
    app.sticky_status("not saving: config.toml line 3: expected `=`");
    app.now_override = Some(time::macros::datetime!(2026-09-07 20:05 UTC));
    app.advance_tick();
    assert!(app.status_line.is_some(), "sticky");
    // A toast after it expires as usual.
    app.toast("pinned KC@TB");
    app.now_override = Some(time::macros::datetime!(2026-09-07 20:05:03 UTC));
    app.advance_tick();
    assert!(app.status_line.is_none());
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
    assert!(
        s.contains("[NFL]"),
        "board must render behind the picker:\n{s}"
    );
    let marked = s
        .lines()
        .find(|l| l.contains("▸ broadcast"))
        .unwrap_or_else(|| panic!("{s}"));
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
    // A second live game so IN PLAY is a real section (a lone live game is
    // the hero, not a section — the rule this test wants to check needs a
    // row under it).
    app.apply_boards(
        League::Nfl,
        vec![with_scoring(game), g("2", "DAL", "PHI", true)],
        false,
    );
    app.tab = Tab::League(League::Nfl);
    let mut t = Terminal::new(TestBackend::new(120, 36)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let b = t.backend().buffer();
    let area = *b.area();
    let fg_at = |needle: &str| -> ratatui::style::Color {
        for y in 0..area.height {
            let row: String = (0..area.width)
                .map(|x| b[(x, y)].symbol())
                .collect::<Vec<_>>()
                .join("");
            if let Some(pos) = row.find(needle) {
                return b[(row[..pos].chars().count() as u16, y)].fg;
            }
        }
        panic!("{needle:?} not on the board:\n{}", buf_text(&t));
    };
    // The sidebar and the tile chrome are gone. What carries the
    // theme's discipline now is the board itself — section rules in `cool`,
    // and the identity floor on the hero's digits.
    assert_eq!(fg_at("IN PLAY"), studio.roles().cool);
    let team = studio.art_color([200, 16, 46]);
    let colored = (0..area.height)
        .flat_map(|y| (0..area.width).map(move |x| (x, y)))
        .filter(|&(x, y)| b[(x, y)].fg == team)
        .count();
    assert!(
        colored > 0,
        "the hero's digits keep team color:\n{}",
        buf_text(&t)
    );
    theme::set_current("broadcast").unwrap();
}

#[test]
fn studio_spends_no_chroma_but_the_red_and_the_team_identity() {
    // Press-box studio is grayscale plus exactly one red. This is the
    // cell-level statement of that — every colored cell on a studio frame is a
    // gray, the hot red, or a team's own color on the hero (the identity
    // floor, which `roles.team = hero` keeps and no theme may spend).
    use gameday::theme;
    use ratatui::style::Color;
    theme::set_current("studio").unwrap();
    let th = theme::builtin("studio");
    let mut app = mk();
    let mut game = g("1", "KC", "BUF", true);
    // A hue the chrome could never justify: if navy shows up anywhere but the
    // hero's identity, this test says so.
    game.home.color = [0, 51, 141];
    game.home.alt_color = [255, 255, 255];
    app.apply_boards(League::Nfl, vec![game.clone()], false);
    app.tab = Tab::League(League::Nfl);
    // 80 cols is under the board's 100-col logo flank floor, so no logo art is
    // drawn and every remaining color is one the theme chose.
    let mut t = Terminal::new(TestBackend::new(80, 24)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let b = t.backend().buffer();
    let area = *b.area();
    let chroma = |c: Color| -> i32 {
        match c {
            Color::Rgb(r, g, bl) => r.max(g).max(bl) as i32 - r.min(g).min(bl) as i32,
            _ => 0,
        }
    };
    let is_red = |c: Color| -> bool {
        match c {
            Color::Rgb(r, g, bl) => r as i32 - g as i32 >= 64 && r as i32 - bl as i32 >= 64,
            _ => false,
        }
    };
    let team_colors: Vec<Color> = [game.away.color, game.home.color]
        .iter()
        .flat_map(|c| [th.art_color(*c), theme::dimmed(th.art_color(*c))])
        .collect();
    let mut navy = 0usize;
    for y in 0..area.height {
        for x in 0..area.width {
            let cell = &b[(x, y)];
            for (what, c) in [("fg", cell.fg), ("bg", cell.bg)] {
                if c == th.art_color(game.home.color) {
                    navy += 1;
                }
                assert!(
                    chroma(c) <= 8 || is_red(c) || team_colors.contains(&c),
                    "studio cell ({x},{y}) {what} = {c:?} is neither gray, the hot red, nor a team color\n{}",
                    buf_text(&t)
                );
            }
        }
    }
    assert!(
        navy > 0,
        "the hero must still wear the home team's navy:\n{}",
        buf_text(&t)
    );
    theme::set_current("broadcast").unwrap();
}

// The GLOBAL ALERTS / TOP PLAYS / RECORDS sidebar is deleted, and
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

/// U6: a traveled day with no games was a blank screen and a lone "next
/// kickoff". It names the day and the next game the app knows about.
#[test]
fn an_empty_day_names_the_next_game_or_says_nothing_is_scheduled() {
    use crossterm::event::KeyCode;
    let mut app = mk();
    let now = time::macros::datetime!(2026-09-07 12:00 UTC); // a Monday
    app.now_override = Some(now);
    let mut next = g("n1", "NYY", "BOS", false);
    next.league = League::Mlb;
    next.start = Some(time::macros::datetime!(2026-09-08 19:10 UTC));
    app.apply_boards(League::Mlb, vec![next], false);
    app.tab = Tab::League(League::Mlb);
    key(&mut app, KeyCode::Char('['));
    let date = app.viewed_date(League::Mlb).expect("traveled");
    app.merge_dated_board(League::Mlb, date, vec![]);
    let s = buf_text(&render(&mut app, 120, 24));
    assert!(
        s.contains("No MLB games SUN SEP 6 · next TUE 7:10 PM NYY @ BOS"),
        "{s}"
    );
    // Centered: the line sits in the body's vertical middle, not at the top.
    let y = s.lines().position(|l| l.contains("No MLB games")).unwrap();
    assert!(
        (8..=14).contains(&y),
        "centered in a 24-row frame, got row {y}:\n{s}"
    );

    let mut app = mk();
    app.now_override = Some(now);
    app.apply_boards(League::Mlb, vec![], false);
    app.tab = Tab::League(League::Mlb);
    let s = buf_text(&render(&mut app, 120, 24));
    assert!(
        s.contains("No MLB games MON SEP 7 · nothing scheduled"),
        "{s}"
    );
    assert!(!s.contains("next kickoff"), "{s}");
}

#[test]
fn home_with_nothing_live_still_lists_the_day() {
    let mut app = mk();
    // Home is ONE list of the day, so a board with nothing live is
    // not an empty board — it is a LATER section. (The "nothing live · next:"
    // message stays for a board with no games at all; the empty-Home strings
    // themselves are pinned by `empty_home_with_no_boards_points_at_config`.)
    app.apply_boards(League::Nfl, vec![g("1", "KC", "TB", false)], false);
    let mut t = Terminal::new(TestBackend::new(120, 24)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t);
    assert!(s.contains("LATER"), "{s}");
    assert!(s.contains("KC") && s.contains("TB"), "{s}");
    assert!(
        !s.contains("nothing live"),
        "a scheduled game is not an empty board:\n{s}"
    );
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
    // The only live game is the hero — its score is digit glyphs and
    // the tile's "[NFL] LIVE" chip is gone with the tile. A lone live game
    // is the hero, not a section, so there is no IN PLAY rule to draw.
    assert!(s.contains("KC") && s.contains("TB"), "{s}");
    assert!(
        s.contains('█') || s.contains("27"),
        "the score renders in some form:\n{s}"
    );
    assert!(!s.contains("IN PLAY"), "{s}");
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
    app.view = View::Zoom {
        game_id: "1".into(),
        tab: ZoomTab::Overview,
    };
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
    assert!(
        !footer.contains("NAV:"),
        "chords must yield to the prompt: {footer:?}"
    );
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
fn the_theme_note_is_in_the_footer_at_startup() {
    let mut app = mk();
    let (_, note) = gameday::theme::select_or_default_noting("dracula");
    app.status_line = note;
    let mut t = Terminal::new(TestBackend::new(120, 36)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t);
    assert!(s.lines().last().unwrap().contains("not found"), "{s}");
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

/// The footer says how many games the filter left: `/tb · 2 games`.
#[test]
fn the_filter_footer_counts_what_it_kept() {
    let mut app = mk();
    app.apply_boards(
        League::Nfl,
        vec![
            g("1", "KC", "TB", true),
            g("2", "DAL", "TB", true),
            g("3", "GB", "CHI", true),
        ],
        false,
    );
    app.tab = Tab::League(League::Nfl);
    app.filter = Some("tb".into());
    let mut t = Terminal::new(TestBackend::new(120, 24)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t);
    assert!(s.contains("/tb · 2 games"), "{s}");
    app.filter = Some("kc".into());
    let mut t = Terminal::new(TestBackend::new(120, 24)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t);
    assert!(s.contains("/kc · 1 game"), "{s}");
    assert!(!s.contains("1 games"), "{s}");
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
        View::Zoom {
            game_id: "1".into(),
            tab: ZoomTab::Overview
        }
    );
    let mut t = Terminal::new(TestBackend::new(120, 24)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t);
    assert!(
        s.contains("OVERVIEW │ PLAYS │ STATS"),
        "tab bar missing:\n{s}"
    );
    // OVERVIEW is highlighted: its cells are styled unlike the idle PLAYS tab.
    let b = t.backend().buffer();
    let (mut over_style, mut plays_style) = (None::<Style>, None::<Style>);
    let area = b.area();
    for y in 0..area.height {
        let row: String = (0..area.width)
            .map(|x| b[(x, y)].symbol())
            .collect::<Vec<_>>()
            .join("");
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
        View::Zoom {
            game_id: "1".into(),
            tab: ZoomTab::Plays
        }
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
        View::Zoom {
            game_id: "1".into(),
            tab: ZoomTab::Stats
        }
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
    // `/ filter` is in the fixed Board legend; `:` earns no
    // footer slot there (CMD is still reachable via `:` and the `?`
    // overlay) — the old bracket-caps "[:] CMD"/"[/] FILTER" style is gone.
    let mut app = mk();
    let mut t = Terminal::new(TestBackend::new(120, 24)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t);
    assert!(s.contains("/ filter"), "{s}");
    assert!(!s.contains("CMD"), "{s}");
    // The Zoomed footer keeps the generic list, CMD included —
    // just lowercase now, same as every other non-board view.
    use gameday::views::{View, ZoomTab};
    app.apply_boards(League::Nfl, vec![g("1", "KC", "TB", true)], false);
    app.tab = Tab::League(League::Nfl);
    app.view = View::Zoom {
        game_id: "1".into(),
        tab: ZoomTab::Overview,
    };
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
    // way out and help never get clipped (the shed-order discipline).
    let mut app = mk();
    let mut t = Terminal::new(TestBackend::new(45, 24)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let footer = buf_text(&t).lines().last().unwrap().to_string();
    assert!(footer.contains("? help"), "help clipped: {footer:?}");
    assert!(footer.contains("q quit"), "quit clipped: {footer:?}");
    assert!(
        !footer.contains("move"),
        "move should be shed first: {footer:?}"
    );
}

#[test]
fn config_footer_at_40_cols_keeps_help() {
    // FOOTER_DROP_ORDER lacked entries for config's
    // toggle/edit/cycle rows, so at 40 cols they never shed and `? help`
    // got clipped off the right edge instead — HELP must never shed.
    use gameday::views::View;
    let mut app = mk();
    app.view = View::ConfigView;
    let mut t = Terminal::new(TestBackend::new(40, 24)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t);
    let footer = s
        .lines()
        .find(|l| l.contains("esc") || l.contains("back"))
        .unwrap_or_default()
        .to_string();
    assert!(footer.contains("? help"), "help clipped: {s}");
}

/// Longest run of consecutive ASCII-uppercase letters in `s`, excluding the
/// footer's known status readouts (`FOCUS`, `UPD`, `GAME`) — those are
/// clock-shaped status text, not the "NAV:" chord grammar the footers no
/// longer speak. Team abbreviations (`KC`, `TB`) are 2-3 letters and never trip
/// the ≤3 budget on their own.
fn max_caps_run_excluding_status(s: &str) -> usize {
    let stripped = s
        .replace("FOCUS", "")
        .replace("UPD", "")
        .replace("GAME", "");
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
    // One footer grammar everywhere — plays feed, standings,
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
                app.view = View::Zoom {
                    game_id: "1".into(),
                    tab: ZoomTab::Overview,
                };
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
        // The key bar is no longer always the last row — the
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
    // FOOTER_DROP_ORDER's shed discipline carries over to
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
        // A feed that fits its pane pulls the key bar up under
        // its last row, so find the legend instead of taking the floor row.
        let s = buf_text(&t);
        let footer = s
            .lines()
            .find(|l| l.contains("? help"))
            .unwrap_or_else(|| panic!("{view:?}: no key bar:\n{s}"))
            .to_string();
        assert!(
            footer.contains("? help"),
            "{view:?}: help clipped: {footer:?}"
        );
        assert!(
            footer.contains("esc back"),
            "{view:?}: back clipped: {footer:?}"
        );
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
    app.view = View::Zoom {
        game_id: "1".into(),
        tab: ZoomTab::Stats,
    };
    app.merge_stats(
        "1",
        GameStats {
            rows: vec![
                StatRow {
                    label: "Total Yards".into(),
                    away: "251".into(),
                    home: "277".into(),
                },
                StatRow {
                    label: "Turnovers".into(),
                    away: "1".into(),
                    home: "0".into(),
                },
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
    app.view = View::Zoom {
        game_id: "1".into(),
        tab: ZoomTab::Stats,
    };
    let mut t = Terminal::new(TestBackend::new(120, 24)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t);
    assert!(s.contains("no stats yet"), "{s}");
}

/// L4: `BOISPASSING YARDS`. The leaders column is the row grid's abbr pitch.
#[test]
fn stats_leaders_keep_a_gap_after_a_four_letter_code() {
    use gameday::views::{View, ZoomTab};
    let mut app = mk();
    app.apply_boards(League::Nfl, vec![g("1", "BOIS", "ORE", true)], false);
    app.tab = Tab::League(League::Nfl);
    app.stats.insert(
        "1".into(),
        GameStats {
            rows: vec![StatRow {
                label: "Total yards".into(),
                away: "412".into(),
                home: "388".into(),
            }],
            leaders: vec![Leader {
                team: "BOIS".into(),
                label: "Passing yards".into(),
                text: "M. Madsen 24/31, 288".into(),
            }],
        },
    );
    app.view = View::Zoom {
        game_id: "1".into(),
        tab: ZoomTab::Stats,
    };
    let term = render(&mut app, 120, 40);
    let s = buf_text(&term);
    let y = s
        .lines()
        .position(|l| l.contains("PASSING YARDS"))
        .expect("leader row") as u16;
    let abbr = col_of(&term, y, "BOIS").expect("abbr");
    assert_eq!(
        col_of(&term, y, "PASSING YARDS"),
        Some(abbr + gameday::board::rows::ABBR_W),
        "{s}"
    );
    assert!(!s.contains("BOISPASSING"), "{s}");
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
    // and scoring word, and carrying the matchup score for orientation. The
    // word is the league's own and carries no "!" — the block letters on the
    // cut are the exclamation, so the feed's copy is the bare noun.
    let nfl_row = lines
        .iter()
        .find(|l| l.contains("Mahomes to Kelce"))
        .unwrap_or_else(|| panic!("NFL scoring play missing from feed:\n{s}"));
    assert!(
        nfl_row.contains("NFL") && nfl_row.contains("TOUCHDOWN"),
        "{nfl_row}"
    );
    assert!(
        nfl_row.contains("KC@TB") && nfl_row.contains("27-24"),
        "{nfl_row}"
    );
    let nba_row = lines
        .iter()
        .find(|l| l.contains("Tatum pull-up three"))
        .unwrap_or_else(|| panic!("NBA scoring play missing from feed:\n{s}"));
    assert!(
        nba_row.contains("NBA") && nba_row.contains("BUCKET"),
        "{nba_row}"
    );
    // Row 0 (the NFL play — enabled-tab order) carries the ▸ marker.
    assert!(
        nfl_row.contains("▸"),
        "marker must start on row 0: {nfl_row}"
    );
    assert!(!nba_row.contains("▸"), "only one row is marked: {nba_row}");
}

#[test]
fn plays_feed_truncates_long_play_text_and_keeps_the_score() {
    // The row's stamp/abbr columns pad but never
    // truncated the play text, so an absurdly long play could push the
    // matchup score off the right edge. It must survive at 80 cols.
    use gameday::views::View;
    let mut app = mk();
    let mut nfl = g("1", "KC", "TB", true);
    nfl.last_plays = vec![Play {
        clock: "1:27".into(),
        team: "KC".into(),
        text: "Mahomes scrambles left, evades three defenders, laterals to Kelce \
               who breaks two tackles and dives for the pylon on a truly absurd play"
            .into(),
        scoring: true,
        ..Default::default()
    }];
    app.apply_boards(League::Nfl, vec![with_scoring(nfl)], false);
    app.view = View::PlaysFeed;
    let mut t = Terminal::new(TestBackend::new(80, 24)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t);
    let row = s
        .lines()
        .find(|l| l.contains("Mahomes scrambles"))
        .unwrap_or_else(|| panic!("play row missing:\n{s}"));
    assert!(
        row.contains('…'),
        "long play text should be truncated with an ellipsis: {row:?}"
    );
    assert!(
        row.contains("KC@TB") && row.contains("27-24"),
        "score must survive: {row:?}"
    );
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
    assert!(
        marked.contains("Tatum pull-up three"),
        "marker follows j: {marked}"
    );
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
        season_type: None,
        groups: vec![
            StandingsGroup {
                name: "American Football Conference".into(),
                rows: vec![row("KC", "Chiefs", 11, 6, 0), row("BUF", "Bills", 10, 7, 1)],
            },
            StandingsGroup {
                name: "National Football Conference".into(),
                rows: vec![
                    row("PHI", "Eagles", 12, 5, 0),
                    row("DAL", "Cowboys", 9, 8, 0),
                ],
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
    assert_eq!(
        wins_end, w_col,
        "wins not aligned under W:\nheader: {header:?}\nrow:    {kc_row:?}"
    );
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

/// With no CFB table in hand the view says the fetch came back empty rather
/// than "no standings yet", which reads as a fetch still in flight — and it
/// never claims ESPN has no FBS table, because it does. It also names no
/// command: there is no per-conference form to point at.
#[test]
fn standings_view_for_cfb_says_the_table_is_missing_not_pending() {
    use gameday::views::View;
    let mut app = mk();
    app.view = View::Standings(League::Cfb);
    let mut t = Terminal::new(TestBackend::new(120, 24)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t);
    assert!(s.contains("no FBS standings right now"), "{s}");
    assert!(
        !s.contains(":standings <conf>"),
        "the parser has no per-conference form; do not offer one:\n{s}"
    );
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
    assert!(
        header.contains("·  updated "),
        "no updated label:\n{header:?}"
    );
    assert!(!header.contains("2025-26"), "{header:?}");

    let mut table = standings_table();
    table.season = Some("2025-26".into());
    app.merge_standings(table);
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t);
    let header = s.lines().find(|l| l.contains("STANDINGS")).unwrap();
    assert!(
        header.contains("·  2025-26"),
        "no season label:\n{header:?}"
    );
    assert!(
        !header.contains("updated"),
        "season wins over the age:\n{header:?}"
    );
}

#[test]
fn a_preseason_table_says_so_in_the_header() {
    let mut app = mk();
    let mut table = standings_table();
    table.season = Some("2026".into());
    table.season_type = Some(1);
    app.merge_standings(table);
    key(&mut app, crossterm::event::KeyCode::Char(':'));
    for c in "standings".chars() {
        key(&mut app, crossterm::event::KeyCode::Char(c));
    }
    key(&mut app, crossterm::event::KeyCode::Enter);
    let mut t = Terminal::new(TestBackend::new(120, 36)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t);
    assert!(s.contains("2026 PRESEASON"), "{s}");
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
        season_type: None,
        groups: vec![
            group("American Football Conference", "A"),
            group("National Football Conference", "N"),
        ],
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
    // 80 cols, where the table is one column and 41 lines
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
    assert!(
        bottom < lines - 1,
        "offset must clamp against the pane: {bottom} of {lines}"
    );
    let s = buf_text(&t);
    assert!(
        s.contains("NTEAM17"),
        "bottom of the table is on screen:\n{s}"
    );
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
    // 80 cols — the one-column width, where a 41-line table is
    // actually clipped by a 24-row pane.
    let mut t = Terminal::new(TestBackend::new(80, 24)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t);
    let marker = s
        .lines()
        .find(|l| l.contains('▼'))
        .unwrap_or_else(|| panic!("clipped table needs a ▼ more marker:\n{s}"));
    assert!(
        marker.contains("BELOW"),
        "marker counts what's hidden: {marker}"
    );
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
    assert!(
        marker.contains("ABOVE") && !marker.contains('▼'),
        "{marker}"
    );
    // A table that fits shows no marker at all.
    let mut small = mk();
    small.view = View::Standings(League::Nfl);
    small.merge_standings(standings_table());
    let mut t = Terminal::new(TestBackend::new(120, 36)).unwrap();
    t.draw(|f| small.draw(f)).unwrap();
    let s = buf_text(&t);
    assert!(
        !s.contains('▼') && !s.contains('▲'),
        "no marker when it fits:\n{s}"
    );
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
        // The key bar follows the content when the view fits,
        // so it is not always the floor row.
        let footer = s
            .lines()
            .find(|l| l.contains("? help"))
            .unwrap_or_else(|| panic!("{view:?}: no key bar:\n{s}"));
        // Lowercase, no "NAV:", no bracket-caps.
        assert!(!footer.contains("NAV:"), "{view:?}: {footer}");
        assert!(
            !footer.contains("tabs"),
            "{view:?}: zoom's tab cycle is a no-op here: {footer}"
        );
        assert!(footer.contains("back"), "{view:?}: {footer}");
        // "tab league" is advertised, so Tab must actually switch tabs.
        assert!(footer.contains("league"), "{view:?}: {footer}");
        assert_eq!(app.tab, Tab::Home);
        gameday::input::handle_key(&mut app, KeyCode::Tab, KeyModifiers::NONE);
        assert_eq!(
            app.tab,
            Tab::League(League::Nfl),
            "{view:?}: Tab switches league"
        );
        assert_eq!(
            app.view,
            View::Board,
            "{view:?}: a tab switch lands on the board"
        );
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
    let row = lines
        .iter()
        .position(|l| l.contains("Mahomes to Kelce"))
        .unwrap();
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
    assert!(
        s.contains('‹') && s.contains('›'),
        "header shows the viewed date:\n{s}"
    );

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
    // No ticker under the board, so a traveled board shows the
    // traveled slate and nothing of today.
    assert!(
        !s.contains("KC"),
        "today's board is hidden while traveling:\n{s}"
    );

    // Clamped at ±7.
    for _ in 0..20 {
        app.on_key(KeyCode::Char('['), KeyModifiers::NONE);
    }
    assert_eq!(app.viewed_date_offset.get(&League::Nfl), Some(&-7));
    // ']' steps forward; back at 0 the marker (and dated board) go away.
    for _ in 0..7 {
        app.on_key(KeyCode::Char(']'), KeyModifiers::NONE);
    }
    assert_eq!(
        app.viewed_date_offset
            .get(&League::Nfl)
            .copied()
            .unwrap_or(0),
        0
    );
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
    // Once, on the LATER row (the mosaic tile that repeated it is
    // deleted).
    assert!(s.contains("LATER"), "{s}");
    assert_eq!(s.matches("O/U 47.5").count(), 1, "{s}");
}

// ---- Mouse support ---------------------------------------------------------

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
    // Tiles are gone — every board row is a Hit::Row zone.
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
    assert_eq!(
        app.tab,
        Tab::Home,
        "clicks are inert under the help overlay"
    );
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
    // The SLATE strip is gone; a LATER row is a board row like any
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

// ---- Config view ----

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
    let mut app = App::new(
        Config::default_all(),
        vec![],
        config_dir("sections"),
        time::UtcOffset::UTC,
    );
    app.config.favorites.push(gameday::config::Favorite {
        league: League::Nfl,
        team_abbr: "KC".into(),
    });
    app.view = View::ConfigView;
    let mut t = Terminal::new(TestBackend::new(120, 36)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t);
    for needle in [
        "CONFIG",
        "TABS",
        "[x] NFL",
        "FAVORITES",
        "★ NFL KC",
        "ADD FAVORITE",
        "THEME",
        "SORT",
    ] {
        assert!(
            s.contains(needle),
            "missing {needle:?} in config view:\n{s}"
        );
    }
}

/// L6: the config editor started at row 15 of 40. It sits under its header.
#[test]
fn config_view_top_aligns_under_its_header() {
    use gameday::views::View;
    let mut app = mk();
    app.view = View::ConfigView;
    let term = render(&mut app, 120, 40);
    let s = buf_text(&term);
    let tabs_y = s
        .lines()
        .position(|l| l.contains("TABS"))
        .expect("TABS section");
    assert_eq!(
        tabs_y, 3,
        "header, CONFIG chip, one row of air, then TABS:\n{s}"
    );
}

#[test]
fn config_scroll_is_per_panel_not_shared() {
    // A single `skip` used to be applied to
    // BOTH the TABS panel and the FAVORITES/DISPLAY panel. With the cursor
    // scrolled to the bottom of the right panel (SORT) and a pane too short
    // to show everything, the shared skip walked the TABS panel's own
    // top row (NFL) off-screen too, even though TABS never needed to
    // scroll on its own. Each panel must scroll independently.
    use gameday::views::{config_view, View};
    let mut app = App::new(
        Config::default_all(),
        vec![],
        config_dir("panelscroll"),
        time::UtcOffset::UTC,
    );
    app.view = View::ConfigView;
    // Move the cursor to the last row: 9 tabs, ADD FAVORITE, THEME, SORT.
    for _ in 0..League::ALL.len() + 3 {
        key(&mut app, crossterm::event::KeyCode::Char('j'));
    }
    // Wide enough for two panels (>= 100), short enough that the block
    // (9 tab rows) doesn't fit in the pane.
    let area = ratatui::layout::Rect {
        x: 0,
        y: 0,
        width: 120,
        height: 5,
    };
    let mut t = Terminal::new(TestBackend::new(120, 5)).unwrap();
    t.draw(|f| {
        config_view::draw(&app, f, area);
    })
    .unwrap();
    let s = buf_text(&t);
    assert!(
        s.contains("NFL"),
        "TABS panel's own top row must stay visible — it never needed to scroll:\n{s}"
    );
}

#[test]
fn config_space_toggles_a_tab_and_round_trips_config_toml() {
    use crossterm::event::KeyCode;
    use gameday::views::View;
    let dir = config_dir("toggle");
    let mut app = App::new(
        Config::default_all(),
        vec![],
        dir.clone(),
        time::UtcOffset::UTC,
    );
    app.view = View::ConfigView;
    // Cursor starts on the first row: the NFL tab toggle.
    key(&mut app, KeyCode::Char(' '));
    assert!(
        !app.config.enabled_tabs.contains(&League::Nfl),
        "space disables NFL"
    );
    let saved = Config::load_from(&dir).unwrap();
    assert!(
        !saved.enabled_tabs.contains(&League::Nfl),
        "written through immediately"
    );
    key(&mut app, KeyCode::Char(' '));
    assert!(
        app.config.enabled_tabs.contains(&League::Nfl),
        "space re-enables"
    );
    let saved = Config::load_from(&dir).unwrap();
    assert!(saved.enabled_tabs.contains(&League::Nfl));
}

#[test]
fn config_enter_adds_a_typed_favorite_and_enter_removes_it() {
    use crossterm::event::KeyCode;
    use gameday::views::View;
    let dir = config_dir("fav");
    let mut app = App::new(
        Config::default_all(),
        vec![],
        dir.clone(),
        time::UtcOffset::UTC,
    );
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
        vec![gameday::config::Favorite {
            league: League::Nfl,
            team_abbr: "KC".into()
        }],
        "abbr resolves its league from the boards"
    );
    let saved = Config::load_from(&dir).unwrap();
    assert_eq!(
        saved.favorites, app.config.favorites,
        "written through immediately"
    );
    // The new favorite row took this index; Enter on it removes the favorite.
    key(&mut app, KeyCode::Enter);
    assert!(
        app.config.favorites.is_empty(),
        "enter on a favorite row removes it"
    );
    let saved = Config::load_from(&dir).unwrap();
    assert!(saved.favorites.is_empty());
}

#[test]
fn config_favorite_miss_names_the_abbr_and_the_league_form() {
    use crossterm::event::KeyCode;
    use gameday::views::View;
    let mut app = App::new(
        Config::default_all(),
        vec![],
        config_dir("favmiss"),
        time::UtcOffset::UTC,
    );
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
        vec![gameday::config::Favorite {
            league: League::Nhl,
            team_abbr: "EDM".into()
        }]
    );
}

// The config editor's SCORE/LAYOUT rows are gone; DISPLAY
// is THEME + SORT now.
#[test]
fn config_h_l_cycle_sort_and_persist() {
    use crossterm::event::KeyCode;
    use gameday::rank::SortKey;
    use gameday::views::View;
    let dir = config_dir("cycle");
    let mut app = App::new(
        Config::default_all(),
        vec![],
        dir.clone(),
        time::UtcOffset::UTC,
    );
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
    let mut app = App::new(
        Config::default_all(),
        vec![],
        config_dir("escpop"),
        time::UtcOffset::UTC,
    );
    app.view = View::ConfigView;
    for _ in 0..League::ALL.len() {
        key(&mut app, KeyCode::Char('j'));
    }
    key(&mut app, KeyCode::Enter);
    type_text(&mut app, "kc");
    key(&mut app, KeyCode::Esc);
    assert_eq!(
        app.view,
        View::ConfigView,
        "esc cancels the edit, not the view"
    );
    assert!(
        app.config.favorites.is_empty(),
        "cancelled edit commits nothing"
    );
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
    // The tile's `[B9]` play stamp went with the tile; the hero
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
    assert!(
        head.contains('R') && head.contains('H') && head.contains('E'),
        "{head:?}"
    );
    // Away row: per-inning runs, then R H E — R is the game score, not a sum.
    let away = lines[i + 1];
    assert!(away.trim_start().starts_with("SEA"), "away row: {away:?}");
    assert!(away.contains("  1  0  2   3  8  0"), "away R H E: {away:?}");
    assert!(
        lines[i + 2].contains("  0  2  0   2  5  1"),
        "home R H E: {:?}",
        lines[i + 2]
    );
    // A short pane keeps the tile whole instead of a headless strip.
    let mut short = Terminal::new(TestBackend::new(120, 19)).unwrap();
    short.draw(|f| app.draw(f)).unwrap();
    assert!(
        !buf_text(&short).contains("  1  2  3"),
        "linescore should be skipped under 20 rows"
    );
}

#[test]
fn zoom_scoring_rows_carry_the_period_and_the_clock() {
    let mut app = mk();
    let mut game = g("1", "TOW", "NAVY", true);
    game.league = League::Cfb;
    game.scoring_plays = vec![
        Play {
            id: "a".into(),
            period: "Q1".into(),
            clock: "9:32".into(),
            team: "NAVY".into(),
            text: "Gutierrez run for 3 yds".into(),
            scoring: true,
            ..Default::default()
        },
        Play {
            id: "b".into(),
            period: "Q2".into(),
            clock: "14:52".into(),
            team: "TOW".into(),
            text: "Indorf pass to Enterline for 48 yds".into(),
            scoring: true,
            ..Default::default()
        },
    ];
    app.apply_boards(League::Cfb, vec![game], false);
    key(&mut app, crossterm::event::KeyCode::Char('z'));
    let mut t = Terminal::new(TestBackend::new(140, 40)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t);
    assert!(s.contains("[Q2 14:52] TOW"), "{s}");
    assert!(s.contains("[Q1 9:32] NAVY"), "{s}");
}

#[test]
fn zoom_last_plays_do_not_print_the_clock_twice() {
    let mut app = mk();
    let mut game = g("1", "TOW", "NAVY", true);
    game.league = League::Cfb;
    game.last_plays = vec![Play {
        id: "c".into(),
        period: "Q2".into(),
        clock: "3:31".into(),
        team: "NAVY".into(),
        text: "(03:39) #11 J.Carlson punt 42 yards to the Towson04".into(),
        ..Default::default()
    }];
    app.apply_boards(League::Cfb, vec![game.clone()], false);
    key(&mut app, crossterm::event::KeyCode::Char('z'));
    let mut t = Terminal::new(TestBackend::new(140, 40)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t);
    assert!(s.contains("[Q2 3:31] NAVY #11 J.Carlson punt"), "{s}");
    assert!(
        !s.contains("(03:39)"),
        "the feed's own clock prefix is not drawn beside ours:\n{s}"
    );
    // The play text itself is untouched (presentation only).
    assert!(app.game_by_id("1").unwrap().last_plays[0]
        .text
        .starts_with("(03:39)"));
}

/// The SCORING list (`draw_feed`'s second block) truncates its own text
/// separately from LAST PLAYS — it must apply the same clock-prefix rule,
/// not just the bracket stamp.
#[test]
fn zoom_scoring_list_does_not_print_the_clock_twice_either() {
    let mut app = mk();
    let mut game = g("1", "BOIS", "ORE", true);
    game.league = League::Cfb;
    game.scoring_plays = vec![Play {
        id: "d".into(),
        period: "Q1".into(),
        clock: "11:09".into(),
        team: "BOIS".into(),
        text: "(11:09) M. Madsen pass to C. Bates for 13 yds, for a TD".into(),
        scoring: true,
        ..Default::default()
    }];
    app.apply_boards(League::Cfb, vec![game], false);
    key(&mut app, crossterm::event::KeyCode::Char('z'));
    let mut t = Terminal::new(TestBackend::new(140, 40)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t);
    assert!(s.contains("[Q1 11:09] BOIS"), "{s}");
    assert!(s.contains("M. Madsen"), "{s}");
    assert!(
        !s.contains("(11:09)"),
        "the feed's own clock prefix is not drawn beside ours:\n{s}"
    );
}

/// L5: five to eight blank rows under SCORING at 40 rows. The two feeds
/// share the body in proportion, each floored at four, so a game with the
/// plays to fill the pane fills it.
#[test]
fn zoom_overview_fills_the_pane_at_30_40_and_60_rows() {
    use gameday::views::{View, ZoomTab};
    let mut game = g("1", "KC", "TB", true);
    game.last_plays = (0..40u16)
        .map(|i| Play {
            clock: format!("{}:{:02}", 14 - i / 4, 59 - i),
            period: "Q1".into(),
            team: "KC".into(),
            text: format!("play {i}"),
            scoring: i % 4 == 0,
            ..Default::default()
        })
        .collect();
    let game = with_scoring(game);
    let mut app = mk();
    app.apply_boards(League::Nfl, vec![game], false);
    app.tab = Tab::League(League::Nfl);
    app.view = View::Zoom {
        game_id: "1".into(),
        tab: ZoomTab::Overview,
    };
    for h in [30u16, 40, 60] {
        let term = render(&mut app, 120, h);
        let s = buf_text(&term);
        let lines: Vec<&str> = s.lines().collect();
        let plays_y = lines
            .iter()
            .position(|l| l.contains("LAST PLAYS"))
            .expect("caption");
        let scoring_y = lines
            .iter()
            .position(|l| l.contains("SCORING"))
            .expect("caption");
        let last_body = h as usize - 2; // row h-1 is the footer
        assert!(
            !lines[last_body].trim().is_empty(),
            "blank band at {h} rows:\n{s}"
        );
        assert!(
            scoring_y - plays_y > 4,
            "LAST PLAYS keeps four rows at {h}:\n{s}"
        );
        assert!(
            last_body - scoring_y >= 4,
            "SCORING keeps four rows at {h}:\n{s}"
        );
    }
}

#[test]
fn a_zero_record_hides_once_the_game_is_underway() {
    let mut app = mk();
    let mut game = g("1", "BOIS", "ORE", true);
    game.away.record = "0-0".into();
    game.home.record = "1-0".into();
    app.apply_boards(League::Nfl, vec![game.clone()], false);
    let mut t = Terminal::new(TestBackend::new(140, 40)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t);
    assert!(s.contains("1-0"), "{s}");
    assert!(
        !s.contains("BOIS 0-0") && !s.contains("0-0 BOIS"),
        "a 0-0 during play says nothing:\n{s}"
    );
    // Pre-game keeps it: 0-0 before the opener is true.
    let mut pre = g("2", "NE", "SEA", false);
    pre.away.record = "0-0".into();
    let mut app2 = mk();
    app2.apply_boards(League::Nfl, vec![pre], false);
    key(&mut app2, crossterm::event::KeyCode::Char('z'));
    let mut t2 = Terminal::new(TestBackend::new(140, 40)).unwrap();
    t2.draw(|f| app2.draw(f)).unwrap();
    assert!(buf_text(&t2).contains("0-0"), "{}", buf_text(&t2));
}

#[test]
fn the_linescore_wears_team_colors() {
    // The linescore's team rows wear `theme::hero_pair`
    // colors instead of the gated `team_text` role, which could fall back
    // to plain `fg` (white) — the white-digit role miss a design review
    // caught on an NYY row.
    use gameday::views::{View, ZoomTab};
    let th = gameday::theme::current();
    let mut game = g("1", "KC", "TB", true);
    game.linescore = vec![(1, 0), (0, 2), (2, 0)];
    let periods = game.linescore.len();
    let (away_color, home_color, _) =
        gameday::theme::hero_pair(&th, game.away.color, game.home.color);
    assert_ne!(
        away_color, home_color,
        "the two rows must not collapse to one color"
    );

    let mut app = mk();
    app.apply_boards(League::Nfl, vec![game], false);
    app.tab = Tab::League(League::Nfl);
    app.view = View::Zoom {
        game_id: "1".into(),
        tab: ZoomTab::Overview,
    };
    let mut t = Terminal::new(TestBackend::new(120, 40)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let b = t.backend().buffer();
    let text = buf_text(&t);
    let lines: Vec<&str> = text.lines().collect();
    let head_i = lines
        .iter()
        .position(|l| l.contains("  1  2  3"))
        .unwrap_or_else(|| panic!("period header missing:\n{text}"));
    let away_y = (head_i + 1) as u16;
    let home_y = (head_i + 2) as u16;
    assert_eq!(
        b[(0, away_y)].fg,
        away_color,
        "KC linescore row wears the away hero color:\n{text}"
    );
    assert_eq!(
        b[(0, home_y)].fg,
        home_color,
        "TB linescore row wears the home hero color:\n{text}"
    );
    // Totals column ("R"): the last cell of the right-aligned 4-wide `R`
    // field, same absolute column on both rows regardless of digit count.
    let totals_x = (5 + 3 * periods + 3) as u16;
    assert!(
        b[(totals_x, away_y)]
            .modifier
            .contains(ratatui::style::Modifier::BOLD),
        "away totals column stays bold:\n{text}"
    );
    assert!(
        b[(totals_x, home_y)]
            .modifier
            .contains(ratatui::style::Modifier::BOLD),
        "home totals column stays bold:\n{text}"
    );
}

#[test]
fn zoom_logo_flanks_are_symmetric_or_absent() {
    // Both or neither. A team with no committed mark used to
    // leave the OTHER team's flank drawn while its own stayed empty — one
    // lone mark reads as a rendering bug, not as an intentional asymmetry.
    // This exercises `hero::draw_hero` directly (the board and the zoom
    // share this one function; the marks are hero-only).
    use gameday::board::hero::{draw_hero, HeroPlan};
    use gameday::domain::Team;

    fn team(abbr: &str, color: [u8; 3], key: &str) -> Team {
        Team {
            id: abbr.into(),
            abbr: abbr.into(),
            name: abbr.into(),
            color,
            logo_key: key.into(),
            ..Default::default()
        }
    }
    fn game(away_key: &str, home_key: &str) -> Game {
        Game {
            id: "1".into(),
            league: League::Nfl,
            away: team("KC", [227, 24, 55], away_key),
            home: team("BUF", [0, 51, 141], home_key),
            away_score: 27,
            home_score: 24,
            status: Status::Live,
            ..Default::default()
        }
    }
    fn plan() -> HeroPlan {
        HeroPlan {
            digits_full: true,
            chip: None,
            now: time::OffsetDateTime::UNIX_EPOCH,
            pinned: false,
            favorite: false,
            show_logos: true,
            selected: false,
        }
    }
    // No situation/last_plays/meter on `game()`, so no optional row is drawn
    // under the digit band — the whole area under the nameplate is band,
    // and the margin columns below are never touched by anything but a
    // flank mark.
    let (w, h) = (120u16, 12u16);
    // away digits: "27" @ 8-wide glyphs = 16 cols, right-aligned in the
    // 40-col left third, landing at x=24 — 0..20 is inside the margin with
    // room to spare. home digits: "24" lands at x=80..96 in the right
    // third — 100..120 is clear of it on the far side.
    let painted = |term: &Terminal<TestBackend>, xs: std::ops::Range<u16>| -> usize {
        let buf = term.backend().buffer();
        let mut n = 0;
        for y in 1..buf.area().height {
            for x in xs.clone() {
                let c = &buf[(x, y)];
                if c.symbol() != " " || c.bg != ratatui::style::Color::Reset {
                    n += 1;
                }
            }
        }
        n
    };
    let render = |g: &Game| -> Terminal<TestBackend> {
        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        term.draw(|f| draw_hero(f, f.area(), g, &plan())).unwrap();
        term
    };

    // Only KC (away) has committed art; BUF's key is unknown.
    let one_sided = game("nfl/kc", "nfl/zzz");
    let term = render(&one_sided);
    assert_eq!(
        painted(&term, 0..20),
        0,
        "one committed mark must not draw its own flank"
    );
    assert_eq!(
        painted(&term, 100..120),
        0,
        "…and the other side must stay empty too"
    );

    // Both KC and BUF have committed art: both flanks draw.
    let both = game("nfl/kc", "nfl/buf");
    let term = render(&both);
    assert!(
        painted(&term, 0..20) > 0,
        "both marks committed: away flank must draw"
    );
    assert!(
        painted(&term, 100..120) > 0,
        "both marks committed: home flank must draw"
    );

    // The height axis: nfl/kc is 6 rows tall, nfl/tb is 8. At a band height
    // of 7 (h=8, one row under the nameplate less than the reference 12),
    // KC's mark fits and TB's doesn't — the per-side fit check used to draw
    // KC's flank alone. Both-or-neither has to hold here too.
    let render_at = |g: &Game, h: u16| -> Terminal<TestBackend> {
        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        term.draw(|f| draw_hero(f, f.area(), g, &plan())).unwrap();
        term
    };
    let mismatched = game("nfl/kc", "nfl/tb");
    let term = render_at(&mismatched, 8); // band.height = 7: KC (6) fits, TB (8) doesn't
    assert_eq!(
        painted(&term, 0..20),
        0,
        "a short mark must not draw when the tall one doesn't fit"
    );
    assert_eq!(painted(&term, 100..120), 0, "…on either side");

    let term = render_at(&mismatched, 9); // band.height = 8: both KC and TB fit
    assert!(
        painted(&term, 0..20) > 0,
        "both marks fit the taller band: away flank must draw"
    );
    assert!(
        painted(&term, 100..120) > 0,
        "both marks fit the taller band: home flank must draw"
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
        (0..b.area().width)
            .map(|x| b[(x, 0)].symbol().to_string())
            .collect()
    };
    assert!(
        row.contains("OFFLINE · retry 40s  "),
        "chip padded: {row:?}"
    );
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
    let row: String = (0..b.area().width)
        .map(|x| b[(x, 0)].symbol().to_string())
        .collect();
    assert!(
        row.contains("OFFLINE"),
        "chip survives a narrow header: {row:?}"
    );
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
    assert!(
        !first.contains("NBA"),
        "a league with no games gets no chip: {first}"
    );

    // The sort chip is Board-only — it disappears in Zoom.
    use gameday::views::{View, ZoomTab};
    app.view = View::Zoom {
        game_id: "1".into(),
        tab: ZoomTab::Overview,
    };
    let mut tz = Terminal::new(TestBackend::new(180, 40)).unwrap();
    tz.draw(|f| app.draw(f)).unwrap();
    let zoomed_first = buf_text(&tz).lines().next().unwrap().to_string();
    assert!(!zoomed_first.contains("SORT:"), "{zoomed_first}");

    // Clock survives the sort chip across the same width sweep that already
    // guarantees the net chip and the tab ladder.
    for width in 40u16..=180 {
        let mut app = mk();
        app.now_override = Some(time::macros::datetime!(2026-08-31 21:37:05 +0));
        app.apply_boards(League::Nfl, vec![g("1", "KC", "TB", true)], false);
        let mut t = Terminal::new(TestBackend::new(width, 40)).unwrap();
        t.draw(|f| app.draw(f)).unwrap();
        let row = buf_text(&t).lines().next().unwrap().to_string();
        assert!(
            row.contains("9:37:05 PM"),
            "clock clipped at {width} with the sort chip: {row:?}"
        );
    }
}

/// Every one of the 9 leagues carries a live game today, so every enabled
/// league's chip actually earns its place: only a league with a game today
/// earns a chip. Ten chips total with `ALL` — what
/// `header_keeps_the_clock_with_ten_chips_at_120_columns` and the width
/// sweep below were named for, before the chip-gating change made a
/// boardless fixture render 0-1 chips regardless of the name.
fn app_with_every_league_live() -> App {
    let mut app = App::new(
        Config::default_all(),
        vec![],
        std::env::temp_dir(),
        time::UtcOffset::UTC,
    );
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
    // This used to run on a boardless fixture, which under chip-gating
    // renders 0-1 chips — the shed
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
    // The header's rule swept: from the narrowest board the app will render (40)
    // up, the clock is always whole and the status chip is never glued to
    // whatever follows it. Caught a clipped "9:37:05" at 40 and an
    // "OFFLINE9:37:05 PM" at 60.
    //
    // A boardless fixture renders 0-1 chips
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
            let row: String = (0..b.area().width)
                .map(|x| b[(x, 0)].symbol().to_string())
                .collect();
            assert!(
                row.contains("9:37:05 PM"),
                "clock clipped at {width} tab={tab:?}: {row:?}"
            );
            assert!(
                !row.contains("YET9") && !row.contains("YETMON"),
                "chip glued at {width} tab={tab:?}: {row:?}"
            );
        }
    }
}

#[test]
fn command_completion_shows_the_candidates_in_the_footer() {
    let mut app = App::new(
        Config::default_all(),
        vec![],
        std::env::temp_dir(),
        time::UtcOffset::UTC,
    );
    for c in [':', 'n'] {
        gameday::input::handle_key(
            &mut app,
            crossterm::event::KeyCode::Char(c),
            crossterm::event::KeyModifiers::NONE,
        );
    }
    gameday::input::handle_key(
        &mut app,
        crossterm::event::KeyCode::Tab,
        crossterm::event::KeyModifiers::NONE,
    );
    let mut term = Terminal::new(TestBackend::new(120, 40)).unwrap();
    term.draw(|f| app.draw(f)).unwrap();
    let last = buf_text(&term).lines().last().unwrap().to_string();
    assert!(
        last.contains(":nfl") && last.contains("nba") && last.contains("nhl"),
        "{last}"
    );
}

// ---------------------------------------------------------------------------
// The ranked board — one list, sections, band, selection, lane.

/// `live` live games, then `finals`, then `later`, all NFL, distinct abbrs so
/// a row can be found by text. Scores differ per game so the ranked order is
/// observable.
fn board_games(live: usize, finals: usize, later: usize) -> Vec<Game> {
    const PAIRS: [(&str, &str); 12] = [
        ("KC", "TB"),
        ("DAL", "PHI"),
        ("GB", "CHI"),
        ("SF", "SEA"),
        ("BUF", "MIA"),
        ("NYJ", "NE"),
        ("DEN", "LV"),
        ("ATL", "NO"),
        ("CIN", "BAL"),
        ("PIT", "CLE"),
        ("HOU", "IND"),
        ("MIN", "DET"),
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
    assert!(
        s.contains("SORTED BY WATCHABILITY"),
        "the rule names the sort:\n{s}"
    );
    assert!(s.contains("FINAL"), "FINAL section:\n{s}");
    assert!(s.contains("LATER"), "LATER section:\n{s}");
    // The hero's digits are drawn as glyph cells, not as "27 - 24" text — and
    // cell-level: the glyphs sit above the IN PLAY rule, in the away
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
    // The tile grammar is gone — no borders, no MOMENTUM rail, no
    // SLATE strip, no GLOBAL ALERTS sidebar.
    for dead in ['┌', '┐', '└', '┘'] {
        assert!(
            !s.contains(dead),
            "no box-drawing on the board ({dead}):\n{s}"
        );
    }
    for dead in ["MOMENTUM", "SLATE", "GLOBAL ALERTS", "TOP PLAYS", "RECORDS"] {
        assert!(!s.contains(dead), "{dead} is deleted:\n{s}");
    }
}

#[test]
fn a_section_with_no_rows_renders_no_header() {
    // An empty section prints no rule at all — no orphan
    // "FINAL ───" or "LATER ───" over nothing. Cell-scan every row (not a
    // whole-buffer string search) so a header hiding off the visible window
    // would not falsely pass.

    // Live games, zero later: no LATER header anywhere in the buffer.
    let mut app = board_app(6, 2, 0);
    let term = render(&mut app, 120, 40);
    assert!(
        !row_contains(&term, "LATER ─"),
        "no later games, no LATER header"
    );
    assert!(
        row_contains(&term, "FINAL ─"),
        "the FINAL section still renders"
    );

    // Live games, zero finals: no FINAL header anywhere in the buffer.
    let mut app = board_app(6, 0, 2);
    let term = render(&mut app, 120, 40);
    assert!(
        !row_contains(&term, "FINAL ─"),
        "no final games, no FINAL header"
    );
    assert!(
        row_contains(&term, "LATER ─"),
        "the LATER section still renders"
    );
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
    // The empty-list guard (above) only catches a
    // section with zero games. A *non-empty* section whose row budget the
    // window cuts to zero must not draw its rule either — the reviewer's
    // exact reproductions at 60x13 and 60x16.

    // LATER orphan: 8 finals + 8 later at 60x13 — the window runs out right
    // after FINAL's rows, so LATER's rule used to draw with nothing under it.
    let mut app = board_app(0, 8, 8);
    let term = render(&mut app, 60, 13);
    let s = buf_text(&term);
    assert!(
        !row_contains(&term, "LATER ─"),
        "no orphan LATER rule at 60x13:\n{s}"
    );
    assert!(
        s.contains("SCORES"),
        "the lane still fires for the truncated games:\n{s}"
    );
    assert!(
        s.contains("8 LATER"),
        "the lane still counts every later game:\n{s}"
    );

    // FINAL orphan: 6 live + 6 final + 6 later at 60x16, selection at the
    // top — IN PLAY's rows eat the window, FINAL's rule used to draw bare
    // directly above the SCORES lane.
    let mut app = board_app(6, 6, 6);
    app.selected = 0;
    let term = render(&mut app, 60, 16);
    let s = buf_text(&term);
    assert!(
        !row_contains(&term, "FINAL ─"),
        "no orphan FINAL rule at 60x16:\n{s}"
    );
    assert!(
        !row_contains(&term, "LATER ─"),
        "no orphan LATER rule either at 60x16:\n{s}"
    );
    assert!(
        s.contains("SCORES"),
        "the lane still fires for the truncated games:\n{s}"
    );
    // Two of this window's 16 rows are the band's reservation, so a
    // LIVE game is off-screen here too — and the lane names live games before
    // it counts anything, which is the whole point of the lane.
    assert!(
        s.contains("NYJ 15 NE 12"),
        "the lane names the off-screen live game:\n{s}"
    );
}

#[test]
fn a_section_granted_rows_still_shows_its_rule() {
    // Regression pin alongside the truncation-path fix: a non-empty section
    // that DOES get visible rows still gets its header — the fix must not
    // over-suppress rules that fit along with real content.
    let mut app = board_app(2, 2, 2);
    let term = render(&mut app, 120, 40);
    let s = buf_text(&term);
    assert!(
        row_contains(&term, "FINAL ─"),
        "FINAL still renders when it has room:\n{s}"
    );
    assert!(
        row_contains(&term, "LATER ─"),
        "LATER still renders when it has room:\n{s}"
    );
    assert!(
        !s.contains("SCORES"),
        "everything fits, no lane needed:\n{s}"
    );
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
        // The MY GAMES rule is hoisted into the band's reserved two
        // rows, so the row directly under it is the reservation's air and the
        // band's own rows start one lower.
        assert!(
            lines[at + 1].trim().is_empty(),
            "the reserved air row:\n{s}"
        );
        lines[at + 2..at + 4]
            .iter()
            .map(|l| l.trim().to_string())
            .collect()
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
    assert!(
        opening.contains("IN PLAY"),
        "the board opens at the top:\n{opening}"
    );
    assert!(
        !opening.contains("LATER"),
        "LATER starts off-screen:\n{opening}"
    );

    for _ in 0..20 {
        key(&mut app, crossterm::event::KeyCode::Char('j'));
    }
    assert_eq!(
        app.selected, 20,
        "j walks the whole list — 24 games, no wrap"
    );

    let term = render(&mut app, 120, 24);
    let buf = term.backend().buffer();
    let s = buf_text(&term);
    // The window moved: what was at the top is gone, what was off the bottom
    // is here.
    assert!(
        !s.contains("IN PLAY"),
        "the top of the list scrolled away:\n{s}"
    );
    assert!(
        s.contains("LATER"),
        "the window followed the selection:\n{s}"
    );

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
    // The size sweep walks even heights and
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
        assert_eq!(
            lines.len(),
            h as usize,
            "the frame is exactly h={h} rows:\n{s}"
        );

        // The selected row is visible: the bright caret in the nudge gutter.
        let bright = gameday::theme::current().bright;
        let caret_y = (0..buf.area().height).find(|&y| {
            (0..buf.area().width).any(|x| buf[(x, y)].symbol() == "▸" && buf[(x, y)].fg == bright)
        });
        let caret_y =
            caret_y.unwrap_or_else(|| panic!("selected row must be on screen at h={h}:\n{s}"));

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
    assert!(
        !s.contains("ALERTS"),
        "the v3.1 ticker is gone from the board:\n{s}"
    );

    let mut app = board_app(10, 2, 4);
    let term = render(&mut app, 80, 24);
    let s = buf_text(&term);
    let lines: Vec<&str> = s.lines().collect();
    let lane_y = (lines.len() - 2) as u16;
    let lane = lines[lane_y as usize];
    assert!(lane.contains("SCORES"), "one lane above the footer:\n{s}");
    // Cell-level: the lane's label starts at column 0 of that exact
    // row, bold in the section-rule `cool` role — not merely text that
    // happens to say SCORES somewhere on the board.
    let buf = term.backend().buffer();
    let r = gameday::theme::current().roles();
    assert_eq!(
        buf[(0, lane_y)].fg,
        r.cool,
        "SCORES label wears the rule color:\n{s}"
    );
    assert!(
        buf[(0, lane_y)]
            .modifier
            .contains(ratatui::style::Modifier::BOLD),
        "SCORES label is bold:\n{s}"
    );
    // The lane accounts for exactly what didn't fit. Here every live game is
    // on screen and it is LATER that ran out of rows, so the
    // lane degrades to the counts rather than naming a live game twice.
    let drawn = lines[..lines.len() - 2].join("\n");
    // 16 games; the 22-row body holds the band's two reserved rows,
    // the hero (6 rows), the 9 other live rows, the IN PLAY and FINAL rules
    // and both finals — so all four LATER games are off, and LATER's rule
    // goes with them rather than standing over nothing.
    assert!(
        lane.contains("4 OFF-SCREEN") && lane.contains("4 LATER"),
        "the lane counts exactly what is not drawn:\n{s}"
    );
    assert_eq!(
        drawn.matches("LATER").count(),
        0,
        "no orphan LATER rule:\n{s}"
    );
}

#[test]
fn scores_lane_lists_off_screen_games_only() {
    // Every other view gets the SAME off-screen SCORES lane the
    // Board would show at this size — gated by the identical
    // `layout::plan(...).scores_lane` truncation check, never a
    // second grammar, and never allocated when the Board itself wouldn't
    // truncate.
    use gameday::views::{View, ZoomTab};
    let mut app = board_app(10, 2, 4);
    app.tab = Tab::League(League::Nfl);
    app.view = View::Zoom {
        game_id: "l0".into(),
        tab: ZoomTab::Overview,
    };
    let small_term = render(&mut app, 80, 24);
    let small = buf_text(&small_term);
    assert!(
        small.contains("SCORES"),
        "the board would truncate at 80x24 too, so Zoom gets the lane:\n{small}"
    );
    // Cell-level: the lane sits on its own row — the bottom of the
    // frame's body, not the footer row itself — and its label is styled like
    // every other view's lane the same way the Board's own is.
    let lane_y = small
        .lines()
        .position(|l| l.contains("SCORES"))
        .expect("SCORES line") as u16;
    let footer_y = small.lines().count() as u16 - 1;
    assert!(
        lane_y < footer_y,
        "the lane is above the footer, not on it:\n{small}"
    );
    let buf = small_term.backend().buffer();
    let x = small
        .lines()
        .nth(lane_y as usize)
        .unwrap()
        .find("SCORES")
        .unwrap() as u16;
    // This is `ticker::draw_lane` (every other view's lane), not
    // `board::mod::draw_lane` — its own grammar, `th.muted` gutter, not the
    // Board's section-rule `cool`.
    let th = gameday::theme::current();
    assert_eq!(
        buf[(x, lane_y)].fg,
        th.muted,
        "the lane label wears the ticker's gutter color:\n{small}"
    );

    let mut wide_app = board_app(4, 0, 0);
    wide_app.tab = Tab::League(League::Nfl);
    wide_app.view = View::Zoom {
        game_id: "l0".into(),
        tab: ZoomTab::Overview,
    };
    let wide = buf_text(&render(&mut wide_app, 120, 40));
    assert!(
        !wide.contains("SCORES"),
        "everything fits on the board at 120x40 — no lane here either:\n{wide}"
    );

    // The Board itself never allocates these rows a second time: exactly one
    // SCORES lane, its own inline one.
    let mut board_view = board_app(10, 2, 4);
    let board_s = buf_text(&render(&mut board_view, 80, 24));
    assert_eq!(
        board_s.matches("SCORES").count(),
        1,
        "one lane, one owner:\n{board_s}"
    );
}

// ---- The size sweep -------------------------------------------------------

/// The brief's 14-game fixture: 8 live (one hot — RED ZONE), 3 final, 3
/// later, 2 pinned. All NFL, distinct abbrs so a row is identifiable.
fn sweep_games() -> Vec<Game> {
    const PAIRS: [(&str, &str); 14] = [
        ("KC", "TB"),
        ("DAL", "PHI"),
        ("GB", "CHI"),
        ("SF", "SEA"),
        ("BUF", "MIA"),
        ("NYJ", "NE"),
        ("DEN", "LV"),
        ("ATL", "NO"),
        ("CIN", "BAL"),
        ("PIT", "CLE"),
        ("HOU", "IND"),
        ("MIN", "DET"),
        ("LAR", "ARI"),
        ("NYG", "WSH"),
    ];
    let mut out = Vec::new();
    let mut next = 0usize;
    for (tag, n, status) in [
        ("l", 8, Status::Live),
        ("f", 3, Status::Final),
        ("p", 3, Status::Pre),
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
    // Make the first live game unambiguously hot: red zone.
    out[0].meter = Some(gameday::domain::Meter::RedZone { yards_to_goal: 3 });
    out
}

fn sweep_app() -> App {
    let mut app = mk();
    let games = sweep_games();
    // Two pins, per the brief — the last two later games (never the hero,
    // which only ever comes from a live game).
    app.pins = vec![
        gameday::config::Pin {
            game_id: "p1".into(),
            league: League::Nfl,
            final_at: None,
        },
        gameday::config::Pin {
            game_id: "p2".into(),
            league: League::Nfl,
            final_at: None,
        },
    ];
    app.apply_boards(League::Nfl, games, false);
    app.tab = Tab::League(League::Nfl);
    app
}

#[test]
fn the_board_survives_every_size_the_app_will_draw_at() {
    // The clock sweep pattern, board edition (the sizes ladder,
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
            assert!(
                has_caret,
                "{w}x{h}: selected row must be visible after 10 j presses:\n{}",
                buf_text(&t2)
            );

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
                    let after: String =
                        line.chars().skip(line[..cut].chars().count() + 1).collect();
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
    let y = (0..area.height).find(|&y| {
        (0..area.width).any(|x| buf[(x, y)].symbol() == "▸" && buf[(x, y)].fg == bright)
    })?;
    let row: String = (0..area.width)
        .map(|x| buf[(x, y)].symbol().to_string())
        .collect();
    for (away, home) in SWEEP_PAIRS {
        if row.contains(away) && row.contains(home) {
            return Some((away.to_string(), home.to_string()));
        }
    }
    None
}

const SWEEP_PAIRS: [(&str, &str); 14] = [
    ("KC", "TB"),
    ("DAL", "PHI"),
    ("GB", "CHI"),
    ("SF", "SEA"),
    ("BUF", "MIA"),
    ("NYJ", "NE"),
    ("DEN", "LV"),
    ("ATL", "NO"),
    ("CIN", "BAL"),
    ("PIT", "CLE"),
    ("HOU", "IND"),
    ("MIN", "DET"),
    ("LAR", "ARI"),
    ("NYG", "WSH"),
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
    assert_eq!(
        wide_pair, narrow_pair,
        "selection survives a resize, by identity"
    );

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

/// U1: `G`, `g`, PgDn did nothing across 189 games. Half a page is half
/// of what the last frame showed; the ends are the ends; nothing wraps.
#[test]
fn paging_keys_move_half_a_page_and_are_in_the_overlay() {
    use crossterm::event::{KeyCode, KeyModifiers};
    let mut app = board_app(40, 0, 0);
    app.tab = Tab::League(League::Nfl);
    let _ = render(&mut app, 120, 40);
    let shown = app.page_rows.expect("the draw records what it showed");
    assert!(
        (8..40).contains(&shown),
        "a 40-row frame shows part of 40 games: {shown}"
    );
    key(&mut app, KeyCode::PageDown);
    assert_eq!(app.selected, shown / 2, "half of the last frame");
    app.on_key(KeyCode::Char('d'), KeyModifiers::CONTROL);
    assert_eq!(app.selected, shown / 2 * 2, "ctrl-d is the same half page");
    key(&mut app, KeyCode::Char('G'));
    assert_eq!(app.selected, 39, "G is the end");
    key(&mut app, KeyCode::PageDown);
    assert_eq!(app.selected, 39, "no wrap at the end");
    let s = buf_text(&render(&mut app, 120, 40));
    assert!(
        s.contains("GAME 40/40"),
        "the window followed the caret:\n{s}"
    );
    key(&mut app, KeyCode::Char('g'));
    assert_eq!(app.selected, 0, "g is the top");
    key(&mut app, KeyCode::PageUp);
    assert_eq!(app.selected, 0, "no wrap at the top");
    key(&mut app, KeyCode::End);
    assert_eq!(app.selected, 39);
    key(&mut app, KeyCode::Home);
    assert_eq!(app.selected, 0);

    // The feed pages by the rows its pane showed.
    let mut app = mk();
    let games: Vec<Game> = (0..30)
        .map(|i| {
            with_scoring({
                let mut x = g(&format!("s{i}"), "KC", "TB", true);
                x.last_plays[0].scoring = true;
                x
            })
        })
        .collect();
    app.apply_boards(League::Nfl, games, false);
    app.view = gameday::views::View::PlaysFeed;
    let _ = render(&mut app, 120, 20);
    let shown = app.page_rows.expect("feed records its pane");
    key(&mut app, KeyCode::PageDown);
    assert_eq!(app.feed_scroll, shown / 2);
    key(&mut app, KeyCode::Char('G'));
    assert_eq!(app.feed_scroll, 29);

    // Advertised.
    let mut app = mk();
    app.help_open = true;
    let s = buf_text(&render(&mut app, 120, 40));
    for needle in [
        "paging",
        "pgdn/pgup",
        "ctrl-d/ctrl-u",
        "half page",
        "g/shift-g",
        "home/end",
        "top/bottom",
    ] {
        assert!(s.contains(needle), "help overlay missing {needle:?}:\n{s}");
    }
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
        ..Default::default()
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
    // The hard rule: there is ONE score formatter. The takeover asks
    // `hero::score_block` for its digits, so the same game rendered both ways
    // must be identical cell for cell inside the score's rect — a takeover
    // that re-implemented the glyphs would drift here immediately.
    use ratatui::layout::Rect;
    let game = cut_game("1");
    let play = scoring_play();

    // Both rungs of the ladder, because the shared call is only *proven*
    // shared at the sizes a test actually renders. 120×40 gives the takeover
    // enough middle for 8-row `PixelSize::Full` digits; 80×19 is the quad
    // rung — `cut::plan` picks `(Full word, score_full = false)` when
    // `8 + 4 <= middle < 8 + 8`, i.e. middle 12–15, i.e. height 16–19 at
    // width ≥ 72. The `painted` floors are the fixture's own cell counts
    // (24–21): 32 solid cells at Full, and 35 at quad (the table's `2`, `4`,
    // `2`, `1` come to 10+8+10+7), each floored a little under.
    for (w, h, want_full, floor) in [(120u16, 40u16, true, 32usize), (80, 19, false, 30)] {
        let area = Rect::new(0, 1, w, h - 1); // everything under the header row
        let (slot, full) = gameday::board::cut::score_slot(area, &game, &play);
        assert!(
            slot.width > 0 && slot.height > 0,
            "{w}x{h}: the takeover must reserve a score band"
        );
        assert_eq!(
            full, want_full,
            "{w}x{h}: this size is here to exercise the other rung"
        );

        let mut cut = Terminal::new(TestBackend::new(w, h)).unwrap();
        let fired = gameday::board::cut::Cut {
            game_id: game.id.clone(),
            play: play.clone(),
            full: true,
            until_tick: gameday::board::cut::CUT_TICKS,
        };
        cut.draw(|f| gameday::board::cut::draw_takeover(f, area, &game, &fired, 0))
            .unwrap();
        let mut hero = Terminal::new(TestBackend::new(w, h)).unwrap();
        hero.draw(|f| gameday::board::hero::score_block(f, slot, &game, full))
            .unwrap();

        let (a, b) = (cut.backend().buffer(), hero.backend().buffer());
        let mut painted = 0;
        for y in slot.y..slot.bottom() {
            for x in slot.x..slot.right() {
                assert_eq!(
                    a[(x, y)].symbol(),
                    b[(x, y)].symbol(),
                    "{w}x{h}: digit cell ({x},{y}) differs between the cut and the hero"
                );
                // The takeover paints its own ground across the whole area, so
                // an empty cell's fg differs by construction; the digits are the
                // claim, and every painted cell must match in color too.
                if a[(x, y)].symbol() != " " {
                    assert_eq!(
                        a[(x, y)].fg,
                        b[(x, y)].fg,
                        "{w}x{h}: digit color at ({x},{y}) differs between the cut and the hero"
                    );
                    painted += 1;
                }
            }
        }
        assert!(
            painted >= floor,
            "{w}x{h}: the score has to actually be drawn: {painted} cells"
        );
    }
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
    assert!(
        rows[0].contains("GAMEDAY"),
        "the header row survives the cut:\n{s}"
    );
    assert!(
        !s.contains("IN PLAY"),
        "the board is not drawn behind the takeover:\n{s}"
    );
    // The word is block letters, so it is cells in the hot role, not text.
    let b = t.backend().buffer();
    let hot = (1..40u16)
        .map(|y| {
            (0..120u16)
                .filter(|&x| b[(x, y)].fg == r.hot && b[(x, y)].symbol() != " ")
                .count()
        })
        .sum::<usize>();
    assert!(
        hot >= 40,
        "TOUCHDOWN must be painted in block letters: {hot} hot cells\n{s}"
    );
    assert!(
        s.contains("CHIEFS AT BILLS"),
        "the dim strip names the game:\n{s}"
    );
    assert!(
        s.contains("MAHOMES"),
        "the detail line comes from the play:\n{s}"
    );
    // The cut's chip, verbatim, on the takeover's one filled element.
    assert!(
        rows[1].contains("▲ SCORING PLAY · KC"),
        "the takeover chip is the spec's:\n{s}"
    );
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
    // `▲ HOME RUN · TEX Seager (32) · ATH 0 TEX 5` on a
    // hot ground, two rows, above an intact list.
    assert!(
        rows[1].starts_with("▲ TOUCHDOWN · KC MAHOMES · KC 24 BUF 21"),
        "band headline:\n{s}"
    );
    // Row two stopped repeating the play and became the
    // affordance — what enter does, and how long the band has left.
    assert!(
        rows[2].starts_with("enter jump · clears in"),
        "the band's second row is the jump affordance:\n{s}"
    );
    assert!(
        !rows[2].contains("KELCE"),
        "the play is not said twice:\n{s}"
    );
    assert!(
        s.contains("IN PLAY"),
        "the board is still there under the band:\n{s}"
    );
    // Cell level: the mark, and the hot fill across both rows including the
    // empty tail — the band is a bar of alert color, not a bare line.
    let b = t.backend().buffer();
    let r = gameday::theme::current().roles();
    assert_eq!(b[(0, 1)].symbol(), "▲", "the mark is the band's first cell");
    assert_eq!(b[(0, 1)].fg, r.ground, "band ink is the ground role on hot");
    for y in 1..=2u16 {
        for x in 0..120u16 {
            assert_eq!(
                b[(x, y)].bg,
                r.hot,
                "the whole band row {y} is hot at ({x},{y})"
            );
        }
    }
    assert_ne!(
        b[(0, 3)].bg,
        r.hot,
        "the fill stops at the band: row 3 is the board"
    );
}

/// A live board with one pinned game (so MY GAMES is the board's top rule)
/// and a score on the *unpinned* game — which is a band, not a takeover.
/// `fired` says whether the band is still up: the two frames are the same
/// board, the same tick's data, differing only in the band.
fn band_frames(fired: bool) -> Terminal<TestBackend> {
    let mut app = mk();
    app.tick = 400; // past the 30 s startup suppression
    let mut before = cut_game("1");
    before.away_score = 17;
    before.last_plays = vec![Play {
        text: "Mahomes pass short right to Kelce for 6 yards".into(),
        ..Default::default()
    }];
    let other = g("2", "DAL", "PHI", true);
    app.apply_boards(League::Nfl, vec![before, other.clone()], false);
    app.pins.push(gameday::config::Pin {
        game_id: "2".into(),
        league: League::Nfl,
        final_at: None,
    });
    let mut after = cut_game("1");
    after.last_plays = vec![scoring_play()];
    app.apply_boards(League::Nfl, vec![after, other], false);
    assert!(
        !app.cuts
            .active(app.tick)
            .expect("the unpinned score fires a band")
            .full,
        "the fixture's cut must be the quiet band, not a takeover"
    );
    if !fired {
        app.tick += gameday::board::cut::CUT_TICKS + 1; // the band has cleared
    }
    render(&mut app, 120, 40)
}

#[test]
fn the_board_never_jumps_when_a_band_fires() {
    // The band's two rows are RESERVED whenever anything is
    // live (`TierPlan::band_rows`), so a band that fires draws into rows the
    // board already gave up — the last layout jump in the app. The IN PLAY
    // rule is the witness: same y, cell for cell, band or no band.
    let quiet = band_frames(false);
    let fired = band_frames(true);
    let qs = buf_text(&quiet);
    let fs = buf_text(&fired);
    assert!(
        fs.lines()
            .nth(1)
            .is_some_and(|l| l.starts_with("▲ TOUCHDOWN")),
        "the fired frame must have the band up:\n{fs}"
    );
    assert!(
        !qs.lines().nth(1).is_some_and(|l| l.starts_with("▲")),
        "the quiet frame must have no band:\n{qs}"
    );
    // The band is an opaque bar, not a tint: the rule it covers must not read
    // through it. (It only ever covers rows the board reserved for it, so the
    // most it can hide is one section label for three seconds.)
    for y in 1..=2usize {
        let row = fs.lines().nth(y).expect("a band row");
        assert!(
            !row.contains('─') && !row.contains("PINNED"),
            "the covered rule shows through band row {y}:\n{fs}"
        );
    }
    let rule_y = |s: &str| {
        s.lines()
            .position(|l| l.contains("IN PLAY"))
            .unwrap_or_else(|| panic!("IN PLAY rule:\n{s}")) as u16
    };
    let (qy, fy) = (rule_y(&qs), rule_y(&fs));
    assert_eq!(
        qy, fy,
        "the IN PLAY rule moved when the band fired\nquiet:\n{qs}\nfired:\n{fs}"
    );
    // Cell level: not just the row index — the whole rule row is identical.
    let (qb, fb) = (quiet.backend().buffer(), fired.backend().buffer());
    for x in 0..120u16 {
        assert_eq!(
            qb[(x, qy)].symbol(),
            fb[(x, fy)].symbol(),
            "row {qy} cell {x} differs between quiet and fired\nquiet:\n{qs}\nfired:\n{fs}"
        );
    }
}

#[test]
fn the_reserved_rows_earn_their_keep_when_quiet() {
    // The reservation is not two blank lines: the board's top section rule
    // moves up into it, leaving one row of air under it, and the first
    // content row follows immediately below the reservation.
    let quiet = band_frames(false);
    let s = buf_text(&quiet);
    let rows: Vec<&str> = s.lines().collect();
    assert!(
        rows[1].starts_with("MY GAMES"),
        "the top section rule sits in the reservation's first row:\n{s}"
    );
    assert!(
        rows[2].trim().is_empty(),
        "the reservation's second row is air, not content:\n{s}"
    );
    assert!(
        !rows[3].trim().is_empty(),
        "the board's first content row follows the reservation:\n{s}"
    );
}

#[test]
fn enter_during_a_band_zooms_the_bands_game_not_the_selection() {
    // While a band is up, enter is the jump to the game that
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
    app.apply_boards(
        League::Nfl,
        vec![cut_game("1"), g("2", "DAL", "PHI", true)],
        false,
    );
    app.selected = 0;
    handle_key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(
        zoomed(&app),
        Some("1".into()),
        "with no band, enter still zooms the selection"
    );

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
    let cut = app
        .cuts
        .active(app.tick)
        .expect("the unpinned score fires a band");
    assert!(!cut.full && cut.game_id == "2");
    handle_key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(
        zoomed(&app),
        Some("2".into()),
        "enter jumped to the band's game, not the selection"
    );

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
    assert!(
        app.cuts.active(app.tick).is_none(),
        "no cut while a prompt is open"
    );

    // The config view's favorite-abbr editor: a text prompt in everything but
    // the enum. A takeover here blanks the editor while keystrokes keep
    // landing in the buffer the user can no longer see.
    let mut app = mk();
    app.tick = 400;
    app.config_edit = Some("K".into());
    land_a_score(&mut app, true);
    assert!(
        app.cuts.active(app.tick).is_none(),
        "no cut while the abbr editor is open"
    );

    // The theme picker is modal and IS a live preview.
    let mut app = mk();
    app.tick = 400;
    app.view = gameday::views::View::ThemePicker;
    land_a_score(&mut app, true);
    assert!(
        app.cuts.active(app.tick).is_none(),
        "no cut over the theme picker"
    );

    // Startup: the first boards arrive carrying a whole day of scores.
    let mut app = mk();
    app.tick = 12;
    land_a_score(&mut app, true);
    assert!(
        app.cuts.active(app.tick).is_none(),
        "no cut in the first 30 s"
    );

    // Same delta once the session is warm and no prompt is open: it fires.
    let mut app = mk();
    app.tick = 400;
    land_a_score(&mut app, true);
    let cut = app
        .cuts
        .active(app.tick)
        .expect("a warm, unblocked delta fires");
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

    // The jumbotron: `PixelSize::Full` digits are 8×8 cells and a band with
    // 16 rows for them paints every glyph row twice, so the away
    // score fills sixteen contiguous rows of the left third in KC's hero
    // color. The quad rung would paint four, the text form one.
    let th = gameday::theme::current();
    let (away_color, ..) = gameday::theme::hero_pair(&th, team("KC").color, team("TB").color);
    let buf = term.backend().buffer();
    let digit_rows: Vec<u16> = (0..40u16)
        .filter(|y| {
            (0..40u16)
                .filter(|x| buf[(*x, *y)].fg == away_color)
                .count()
                >= 8
        })
        .collect();
    assert_eq!(
        digit_rows.len(),
        16,
        "TV draws the doubled Full form:\n{text}"
    );
    assert_eq!(
        digit_rows[15] - digit_rows[0],
        15,
        "the digit band is contiguous: {digit_rows:?}\n{text}"
    );
    // The shown game is the ranking's top; both its abbrs are on the hero.
    assert!(text.contains("KC") && text.contains("TB"), "{text}");

    // TV stays logo-free, even at the ≥100 columns where the board
    // flanks its hero — the margin OUTSIDE the digits is untouched ground.
    // (KC and TB both have committed art, so this would paint otherwise.)
    // The margin is measured, not assumed: the axis gate lets a 2-digit score
    // take double-width glyphs, which start further left than the old fixed
    // `x < 20` window did. What must stay empty is whatever the digits did
    // not take.
    let digits_x = (0..120u16)
        .find(|x| digit_rows.iter().any(|y| buf[(*x, *y)].fg == away_color))
        .expect("the away digits are somewhere");
    assert!(
        digits_x > 0,
        "the away digits must leave a margin at all:\n{text}"
    );
    for y in digit_rows[0]..=digit_rows[15] {
        for x in 0..digits_x {
            assert_eq!(
                buf[(x, y)].symbol(),
                " ",
                "no hero mark in TV's margin at ({x},{y}), digits start at x={digits_x}:\n{text}"
            );
        }
    }

    // Everything else rides the strip, one row each.
    assert!(
        text.contains("ALSO LIVE"),
        "the strip names itself:\n{text}"
    );
    assert!(
        text.contains("5 GAMES"),
        "the strip counts the rest:\n{text}"
    );
    for abbr in [
        "DAL", "PHI", "GB", "CHI", "SF", "LAR", "NYJ", "MIA", "CIN", "BAL",
    ] {
        assert!(text.contains(abbr), "strip is missing {abbr}:\n{text}");
    }
    // TV is not the board: no section rules, no off-screen lane.
    assert!(!text.contains("IN PLAY"), "no board rules in TV:\n{text}");
    assert!(!text.contains("OFF-SCREEN"), "no lane in TV:\n{text}");
}

/// The jumbotron's own play ticker (`draw`'s `plays` loop, distinct from the
/// zoom's feed) must apply the same clock-prefix rule as the zoom.
#[test]
fn tv_ticker_plays_do_not_print_the_clock_twice_either() {
    use gameday::views::View;
    let mut app = mk();
    let mut game = g("1", "BOIS", "ORE", true);
    game.league = League::Cfb;
    game.last_plays = vec![Play {
        id: "e".into(),
        period: "Q2".into(),
        clock: "3:31".into(),
        team: "BOIS".into(),
        text: "(03:39) #11 J.Carlson punt 42 yards to the Towson04".into(),
        ..Default::default()
    }];
    app.apply_boards(League::Cfb, vec![game], false);
    app.tab = Tab::League(League::Cfb);
    key(&mut app, crossterm::event::KeyCode::Char('v'));
    assert_eq!(app.view, View::Tv);
    let mut t = Terminal::new(TestBackend::new(120, 40)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t);
    assert!(
        !s.contains("(03:39)"),
        "the feed's own clock prefix is not drawn beside ours:\n{s}"
    );
    assert!(s.contains("3:31"), "{s}");
}

#[test]
fn tv_fills_its_frame() {
    // The design review called TV the weakest frame — digits
    // half the mockup's height, ~6 dead rows under them, and a one-column
    // strip wasting half the width. All three are pinned here.
    use crossterm::event::{KeyCode, KeyModifiers};
    let mut app = mk();
    // A real live game, not the thin fixture: three plays, a linescore and a
    // red-zone meter is what the jumbotron is laying out on a Sunday, and a
    // frame is only "full" against the content it actually has.
    let slate: Vec<Game> = tv_slate()
        .into_iter()
        .map(|mut g| {
            g.linescore = vec![(7, 3), (10, 7), (0, 7), (7, 4)];
            g.meter = Some(Meter::RedZone { yards_to_goal: 3 });
            g.last_plays = (0..3)
                .map(|i| Play {
                    clock: format!("1:2{i}"),
                    team: g.away.abbr.clone(),
                    text: format!("Mahomes pass to Kelce for {} yards", i + 3),
                    ..Default::default()
                })
                .collect();
            g
        })
        .collect();
    app.apply_boards(League::Nfl, slate, false);
    app.tab = Tab::League(League::Nfl);
    app.on_key(KeyCode::Char('v'), KeyModifiers::NONE);
    let term = render(&mut app, 120, 40);
    let text = buf_text(&term);
    let buf = term.backend().buffer();
    let lines: Vec<&str> = text.lines().collect();
    use ratatui::style::Color;

    // The body is everything TV draws: the hero's nameplate row down to the
    // last strip row, header/reserved-band/footer excluded.
    let top = lines
        .iter()
        .position(|l| l.contains("KC"))
        .expect("hero nameplate") as u16;
    let footer = lines
        .iter()
        .rposition(|l| l.contains("esc board"))
        .expect("footer") as u16;
    let ground = gameday::theme::current().roles().ground;
    let empty: Vec<u16> = (top..footer)
        .filter(|y| {
            (0..120u16).all(|x| {
                buf[(x, *y)].symbol() == " "
                    && (buf[(x, *y)].bg == ground || buf[(x, *y)].bg == Color::Reset)
            })
        })
        .collect();
    // The budget, with its receipt. Before the rebudget the same frame left 20
    // dead rows: 8-row digits floating in a 25-row band. What is left is
    // structural, not slack —
    //   * 2 rows are the glyph cell's own baseline gap, doubled (`24`/`21`
    //     ink 7 of 8 glyph rows), and 1 is the band's air under the nameplate;
    //   * the rest is ONE gap between the hero block and the linescore, which
    //     is where the reference frame's own blank rows are
    //     (docs/research/v3-identity/tv-nfl-sunday-120x40.png has five).
    // Nine is what a 40-row terminal has left over once the doubled digits,
    // the fragment, the meter, the linescore, three plays and the strip have
    // taken their rows; the shape assertion below is the real claim — the
    // slack is one gap, never holes scattered through the stack.
    assert!(
        empty.len() <= 9,
        "the jumbotron leaves {} dead rows ({empty:?}), 9 is the budget:\n{text}",
        empty.len()
    );
    let runs = empty.windows(2).filter(|w| w[1] != w[0] + 1).count() + 1;
    assert!(
        runs <= 2,
        "the slack must be the band's air and ONE gap, not {runs} holes ({empty:?}):\n{text}"
    );

    // Doubled digits: 16 rows of away color, not 8.
    let th = gameday::theme::current();
    let (away_color, ..) = gameday::theme::hero_pair(&th, team("KC").color, team("TB").color);
    let digit_rows: Vec<u16> = (0..40u16)
        .filter(|y| {
            (0..40u16)
                .filter(|x| buf[(*x, *y)].fg == away_color)
                .count()
                >= 8
        })
        .collect();
    assert_eq!(
        digit_rows.len(),
        16,
        "the jumbotron doubles the Full form:\n{text}"
    );
    assert_eq!(
        digit_rows[15] - digit_rows[0],
        15,
        "contiguous: {digit_rows:?}\n{text}"
    );

    // Hero nameplates: the abbrs sit above the digits, in team color.
    assert!(
        top < digit_rows[0],
        "the nameplate row is above the digits:\n{text}"
    );
    // Columns, not byte offsets: nameplate marks (⚑/★) are multi-byte, so
    // a raw `find` position would walk off the buffer if either were drawn.
    let plate = lines[top as usize];
    let col = |byte: usize| plate[..byte].chars().count() as u16;
    let kc_x = col(plate.find("KC").expect("KC nameplate"));
    assert_eq!(
        buf[(kc_x, top)].fg,
        away_color,
        "the away nameplate wears the away color:\n{text}"
    );
    let tb_x = col(plate.rfind("TB").expect("TB nameplate"));
    let (_, home_color, _) = gameday::theme::hero_pair(&th, team("KC").color, team("TB").color);
    assert_eq!(
        buf[(tb_x, top)].fg,
        home_color,
        "the home nameplate wears the home color:\n{text}"
    );

    // Two columns on the strip at 120 cols: some row carries two games,
    // half a screen apart.
    let strip_top = lines
        .iter()
        .position(|l| l.contains("ALSO LIVE"))
        .expect("strip rule");
    let paired = lines[strip_top + 1..footer as usize].iter().find_map(|l| {
        let (a, b) = (l.find("DAL")?, l.rfind("NYJ")?);
        Some((a, b))
    });
    let (a, b) = paired.unwrap_or_else(|| panic!("no two-column strip row:\n{text}"));
    assert!(
        b - a >= 50,
        "the strip's second column starts at x={b}, first at x={a}:\n{text}"
    );
}

/// The doubled rung is still the ONE formatter: what
/// TV paints is cell-for-cell what `hero::score_block` paints into a 16-row
/// rect. Without this a jumbotron-only tweak drifts from the board's digits.
#[test]
fn the_jumbotron_digits_are_the_one_formatters_doubled_rung() {
    use crossterm::event::{KeyCode, KeyModifiers};
    let mut app = mk();
    app.apply_boards(League::Nfl, tv_slate(), false);
    app.tab = Tab::League(League::Nfl);
    app.on_key(KeyCode::Char('v'), KeyModifiers::NONE);
    let tv = render(&mut app, 120, 40);

    let game = tv_slate().into_iter().next().unwrap();
    let mut direct = Terminal::new(TestBackend::new(120, 16)).unwrap();
    direct
        .draw(|f| gameday::board::hero::score_block(f, f.area(), &game, true))
        .unwrap();

    // Glyph cells only: TV writes the clock and the chip in the center
    // column, inside the digits' bounding box, and that is not the claim
    // here — the claim is that every painted digit cell is the same cell.
    let glyphs = |term: &Terminal<TestBackend>| {
        digit_grid(term)
            .into_iter()
            .map(|row| {
                row.into_iter()
                    .map(|(s, fg)| {
                        if SCORE_GLYPHS.contains(&s.as_str()) {
                            (s, Some(fg))
                        } else {
                            (" ".to_string(), None)
                        }
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(
        glyphs(&direct).len(),
        14,
        "the direct render is the doubled rung (7 inked glyph rows × 2)"
    );
    assert_eq!(
        glyphs(&tv),
        glyphs(&direct),
        "TV's digits differ from `score_block`'s at the doubled rung:\n{}",
        buf_text(&tv)
    );
}

#[test]
fn tv_keeps_a_three_digit_score_on_the_full_rung() {
    // The axis gate's other half: a basketball jumbotron. 3 × 8 = 24 cells of
    // digits doubled would be 48, past the 40-col third at 120 columns, so
    // the columns must NOT double — and the score must not fall to the 4-row
    // quad form either. Rows double, columns don't, and the logo-free
    // margin survives.
    use crossterm::event::{KeyCode, KeyModifiers};
    let mut app = mk();
    let slate: Vec<Game> = tv_slate()
        .into_iter()
        .map(|mut g| {
            g.away_score = 118;
            g.home_score = 121;
            g
        })
        .collect();
    app.apply_boards(League::Nfl, slate, false);
    app.tab = Tab::League(League::Nfl);
    app.on_key(KeyCode::Char('v'), KeyModifiers::NONE);
    let term = render(&mut app, 120, 40);
    let text = buf_text(&term);
    let buf = term.backend().buffer();

    let th = gameday::theme::current();
    let (away_color, ..) = gameday::theme::hero_pair(&th, team("KC").color, team("TB").color);
    let digit_rows: Vec<u16> = (0..40u16)
        .filter(|y| {
            (0..40u16)
                .filter(|x| buf[(*x, *y)].fg == away_color)
                .count()
                >= 8
        })
        .collect();
    assert_eq!(
        digit_rows.len(),
        16,
        "the rows still double for a 3-digit score:\n{text}"
    );

    // 24 columns wide, right-aligned against the 40-col third: x 16..40.
    let inked: Vec<u16> = (0..40u16)
        .filter(|x| digit_rows.iter().any(|y| buf[(*x, *y)].fg == away_color))
        .collect();
    let (first, last) = (inked[0], inked[inked.len() - 1]);
    assert!(
        first >= 16,
        "3 × 8 = 24 columns of digits start at x=16, not {first}:\n{text}"
    );
    assert!(
        last < 40,
        "the away score stays inside its third, ended at {last}:\n{text}"
    );

    // Again: whatever the digits did not take is untouched ground.
    for y in digit_rows[0]..=digit_rows[15] {
        for x in 0..first {
            assert_eq!(
                buf[(x, y)].symbol(),
                " ",
                "no hero mark at ({x},{y}):\n{text}"
            );
        }
    }
}

#[test]
fn a_locked_game_going_final_never_leaves_tv_saying_nothing_is_live() {
    // One slate. The lock and the shown id are both validated
    // against the LIVE slate the strip draws from — a game that has gone
    // final can't stay "shown" while five games are live.
    use crossterm::event::{KeyCode, KeyModifiers};
    let mut app = mk();
    app.apply_boards(League::Nfl, tv_slate(), false);
    app.tab = Tab::League(League::Nfl);
    app.on_key(KeyCode::Char('v'), KeyModifiers::NONE);
    app.on_key(KeyCode::Char(' '), KeyModifiers::NONE);
    assert_eq!(
        app.tv_lock.as_deref(),
        Some("1"),
        "space locks the shown game"
    );

    let mut games = tv_slate();
    games[0].status = Status::Final;
    app.apply_boards(League::Nfl, games, false);
    assert_eq!(app.tv_lock, None, "the lock released with its game");

    let mut term = Terminal::new(TestBackend::new(120, 40)).unwrap();
    term.draw(|f| app.draw(f)).unwrap();
    let text = buf_text(&term);
    assert!(
        !text.contains("nothing is live"),
        "five games are live:\n{text}"
    );
    assert!(
        text.contains("DAL") && text.contains("PHI"),
        "the next live game leads:\n{text}"
    );
    assert!(
        text.contains("4 GAMES"),
        "the strip counts the remaining live games:\n{text}"
    );
}

#[test]
fn tv_never_jumps_when_a_band_fires() {
    // TV reserves the band's two rows exactly as the board
    // does (the earlier interim — "the band draws over the strip's top rows" —
    // closes here). The ALSO LIVE rule is the witness: same y, band or no.
    use crossterm::event::{KeyCode, KeyModifiers};
    let frame = |fired: bool| -> Terminal<TestBackend> {
        let mut app = mk();
        app.tick = 400;
        let mut games = tv_slate();
        app.apply_boards(League::Nfl, games.clone(), false);
        app.tab = Tab::League(League::Nfl);
        app.on_key(KeyCode::Char('v'), KeyModifiers::NONE);
        let shown = app.tv_shown.clone().expect("TV shows a game");
        // Score on a game that is neither shown nor MY GAMES: a band, not a
        // takeover.
        let other = games
            .iter_mut()
            .find(|g| g.id != shown)
            .expect("another live game");
        other.away_score += 7;
        other.last_plays = vec![scoring_play()];
        app.apply_boards(League::Nfl, games, false);
        let cut = app
            .cuts
            .active(app.tick)
            .expect("the unshown score fires a cut");
        assert!(
            !cut.full,
            "an unshown, unpinned game's cut is the quiet band"
        );
        if !fired {
            app.tick += gameday::board::cut::CUT_TICKS + 1;
        }
        render(&mut app, 120, 40)
    };
    let (quiet, fired) = (frame(false), frame(true));
    let (qs, fs) = (buf_text(&quiet), buf_text(&fired));
    assert!(
        fs.lines().nth(1).is_some_and(|l| l.starts_with('▲')),
        "the fired frame must have the band up:\n{fs}"
    );
    // The reservation is what makes the band harmless: when nothing is
    // firing those two rows are air, so the band covers no TV content at all.
    // (Without it the band landed on the shown game's nameplate row.)
    for y in 1..=2usize {
        assert!(
            qs.lines().nth(y).is_some_and(|l| l.trim().is_empty()),
            "TV's reserved row {y} must be air when quiet:\n{qs}"
        );
    }
    let strip_y = |s: &str| {
        s.lines()
            .position(|l| l.contains("ALSO LIVE"))
            .unwrap_or_else(|| panic!("ALSO LIVE rule:\n{s}")) as u16
    };
    assert_eq!(
        strip_y(&qs),
        strip_y(&fs),
        "TV's strip moved when the band fired\nquiet:\n{qs}\nfired:\n{fs}"
    );
    // Cell level, and the whole screen: the ONLY rows that may differ are the
    // band's own two. The nameplate, the digits, the plays and the strip are
    // all where they were — without the reservation the band painted straight
    // over TV's nameplate row, which is what the earlier interim left open.
    let (qb, fb) = (quiet.backend().buffer(), fired.backend().buffer());
    for y in 0..40u16 {
        if (1..=2).contains(&y) {
            continue; // the reserved rows: air when quiet, the band when fired
        }
        for x in 0..120u16 {
            assert_eq!(
                qb[(x, y)].symbol(),
                fb[(x, y)].symbol(),
                "TV moved at ({x},{y})\nquiet:\n{qs}\nfired:\n{fs}"
            );
        }
    }
}

#[test]
fn tv_never_panics_and_never_blanks_the_score() {
    // The clamping discipline: every slot in TV is derived from the
    // area, so no size may panic (a debug build catches the underflows) —
    // and none may leave the jumbotron without a score.
    use crossterm::event::{KeyCode, KeyModifiers};
    let mut app = mk();
    app.apply_boards(League::Nfl, tv_slate(), false);
    app.tab = Tab::League(League::Nfl);
    app.on_key(KeyCode::Char('v'), KeyModifiers::NONE);
    for (w, h) in [
        (40, 12),
        (41, 13),
        (60, 20),
        (80, 24),
        (100, 30),
        (120, 40),
        (200, 60),
    ] {
        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        term.draw(|f| app.draw(f)).unwrap();
        let text = buf_text(&term);
        // The three rungs of the ladder: `PixelSize::Full` (`████`), the
        // quadrant mid form (`█` plus half blocks), or
        // the bold text arm. The sextant marker (`🬂`) is gone with the form.
        assert!(
            text.contains("24 - 21") || text.contains('█'),
            "{w}x{h} must still show a score in some form:\n{text}"
        );
    }
}

#[test]
fn in_tv_only_the_shown_game_and_my_teams_take_the_screen() {
    // TV mode no longer promotes every scoring play
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
    assert!(
        cut.full,
        "a pinned/favorited game takes the whole screen even unshown"
    );
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
    let cut = app
        .cuts
        .active(app.tick)
        .expect("a delta fires a cut")
        .clone();
    assert!(
        !cut.full,
        "an unrelated game's cut is the quiet band, not a takeover"
    );
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
    // The named edge: TV can be locked (space) onto a
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
    assert_eq!(
        app.tv_shown.as_deref(),
        Some("1"),
        "the lock held tv_shown on 1"
    );

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
    let cut = app
        .cuts
        .active(app.tick)
        .expect("a delta fires a cut")
        .clone();
    assert!(
        !cut.full,
        "an unshown game's cut is the quiet band even when it outranks the locked game"
    );
    assert_eq!(cut.game_id, "2");
}

// ------------------------------------------------------------------- zoom
// The Overview tab is hero + linescore + a per-sport matchup line, which is
// where the mapped-but-never-drawn fields (situation.pitcher /
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
        meter: Some(Meter::Diamond {
            occupied: [true, false, true],
        }),
        last_plays: vec![Play {
            period: "B7".into(),
            team: "BOS".into(),
            text: "Devers singles to right field".into(),
            ..Default::default()
        }],
        linescore: vec![(0, 1), (2, 0), (0, 0), (1, 1), (0, 0), (1, 0), (0, 1)],
        extras: Extras::Baseball {
            hits: Some((8, 7)),
            errors: Some((0, 1)),
        },
        ..Game::default()
    }
}

/// The score glyphs of a rendered frame, cropped to their bounding box, with
/// each cell's fg. Position-independent, so the same score drawn at two
/// different y offsets compares equal.
///
/// The glyph set is the four characters the two score forms actually draw:
/// `PixelSize::Full` paints solid `█`, and the quad table
/// adds `▀ ▄ ▝`. Deliberately NOT the whole U+2580–259F run — the tier rows'
/// hot mark is `▌` and the hero marks are quadrant art, and either would drag
/// non-score cells into the bounding box.
const SCORE_GLYPHS: [&str; 4] = ["█", "▀", "▄", "▝"];

fn digit_grid(term: &Terminal<TestBackend>) -> Vec<Vec<(String, ratatui::style::Color)>> {
    let b = term.backend().buffer();
    let area = b.area();
    let mut cells = Vec::new();
    for y in 0..area.height {
        for x in 0..area.width {
            if SCORE_GLYPHS.contains(&b[(x, y)].symbol()) {
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
    app.view = View::Zoom {
        game_id: game.id.clone(),
        tab: ZoomTab::Overview,
    };
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

    // The matchup line, from fields the mapper has long filled.
    assert!(s.contains("P: G. Kirby"), "pitcher missing:\n{s}");
    assert!(s.contains("AB: R. Devers"), "batter missing:\n{s}");
    assert!(s.contains("DUE UP"), "due up missing:\n{s}");
    assert!(
        s.contains("A. Riley"),
        "the first due-up hitter missing:\n{s}"
    );
    // …and the linescore table.
    assert!(
        s.lines()
            .any(|l| l.contains("SEA") && l.trim_end().ends_with(" 0")),
        "away linescore row (R H E ending in E=0) missing:\n{s}"
    );
    assert!(
        s.contains("LAST PLAYS"),
        "the feed survived the rebuild:\n{s}"
    );

    // The hard rule: one score formatter. The zoom hero IS the board
    // hero, so the digits match cell for cell — chars and colors.
    assert_eq!(
        digit_grid(&zoom),
        board_digits,
        "the zoom hero's digits differ from the board hero's for the same game"
    );

    // …and again at the QUAD rung. 120×40 is `hero_digits_full = true`
    // (layout: width ≥ 100 && height ≥ 32), so the pass above only ever
    // exercises `PixelSize::Full`. Below that bracket the zoom and the board
    // both draw the quadrant form, and that rung has to be pinned too —
    // otherwise a quad-only tweak inside the zoom drifts from `score_block`
    // and this test stays green.
    let mut app = mk();
    app.apply_boards(League::Mlb, vec![game.clone()], false);
    app.tab = Tab::League(League::Mlb);
    let mut board = Terminal::new(TestBackend::new(80, 24)).unwrap();
    board.draw(|f| app.draw(f)).unwrap();
    let board_quad = digit_grid(&board);
    // The rung really is quad here: `PixelSize::Full` is 8 rows tall, the
    // quad table is 4, and nothing else on this frame draws these glyphs.
    assert_eq!(
        board_quad.len(),
        4,
        "80×24 must render the 4-row quad form, got {board_quad:?}"
    );

    zoomed(&mut app, &game);
    let mut zoom = Terminal::new(TestBackend::new(80, 24)).unwrap();
    zoom.draw(|f| app.draw(f)).unwrap();
    assert_eq!(
        digit_grid(&zoom),
        board_quad,
        "at the quad rung the zoom hero's digits differ from the board hero's:\n{}",
        buf_text(&zoom)
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
    assert_eq!(
        s.matches("KC BALL").count(),
        1,
        "possession said twice:\n{s}"
    );
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
            MatchEvent {
                minute: "12'".into(),
                kind: EventKind::Yellow,
                team: "NEW".into(),
                player: "B. Burn".into(),
                athlete_id: None,
            },
            MatchEvent {
                minute: "24'".into(),
                kind: EventKind::Goal,
                team: "NEW".into(),
                player: "D. Ndoye".into(),
                athlete_id: None,
            },
            MatchEvent {
                minute: "61'".into(),
                kind: EventKind::Yellow,
                team: "NFO".into(),
                player: "O. Aina".into(),
                athlete_id: None,
            },
            MatchEvent {
                minute: "70'".into(),
                kind: EventKind::Penalty,
                team: "NFO".into(),
                player: "M. Gibbs-White".into(),
                athlete_id: None,
            },
        ],
        men: None,
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
    assert!(
        s.contains("70' PEN M. Gibbs-White"),
        "penalty event missing:\n{s}"
    );
    assert!(
        !s.contains("12' Y B. Burn"),
        "only the last three events:\n{s}"
    );
    for emoji in ['⚽', '🟨', '🟥'] {
        assert!(
            !s.contains(emoji),
            "emoji {emoji} in a terminal frame:\n{s}"
        );
    }
}

/// A sending-off is a board-wide state, not a zoom detail —
/// the count comes from the scoreboard every game already has, so the chip
/// rides the tier-1 row for every short-handed match at once.
#[test]
fn the_ten_men_chip_renders_hot() {
    let r = gameday::theme::current().roles();
    let mut game = g("s1", "AVL", "BHA", true);
    game.league = League::Epl;
    game.away_score = 1;
    game.home_score = 1;
    game.period = "63'".into();
    game.clock = String::new();
    game.situation = None;
    game.meter = None;
    game.last_plays = vec![];
    game.extras = Extras::Soccer {
        events: vec![MatchEvent {
            minute: "40'".into(),
            kind: EventKind::Red,
            team: "AVL".into(),
            player: "J. Gomes".into(),
            athlete_id: Some("301524".into()),
        }],
        men: Some((10, 11)),
    };
    // A stoppage-time thriller outranks the sending-off, so the carded match
    // is a tier-1 BOARD ROW, not the hero — which is the whole claim: the
    // count comes off the scoreboard, so every row can carry it.
    let mut thriller = g("s0", "ARS", "LIV", true);
    thriller.league = League::Epl;
    thriller.away_score = 2;
    thriller.home_score = 2;
    thriller.period = "90'+3'".into();
    thriller.clock = String::new();
    thriller.situation = None;
    thriller.meter = None;
    thriller.last_plays = vec![];
    thriller.extras = Extras::Soccer {
        events: vec![],
        men: None,
    };

    let mut app = mk();
    app.apply_boards(League::Epl, vec![game, thriller], false);
    app.tab = Tab::League(League::Epl);
    let mut t = Terminal::new(TestBackend::new(120, 24)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t);
    assert!(
        s.contains("10 MEN"),
        "the short-handed chip is on the board:\n{s}"
    );
    assert!(
        s.contains("STOPPAGE"),
        "and the hero is the other match:\n{s}"
    );

    // Cell level: the chip's own cells are hot, not merely present. The row
    // chip is drawn in the hot foreground (the hero's is filled instead).
    let b = t.backend().buffer();
    // Byte offsets are not columns — the row's left rail is a multi-byte
    // block character, so the chip's cell x has to be counted in chars.
    let (cx, cy) = s
        .lines()
        .enumerate()
        .find_map(|(y, line)| {
            line.find("10 MEN")
                .map(|b| (line[..b].chars().count() as u16, y as u16))
        })
        .expect("chip located");
    for i in 0..6u16 {
        assert_eq!(b[(cx + i, cy)].fg, r.hot, "chip cell {i} is hot\n{s}");
    }
    assert!(
        !s[..s.find("10 MEN").unwrap()].contains("MEN"),
        "one chip, one match:\n{s}"
    );

    // 11 v 11 says nothing.
    let mut quiet = g("s2", "ARS", "LIV", true);
    quiet.league = League::Epl;
    quiet.period = "63'".into();
    quiet.clock = String::new();
    quiet.situation = None;
    quiet.meter = None;
    quiet.last_plays = vec![];
    quiet.extras = Extras::Soccer {
        events: vec![],
        men: None,
    };
    let mut app = mk();
    app.apply_boards(League::Epl, vec![quiet], false);
    app.tab = Tab::League(League::Epl);
    let mut t = Terminal::new(TestBackend::new(120, 24)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    assert!(!buf_text(&t).contains("MEN"), "no chip at full strength");
}

// ── Screen layouts — no screen floats a narrow column in a
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
    // At 120 cols the two conference tables sit side by side
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
    assert!(
        nfc - afc >= 40,
        "columns must be a real split, {afc} vs {nfc}: {row:?}"
    );
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
    assert!(
        n.contains("NATIONAL FOOTBALL CONFERENCE"),
        "stacked, still both groups:\n{n}"
    );
}

#[test]
fn the_plays_feed_fills_the_width() {
    // The feed is a full-width row, not a 60-col column in a
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
        assert_eq!(
            word_col(row),
            word_col(rows[0]),
            "stamp column drifts: {row:?}"
        );
    }
}

#[test]
fn the_re_laid_out_screens_survive_every_size() {
    // Width arithmetic on three more screens (two-column
    // standings, the full-width feed, the two-panel editor), so they take the
    // board's own sweep — and the sweep asserts the layout, not just survival:
    // nothing writes past the frame, the two-column gate flips at exactly 100
    // (receipt: two 48-col tables + a 4-col gutter), and the gutter stays a
    // gutter instead of the right column bleeding into the left one's rule.
    use gameday::views::View;
    // (name, view, header needle, the right column's first word)
    for (name, view, needle, right) in [
        (
            "standings",
            View::Standings(League::Nfl),
            "STANDINGS",
            Some("NATIONAL"),
        ),
        ("plays feed", View::PlaysFeed, "PLAYS", None),
        ("config", View::ConfigView, "CONFIG", Some("FAVORITES")),
    ] {
        for w in [40u16, 55, 60, 80, 99, 100, 101, 120, 180] {
            for h in [12u16, 16, 24, 30, 40, 60] {
                let mut app = mk();
                app.config.enabled_tabs = vec![League::Nfl];
                let mut nfl = g("1", "KC", "TB", true);
                nfl.last_plays = vec![Play {
                    clock: "1:27".into(),
                    team: "KC".into(),
                    text: "Mahomes to Kelce, 12 yd".into(),
                    scoring: true,
                    ..Default::default()
                }];
                app.apply_boards(League::Nfl, vec![with_scoring(nfl)], false);
                // The two-group fixture, not the tall one: the gate is about
                // width, and this table fits every height on the ladder.
                app.merge_standings(standings_table());
                app.view = view.clone();
                let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
                t.draw(|f| app.draw(f)).unwrap();
                let s = buf_text(&t);
                let at = format!("{name} at {w}x{h}");
                assert!(s.contains(needle), "{at} lost its header:\n{s}");
                // Nothing renders past the frame's last column.
                let widest = s
                    .lines()
                    .map(|l| l.trim_end().chars().count())
                    .max()
                    .unwrap();
                assert!(
                    widest <= w as usize,
                    "{at} wrote to column {widest} of {w}:\n{s}"
                );

                match right {
                    // The gate: two columns at 100 and up, one below it.
                    Some(right) => {
                        let paired = s.lines().find(|l| {
                            l.contains(right)
                                && l.contains(if name == "standings" {
                                    "AMERICAN"
                                } else {
                                    "TABS"
                                })
                        });
                        if w >= 100 {
                            let row =
                                paired.unwrap_or_else(|| panic!("{at} must be two columns:\n{s}"));
                            // The gutter is real: the left column's rule stops
                            // before the right column's title.
                            let cut = row.find(right).unwrap();
                            assert!(
                                row[..cut].ends_with("  "),
                                "{at} has no gutter before {right}: {row:?}"
                            );
                        } else {
                            assert!(paired.is_none(), "{at} must be one column:\n{s}");
                        }
                    }
                    // The feed has no gate — every row reaches the frame's
                    // edge at every width (flush-right score, or clipped text).
                    None => {
                        let row = s
                            .lines()
                            .find(|l| l.contains("TOUCHDOWN"))
                            .unwrap_or_else(|| panic!("{at} lost its play row:\n{s}"));
                        let end = row.trim_end().chars().count();
                        assert!(
                            end + 3 >= w as usize,
                            "{at} row ends at {end} of {w}: {row:?}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn a_resize_that_shortens_the_standings_does_not_swallow_a_keypress() {
    // The renderer clamps `standings_scroll` and writes it back: scrolled to
    // the bottom of a one-column 80x14 table, then redrawn at 120x40 where two
    // columns make the same table fit, the stored offset is 0 — so the next j
    // moves the table instead of being spent snapping the stale offset back.
    use crossterm::event::{KeyCode, KeyModifiers};
    use gameday::views::View;
    let mut app = mk();
    app.view = View::Standings(League::Nfl);
    app.merge_standings(tall_standings_table());
    let mut small = Terminal::new(TestBackend::new(80, 14)).unwrap();
    small.draw(|f| app.draw(f)).unwrap();
    for _ in 0..60 {
        gameday::input::handle_key(&mut app, KeyCode::Char('j'), KeyModifiers::NONE);
    }
    small.draw(|f| app.draw(f)).unwrap();
    let bottom = app.standings_scroll;
    assert!(bottom > 0, "the 80-col table must scroll at all: {bottom}");

    let mut wide = Terminal::new(TestBackend::new(120, 40)).unwrap();
    wide.draw(|f| app.draw(f)).unwrap();
    assert_eq!(
        app.standings_scroll, 0,
        "a table that now fits carries no offset"
    );
    let before = buf_text(&wide);
    gameday::input::handle_key(&mut app, KeyCode::Char('k'), KeyModifiers::NONE);
    assert_eq!(app.standings_scroll, 0, "k at the top stays at the top");
    // And the table is drawn from the top, not from the stale offset.
    assert!(
        before.contains("ATEAM00"),
        "the top of the table is on screen:\n{before}"
    );
}

#[test]
fn no_screen_floats_a_dead_column() {
    // At 120x40 the key bar sits with the content it describes
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
    let content: Vec<usize> = (top..bar)
        .filter(|&y| !lines[y].trim().is_empty())
        .collect();
    let last = *content
        .last()
        .unwrap_or_else(|| panic!("config has no content:\n{s}"));
    assert!(
        bar - last <= 2,
        "key bar floats {} rows under the content:\n{s}",
        bar - last
    );
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
        (
            "help",
            Box::new(|a: &mut App| a.help_open = true) as Box<dyn Fn(&mut App)>,
        ),
        (
            "theme picker",
            Box::new(|a: &mut App| a.view = View::ThemePicker),
        ),
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
        assert!(
            l.abs_diff(r) <= 2,
            "{name}: panel off center: left {l}, right {r}\n{s}"
        );
    }
}

/// `Situation::drive_desc` is mapped and reaches the render
/// path — it is *available* to a fragment line, and deliberately does not
/// change one yet. Two boards that differ only in that field must draw the
/// same buffer: a field arriving in the model is not a layout change, and
/// this test is the guard that says so out loud (if a later polish pass
/// spends a row on the drive line, this is the test that fails first, and
/// updating it is the decision to spend that row).
#[test]
fn the_drive_description_is_available_without_moving_the_board() {
    let text_at = |drive: Option<&str>| -> String {
        let mut app = mk();
        let mut game = g("1", "KC", "TB", true);
        if let Some(sit) = game.situation.as_mut() {
            sit.drive_desc = drive.map(str::to_string);
        }
        app.apply_boards(League::Nfl, vec![game], false);
        let mut t = Terminal::new(TestBackend::new(120, 36)).unwrap();
        t.draw(|f| app.draw(f)).unwrap();
        buf_text(&t)
    };
    let without = text_at(None);
    let with = text_at(Some("1 play, 3 yards, 0:08"));
    assert_eq!(
        with, without,
        "drive_desc must not redesign the fragment yet"
    );
    assert!(
        !with.contains("1 play, 3 yards"),
        "the drive line is data, not a rendered row yet:\n{with}"
    );
}

// ---------------------------------------------------------------- NHL zoom

fn nhl_zoom_game() -> Game {
    Game {
        id: "n1".into(),
        league: League::Nhl,
        away: zoom_team("nhl", "PIT", [0, 0, 0]),
        home: zoom_team("nhl", "WSH", [4, 30, 66]),
        away_score: 2,
        home_score: 3,
        status: Status::Live,
        period: "2ND".into(),
        clock: "15:37".into(),
        // The penalty meter is derived from these extras at the zoom,
        // never stored here — the board and :tv read `meter`.
        meter: None,
        extras: Extras::Hockey {
            strength: HockeyStrength::PowerPlay,
            penalties: vec![PenaltyEvent {
                team: "WSH".into(),
                minutes: 2,
                kind: "Minor".into(),
                period: 2,
                clock: "15:37".into(),
            }],
        },
        last_plays: vec![Play {
            period: "2ND".into(),
            team: "PIT".into(),
            text: "Sidney Crosby Slap Shot saved by Logan Thompson".into(),
            ..Default::default()
        }],
        ..Game::default()
    }
}

#[test]
fn the_zoom_shows_the_penalty_meter_and_pp_chip() {
    let game = nhl_zoom_game();
    let mut app = mk();
    app.apply_boards(League::Nhl, vec![game.clone()], false);
    app.tab = Tab::League(League::Nhl);
    zoomed(&mut app, &game);
    let mut t = Terminal::new(TestBackend::new(120, 40)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let b = t.backend().buffer();
    let hot = gameday::theme::current().roles().hot;

    // The chip, cell-level: the hero's chip is the frame's only hot-filled
    // run, and it says POWER PLAY (strength is structural).
    let mut chip = String::new();
    for y in 0..b.area().height {
        for x in 0..b.area().width {
            if b[(x, y)].bg == hot {
                chip.push_str(b[(x, y)].symbol());
            }
        }
    }
    assert_eq!(
        chip.trim(),
        "POWER PLAY",
        "the hot-filled chip cells: {chip:?}"
    );

    // The penalty meter row: label, the penalized team, and a countdown bar.
    let row = (0..b.area().height)
        .find(|y| {
            (0..b.area().width)
                .map(|x| b[(x, *y)].symbol())
                .collect::<String>()
                .contains("PENALTY")
        })
        .expect("the Penalty meter must render a row");
    let cells: String = (0..b.area().width).map(|x| b[(x, row)].symbol()).collect();
    assert!(cells.contains("PENALTY  WSH"), "meter row: {cells:?}");
    assert!(
        cells.contains('▮'),
        "the countdown bar draws filled cells: {cells:?}"
    );
    assert!(cells.contains("2:00"), "a minor's clock: {cells:?}");
}

#[test]
fn the_board_never_wears_the_zoomed_games_power_play() {
    // Strength reaches only the zoomed game, so scoring it would give
    // that one row a chip, the hot flag and a rank bonus no identical
    // unzoomed power play could earn. Same game, two surfaces.
    let game = nhl_zoom_game();
    let mut app = mk();
    app.apply_boards(League::Nhl, vec![game.clone()], false);
    app.tab = Tab::League(League::Nhl);

    let hot = gameday::theme::current().roles().hot;
    let render = |app: &mut App| -> (String, String) {
        let mut t = Terminal::new(TestBackend::new(120, 40)).unwrap();
        t.draw(|f| app.draw(f)).unwrap();
        let b = t.backend().buffer();
        let mut chip = String::new();
        for y in 0..b.area().height {
            for x in 0..b.area().width {
                if b[(x, y)].bg == hot {
                    chip.push_str(b[(x, y)].symbol());
                }
            }
        }
        (chip, buf_text(&t))
    };

    let (chip, text) = render(&mut app);
    assert_eq!(chip.trim(), "", "no hot-filled chip on the board: {chip:?}");
    assert!(
        !text.contains("PENALTY"),
        "and no penalty meter row either:\n{text}"
    );

    zoomed(&mut app, &game);
    let (chip, text) = render(&mut app);
    assert_eq!(
        chip.trim(),
        "POWER PLAY",
        "the zoom is where it shows: {chip:?}"
    );
    assert!(text.contains("PENALTY"), "with its meter:\n{text}");
}

#[test]
fn a_college_row_prints_the_field_position_once_from_the_feed() {
    // Starts from raw ESPN JSON and the real mapper, not a hand-built
    // `Situation` — a hand-built one never exercises `down_distance_from`, so
    // it can't tell a fixed mapper from a reverted one. Event A carries only
    // the long form (`downDistanceText` ending in " at BAY 2", no
    // `shortDownDistanceText`), the shape that used to leak the ball
    // position into `down_distance`; event B is a red-zone game that
    // outranks A's quiet Q2 snap, so A lands in a tier row
    // (`board::rows::situation_summary`, which appends `AT {ball_on}` on its
    // own) rather than the board's hero.
    let json = r#"{"events":[
        {"id":"10","competitions":[{"status":{"displayClock":"7:13","period":2,"type":{"state":"in"}},
          "competitors":[
            {"homeAway":"away","score":"0","team":{"id":"101","abbreviation":"BAY"}},
            {"homeAway":"home","score":"0","team":{"id":"102","abbreviation":"AUB"}}
          ],
          "situation":{"possession":"101","downDistanceText":"1st & 10 at BAY 2","possessionText":"BAY 2"}}]},
        {"id":"11","competitions":[{"status":{"displayClock":"9:05","period":3,"type":{"state":"in"}},
          "competitors":[
            {"homeAway":"away","score":"0","team":{"id":"201","abbreviation":"HOU"}},
            {"homeAway":"home","score":"0","team":{"id":"202","abbreviation":"TEN"}}
          ],
          "situation":{"possession":"201","isRedZone":true,"yardLine":15}}]}
    ]}"#;
    let games =
        gameday::provider::map::map_scoreboard(League::Cfb, json, time::UtcOffset::UTC).unwrap();
    let mut app = mk();
    app.apply_boards(League::Cfb, games, false);
    let mut t = Terminal::new(TestBackend::new(140, 40)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let s = buf_text(&t);
    assert!(s.contains("BAY 1ST & 10 AT BAY 2"), "{s}");
    assert!(
        !s.contains("AT BAY 2 AT BAY 2"),
        "the field position printed twice:\n{s}"
    );
}

/// Round-1 fix regression: a section that exists (its content or its own
/// "no plays/scoring yet" line) gets its caption only when it also gets at
/// least one row of body under it — never a caption over a void, and never
/// a footer row silently wearing what was meant to be SCORING's content.
#[test]
fn a_short_zoom_never_captions_a_void() {
    use gameday::views::{View, ZoomTab};
    let mut game = g("1", "KC", "TB", true);
    game.last_plays = vec![];
    game.scoring_plays = (0..10u16)
        .map(|i| Play {
            id: format!("s{i}"),
            clock: format!("{}:{:02}", 14 - i / 4, 59 - i),
            period: "Q1".into(),
            team: "KC".into(),
            text: format!("score {i}"),
            scoring: true,
            ..Default::default()
        })
        .collect();
    let mut app = mk();
    app.apply_boards(League::Nfl, vec![game], false);
    app.tab = Tab::League(League::Nfl);
    app.view = View::Zoom {
        game_id: "1".into(),
        tab: ZoomTab::Overview,
    };
    for h in [19u16, 20, 21, 22] {
        let term = render(&mut app, 120, h);
        let s = buf_text(&term);
        let lines: Vec<&str> = s.lines().collect();
        let footer = h as usize - 1;
        for caption in ["LAST PLAYS", "SCORING"] {
            let Some(y) = lines.iter().position(|l| l.contains(caption)) else {
                continue;
            };
            assert_ne!(
                y, footer,
                "{caption} itself must never land on the footer row at {h}:\n{s}"
            );
            assert!(
                !lines[y + 1].trim().is_empty(),
                "{caption} captions a void at {h}:\n{s}"
            );
            assert_ne!(
                y + 1,
                footer,
                "{caption}'s body row must not be the footer at {h}:\n{s}"
            );
        }
        assert!(
            !lines[footer].contains("SCORING"),
            "SCORING must never spill onto the footer row at {h}:\n{s}"
        );
    }
}

/// U8: the IN PLAY rule drew over nothing when the hero absorbed the only
/// live game. A section rule needs a row under it.
#[test]
fn the_in_play_rule_needs_a_row_under_it() {
    let mut app = board_app(1, 1, 1);
    let s = buf_text(&render(&mut app, 120, 40));
    assert!(
        !s.contains("IN PLAY"),
        "one live game is the hero, not a section:\n{s}"
    );
    assert!(s.contains("FINAL") && s.contains("LATER"), "{s}");
    let mut app = board_app(2, 0, 0);
    let s = buf_text(&render(&mut app, 120, 40));
    assert!(
        s.contains("IN PLAY"),
        "a second live game is a section:\n{s}"
    );
}

/// U9: `0-0 ▌ISU` — the `▌` on the hero record line was the lookalike-color
/// swatch, not a possession mark. Gone; the record meets the abbr.
#[test]
fn the_hero_nameplate_carries_no_color_swatch() {
    let mut game = g("1", "KC", "TB", true);
    game.home.color = game.away.color; // the lookalike rule "fells" the home color
    game.home.record = "1-0".into();
    game.away.record = "1-0".into();
    let mut app = mk();
    app.apply_boards(League::Nfl, vec![game], false);
    app.tab = Tab::League(League::Nfl);
    let s = buf_text(&render(&mut app, 120, 40));
    assert!(!s.contains("▌TB"), "swatch beside the home abbr:\n{s}");
    assert!(s.contains("1-0 TB"), "record then abbr, one space:\n{s}");
}

/// U4: `c` cycled themes silently while `:theme` opened a picker.
#[test]
fn c_opens_the_theme_picker() {
    use gameday::views::View;
    let mut app = mk();
    app.apply_boards(League::Nfl, vec![g("1", "KC", "TB", true)], false);
    app.tab = Tab::League(League::Nfl);
    key(&mut app, crossterm::event::KeyCode::Char('c'));
    assert_eq!(app.view, View::ThemePicker);
    let s = buf_text(&render(&mut app, 120, 40));
    assert!(
        s.contains("broadcast") && s.contains("studio"),
        "picker over the board:\n{s}"
    );
    assert!(
        app.status_line.is_none(),
        "no 'theme X' toast: nothing changed yet"
    );
}
