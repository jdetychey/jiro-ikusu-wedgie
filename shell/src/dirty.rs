//! The dirty-cell diff: the cells whose state changed since the last redraw, found by comparing
//! the core grid with a shadow copy, in row-major order. `Shadow` is the diff itself, which the
//! wedgie's native module runs with its own drawing; `DirtyCells` keeps the changed indices for
//! the browser.

use jiro_core::consts::CELLS;
use jiro_core::grid::Grid;
use jiro_core::types::{Cell, CellIndex};

/// One byte per cell, nothing else: the wedgie's native module uses a caller's buffer as one.
#[repr(transparent)]
pub struct Shadow {
    /// Each cell's state plus one, as last reported; 0 for a cell never reported. All zero at
    /// first, so a static `Shadow` needs no initialized data, and any byte value is a valid
    /// shadow: a value that matches no state only makes its cell reported again.
    cells: [u8; CELLS],
}

impl Default for Shadow {
    fn default() -> Self {
        Self::new()
    }
}

impl Shadow {
    /// A shadow that matches no grid, so the first `update` reports every cell.
    pub const fn new() -> Self {
        Self { cells: [0; CELLS] }
    }

    /// Compares `grid` with the shadow, calls `changed` with each cell that differs and its
    /// state, in row-major order, and copies those states into the shadow.
    pub fn update(&mut self, grid: &Grid, mut changed: impl FnMut(CellIndex, Cell)) {
        for (cell, shadow) in CellIndex::all().zip(self.cells.iter_mut()) {
            let state = grid.get(cell);
            let reported = (state as u8).saturating_add(1);
            if *shadow != reported {
                *shadow = reported;
                changed(cell, state);
            }
        }
    }
}

pub struct DirtyCells {
    shadow: Shadow,
    changed: [u16; CELLS],
    count: u16,
}

impl Default for DirtyCells {
    fn default() -> Self {
        Self::new()
    }
}

impl DirtyCells {
    /// A shadow that matches no grid, so the first `update` reports every cell.
    pub const fn new() -> Self {
        Self {
            shadow: Shadow::new(),
            changed: [0; CELLS],
            count: 0,
        }
    }

    /// Compares `grid` with the shadow, records the indices of the cells that differ, and
    /// copies them into the shadow. Returns their indices, in row-major order.
    pub fn update(&mut self, grid: &Grid) -> &[u16] {
        let Self {
            shadow,
            changed,
            count,
        } = self;
        *count = 0;
        shadow.update(grid, |cell, _| {
            if let (Some(slot), Ok(index)) = (
                changed.get_mut(usize::from(*count)),
                u16::try_from(cell.get()),
            ) {
                *slot = index;
                *count = count.saturating_add(1);
            }
        });
        self.changed()
    }

    /// The indices that the last `update` found.
    pub fn changed(&self) -> &[u16] {
        self.changed
            .get(..usize::from(self.count))
            .unwrap_or(&self.changed)
    }
}
