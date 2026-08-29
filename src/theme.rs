use crate::domain::League;
use ratatui::style::Color;

// RedZone board palette: near-black ground, warm live red, cool structure.
pub const BG: Color = Color::Rgb(10, 10, 10);
pub const FG: Color = Color::Rgb(200, 200, 200);
pub const BRIGHT: Color = Color::Rgb(235, 235, 235);
pub const MUTED: Color = Color::Rgb(120, 120, 120);
pub const DIM: Color = Color::Rgb(60, 60, 60);
pub const BORDER: Color = Color::Rgb(80, 80, 80);
pub const AMBER: Color = Color::Rgb(230, 180, 60);
pub const LIVE: Color = Color::Rgb(255, 60, 60);
pub const GREEN: Color = Color::Rgb(80, 210, 110);
pub const CYAN: Color = Color::Rgb(70, 200, 220);
pub const MAGENTA: Color = Color::Rgb(220, 100, 220);
pub const STAR: Color = Color::Rgb(240, 200, 70);

pub fn rgb(c: [u8; 3]) -> Color {
    Color::Rgb(c[0], c[1], c[2])
}

/// Accent color for a league's chip, LAST PLAYS label, and meter.
pub fn league_accent(league: League) -> Color {
    match league {
        League::Nfl => Color::Rgb(255, 70, 70),
        League::Cfb => Color::Rgb(255, 150, 60),
        League::Nba => Color::Rgb(80, 140, 255),
        League::Wnba => Color::Rgb(250, 110, 40),
        League::Cbb => Color::Rgb(120, 120, 255),
        League::Mlb => Color::Rgb(230, 200, 60),
        League::Nhl => Color::Rgb(70, 200, 220),
        League::Epl => Color::Rgb(160, 90, 230),
        League::Mls => Color::Rgb(90, 200, 110),
    }
}

/// Word the ticker/alerts use for a scoring play in this league.
pub fn scoring_word(league: League) -> &'static str {
    match league {
        League::Nfl | League::Cfb => "TOUCHDOWN!",
        League::Nba | League::Wnba | League::Cbb => "BUCKET!",
        League::Mlb => "HOME RUN!",
        League::Nhl | League::Epl | League::Mls => "GOAL!",
    }
}
