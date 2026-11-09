//! Journal codec v1 (README §6.3): one test per decoder rejection rule, the order of the
//! rules, valid journals and the round trip.

use jiro_core::Error;
use jiro_core::consts::{
    EVENT_LEN, HEADER_LEN, MAX_EVENTS, MAX_JOURNAL_LEN, MAX_TICKS, RULES_VERSION,
};
use jiro_core::journal::{
    Event, Header, Mode, decode, encode, journal_hash, push_event, run_commitment,
};
use jiro_core::keccak::keccak256;
use jiro_core::types::InputMask;

/// Journal bytes built field by field, independently of `encode`, so that a test can break
/// any rule.
#[derive(Clone)]
struct RawJournal {
    version: u8,
    rules_version: u16,
    mode: u8,
    run_nonce: [u8; 32],
    level_id: [u8; 32],
    level_index: u8,
    event_count: u16,
    events: Vec<(u16, u8)>,
}

impl RawJournal {
    fn new(events: &[(u16, u8)]) -> Self {
        Self {
            version: 1,
            rules_version: RULES_VERSION,
            mode: 0,
            run_nonce: [0x11; 32],
            level_id: [0x22; 32],
            level_index: 0,
            event_count: u16::try_from(events.len()).unwrap(),
            events: events.to_vec(),
        }
    }

    fn of(header: &Header, events: &[Event]) -> Self {
        let events: Vec<(u16, u8)> = events.iter().map(|e| (e.tick, e.mask.bits())).collect();
        Self {
            mode: header.mode as u8,
            run_nonce: header.run_nonce,
            level_id: header.level_id,
            level_index: header.level_index,
            ..Self::new(&events)
        }
    }

    fn bytes(&self) -> Vec<u8> {
        let mut bytes = vec![self.version];
        bytes.extend(self.rules_version.to_be_bytes());
        bytes.push(self.mode);
        bytes.extend(self.run_nonce);
        bytes.extend(self.level_id);
        bytes.push(self.level_index);
        bytes.extend(self.event_count.to_be_bytes());
        for (tick, mask) in &self.events {
            bytes.extend(tick.to_be_bytes());
            bytes.push(*mask);
        }
        bytes
    }
}

fn decode_error(bytes: &[u8]) -> Error {
    decode(bytes).unwrap_err()
}

fn event(tick: u16, mask: u8) -> Event {
    Event {
        tick,
        mask: InputMask::new(mask).unwrap(),
    }
}

fn header() -> Header {
    Header {
        mode: Mode::Training,
        run_nonce: [0x11; 32],
        level_id: [0x22; 32],
        level_index: 0,
    }
}

fn hex32(hex: &str) -> [u8; 32] {
    let hex = hex.strip_prefix("0x").unwrap();
    let bytes: Vec<u8> = (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
        .collect();
    bytes.try_into().unwrap()
}

fn hex_bytes(hex: &str) -> Vec<u8> {
    let hex = hex.strip_prefix("0x").unwrap();
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
        .collect()
}

/// A seeded xorshift64 generator, so the round trip needs no dependency.
struct XorShift(u64);

impl XorShift {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, bound: u64) -> u64 {
        self.next() % bound
    }

    fn bytes32(&mut self) -> [u8; 32] {
        std::array::from_fn(|_| self.next() as u8)
    }
}

/// A random valid journal: up to `MAX_EVENTS` events at distinct ticks below `MAX_TICKS`,
/// each changing the mask.
fn random_journal(rng: &mut XorShift) -> (Header, Vec<Event>) {
    let header = Header {
        mode: if rng.next() & 1 == 0 {
            Mode::Training
        } else {
            Mode::Challenge
        },
        run_nonce: rng.bytes32(),
        level_id: rng.bytes32(),
        level_index: rng.below(6) as u8,
    };
    let mut needed = rng.below(u64::from(MAX_EVENTS) + 1);
    let mut events = Vec::new();
    let mut mask = 0;
    for tick in 0..MAX_TICKS {
        if rng.below(u64::from(MAX_TICKS - tick)) < needed {
            mask = loop {
                let next = rng.next() as u8 & 0x7f;
                if next != mask {
                    break next;
                }
            };
            events.push(event(tick, mask));
            needed -= 1;
        }
    }
    (header, events)
}

#[test]
fn input_shorter_than_the_header_is_truncated() {
    let bytes = RawJournal::new(&[]).bytes();
    for len in [0, 1, HEADER_LEN - 1] {
        assert_eq!(decode_error(&bytes[..len]), Error::Truncated, "{len} bytes");
    }
}

#[test]
fn event_area_shorter_than_event_count_is_truncated() {
    let mut journal = RawJournal::new(&[(0, 1), (1, 2)]);
    journal.event_count = 3;
    let bytes = journal.bytes();
    for len in [bytes.len(), bytes.len() - 1] {
        assert_eq!(decode_error(&bytes[..len]), Error::Truncated, "{len} bytes");
    }
}

#[test]
fn bytes_after_the_last_event_are_trailing() {
    let mut journal = RawJournal::new(&[(0, 1), (1, 2)]);
    journal.event_count = 1;
    let bytes = journal.bytes();
    for len in [bytes.len() - EVENT_LEN + 1, bytes.len()] {
        assert_eq!(
            decode_error(&bytes[..len]),
            Error::TrailingBytes,
            "{len} bytes"
        );
    }
}

#[test]
fn unknown_version_is_rejected() {
    for version in [0, 2, 255] {
        let mut journal = RawJournal::new(&[]);
        journal.version = version;
        assert_eq!(
            decode_error(&journal.bytes()),
            Error::Version,
            "version {version}"
        );
    }
}

/// [PIN-13]
#[test]
fn other_rules_version_is_rejected() {
    for rules_version in [
        0,
        RULES_VERSION - 1,
        RULES_VERSION + 1,
        RULES_VERSION.swap_bytes(),
        u16::MAX,
    ] {
        let mut journal = RawJournal::new(&[]);
        journal.rules_version = rules_version;
        assert_eq!(
            decode_error(&journal.bytes()),
            Error::RulesVersion,
            "rulesVersion {rules_version}"
        );
    }
}

/// [PIN-12]
#[test]
fn reserved_mode_2_is_rejected() {
    let mut journal = RawJournal::new(&[]);
    journal.mode = 2;
    assert_eq!(decode_error(&journal.bytes()), Error::Mode);
}

#[test]
fn unknown_mode_is_rejected() {
    for mode in [3, 255] {
        let mut journal = RawJournal::new(&[]);
        journal.mode = mode;
        assert_eq!(decode_error(&journal.bytes()), Error::Mode, "mode {mode}");
    }
}

#[test]
fn level_index_6_or_more_is_rejected() {
    for level_index in [6, 255] {
        let mut journal = RawJournal::new(&[]);
        journal.level_index = level_index;
        assert_eq!(
            decode_error(&journal.bytes()),
            Error::LevelIndex,
            "levelIndex {level_index}"
        );
    }
}

#[test]
fn event_count_above_2700_is_rejected() {
    for event_count in [MAX_EVENTS + 1, u16::MAX] {
        let mut journal = RawJournal::new(&[]);
        journal.event_count = event_count;
        assert_eq!(
            decode_error(&journal.bytes()),
            Error::EventCount,
            "eventCount {event_count}"
        );
    }
}

#[test]
fn tick_2700_or_more_is_rejected() {
    for events in [
        vec![(MAX_TICKS, 1)],
        vec![(u16::MAX, 1)],
        vec![(0, 1), (MAX_TICKS, 2)],
        vec![(0, 1), (u16::MAX, 2)],
    ] {
        let journal = RawJournal::new(&events);
        assert_eq!(
            decode_error(&journal.bytes()),
            Error::TickLimit,
            "{events:?}"
        );
    }
}

#[test]
fn tick_not_above_the_previous_one_is_rejected() {
    for events in [
        vec![(5, 1), (5, 2)],
        vec![(5, 1), (4, 2)],
        vec![(5, 1), (0, 2)],
        vec![(5, 1), (10, 2), (7, 3)],
    ] {
        let journal = RawJournal::new(&events);
        assert_eq!(
            decode_error(&journal.bytes()),
            Error::TickOrder,
            "{events:?}"
        );
    }
}

#[test]
fn reserved_bit_7_is_rejected() {
    for events in [
        vec![(0, 0x80)],
        vec![(0, 1), (1, 0x80)],
        vec![(0, 1), (1, 0x81)],
        vec![(0, 1), (1, 0xff)],
    ] {
        let journal = RawJournal::new(&events);
        assert_eq!(
            decode_error(&journal.bytes()),
            Error::ReservedBit,
            "{events:?}"
        );
    }
}

#[test]
fn mask_equal_to_the_previous_one_is_rejected() {
    let journal = RawJournal::new(&[(0, 1), (3, 1)]);
    assert_eq!(decode_error(&journal.bytes()), Error::UnchangedMask);
}

#[test]
fn first_event_with_mask_0_is_rejected() {
    let journal = RawJournal::new(&[(0, 0)]);
    assert_eq!(decode_error(&journal.bytes()), Error::UnchangedMask);
}

/// Each case breaks two rules; the decoder returns the first one in README §6.3's order.
#[test]
fn first_broken_rule_wins() {
    let base = RawJournal::new(&[(0, 1), (1, 2)]);
    let with = |change: fn(&mut RawJournal)| {
        let mut journal = base.clone();
        change(&mut journal);
        journal
    };
    let short_bad_version = {
        let mut bytes = with(|j| j.version = 2).bytes();
        bytes.truncate(HEADER_LEN - 1);
        bytes
    };
    let mut events_short_bad_tick = with(|j| j.events[0].0 = MAX_TICKS).bytes();
    events_short_bad_tick.pop();
    let mut trailing_bad_tick = with(|j| j.events[0].0 = MAX_TICKS).bytes();
    trailing_bad_tick.push(0);
    let cases: [(&str, Vec<u8>, Error); 12] = [
        (
            "truncated header, version",
            short_bad_version,
            Error::Truncated,
        ),
        (
            "version, rulesVersion",
            with(|j| (j.version, j.rules_version) = (2, RULES_VERSION + 1)).bytes(),
            Error::Version,
        ),
        (
            "rulesVersion, mode",
            with(|j| (j.rules_version, j.mode) = (RULES_VERSION + 1, 2)).bytes(),
            Error::RulesVersion,
        ),
        (
            "mode, levelIndex",
            with(|j| (j.mode, j.level_index) = (2, 6)).bytes(),
            Error::Mode,
        ),
        (
            "levelIndex, eventCount",
            with(|j| (j.level_index, j.event_count) = (6, MAX_EVENTS + 1)).bytes(),
            Error::LevelIndex,
        ),
        (
            "eventCount, truncated events",
            with(|j| j.event_count = MAX_EVENTS + 1).bytes(),
            Error::EventCount,
        ),
        (
            "truncated events, tick limit",
            events_short_bad_tick,
            Error::Truncated,
        ),
        (
            "trailing bytes, tick limit",
            trailing_bad_tick,
            Error::TrailingBytes,
        ),
        (
            "tick limit, reserved bit",
            with(|j| j.events[1] = (MAX_TICKS, 0x82)).bytes(),
            Error::TickLimit,
        ),
        (
            "tick order, reserved bit",
            with(|j| j.events[1] = (0, 0x80)).bytes(),
            Error::TickOrder,
        ),
        (
            "tick order, unchanged mask",
            with(|j| j.events[1] = (0, 1)).bytes(),
            Error::TickOrder,
        ),
        (
            "first event's unchanged mask, second event's tick limit",
            with(|j| j.events = vec![(0, 0), (MAX_TICKS, 1)]).bytes(),
            Error::UnchangedMask,
        ),
    ];
    for (name, bytes, error) in cases {
        assert_eq!(decode_error(&bytes), error, "{name}");
    }
}

#[test]
fn journal_without_events_decodes_to_its_header() {
    let mut raw = RawJournal::new(&[]);
    raw.mode = 1;
    raw.level_index = 5;
    let bytes = raw.bytes();
    let journal = decode(&bytes).unwrap();
    let expected = Header {
        mode: Mode::Challenge,
        level_index: 5,
        ..header()
    };
    assert_eq!(
        (
            journal.header,
            journal.event_count(),
            journal.events().len()
        ),
        (expected, 0, 0)
    );
}

#[test]
fn events_decode_in_journal_order() {
    let bytes = RawJournal::new(&[(0, 0x01), (7, 0x00), (MAX_TICKS - 1, 0x7f)]).bytes();
    let events: Vec<Event> = decode(&bytes).unwrap().events().collect();
    assert_eq!(
        events,
        [event(0, 0x01), event(7, 0x00), event(MAX_TICKS - 1, 0x7f)]
    );
}

/// README §2: one event per tick of the longest level fills the 8,100-byte event area.
#[test]
fn full_journal_fills_the_buffer() {
    let events: Vec<(u16, u8)> = (0..MAX_TICKS)
        .map(|tick| (tick, 1 + (tick % 2) as u8))
        .collect();
    let bytes = RawJournal::new(&events).bytes();
    let journal = decode(&bytes).unwrap();
    assert_eq!(
        (bytes.len(), journal.event_count()),
        (MAX_JOURNAL_LEN, MAX_EVENTS)
    );
}

#[test]
fn event_area_is_8100_bytes() {
    let event_area = usize::from(MAX_EVENTS) * EVENT_LEN;
    assert_eq!(
        (HEADER_LEN, event_area, MAX_JOURNAL_LEN),
        (71, 8_100, HEADER_LEN + 8_100)
    );
}

/// README §6.3, the worked example `open-field-empty`.
#[test]
fn worked_example_decodes_and_reencodes() {
    let bytes = hex_bytes(
        "0x01000200277b9e3cebd776c6d46a8b4eb9b83ac165961f4b46a79baef5df1514bb5219cd04e24fd698778a8dbcf62ab0c9ed0228d67be3ddd3aeb682f4a67a53410dacc9000000",
    );
    let journal = decode(&bytes).unwrap();
    let expected = Header {
        mode: Mode::Training,
        run_nonce: keccak256(&[b"jiro.example.9"]),
        level_id: hex32("0x04e24fd698778a8dbcf62ab0c9ed0228d67be3ddd3aeb682f4a67a53410dacc9"),
        level_index: 0,
    };
    let mut out = [0; MAX_JOURNAL_LEN];
    let reencoded = encode(&journal.header, &[], &mut out).unwrap();
    assert_eq!((journal.header, reencoded), (expected, bytes.as_slice()));
}

#[test]
fn round_trip_holds_for_10000_random_journals() {
    let mut rng = XorShift(0x9e37_79b9_7f4a_7c15);
    let mut out = [0; MAX_JOURNAL_LEN];
    for case in 0..10_000 {
        let (header, events) = random_journal(&mut rng);
        let bytes = RawJournal::of(&header, &events).bytes();
        let journal = decode(&bytes).unwrap();
        let decoded: Vec<Event> = journal.events().collect();
        assert_eq!((journal.header, &decoded), (header, &events), "case {case}");
        let reencoded = encode(&journal.header, &decoded, &mut out).unwrap();
        assert_eq!(reencoded, bytes.as_slice(), "case {case}");
    }
}

/// `encode` rejects what `decode` would reject, with the same error.
#[test]
fn encode_rejects_invalid_journals() {
    let too_many: Vec<Event> = (0..=MAX_EVENTS)
        .map(|i| event(i, 1 + (i % 2) as u8))
        .collect();
    let cases: [(&str, Header, Vec<Event>, Error); 6] = [
        (
            "levelIndex 6",
            Header {
                level_index: 6,
                ..header()
            },
            vec![],
            Error::LevelIndex,
        ),
        ("2,701 events", header(), too_many, Error::EventCount),
        (
            "tick 2,700",
            header(),
            vec![event(MAX_TICKS, 1)],
            Error::TickLimit,
        ),
        (
            "tick order",
            header(),
            vec![event(3, 1), event(3, 2)],
            Error::TickOrder,
        ),
        (
            "first event's mask 0",
            header(),
            vec![event(0, 0)],
            Error::UnchangedMask,
        ),
        (
            "unchanged mask",
            header(),
            vec![event(0, 1), event(1, 1)],
            Error::UnchangedMask,
        ),
    ];
    let mut out = [0; MAX_JOURNAL_LEN];
    for (name, header, events, error) in cases {
        assert_eq!(encode(&header, &events, &mut out), Err(error), "{name}");
    }
}

#[test]
fn keccak256_of_empty_input() {
    assert_eq!(
        keccak256(&[]),
        hex32("0xc5d2460186f7233c927e7db2dcc703c0e500b653ca82273b7bfad8045d85a470")
    );
}

#[test]
fn journal_hash_is_keccak256_of_the_bytes() {
    let bytes = RawJournal::new(&[(0, 1)]).bytes();
    assert_eq!(journal_hash(&bytes), keccak256(&[&bytes]));
}

#[test]
fn run_commitment_hashes_the_six_journal_hashes_in_order() {
    let hashes: [[u8; 32]; 6] = std::array::from_fn(|i| [i as u8; 32]);
    assert_eq!(run_commitment(&hashes), keccak256(&[&hashes.concat()]));
}

/// push_event appends to the journal that encode wrote, and gives the bytes encode would.
#[test]
fn push_event_appends_what_encode_writes() {
    let events = [event(0, 0x01), event(7, 0x11)];
    let mut pushed = [0; MAX_JOURNAL_LEN];
    encode(&header(), &[], &mut pushed).unwrap();
    push_event(&mut pushed, events[0]).unwrap();
    let pushed = push_event(&mut pushed, events[1]).unwrap().to_vec();
    let mut encoded = [0; MAX_JOURNAL_LEN];
    assert_eq!(pushed, encode(&header(), &events, &mut encoded).unwrap());
}

/// push_event rejects what decode rejects, and leaves the journal unchanged.
#[test]
fn push_event_rejects_an_unchanged_mask() {
    let mut out = [0; MAX_JOURNAL_LEN];
    encode(&header(), &[event(0, 0x01)], &mut out).unwrap();
    let before = out;
    let result = push_event(&mut out, event(1, 0x01)).map(<[u8]>::to_vec);
    assert_eq!((result, out == before), (Err(Error::UnchangedMask), true));
}
