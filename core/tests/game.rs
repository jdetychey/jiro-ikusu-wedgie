//! A level in play (README §4.4, §4.9, §4.10): the hit sources, the minion hit predicate of
//! PIN-15, the resolution of hits at step 6, and the end conditions that `Game::step` reaches.
//! Each test arranges the state directly, starting at tick 1 with the boss parked, so that only
//! the arranged pieces move.

use jiro_core::boss::deflection_draw;
use jiro_core::game::{EndReason, Game};
use jiro_core::level::{Level, Rect};
use jiro_core::minion::{Minion, State};
use jiro_core::types::{Cell, CellIndex, Dir, InputMask, Pos};

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

/// The level at tick 1, with the boss parked at (40, 10): its period of 255 keeps it still
/// until tick 255.
fn game_on(level: &Level) -> Game {
    let mut game = Game::new(level, &[7; 32], 0).unwrap();
    game.ticks_used = 1;
    game.boss.pos = at(40, 10);
    game.boss.period = 255;
    game
}

fn game() -> Game {
    game_on(&level(&[]))
}

fn at(x: u8, y: u8) -> Pos {
    Pos { x, y }
}

fn mask(parts: &[InputMask]) -> InputMask {
    InputMask::new(parts.iter().fold(0, |bits, part| bits | part.bits())).unwrap()
}

fn paint(game: &mut Game, cells: &[(u8, u8)], cell: Cell) {
    for &(x, y) in cells {
        game.grid.set(CellIndex::new(at(x, y)).unwrap(), cell);
    }
}

/// The player cutting at `pos`, from `cut_start`, along `trail`.
fn cutting(game: &mut Game, pos: Pos, cut_start: Pos, trail: &[(u8, u8)]) {
    paint(game, trail, Cell::Trail);
    game.player.pos = pos;
    game.player.cut_start = cut_start;
    game.player.cutting = true;
}

/// A live minion in `slot` at `pos`, facing `heading`, moving every tick from tick 1 on, or
/// never when `period` is 255.
fn minion(game: &mut Game, slot: usize, pos: Pos, heading: Dir, period: u8) {
    game.minions.period = period;
    game.minions.slots[slot] = Some(Minion {
        spawn: pos,
        state: State::Live { pos, heading },
    });
}

/// The boss moving at every tick toward −x and −y from `pos`, with a deflection key that draws
/// no deflection on ticks 1 to 40.
fn boss_moving(game: &mut Game, pos: Pos) {
    game.boss.pos = pos;
    game.boss.period = 1;
    (game.boss.vx, game.boss.vy) = (-1, -1);
    game.boss.deflection_key = (0..)
        .find(|&key| (1..=40).all(|tick| deflection_draw(key, tick) >= 2))
        .unwrap();
}

fn lives_after(game: &mut Game, masks: &[InputMask]) -> u8 {
    for &mask in masks {
        game.step(mask).unwrap();
    }
    game.player.lives
}

/// [R4.9-2] [R4.4-9] The boss's leading edge on x reaches the trail the player just laid.
#[test]
fn boss_bumping_into_the_trail_hits() {
    let mut game = game();
    boss_moving(&mut game, at(31, 54));
    assert_eq!(lives_after(&mut game, &[InputMask::UP]), 2);
}

/// [R4.9-3] A step onto the boss's body is a hit, and the player stays.
#[test]
fn step_onto_the_boss_body_hits_and_does_not_move() {
    let mut game = game();
    game.boss.pos = at(28, 54);
    let lives = lives_after(&mut game, &[InputMask::UP]);
    assert_eq!((lives, game.player.pos), (2, at(30, 59)));
}

/// [R4.9-4] A minion walking onto a player who stands still hits.
#[test]
fn minion_walking_onto_the_player_hits() {
    let mut game = game();
    minion(&mut game, 0, at(31, 59), Dir::Left, 1);
    assert_eq!(lives_after(&mut game, &[InputMask::NONE]), 2);
}

/// [R4.9-9] Share: the minion ends on Pn, the cell the player moved into.
#[test]
fn minion_ending_on_the_players_new_cell_hits() {
    let mut game = game();
    minion(&mut game, 0, at(32, 59), Dir::Left, 1);
    assert_eq!(lives_after(&mut game, &[InputMask::RIGHT]), 2);
}

/// [R4.9-10] Swap on the rail: the player moves from (30, 59) to (31, 59) as the minion moves
/// from (31, 59) to (30, 59).
#[test]
fn minion_swapping_cells_with_the_player_on_the_rail_hits() {
    let mut game = game();
    minion(&mut game, 0, at(31, 59), Dir::Left, 1);
    assert_eq!(lives_after(&mut game, &[InputMask::RIGHT]), 2);
}

/// [R4.9-10] Swap at a fast-cut close: the cut closes onto (31, 59), where the minion stands,
/// and after the capture the minion, oriented W, turns right onto the captured trail cell
/// (31, 58) the player left.
#[test]
fn minion_swapping_cells_at_a_fast_cut_close_hits() {
    let mut game = game();
    cutting(&mut game, at(31, 58), at(30, 59), &[(30, 58), (31, 58)]);
    minion(&mut game, 0, at(31, 59), Dir::Up, 1);
    let lives = lives_after(&mut game, &[mask(&[InputMask::DOWN, InputMask::A])]);
    assert_eq!((lives, game.captured), (2, 2));
}

/// [R4.9-11] Pass: the fast cut passes through (31, 58) and closes onto (31, 59); after the
/// capture, the minion on the obstacle (32, 58) steps W onto (31, 58).
#[test]
fn minion_stopping_on_the_cell_a_fast_cut_passed_through_hits() {
    let mut game = game_on(&level(&[(32, 58)]));
    cutting(
        &mut game,
        at(31, 57),
        at(30, 59),
        &[(30, 58), (30, 57), (31, 57)],
    );
    minion(&mut game, 0, at(32, 58), Dir::Up, 1);
    let lives = lives_after(&mut game, &[mask(&[InputMask::DOWN, InputMask::A])]);
    assert_eq!((lives, game.captured), (2, 4));
}

/// [R4.9-12] The player moves from (30, 59) to (31, 59), and a minion moves into (30, 59)
/// from (29, 59), not from (31, 59): no hit.
#[test]
fn minion_moving_into_the_cell_the_player_left_does_not_hit() {
    let mut game = game();
    minion(&mut game, 0, at(29, 59), Dir::Right, 1);
    assert_eq!(lives_after(&mut game, &[InputMask::RIGHT]), 3);
}

/// [PIN-11] A minion respawning at step 5 onto the player's cell hits, by the share case.
#[test]
fn minion_respawning_onto_the_player_hits() {
    let mut game = game();
    game.minions.period = 1;
    game.minions.slots[0] = Some(Minion {
        spawn: at(30, 59),
        state: State::Destroyed { countdown: 1 },
    });
    assert_eq!(lives_after(&mut game, &[InputMask::NONE]), 2);
}

/// [R4.9-5] A step onto the boss's body and two minions sharing the player's cell: one life.
#[test]
fn three_hits_in_one_tick_cost_one_life() {
    let mut game = game();
    game.boss.pos = at(28, 54);
    minion(&mut game, 0, at(29, 59), Dir::Right, 1);
    minion(&mut game, 1, at(31, 59), Dir::Left, 1);
    assert_eq!(lives_after(&mut game, &[InputMask::UP]), 2);
}

/// [R4.9-15] With B held and a charge left, a hit spends the charge and nothing else.
#[test]
fn hit_with_b_held_and_a_charge_left_spends_only_the_charge() {
    let mut game = game();
    minion(&mut game, 0, at(31, 59), Dir::Left, 1);
    game.step(InputMask::B).unwrap();
    let player = game.player;
    assert_eq!(
        (player.lives, player.shield, player.invulnerable),
        (3, 0, 0)
    );
}

/// [R4.9-14] An invulnerable player ignores a hit, even with B held and a charge left: the
/// charge stays, and invulnerability counts down.
#[test]
fn hit_while_invulnerable_does_nothing() {
    let mut game = game();
    game.player.invulnerable = 5;
    minion(&mut game, 0, at(31, 59), Dir::Left, 1);
    game.step(InputMask::B).unwrap();
    let player = game.player;
    assert_eq!(
        (player.lives, player.shield, player.invulnerable),
        (3, 1, 4)
    );
}

/// A minion that never moves shares the player's cell on every tick from tick 1 on, so tick 1
/// is the hit tick h.
fn game_hit_every_tick() -> Game {
    let mut game = game();
    minion(&mut game, 0, at(30, 59), Dir::Left, 255);
    game
}

/// [PIN-02] A life lost at tick h protects ticks h + 1 to h + 30: tick h + 30 is protected.
#[test]
fn tick_h_plus_30_is_protected() {
    let mut game = game_hit_every_tick();
    assert_eq!(lives_after(&mut game, &[InputMask::NONE; 31]), 2);
}

/// [PIN-02] Tick h + 31 is not protected.
#[test]
fn tick_h_plus_31_is_not_protected() {
    let mut game = game_hit_every_tick();
    assert_eq!(lives_after(&mut game, &[InputMask::NONE; 32]), 1);
}

/// [PIN-02] [R4.9-17] Starting a cut ends invulnerability at step 3, before that tick's
/// collisions: the boss's bump into the new trail costs a life.
#[test]
fn starting_a_cut_ends_invulnerability() {
    let mut game = game();
    game.player.invulnerable = 10;
    boss_moving(&mut game, at(31, 54));
    assert_eq!(lives_after(&mut game, &[InputMask::UP]), 2);
}

/// [R4.9-17] After a capture along the bottom border, (30, 59) is off the rail; a hit during a
/// cut returns the player to the cut start (30, 50), and the trail is erased.
#[test]
fn player_respawns_at_the_cut_start_after_the_bottom_border_was_captured() {
    let mut game = game();
    let captured: Vec<(u8, u8)> = (1..59)
        .flat_map(|x| (50..59).map(move |y| (x, y)))
        .collect();
    paint(&mut game, &captured, Cell::Claimed);
    game.player.pos = at(30, 50);
    boss_moving(&mut game, at(31, 45));
    game.step(InputMask::UP).unwrap();
    let respawned = (game.player.lives, game.player.pos, game.player.cutting);
    assert_eq!(respawned, (2, at(30, 50), false));
    assert_eq!(game.grid.count(Cell::Trail), 0);
}

/// [R4.4-7] [R4.10-2] X during a cut ends the level as committed: the trail is erased, the
/// player returns to the cut start, and nothing is captured.
#[test]
fn x_during_a_cut_erases_the_trail_and_keeps_the_claim() {
    let mut game = game();
    game.captured = 100;
    cutting(&mut game, at(30, 57), at(30, 59), &[(30, 58), (30, 57)]);
    let end = game.step(InputMask::X).unwrap();
    let after = (
        end,
        game.player.pos,
        game.grid.count(Cell::Trail),
        game.captured,
    );
    assert_eq!(after, (Some(EndReason::Committed), at(30, 59), 0, 100));
}

/// [PIN-09] A clear wins the tick: the cut that reaches the target closes onto the cell where
/// a minion stands, with the last life left, and the level ends cleared at step 3.
#[test]
fn clear_and_last_life_lost_on_the_same_tick_end_the_level_as_cleared() {
    let mut level = level(&[]);
    level.target_bp = 1;
    let mut game = game_on(&level);
    game.player.lives = 1;
    cutting(&mut game, at(31, 58), at(30, 59), &[(30, 58), (31, 58)]);
    minion(&mut game, 0, at(31, 59), Dir::Up, 255);
    let end = game.step(InputMask::DOWN).unwrap();
    assert_eq!(end, Some(EndReason::Cleared));
}

/// [R4.10-3] Losing the last life ends the level at step 6.
#[test]
fn losing_the_last_life_ends_the_level() {
    let mut game = game();
    game.player.lives = 1;
    minion(&mut game, 0, at(31, 59), Dir::Left, 1);
    assert_eq!(
        game.step(InputMask::NONE).unwrap(),
        Some(EndReason::OutOfLives)
    );
}

/// [R4.4-4] An ended level does not change.
#[test]
fn ended_level_does_not_change() {
    let mut game = game();
    game.step(InputMask::X).unwrap();
    let ticks_used = game.ticks_used;
    let end = game.step(InputMask::UP).unwrap();
    assert_eq!(
        (end, game.ticks_used),
        (Some(EndReason::Committed), ticks_used)
    );
}

/// [R4.10-3] A lost life that leaves lives keeps the level running.
#[test]
fn losing_a_life_with_lives_left_keeps_the_level_running() {
    let mut game = game();
    game.player.lives = 2;
    minion(&mut game, 0, at(31, 59), Dir::Left, 1);
    let end = game.step(InputMask::NONE).unwrap();
    assert_eq!((end, game.player.lives), (None, 1));
}

/// [R4.9-9] Share with two sub-steps: the fast cut passes (31, 58) and closes onto (31, 59),
/// where a minion stands still; it shares Pn = P2.
#[test]
fn minion_on_the_cell_a_fast_cut_closes_onto_hits() {
    let mut game = game();
    cutting(
        &mut game,
        at(31, 57),
        at(30, 59),
        &[(30, 58), (30, 57), (31, 57)],
    );
    minion(&mut game, 0, at(31, 59), Dir::Up, 255);
    let lives = lives_after(&mut game, &[mask(&[InputMask::DOWN, InputMask::A])]);
    assert_eq!((lives, game.captured), (2, 4));
}

/// [PIN-10] A claim exactly at the target clears: 2 of 3,364 cells is 5 bp.
#[test]
fn claim_exactly_at_the_target_clears_the_level() {
    let mut level = level(&[]);
    level.target_bp = 5;
    let mut game = game_on(&level);
    cutting(&mut game, at(31, 58), at(30, 59), &[(30, 58), (31, 58)]);
    let end = game.step(InputMask::DOWN).unwrap();
    assert_eq!(
        (end, game.claim_bp().unwrap()),
        (Some(EndReason::Cleared), 5)
    );
}

/// [R4.1-8] Captures add up: 2 cells captured on top of 100.
#[test]
fn captured_cells_add_up_across_captures() {
    let mut game = game();
    game.captured = 100;
    cutting(&mut game, at(31, 58), at(30, 59), &[(30, 58), (31, 58)]);
    game.step(InputMask::DOWN).unwrap();
    assert_eq!(game.captured, 102);
}

/// [R4.5-7] [R4.4-6] B pressed at the odd tick 3 sets press_tick to 3 at step 1: the player
/// moves on ticks 3 and 5, not 4.
#[test]
fn b_pressed_at_a_tick_sets_the_slow_shield_phase() {
    let mut game = game();
    game.step(InputMask::NONE).unwrap();
    game.step(InputMask::NONE).unwrap();
    let held = mask(&[InputMask::UP, InputMask::B]);
    let positions: Vec<Pos> = (3..=5)
        .map(|_| {
            game.step(held).unwrap();
            game.player.pos
        })
        .collect();
    assert_eq!(positions, [at(30, 58), at(30, 58), at(30, 57)]);
}
