//! `Game::new_in` and `Recorder::new_in` (plan M3.10), which the wedgie app's native module uses
//! so that starting a level fits a small stack: written in place, they hold what `new` returns.

use std::mem::MaybeUninit;
use std::path::Path;

use jiro_core::Error;
use jiro_core::game::Game;
use jiro_core::journal::{Header, Mode};
use jiro_core::keccak::keccak256;
use jiro_core::level::{self, Level, level_id};
use jiro_core::recorder::Recorder;
use jiro_core::types::InputMask;

/// The 16 shipped levels' bytes, by file name.
fn shipped_levels() -> Vec<(String, Vec<u8>)> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../levels");
    let mut levels: Vec<(String, Vec<u8>)> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "bin"))
        .map(|path| {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            (name, std::fs::read(&path).unwrap())
        })
        .collect();
    levels.sort();
    assert_eq!(levels.len(), 16);
    levels
}

fn header(bytes: &[u8], level_index: u8) -> Header {
    Header {
        mode: Mode::Training,
        run_nonce: keccak256(&[bytes, &[level_index]]),
        level_id: level_id(bytes),
        level_index,
    }
}

/// The first shipped level with an obstacle on its boss's spawn area, which validity rejects
/// [R4.1-9].
fn level_with_a_covered_spawn() -> Level {
    let (_, bytes) = &shipped_levels()[0];
    let mut level = level::decode(bytes).unwrap();
    let cell = usize::from(level.spawn.y0) * 60 + usize::from(level.spawn.x0);
    level.obstacles[cell / 8] |= 0x80 >> (cell % 8);
    level
}

#[test]
fn game_new_in_writes_what_new_returns() {
    for (name, bytes) in shipped_levels() {
        let level = level::decode(&bytes).unwrap();
        for level_index in [0, 5] {
            let run_nonce = header(&bytes, level_index).run_nonce;
            let expected = Game::new(&level, &run_nonce, level_index).unwrap();
            let mut place = MaybeUninit::uninit();
            let game = Game::new_in(&mut place, &level, &run_nonce, level_index).unwrap();
            assert_eq!(
                format!("{game:?}"),
                format!("{expected:?}"),
                "{name}, level index {level_index}"
            );
        }
    }
}

#[test]
fn game_new_in_rejects_what_new_rejects() {
    let level = level_with_a_covered_spawn();
    let run_nonce = keccak256(&[b"jiro.in-place"]);
    let mut place = MaybeUninit::uninit();
    assert_eq!(
        Game::new(&level, &run_nonce, 0).err(),
        Some(Error::BossSpawn)
    );
    assert_eq!(
        Game::new_in(&mut place, &level, &run_nonce, 0).err(),
        Some(Error::BossSpawn)
    );
}

#[test]
fn recorder_new_in_writes_what_new_returns() {
    for (name, bytes) in shipped_levels() {
        let level = level::decode(&bytes).unwrap();
        let header = header(&bytes, 3);
        let expected = Recorder::new(&level, &header).unwrap();
        let mut place = MaybeUninit::uninit();
        let recorder = Recorder::new_in(&mut place, &level, &header).unwrap();
        assert_eq!(format!("{recorder:?}"), format!("{expected:?}"), "{name}");
    }
}

/// The native module starts every level in the same buffer: nothing of a level played in it
/// stays.
#[test]
fn recorder_new_in_rewrites_a_place_that_held_a_played_level() {
    let levels = shipped_levels();
    let (first, second) = (&levels[0].1, &levels[15].1);
    let mut place = MaybeUninit::uninit();
    let played = Recorder::new_in(
        &mut place,
        &level::decode(first).unwrap(),
        &header(first, 0),
    )
    .unwrap();
    for tick in 0..200_u16 {
        let mask = [
            InputMask::LEFT,
            InputMask::UP,
            InputMask::RIGHT,
            InputMask::DOWN,
        ];
        played.step(mask[usize::from(tick / 20 % 4)]).unwrap();
    }
    let level = level::decode(second).unwrap();
    let expected = Recorder::new(&level, &header(second, 1)).unwrap();
    let recorder = Recorder::new_in(&mut place, &level, &header(second, 1)).unwrap();
    assert_eq!(format!("{recorder:?}"), format!("{expected:?}"));
}
