//! DirtyCells reports exactly the cells that changed since the last update.

use jiro_core::grid::Grid;
use jiro_core::level::{Level, Rect};
use jiro_core::types::{Cell, CellIndex, Pos};
use jiro_shell::dirty::{DirtyCells, Shadow};

fn grid() -> Grid {
    Grid::level_start(&Level {
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
    })
}

#[test]
fn first_update_reports_every_cell() {
    assert_eq!(DirtyCells::new().update(&grid()).len(), 3_600);
}

#[test]
fn later_updates_report_the_changed_cells_in_row_major_order() {
    let mut grid = grid();
    let mut dirty = DirtyCells::new();
    dirty.update(&grid);
    for (x, y) in [(31, 58), (30, 58)] {
        grid.set(CellIndex::new(Pos { x, y }).unwrap(), Cell::Trail);
    }
    assert_eq!(dirty.update(&grid), &[58 * 60 + 30, 58 * 60 + 31]);
}

#[test]
fn update_without_changes_reports_nothing() {
    let grid = grid();
    let mut dirty = DirtyCells::new();
    dirty.update(&grid);
    assert!(dirty.update(&grid).is_empty());
}

#[test]
fn shadow_reports_the_state_of_each_changed_cell() {
    let mut grid = grid();
    let mut shadow = Shadow::new();
    shadow.update(&grid, |_, _| {});
    grid.set(CellIndex::new(Pos { x: 30, y: 58 }).unwrap(), Cell::Trail);
    let mut reported = Vec::new();
    shadow.update(&grid, |cell, state| reported.push((cell, state)));
    assert_eq!(
        reported,
        [(CellIndex::new(Pos { x: 30, y: 58 }).unwrap(), Cell::Trail)]
    );
}
