use crate::config::{prune_pins, Favorite, Pin};
use crate::domain::Game;
use time::OffsetDateTime;

pub fn home_games<'a>(
    pins: &[Pin],
    favorites: &[Favorite],
    boards: &'a [Game],
    now: OffsetDateTime,
) -> Vec<&'a Game> {
    let mut out: Vec<&'a Game> = Vec::new();
    let surviving = prune_pins(pins.to_vec(), now);

    for pin in &surviving {
        if let Some(g) = boards.iter().find(|g| g.id == pin.game_id) {
            if !out.iter().any(|x| x.id == g.id) {
                out.push(g);
            }
        }
    }

    for fav in favorites {
        for g in boards {
            if g.league != fav.league {
                continue;
            }
            let match_away = g.away.abbr.eq_ignore_ascii_case(&fav.team_abbr);
            let match_home = g.home.abbr.eq_ignore_ascii_case(&fav.team_abbr);
            if !(match_away || match_home) {
                continue;
            }
            if out.iter().any(|x| x.id == g.id) {
                continue;
            }
            out.push(g);
        }
    }

    out
}
