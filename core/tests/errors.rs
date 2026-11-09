//! Error codes: the journal and replay codes of README §6.3 and the level codes of README §5.

use jiro_core::Error;

const ERRORS: [Error; 35] = [
    Error::Truncated,
    Error::TrailingBytes,
    Error::Version,
    Error::RulesVersion,
    Error::Mode,
    Error::LevelIndex,
    Error::EventCount,
    Error::TickLimit,
    Error::TickOrder,
    Error::ReservedBit,
    Error::UnchangedMask,
    Error::LevelId,
    Error::EventPastTimeLimit,
    Error::EventAfterEnd,
    Error::Invariant,
    Error::LevelLength,
    Error::LevelVersion,
    Error::BorderObstacle,
    Error::BossSize,
    Error::SpawnOrder,
    Error::SpawnBounds,
    Error::BossPeriod,
    Error::MinionCount,
    Error::MinionSpawn,
    Error::MinionSlot,
    Error::MinionPeriod,
    Error::RespawnDelay,
    Error::TimeLimit,
    Error::Lives,
    Error::Target,
    Error::OpenAreaSplit,
    Error::OpenAreaSmall,
    Error::BossSpawn,
    Error::MinionSpawnRail,
    Error::PlayerStartRail,
];

/// The codes of README §6.3 and §5. The match is exhaustive, so a new error fails to compile
/// here until it gets its code; add it to `ERRORS` as well.
fn readme_code(error: Error) -> u8 {
    match error {
        Error::Truncated => 1,
        Error::TrailingBytes => 2,
        Error::Version => 3,
        Error::RulesVersion => 4,
        Error::Mode => 5,
        Error::LevelIndex => 6,
        Error::EventCount => 7,
        Error::TickLimit => 8,
        Error::TickOrder => 9,
        Error::ReservedBit => 10,
        Error::UnchangedMask => 11,
        Error::LevelId => 12,
        Error::EventPastTimeLimit => 13,
        Error::EventAfterEnd => 14,
        Error::Invariant => 15,
        Error::LevelLength => 20,
        Error::LevelVersion => 21,
        Error::BorderObstacle => 22,
        Error::BossSize => 23,
        Error::SpawnOrder => 24,
        Error::SpawnBounds => 25,
        Error::BossPeriod => 26,
        Error::MinionCount => 27,
        Error::MinionSpawn => 28,
        Error::MinionSlot => 29,
        Error::MinionPeriod => 30,
        Error::RespawnDelay => 31,
        Error::TimeLimit => 32,
        Error::Lives => 33,
        Error::Target => 34,
        Error::OpenAreaSplit => 35,
        Error::OpenAreaSmall => 36,
        Error::BossSpawn => 37,
        Error::MinionSpawnRail => 38,
        Error::PlayerStartRail => 39,
    }
}

#[test]
fn error_codes_are_the_readme_codes() {
    for error in ERRORS {
        assert_eq!(error.code(), readme_code(error), "{error:?}");
    }
}

/// The compiler already rejects duplicate discriminants; this guards `Error::code` if it
/// ever stops returning the discriminant.
#[test]
fn error_codes_are_unique() {
    let mut codes: Vec<u8> = ERRORS.iter().map(|error| error.code()).collect();
    codes.sort_unstable();
    codes.dedup();
    assert_eq!(codes.len(), ERRORS.len());
}
