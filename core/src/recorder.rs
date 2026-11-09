//! The Recorder (README §6.2): the single input path of every target. A raw mask is journaled
//! as an event when it changes, and the game then steps with the journaled mask.

use core::mem::MaybeUninit;

use crate::Error;
use crate::consts::MAX_JOURNAL_LEN;
use crate::game::{EndReason, Game};
use crate::journal::{self, Event, Header};
use crate::level::Level;
use crate::types::InputMask;

#[derive(Clone, Debug)]
pub struct Recorder {
    game: Game,
    journal: [u8; MAX_JOURNAL_LEN],
    /// The length of the journal so far, at most `MAX_JOURNAL_LEN`.
    journal_len: u16,
}

impl Recorder {
    /// A level ready to play, with the journal of `header` and no events yet.
    pub fn new(level: &Level, header: &Header) -> Result<Self, Error> {
        Self::new_in(&mut MaybeUninit::uninit(), level, header).cloned()
    }

    /// `new`, written into `place` field by field, as `Game::new_in` writes the game: the stack
    /// never holds a Game or a journal (plan M3.10).
    #[allow(unsafe_code)]
    pub fn new_in<'a>(
        place: &'a mut MaybeUninit<Self>,
        level: &Level,
        header: &Header,
    ) -> Result<&'a mut Self, Error> {
        let recorder = place.as_mut_ptr();
        // SAFETY: `recorder` is valid for writes of a Recorder, and until every field is written
        // only fields are written, each once and whole, never read: the game by `Game::new_in`
        // through a MaybeUninit of its place, the journal zeroed in place before encode writes
        // the header into it.
        let game = unsafe { &mut *(&raw mut (*recorder).game).cast::<MaybeUninit<Game>>() };
        Game::new_in(game, level, &header.run_nonce, header.level_index)?;
        let journal = unsafe {
            let journal = &raw mut (*recorder).journal;
            journal.write_bytes(0, 1);
            &mut *journal
        };
        let journal_len = length(journal::encode(header, &[], journal)?)?;
        // SAFETY: as above; this writes the last field, as the pattern below checks.
        let recorder = unsafe {
            (&raw mut (*recorder).journal_len).write(journal_len);
            place.assume_init_mut()
        };
        // Lists every field, so that a field added to Recorder fails to compile until it is
        // written above.
        let Self {
            game: _,
            journal: _,
            journal_len: _,
        } = recorder;
        Ok(recorder)
    }

    /// Processes the next tick: journals `raw` if it differs from the mask in force, then
    /// steps the game with it. An ended level does not change.
    pub fn step(&mut self, raw: InputMask) -> Result<Option<EndReason>, Error> {
        if self.game.end.is_none() && raw != self.game.mask {
            let event = Event {
                tick: self.game.ticks_used,
                mask: raw,
            };
            self.journal_len = length(journal::push_event(&mut self.journal, event)?)?;
        }
        self.game.step(raw)
    }

    pub fn game(&self) -> &Game {
        &self.game
    }

    /// The journal bytes so far.
    pub fn journal(&self) -> &[u8] {
        // journal_len is the length of a prefix of the buffer that encode or push_event returned.
        self.journal
            .get(..usize::from(self.journal_len))
            .unwrap_or(&self.journal)
    }
}

fn length(journal: &[u8]) -> Result<u16, Error> {
    u16::try_from(journal.len()).map_err(|_| Error::Invariant)
}
