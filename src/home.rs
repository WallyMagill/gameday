use crate::config::{prune_pins, Favorite, Pin};
use crate::domain::{Game, Status};
use time::OffsetDateTime;

/// Home's order, and the only place it is decided: pinned games (those
/// surviving the prune) first, then favorites' games, then every other live
/// game — each band in `boards` order. Pre and Final games reach Home only
/// through a pin or a favorite.
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

    for g in boards {
        if out.iter().any(|x| x.id == g.id) {
            continue;
        }
        let matched = favorites.iter().any(|fav| {
            fav.league == g.league
                && (g.away.abbr.eq_ignore_ascii_case(&fav.team_abbr)
                    || g.home.abbr.eq_ignore_ascii_case(&fav.team_abbr))
        });
        if matched {
            out.push(g);
        }
    }

    // Everything else, live first: Home is the room with every TV on; pins
    // and favorites only decide which TV is in front. The board is ONE list
    // with FINAL and LATER sections, so the finals and the day's remaining
    // kickoffs belong here too — the board cuts them into sections, this only
    // orders them.
    for want_live in [true, false] {
        for g in boards {
            if (g.status == Status::Live) == want_live && !out.iter().any(|x| x.id == g.id) {
                out.push(g);
            }
        }
    }

    out
}
