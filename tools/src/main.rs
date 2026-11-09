//! The `replay` command of jiro-ikusu's jiro-tools, alone: it replays a journal on a level with
//! jiro-core and prints the Outcome's fields, as `name value` lines. The play test
//! (wedgie/scripts/qemu_app_check.py) checks the app's journals with it.
//!
//! Usage: jiro-tools replay --level <level.bin> --journal <journal> [--weight <bp>]

use std::fs;
use std::process::ExitCode;

use jiro_core::replay::{Outcome, replay};

const USAGE: &str =
    "usage: jiro-tools replay --level <level.bin> --journal <journal> [--weight <bp>]";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(outcome) => {
            print_outcome(&outcome);
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[String]) -> Result<Outcome, String> {
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let ["replay", rest @ ..] = args.as_slice() else {
        return Err(USAGE.to_string());
    };
    let (mut level, mut journal, mut weight) = (None, None, 10_000_u16);
    for pair in rest.chunks(2) {
        match *pair {
            ["--level", path] => level = Some(read(path)?),
            ["--journal", path] => journal = Some(read(path)?),
            ["--weight", bp] => {
                weight = bp
                    .parse()
                    .map_err(|_| format!("--weight {bp}: not 0 to 65,535"))?
            }
            _ => return Err(USAGE.to_string()),
        }
    }
    let (Some(level), Some(journal)) = (level, journal) else {
        return Err(USAGE.to_string());
    };
    replay(&level, &journal, weight)
        .map_err(|error| format!("replay rejects it: {error:?} (code {})", error.code()))
}

fn read(path: &str) -> Result<Vec<u8>, String> {
    fs::read(path).map_err(|error| format!("{path}: {error}"))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn print_outcome(outcome: &Outcome) {
    println!("journalHash 0x{}", hex(&outcome.journal_hash));
    println!("levelId 0x{}", hex(&outcome.level_id));
    println!("runNonce 0x{}", hex(&outcome.run_nonce));
    println!("levelIndex {}", outcome.level_index);
    println!("drawSlot {}", outcome.draw_slot);
    println!("mode {}", outcome.mode as u8);
    println!("rulesVersion {}", outcome.rules_version);
    println!(
        "endReason {} ({:?})",
        outcome.end_reason as u8, outcome.end_reason
    );
    println!("claimBp {}", outcome.claim_bp);
    println!("ticksUsed {}", outcome.ticks_used);
    println!("levelScore {}", outcome.level_score);
}
