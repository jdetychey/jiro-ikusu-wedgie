//! Replay (README §6.3, §7.3), the state hash v1 (§6.4) and the Recorder (§6.2).

use jiro_core::Error;
use jiro_core::consts::MAX_JOURNAL_LEN;
use jiro_core::game::{EndReason, Game};
use jiro_core::hash::{preimage, state_hash};
use jiro_core::journal::{Event, Header, Mode, encode};
use jiro_core::keccak::keccak256;
use jiro_core::level::{Level, Rect, encode as encode_level, level_id};
use jiro_core::minion::{Minion, State};
use jiro_core::player::Player;
use jiro_core::recorder::Recorder;
use jiro_core::replay::{Outcome, level_score, replay, replay_observed};
use jiro_core::seed::draw;
use jiro_core::types::{Cell, CellIndex, Dir, InputMask, Pos};

/// README §5's open-field level.
fn open_field() -> Level {
    Level {
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
    }
}

/// The worked-example run nonce of README §6.4.
fn example_nonce() -> [u8; 32] {
    keccak256(&[b"jiro.example.9"])
}

fn header(level: &Level, level_index: u8) -> Header {
    Header {
        mode: Mode::Training,
        run_nonce: example_nonce(),
        level_id: level_id(&encode_level(level)),
        level_index,
    }
}

fn journal(header: &Header, events: &[(u16, InputMask)]) -> Vec<u8> {
    let events: Vec<Event> = events
        .iter()
        .map(|&(tick, mask)| Event { tick, mask })
        .collect();
    let mut out = [0; MAX_JOURNAL_LEN];
    encode(header, &events, &mut out).unwrap().to_vec()
}

fn replay_events(level: &Level, events: &[(u16, InputMask)]) -> Result<Outcome, Error> {
    let journal = journal(&header(level, 0), events);
    replay(&encode_level(level), &journal, 10_000)
}

fn short_level(time_limit: u16) -> Level {
    Level {
        time_limit,
        ..open_field()
    }
}

/// [PIN-07] [PIN-08] X at tick 0 commits the level with claim 0, and ticks used is 1.
#[test]
fn x_at_tick_0_commits_with_claim_0_after_one_tick() {
    let outcome = replay_events(&open_field(), &[(0, InputMask::X)]).unwrap();
    let ended = (outcome.end_reason, outcome.claim_bp, outcome.ticks_used);
    assert_eq!(ended, (EndReason::Committed, 0, 1));
}

/// [R4.10-4] An empty journal runs T ticks and ends on time.
#[test]
fn empty_journal_runs_t_ticks_and_ends_on_time() {
    let outcome = replay_events(&short_level(10), &[]).unwrap();
    assert_eq!(
        (outcome.end_reason, outcome.ticks_used),
        (EndReason::TimeLimit, 10)
    );
}

/// [R4.4-1] An event at tick T − 1 is accepted.
#[test]
fn event_at_the_last_tick_is_accepted() {
    let outcome = replay_events(&short_level(10), &[(9, InputMask::UP)]).unwrap();
    assert_eq!(outcome.ticks_used, 10);
}

/// [R4.4-1] README §6.3: an event at tick T or later is rejected before play (13).
#[test]
fn event_at_the_time_limit_is_rejected() {
    let result = replay_events(&short_level(10), &[(10, InputMask::UP)]);
    assert_eq!(result, Err(Error::EventPastTimeLimit));
}

/// README §6.3: an event after the tick on which the level ended is rejected (14).
#[test]
fn event_after_a_commit_is_rejected() {
    let result = replay_events(&open_field(), &[(0, InputMask::X), (5, InputMask::UP)]);
    assert_eq!(result, Err(Error::EventAfterEnd));
}

/// README §6.3: a journal whose levelId is not keccak256 of the level bytes is rejected (12).
#[test]
fn journal_of_another_level_is_rejected() {
    let mut header = header(&open_field(), 0);
    header.level_id = [0; 32];
    let result = replay(&encode_level(&open_field()), &journal(&header, &[]), 10_000);
    assert_eq!(result, Err(Error::LevelId));
}

/// README §6.3: the level's validity (20 to 39) is checked before its id (12).
#[test]
fn invalid_level_is_rejected_before_its_id_is_checked() {
    let mut level = open_field();
    level.lives = 0;
    let result = replay(
        &encode_level(&level),
        &journal(&header(&open_field(), 0), &[]),
        1,
    );
    assert_eq!(result, Err(Error::Lives));
}

/// [R4.10-4] The open-field-empty run of README §6.4 ends on time with claim 0, and every
/// Outcome field matches its source.
#[test]
fn open_field_empty_outcome() {
    let level = open_field();
    let header = header(&level, 0);
    let journal = journal(&header, &[]);
    let outcome = replay(&encode_level(&level), &journal, 10_000).unwrap();
    let expected = Outcome {
        journal_hash: keccak256(&[&journal]),
        level_id: header.level_id,
        run_nonce: example_nonce(),
        level_index: 0,
        draw_slot: draw(&example_nonce())[0],
        mode: Mode::Training,
        rules_version: 2,
        end_reason: EndReason::TimeLimit,
        claim_bp: 0,
        ticks_used: 2_700,
        level_score: 0,
    };
    assert_eq!(outcome, expected);
}

/// README §7.3: drawSlot is draw(runNonce)[levelIndex].
#[test]
fn draw_slot_is_the_draw_of_the_level_index() {
    let level = short_level(1);
    let journal = journal(&header(&level, 4), &[]);
    let outcome = replay(&encode_level(&level), &journal, 1).unwrap();
    assert_eq!(outcome.draw_slot, draw(&example_nonce())[4]);
}

/// README §8.4: claim × weight / 10,000, rounded down.
#[test]
fn level_score_is_one_floor_division() {
    assert_eq!(level_score(7_501, 3_333, EndReason::Cleared), 2_500);
}

/// [R4.10-3] README §8.4: running out of lives scores zero.
#[test]
fn level_score_is_zero_when_the_lives_ran_out() {
    assert_eq!(level_score(10_000, 10_000, EndReason::OutOfLives), 0);
}

fn trace(level: &Level, journal: &[u8]) -> Vec<[u8; 32]> {
    let mut hashes = Vec::new();
    replay_observed(&encode_level(level), journal, 10_000, |game| {
        hashes.push(state_hash(game))
    })
    .unwrap();
    hashes
}

/// README §6.4: the tick-0 state hash of open-field-empty, and the preimage length.
#[test]
fn open_field_empty_tick_0_hashes_to_the_worked_example() {
    let level = open_field();
    let hashes = trace(&level, &journal(&header(&level, 0), &[]));
    assert_eq!(
        hex(&hashes[0]),
        "0xc9775c491d677d941c3ac733e1b47b51a64024cb4da22f9c05ceea58d93c76d1"
    );
}

/// [R4.3-1] [R4.3-2] [R4.3-3] [R4.3-4] README §6.4: every field of the preimage, in the order and
/// encoding of the table, for a state with a distinct value in each field.
#[test]
fn preimage_follows_the_readme_layout() {
    let level = open_field();
    let mut game = Game::new(&level, &example_nonce(), 0).unwrap();
    game.ticks_used = 0x0103;
    game.mask = InputMask::new(0x11).unwrap();
    game.grid
        .set(CellIndex::new(Pos { x: 30, y: 58 }).unwrap(), Cell::Trail);
    game.player = Player {
        pos: Pos { x: 30, y: 58 },
        cutting: true,
        cut_start: Pos { x: 30, y: 59 },
        lives: 2,
        shield: 0,
        invulnerable: 7,
        direction: Some(Dir::Left),
        press_tick: 0x0203,
    };
    (game.boss.pos, game.boss.vx, game.boss.vy) = (Pos { x: 40, y: 10 }, 1, -1);
    game.minions.period = 3;
    game.minions.slots = [
        Some(Minion {
            spawn: Pos { x: 0, y: 5 },
            state: State::Live {
                pos: Pos { x: 0, y: 5 },
                heading: Dir::Right,
            },
        }),
        Some(Minion {
            spawn: Pos { x: 59, y: 5 },
            state: State::Destroyed { countdown: 0x0104 },
        }),
        None,
    ];
    (game.captured, game.events_applied) = (0x0506, 0x0708);
    let preimage = preimage(&game);
    let tail: [u8; 44] = [
        30, 58, 1, 30, 59, 2, 0, 7, 3, 2, 3, // player
        40, 10, 5, 0x01, 0xff, 2, // boss
        1, 0, 5, 1, 3, 0, 0, // live minion: x 0, y 5, heading E, period 3
        2, 0, 0, 0, 3, 1, 4, // destroyed minion, countdown 0x0104
        0, 0, 0, 0, 0, 0, 0, // unused slot
        5, 6, 0x0d, 0x24, 7, 8, // captured, claimable 3,364, next event index
    ];
    let fields = (
        &preimage[..4],
        preimage[4],
        preimage[4 + 877],
        &preimage[904..],
    );
    assert_eq!(fields, (&[1, 0x01, 0x02, 0x11][..], 0x55, 0x08, &tail[..]));
}

/// README §6.2: the Recorder's journal replays to the per-tick trace of the live run.
#[test]
fn recorder_journal_replays_to_the_live_trace() {
    let level = open_field();
    let mut recorder = Recorder::new(&level, &header(&level, 0)).unwrap();
    let mut live = Vec::new();
    let (mut state, mut raw) = (0x2545_f491_u32, InputMask::NONE);
    loop {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        if state % 8 == 0 {
            raw = InputMask::new((state >> 8) as u8 & 0x3f).unwrap();
        }
        let end = recorder.step(raw).unwrap();
        live.push(state_hash(recorder.game()));
        if end.is_some() {
            break;
        }
    }
    assert_eq!(trace(&level, recorder.journal()), live);
}

/// README §6.2: the Recorder journals a mask only when it changes.
#[test]
fn recorder_journals_only_changes_of_mask() {
    let level = open_field();
    let mut recorder = Recorder::new(&level, &header(&level, 0)).unwrap();
    for raw in [
        InputMask::NONE,
        InputMask::UP,
        InputMask::UP,
        InputMask::NONE,
    ] {
        recorder.step(raw).unwrap();
    }
    let expected = journal(
        &header(&level, 0),
        &[(1, InputMask::UP), (3, InputMask::NONE)],
    );
    assert_eq!(recorder.journal(), expected.as_slice());
}

fn hex(bytes: &[u8]) -> String {
    let digits: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    format!("0x{digits}")
}

/// [R4.4-6] README §6.4: on the commit tick, step 1 has run: the hash records X as the mask in
/// force and 2 as the next event index.
#[test]
fn commit_tick_records_its_mask_and_event_index() {
    let level = open_field();
    let mut game = Game::new(&level, &example_nonce(), 0).unwrap();
    for mask in [InputMask::UP, InputMask::UP, InputMask::UP, InputMask::X] {
        game.step(mask).unwrap();
    }
    let preimage = preimage(&game);
    assert_eq!((preimage[3], &preimage[946..]), (0x40, &[0, 2][..]));
}

/// README §6.4: the direction codes (1 up, 2 down, 3 left, 4 right) and the heading codes
/// (0 N, 1 E, 2 S, 3 W), at their offsets in the preimage, for every direction.
#[test]
fn preimage_encodes_each_direction_and_heading() {
    let level = open_field();
    let mut game = Game::new(&level, &example_nonce(), 0).unwrap();
    game.ticks_used = 1;
    for (dir, direction_code, heading_code) in [
        (Dir::Up, 1, 0),
        (Dir::Down, 2, 2),
        (Dir::Left, 3, 3),
        (Dir::Right, 4, 1),
    ] {
        game.player.direction = Some(dir);
        game.minions.slots[0] = Some(Minion {
            spawn: Pos { x: 0, y: 5 },
            state: State::Live {
                pos: Pos { x: 0, y: 5 },
                heading: dir,
            },
        });
        let preimage = preimage(&game);
        assert_eq!(
            (preimage[912], preimage[924]),
            (direction_code, heading_code),
            "{dir:?}"
        );
    }
}

/// README §6.3: a broken journal is reported before a broken level.
#[test]
fn journal_is_checked_before_the_level() {
    let journal = journal(&header(&open_field(), 0), &[]);
    let level = encode_level(&open_field());
    assert_eq!(
        replay(&level[..471], &journal[..70], 1),
        Err(Error::Truncated)
    );
}

/// README §6.3: level validity (35 to 39) is checked before the level id (12). The player start
/// (30, 59) is off the rail (39), and the journal names another level.
#[test]
fn level_validity_is_checked_before_the_level_id() {
    let mut level = open_field();
    for x in 29..=31 {
        let cell = 58 * 60 + x;
        level.obstacles[cell / 8] |= 0x80 >> (cell % 8);
    }
    let mut header = header(&open_field(), 0);
    header.level_id = [0; 32];
    let result = replay(&encode_level(&level), &journal(&header, &[]), 1);
    assert_eq!(result, Err(Error::PlayerStartRail));
}

/// README §6.3: the level id (12) is checked before events at or after T (13).
#[test]
fn level_id_is_checked_before_late_events() {
    let level = short_level(10);
    let mut header = header(&level, 0);
    header.level_id = [0; 32];
    let journal = journal(&header, &[(10, InputMask::UP)]);
    assert_eq!(
        replay(&encode_level(&level), &journal, 1),
        Err(Error::LevelId)
    );
}

/// README §6.2: an ended level journals nothing more: a mask after the commit changes neither
/// the journal nor the ticks used.
#[test]
fn recorder_after_the_end_records_nothing() {
    let level = open_field();
    let mut recorder = Recorder::new(&level, &header(&level, 0)).unwrap();
    recorder.step(InputMask::X).unwrap();
    let (journal, ticks_used) = (recorder.journal().to_vec(), recorder.game().ticks_used);
    recorder.step(InputMask::UP).unwrap();
    assert_eq!(
        (recorder.journal(), recorder.game().ticks_used),
        (journal.as_slice(), ticks_used)
    );
}
