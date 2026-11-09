//! Capture (README §4.8): shapes, and the stack fill of the core against a queue fill and a
//! scanline fill that exist only here. The result depends only on which `EMPTY` cells are
//! 4-connected to the boss's body, not on the visit order.

use std::collections::VecDeque;

use jiro_core::capture::capture;
use jiro_core::consts::CAPTURE_STACK;
use jiro_core::grid::Grid;
use jiro_core::level::{Level, Rect};
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

fn body(x: u8, y: u8, size: u8) -> Vec<CellIndex> {
    (0..size)
        .flat_map(|dy| (0..size).map(move |dx| index(x + dx, y + dy)))
        .collect()
}

fn core_capture(grid: &mut Grid, body: &[CellIndex]) -> u16 {
    capture(grid, body.iter().copied(), &mut [0; CAPTURE_STACK]).unwrap()
}

fn cell_at(grid: &Grid, x: u8, y: u8) -> Cell {
    grid.get(index(x, y))
}

/// A U-shaped cut from the top border: x = 10 and x = 20 for y = 1 to 5, and y = 5 between.
fn top_u() -> Vec<(u8, u8)> {
    let sides = (1..=5).flat_map(|y| [(10, y), (20, y)]);
    sides.chain((11..20).map(|x| (x, 5))).collect()
}

#[test]
fn cut_along_the_top_border_claims_the_rectangle_it_closes() {
    let mut grid = Grid::level_start(&open_field());
    paint(&mut grid, top_u(), Cell::Trail);
    let size = core_capture(&mut grid, &body(30, 30, 5));
    let enclosed = (11..20).flat_map(|x| (1..5).map(move |y| (x, y)));
    assert_eq!(size, 9 * 4 + 19);
    assert!(
        enclosed
            .chain(top_u())
            .all(|(x, y)| cell_at(&grid, x, y) == Cell::Claimed)
    );
}

#[test]
fn l_shaped_cut_claims_the_corner() {
    let mut grid = Grid::level_start(&open_field());
    let cut: Vec<(u8, u8)> = (1..=10)
        .map(|y| (10, y))
        .chain((1..10).map(|x| (x, 10)))
        .collect();
    paint(&mut grid, cut, Cell::Trail);
    let size = core_capture(&mut grid, &body(30, 30, 5));
    assert_eq!(
        (
            size,
            cell_at(&grid, 1, 1),
            cell_at(&grid, 9, 9),
            cell_at(&grid, 11, 11)
        ),
        (81 + 19, Cell::Claimed, Cell::Claimed, Cell::Empty)
    );
}

#[test]
fn obstacles_inside_a_cut_are_not_counted() {
    let mut grid = Grid::level_start(&open_field());
    paint(&mut grid, [(14, 2), (15, 2)], Cell::Claimed);
    paint(&mut grid, top_u(), Cell::Trail);
    assert_eq!(core_capture(&mut grid, &body(30, 30, 5)), 9 * 4 - 2 + 19);
}

/// A dead-end corridor one cell wide, open toward the boss's side, stays open: it is connected
/// to the body, even though the body can't enter it.
#[test]
fn pocket_narrower_than_the_body_stays_open() {
    let mut grid = Grid::level_start(&open_field());
    paint(
        &mut grid,
        (20..=30).flat_map(|y| [(40, y), (42, y)]).chain([(41, 20)]),
        Cell::Claimed,
    );
    paint(&mut grid, top_u(), Cell::Trail);
    core_capture(&mut grid, &body(30, 40, 5));
    assert!((21..=30).all(|y| cell_at(&grid, 41, y) == Cell::Empty));
}

#[test]
fn cut_across_the_field_claims_the_side_without_the_boss() {
    let wall = || (1..59).map(|y| (30, y));
    for (boss_x, claimed_x, open_x, size) in
        [(10, 45, 10, 28 * 58 + 58), (40, 10, 45, 29 * 58 + 58)]
    {
        let mut grid = Grid::level_start(&open_field());
        paint(&mut grid, wall(), Cell::Trail);
        let captured = core_capture(&mut grid, &body(boss_x, 20, 5));
        assert_eq!(
            (
                captured,
                cell_at(&grid, claimed_x, 40),
                cell_at(&grid, open_x, 40)
            ),
            (size, Cell::Claimed, Cell::Empty),
            "boss at x = {boss_x}"
        );
    }
}

#[test]
fn body_cells_stay_empty_after_a_capture() {
    let mut grid = Grid::level_start(&open_field());
    paint(&mut grid, (1..59).map(|y| (30, y)), Cell::Trail);
    let boss = body(10, 20, 8);
    core_capture(&mut grid, &boss);
    assert!(boss.iter().all(|&cell| grid.get(cell) == Cell::Empty));
}

/// [R4.8-2] [R4.8-3] A cell is marked `BOSS_ZONE` when it is pushed, so it is pushed once: the
/// entries the fill wrote to the caller's stack are all different.
#[test]
fn fill_pushes_each_cell_once() {
    let mut grid = Grid::level_start(&open_field());
    let mut stack = [u16::MAX; CAPTURE_STACK];
    capture(&mut grid, body(1, 1, 8), &mut stack).unwrap();
    let mut pushed: Vec<u16> = stack
        .into_iter()
        .take_while(|&entry| entry != u16::MAX)
        .collect();
    let count = pushed.len();
    pushed.sort_unstable();
    pushed.dedup();
    assert_eq!(pushed.len(), count);
}

/// Step 3 of README §4.8, shared by the reference fills.
fn settle(grid: &mut Grid) -> u16 {
    let mut captured = 0;
    for cell in CellIndex::all() {
        match grid.get(cell) {
            Cell::BossZone => grid.set(cell, Cell::Empty),
            Cell::Empty | Cell::Trail => {
                grid.set(cell, Cell::Claimed);
                captured += 1;
            }
            Cell::Claimed => {}
        }
    }
    captured
}

fn neighbors(cell: CellIndex) -> impl Iterator<Item = CellIndex> {
    let pos = cell.pos();
    [(0, -1), (1, 0), (0, 1), (-1, 0)]
        .into_iter()
        .filter_map(move |(dx, dy)| pos.offset(dx, dy).and_then(CellIndex::new))
}

fn queue_capture(grid: &mut Grid, body: &[CellIndex]) -> u16 {
    let mut queue: VecDeque<CellIndex> = body.iter().copied().collect();
    for &cell in body {
        grid.set(cell, Cell::BossZone);
    }
    while let Some(cell) = queue.pop_front() {
        for next in neighbors(cell) {
            if grid.get(next) == Cell::Empty {
                grid.set(next, Cell::BossZone);
                queue.push_back(next);
            }
        }
    }
    settle(grid)
}

fn scanline_capture(grid: &mut Grid, body: &[CellIndex]) -> u16 {
    for &cell in body {
        grid.set(cell, Cell::BossZone);
    }
    let mut pending: Vec<Pos> = body.iter().map(|cell| cell.pos()).collect();
    let empty = |grid: &Grid, x: u8, y: u8| grid.at(Pos { x, y }) == Some(Cell::Empty);
    while let Some(Pos { x, y }) = pending.pop() {
        let (mut left, mut right) = (x, x);
        while left > 0 && empty(grid, left - 1, y) {
            left -= 1;
        }
        while empty(grid, right + 1, y) {
            right += 1;
        }
        for span_x in left..=right {
            if empty(grid, span_x, y) {
                grid.set(index(span_x, y), Cell::BossZone);
            }
        }
        for row in [y.checked_sub(1), Some(y + 1)].into_iter().flatten() {
            pending.extend(
                (left..=right)
                    .filter(|&span_x| empty(grid, span_x, row))
                    .map(|span_x| Pos { x: span_x, y: row }),
            );
        }
    }
    settle(grid)
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

/// Walls, blocks and random cut paths on the open field, and a body on `EMPTY` cells.
fn random_board(rng: &mut XorShift) -> Option<(Grid, Vec<CellIndex>)> {
    let mut grid = Grid::level_start(&open_field());
    for _ in 0..rng.below(6) {
        let (x0, y0) = (1 + rng.below(58), 1 + rng.below(58));
        let (w, h) = (1 + rng.below(20), 1 + rng.below(20));
        paint(
            &mut grid,
            (x0..(x0 + w).min(59)).flat_map(|x| (y0..(y0 + h).min(59)).map(move |y| (x, y))),
            Cell::Claimed,
        );
    }
    for _ in 0..rng.below(5) {
        let (mut x, mut y) = (1 + rng.below(58), 1 + rng.below(58));
        for _ in 0..rng.below(120) {
            grid.set(index(x, y), Cell::Trail);
            match rng.below(4) {
                0 => x = (x + 1).min(58),
                1 => x = (x - 1).max(1),
                2 => y = (y + 1).min(58),
                _ => y = (y - 1).max(1),
            }
        }
    }
    let size = 2 + rng.below(7);
    for _ in 0..50 {
        let (x, y) = (
            1 + rng.below(u64::from(59 - size)),
            1 + rng.below(u64::from(59 - size)),
        );
        let candidate = body(x, y, size);
        if candidate.iter().all(|&cell| grid.get(cell) == Cell::Empty) {
            return Some((grid, candidate));
        }
    }
    None
}

#[test]
#[cfg_attr(debug_assertions, ignore)]
fn stack_queue_and_scanline_fills_agree_on_5000_random_boards() {
    let mut rng = XorShift(0x0bad_5eed_1234_5678);
    let mut boards = 0;
    while boards < 5_000 {
        let Some((grid, body)) = random_board(&mut rng) else {
            continue;
        };
        let (mut stack, mut queue, mut scanline) = (grid.clone(), grid.clone(), grid.clone());
        let sizes = (
            core_capture(&mut stack, &body),
            queue_capture(&mut queue, &body),
            scanline_capture(&mut scanline, &body),
        );
        assert_eq!(
            (sizes.0, &stack),
            (sizes.1, &queue),
            "board {boards}: stack and queue"
        );
        assert_eq!(
            (sizes.0, &stack),
            (sizes.2, &scanline),
            "board {boards}: stack and scanline"
        );
        boards += 1;
    }
}
