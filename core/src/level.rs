//! Level bytes v1 (README §5): 472 big-endian bytes, the range rules checked while decoding,
//! then the validity of the level itself.

use core::num::NonZeroU16;

use crate::consts::{
    BP, CAPTURE_STACK, CELLS, LEVEL_LEN, LEVEL_VERSION, MAX_BOSS_SIZE, MAX_TICKS, MIN_BOSS_SIZE,
    MIN_OPEN_AREA, MINION_SLOTS, OBSTACLE_BYTES, PLAYER_START, W,
};
use crate::error::Error;
use crate::grid::Grid;
use crate::keccak::keccak256;
use crate::types::{Cell, CellIndex, Pos};

/// The cells the top-left cell of the boss's body may start on, inclusive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x0: u8,
    pub y0: u8,
    pub x1: u8,
    pub y1: u8,
}

/// The fields of level bytes v1, in byte order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Level {
    /// Cell i = y × 60 + x is bit 7 − i % 8 of byte i / 8; a set bit is an obstacle.
    pub obstacles: [u8; OBSTACLE_BYTES],
    pub boss_size: u8,
    pub spawn: Rect,
    pub boss_period: u8,
    pub minion_count: u8,
    pub minion_spawns: [Pos; MINION_SLOTS],
    pub minion_period: u8,
    pub respawn_delay: u16,
    pub time_limit: u16,
    pub lives: u8,
    pub target_bp: u16,
}

impl Level {
    /// The obstacle bit of every cell, in row-major order.
    pub fn obstacle_bits(&self) -> impl Iterator<Item = bool> + '_ {
        self.obstacles
            .iter()
            .flat_map(|&byte| (0..8).map(move |bit| (byte << bit) & 0x80 != 0))
    }

    /// The obstacle bitmap of `cells`, one obstacle bit per cell in row-major order: the
    /// inverse of `obstacle_bits`.
    pub fn obstacle_bitmap(cells: &[bool; CELLS]) -> [u8; OBSTACLE_BYTES] {
        let mut bitmap = [0; OBSTACLE_BYTES];
        for (byte, bits) in bitmap.iter_mut().zip(cells.as_chunks::<8>().0) {
            *byte = bits
                .iter()
                .fold(0_u8, |byte, &bit| byte.wrapping_shl(1) | u8::from(bit));
        }
        bitmap
    }

    /// The spawns of the level's minions: its first `minion_count` slots.
    pub fn minions(&self) -> impl Iterator<Item = Pos> + '_ {
        self.minion_spawns
            .iter()
            .copied()
            .take(usize::from(self.minion_count))
    }
}

/// What validity establishes at level start.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LevelStart {
    pub grid: Grid,
    /// The cells `EMPTY` at level start, the denominator of `claim_bp` [R4.1-8].
    pub claimable: NonZeroU16,
}

/// Reads level bytes, checking the rules of README §5's table in order (20 to 34) and
/// returning the first broken rule's error.
pub fn decode(bytes: &[u8]) -> Result<Level, Error> {
    let bytes: &[u8; LEVEL_LEN] = bytes.try_into().map_err(|_| Error::LevelLength)?;
    let &[
        version,
        obstacles @ ..,
        boss_size,
        x0,
        y0,
        x1,
        y1,
        boss_period,
        minion_count,
        m0x,
        m0y,
        m1x,
        m1y,
        m2x,
        m2y,
        minion_period,
        respawn_high,
        respawn_low,
        time_high,
        time_low,
        lives,
        target_high,
        target_low,
    ] = bytes;
    if version != LEVEL_VERSION {
        return Err(Error::LevelVersion);
    }
    let level = Level {
        obstacles,
        boss_size,
        spawn: Rect { x0, y0, x1, y1 },
        boss_period,
        minion_count,
        minion_spawns: [
            Pos { x: m0x, y: m0y },
            Pos { x: m1x, y: m1y },
            Pos { x: m2x, y: m2y },
        ],
        minion_period,
        respawn_delay: u16::from_be_bytes([respawn_high, respawn_low]),
        time_limit: u16::from_be_bytes([time_high, time_low]),
        lives,
        target_bp: u16::from_be_bytes([target_high, target_low]),
    };
    check_ranges(&level)?;
    Ok(level)
}

/// The level bytes of `level`, without checking it.
pub fn encode(level: &Level) -> [u8; LEVEL_LEN] {
    let mut bytes = [0; LEVEL_LEN];
    let [
        version,
        obstacles @ ..,
        boss_size,
        x0,
        y0,
        x1,
        y1,
        boss_period,
        minion_count,
        m0x,
        m0y,
        m1x,
        m1y,
        m2x,
        m2y,
        minion_period,
        respawn_high,
        respawn_low,
        time_high,
        time_low,
        lives,
        target_high,
        target_low,
    ] = &mut bytes;
    let [m0, m1, m2] = level.minion_spawns;
    *version = LEVEL_VERSION;
    *obstacles = level.obstacles;
    [*boss_size, *x0, *y0, *x1, *y1, *boss_period, *minion_count] = [
        level.boss_size,
        level.spawn.x0,
        level.spawn.y0,
        level.spawn.x1,
        level.spawn.y1,
        level.boss_period,
        level.minion_count,
    ];
    [*m0x, *m0y, *m1x, *m1y, *m2x, *m2y] = [m0.x, m0.y, m1.x, m1.y, m2.x, m2.y];
    *minion_period = level.minion_period;
    [*respawn_high, *respawn_low] = level.respawn_delay.to_be_bytes();
    [*time_high, *time_low] = level.time_limit.to_be_bytes();
    *lives = level.lives;
    [*target_high, *target_low] = level.target_bp.to_be_bytes();
    bytes
}

/// keccak256 of the level bytes (README §5).
pub fn level_id(bytes: &[u8]) -> [u8; 32] {
    keccak256(&[bytes])
}

/// Checks the validity of a level that `decode` accepted, in README §5's order (35 to 39),
/// and returns its level-start state. The region search runs on `stack`.
pub fn validate(level: &Level, stack: &mut [u16; CAPTURE_STACK]) -> Result<LevelStart, Error> {
    let mut grid = Grid::level_start(level);
    let open = grid.count(Cell::Empty);
    let start = CellIndex::all()
        .find(|&cell| grid.get(cell) == Cell::Empty)
        .ok_or(Error::OpenAreaSplit)?;
    grid.mark_boss_zone([start], stack)
        .ok_or(Error::OpenAreaSplit)?;
    let reached = grid.count(Cell::BossZone);
    grid.replace(Cell::BossZone, Cell::Empty);
    if reached != open {
        return Err(Error::OpenAreaSplit);
    }
    let claimable = NonZeroU16::new(open)
        .filter(|open| open.get() >= MIN_OPEN_AREA)
        .ok_or(Error::OpenAreaSmall)?;
    let body_area_open = CellIndex::all()
        .filter(|cell| spawn_covers(level, cell.pos()))
        .all(|cell| grid.get(cell) == Cell::Empty);
    if !body_area_open {
        return Err(Error::BossSpawn);
    }
    if level.minions().any(|spawn| !grid.is_rail(spawn)) {
        return Err(Error::MinionSpawnRail);
    }
    if !grid.is_rail(PLAYER_START) {
        return Err(Error::PlayerStartRail);
    }
    Ok(LevelStart { grid, claimable })
}

fn check_ranges(level: &Level) -> Result<(), Error> {
    let border_obstacle = CellIndex::all()
        .zip(level.obstacle_bits())
        .any(|(cell, obstacle)| obstacle && cell.pos().is_border());
    if border_obstacle {
        return Err(Error::BorderObstacle);
    }
    if !(MIN_BOSS_SIZE..=MAX_BOSS_SIZE).contains(&level.boss_size) {
        return Err(Error::BossSize);
    }
    let Rect { x0, y0, x1, y1 } = level.spawn;
    if x0 > x1 || y0 > y1 {
        return Err(Error::SpawnOrder);
    }
    let body_inside = |start: u8| {
        start
            .checked_add(level.boss_size)
            .is_some_and(|end| end < W)
    };
    if x0 < 1 || y0 < 1 || !body_inside(x1) || !body_inside(y1) {
        return Err(Error::SpawnBounds);
    }
    if level.boss_period == 0 {
        return Err(Error::BossPeriod);
    }
    if usize::from(level.minion_count) > MINION_SLOTS {
        return Err(Error::MinionCount);
    }
    for (slot, spawn) in level.minion_spawns.iter().enumerate() {
        if slot < usize::from(level.minion_count) {
            if spawn.x >= W || spawn.y >= W {
                return Err(Error::MinionSpawn);
            }
        } else if *spawn != (Pos { x: 0, y: 0 }) {
            return Err(Error::MinionSlot);
        }
    }
    let has_minions = level.minion_count > 0;
    if (level.minion_period > 0) != has_minions {
        return Err(Error::MinionPeriod);
    }
    if (level.respawn_delay > 0) != has_minions {
        return Err(Error::RespawnDelay);
    }
    if !(1..=MAX_TICKS).contains(&level.time_limit) {
        return Err(Error::TimeLimit);
    }
    if level.lives == 0 {
        return Err(Error::Lives);
    }
    if !(1..=BP).contains(&level.target_bp) {
        return Err(Error::Target);
    }
    Ok(())
}

/// Whether some start in the spawn rectangle puts the B × B body on `pos`: x0 ≤ x < x1 + B,
/// and the same for y.
fn spawn_covers(level: &Level, pos: Pos) -> bool {
    let Rect { x0, y0, x1, y1 } = level.spawn;
    let covers = |start: u8, end: u8, at: u8| {
        at >= start
            && end
                .checked_add(level.boss_size)
                .is_none_or(|limit| at < limit)
    };
    covers(x0, x1, pos.x) && covers(y0, y1, pos.y)
}
