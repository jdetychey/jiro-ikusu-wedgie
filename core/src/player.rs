//! The player (README §4.5): direction, rail movement, cuts, the fast cut and the slow shield,
//! at step 3 of the tick (R4.4-8).

use crate::boss::Boss;
use crate::capture::capture;
use crate::consts::{CAPTURE_STACK, PLAYER_START};
use crate::grid::Grid;
use crate::types::{Cell, CellIndex, Dir, InputMask, Pos};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Player {
    pub pos: Pos,
    pub cutting: bool,
    /// The cell the last cut left from; it keeps its value between cuts (R4.3-5).
    pub cut_start: Pos,
    pub lives: u8,
    pub shield: u8,
    /// Protected ticks left (R4.3-5); the hits of step 6 set and count it down.
    pub invulnerable: u8,
    /// The direction resolved at the last step 3 (R4.5-1, PIN-05).
    pub direction: Option<Dir>,
    /// The tick on which B last went from released to held, 0 before any press (R4.3-5).
    pub press_tick: u16,
}

/// What step 3 did, for the steps after it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Moves {
    /// P0, the player's cell at the start of step 3 (PIN-15).
    pub from: Pos,
    /// P1 … Pn, the cells actually entered, n = 0, 1 or 2 (PIN-15).
    pub entered: [Option<Pos>; 2],
    /// The player tried to step onto the boss's body (R4.9-3).
    pub hit: bool,
    /// The capture size, if a cut closed this tick (R4.5-5).
    pub captured: Option<u16>,
}

/// The outcome of one sub-step.
enum SubStep {
    /// The player entered an `EMPTY` cell while cutting: a fast cut may take one more sub-step.
    Cut(Pos),
    /// The player entered a cell, and the tick's movement ends.
    Entered(Pos),
    /// The cut closed onto this cell, with this capture size.
    Closed(Pos, u16),
    Hit,
    Ignored,
}

impl Player {
    /// The player at level start (PIN-01).
    pub fn start(lives: u8) -> Self {
        Self {
            pos: PLAYER_START,
            cutting: false,
            cut_start: PLAYER_START,
            lives,
            shield: 1,
            invulnerable: 0,
            direction: None,
            press_tick: 0,
        }
    }

    /// Step 1's part for the player: B going from released to held sets `press_tick`
    /// (R4.3-5, PIN-03).
    pub fn input(&mut self, previous: InputMask, current: InputMask, tick: u16) {
        if current.contains(InputMask::B) && !previous.contains(InputMask::B) {
            self.press_tick = tick;
        }
    }

    /// Step 3: resolves the direction, then moves 0, 1 or 2 sub-steps. A cut that closes
    /// captures at once through `stack`. `None` only if the capture's stack overflowed, which
    /// `Grid::mark_boss_zone` rules out.
    pub fn step(
        &mut self,
        grid: &mut Grid,
        boss: &Boss,
        mask: InputMask,
        tick: u16,
        stack: &mut [u16; CAPTURE_STACK],
    ) -> Option<Moves> {
        self.direction = self.resolve_direction(mask);
        let mut moves = Moves {
            from: self.pos,
            entered: [None, None],
            hit: false,
            captured: None,
        };
        let Some(direction) = self.direction else {
            return Some(moves);
        };
        for entered in moves
            .entered
            .iter_mut()
            .take(self.sub_steps(mask, tick).into())
        {
            match self.sub_step(grid, boss, direction, stack)? {
                SubStep::Cut(pos) => *entered = Some(pos),
                SubStep::Entered(pos) => {
                    *entered = Some(pos);
                    break;
                }
                SubStep::Closed(pos, size) => {
                    *entered = Some(pos);
                    moves.captured = Some(size);
                    break;
                }
                SubStep::Hit => {
                    moves.hit = true;
                    break;
                }
                SubStep::Ignored => break,
            }
        }
        Some(moves)
    }

    /// Erases the trail, and a player still cutting returns to the cut start and stops cutting:
    /// a commit (R4.4-7) and a lost life (R4.9-17) both discard the unfinished cut this way.
    pub fn abandon_cut(&mut self, grid: &mut Grid) {
        grid.replace(Cell::Trail, Cell::Empty);
        if self.cutting {
            self.pos = self.cut_start;
            self.cutting = false;
        }
    }

    /// Keeps the previous direction while it is held, otherwise takes the first held one in the
    /// order up, down, left, right [R4.5-1].
    fn resolve_direction(&self, mask: InputMask) -> Option<Dir> {
        self.direction
            .filter(|previous| mask.contains(previous.mask()))
            .or_else(|| {
                Dir::PRIORITY
                    .into_iter()
                    .find(|dir| mask.contains(dir.mask()))
            })
    }

    /// B held: one sub-step on the ticks where (tick − press_tick) % 2 == 0, else none, and B
    /// wins over A [R4.5-7]. Otherwise two when cutting at the start of the tick with A held
    /// [R4.5-6], else one.
    fn sub_steps(&self, mask: InputMask, tick: u16) -> u8 {
        if mask.contains(InputMask::B) {
            // press_tick ≤ tick, and 2^16 is even, so the wrapping difference has the right parity.
            u8::from(tick.wrapping_sub(self.press_tick).is_multiple_of(2))
        } else if self.cutting && mask.contains(InputMask::A) {
            2
        } else {
            1
        }
    }

    fn sub_step(
        &mut self,
        grid: &mut Grid,
        boss: &Boss,
        direction: Dir,
        stack: &mut [u16; CAPTURE_STACK],
    ) -> Option<SubStep> {
        let (dx, dy) = direction.offset();
        let Some(target) = self.pos.offset(dx, dy) else {
            return Some(SubStep::Ignored);
        };
        if boss.body().any(|cell| cell == target) {
            return Some(SubStep::Hit);
        }
        let Some(index) = CellIndex::new(target) else {
            return Some(SubStep::Ignored);
        };
        Some(match (self.cutting, grid.get(index)) {
            (_, Cell::Trail | Cell::BossZone) => SubStep::Ignored,
            (true, Cell::Empty) => {
                grid.set(index, Cell::Trail);
                self.pos = target;
                SubStep::Cut(target)
            }
            (true, Cell::Claimed) => {
                self.pos = target;
                self.cutting = false;
                let body = boss.body().filter_map(CellIndex::new);
                SubStep::Closed(target, capture(grid, body, stack)?)
            }
            (false, Cell::Empty) => {
                grid.set(index, Cell::Trail);
                self.cut_start = self.pos;
                self.pos = target;
                self.cutting = true;
                self.invulnerable = 0;
                SubStep::Entered(target)
            }
            (false, Cell::Claimed) if !grid.is_rail(self.pos) || grid.is_rail(target) => {
                self.pos = target;
                SubStep::Entered(target)
            }
            (false, Cell::Claimed) => SubStep::Ignored,
        })
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use std::vec::Vec;

    use super::*;
    use crate::level::{Level, Rect};

    fn level(obstacles: &[(u8, u8)]) -> Level {
        let mut level = Level {
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
        };
        for &(x, y) in obstacles {
            let cell = usize::from(y) * 60 + usize::from(x);
            level.obstacles[cell / 8] |= 0x80 >> (cell % 8);
        }
        level
    }

    fn grid(obstacles: &[(u8, u8)]) -> Grid {
        Grid::level_start(&level(obstacles))
    }

    fn boss_at(x: u8, y: u8) -> Boss {
        Boss {
            pos: Pos { x, y },
            size: 5,
            vx: 1,
            vy: 1,
            period: 2,
            deflection_key: 0,
        }
    }

    fn at(x: u8, y: u8) -> Pos {
        Pos { x, y }
    }

    fn player_at(x: u8, y: u8) -> Player {
        Player {
            pos: at(x, y),
            ..Player::start(3)
        }
    }

    fn held(masks: &[InputMask]) -> InputMask {
        InputMask::new(masks.iter().fold(0, |bits, mask| bits | mask.bits())).unwrap()
    }

    fn step(player: &mut Player, grid: &mut Grid, mask: InputMask, tick: u16) -> Moves {
        player
            .step(grid, &boss_at(40, 40), mask, tick, &mut [0; CAPTURE_STACK])
            .unwrap()
    }

    fn cell(grid: &Grid, x: u8, y: u8) -> Cell {
        grid.at(at(x, y)).unwrap()
    }

    /// [PIN-01]
    #[test]
    fn player_starts_on_30_59_with_the_level_lives() {
        let player = Player::start(4);
        assert_eq!(
            (
                player.pos,
                player.cutting,
                player.cut_start,
                player.lives,
                player.shield,
                player.invulnerable,
                player.direction,
                player.press_tick
            ),
            (at(30, 59), false, at(30, 59), 4, 1, 0, None, 0)
        );
    }

    /// [R4.5-1] Two held directions keep the previous one.
    #[test]
    fn player_keeps_the_previous_direction_while_it_is_held() {
        let mut grid = grid(&[]);
        let mut player = Player {
            direction: Some(Dir::Left),
            ..Player::start(3)
        };
        step(
            &mut player,
            &mut grid,
            held(&[InputMask::UP, InputMask::LEFT]),
            0,
        );
        assert_eq!(
            (player.direction, player.pos),
            (Some(Dir::Left), at(29, 59))
        );
    }

    /// [R4.5-1] With no previous direction, the first held one in the order up, down, left,
    /// right, for each place in the order.
    #[test]
    fn player_takes_up_down_left_right_in_that_order() {
        let (up, down, left, right) = (
            InputMask::UP,
            InputMask::DOWN,
            InputMask::LEFT,
            InputMask::RIGHT,
        );
        let cases = [
            (held(&[right, left, down, up]), Dir::Up),
            (held(&[right, left, down]), Dir::Down),
            (held(&[right, left]), Dir::Left),
            (right, Dir::Right),
        ];
        for (mask, expected) in cases {
            let mut grid = grid(&[]);
            let mut player = Player::start(3);
            step(&mut player, &mut grid, mask, 0);
            assert_eq!(player.direction, Some(expected), "{mask:?}");
        }
    }

    /// [R4.5-1]
    #[test]
    fn player_with_no_direction_stands_still() {
        let mut grid = grid(&[]);
        let mut player = Player {
            direction: Some(Dir::Left),
            ..Player::start(3)
        };
        let moves = step(&mut player, &mut grid, InputMask::NONE, 0);
        assert_eq!(
            (player.direction, player.pos, moves.entered),
            (None, at(30, 59), [None, None])
        );
    }

    /// [PIN-05]
    #[test]
    fn player_keeps_the_resolved_direction_when_the_move_does_not_happen() {
        let mut grid = grid(&[]);
        let mut player = Player::start(3);
        let moves = step(&mut player, &mut grid, InputMask::DOWN, 0);
        assert_eq!(
            (player.direction, player.pos, moves.hit),
            (Some(Dir::Down), at(30, 59), false)
        );
    }

    /// [R4.5-2] (D3)
    #[test]
    fn player_moves_from_rail_cell_to_rail_cell() {
        let mut grid = grid(&[]);
        let mut player = Player::start(3);
        step(&mut player, &mut grid, InputMask::RIGHT, 0);
        assert_eq!(player.pos, at(31, 59));
    }

    /// [R4.5-2] (D3): (11, 0) is `CLAIMED` but all its neighbors are too.
    #[test]
    fn player_on_the_rail_does_not_step_onto_a_claimed_cell_off_the_rail() {
        let block: Vec<(u8, u8)> = (10..=12)
            .flat_map(|x| (1..=3).map(move |y| (x, y)))
            .collect();
        let mut grid = grid(&block);
        let mut player = player_at(10, 0);
        let moves = step(&mut player, &mut grid, InputMask::RIGHT, 0);
        assert_eq!((player.pos, moves.hit), (at(10, 0), false));
    }

    /// [R4.5-2] and R4.3-5: the cut starts from the cell left, (31, 59), not the cut start the
    /// player had, lays `TRAIL`, and ends invulnerability.
    #[test]
    fn player_stepping_into_empty_starts_a_cut() {
        let mut grid = grid(&[]);
        let mut player = Player {
            invulnerable: 12,
            ..player_at(31, 59)
        };
        step(&mut player, &mut grid, InputMask::UP, 0);
        assert_eq!(
            (
                player.pos,
                player.cutting,
                player.cut_start,
                player.invulnerable,
                cell(&grid, 31, 58)
            ),
            (at(31, 58), true, at(31, 59), 0, Cell::Trail)
        );
    }

    /// [R4.5-3]: the cut closes at (14, 3), whose neighbors are all `CLAIMED` once the capture has
    /// claimed the trail; off the rail, the player may step onto any `CLAIMED` cell: onto (14, 4),
    /// which is off the rail too, then onto the rail cell (14, 5).
    #[test]
    fn player_stranded_off_the_rail_by_its_capture_walks_over_claimed_cells_back() {
        let mut obstacles: Vec<(u8, u8)> = (10..=14)
            .flat_map(|x| (1..=2).map(move |y| (x, y)))
            .collect();
        obstacles.extend([
            (10, 3),
            (15, 2),
            (15, 3),
            (15, 4),
            (14, 4),
            (13, 4),
            (14, 3),
            (13, 5),
            (14, 5),
            (15, 5),
        ]);
        let mut grid = grid(&obstacles);
        let mut player = player_at(10, 3);
        for _ in 0..4 {
            step(&mut player, &mut grid, InputMask::RIGHT, 0);
        }
        let state =
            |player: &Player, grid: &Grid| (player.pos, player.cutting, grid.is_rail(player.pos));
        let stranded = state(&player, &grid);
        step(&mut player, &mut grid, InputMask::DOWN, 1);
        let between = state(&player, &grid);
        step(&mut player, &mut grid, InputMask::DOWN, 2);
        assert_eq!(
            [stranded, between, state(&player, &grid)],
            [
                (at(14, 3), false, false),
                (at(14, 4), false, false),
                (at(14, 5), false, true)
            ]
        );
    }

    /// [R4.5-4] (D4)
    #[test]
    fn player_cannot_step_onto_its_own_trail() {
        let mut grid = grid(&[]);
        let mut player = Player::start(3);
        step(&mut player, &mut grid, InputMask::UP, 0);
        step(&mut player, &mut grid, InputMask::UP, 1);
        let moves = step(&mut player, &mut grid, InputMask::DOWN, 2);
        assert_eq!(
            (player.pos, moves.hit, moves.entered),
            (at(30, 57), false, [None, None])
        );
    }

    /// [R4.9-3]
    #[test]
    fn player_stepping_onto_the_boss_body_is_hit_and_does_not_move() {
        let mut grid = grid(&[]);
        let mut player = Player {
            pos: at(30, 55),
            cutting: true,
            ..Player::start(3)
        };
        let boss = boss_at(28, 50);
        let moves = player
            .step(&mut grid, &boss, InputMask::UP, 0, &mut [0; CAPTURE_STACK])
            .unwrap();
        assert_eq!(
            (moves.hit, player.pos, moves.entered, player.direction),
            (true, at(30, 55), [None, None], Some(Dir::Up))
        );
    }

    /// [R4.5-5]: stepping from `TRAIL` onto `CLAIMED` captures and ends the cut.
    #[test]
    fn player_closing_a_cut_captures() {
        let mut grid = grid(&[]);
        let mut player = player_at(2, 59);
        step(&mut player, &mut grid, InputMask::UP, 0);
        step(&mut player, &mut grid, InputMask::LEFT, 1);
        let moves = step(&mut player, &mut grid, InputMask::LEFT, 2);
        assert_eq!(
            (
                moves.captured,
                player.pos,
                player.cutting,
                cell(&grid, 1, 58),
                cell(&grid, 2, 58)
            ),
            (Some(2), at(0, 58), false, Cell::Claimed, Cell::Claimed)
        );
    }

    /// [R4.5-6]
    #[test]
    fn player_fast_cut_takes_two_sub_steps() {
        let mut grid = grid(&[]);
        let mut player = Player::start(3);
        step(&mut player, &mut grid, InputMask::UP, 0);
        let moves = step(
            &mut player,
            &mut grid,
            held(&[InputMask::UP, InputMask::A]),
            1,
        );
        assert_eq!(
            (
                moves.from,
                moves.entered,
                cell(&grid, 30, 57),
                cell(&grid, 30, 56)
            ),
            (
                at(30, 58),
                [Some(at(30, 57)), Some(at(30, 56))],
                Cell::Trail,
                Cell::Trail
            )
        );
    }

    /// [R4.5-6]: a cut starting this tick moves at normal speed.
    #[test]
    fn player_fast_cut_starts_only_when_already_cutting() {
        let mut grid = grid(&[]);
        let mut player = Player::start(3);
        let moves = step(
            &mut player,
            &mut grid,
            held(&[InputMask::UP, InputMask::A]),
            0,
        );
        assert_eq!(moves.entered, [Some(at(30, 58)), None]);
    }

    /// [PIN-04]: a fast cut that closes on its first sub-step, onto the obstacle (30, 50), stops
    /// there, although a second sub-step could enter the open cell (30, 49) beyond it.
    #[test]
    fn player_fast_cut_closing_on_its_first_sub_step_stops() {
        let mut grid = grid(&[(30, 50)]);
        let mut player = Player::start(3);
        for tick in 0..8 {
            step(&mut player, &mut grid, InputMask::UP, tick);
        }
        let moves = step(
            &mut player,
            &mut grid,
            held(&[InputMask::UP, InputMask::A]),
            8,
        );
        assert_eq!(
            (
                moves.entered,
                moves.captured.is_some(),
                player.pos,
                player.cutting,
                cell(&grid, 30, 49)
            ),
            (
                [Some(at(30, 50)), None],
                true,
                at(30, 50),
                false,
                Cell::Empty
            )
        );
    }

    /// [PIN-04]: a first sub-step blocked by the own trail ends the tick.
    #[test]
    fn player_fast_cut_blocked_by_its_trail_stops() {
        let mut grid = grid(&[]);
        let mut player = Player::start(3);
        step(&mut player, &mut grid, InputMask::UP, 0);
        step(&mut player, &mut grid, InputMask::UP, 1);
        let moves = step(
            &mut player,
            &mut grid,
            held(&[InputMask::DOWN, InputMask::A]),
            2,
        );
        assert_eq!((moves.entered, player.pos), ([None, None], at(30, 57)));
    }

    /// [PIN-04]: a first sub-step onto the boss's body ends the tick.
    #[test]
    fn player_fast_cut_hitting_the_boss_stops() {
        let mut grid = grid(&[]);
        let mut player = Player {
            pos: at(30, 55),
            cutting: true,
            ..Player::start(3)
        };
        let moves = player
            .step(
                &mut grid,
                &boss_at(28, 50),
                held(&[InputMask::UP, InputMask::A]),
                0,
                &mut [0; CAPTURE_STACK],
            )
            .unwrap();
        assert_eq!((moves.hit, moves.entered), (true, [None, None]));
    }

    /// [R4.5-7]
    #[test]
    fn player_with_the_shield_moves_every_second_tick_from_the_press() {
        let mut grid = grid(&[]);
        let mut player = Player {
            press_tick: 3,
            ..Player::start(3)
        };
        let moved: Vec<bool> = (3..7)
            .map(|tick| {
                let before = player.pos;
                step(
                    &mut player,
                    &mut grid,
                    held(&[InputMask::RIGHT, InputMask::B]),
                    tick,
                );
                player.pos != before
            })
            .collect();
        assert_eq!(moved, [true, false, true, false]);
    }

    /// [R4.5-7]: with A and B held, B wins.
    #[test]
    fn player_with_a_and_b_held_takes_one_sub_step() {
        let mut grid = grid(&[]);
        let mut player = Player::start(3);
        step(&mut player, &mut grid, InputMask::UP, 0);
        let shield_and_fast = held(&[InputMask::UP, InputMask::A, InputMask::B]);
        player.input(InputMask::UP, shield_and_fast, 1);
        let moves = step(&mut player, &mut grid, shield_and_fast, 1);
        assert_eq!(moves.entered, [Some(at(30, 57)), None]);
    }

    /// [PIN-03]
    #[test]
    fn player_press_tick_is_0_when_b_is_held_at_tick_0() {
        let mut player = Player {
            press_tick: 9,
            ..Player::start(3)
        };
        player.input(InputMask::NONE, InputMask::B, 0);
        assert_eq!(player.press_tick, 0);
    }

    /// R4.3-5: only B going from released to held sets `press_tick`.
    #[test]
    fn player_press_tick_follows_new_presses_of_b_only() {
        let mut player = Player::start(3);
        player.input(InputMask::NONE, InputMask::B, 7);
        player.input(InputMask::B, held(&[InputMask::B, InputMask::UP]), 9);
        assert_eq!(player.press_tick, 7);
    }

    /// [R4.5-8]
    #[test]
    fn player_may_stop_mid_cut() {
        let mut grid = grid(&[]);
        let mut player = Player::start(3);
        step(&mut player, &mut grid, InputMask::UP, 0);
        step(&mut player, &mut grid, InputMask::NONE, 1);
        assert_eq!(
            (player.pos, player.cutting, cell(&grid, 30, 58)),
            (at(30, 58), true, Cell::Trail)
        );
    }
}
