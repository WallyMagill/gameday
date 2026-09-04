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
    /// AP/coaches rank for college sports (`competitors[].curatedRank.current`,
    /// verified `14` for USC 2026-08-31). None for pro leagues and unranked teams.
    pub rank: Option<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Play {
    pub clock: String,
    /// Period label for sports without a play clock: baseball `B9`/`T7`;
    /// empty when the clock carries the moment. Renders where `[-:--]` did.
    pub period: String,
    /// Abbr of the team credited with the play. Empty when unknown.
    pub team: String,
    pub text: String,
    pub scoring: bool,
    /// Structural play kind from the ESPN id tables (`provider::kinds`).
    /// Defaults to `Other` for every legacy/demo/sim constructor — Task 3
    /// wires the mapper to populate this from real feed ids.
    pub kind: PlayKind,
    /// Runs/points this play was worth, where the feed says so (MLB pitch
    /// outcomes, NBA/WNBA/CBB shots, NHL goals). None everywhere else.
    pub score_value: Option<u8>,
}

/// Structural classification of a play, derived from ESPN's per-league type
/// ids (see `provider::kinds`). Map-time only: an id that doesn't land in a
/// named variant becomes `Other` and carries no payload — no consumer reads
/// raw ids, so there is nothing to retain (YAGNI).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum PlayKind {
    // Football (scoringType.name wins for scoring plays).
    Touchdown,
    FieldGoal,
    Safety,
    // MLB (pitch-type id 28 == HomeRun; any other pitch-kind with
    // score_value > 0 == RunScoringPlay).
    HomeRun,
    RunScoringPlay,
    // Soccer + NHL goal all collapse to Goal.
    Goal,
    OwnGoal,
    PenaltyGoal,
    // Soccer cards.
    YellowCard,
    RedCard,
    // NHL penalty plays (meta lives in Extras::Hockey, Task 7).
    HockeyPenalty,
    // Hoops, DERIVED: scoringPlay && score_value == Some(3). Not
    // shootingPlay: CBB's endpoint stamps scoreValue on missed threes too
    // (v3.4 T3 review: CBB stamps scoreValue on misses), so shootingPlay
    // alone would tag a miss as a make.
    ThreePointer,
    /// Everything unmapped.
    #[default]
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Situation {
    /// Headline situation text: football "1st & Goal", baseball "2 OUT · 1-2".
    pub down_distance: String,
    pub possession: Option<String>,
    pub ball_on: Option<String>,
    // Football-only structure, straight off `competition.situation` (spec
    // v3.4 §3): the numbers ESPN already computed, never re-derived from
    // `downDistanceText`/`possessionText`. None for every other sport, and
    // for a football feed that doesn't send them (pre/final games).
    pub down: Option<u8>,
    pub distance: Option<u8>,
    /// Absolute field coordinate, 0..=100, measured from the HOME team's own
    /// goal line — 0 is the home goal, 100 the away goal (verified against
    /// `fixtures/live/cfb_scoreboard_live.json`: UAPB on its own 25 with
    /// MIZ at home maps to `yardLine: 75`). Yards-to-goal for the possessing
    /// team is therefore `100 - yard_line` when home has the ball and
    /// `yard_line` when away does.
    pub yard_line: Option<u8>,
    /// ESPN's own `isRedZone` — the single source for the RED ZONE chip and
    /// meter. `None` means the feed didn't say, which is not "no".
    pub is_red_zone: Option<bool>,
    /// `situation.lastPlay.drive.description` — "1 play, 3 yards, 0:08".
    pub drive_desc: Option<String>,
    // Baseball-only fields (None for every other sport).
    pub balls: Option<u8>,
    pub strikes: Option<u8>,
    pub outs: Option<u8>,
    /// Base runners as [first, second, third].
    pub on_base: Option<[bool; 3]>,
    /// Baseball matchup from `situation.pitcher/.batter` (athlete shortName).
    pub pitcher: Option<String>,
    pub batter: Option<String>,
    /// Baseball `situation.dueUp[]` as "A. Riley (2-3, HR)" strings, in order.
    pub due_up: Vec<String>,
    /// Basketball shot clock in seconds. ESPN's public scoreboard doesn't
    /// carry one (checked 2026-08-29: wnba fixture + live NBA/WNBA feeds), so
    /// on real data this stays None and the chip simply doesn't render;
    /// `--demo` supplies it. Never synthesized for live games.
    pub shot_clock: Option<u8>,
}

impl Situation {
    /// Baseball headline, reference-board style: "2 OUT · 1-2" (outs first,
    /// then balls-strikes). `OUT` never pluralizes — the column stays the
    /// same width at every out count — and the `·` is what keeps the count
    /// from reading as a score. None unless all three are known.
    pub fn mlb_count_headline(&self) -> Option<String> {
        let (o, b, s) = (self.outs?, self.balls?, self.strikes?);
        Some(format!("{o} OUT · {b}-{s}"))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Meter {
    RedZone { yards_to_goal: u8 },
    Lead { plus_minus: i16 },
    Diamond { occupied: [bool; 3] },
    Penalty { team_abbr: String, seconds: u16 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventKind { Goal, OwnGoal, Penalty, Yellow, Red, Sub }

/// One soccer match event from the scoreboard's `competition.details[]`
/// (goals, cards, substitutions), verified in fixtures/epl_scoreboard.json.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MatchEvent {
    pub minute: String,
    pub kind: EventKind,
    pub team: String,
    pub player: String,
}

/// NHL `plays[].strength.id`, the ids the v3.4 research probe pinned: 701
/// Even Strength, 702 Power Play, 703 Shorthanded, 903 Empty Net. Every play
/// of a live summary carries one, so the game's current strength is simply
/// the most recent play's.
///
/// Read it as relative to the play's own team, not to the home side: in
/// `fixtures/live/nhl_summary_final_full.json` WSH takes a minor at 10:48
/// and the next four plays are PIT's, stamped 702 — while WSH's own plays in
/// the same window are stamped 703. One situation, two spellings, depending
/// on who acted. Nothing here names an advantaged team on its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HockeyStrength {
    Even,
    PowerPlay,
    Shorthanded,
    EmptyNet,
}

/// One NHL penalty, off a play whose `type` carries `penaltyMinutes`.
/// `team` is the penalized side's abbreviation (the side that will serve it),
/// `kind` ESPN's own `penaltyType` word — "Minor", "Major".
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PenaltyEvent {
    pub team: String,
    pub minutes: u8,
    pub kind: String,
    pub period: u8,
    pub clock: String,
}

/// Per-sport facts that don't fit the shared fields. One variant per sport
/// family; `None` for sports with nothing extra yet.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum Extras {
    #[default]
    None,
    Baseball { hits: Option<(u16, u16)>, errors: Option<(u16, u16)> },
    Soccer { events: Vec<MatchEvent> },
    /// NHL, from the summary's play list (spec v3.4 §4): the current
    /// strength and every penalty called so far, oldest first.
    Hockey { strength: HockeyStrength, penalties: Vec<PenaltyEvent> },
}

impl Extras {
    /// `Meter::Penalty` for a special-teams situation the feed itself
    /// declares. Derived on demand rather than stored on [`Game`]: only the
    /// zoomed game has a summary, so a stored meter would light a chip and a
    /// meter row on that one board row while an identical unzoomed power
    /// play showed nothing (R49). The zoom calls this; the board and `:tv`
    /// read `game.meter`, so they cannot see it.
    ///
    /// Gate: the current strength is PowerPlay or Shorthanded — ESPN
    /// stamping either on the newest play IS the statement that a penalty is
    /// being served right now, and it is the only expiry signal the payload
    /// has. (Receipt: the fixture's game-ending play is 701, so a finished
    /// game yields no meter.) The team named is the penalized one — the side
    /// serving it, which is what the meter's label reads as.
    ///
    /// `seconds` is the penalty's nominal length, `minutes × 60`, NOT time
    /// remaining. Elapsed math would need the live game clock minus the
    /// penalty's clock, and the only NHL fixture we have is a final (no live
    /// clock) whose per-period clock counts *up* — a direction no live NHL
    /// capture exists to confirm. A static, honest 2:00 beats arithmetic we
    /// cannot check; the countdown becomes real when a live NHL fixture
    /// lands.
    ///
    /// Known gap, deliberately not coded around: a 903 (empty net) stamp on
    /// the newest play during a 6-on-4 drops the chip and the meter until
    /// the next 702/703 play. Keeping the meter alive through it would mean
    /// deciding the penalty is "still running", which is exactly the
    /// elapsed-clock arithmetic this refuses to guess — and widening the
    /// gate to 903 would resurrect a first-period minor under a late-game
    /// empty net, since the penalty list is the whole game's. Rare,
    /// self-correcting on the next play, and a blank beats a wrong clock.
    pub fn penalty_meter(&self) -> Option<Meter> {
        let Extras::Hockey { strength, penalties } = self else { return None };
        if !matches!(strength, HockeyStrength::PowerPlay | HockeyStrength::Shorthanded) {
            return None;
        }
        let newest = penalties.last()?;
        Some(Meter::Penalty {
            team_abbr: newest.team.clone(),
            seconds: u16::from(newest.minutes) * 60,
        })
    }
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
    /// Scheduled start, already in the user's local offset (mapper applies
    /// `text::local_time`). Rendered through `text::fmt_start` — never raw.
    pub start: Option<time::OffsetDateTime>,
    /// Scoring plays, oldest first. Filled two ways: a score delta between
    /// scoreboard polls captures that poll's `lastPlay` (every game); the
    /// summary's full list replaces it for the zoomed game.
    pub scoring_plays: Vec<Play>,
    /// Per period/inning (away, home) from `competitors[].linescores[]`.
    pub linescore: Vec<(u16, u16)>,
    /// (away, home) timeouts remaining, football/basketball only.
    pub timeouts: Option<(u8, u8)>,
    pub extras: Extras,
    pub broadcast: Option<String>,
    /// Pre-game betting line, already formatted for display
    /// ("KC -3.5  O/U 47.5"). None when the feed carries no odds — ESPN
    /// strips them once a game goes final.
    pub odds: Option<String>,
}

impl Default for Game {
    fn default() -> Self {
        Game {
            id: String::new(),
            league: League::Nfl,
            home: Team::default(),
            away: Team::default(),
            home_score: 0,
            away_score: 0,
            status: Status::Pre,
            period: String::new(),
            clock: String::new(),
            situation: None,
            last_plays: vec![],
            meter: None,
            start: None,
            broadcast: None,
            odds: None,
            scoring_plays: vec![],
            linescore: vec![],
            timeouts: None,
            extras: Extras::None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Summary {
    pub last_plays: Vec<Play>,
    pub scoring_plays: Vec<Play>,
    pub meter: Option<Meter>,
    /// Per-sport facts only the summary carries. NHL fills it (spec v3.4 §4:
    /// strength + penalties); every other league leaves it `None` and the
    /// merge keeps whatever the scoreboard already put on the game.
    pub extras: Extras,
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

/// One team's line in a standings group: "BUF  Bills  3  0  0".
/// `third` is the sport's third record column — ties for football, overtime
/// losses for hockey — labeled by `third_label` ("T"/"OTL"); None when the
/// feed carries neither (basketball, baseball).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct StandingRow {
    pub abbr: String,
    pub name: String,
    pub wins: u32,
    pub losses: u32,
    pub third: Option<u32>,
    pub third_label: &'static str,
}

/// One conference/division block of a standings table.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct StandingsGroup {
    pub name: String,
    pub rows: Vec<StandingRow>,
}

/// One league's standings, mapped from ESPN's standings endpoint. Groups are
/// whatever the feed sends (conferences for NFL/NHL/NBA, conference ·
/// division when the feed nests divisions under them).
///
/// `season` is the feed's own season label ("2025-26") when it carries one —
/// the view prints it so an out-of-season table never reads as this season's.
/// `fetched_at` is when *we* took the snapshot, stamped in
/// `App::merge_standings`; the view falls back to it ("updated 9:41 PM") when
/// the feed gave no season.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StandingsTable {
    pub league: League,
    pub season: Option<String>,
    pub groups: Vec<StandingsGroup>,
    pub fetched_at: Option<time::OffsetDateTime>,
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
            ..Default::default()
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
        assert_eq!(sit(1, 2, 2).mlb_count_headline().as_deref(), Some("2 OUT · 1-2"));
        assert_eq!(sit(3, 2, 1).mlb_count_headline().as_deref(), Some("1 OUT · 3-2"));
        assert_eq!(sit(0, 0, 0).mlb_count_headline().as_deref(), Some("0 OUT · 0-0"));
        // Any missing component: no headline rather than a half-made one.
        let partial = Situation { balls: Some(1), strikes: Some(2), ..Default::default() };
        assert_eq!(partial.mlb_count_headline(), None);
        assert_eq!(Situation::default().mlb_count_headline(), None);
    }

    #[test]
    fn game_default_is_an_empty_pregame() {
        let g = Game::default();
        assert_eq!(g.status, Status::Pre);
        assert_eq!(g.league, League::Nfl);
        assert!(g.scoring_plays.is_empty() && g.linescore.is_empty() && g.last_plays.is_empty());
        assert_eq!(g.start, None);
        assert_eq!(g.timeouts, None);
        assert_eq!(g.extras, Extras::None);
        assert_eq!(g.away.rank, None);
        // Literal sites use `..Game::default()`; a play carries its period.
        let p = Play { period: "B9".into(), ..Default::default() };
        assert_eq!(p.period, "B9");
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
                ..Default::default()
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
                ..Default::default()
            }],
            meter: Some(Meter::RedZone { yards_to_goal: 3 }),
            broadcast: Some("CBS".into()),
            ..Game::default()
        };
        assert_eq!(g.status, Status::Live);
        assert_eq!(g.meter, Some(Meter::RedZone { yards_to_goal: 3 }));
        assert_eq!(g.away.logo_key, "nfl/kc");
    }
}
