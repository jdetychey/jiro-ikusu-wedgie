//! Core types: the input mask bits of README §6.1, cell states and the bounded cell index of
//! README §4.1.

use jiro_core::types::{Cell, CellIndex, InputMask, Pos};

#[test]
fn input_mask_bits_are_the_readme_bits() {
    let bits = [
        (InputMask::UP, 0),
        (InputMask::DOWN, 1),
        (InputMask::LEFT, 2),
        (InputMask::RIGHT, 3),
        (InputMask::A, 4),
        (InputMask::B, 5),
        (InputMask::X, 6),
    ];
    for (mask, bit) in bits {
        assert_eq!(mask.bits(), 1 << bit, "bit {bit}");
    }
}

#[test]
fn input_mask_accepts_bits_0_to_6() {
    assert_eq!(InputMask::new(0x7f).map(InputMask::bits), Some(0x7f));
}

#[test]
fn input_mask_rejects_reserved_bit_7() {
    assert_eq!(InputMask::new(0x80), None);
}

#[test]
fn input_mask_contains_only_its_subsets() {
    let up_and_a = InputMask::new(0x11).unwrap();
    let cases = [
        (InputMask::UP, true),
        (InputMask::A, true),
        (up_and_a, true),
        (InputMask::NONE, true),
        (InputMask::DOWN, false),
        (InputMask::new(0x13).unwrap(), false),
    ];
    for (other, expected) in cases {
        assert_eq!(up_and_a.contains(other), expected, "{other:?}");
    }
}

/// [R4.1-2] [R4.1-3] [R4.1-4] [R4.1-5] [R4.1-6]
#[test]
fn cell_values_are_the_readme_values() {
    let values = [
        (Cell::Empty, 0),
        (Cell::Claimed, 1),
        (Cell::Trail, 2),
        (Cell::BossZone, 3),
    ];
    for (cell, value) in values {
        assert_eq!(cell as u8, value, "{cell:?}");
    }
}

/// [R4.1-1]
#[test]
fn cell_index_is_y_times_60_plus_x() {
    let cases = [
        ((0, 0), 0),
        ((59, 0), 59),
        ((0, 1), 60),
        ((30, 59), 3_570),
        ((59, 59), 3_599),
    ];
    for ((x, y), index) in cases {
        assert_eq!(
            CellIndex::new(Pos { x, y }).map(CellIndex::get),
            Some(index),
            "({x}, {y})"
        );
    }
}

#[test]
fn cell_index_rejects_positions_outside_the_grid() {
    for (x, y) in [(60, 0), (0, 60), (60, 59), (59, 60), (255, 255)] {
        assert_eq!(CellIndex::new(Pos { x, y }), None, "({x}, {y})");
    }
}
