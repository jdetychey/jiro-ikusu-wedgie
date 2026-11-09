//! Journal bytes v1 (README §6.3): a 71-byte big-endian header, then 3 bytes per event.

use crate::consts::{
    EVENT_LEN, HEADER_LEN, JOURNAL_VERSION, MAX_EVENTS, MAX_JOURNAL_LEN, MAX_TICKS, RULES_VERSION,
    RUN_LEVELS,
};
use crate::error::Error;
use crate::keccak::keccak256;
use crate::types::InputMask;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Mode {
    Training = 0,
    Challenge = 1,
}

/// The header fields a journal varies. `version` and `rulesVersion` are always
/// `JOURNAL_VERSION` and `RULES_VERSION`, and `eventCount` is the number of events.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    pub mode: Mode,
    pub run_nonce: [u8; 32],
    pub level_id: [u8; 32],
    pub level_index: u8,
}

/// A mask that applies from `tick` until the next event (README §6.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Event {
    pub tick: u16,
    pub mask: InputMask,
}

/// A decoded journal: its header, and its events read in place from the decoded bytes.
#[derive(Clone, Copy, Debug)]
pub struct Journal<'a> {
    pub header: Header,
    event_count: u16,
    events: &'a [[u8; EVENT_LEN]],
}

impl<'a> Journal<'a> {
    pub fn event_count(&self) -> u16 {
        self.event_count
    }

    pub fn events(&self) -> impl ExactSizeIterator<Item = Event> + 'a {
        self.events.iter().map(read_event)
    }
}

/// Validates `bytes` as a journal, checking the rules of README §6.3 in their order and
/// returning the first broken rule's error.
pub fn decode(bytes: &[u8]) -> Result<Journal<'_>, Error> {
    let (header, rest) = split_header(bytes).ok_or(Error::Truncated)?;
    if header.version != JOURNAL_VERSION {
        return Err(Error::Version);
    }
    if u16::from_be_bytes(header.rules_version) != RULES_VERSION {
        return Err(Error::RulesVersion);
    }
    let mode = match header.mode {
        0 => Mode::Training,
        1 => Mode::Challenge,
        _ => return Err(Error::Mode),
    };
    check_level_index(header.level_index)?;
    let event_count = u16::from_be_bytes(header.event_count);
    if event_count > MAX_EVENTS {
        return Err(Error::EventCount);
    }
    let (events, trailing) = rest.as_chunks::<EVENT_LEN>();
    match events.len().cmp(&usize::from(event_count)) {
        core::cmp::Ordering::Less => return Err(Error::Truncated),
        core::cmp::Ordering::Greater => return Err(Error::TrailingBytes),
        core::cmp::Ordering::Equal if !trailing.is_empty() => return Err(Error::TrailingBytes),
        core::cmp::Ordering::Equal => {}
    }
    let mut previous = None;
    for event in events.iter().map(read_event) {
        previous = Some(check_event(previous, event.tick, event.mask.bits())?);
    }
    Ok(Journal {
        header: Header {
            mode,
            run_nonce: *header.run_nonce,
            level_id: *header.level_id,
            level_index: header.level_index,
        },
        event_count,
        events,
    })
}

/// Writes the journal of `header` and `events` into `out` and returns its bytes. Rejects
/// what `decode` would reject, with the same errors.
pub fn encode<'o>(
    header: &Header,
    events: &[Event],
    out: &'o mut [u8; MAX_JOURNAL_LEN],
) -> Result<&'o [u8], Error> {
    check_level_index(header.level_index)?;
    let event_count = u16::try_from(events.len())
        .ok()
        .filter(|&count| count <= MAX_EVENTS)
        .ok_or(Error::EventCount)?;
    let mut previous = None;
    for event in events {
        previous = Some(check_event(previous, event.tick, event.mask.bits())?);
    }
    let header_fields: [&[u8]; 7] = [
        &[JOURNAL_VERSION],
        &RULES_VERSION.to_be_bytes(),
        &[header.mode as u8],
        &header.run_nonce,
        &header.level_id,
        &[header.level_index],
        &event_count.to_be_bytes(),
    ];
    let header_bytes = header_fields.into_iter().flatten().copied();
    let events_bytes = events.iter().flat_map(|&event| event_bytes(event));
    for (slot, byte) in out.iter_mut().zip(header_bytes.chain(events_bytes)) {
        *slot = byte;
    }
    journal_of(out, event_count)
}

/// Appends `event` to the journal that `encode` wrote into `out`, and returns the journal's
/// bytes. Rejects what `decode` would reject, with the same errors, and leaves `out` unchanged
/// then.
pub fn push_event(out: &mut [u8; MAX_JOURNAL_LEN], event: Event) -> Result<&[u8], Error> {
    let (header, event_area) = out.split_at_mut(HEADER_LEN);
    let count_bytes = header.last_chunk_mut::<2>().ok_or(Error::Truncated)?;
    let count = u16::from_be_bytes(*count_bytes);
    if count >= MAX_EVENTS {
        return Err(Error::EventCount);
    }
    let (events, _) = event_area.as_chunks_mut::<EVENT_LEN>();
    let previous = count
        .checked_sub(1)
        .and_then(|last| events.get(usize::from(last)))
        .map(read_event);
    let event = check_event(previous, event.tick, event.mask.bits())?;
    *events
        .get_mut(usize::from(count))
        .ok_or(Error::EventCount)? = event_bytes(event);
    let count = count.saturating_add(1);
    *count_bytes = count.to_be_bytes();
    journal_of(out, count)
}

/// An event's bytes, unchecked: the tick big-endian, then the mask (README §6.3).
fn read_event(&[tick_high, tick_low, mask]: &[u8; EVENT_LEN]) -> Event {
    Event {
        tick: u16::from_be_bytes([tick_high, tick_low]),
        mask: InputMask(mask),
    }
}

fn event_bytes(event: Event) -> [u8; EVENT_LEN] {
    let [tick_high, tick_low] = event.tick.to_be_bytes();
    [tick_high, tick_low, event.mask.bits()]
}

/// The journal of `count` events at the start of `out`: its first 71 + 3 × count bytes.
fn journal_of(out: &[u8; MAX_JOURNAL_LEN], count: u16) -> Result<&[u8], Error> {
    usize::from(count)
        .checked_mul(EVENT_LEN)
        .and_then(|events_len| events_len.checked_add(HEADER_LEN))
        .and_then(|len| out.get(..len))
        .ok_or(Error::EventCount)
}

/// keccak256 of the journal bytes, as the run commitment and the Outcome use it (README §6.4).
pub fn journal_hash(bytes: &[u8]) -> [u8; 32] {
    keccak256(&[bytes])
}

/// keccak256(keccak256(journal₀) ‖ … ‖ keccak256(journal₅)) (README §6.4).
pub fn run_commitment(journal_hashes: &[[u8; 32]; RUN_LEVELS]) -> [u8; 32] {
    let [h0, h1, h2, h3, h4, h5] = journal_hashes;
    keccak256(&[h0, h1, h2, h3, h4, h5])
}

struct RawHeader<'a> {
    version: u8,
    rules_version: [u8; 2],
    mode: u8,
    run_nonce: &'a [u8; 32],
    level_id: &'a [u8; 32],
    level_index: u8,
    event_count: [u8; 2],
}

/// Splits off the header fields in their byte order, or `None` if `bytes` is shorter than
/// `HEADER_LEN`.
fn split_header(bytes: &[u8]) -> Option<(RawHeader<'_>, &[u8])> {
    let (&[version], rest) = bytes.split_first_chunk::<1>()?;
    let (&rules_version, rest) = rest.split_first_chunk::<2>()?;
    let (&[mode], rest) = rest.split_first_chunk::<1>()?;
    let (run_nonce, rest) = rest.split_first_chunk::<32>()?;
    let (level_id, rest) = rest.split_first_chunk::<32>()?;
    let (&[level_index], rest) = rest.split_first_chunk::<1>()?;
    let (&event_count, rest) = rest.split_first_chunk::<2>()?;
    let header = RawHeader {
        version,
        rules_version,
        mode,
        run_nonce,
        level_id,
        level_index,
        event_count,
    };
    Some((header, rest))
}

fn check_level_index(level_index: u8) -> Result<(), Error> {
    if usize::from(level_index) < RUN_LEVELS {
        Ok(())
    } else {
        Err(Error::LevelIndex)
    }
}

/// Checks one event against the previous one, in the order of README §6.3 rule 8. Before
/// the first event, the mask is 0.
fn check_event(previous: Option<Event>, tick: u16, mask: u8) -> Result<Event, Error> {
    if tick >= MAX_TICKS {
        return Err(Error::TickLimit);
    }
    if previous.is_some_and(|previous| tick <= previous.tick) {
        return Err(Error::TickOrder);
    }
    let mask = InputMask::new(mask).ok_or(Error::ReservedBit)?;
    if mask == previous.map_or(InputMask::NONE, |previous| previous.mask) {
        return Err(Error::UnchangedMask);
    }
    Ok(Event { tick, mask })
}
