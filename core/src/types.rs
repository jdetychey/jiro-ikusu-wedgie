use crate::consts::{CELLS, W};

/// A cell state [R4.1-2].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Cell {
    Empty = 0,
    Claimed = 1,
    Trail = 2,
    BossZone = 3,
}

/// A cell position; (0, 0) is the top-left corner [R4.1-1].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pos {
    pub x: u8,
    pub y: u8,
}

impl Pos {
    /// The position (x + dx, y + dy), or `None` if it is outside the grid.
    pub fn offset(self, dx: i8, dy: i8) -> Option<Self> {
        let pos = Self {
            x: self.x.checked_add_signed(dx)?,
            y: self.y.checked_add_signed(dy)?,
        };
        (pos.x < W && pos.y < W).then_some(pos)
    }

    /// On the border: x or y is 0 or 59 [R4.1-7].
    pub fn is_border(self) -> bool {
        self.x == 0 || self.y == 0 || self.x == W - 1 || self.y == W - 1
    }
}

/// A direction, in the order of the input mask bits and of direction priority [R4.5-1].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    Up,
    Down,
    Left,
    Right,
}

impl Dir {
    /// The order in which a newly held direction is chosen [R4.5-1].
    pub const PRIORITY: [Self; 4] = [Self::Up, Self::Down, Self::Left, Self::Right];

    /// The step on the grid: up is y − 1, as README §4.1 orients it [R4.1-1].
    pub fn offset(self) -> (i8, i8) {
        match self {
            Self::Up => (0, -1),
            Self::Down => (0, 1),
            Self::Left => (-1, 0),
            Self::Right => (1, 0),
        }
    }

    /// A quarter turn clockwise: E for N, S for E, W for S, N for W (PIN-06).
    pub fn clockwise(self) -> Self {
        match self {
            Self::Up => Self::Right,
            Self::Right => Self::Down,
            Self::Down => Self::Left,
            Self::Left => Self::Up,
        }
    }

    pub fn mask(self) -> InputMask {
        match self {
            Self::Up => InputMask::UP,
            Self::Down => InputMask::DOWN,
            Self::Left => InputMask::LEFT,
            Self::Right => InputMask::RIGHT,
        }
    }
}

/// The input mask of README §6.1. Bit 7 is reserved, so a mask never has it set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InputMask(pub(crate) u8);

impl InputMask {
    pub const NONE: Self = Self(0);
    pub const UP: Self = Self(1 << 0);
    pub const DOWN: Self = Self(1 << 1);
    pub const LEFT: Self = Self(1 << 2);
    pub const RIGHT: Self = Self(1 << 3);
    pub const A: Self = Self(1 << 4);
    pub const B: Self = Self(1 << 5);
    pub const X: Self = Self(1 << 6);
    const RESERVED: u8 = 1 << 7;

    /// The mask of these bits, or `None` if the reserved bit 7 is set.
    pub const fn new(bits: u8) -> Option<Self> {
        if bits & Self::RESERVED == 0 {
            Some(Self(bits))
        } else {
            None
        }
    }

    pub const fn bits(self) -> u8 {
        self.0
    }

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

/// The index `y × 60 + x` of a cell inside the grid, so always below `CELLS` [R4.1-1].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CellIndex(u16);

impl CellIndex {
    /// Every cell, in row-major order.
    pub fn all() -> impl Iterator<Item = Self> {
        (0..CELLS as u16).map(Self)
    }

    /// The index of `pos`, or `None` if `pos` is outside the grid.
    pub fn new(pos: Pos) -> Option<Self> {
        if pos.x >= W || pos.y >= W {
            return None;
        }
        u16::from(pos.y)
            .checked_mul(u16::from(W))
            .and_then(|row| row.checked_add(u16::from(pos.x)))
            .map(Self)
    }

    /// The index stored in a cell stack, if it is a cell's.
    pub(crate) fn from_raw(raw: u16) -> Option<Self> {
        (usize::from(raw) < CELLS).then_some(Self(raw))
    }

    pub(crate) fn raw(self) -> u16 {
        self.0
    }

    pub fn get(self) -> usize {
        usize::from(self.0)
    }

    pub fn pos(self) -> Pos {
        const WIDTH: u16 = W as u16;
        Pos {
            x: (self.0 % WIDTH) as u8,
            y: (self.0 / WIDTH) as u8,
        }
    }
}
