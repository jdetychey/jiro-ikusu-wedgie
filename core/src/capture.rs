//! Capture (README §4.8): a cut that reaches `CLAIMED` claims every region the boss is not in.

use crate::consts::CAPTURE_STACK;
use crate::grid::Grid;
use crate::types::{Cell, CellIndex};

/// Marks the boss's body and the `EMPTY` cells 4-connected to it as `BOSS_ZONE`, then makes one
/// pass: `BOSS_ZONE` becomes `EMPTY`, and `EMPTY` and `TRAIL` become `CLAIMED` [R4.8-1 to
/// R4.8-4]. Returns the capture size, the number of cells that became `CLAIMED`. `body` holds
/// the body cells of every boss. `None` only if `stack` overflowed, which
/// `Grid::mark_boss_zone` rules out.
pub fn capture(
    grid: &mut Grid,
    body: impl IntoIterator<Item = CellIndex>,
    stack: &mut [u16; CAPTURE_STACK],
) -> Option<u16> {
    grid.mark_boss_zone(body, stack)?;
    let mut captured: u16 = 0;
    for cell in CellIndex::all() {
        match grid.get(cell) {
            Cell::BossZone => grid.set(cell, Cell::Empty),
            Cell::Empty | Cell::Trail => {
                grid.set(cell, Cell::Claimed);
                captured = captured.checked_add(1)?;
            }
            Cell::Claimed => {}
        }
    }
    Some(captured)
}
