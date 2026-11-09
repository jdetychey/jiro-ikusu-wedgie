//! The boss (README §4.6): axis-separated moves, leading-edge tests, bumps into `TRAIL`, and
//! the seeded deflections of D14 (R4.6-6 to R4.6-12).

use jiro_core::boss::{Boss, deflection_draw, fmix32};
use jiro_core::grid::Grid;
use jiro_core::keccak::keccak256;
use jiro_core::level::{Level, Rect};
use jiro_core::seed::level_seed;
use jiro_core::types::{Cell, CellIndex, Pos};

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

fn index(x: u8, y: u8) -> CellIndex {
    CellIndex::new(Pos { x, y }).unwrap()
}

fn paint(grid: &mut Grid, cells: impl IntoIterator<Item = (u8, u8)>, cell: Cell) {
    for (x, y) in cells {
        grid.set(index(x, y), cell);
    }
}

/// A key whose draws never deflect on ticks 0 to 199, so a test sees R4.6-3 alone.
fn quiet_key() -> u32 {
    (0..)
        .find(|&key| (0..200).all(|tick| deflection_draw(key, tick) >= 2))
        .unwrap()
}

fn boss(x: u8, y: u8, size: u8, vx: i8, vy: i8) -> Boss {
    Boss {
        pos: Pos { x, y },
        size,
        vx,
        vy,
        period: 1,
        deflection_key: quiet_key(),
    }
}

fn body_open(grid: &Grid, boss: &Boss) -> bool {
    boss.body().all(|pos| grid.at(pos) == Some(Cell::Empty))
}

/// The draft's diagonal test would enter (30, 30); one axis at a time, the boss slides past it.
#[test]
fn outside_corner_is_never_entered() {
    for size in [2, 5, 8] {
        let mut grid = Grid::level_start(&open_field());
        let corner = 30;
        paint(&mut grid, [(corner, corner)], Cell::Claimed);
        let mut boss = boss(corner - size, corner - size, size, 1, 1);
        boss.step(&grid, 0);
        assert_eq!(
            (boss.pos, boss.vx, boss.vy, body_open(&grid, &boss)),
            (
                Pos {
                    x: corner - size + 1,
                    y: corner - size
                },
                1,
                -1,
                true
            ),
            "B = {size}"
        );
    }
}

/// In a corridor exactly as wide as the body, the boss bounces on x every move and slides on y.
#[test]
fn corridor_as_wide_as_the_body_is_followed() {
    for size in [2, 5, 8] {
        let mut grid = Grid::level_start(&open_field());
        paint(
            &mut grid,
            (1..59).flat_map(|y| [(19, y), (20 + size, y)]),
            Cell::Claimed,
        );
        let mut boss = boss(20, 10, size, 1, 1);
        for tick in 0..20 {
            boss.step(&grid, tick);
            assert!(
                boss.pos.x == 20 && body_open(&grid, &boss),
                "B = {size}, tick {tick}: {boss:?}"
            );
        }
        assert_eq!(boss.pos.y, 30, "B = {size}");
    }
}

/// [R4.6-3] A `TRAIL` cell anywhere on a leading edge hits the player and bounces the boss.
#[test]
fn trail_on_the_leading_edge_hits_and_bounces() {
    for size in [2, 5, 8] {
        let last = size - 1;
        for offset in [0, last / 2, last] {
            let mut grid = Grid::level_start(&open_field());
            paint(&mut grid, [(20 + size, 20 + offset)], Cell::Trail);
            let mut on_x = boss(20, 20, size, 1, 1);
            let hit_x = on_x.step(&grid, 0);
            assert_eq!(
                (hit_x, on_x.pos.x, on_x.vx),
                (true, 20, -1),
                "x edge, B = {size}, offset {offset}"
            );

            let mut grid = Grid::level_start(&open_field());
            paint(&mut grid, [(19 + offset, 20 + size)], Cell::Trail);
            let mut on_y = boss(20, 20, size, -1, 1);
            let hit_y = on_y.step(&grid, 0);
            assert_eq!(
                (hit_y, on_y.pos.y, on_y.vy),
                (true, 20, -1),
                "y edge, B = {size}, offset {offset}"
            );
        }
    }
}

#[test]
fn claimed_edge_bounces_without_a_hit() {
    let mut grid = Grid::level_start(&open_field());
    paint(&mut grid, [(25, 22)], Cell::Claimed);
    let mut boss = boss(20, 20, 5, 1, 1);
    assert_eq!((boss.step(&grid, 0), boss.vx), (false, -1));
}

/// [R4.4-2] The boss moves on the ticks where tick % period == 0.
#[test]
fn boss_moves_only_on_ticks_that_are_multiples_of_its_period() {
    let grid = Grid::level_start(&open_field());
    let mut boss = Boss {
        period: 3,
        ..boss(20, 20, 5, 1, 1)
    };
    let moved: Vec<bool> = (0..7)
        .map(|tick| {
            let before = boss.pos;
            boss.step(&grid, tick);
            boss.pos != before
        })
        .collect();
    assert_eq!(moved, [true, false, false, true, false, false, true]);
}

/// [R4.6-8] [R4.6-9] README §6.4: fmix32's test values and the tick-0 draw of the worked example.
#[test]
fn fmix32_test_values() {
    assert_eq!(
        (
            fmix32(0),
            fmix32(1),
            fmix32(0xffff_ffff),
            fmix32(0x9467_6303),
            deflection_draw(0x9467_6303, 0)
        ),
        (0, 0x514e_28b7, 0x81f1_6f39, 0x5cb2_ceac, 23)
    );
}

/// [R4.6-5] README §6.4's levelIndex 2 example, whose vx and vy differ: the boss starts at its
/// top-left cell (20, 32), moving toward +x and −y.
#[test]
fn boss_starts_at_the_seeds_cell_and_direction() {
    let seed = level_seed(&keccak256(&[b"jiro.example.9"]), 2);
    let boss = Boss::start(&open_field(), &seed).unwrap();
    assert_eq!((boss.pos, boss.vx, boss.vy), (Pos { x: 20, y: 32 }, 1, -1));
}

/// [R4.6-11] README §6.4, the worked example `open-field-empty`, played to the end with the player
/// standing still.
#[test]
fn open_field_worked_example() {
    let level = open_field();
    let seed = level_seed(&keccak256(&[b"jiro.example.9"]), 0);
    let grid = Grid::level_start(&level);
    let mut boss = Boss::start(&level, &seed).unwrap();
    let mut tick_0 = None;
    let mut deflections = Vec::new();
    for tick in 0..2_700 {
        let moves = tick % u16::from(boss.period) == 0;
        let draw = deflection_draw(boss.deflection_key, tick);
        boss.step(&grid, tick);
        if tick == 0 {
            tick_0 = Some((boss.pos, boss.vx, boss.vy));
        }
        if moves && draw < 2 {
            deflections.push((tick, draw, boss.pos, boss.vx, boss.vy));
        }
    }
    let pos = |x, y| Pos { x, y };
    assert_eq!(
        (
            tick_0,
            deflections.len(),
            &deflections[..2],
            (boss.pos, boss.vx, boss.vy)
        ),
        (
            Some((pos(22, 21), -1, -1)),
            43,
            &[(82, 1, pos(20, 19), 1, -1), (140, 0, pos(47, 11), -1, 1)][..],
            (pos(3, 18), 1, -1)
        )
    );
}

struct XorShift(u64);

impl XorShift {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, bound: u64) -> u8 {
        (self.next() % bound) as u8
    }
}

/// A random board for body size `size`, and a start whose body is on `EMPTY` cells.
fn random_board(rng: &mut XorShift, size: u8) -> (Grid, Pos) {
    loop {
        let mut grid = Grid::level_start(&open_field());
        for _ in 0..rng.below(6) {
            let (x0, y0) = (1 + rng.below(58), 1 + rng.below(58));
            let (w, h) = (1 + rng.below(12), 1 + rng.below(12));
            paint(
                &mut grid,
                (x0..(x0 + w).min(59)).flat_map(|x| (y0..(y0 + h).min(59)).map(move |y| (x, y))),
                Cell::Claimed,
            );
        }
        let start = Pos {
            x: 1 + rng.below(u64::from(59 - size)),
            y: 1 + rng.below(u64::from(59 - size)),
        };
        let probe = boss(start.x, start.y, size, 1, 1);
        if body_open(&grid, &probe) {
            return (grid, start);
        }
    }
}

/// Changes 3 cells per tick outside both bodies, as cuts and captures would.
fn mutate(grid: &mut Grid, bosses: [&Boss; 2], rng: &mut XorShift) {
    for _ in 0..3 {
        let pos = Pos {
            x: 1 + rng.below(58),
            y: 1 + rng.below(58),
        };
        if bosses
            .iter()
            .any(|boss| boss.body().any(|cell| cell == pos))
        {
            continue;
        }
        let cell = index(pos.x, pos.y);
        let next = match (grid.get(cell), rng.below(10)) {
            (Cell::Empty, 0) => Cell::Claimed,
            (Cell::Empty, 1..=3) => Cell::Trail,
            (Cell::Trail, 0..=6) => Cell::Empty,
            (Cell::Trail, _) => Cell::Claimed,
            (cell, _) => cell,
        };
        grid.set(cell, next);
    }
}

/// [R4.6-10] [R4.6-1] and [R4.6-12]: over 5,000 random boards × 2,700 ticks for each B from 2 to 8,
/// with cells changing around the boss and the deflections on, the body never covers a cell that
/// isn't `EMPTY`, vx and vy stay ±1, and two keys with the same start and direction part within 400
/// boss moves at move periods 1 to 6.
#[test]
#[cfg_attr(debug_assertions, ignore)]
fn body_stays_on_empty_cells_and_seeds_diverge_on_random_boards() {
    let threads: Vec<_> = (2..=8u8)
        .map(|size| {
            std::thread::spawn(move || {
                let mut rng = XorShift(0x5eed_0000 + u64::from(size));
                for board in 0..5_000 {
                    let (mut grid, start) = random_board(&mut rng, size);
                    let period = 1 + rng.below(6);
                    let key_a = rng.next() as u32;
                    let key_b = loop {
                        let key = rng.next() as u32;
                        if key != key_a {
                            break key;
                        }
                    };
                    let vx = if rng.next() & 1 == 0 { 1 } else { -1 };
                    let vy = if rng.next() & 1 == 0 { 1 } else { -1 };
                    let start_boss = Boss {
                        pos: start,
                        size,
                        vx,
                        vy,
                        period,
                        deflection_key: key_a,
                    };
                    let (mut a, mut b) = (
                        start_boss,
                        Boss {
                            deflection_key: key_b,
                            ..start_boss
                        },
                    );
                    let (mut moves, mut parted_at) = (0u32, None);
                    for tick in 0..2_700 {
                        mutate(&mut grid, [&a, &b], &mut rng);
                        a.step(&grid, tick);
                        b.step(&grid, tick);
                        for boss in [&a, &b] {
                            assert!(
                                body_open(&grid, boss),
                                "B = {size}, board {board}, tick {tick}: {boss:?}"
                            );
                            assert!(
                                boss.vx.abs() == 1 && boss.vy.abs() == 1,
                                "B = {size}, board {board}"
                            );
                        }
                        if tick % u16::from(period) == 0 {
                            moves += 1;
                            if parted_at.is_none() && (a.pos, a.vx, a.vy) != (b.pos, b.vx, b.vy) {
                                parted_at = Some(moves);
                            }
                        }
                    }
                    let limit = moves.min(400);
                    assert!(
                        parted_at.is_some_and(|at| at <= limit),
                        "B = {size}, board {board}: parted at {parted_at:?} of {moves} moves"
                    );
                }
            })
        })
        .collect();
    for thread in threads {
        thread.join().unwrap();
    }
}
