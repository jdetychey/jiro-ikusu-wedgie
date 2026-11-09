//! A level in play (README §4.4): the whole state, and one tick of the seven steps.

use core::num::NonZeroU16;

use crate::Error;
use crate::boss::Boss;
use crate::consts::CAPTURE_STACK;
use crate::grid::{Grid, claim_bp};
use crate::hits::{minion_hits, resolve};
use crate::level::{Level, validate};
use crate::minion::Minions;
use crate::player::Player;
use crate::seed::level_seed;
use crate::types::InputMask;

/// How a level ended (README §4.10), with the `endReason` codes of the Outcome (§7.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum EndReason {
    Committed = 1,
    Cleared = 2,
    OutOfLives = 3,
    TimeLimit = 4,
}

#[derive(Clone, Debug)]
pub struct Game {
    pub grid: Grid,
    pub player: Player,
    pub boss: Boss,
    pub minions: Minions,
    /// The mask in force, from the last journal event (R4.4-6).
    pub mask: InputMask,
    /// The ticks processed so far, which is also the next tick to process (PIN-08).
    pub ticks_used: u16,
    /// The journal events applied so far. Every event changes the mask (README §6.2), so this
    /// counts the changes of mask.
    pub events_applied: u16,
    /// The cells captured so far in the level.
    pub captured: u16,
    /// The cells `EMPTY` at level start [R4.1-8].
    pub claimable: NonZeroU16,
    pub target_bp: u16,
    pub time_limit: u16,
    pub end: Option<EndReason>,
    stack: [u16; CAPTURE_STACK],
}

impl Game {
    /// The level before tick 0, with the boss start and deflection key of the level seed of
    /// `run_nonce` and `level_index` (README §6.4). Rejects a level that fails validity (35 to
    /// 39).
    pub fn new(level: &Level, run_nonce: &[u8; 32], level_index: u8) -> Result<Self, Error> {
        let mut stack = [0; CAPTURE_STACK];
        let start = validate(level, &mut stack)?;
        let boss =
            Boss::start(level, &level_seed(run_nonce, level_index)).ok_or(Error::SpawnOrder)?;
        Ok(Self {
            minions: Minions::start(level, &start.grid),
            grid: start.grid,
            player: Player::start(level.lives),
            boss,
            mask: InputMask::NONE,
            ticks_used: 0,
            events_applied: 0,
            captured: 0,
            claimable: start.claimable,
            target_bp: level.target_bp,
            time_limit: level.time_limit,
            end: None,
            stack,
        })
    }

    /// The claim in basis points: captured × 10,000 / claimable, rounded down [R4.1-8].
    pub fn claim_bp(&self) -> Result<u16, Error> {
        u16::try_from(claim_bp(self.captured, self.claimable)).map_err(|_| Error::Invariant)
    }

    /// Processes the next tick with `mask` in force, steps 1 to 7, and returns how the level
    /// ended, if it did on this tick or before. An ended level does not change.
    pub fn step(&mut self, mask: InputMask) -> Result<Option<EndReason>, Error> {
        if self.end.is_none() {
            let tick = self.ticks_used;
            // tick < time_limit ≤ 2,700, so the count cannot saturate.
            self.ticks_used = tick.saturating_add(1);
            self.end = self.tick(tick, mask)?;
        }
        Ok(self.end)
    }

    /// The steps of R4.4-5, stopping at the first that ends the level [R4.4-4].
    fn tick(&mut self, tick: u16, mask: InputMask) -> Result<Option<EndReason>, Error> {
        self.input(tick, mask);
        if mask.contains(InputMask::X) {
            self.player.abandon_cut(&mut self.grid);
            return Ok(Some(EndReason::Committed));
        }
        let moves = self
            .player
            .step(&mut self.grid, &self.boss, mask, tick, &mut self.stack)
            .ok_or(Error::Invariant)?;
        if let Some(size) = moves.captured {
            self.captured = self.captured.saturating_add(size);
            self.minions.after_capture(&self.grid);
            if self.claim_bp()? >= self.target_bp {
                return Ok(Some(EndReason::Cleared));
            }
        }
        let boss_hit = self.boss.step(&self.grid, tick);
        let minions = self.minions.step(&self.grid, tick);
        let hit = moves.hit || boss_hit || minion_hits(&moves, &minions);
        if resolve(&mut self.player, &mut self.grid, hit, mask) && self.player.lives == 0 {
            return Ok(Some(EndReason::OutOfLives));
        }
        let last_tick = self.time_limit.checked_sub(1) == Some(tick);
        Ok(last_tick.then_some(EndReason::TimeLimit))
    }

    /// Step 1: the mask of this tick's event, if any, replaces the mask in force [R4.4-6].
    fn input(&mut self, tick: u16, mask: InputMask) {
        if mask != self.mask {
            self.events_applied = self.events_applied.saturating_add(1);
        }
        self.player.input(self.mask, mask, tick);
        self.mask = mask;
    }
}

impl Game {
    /// `new`, written into `place` field by field, so that the stack holds the level start's
    /// grid but never a Game: the wedgie app runs the core on a stack it allocates (plan M3.10).
    /// `new` stays as it was, since the referees run it and `ZkReferee` pins their image id.
    #[allow(unsafe_code)]
    pub fn new_in<'a>(
        place: &'a mut core::mem::MaybeUninit<Self>,
        level: &Level,
        run_nonce: &[u8; 32],
        level_index: u8,
    ) -> Result<&'a mut Self, Error> {
        let game = place.as_mut_ptr();
        // SAFETY: `game` is valid for writes of a Game, and until every field is written only
        // fields are written, each once and whole, never read: the capture stack is zeroed in
        // place before validate uses it, as `new` zeroes its own.
        let stack = unsafe {
            let stack = &raw mut (*game).stack;
            stack.write_bytes(0, 1);
            &mut *stack
        };
        let start = validate(level, stack)?;
        let boss =
            Boss::start(level, &level_seed(run_nonce, level_index)).ok_or(Error::SpawnOrder)?;
        // SAFETY: as above; these write every other field, as the pattern below checks.
        let game = unsafe {
            (&raw mut (*game).minions).write(Minions::start(level, &start.grid));
            (&raw mut (*game).grid).write(start.grid);
            (&raw mut (*game).player).write(Player::start(level.lives));
            (&raw mut (*game).boss).write(boss);
            (&raw mut (*game).mask).write(InputMask::NONE);
            (&raw mut (*game).ticks_used).write(0);
            (&raw mut (*game).events_applied).write(0);
            (&raw mut (*game).captured).write(0);
            (&raw mut (*game).claimable).write(start.claimable);
            (&raw mut (*game).target_bp).write(level.target_bp);
            (&raw mut (*game).time_limit).write(level.time_limit);
            (&raw mut (*game).end).write(None);
            place.assume_init_mut()
        };
        // Lists every field, so that a field added to Game fails to compile until it is written
        // above.
        let Self {
            grid: _,
            player: _,
            boss: _,
            minions: _,
            mask: _,
            ticks_used: _,
            events_applied: _,
            captured: _,
            claimable: _,
            target_bp: _,
            time_limit: _,
            end: _,
            stack: _,
        } = game;
        Ok(game)
    }
}
