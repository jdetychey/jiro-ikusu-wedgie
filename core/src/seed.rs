//! Derivations from the run nonce (README §6.4, §8.2). keccak256 runs once per level and
//! once per draw step, never per tick.

use core::num::NonZeroU32;

use crate::consts::{RUN_LEVELS, SEASON_LEVELS};
use crate::keccak::keccak256;
use crate::level::Rect;
use crate::types::Pos;

/// The draw step i reduces its hash modulo 16 − i (README §8.2).
const DRAW_MODULI: [NonZeroU32; RUN_LEVELS] = [
    NonZeroU32::new(16).unwrap(),
    NonZeroU32::new(15).unwrap(),
    NonZeroU32::new(14).unwrap(),
    NonZeroU32::new(13).unwrap(),
    NonZeroU32::new(12).unwrap(),
    NonZeroU32::new(11).unwrap(),
];

/// The boss's top-left cell and diagonal direction at level start (R4.6-5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BossStart {
    pub pos: Pos,
    pub vx: i8,
    pub vy: i8,
}

/// keccak256("jiro.level" ‖ runNonce ‖ levelIndex), packed as `abi.encodePacked` does.
pub fn level_seed(run_nonce: &[u8; 32], level_index: u8) -> [u8; 32] {
    keccak256(&[b"jiro.level", run_nonce, &[level_index]])
}

/// The candidate whose row-major index in the spawn rectangle is the big-endian u32 of seed
/// bytes 0 to 3 modulo the number of candidates; vx = +1 if bit 0 of seed byte 4 is set, and
/// vy = +1 if bit 1 is. `None` if the rectangle is empty, which level validity rules out
/// (code 24).
pub fn boss_start(spawn: &Rect, seed: &[u8; 32]) -> Option<BossStart> {
    let width = u32::from(spawn.x1.checked_sub(spawn.x0)?) + 1;
    let height = u32::from(spawn.y1.checked_sub(spawn.y0)?) + 1;
    let candidates = NonZeroU32::new(width.checked_mul(height)?)?;
    let width = NonZeroU32::new(width)?;
    let index = u32::from_be_bytes([seed[0], seed[1], seed[2], seed[3]]) % candidates;
    let pos = Pos {
        x: spawn.x0.checked_add(u8::try_from(index % width).ok()?)?,
        y: spawn.y0.checked_add(u8::try_from(index / width).ok()?)?,
    };
    let direction = |bit: u8| if seed[4] & bit != 0 { 1 } else { -1 };
    Some(BossStart {
        pos,
        vx: direction(1),
        vy: direction(2),
    })
}

/// The key of the boss's deflections: the big-endian u32 of seed bytes 5 to 8 (R4.6-7).
pub fn deflection_key(seed: &[u8; 32]) -> u32 {
    u32::from_be_bytes([seed[5], seed[6], seed[7], seed[8]])
}

/// The run's draw slots: a forward partial Fisher–Yates shuffle of the 16 season levels,
/// where step i swaps deck[i] with deck[i + h mod (16 − i)] and h is
/// keccak256("jiro.draw" ‖ runNonce ‖ i) read as a big-endian integer (README §8.2).
pub fn draw(run_nonce: &[u8; 32]) -> [u8; RUN_LEVELS] {
    let mut deck: [u8; SEASON_LEVELS] = core::array::from_fn(|slot| slot as u8);
    for (step, modulus) in DRAW_MODULI.into_iter().enumerate() {
        let hash = keccak256(&[b"jiro.draw", run_nonce, &[step as u8]]);
        let offset = hash
            .iter()
            .fold(0, |rest, &byte| ((rest << 8) | u32::from(byte)) % modulus);
        let (_, unshuffled) = deck.split_at_mut(step);
        unshuffled.swap(0, offset as usize);
    }
    let [s0, s1, s2, s3, s4, s5, ..] = deck;
    [s0, s1, s2, s3, s4, s5]
}
