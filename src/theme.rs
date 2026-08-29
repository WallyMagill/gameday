use ratatui::style::Color;

pub const BG: Color = Color::Rgb(18, 18, 18);
pub const FG: Color = Color::Rgb(208, 208, 208);
pub const MUTED: Color = Color::Rgb(110, 110, 110);
pub const AMBER: Color = Color::Rgb(218, 176, 74);
pub const LIVE: Color = Color::Rgb(214, 72, 72);
pub const DIM: Color = Color::Rgb(42, 42, 42);
pub const BORDER: Color = Color::Rgb(70, 70, 70);

pub fn rgb(c: [u8; 3]) -> Color {
    Color::Rgb(c[0], c[1], c[2])
}
