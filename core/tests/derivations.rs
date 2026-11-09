//! Derivations (README §6.4, §8.2) against vectors/derivations.json, which
//! tools/xcheck/derivations.mjs computes with ethers.

use jiro_core::journal::{journal_hash, run_commitment};
use jiro_core::keccak::keccak256;
use jiro_core::level::{Rect, level_id};
use jiro_core::seed::{BossStart, boss_start, deflection_key, draw, level_seed};
use jiro_core::types::Pos;
use serde_json::Value;

fn fixture() -> Value {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../vectors/derivations.json");
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn entries<'a>(fixture: &'a Value, key: &str) -> &'a Vec<Value> {
    let entries = fixture[key].as_array().unwrap();
    assert!(!entries.is_empty(), "{key} is empty");
    entries
}

fn bytes(value: &Value) -> Vec<u8> {
    let hex = value.as_str().unwrap().strip_prefix("0x").unwrap();
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
        .collect()
}

fn bytes32(value: &Value) -> [u8; 32] {
    bytes(value).try_into().unwrap()
}

fn number<T: TryFrom<u64>>(value: &Value) -> T {
    T::try_from(value.as_u64().unwrap()).ok().unwrap()
}

fn signed(value: &Value) -> i8 {
    i8::try_from(value.as_i64().unwrap()).unwrap()
}

#[test]
fn level_seeds_and_deflection_keys_match_the_fixture() {
    let fixture = fixture();
    for entry in entries(&fixture, "levelSeeds") {
        let seed = level_seed(&bytes32(&entry["runNonce"]), number(&entry["levelIndex"]));
        assert_eq!(seed, bytes32(&entry["seed"]), "{entry}");
        assert_eq!(
            deflection_key(&seed),
            number::<u32>(&entry["deflectionKey"]),
            "{entry}"
        );
    }
}

#[test]
fn level_ids_match_the_fixture() {
    let fixture = fixture();
    for entry in entries(&fixture, "levelIds") {
        assert_eq!(
            level_id(&bytes(&entry["level"])),
            bytes32(&entry["levelId"]),
            "{}",
            entry["name"]
        );
    }
}

#[test]
fn boss_starts_match_the_fixture() {
    let fixture = fixture();
    for entry in entries(&fixture, "bossStarts") {
        let spawn = &entry["spawn"];
        let rect = Rect {
            x0: number(&spawn["x0"]),
            y0: number(&spawn["y0"]),
            x1: number(&spawn["x1"]),
            y1: number(&spawn["y1"]),
        };
        let start = &entry["start"];
        let expected = (!start.is_null()).then(|| BossStart {
            pos: Pos {
                x: number(&start["x"]),
                y: number(&start["y"]),
            },
            vx: signed(&start["vx"]),
            vy: signed(&start["vy"]),
        });
        assert_eq!(
            boss_start(&rect, &bytes32(&entry["seed"])),
            expected,
            "{entry}"
        );
    }
}

#[test]
fn draws_match_the_fixture() {
    let fixture = fixture();
    for entry in entries(&fixture, "draws") {
        let slots: Vec<u8> = entry["slots"]
            .as_array()
            .unwrap()
            .iter()
            .map(number)
            .collect();
        assert_eq!(
            draw(&bytes32(&entry["runNonce"])).to_vec(),
            slots,
            "{entry}"
        );
    }
}

#[test]
fn run_commitments_match_the_fixture() {
    let fixture = fixture();
    for entry in entries(&fixture, "runCommitments") {
        let journals = entry["journals"].as_array().unwrap();
        let hashes: Vec<[u8; 32]> = journals.iter().map(|j| journal_hash(&bytes(j))).collect();
        let expected: Vec<[u8; 32]> = entry["journalHashes"]
            .as_array()
            .unwrap()
            .iter()
            .map(bytes32)
            .collect();
        assert_eq!(hashes, expected);
        let hashes: [[u8; 32]; 6] = hashes.try_into().unwrap();
        assert_eq!(run_commitment(&hashes), bytes32(&entry["runCommitment"]));
    }
}

fn example_run_nonce() -> [u8; 32] {
    keccak256(&[b"jiro.example.9"])
}

/// README §6.4, the worked examples: the level seed, the boss starts and the deflection key.
#[test]
fn seed_and_boss_start_worked_examples_match() {
    let run_nonce = example_run_nonce();
    let seed = level_seed(&run_nonce, 0);
    let open_field = Rect {
        x0: 20,
        y0: 20,
        x1: 34,
        y1: 34,
    };
    let wide = Rect {
        x0: 20,
        y0: 20,
        x1: 34,
        y1: 25,
    };
    let start = |x, y, vx, vy| {
        Some(BossStart {
            pos: Pos { x, y },
            vx,
            vy,
        })
    };
    assert_eq!(
        (
            seed[..4].to_vec(),
            boss_start(&open_field, &seed),
            boss_start(&open_field, &level_seed(&run_nonce, 2)),
            boss_start(&wide, &seed),
            deflection_key(&seed),
        ),
        (
            vec![0x37, 0x47, 0xfc, 0xd2],
            start(23, 22, -1, -1),
            start(20, 32, 1, -1),
            start(23, 25, -1, -1),
            0x9467_6303,
        )
    );
}

/// README §8.2, the worked example of the draw.
#[test]
fn draw_worked_example_matches() {
    assert_eq!(draw(&example_run_nonce()), [1, 0, 10, 14, 12, 2]);
}

/// README §8.2: six different levels out of the 16, for any nonce.
#[test]
#[cfg_attr(debug_assertions, ignore)]
fn draws_of_100000_random_nonces_are_six_distinct_slots_below_16() {
    let mut state: u64 = 0x2545_f491_4f6c_dd1d;
    for case in 0..100_000 {
        let run_nonce: [u8; 32] = std::array::from_fn(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state as u8
        });
        let mut slots = draw(&run_nonce).to_vec();
        slots.sort_unstable();
        slots.dedup();
        assert!(
            slots.len() == 6 && slots.iter().all(|&slot| slot < 16),
            "case {case}"
        );
    }
}
