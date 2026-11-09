//! Replay (README §6.3, §7.3): checks a level and a journal, plays the journal, and returns the
//! Outcome that the referee returns.

use crate::Error;
use crate::consts::{BP, RULES_VERSION};
use crate::game::{EndReason, Game};
use crate::journal::{self, Mode, journal_hash};
use crate::level::{self, level_id};
use crate::seed::draw;
use crate::types::InputMask;

/// The `Outcome` struct of README §7.3, field for field.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub journal_hash: [u8; 32],
    pub level_id: [u8; 32],
    pub run_nonce: [u8; 32],
    pub level_index: u8,
    /// draw(runNonce)[levelIndex] (README §8.2).
    pub draw_slot: u8,
    pub mode: Mode,
    pub rules_version: u16,
    pub end_reason: EndReason,
    pub claim_bp: u16,
    pub ticks_used: u16,
    pub level_score: u32,
}

/// Replays `journal` on `level` and scores it with `weight_bp`, or returns the first broken
/// rule's code in the order of README §6.3.
pub fn replay(level: &[u8], journal: &[u8], weight_bp: u16) -> Result<Outcome, Error> {
    replay_observed(level, journal, weight_bp, |_| {})
}

/// `replay`, calling `observe` with the game after every processed tick, the ending tick
/// included, so that a caller can record the trace of state hashes (README §6.6).
pub fn replay_observed(
    level_bytes: &[u8],
    journal_bytes: &[u8],
    weight_bp: u16,
    mut observe: impl FnMut(&Game),
) -> Result<Outcome, Error> {
    let journal = journal::decode(journal_bytes)?;
    let header = journal.header;
    let level = level::decode(level_bytes)?;
    let mut game = Game::new(&level, &header.run_nonce, header.level_index)?;
    if level_id(level_bytes) != header.level_id {
        return Err(Error::LevelId);
    }
    if journal.events().any(|event| event.tick >= level.time_limit) {
        return Err(Error::EventPastTimeLimit);
    }
    let mut events = journal.events().peekable();
    let mut mask = InputMask::NONE;
    let end_reason = loop {
        let tick = game.ticks_used;
        if let Some(event) = events.next_if(|event| event.tick == tick) {
            mask = event.mask;
        }
        let end = game.step(mask)?;
        observe(&game);
        if let Some(end) = end {
            break end;
        }
    };
    if events.next().is_some() {
        return Err(Error::EventAfterEnd);
    }
    let claim_bp = game.claim_bp()?;
    Ok(Outcome {
        journal_hash: journal_hash(journal_bytes),
        level_id: header.level_id,
        run_nonce: header.run_nonce,
        level_index: header.level_index,
        draw_slot: draw(&header.run_nonce)
            .get(usize::from(header.level_index))
            .copied()
            .ok_or(Error::LevelIndex)?,
        mode: header.mode,
        rules_version: RULES_VERSION,
        end_reason,
        claim_bp,
        ticks_used: game.ticks_used,
        level_score: level_score(claim_bp, weight_bp, end_reason),
    })
}

/// claim_bp × weight_bp / 10,000, rounded down, and 0 when the lives ran out (README §8.4).
pub fn level_score(claim_bp: u16, weight_bp: u16, end_reason: EndReason) -> u32 {
    const WHOLE: u32 = BP as u32;
    if end_reason == EndReason::OutOfLives {
        return 0;
    }
    // Two u16 factors: the product is below 2^32, so it never wraps.
    u32::from(claim_bp).wrapping_mul(u32::from(weight_bp)) / WHOLE
}
