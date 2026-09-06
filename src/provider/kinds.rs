//! Pure lookup tables mapping ESPN's per-league play `type.id` (a string in
//! the feed) to the structural [`PlayKind`](crate::domain::PlayKind). Every
//! function here is a `match` — no allocation, no I/O — because these run
//! once per play per poll. Ids are receipts from the v3.4 research probe
//! (`.superpowers/sdd/2026-09-03-gameday-v3-4-data-truth/`); an id not in a
//! table maps to `Other` rather than guessing.

use crate::domain::{HockeyStrength, PlayKind};

/// NFL + CFB share a play-type id space. `scoring_type` is
/// `scoringType.name` off the play object; when present it wins outright,
/// since ESPN itself already classified the play as a score.
pub fn football_kind(type_id: &str, scoring_type: Option<&str>) -> PlayKind {
    match scoring_type {
        Some("touchdown") => return PlayKind::Touchdown,
        Some("field-goal") => return PlayKind::FieldGoal,
        Some("safety") => return PlayKind::Safety,
        _ => {}
    }
    match type_id {
        "67" => PlayKind::Touchdown, // 67 Passing Touchdown (research §1)
        "68" => PlayKind::Touchdown, // 68 Rushing Touchdown (research §1)
        "59" => PlayKind::FieldGoal, // 59 Field Goal Good (research §1)
        _ => PlayKind::Other,
    }
}

/// NBA/WNBA and CBB use different id spaces *and* different spellings for
/// the same shot, so they get separate tables even though the derivation
/// rule (three-pointer = scoring play worth 3) is shared. `scoring` must be
/// the play's own `scoringPlay` flag, not `shootingPlay`: CBB's endpoint
/// stamps `scoreValue: 3` on a missed three exactly as it does on a made
/// one, so `shootingPlay` alone would tag a miss as a make. NBA/WNBA don't
/// exhibit this, but the gate is correct for both since a make is always
/// `scoringPlay: true`.
pub fn hoops_kind(type_id: &str, scoring: bool, score_value: Option<u8>, cbb: bool) -> PlayKind {
    if scoring && score_value == Some(3) {
        let three = if cbb {
            matches!(type_id, "558") // 558 JumpShot (CBB) (research §2)
        } else {
            matches!(type_id, "92") // 92 Jump Shot (NBA/WNBA) (research §2)
        };
        if three {
            return PlayKind::ThreePointer;
        }
    }
    PlayKind::Other
}

/// MLB pitch-outcome id 28 is always a home run; any other outcome that
/// carries a positive `score_value` (runs batted in on the play) is a
/// generic scoring play. Everything else — including outs — is `Other`.
pub fn mlb_kind(pitch_type_id: &str, score_value: Option<u8>) -> PlayKind {
    match pitch_type_id {
        "28" => PlayKind::HomeRun, // 28 Home Run (research §3)
        _ if score_value.is_some_and(|v| v > 0) => PlayKind::RunScoringPlay,
        _ => PlayKind::Other,
    }
}

/// NHL goal is a single id. Penalties are NOT identified by id here — the
/// id space for penalties is large and unenumerable, so the mapper detects
/// a penalty by the presence of `type.penaltyMinutes` in the payload
/// and passes that fact in separately. `has_penalty_minutes`
/// lets this stay a pure, honest function instead of a partial id list that
/// silently misses new penalty ids.
pub fn nhl_kind(type_id: &str, has_penalty_minutes: bool) -> PlayKind {
    if has_penalty_minutes {
        return PlayKind::HockeyPenalty;
    }
    match type_id {
        "505" => PlayKind::Goal, // 505 Goal (research §4)
        _ => PlayKind::Other,
    }
}

/// NHL `plays[].strength.id` → [`HockeyStrength`]: 701 Even,
/// 702 Power Play, 703 Shorthanded, 903 Empty Net. Every play of a live NHL
/// summary carries one of the four (verified across all 306 plays of
/// `fixtures/live/nhl_summary_final_full.json`: 277×701, 19×702, 9×703,
/// 1×903), so an id outside the table is a shape we have never seen —
/// it reads as Even rather than inventing a fifth state.
pub fn hockey_strength(id: &str) -> HockeyStrength {
    match id {
        "702" => HockeyStrength::PowerPlay,
        "703" => HockeyStrength::Shorthanded,
        "903" => HockeyStrength::EmptyNet,
        _ => HockeyStrength::Even, // 701 Even Strength, and anything unmapped
    }
}

/// Soccer play-type ids, shared across EPL/MLS (both use ESPN's generic
/// soccer feed). 137/173 are the two "headed"/"volleyed" flavors of Goal.
pub fn soccer_kind(type_id: &str) -> PlayKind {
    match type_id {
        "70" | "137" | "173" => PlayKind::Goal, // 70 Goal, 137 Goal-Header, 173 Goal-Volley (research §5)
        "97" => PlayKind::OwnGoal,              // 97 Own Goal (research §5)
        "98" => PlayKind::PenaltyGoal,          // 98 Penalty - Scored (research §5)
        "94" => PlayKind::YellowCard,           // 94 Yellow Card (research §5)
        "93" => PlayKind::RedCard,              // 93 Red Card (research §5)
        _ => PlayKind::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn football_scoring_types_win() {
        assert_eq!(football_kind("67", Some("touchdown")), PlayKind::Touchdown);
        assert_eq!(football_kind("59", Some("field-goal")), PlayKind::FieldGoal);
        assert_eq!(football_kind("5", None), PlayKind::Other); // plain rush
    }

    #[test]
    fn cbb_ids_are_not_nba_ids() {
        // NBA 92 Jump Shot w/ 3 pts → ThreePointer; CBB uses 558 JumpShot:
        assert_eq!(
            hoops_kind("92", true, Some(3), false),
            PlayKind::ThreePointer
        );
        assert_eq!(
            hoops_kind("558", true, Some(3), true),
            PlayKind::ThreePointer
        );
        assert_eq!(hoops_kind("92", true, Some(3), true), PlayKind::Other); // NBA id through CBB table stays honest
    }

    #[test]
    fn cbb_missed_three_stays_other() {
        // CBB's endpoint stamps scoreValue: 3 on a missed three exactly as
        // it does on a make (proved from fixtures/cbb_summary_full.json: 7
        // misses carry scoreValue 3 with scoringPlay false) — scoring: false
        // must gate it out.
        assert_eq!(hoops_kind("558", false, Some(3), true), PlayKind::Other);
    }

    #[test]
    fn mlb_kind_is_homer_or_scoring_or_other() {
        assert_eq!(mlb_kind("28", Some(1)), PlayKind::HomeRun);
        assert_eq!(mlb_kind("35", Some(1)), PlayKind::RunScoringPlay); // sac fly
        assert_eq!(mlb_kind("22", None), PlayKind::Other); // fly out
    }

    #[test]
    fn soccer_and_nhl_tables() {
        assert_eq!(soccer_kind("93"), PlayKind::RedCard);
        assert_eq!(soccer_kind("97"), PlayKind::OwnGoal);
        assert_eq!(nhl_kind("505", false), PlayKind::Goal);
        assert_eq!(nhl_kind("29", true), PlayKind::HockeyPenalty);
        assert_eq!(soccer_kind("9999"), PlayKind::Other);
    }

    #[test]
    fn additional_verified_mlb_ids_stay_honest() {
        // Base hits and ground/fly outs carry no score_value in the feed
        // for a solo, un-driven-in play; RunScoringPlay only fires when the
        // feed itself reports a run scored.
        assert_eq!(mlb_kind("2", None), PlayKind::Other); // 2 Single (research §3)
        assert_eq!(mlb_kind("3", None), PlayKind::Other); // 3 Double (research §3)
        assert_eq!(mlb_kind("35", None), PlayKind::Other); // 35 Sacrifice Fly, no run this play
        assert_eq!(mlb_kind("24", None), PlayKind::Other); // 24 Ground Out (research §3)
        assert_eq!(mlb_kind("26", Some(1)), PlayKind::RunScoringPlay); // 26 Hit By Pitch, forces in a run
    }

    #[test]
    fn nhl_penalty_ids_observed_in_research() {
        assert_eq!(nhl_kind("55", true), PlayKind::HockeyPenalty); // 55 Tripping (research §4)
        assert_eq!(nhl_kind("29", true), PlayKind::HockeyPenalty); // 29 High-sticking (research §4)
    }
}
