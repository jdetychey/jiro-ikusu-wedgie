//! The boss (README §4.6): a B × B body that moves one axis at a time, testing its whole
//! leading edge, with the seeded deflections of D14.

use crate::grid::Grid;
use crate::level::Level;
use crate::seed::{boss_start, deflection_key};
use crate::types::{Cell, Pos};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Boss {
    /// The top-left cell of the body [R4.6-1].
    pub pos: Pos,
    pub size: u8,
    /// +1 or −1.
    pub vx: i8,
    /// +1 or −1.
    pub vy: i8,
    pub period: u8,
    /// A level constant from the level seed, not boss state (R4.6-11).
    pub deflection_key: u32,
}

#[derive(Clone, Copy)]
enum Axis {
    X,
    Y,
}

impl Boss {
    /// The boss at level start: its start and direction from the level seed (R4.6-5), and its
    /// deflection key (R4.6-7). `None` if the spawn rectangle is empty, which level validity
    /// rules out.
    pub fn start(level: &Level, seed: &[u8; 32]) -> Option<Self> {
        let start = boss_start(&level.spawn, seed)?;
        Some(Self {
            pos: start.pos,
            size: level.boss_size,
            vx: start.vx,
            vy: start.vy,
            period: level.boss_period,
            deflection_key: deflection_key(seed),
        })
    }

    /// The cells under the body, row by row.
    pub fn body(&self) -> impl Iterator<Item = Pos> {
        let Self { pos, size, .. } = *self;
        (0..size).flat_map(move |dy| {
            (0..size).filter_map(move |dx| {
                Some(Pos {
                    x: pos.x.checked_add(dx)?,
                    y: pos.y.checked_add(dy)?,
                })
            })
        })
    }

    /// Step 4 of the tick (R4.4-9): on a tick the boss moves, the deflection draw of R4.6-7, then
    /// the edge tests of R4.6-3, x first. Returns whether a leading edge held `TRAIL`, which hits
    /// the player.
    pub fn step(&mut self, grid: &Grid, tick: u16) -> bool {
        if tick.checked_rem(u16::from(self.period)) != Some(0) {
            return false;
        }
        match deflection_draw(self.deflection_key, tick) {
            0 => self.vx = self.vx.wrapping_neg(),
            1 => self.vy = self.vy.wrapping_neg(),
            _ => {}
        }
        let hit_x = self.move_axis(grid, Axis::X);
        let hit_y = self.move_axis(grid, Axis::Y);
        hit_x || hit_y
    }

    /// Tests the B cells the body would enter on `axis`: if any is `CLAIMED` or `TRAIL`, the
    /// velocity component inverts, and otherwise the body moves one cell. A cell outside the grid
    /// blocks like the border, though the border keeps the body from ever reaching it.
    fn move_axis(&mut self, grid: &Grid, axis: Axis) -> bool {
        let (along, across, velocity) = match axis {
            Axis::X => (self.pos.x, self.pos.y, self.vx),
            Axis::Y => (self.pos.y, self.pos.x, self.vy),
        };
        let edge = if velocity > 0 {
            along.checked_add(self.size)
        } else {
            along.checked_sub(1)
        };
        let (mut blocked, mut trail) = (false, false);
        for offset in 0..self.size {
            let cell = edge
                .zip(across.checked_add(offset))
                .and_then(|(edge, across)| {
                    grid.at(match axis {
                        Axis::X => Pos { x: edge, y: across },
                        Axis::Y => Pos { x: across, y: edge },
                    })
                });
            match cell {
                Some(Cell::Trail) => (blocked, trail) = (true, true),
                Some(Cell::Claimed) | None => blocked = true,
                Some(Cell::Empty | Cell::BossZone) => {}
            }
        }
        match (blocked, along.checked_add_signed(velocity), axis) {
            (false, Some(next), Axis::X) => self.pos.x = next,
            (false, Some(next), Axis::Y) => self.pos.y = next,
            (_, _, Axis::X) => self.vx = self.vx.wrapping_neg(),
            (_, _, Axis::Y) => self.vy = self.vy.wrapping_neg(),
        }
        trail
    }
}

/// r = fmix32(key XOR tick) >> 26, from 0 to 63: 0 inverts vx and 1 inverts vy (R4.6-7).
pub fn deflection_draw(key: u32, tick: u16) -> u32 {
    fmix32(key ^ u32::from(tick)) >> 26
}

/// The 32-bit finalizer of MurmurHash3, with wrapping multiplications (R4.6-9).
pub fn fmix32(mut h: u32) -> u32 {
    h ^= h >> 16;
    h = h.wrapping_mul(0x85eb_ca6b);
    h ^= h >> 13;
    h = h.wrapping_mul(0xc2b2_ae35);
    h ^ (h >> 16)
}
