//! The D7 coverage property (README §4.7, PIN-14) of the right-hand rule with the orientation
//! step, on random boards with obstacle blobs and captures applied mid-patrol. Each blob is
//! followed by a capture that keeps only the boss's open region, so every board can occur in
//! play. A minion is trapped if its (cell, heading) state repeats before it has visited every
//! cell of its rail component C [R4.7-4, R4.7-5, R4.7-6].

use jiro_core::grid::Grid;
use jiro_core::level::{Level, Rect};
use jiro_core::minion::{orient, right_hand_step};
use jiro_core::types::{Cell, CellIndex, Dir, Pos};

/// Boards with at least one trap among their 10,000 and 100,000 fixed-seed boards.
const EXPECTED_TRAPS_10K: usize = 0;
const EXPECTED_TRAPS_100K: usize = 0;

/// Blobs claimed after the spawn, each after a random number of moves and followed by a capture.
const CAPTURES_PER_BOARD: u32 = 3;

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

fn empty_level() -> Level {
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

fn claim(grid: &mut Grid, x: u8, y: u8) {
    if let Some(cell) = CellIndex::new(Pos { x, y }) {
        grid.set(cell, Cell::Claimed);
    }
}

/// Claims a blob: a block, or a band one or two cells thick, as captures and obstacles leave.
fn blob(grid: &mut Grid, rng: &mut XorShift) {
    let (x0, y0) = (1 + rng.below(58), 1 + rng.below(58));
    let (w, h) = match rng.below(3) {
        0 => (1 + rng.below(12), 1 + rng.below(12)),
        1 => (1 + rng.below(2), 2 + rng.below(25)),
        _ => (2 + rng.below(25), 1 + rng.below(2)),
    };
    for x in x0..x0.saturating_add(w).min(59) {
        for y in y0..y0.saturating_add(h).min(59) {
            claim(grid, x, y);
        }
    }
}

/// Claims every open cell outside the open region of a random open cell, the boss's, as a capture
/// does [R4.8-4]: in play the open field is always one 4-connected region.
fn capture_around_boss(grid: &mut Grid, rng: &mut XorShift) {
    let open: Vec<Pos> = CellIndex::all()
        .map(CellIndex::pos)
        .filter(|&pos| is_open(grid, pos))
        .collect();
    if open.is_empty() {
        return;
    }
    let boss = open[(rng.next() % open.len() as u64) as usize];
    let boss_region = component(boss, |pos| is_open(grid, pos));
    for pos in open {
        if !boss_region[index(pos)] {
            claim(grid, pos.x, pos.y);
        }
    }
}

fn is_open(grid: &Grid, pos: Pos) -> bool {
    matches!(grid.at(pos), Some(Cell::Empty | Cell::Trail))
}

fn random_rail_cell(grid: &Grid, rng: &mut XorShift) -> Option<Pos> {
    let rail: Vec<Pos> = CellIndex::all()
        .map(CellIndex::pos)
        .filter(|&pos| grid.is_rail(pos))
        .collect();
    (!rail.is_empty()).then(|| rail[(rng.next() % rail.len() as u64) as usize])
}

fn index(pos: Pos) -> usize {
    CellIndex::new(pos).unwrap().get()
}

/// The 4-connected component of `start` among the cells for which `member` holds, by cell index.
fn component(start: Pos, member: impl Fn(Pos) -> bool) -> Vec<bool> {
    let mut inside = vec![false; 3_600];
    inside[index(start)] = true;
    let mut pending = vec![start];
    while let Some(cell) = pending.pop() {
        for dir in Dir::PRIORITY {
            let (dx, dy) = dir.offset();
            let next = cell
                .offset(dx, dy)
                .filter(|&next| member(next) && !inside[index(next)]);
            if let Some(next) = next {
                inside[index(next)] = true;
                pending.push(next);
            }
        }
    }
    inside
}

/// Whether the minion's (cell, heading) state repeats before it has visited all of C.
fn trapped(grid: &Grid, mut pos: Pos, mut heading: Dir) -> bool {
    let component = component(pos, |cell| grid.is_rail(cell))
        .into_iter()
        .filter(|&inside| inside)
        .count();
    let mut visited = vec![false; 3_600];
    let mut states = vec![false; 4 * 3_600];
    let mut covered = 0;
    loop {
        if !visited[index(pos)] {
            visited[index(pos)] = true;
            covered += 1;
        }
        if covered == component {
            return false;
        }
        let state = 4 * index(pos) + heading as usize;
        if states[state] {
            return true;
        }
        states[state] = true;
        (pos, heading) = right_hand_step(grid, pos, heading);
    }
}

/// One board: the minion spawns, then captures land mid-patrol; returns whether it was trapped
/// after the spawn or after any capture.
fn board_traps(seed: u64) -> bool {
    let mut rng = XorShift(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1);
    let mut grid = Grid::level_start(&empty_level());
    for _ in 0..rng.below(10) {
        blob(&mut grid, &mut rng);
    }
    capture_around_boss(&mut grid, &mut rng);
    let Some(spawn) = random_rail_cell(&grid, &mut rng) else {
        return false;
    };
    let (mut pos, mut heading) = (spawn, orient(&grid, spawn, Dir::Up));
    if trapped(&grid, pos, heading) {
        return true;
    }
    for _ in 0..CAPTURES_PER_BOARD {
        for _ in 0..rng.below(200) {
            (pos, heading) = right_hand_step(&grid, pos, heading);
        }
        blob(&mut grid, &mut rng);
        capture_around_boss(&mut grid, &mut rng);
        if grid.is_rail(pos) {
            heading = orient(&grid, pos, heading);
        } else {
            let Some(respawn) = (if grid.is_rail(spawn) {
                Some(spawn)
            } else {
                first_rail_cell(&grid)
            }) else {
                return false;
            };
            (pos, heading) = (respawn, orient(&grid, respawn, Dir::Up));
        }
        if trapped(&grid, pos, heading) {
            return true;
        }
    }
    false
}

fn first_rail_cell(grid: &Grid) -> Option<Pos> {
    CellIndex::all()
        .map(CellIndex::pos)
        .find(|&pos| grid.is_rail(pos))
}

fn trapped_seeds(boards: u64) -> Vec<u64> {
    (0..boards).filter(|&seed| board_traps(seed)).collect()
}

/// [PIN-14] [R4.7-4] [R4.7-5] [R4.7-6] No minion is trapped on the 10,000 boards.
#[test]
#[cfg_attr(debug_assertions, ignore)]
fn traps_on_10000_boards_match_the_committed_count() {
    let seeds = trapped_seeds(10_000);
    println!("10,000 boards: {} trapped; seeds {seeds:?}", seeds.len());
    assert_eq!(seeds.len(), EXPECTED_TRAPS_10K, "trapped seeds: {seeds:?}");
}

/// [PIN-14] No minion is trapped on the 100,000 boards.
#[test]
#[ignore]
fn traps_on_100000_boards_match_the_committed_count() {
    let seeds = trapped_seeds(100_000);
    println!(
        "100,000 boards: {} trapped; first seeds {:?}",
        seeds.len(),
        &seeds[..seeds.len().min(40)]
    );
    assert_eq!(seeds.len(), EXPECTED_TRAPS_100K);
}
