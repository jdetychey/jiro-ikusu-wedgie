//! Every vector of vectors/ (README §6.6) against the core. A malformed format vector is
//! rejected with its recorded code, and a valid one re-encodes to its recorded bytes, which are
//! its input (encode(decode(x)) == x). A game vector replays to its recorded Outcome or error,
//! and to its trace.

use std::path::PathBuf;

use jiro_core::consts::{CAPTURE_STACK, MAX_JOURNAL_LEN, RULES_VERSION};
use jiro_core::hash::state_hash;
use jiro_core::journal::{self, Event};
use jiro_core::keccak::keccak256;
use jiro_core::level::{self, level_id};
use jiro_core::replay::{Outcome, replay_observed};
use serde_json::Value;

fn vectors_dir(kind: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../vectors")
        .join(kind)
}

fn format_vectors() -> Vec<(String, Value)> {
    vectors_in("format")
}

fn vectors_in(kind: &str) -> Vec<(String, Value)> {
    let dir = vectors_dir(kind);
    let mut vectors: Vec<(String, Value)> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .map(|path| {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            let text = std::fs::read_to_string(&path).unwrap();
            (name, serde_json::from_str(&text).unwrap())
        })
        .collect();
    vectors.sort_by(|a, b| a.0.cmp(&b.0));
    vectors
}

fn bytes(value: &Value) -> Vec<u8> {
    let hex = value.as_str().unwrap().strip_prefix("0x").unwrap();
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
        .collect()
}

/// The `error` code a vector records, or None for a valid vector.
fn recorded_error(vector: &Value) -> Option<u8> {
    vector
        .get("error")
        .map(|code| u8::try_from(code.as_u64().unwrap()).unwrap())
}

fn check_journal(file: &str, vector: &Value) {
    let input = bytes(&vector["input"]);
    match (journal::decode(&input), recorded_error(vector)) {
        (Err(error), Some(code)) => assert_eq!(error.code(), code, "{file}: field error"),
        (Ok(decoded), None) => {
            let events: Vec<Event> = decoded.events().collect();
            let mut out = [0; MAX_JOURNAL_LEN];
            let reencoded = journal::encode(&decoded.header, &events, &mut out).unwrap();
            assert_eq!(
                reencoded,
                bytes(&vector["reencoded"]),
                "{file}: field reencoded"
            );
            assert_eq!(
                reencoded,
                input.as_slice(),
                "{file}: reencoded differs from input"
            );
        }
        (actual, code) => panic!(
            "{file}: field error: the core returns {:?}, the vector records {code:?}",
            actual.map(|_| ())
        ),
    }
}

fn check_level(file: &str, vector: &Value) {
    let input = bytes(&vector["input"]);
    let mut stack = [0; CAPTURE_STACK];
    let result = level::decode(&input)
        .and_then(|decoded| level::validate(&decoded, &mut stack).map(|_| decoded));
    match (result, recorded_error(vector)) {
        (Err(error), Some(code)) => assert_eq!(error.code(), code, "{file}: field error"),
        (Ok(decoded), None) => {
            assert_eq!(
                level_id(&input).to_vec(),
                bytes(&vector["levelId"]),
                "{file}: field levelId"
            );
            let reencoded = level::encode(&decoded);
            assert_eq!(
                reencoded.to_vec(),
                bytes(&vector["reencoded"]),
                "{file}: field reencoded"
            );
            assert_eq!(
                reencoded.to_vec(),
                input,
                "{file}: reencoded differs from input"
            );
        }
        (actual, code) => panic!(
            "{file}: field error: the core returns {:?}, the vector records {code:?}",
            actual.map(|_| ())
        ),
    }
}

#[test]
fn every_format_vector_matches_the_core() {
    let vectors = format_vectors();
    assert!(!vectors.is_empty(), "no vectors in vectors/format");
    for (file, vector) in &vectors {
        match vector["kind"].as_str() {
            Some("journal") => check_journal(file, vector),
            Some("level") => check_level(file, vector),
            other => panic!("{file}: field kind: unknown kind {other:?}"),
        }
    }
}

fn int(vector: &Value, field: &str) -> u64 {
    vector[field].as_u64().unwrap()
}

/// The Outcome's fields under their README §7.3 names, as the vectors record them.
fn outcome_fields(outcome: &Outcome) -> Vec<(&'static str, Value)> {
    let hex = |bytes: &[u8]| Value::from(format!("0x{}", hex_digits(bytes)));
    vec![
        ("journalHash", hex(&outcome.journal_hash)),
        ("levelId", hex(&outcome.level_id)),
        ("runNonce", hex(&outcome.run_nonce)),
        ("levelIndex", Value::from(outcome.level_index)),
        ("drawSlot", Value::from(outcome.draw_slot)),
        ("mode", Value::from(outcome.mode as u8)),
        ("rulesVersion", Value::from(outcome.rules_version)),
        ("endReason", Value::from(outcome.end_reason as u8)),
        ("claimBp", Value::from(outcome.claim_bp)),
        ("ticksUsed", Value::from(outcome.ticks_used)),
        ("levelScore", Value::from(outcome.level_score)),
    ]
}

fn hex_digits(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// A game vector of `vectors/<kind>/` against the core: its replay inputs, its trace and its
/// Outcome or error.
fn check_game(kind: &str, file: &str, vector: &Value) {
    assert_eq!(
        int(vector, "rulesVersion"),
        u64::from(RULES_VERSION),
        "{file}: field rulesVersion"
    );
    let journal_bytes = bytes(&vector["journal"]);
    let header = journal::decode(&journal_bytes).unwrap().header;
    assert_eq!(
        header.run_nonce.to_vec(),
        bytes(&vector["runNonce"]),
        "{file}: field runNonce"
    );
    assert_eq!(
        u64::from(header.level_index),
        int(vector, "levelIndex"),
        "{file}: field levelIndex"
    );
    assert_eq!(
        header.mode as u64,
        int(vector, "mode"),
        "{file}: field mode"
    );
    let weight_bp = u16::try_from(int(vector, "weightBp")).unwrap();
    let mut trace = Vec::new();
    let result = replay_observed(
        &bytes(&vector["level"]),
        &journal_bytes,
        weight_bp,
        |game| trace.extend(state_hash(game)),
    );
    let recorded =
        std::fs::read(vectors_dir(kind).join(vector["trace"].as_str().unwrap())).unwrap();
    // The trace first: a rule change shows as the first tick whose state differs, the clue that
    // `jiro-tools replay --dump-tick --diff` then turns into field names.
    let first_differing_tick = trace
        .chunks(32)
        .zip(recorded.chunks(32))
        .position(|(computed, recorded)| computed != recorded)
        .or((trace.len() != recorded.len()).then(|| trace.len().min(recorded.len()) / 32));
    assert_eq!(
        first_differing_tick,
        None,
        "{file}: trace: first differing tick ({} ticks here, {} recorded)",
        trace.len() / 32,
        recorded.len() / 32
    );
    assert_eq!(
        keccak256(&[&recorded]).to_vec(),
        bytes(&vector["traceKeccak"]),
        "{file}: field traceKeccak"
    );
    match (result, recorded_error(vector)) {
        (Err(error), Some(code)) => assert_eq!(error.code(), code, "{file}: field error"),
        (Ok(outcome), None) => {
            for (key, value) in outcome_fields(&outcome) {
                assert_eq!(value, vector["outcome"][key], "{file}: field outcome.{key}");
            }
            let keys = vector["outcome"].as_object().unwrap().len();
            assert_eq!(keys, 11, "{file}: field outcome has {keys} keys");
        }
        (actual, code) => panic!(
            "{file}: field error: replay returns {:?}, the vector records {code:?}",
            actual.map(|outcome| outcome.end_reason)
        ),
    }
}

#[test]
fn every_game_vector_matches_the_core() {
    let vectors = vectors_in("game");
    assert!(!vectors.is_empty(), "no vectors in vectors/game");
    for (file, vector) in &vectors {
        assert_eq!(vector["kind"].as_str(), Some("game"), "{file}: field kind");
        check_game("game", file, vector);
    }
}

/// The gate set of M2.9 (plan §4): each level's longest bot journal and synthetic worst case.
#[test]
fn every_gate_set_vector_matches_the_core() {
    let vectors = vectors_in("levels");
    assert_eq!(
        vectors.len(),
        32,
        "vectors/levels holds the 32 gate-set vectors"
    );
    for (file, vector) in &vectors {
        assert_eq!(vector["kind"].as_str(), Some("game"), "{file}: field kind");
        check_game("levels", file, vector);
    }
}

/// The journal and the level of every game vector survive the round trip
/// (encode(decode(x)) == x); a level that decode rejects is the one its vector rejects.
#[test]
fn every_game_vector_round_trips() {
    for (file, vector) in &vectors_in("game") {
        let journal_bytes = bytes(&vector["journal"]);
        let decoded = journal::decode(&journal_bytes).unwrap();
        let events: Vec<Event> = decoded.events().collect();
        let mut out = [0; MAX_JOURNAL_LEN];
        let reencoded = journal::encode(&decoded.header, &events, &mut out).unwrap();
        assert_eq!(
            reencoded,
            journal_bytes.as_slice(),
            "{file}: journal round trip"
        );
        let level_bytes = bytes(&vector["level"]);
        match level::decode(&level_bytes) {
            Ok(level) => assert_eq!(
                level::encode(&level).to_vec(),
                level_bytes,
                "{file}: level round trip"
            ),
            Err(error) => assert_eq!(
                Some(error.code()),
                recorded_error(vector),
                "{file}: the level does not decode"
            ),
        }
    }
}

/// Every journal rejection code of README §6.3 (1-11) and level code of §5 (20-39) has a vector.
#[test]
fn every_decoder_and_validity_code_has_a_vector() {
    let mut codes: Vec<u8> = format_vectors()
        .iter()
        .filter_map(|(_, vector)| recorded_error(vector))
        .collect();
    codes.sort_unstable();
    codes.dedup();
    let expected: Vec<u8> = (1..=11).chain(20..=39).collect();
    assert_eq!(codes, expected);
}
