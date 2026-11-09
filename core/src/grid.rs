use core::num::{NonZeroU16, NonZeroU32};

use crate::consts::{BP, CAPTURE_STACK, CELLS};
use crate::level::Level;
use crate::types::{Cell, CellIndex, Pos};

const NEIGHBORS_4: [(i8, i8); 4] = [(0, -1), (1, 0), (0, 1), (-1, 0)];

const NEIGHBORS_8: [(i8, i8); 8] = [
    (-1, -1),
    (0, -1),
    (1, -1),
    (-1, 0),
    (1, 0),
    (-1, 1),
    (0, 1),
    (1, 1),
];

/// The 60 × 60 cells, row-major [R4.1-1].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Grid {
    cells: [Cell; CELLS],
}

impl Grid {
    /// The border and the level's obstacles are `CLAIMED`, every other cell is `EMPTY`
    /// [R4.1-7].
    pub fn level_start(level: &Level) -> Self {
        let mut cells = [Cell::Empty; CELLS];
        let cells_in_order = cells.iter_mut().zip(CellIndex::all());
        for ((cell, index), obstacle) in cells_in_order.zip(level.obstacle_bits()) {
            if obstacle || index.pos().is_border() {
                *cell = Cell::Claimed;
            }
        }
        Self { cells }
    }

    #[allow(clippy::indexing_slicing, reason = "a CellIndex is below CELLS")]
    pub fn get(&self, index: CellIndex) -> Cell {
        self.cells[index.get()]
    }

    #[allow(clippy::indexing_slicing, reason = "a CellIndex is below CELLS")]
    pub fn set(&mut self, index: CellIndex, cell: Cell) {
        self.cells[index.get()] = cell;
    }

    /// The cell at `pos`, or `None` outside the grid.
    pub fn at(&self, pos: Pos) -> Option<Cell> {
        CellIndex::new(pos).map(|index| self.get(index))
    }

    /// A `CLAIMED` cell with an `EMPTY` or `TRAIL` cell among its 8 neighbors
    /// [R4.2-1, R4.2-2, R4.2-3].
    pub fn is_rail(&self, pos: Pos) -> bool {
        self.at(pos) == Some(Cell::Claimed)
            && NEIGHBORS_8.iter().any(|&(dx, dy)| {
                let neighbor = pos.offset(dx, dy).and_then(|next| self.at(next));
                matches!(neighbor, Some(Cell::Empty | Cell::Trail))
            })
    }

    /// Marks `BOSS_ZONE` the seeds and every `EMPTY` cell 4-connected to them, with `TRAIL` and
    /// `CLAIMED` as walls, by the stack fill of README §4.8 [R4.8-2, R4.8-3]. The seeds must be
    /// `EMPTY`. A cell is marked when it is pushed, so it is pushed at most once, and only
    /// `EMPTY` cells, which are interior cells, are pushed: `stack` never overflows. `None` would
    /// mean it did.
    pub fn mark_boss_zone(
        &mut self,
        seeds: impl IntoIterator<Item = CellIndex>,
        stack: &mut [u16; CAPTURE_STACK],
    ) -> Option<()> {
        let mut len: usize = 0;
        for seed in seeds {
            self.set(seed, Cell::BossZone);
            *stack.get_mut(len)? = seed.raw();
            len = len.checked_add(1)?;
        }
        while let Some(top) = len.checked_sub(1) {
            let cell = CellIndex::from_raw(*stack.get(top)?)?;
            len = top;
            for (dx, dy) in NEIGHBORS_4 {
                let Some(next) = cell.pos().offset(dx, dy).and_then(CellIndex::new) else {
                    continue;
                };
                if self.get(next) == Cell::Empty {
                    self.set(next, Cell::BossZone);
                    *stack.get_mut(len)? = next.raw();
                    len = len.checked_add(1)?;
                }
            }
        }
        Some(())
    }

    pub fn count(&self, cell: Cell) -> u16 {
        self.cells.iter().filter(|&&other| other == cell).count() as u16
    }

    pub fn replace(&mut self, from: Cell, to: Cell) {
        for cell in &mut self.cells {
            if *cell == from {
                *cell = to;
            }
        }
    }
}

/// captured × 10,000 / claimable, with a single floor division [R4.1-8].
pub fn claim_bp(captured: u16, claimable: NonZeroU16) -> u32 {
    const WHOLE: u32 = BP as u32;
    u32::from(captured) * WHOLE / NonZeroU32::from(claimable)
}
