//! Minions (README §4.7): the right-hand rule, the orientation step and the lifecycle.

use jiro_core::grid::Grid;
use jiro_core::level::{Level, Rect};
use jiro_core::minion::{Minion, Minions, State, orient, right_hand_step};
use jiro_core::types::{Cell, CellIndex, Dir, Pos};

fn level(minions: &[(u8, u8)], obstacles: &[(u8, u8)]) -> Level {
    let mut level = Level {
        obstacles: [0; 450],
        boss_size: 5,
        spawn: Rect {
            x0: 20,
            y0: 20,
            x1: 34,
            y1: 34,
        },
        boss_period: 2,
        minion_count: minions.len() as u8,
        minion_spawns: [Pos { x: 0, y: 0 }; 3],
        minion_period: 1,
        respawn_delay: 3,
        time_limit: 2_700,
        lives: 3,
        target_bp: 7_500,
    };
    for (slot, &(x, y)) in level.minion_spawns.iter_mut().zip(minions) {
        *slot = Pos { x, y };
    }
    for &(x, y) in obstacles {
        let cell = usize::from(y) * 60 + usize::from(x);
        level.obstacles[cell / 8] |= 0x80 >> (cell % 8);
    }
    level
}

fn at(x: u8, y: u8) -> Pos {
    Pos { x, y }
}

fn paint(grid: &mut Grid, cells: impl IntoIterator<Item = (u8, u8)>, cell: Cell) {
    for (x, y) in cells {
        grid.set(CellIndex::new(at(x, y)).unwrap(), cell);
    }
}

fn live(minions: &Minions, slot: usize) -> (Pos, Dir) {
    match minions.slots[slot].unwrap().state {
        State::Live { pos, heading } => (pos, heading),
        State::Destroyed { countdown } => panic!("slot {slot} is destroyed, countdown {countdown}"),
    }
}

/// [R4.7-1] On the top border, heading E with the field on its right, a minion walks the rail
/// one cell per move.
#[test]
fn minion_walks_the_rail_one_cell_per_move() {
    let grid = Grid::level_start(&level(&[], &[]));
    let (pos, heading) = right_hand_step(&grid, at(10, 0), Dir::Right);
    assert_eq!((pos, heading), (at(11, 0), Dir::Right));
}

/// [R4.7-2] Right first: heading S on the left border, the rail turns at the corner (0, 59).
#[test]
fn minion_tries_right_then_straight_then_left_then_back() {
    let grid = Grid::level_start(&level(&[], &[]));
    let cases = [
        ("straight", at(0, 10), Dir::Up, at(0, 9), Dir::Up),
        (
            "right at the corner",
            at(0, 0),
            Dir::Up,
            at(1, 0),
            Dir::Right,
        ),
        (
            "left at the corner",
            at(59, 0),
            Dir::Up,
            at(58, 0),
            Dir::Left,
        ),
    ];
    for (name, pos, heading, to, new_heading) in cases {
        assert_eq!(
            right_hand_step(&grid, pos, heading),
            (to, new_heading),
            "{name}"
        );
    }
}

/// A band of `CLAIMED` cells on row 30 from the left border to x = 20, with a column on x = 5
/// from y = 20 to the band when `with_column`.
fn band_grid(with_column: bool) -> Grid {
    let mut grid = Grid::level_start(&level(&[], &[]));
    paint(&mut grid, (1..=20).map(|x| (x, 30)), Cell::Claimed);
    if with_column {
        paint(&mut grid, (20..30).map(|y| (5, y)), Cell::Claimed);
    }
    grid
}

/// [R4.7-2] Back is the last choice: at the tip (20, 30) of the band, right, straight and left
/// are open field, and the minion turns back.
#[test]
fn minion_turns_back_only_at_a_dead_end() {
    let grid = band_grid(false);
    assert_eq!(
        right_hand_step(&grid, at(20, 30), Dir::Right),
        (at(19, 30), Dir::Left)
    );
}

/// [R4.7-2] Straight before left: at the junction (5, 30), heading E, right is open field, and
/// straight (6, 30) and left (5, 29) are both rail cells.
#[test]
fn minion_goes_straight_before_turning_left() {
    let grid = band_grid(true);
    assert_eq!(
        right_hand_step(&grid, at(5, 30), Dir::Right),
        (at(6, 30), Dir::Right)
    );
}

/// [PIN-18] With no rail cell around, the minion stays and keeps its heading.
#[test]
fn minion_with_no_rail_direction_stays_and_keeps_its_heading() {
    let grid = Grid::level_start(&level(&[], &[]));
    assert_eq!(
        right_hand_step(&grid, at(30, 30), Dir::Left),
        (at(30, 30), Dir::Left)
    );
}

/// [R4.7-3] [PIN-06] On the bottom border, the open field is on the right of W (its right is N). S,
/// tested earlier, has it only behind its right (NW), which counts only when no heading has it on
/// its right.
#[test]
fn minion_orients_with_the_field_on_its_right() {
    let grid = Grid::level_start(&level(&[], &[]));
    assert_eq!(orient(&grid, at(30, 59), Dir::Up), Dir::Left);
}

/// Every interior cell claimed but `open`.
fn claimed_but(open: &[(u8, u8)]) -> Grid {
    let mut grid = Grid::level_start(&level(&[], &[]));
    paint(
        &mut grid,
        (1..59).flat_map(|x| (1..59).map(move |y| (x, y))),
        Cell::Claimed,
    );
    paint(&mut grid, open.iter().copied(), Cell::Empty);
    grid
}

/// [R4.7-3] The order N, E, S, W: around (30, 30) only (30, 31) and (29, 30) are open, so E
/// (right: S) and S (right: W) both have the field on their right, and E comes first.
#[test]
fn minion_takes_the_first_qualifying_heading_in_n_e_s_w_order() {
    let grid = claimed_but(&[(30, 31), (29, 30)]);
    assert_eq!(orient(&grid, at(30, 30), Dir::Up), Dir::Right);
}

/// [R4.7-3] [PIN-06] The same order for the field behind the right: only the diagonals (31, 29)
/// and (29, 31) are open, behind the right of W (NE) and of E (SW), and E comes first.
#[test]
fn minion_takes_the_first_heading_with_the_field_behind_its_right_in_n_e_s_w_order() {
    let grid = claimed_but(&[(31, 29), (29, 31)]);
    assert_eq!(orient(&grid, at(30, 30), Dir::Up), Dir::Right);
}

/// [R4.7-3] A heading needs no rail cell ahead: on a one-cell bump of the bottom border, N has the
/// field on its right (E) and ahead.
#[test]
fn minion_orients_without_a_rail_cell_ahead() {
    let grid = Grid::level_start(&level(&[], &[(30, 58)]));
    assert_eq!(orient(&grid, at(30, 58), Dir::Down), Dir::Up);
}

/// [R4.7-3] [PIN-06] A border corner touches the field only diagonally: no heading has the field
/// on its right, and S is the first with the field behind its right (NW).
#[test]
fn minion_on_a_cell_touching_the_field_diagonally_faces_the_field_behind_its_right() {
    let grid = Grid::level_start(&level(&[], &[]));
    assert_eq!(orient(&grid, at(59, 59), Dir::Up), Dir::Down);
}

/// [R4.7-3] [PIN-16] With no qualifying heading, which no rail cell has, a minion keeps its heading,
/// which at level start is N.
#[test]
fn minion_with_no_qualifying_heading_faces_n_at_level_start() {
    let block: Vec<(u8, u8)> = (9..=11)
        .flat_map(|x| (9..=11).map(move |y| (x, y)))
        .collect();
    let level = level(&[(10, 10)], &block);
    let grid = Grid::level_start(&level);
    let minions = Minions::start(&level, &grid);
    assert_eq!(live(&minions, 0), (at(10, 10), Dir::Up));
}

/// [PIN-16] Every minion runs the orientation step at level start.
#[test]
fn minions_are_oriented_at_level_start() {
    let level = level(&[(30, 59), (0, 30)], &[]);
    let grid = Grid::level_start(&level);
    let minions = Minions::start(&level, &grid);
    assert_eq!(
        (live(&minions, 0), live(&minions, 1)),
        ((at(30, 59), Dir::Left), (at(0, 30), Dir::Up))
    );
}

/// [R4.7-10] A live minion moves on the ticks that are multiples of the period.
#[test]
fn minion_moves_on_ticks_that_are_multiples_of_the_period() {
    let mut level = level(&[(30, 59)], &[]);
    level.minion_period = 3;
    let grid = Grid::level_start(&level);
    let mut minions = Minions::start(&level, &grid);
    let moved: Vec<bool> = (0..6)
        .map(|tick| {
            let step = minions.step(&grid, tick)[0].unwrap();
            step.from != step.to
        })
        .collect();
    assert_eq!(moved, [true, false, false, true, false, false]);
}

/// [R4.7-7] [R4.7-8] A capture that takes a minion's cell off the rail destroys it with its
/// countdown set to R.
#[test]
fn minion_stranded_by_a_capture_is_destroyed() {
    let level = level(&[(30, 59)], &[]);
    let mut grid = Grid::level_start(&level);
    let mut minions = Minions::start(&level, &grid);
    paint(&mut grid, (29..=31).map(|x| (x, 58)), Cell::Claimed);
    minions.after_capture(&grid);
    assert_eq!(
        minions.slots[0].unwrap().state,
        State::Destroyed { countdown: 3 }
    );
}

/// [R4.7-8] A minion still on the rail after a capture runs the orientation step.
#[test]
fn minion_on_the_rail_after_a_capture_is_reoriented() {
    let level = level(&[(30, 59)], &[]);
    let mut grid = Grid::level_start(&level);
    let mut minions = Minions::start(&level, &grid);
    minions.slots[0] = Some(Minion {
        spawn: at(30, 59),
        state: State::Live {
            pos: at(30, 59),
            heading: Dir::Right,
        },
    });
    paint(&mut grid, [(29, 58)], Cell::Claimed);
    minions.after_capture(&grid);
    assert_eq!(live(&minions, 0), (at(30, 59), Dir::Left));
}

/// [R4.7-9] [PIN-17] Destroyed at tick t, a minion respawns at step 5 of tick t + R − 1, oriented,
/// without moving on that tick, at its spawn cell when it is still a rail cell.
#[test]
fn minion_respawns_at_its_spawn_after_r_ticks_counting_the_tick_of_destruction() {
    let level = level(&[(30, 59)], &[]);
    let grid = Grid::level_start(&level);
    let mut minions = Minions::start(&level, &grid);
    minions.slots[0] = Some(Minion {
        spawn: at(30, 59),
        state: State::Destroyed { countdown: 3 },
    });
    let destruction_tick = 10;
    let steps: Vec<_> = (destruction_tick..destruction_tick + 3)
        .map(|tick| minions.step(&grid, tick)[0])
        .collect();
    let respawned = steps[2].unwrap();
    assert_eq!(
        (
            steps[0],
            steps[1],
            respawned.from,
            respawned.to,
            live(&minions, 0)
        ),
        (None, None, at(30, 59), at(30, 59), (at(30, 59), Dir::Left))
    );
}

/// [R4.7-7] A spawn no longer on the rail sends the respawn to the first rail cell in row-major
/// order: (0, 0) on this board.
#[test]
fn minion_respawns_at_the_first_rail_cell_when_its_spawn_is_off_the_rail() {
    let level = level(&[(30, 59)], &[]);
    let mut grid = Grid::level_start(&level);
    paint(&mut grid, (29..=31).map(|x| (x, 58)), Cell::Claimed);
    // The top-left block takes the corner off the rail: in row-major order the first rail cell
    // is (10, 0), in column-major order it would be (0, 10).
    paint(
        &mut grid,
        (1..=10).flat_map(|x| (1..=10).map(move |y| (x, y))),
        Cell::Claimed,
    );
    let mut minions = Minions::start(&level, &grid);
    minions.slots[0] = Some(Minion {
        spawn: at(30, 59),
        state: State::Destroyed { countdown: 1 },
    });
    minions.step(&grid, 0);
    assert_eq!(live(&minions, 0).0, at(10, 0));
}

/// [R4.7-11] With every cell claimed but the boss's 5 x 5 square, the rail is the ring around
/// it, and a minion whose spawn is off the rail respawns there.
#[test]
fn minion_respawns_on_the_rail_around_the_boss_when_nothing_else_is_open() {
    let level = level(&[(30, 59)], &[]);
    let mut grid = Grid::level_start(&level);
    paint(
        &mut grid,
        (1..59).flat_map(|x| (1..59).map(move |y| (x, y))),
        Cell::Claimed,
    );
    paint(
        &mut grid,
        (20..25).flat_map(|x| (20..25).map(move |y| (x, y))),
        Cell::Empty,
    );
    let mut minions = Minions::start(&level, &grid);
    minions.slots[0] = Some(Minion {
        spawn: at(30, 59),
        state: State::Destroyed { countdown: 1 },
    });
    minions.step(&grid, 0);
    assert_eq!(live(&minions, 0).0, at(19, 19));
}

/// [R4.7-8] A capture destroys live minions only: a destroyed minion's countdown goes on.
#[test]
fn capture_leaves_a_destroyed_minions_countdown_alone() {
    let level = level(&[(30, 59)], &[]);
    let grid = Grid::level_start(&level);
    let mut minions = Minions::start(&level, &grid);
    minions.slots[0] = Some(Minion {
        spawn: at(30, 59),
        state: State::Destroyed { countdown: 2 },
    });
    minions.after_capture(&grid);
    assert_eq!(
        minions.slots[0].unwrap().state,
        State::Destroyed { countdown: 2 }
    );
}

/// [R4.7-1] Over random boards, a minion is on a rail cell after every move.
#[test]
fn minions_are_on_rail_cells_after_every_move() {
    let mut state: u64 = 0x5eed_7777;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    for board in 0..200 {
        let mut grid = Grid::level_start(&level(&[], &[]));
        for _ in 0..8 {
            let (x0, y0, w, h) = (
                1 + next() % 55,
                1 + next() % 55,
                1 + next() % 6,
                1 + next() % 6,
            );
            paint(
                &mut grid,
                (x0..(x0 + w).min(59))
                    .flat_map(|x| (y0..(y0 + h).min(59)).map(move |y| (x as u8, y as u8))),
                Cell::Claimed,
            );
        }
        let start = CellIndex::all()
            .map(CellIndex::pos)
            .find(|&pos| grid.is_rail(pos))
            .unwrap();
        let (mut pos, mut heading) = (start, Dir::Right);
        for _ in 0..1_000 {
            (pos, heading) = right_hand_step(&grid, pos, heading);
            assert!(grid.is_rail(pos), "board {board}: {pos:?}");
        }
    }
}
