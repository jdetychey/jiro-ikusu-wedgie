//! Hits (README §4.9): the minion hit predicate of PIN-15, and their resolution at step 6.

use crate::consts::{INVULNERABLE_TICKS, MINION_SLOTS};
use crate::grid::Grid;
use crate::minion::Moved;
use crate::player::{Moves, Player};
use crate::types::InputMask;

/// Whether a minion meets the player at step 5: it ends on the player's last cell (share),
/// swaps cells with it across one sub-step (swap), or ends on the cell a fast cut passed
/// through (pass) [R4.9-6 to R4.9-12, PIN-11].
pub fn minion_hits(moves: &Moves, minions: &[Option<Moved>; MINION_SLOTS]) -> bool {
    let entered = || moves.entered.iter().flatten().copied();
    let last = entered().last().unwrap_or(moves.from);
    let passed = match moves.entered {
        [Some(first), Some(_)] => Some(first),
        _ => None,
    };
    minions.iter().flatten().any(|minion| {
        let share = minion.to == last;
        let swap = core::iter::once(moves.from)
            .chain(entered())
            .zip(entered())
            .any(|(before, after)| minion.from == after && minion.to == before);
        share || swap || passed == Some(minion.to)
    })
}

/// Step 6: resolves the tick's single hit flag, then counts the invulnerability down. An
/// invulnerable player ignores the hit; otherwise B held with a charge left spends the charge;
/// otherwise a life is lost, the cut is abandoned, and the player is invulnerable for the next
/// 30 ticks. Returns whether a life was lost [R4.9-13 to R4.9-17, PIN-02, R4.3-5].
pub fn resolve(player: &mut Player, grid: &mut Grid, hit: bool, mask: InputMask) -> bool {
    if hit && player.invulnerable == 0 {
        if mask.contains(InputMask::B) && player.shield > 0 {
            player.shield = player.shield.saturating_sub(1);
        } else {
            player.lives = player.lives.saturating_sub(1);
            player.abandon_cut(grid);
            player.invulnerable = INVULNERABLE_TICKS;
            return true;
        }
    }
    player.invulnerable = player.invulnerable.saturating_sub(1);
    false
}
