#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum League { Nfl, Cfb, Cbb, Nba, Wnba, Nhl, Mlb, Epl, Mls }

impl League {
    pub const ALL: [League; 9] = [
        League::Nfl, League::Cfb, League::Cbb, League::Nba, League::Wnba,
        League::Nhl, League::Mlb, League::Epl, League::Mls,
    ];

    /// ESPN URL parts: (sport, competition slug). Soccer competitions use
    /// ESPN's league codes ("eng.1", "usa.1") rather than a name slug.
    pub fn espn_path(self) -> (&'static str, &'static str) {
        match self {
            League::Nfl => ("football", "nfl"),
            League::Cfb => ("football", "college-football"),
            League::Cbb => ("basketball", "mens-college-basketball"),
            League::Nba => ("basketball", "nba"),
            League::Wnba => ("basketball", "wnba"),
            League::Nhl => ("hockey", "nhl"),
            League::Mlb => ("baseball", "mlb"),
            League::Epl => ("soccer", "eng.1"),
            League::Mls => ("soccer", "usa.1"),
        }
    }

    pub fn slug(self) -> &'static str {
        match self {
            League::Nfl => "nfl",
            League::Cfb => "cfb",
            League::Cbb => "cbb",
            League::Nba => "nba",
            League::Wnba => "wnba",
            League::Nhl => "nhl",
            League::Mlb => "mlb",
            League::Epl => "epl",
            League::Mls => "mls",
        }
    }

    pub fn from_slug(s: &str) -> Option<League> {
        League::ALL.into_iter().find(|l| l.slug() == s)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status { Pre, Live, Final }

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Team {
    pub id: String,
    pub abbr: String,
    pub name: String,
    /// City / market ("KANSAS CITY"). Empty when the feed only had a display name.
    pub location: String,
    /// Overall record ("11-6"). Empty when unknown.
    pub record: String,
    pub color: [u8; 3],
    pub alt_color: [u8; 3],
    pub logo_key: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Play {
    pub clock: String,
    /// Abbr of the team credited with the play. Empty when unknown.
    pub team: String,
    pub text: String,
    pub scoring: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Situation {
    /// Headline situation text: football "1st & Goal", baseball "2 OUTS  1-2".
    pub down_distance: String,
    pub possession: Option<String>,
    pub ball_on: Option<String>,
    // Baseball-only fields (None for every other sport).
    pub balls: Option<u8>,
    pub strikes: Option<u8>,
    pub outs: Option<u8>,
    /// Base runners as [first, second, third].
    pub on_base: Option<[bool; 3]>,
    /// Basketball shot clock in seconds. ESPN's public scoreboard doesn't
    /// carry one (checked 2026-08-29: wnba fixture + live NBA/WNBA feeds), so
    /// on real data this stays None and the chip simply doesn't render;
    /// `--demo` supplies it. Never synthesized for live games.
    pub shot_clock: Option<u8>,
}

impl Situation {
    /// Baseball headline, reference-board style: "2 OUTS  1-2"
    /// (outs first, then balls-strikes). None unless all three are known.
    pub fn mlb_count_headline(&self) -> Option<String> {
        let (o, b, s) = (self.outs?, self.balls?, self.strikes?);
        let plural = if o == 1 { "" } else { "S" };
        Some(format!("{o} OUT{plural}  {b}-{s}"))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Meter {
    RedZone { yards_to_goal: u8 },
    Lead { plus_minus: i16 },
    Diamond { occupied: [bool; 3] },
    Penalty { team_abbr: String, seconds: u16 },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Game {
    pub id: String,
    pub league: League,
    pub home: Team,
    pub away: Team,
    pub home_score: u16,
    pub away_score: u16,
    pub status: Status,
    pub period: String,
    pub clock: String,
    pub situation: Option<Situation>,
    pub last_plays: Vec<Play>,
    pub meter: Option<Meter>,
    pub start_time: Option<String>,
    pub broadcast: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Summary {
    pub last_plays: Vec<Play>,
    pub scoring_plays: Vec<Play>,
    pub meter: Option<Meter>,
}

/// One box-score comparison row: "Total Yards  251  277".
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct StatRow {
    pub label: String,
    pub away: String,
    pub home: String,
}

/// One team's statistical leader in one category:
/// team "SEA", label "Passing Yards", text "D. Lock 12/14, 103 YDS, 1 TD".
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Leader {
    pub team: String,
    pub label: String,
    pub text: String,
}

/// Box score for one game, mapped from the summary endpoint's
/// `boxscore.teams[].statistics` + `leaders`. Per-sport row sets — the
/// mapper keeps whatever ESPN sends, it doesn't normalize across leagues.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct GameStats {
    pub rows: Vec<StatRow>,
    pub leaders: Vec<Leader>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chiefs() -> Team {
        Team {
            id: "12".into(),
            abbr: "KC".into(),
            name: "Chiefs".into(),
            location: "Kansas City".into(),
            record: "11-6".into(),
            color: [227, 24, 55],
            alt_color: [255, 184, 28],
            logo_key: "nfl/kc".into(),
        }
    }

    #[test]
    fn nfl_espn_path() {
        assert_eq!(League::Nfl.espn_path(), ("football", "nfl"));
        assert_eq!(League::Cfb.espn_path(), ("football", "college-football"));
        assert_eq!(League::Nba.espn_path(), ("basketball", "nba"));
        assert_eq!(League::Wnba.espn_path(), ("basketball", "wnba"));
        assert_eq!(League::Nhl.espn_path(), ("hockey", "nhl"));
        assert_eq!(League::Cbb.espn_path(), ("basketball", "mens-college-basketball"));
        assert_eq!(League::Mlb.espn_path(), ("baseball", "mlb"));
        assert_eq!(League::Epl.espn_path(), ("soccer", "eng.1"));
        assert_eq!(League::Mls.espn_path(), ("soccer", "usa.1"));
    }

    #[test]
    fn slugs() {
        assert_eq!(League::Nfl.slug(), "nfl");
        assert_eq!(League::Cfb.slug(), "cfb");
        assert_eq!(League::Wnba.slug(), "wnba");
        assert_eq!(League::Epl.slug(), "epl");
        assert_eq!(League::Mls.slug(), "mls");
    }

    #[test]
    fn from_slug_roundtrips_every_league() {
        for l in League::ALL {
            assert_eq!(League::from_slug(l.slug()), Some(l), "slug {}", l.slug());
        }
        assert_eq!(League::from_slug("xfl"), None);
    }

    #[test]
    fn mlb_count_headline_formats_and_pluralizes() {
        let sit = |b, s, o| Situation {
            balls: Some(b),
            strikes: Some(s),
            outs: Some(o),
            ..Default::default()
        };
        assert_eq!(sit(1, 2, 2).mlb_count_headline().as_deref(), Some("2 OUTS  1-2"));
        assert_eq!(sit(3, 2, 1).mlb_count_headline().as_deref(), Some("1 OUT  3-2"));
        assert_eq!(sit(0, 0, 0).mlb_count_headline().as_deref(), Some("0 OUTS  0-0"));
        // Any missing component: no headline rather than a half-made one.
        let partial = Situation { balls: Some(1), strikes: Some(2), ..Default::default() };
        assert_eq!(partial.mlb_count_headline(), None);
        assert_eq!(Situation::default().mlb_count_headline(), None);
    }

    #[test]
    fn live_game_holds_situation_and_plays() {
        let g = Game {
            id: "401".into(),
            league: League::Nfl,
            away: chiefs(),
            home: Team {
                id: "27".into(),
                abbr: "TB".into(),
                name: "Buccaneers".into(),
                location: "Tampa Bay".into(),
                record: "11-6".into(),
                color: [213, 10, 10],
                alt_color: [52, 48, 43],
                logo_key: "nfl/tb".into(),
            },
            away_score: 27,
            home_score: 24,
            status: Status::Live,
            period: "Q4".into(),
            clock: "1:27".into(),
            situation: Some(Situation {
                down_distance: "1st & Goal".into(),
                possession: Some("KC".into()),
                ball_on: Some("TB 3".into()),
                ..Default::default()
            }),
            last_plays: vec![Play {
                clock: "1:27".into(),
                team: "KC".into(),
                text: "Mahomes pass to Kelce for 3 yards".into(),
                scoring: false,
            }],
            meter: Some(Meter::RedZone { yards_to_goal: 3 }),
            start_time: None,
            broadcast: Some("CBS".into()),
        };
        assert_eq!(g.status, Status::Live);
        assert_eq!(g.meter, Some(Meter::RedZone { yards_to_goal: 3 }));
        assert_eq!(g.away.logo_key, "nfl/kc");
    }
}
