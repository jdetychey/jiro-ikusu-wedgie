//! Level bytes v1, validity, the level-start grid, the rail and claim_bp (README §4.1, §4.2,
//! §5). Test levels are built in code.

use std::num::NonZeroU16;

use jiro_core::Error;
use jiro_core::consts::{CAPTURE_STACK, CELLS, LEVEL_LEN, PLAYER_START};
use jiro_core::grid::claim_bp;
use jiro_core::level::{Level, LevelStart, Rect, decode, encode, level_id, validate};
use jiro_core::types::{Cell, CellIndex, Pos};

/// The open-field level of README §5's worked example.
fn open_field() -> Level {
    Level {
        obstacles: [0; 450],
        boss_size: 5,
        spawn: Rect {
            x0: 20,
            y0: 20,
            x1: 34,
            y1: 34,
        },
        boss_period: 2,
        minion_count: 0,
        minion_spawns: [Pos { x: 0, y: 0 }; 3],
        minion_period: 0,
        respawn_delay: 0,
        time_limit: 2_700,
        lives: 3,
        target_bp: 7_500,
    }
}

fn with_obstacles(mut level: Level, cells: impl IntoIterator<Item = (u8, u8)>) -> Level {
    for (x, y) in cells {
        let cell = usize::from(y) * 60 + usize::from(x);
        level.obstacles[cell / 8] |= 0x80 >> (cell % 8);
    }
    level
}

fn with_minions(level: Level, spawns: &[(u8, u8)]) -> Level {
    let mut minion_spawns = [Pos { x: 0, y: 0 }; 3];
    for (slot, &(x, y)) in minion_spawns.iter_mut().zip(spawns) {
        *slot = Pos { x, y };
    }
    Level {
        minion_count: spawns.len() as u8,
        minion_spawns,
        minion_period: 1,
        respawn_delay: 300,
        ..level
    }
}

/// Rows 1 to `last_row` filled with obstacles, which leaves the open area below them, and a
/// spawn rectangle there.
fn rows_blocked(last_row: u8) -> Level {
    let rows = (1..=last_row).flat_map(|y| (1..59).map(move |x| (x, y)));
    let level = Level {
        spawn: Rect {
            x0: 20,
            y0: 40,
            x1: 34,
            y1: 45,
        },
        ..open_field()
    };
    with_obstacles(level, rows)
}

fn decode_error(level: &Level) -> Error {
    decode(&encode(level)).unwrap_err()
}

/// Decodes the bytes of `level` and validates it, checking encode(decode(x)) == x on the way.
fn validity(level: &Level) -> Result<LevelStart, Error> {
    let bytes = encode(level);
    let decoded = decode(&bytes)?;
    assert_eq!(encode(&decoded), bytes, "round trip");
    validate(&decoded, &mut [0; CAPTURE_STACK])
}

fn validity_error(level: &Level) -> Error {
    validity(level).unwrap_err()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[test]
fn level_bytes_other_than_472_are_rejected() {
    let bytes = encode(&open_field());
    let mut longer = bytes.to_vec();
    longer.push(0);
    for input in [&[][..], &bytes[..LEVEL_LEN - 1], &longer[..]] {
        assert_eq!(
            decode(input),
            Err(Error::LevelLength),
            "{} bytes",
            input.len()
        );
    }
}

#[test]
fn unknown_level_version_is_rejected() {
    for version in [0, 2, 255] {
        let mut bytes = encode(&open_field());
        bytes[0] = version;
        assert_eq!(
            decode(&bytes),
            Err(Error::LevelVersion),
            "version {version}"
        );
    }
}

#[test]
fn obstacle_on_the_border_is_rejected() {
    let border = [
        (0, 0),
        (59, 0),
        (0, 59),
        (59, 59),
        (30, 0),
        (0, 30),
        (59, 30),
        (30, 59),
    ];
    for cell in border {
        let level = with_obstacles(open_field(), [cell]);
        assert_eq!(decode_error(&level), Error::BorderObstacle, "{cell:?}");
    }
}

#[test]
fn boss_size_outside_2_to_8_is_rejected() {
    for boss_size in [0, 1, 9, 255] {
        let level = Level {
            boss_size,
            ..open_field()
        };
        assert_eq!(decode_error(&level), Error::BossSize, "B = {boss_size}");
    }
}

#[test]
fn spawn_rectangle_out_of_order_is_rejected() {
    let cases = [
        (
            "x0 > x1",
            Rect {
                x0: 21,
                y0: 20,
                x1: 20,
                y1: 34,
            },
        ),
        (
            "y0 > y1",
            Rect {
                x0: 20,
                y0: 21,
                x1: 34,
                y1: 20,
            },
        ),
    ];
    for (name, spawn) in cases {
        let level = Level {
            spawn,
            ..open_field()
        };
        assert_eq!(decode_error(&level), Error::SpawnOrder, "{name}");
    }
}

/// x0 ≥ 1, y0 ≥ 1, x1 ≤ 59 − B and y1 ≤ 59 − B keep the body inside the border.
#[test]
fn spawn_rectangle_reaching_the_border_is_rejected() {
    let cases = [
        (
            "x0 = 0",
            5,
            Rect {
                x0: 0,
                y0: 20,
                x1: 34,
                y1: 34,
            },
        ),
        (
            "y0 = 0",
            5,
            Rect {
                x0: 20,
                y0: 0,
                x1: 34,
                y1: 34,
            },
        ),
        (
            "x1 = 60 - B",
            5,
            Rect {
                x0: 20,
                y0: 20,
                x1: 55,
                y1: 34,
            },
        ),
        (
            "y1 = 60 - B",
            5,
            Rect {
                x0: 20,
                y0: 20,
                x1: 34,
                y1: 55,
            },
        ),
        (
            "x1 = 255, B = 8",
            8,
            Rect {
                x0: 20,
                y0: 20,
                x1: 255,
                y1: 34,
            },
        ),
        (
            "y1 = 255, B = 8",
            8,
            Rect {
                x0: 20,
                y0: 20,
                x1: 34,
                y1: 255,
            },
        ),
    ];
    for (name, boss_size, spawn) in cases {
        let level = Level {
            boss_size,
            spawn,
            ..open_field()
        };
        assert_eq!(decode_error(&level), Error::SpawnBounds, "{name}");
    }
}

#[test]
fn spawn_rectangle_up_to_59_minus_b_is_valid() {
    for boss_size in [2, 5, 8] {
        let far = 59 - boss_size;
        let level = Level {
            boss_size,
            spawn: Rect {
                x0: 1,
                y0: 1,
                x1: far,
                y1: far,
            },
            ..open_field()
        };
        assert!(validity(&level).is_ok(), "B = {boss_size}");
    }
}

#[test]
fn boss_period_0_is_rejected() {
    let level = Level {
        boss_period: 0,
        ..open_field()
    };
    assert_eq!(decode_error(&level), Error::BossPeriod);
}

#[test]
fn minion_count_above_3_is_rejected() {
    for minion_count in [4, 255] {
        let level = Level {
            minion_count,
            ..with_minions(open_field(), &[(0, 0), (59, 0), (0, 59)])
        };
        assert_eq!(
            decode_error(&level),
            Error::MinionCount,
            "{minion_count} minions"
        );
    }
}

#[test]
fn minion_spawn_coordinate_of_60_or_more_is_rejected() {
    for spawn in [(60, 0), (0, 60), (255, 255)] {
        let level = with_minions(open_field(), &[(0, 0), spawn]);
        assert_eq!(decode_error(&level), Error::MinionSpawn, "{spawn:?}");
    }
}

#[test]
fn nonzero_unused_minion_slot_is_rejected() {
    let cases = [
        ("no minions, slot 0 = (1, 0)", 0, 0, Pos { x: 1, y: 0 }),
        ("1 minion, slot 1 = (0, 1)", 1, 1, Pos { x: 0, y: 1 }),
        ("2 minions, slot 2 = (5, 5)", 2, 2, Pos { x: 5, y: 5 }),
        ("no minions, slot 0 = (60, 0)", 0, 0, Pos { x: 60, y: 0 }),
        (
            "1 minion, slot 2 = (255, 255)",
            1,
            2,
            Pos { x: 255, y: 255 },
        ),
    ];
    for (name, minion_count, slot, spawn) in cases {
        let mut level = with_minions(open_field(), &[(0, 0), (59, 0)][..minion_count]);
        level.minion_spawns[slot] = spawn;
        if minion_count == 0 {
            (level.minion_period, level.respawn_delay) = (0, 0);
        }
        assert_eq!(decode_error(&level), Error::MinionSlot, "{name}");
    }
}

#[test]
fn minion_period_follows_the_minion_count() {
    let cases = [
        (
            "period 1, no minions",
            Level {
                minion_period: 1,
                ..open_field()
            },
        ),
        (
            "period 0, 1 minion",
            Level {
                minion_period: 0,
                ..with_minions(open_field(), &[(0, 0)])
            },
        ),
    ];
    for (name, level) in cases {
        assert_eq!(decode_error(&level), Error::MinionPeriod, "{name}");
    }
}

#[test]
fn respawn_delay_follows_the_minion_count() {
    let cases = [
        (
            "R 1, no minions",
            Level {
                respawn_delay: 1,
                ..open_field()
            },
        ),
        (
            "R 256, no minions",
            Level {
                respawn_delay: 256,
                ..open_field()
            },
        ),
        (
            "R 0, 1 minion",
            Level {
                respawn_delay: 0,
                ..with_minions(open_field(), &[(0, 0)])
            },
        ),
    ];
    for (name, level) in cases {
        assert_eq!(decode_error(&level), Error::RespawnDelay, "{name}");
    }
}

#[test]
fn time_limit_outside_1_to_2700_is_rejected() {
    for time_limit in [0, 2_701, u16::MAX] {
        let level = Level {
            time_limit,
            ..open_field()
        };
        assert_eq!(decode_error(&level), Error::TimeLimit, "T = {time_limit}");
    }
}

#[test]
fn zero_lives_is_rejected() {
    let level = Level {
        lives: 0,
        ..open_field()
    };
    assert_eq!(decode_error(&level), Error::Lives);
}

#[test]
fn target_outside_1_to_10000_is_rejected() {
    for target_bp in [0, 10_001, u16::MAX] {
        let level = Level {
            target_bp,
            ..open_field()
        };
        assert_eq!(decode_error(&level), Error::Target, "target {target_bp}");
    }
}

#[test]
fn range_limits_are_valid() {
    let cases = [
        (
            "B = 2",
            Level {
                boss_size: 2,
                ..open_field()
            },
        ),
        (
            "B = 8",
            Level {
                boss_size: 8,
                ..open_field()
            },
        ),
        (
            "T = 1",
            Level {
                time_limit: 1,
                ..open_field()
            },
        ),
        (
            "1 life",
            Level {
                lives: 1,
                ..open_field()
            },
        ),
        (
            "target 1",
            Level {
                target_bp: 1,
                ..open_field()
            },
        ),
        (
            "target 10,000",
            Level {
                target_bp: 10_000,
                ..open_field()
            },
        ),
        (
            "one-cell spawn rectangle",
            Level {
                spawn: Rect {
                    x0: 20,
                    y0: 30,
                    x1: 20,
                    y1: 30,
                },
                ..open_field()
            },
        ),
        (
            "3 minions",
            with_minions(open_field(), &[(0, 0), (59, 0), (30, 0)]),
        ),
    ];
    for (name, level) in cases {
        assert!(validity(&level).is_ok(), "{name}");
    }
}

/// A cup open at the bottom: the search from (1, 1) reaches its inside only by stepping up.
#[test]
fn open_area_entered_only_upward_is_one_region() {
    let cup = (10..=20)
        .map(|x| (x, 5))
        .chain((6..=15).flat_map(|y| [(10, y), (20, y)]));
    let level = with_obstacles(open_field(), cup);
    assert_eq!(validity(&level).unwrap().claimable.get(), 3_364 - 31);
}

#[test]
fn open_area_split_by_a_wall_is_rejected() {
    let level = with_obstacles(open_field(), (1..59).map(|y| (30, y)));
    assert_eq!(validity_error(&level), Error::OpenAreaSplit);
}

/// The open area is 4-connected: two parts that touch only at a corner are two regions.
#[test]
fn diagonal_contact_does_not_connect_the_open_area() {
    let wall = (1..30).map(|y| (30, y)).chain((30..59).map(|y| (31, y)));
    let level = with_obstacles(open_field(), wall);
    assert_eq!(validity_error(&level), Error::OpenAreaSplit);
}

#[test]
fn empty_open_area_is_rejected() {
    let level = rows_blocked(58);
    assert_eq!(validity_error(&level), Error::OpenAreaSplit);
}

#[test]
fn open_area_below_1682_cells_is_rejected() {
    let level = with_obstacles(rows_blocked(29), [(1, 30)]);
    assert_eq!(validity_error(&level), Error::OpenAreaSmall);
}

#[test]
fn open_area_of_1682_cells_is_valid() {
    let start = validity(&rows_blocked(29)).unwrap();
    assert_eq!(start.claimable.get(), 1_682);
}

/// A spawn rectangle that is not square: (10, 20) to (12, 40), so with B = 5 the bodies cover
/// x 10 to 16 and y 20 to 44.
fn tall_spawn() -> Level {
    Level {
        spawn: Rect {
            x0: 10,
            y0: 20,
            x1: 12,
            y1: 40,
        },
        ..open_field()
    }
}

/// The union of the body positions runs from (x0, y0) to (x1 + B − 1, y1 + B − 1).
#[test]
fn boss_body_covering_an_obstacle_is_rejected() {
    let cases = [
        (open_field(), (20, 20)),
        (open_field(), (38, 38)),
        (open_field(), (38, 20)),
        (open_field(), (20, 38)),
        (open_field(), (27, 27)),
        (tall_spawn(), (10, 20)),
        (tall_spawn(), (16, 44)),
        (tall_spawn(), (16, 20)),
        (tall_spawn(), (10, 44)),
    ];
    for (level, cell) in cases {
        let level = with_obstacles(level, [cell]);
        assert_eq!(
            validity_error(&level),
            Error::BossSpawn,
            "{:?} {cell:?}",
            level.spawn
        );
    }
}

#[test]
fn obstacle_just_outside_every_body_position_is_valid() {
    let cases = [
        (open_field(), (19, 20)),
        (open_field(), (20, 19)),
        (open_field(), (39, 38)),
        (open_field(), (38, 39)),
        (tall_spawn(), (9, 30)),
        (tall_spawn(), (17, 30)),
        (tall_spawn(), (12, 19)),
        (tall_spawn(), (12, 45)),
    ];
    for (level, cell) in cases {
        let level = with_obstacles(level, [cell]);
        assert!(validity(&level).is_ok(), "{:?} {cell:?}", level.spawn);
    }
}

#[test]
fn minion_spawn_off_the_rail_is_rejected() {
    let enclosed_border = with_obstacles(open_field(), [(1, 29), (1, 30), (1, 31)]);
    let cases = [
        ("interior cell", with_minions(open_field(), &[(30, 30)])),
        (
            "third slot",
            with_minions(open_field(), &[(0, 0), (59, 0), (30, 30)]),
        ),
        (
            "enclosed border cell",
            with_minions(enclosed_border, &[(0, 30)]),
        ),
    ];
    for (name, level) in cases {
        assert_eq!(validity_error(&level), Error::MinionSpawnRail, "{name}");
    }
}

#[test]
fn player_start_off_the_rail_is_rejected() {
    let level = with_obstacles(open_field(), [(29, 58), (30, 58), (31, 58)]);
    assert_eq!(validity_error(&level), Error::PlayerStartRail);
}

/// Each case breaks two rules; the first in README §5's order wins.
#[test]
fn first_broken_rule_wins() {
    let field = open_field;
    let short_bad_version = {
        let mut bytes = encode(&field()).to_vec();
        bytes[0] = 2;
        bytes.pop();
        bytes
    };
    let mut bad_version_border = encode(&with_obstacles(field(), [(0, 0)]));
    bad_version_border[0] = 2;
    let decode_cases: [(&str, Vec<u8>, Error); 2] = [
        ("length, version", short_bad_version, Error::LevelLength),
        (
            "version, border obstacle",
            bad_version_border.to_vec(),
            Error::LevelVersion,
        ),
    ];
    for (name, bytes, error) in decode_cases {
        assert_eq!(decode(&bytes), Err(error), "{name}");
    }

    let small_and_split = with_obstacles(rows_blocked(29), (30..59).map(|y| (30, y)));
    let small_with_bad_spawn = Level {
        spawn: Rect {
            x0: 20,
            y0: 20,
            x1: 34,
            y1: 34,
        },
        ..with_obstacles(rows_blocked(29), [(1, 30)])
    };
    let blocked_start = [(29, 58), (30, 58), (31, 58)];
    let cases = [
        (
            "border obstacle, B = 1",
            Level {
                boss_size: 1,
                ..with_obstacles(field(), [(0, 0)])
            },
            Error::BorderObstacle,
        ),
        (
            "B = 1, spawn order",
            Level {
                boss_size: 1,
                spawn: Rect {
                    x0: 21,
                    y0: 20,
                    x1: 20,
                    y1: 34,
                },
                ..field()
            },
            Error::BossSize,
        ),
        (
            "spawn order, spawn bounds",
            Level {
                spawn: Rect {
                    x0: 0,
                    y0: 21,
                    x1: 34,
                    y1: 20,
                },
                ..field()
            },
            Error::SpawnOrder,
        ),
        (
            "spawn bounds, boss period",
            Level {
                spawn: Rect {
                    x0: 0,
                    y0: 20,
                    x1: 34,
                    y1: 34,
                },
                boss_period: 0,
                ..field()
            },
            Error::SpawnBounds,
        ),
        (
            "boss period, minion count",
            Level {
                boss_period: 0,
                minion_count: 4,
                ..field()
            },
            Error::BossPeriod,
        ),
        (
            "minion count, minion period",
            Level {
                minion_count: 4,
                ..field()
            },
            Error::MinionCount,
        ),
        (
            "minion count, minion spawn",
            Level {
                minion_count: 4,
                ..with_minions(field(), &[(60, 0), (59, 0), (0, 59)])
            },
            Error::MinionCount,
        ),
        (
            "minion spawn, unused slot",
            {
                let mut level = with_minions(field(), &[(60, 0)]);
                level.minion_spawns[2] = Pos { x: 1, y: 1 };
                level
            },
            Error::MinionSpawn,
        ),
        (
            "unused slot, minion period",
            {
                let mut level = field();
                level.minion_spawns[0] = Pos { x: 1, y: 1 };
                level.minion_period = 1;
                level
            },
            Error::MinionSlot,
        ),
        (
            "minion period, respawn delay",
            Level {
                minion_period: 1,
                respawn_delay: 1,
                ..field()
            },
            Error::MinionPeriod,
        ),
        (
            "respawn delay, time limit",
            Level {
                respawn_delay: 1,
                time_limit: 0,
                ..field()
            },
            Error::RespawnDelay,
        ),
        (
            "time limit, lives",
            Level {
                time_limit: 0,
                lives: 0,
                ..field()
            },
            Error::TimeLimit,
        ),
        (
            "lives, target",
            Level {
                lives: 0,
                target_bp: 0,
                ..field()
            },
            Error::Lives,
        ),
        (
            "target, split open area",
            Level {
                target_bp: 0,
                ..with_obstacles(field(), (1..59).map(|y| (30, y)))
            },
            Error::Target,
        ),
        (
            "split open area, small open area",
            small_and_split,
            Error::OpenAreaSplit,
        ),
        (
            "small open area, boss spawn",
            small_with_bad_spawn,
            Error::OpenAreaSmall,
        ),
        (
            "boss spawn, minion rail",
            with_minions(with_obstacles(field(), [(20, 20)]), &[(30, 30)]),
            Error::BossSpawn,
        ),
        (
            "minion rail, player start",
            with_minions(with_obstacles(field(), blocked_start), &[(30, 30)]),
            Error::MinionSpawnRail,
        ),
    ];
    for (name, level, error) in cases {
        assert_eq!(validity(&level).err(), Some(error), "{name}");
    }
}

/// README §5, the open-field level.
#[test]
fn open_field_worked_example() {
    let bytes = encode(&open_field());
    let obstacles_clear = bytes[1..451].iter().all(|&byte| byte == 0);
    assert_eq!(
        (
            bytes[0],
            obstacles_clear,
            hex(&bytes[451..]),
            hex(&level_id(&bytes)),
            validity(&open_field()).err(),
        ),
        (
            1,
            true,
            "051414222202000000000000000000000a8c031d4c".to_string(),
            "04e24fd698778a8dbcf62ab0c9ed0228d67be3ddd3aeb682f4a67a53410dacc9".to_string(),
            None,
        )
    );
}

/// README §5, the blocked-start level: bitmap byte 438 is 0x07, and validity returns 39.
#[test]
fn blocked_start_worked_example() {
    let level = with_obstacles(open_field(), [(29, 58), (30, 58), (31, 58)]);
    assert_eq!(
        (level.obstacles[438], validity(&level).err()),
        (0x07, Some(Error::PlayerStartRail))
    );
}

/// [R4.1-7]
#[test]
fn level_start_grid_claims_the_border_and_the_obstacles() {
    let level = with_obstacles(open_field(), [(10, 10), (48, 52)]);
    let grid = validity(&level).unwrap().grid;
    let cases = [
        ((0, 0), Cell::Claimed),
        ((59, 0), Cell::Claimed),
        ((0, 59), Cell::Claimed),
        ((59, 59), Cell::Claimed),
        ((30, 59), Cell::Claimed),
        ((10, 10), Cell::Claimed),
        ((48, 52), Cell::Claimed),
        ((1, 1), Cell::Empty),
        ((11, 10), Cell::Empty),
        ((58, 58), Cell::Empty),
    ];
    for ((x, y), cell) in cases {
        assert_eq!(grid.at(Pos { x, y }), Some(cell), "({x}, {y})");
    }
}

#[test]
fn empty_level_has_3364_claimable_cells() {
    let start = validity(&open_field()).unwrap();
    assert_eq!(start.claimable.get(), 3_364);
}

#[test]
fn obstacles_are_not_claimable() {
    let level = with_obstacles(open_field(), [(10, 10), (48, 52), (1, 58)]);
    assert_eq!(validity(&level).unwrap().claimable.get(), 3_361);
}

/// [R4.2-2]
#[test]
fn border_corners_are_rail_cells_on_the_empty_level() {
    let grid = validity(&open_field()).unwrap().grid;
    for (x, y) in [(0, 0), (59, 0), (0, 59), (59, 59)] {
        assert!(grid.is_rail(Pos { x, y }), "({x}, {y})");
    }
}

/// [R4.2-3]
#[test]
fn trail_counts_as_open_for_the_rail() {
    let mut grid = validity(&open_field()).unwrap().grid;
    for (x, y) in [(1, 29), (1, 30)] {
        grid.set(CellIndex::new(Pos { x, y }).unwrap(), Cell::Claimed);
    }
    grid.set(CellIndex::new(Pos { x: 1, y: 31 }).unwrap(), Cell::Trail);
    assert!(grid.is_rail(Pos { x: 0, y: 30 }));
}

#[test]
fn claimed_cell_without_open_neighbors_is_not_rail() {
    let mut grid = validity(&open_field()).unwrap().grid;
    for (x, y) in [(1, 29), (1, 30), (1, 31)] {
        grid.set(CellIndex::new(Pos { x, y }).unwrap(), Cell::Claimed);
    }
    assert!(!grid.is_rail(Pos { x: 0, y: 30 }));
}

/// [R4.2-1]
#[test]
fn only_claimed_cells_inside_the_grid_are_rail() {
    let mut grid = validity(&open_field()).unwrap().grid;
    grid.set(CellIndex::new(Pos { x: 2, y: 2 }).unwrap(), Cell::Trail);
    for (x, y) in [(1, 1), (2, 2), (60, 0), (0, 60)] {
        assert!(!grid.is_rail(Pos { x, y }), "({x}, {y})");
    }
}

/// [R4.2-1] counts the 4 orthogonal neighbors as well as the diagonal ones.
#[test]
fn rail_through_an_orthogonal_neighbor_alone() {
    let mut grid = validity(&open_field()).unwrap().grid;
    let cells = [
        ((30, 0), [(29, 1), (31, 1)]),
        ((0, 30), [(1, 29), (1, 31)]),
        ((59, 30), [(58, 29), (58, 31)]),
        ((30, 59), [(29, 58), (31, 58)]),
    ];
    for (_, diagonals) in cells {
        for (x, y) in diagonals {
            grid.set(CellIndex::new(Pos { x, y }).unwrap(), Cell::Claimed);
        }
    }
    for ((x, y), _) in cells {
        assert!(grid.is_rail(Pos { x, y }), "({x}, {y})");
    }
}

#[test]
fn player_start_with_one_open_neighbor_is_valid() {
    let level = with_obstacles(open_field(), [(29, 58), (31, 58)]);
    assert!(validity(&level).is_ok());
}

#[test]
fn player_start_is_rail_on_the_empty_level() {
    let grid = validity(&open_field()).unwrap().grid;
    assert!(grid.is_rail(PLAYER_START));
}

/// [R4.1-8]
#[test]
fn claim_bp_is_one_floor_division() {
    let claimable = NonZeroU16::new(3_364).unwrap();
    let cases = [
        (3_364, 10_000),
        (1, 2),
        (0, 0),
        (2_523, 7_500),
        (2_522, 7_497),
    ];
    for (captured, expected) in cases {
        assert_eq!(claim_bp(captured, claimable), expected, "{captured} cells");
    }
}

/// `obstacle_bitmap` inverts `obstacle_bits`, at every bit position of every byte.
#[test]
fn the_bitmap_of_a_level_s_obstacle_bits_is_its_obstacles() {
    let level = Level {
        obstacles: core::array::from_fn(|i| (i as u8).wrapping_mul(37)),
        ..open_field()
    };
    let mut cells = [false; CELLS];
    for (cell, bit) in cells.iter_mut().zip(level.obstacle_bits()) {
        *cell = bit;
    }
    assert_eq!(Level::obstacle_bitmap(&cells), level.obstacles);
}
