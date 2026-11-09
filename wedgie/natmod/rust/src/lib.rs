//! jiro-core for a MicroPython dynamic native module (armv6m). Every entry point that runs the
//! core has the shape `fn(*mut Args) -> u32`, so that the C glue can call it on a stack of its
//! own (the rp2 port's C stack is 8 KB; a Game alone is 10 KB). No statics: mpy_ld
//! links no .data and rejects a PC-relative reference to .bss, so every buffer comes from the
//! caller.

#![no_std]

use core::mem::{MaybeUninit, align_of, size_of};
use core::slice;

use jiro_core::Error;
use jiro_core::consts::{MINION_SLOTS, RULES_VERSION, W};
use jiro_core::hash::state_hash;
use jiro_core::journal::{Header, Mode};
use jiro_core::level::{self, level_id};
use jiro_core::minion::State;
use jiro_core::recorder::Recorder;
use jiro_core::replay::{Outcome, level_score, replay_observed};
use jiro_core::types::{CellIndex, InputMask, Pos};
use jiro_shell::dirty::Shadow;
use tiny_keccak::{Hasher, Keccak};

/// The Outcome of README §7.3 in field order, integers big-endian, as jiro-wasm encodes it.
pub const OUTCOME_LEN: usize = 110;
const HASH_LEN: usize = 32;
/// A call the interface does not allow (a weight above 65,535, a mask or mode out of range).
pub const MISUSE: u32 = 255;
/// `jiro_recorder_step` adds this to an error code, to tell it from an end reason (1 to 4).
pub const STEP_ERROR: u32 = 0x100;

unsafe extern "C" {
    /// Raises a MicroPython exception; never returns.
    fn jiro_glue_panic() -> !;
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    // SAFETY: the glue raises, which unwinds to the MicroPython caller's nlr frame.
    unsafe { jiro_glue_panic() }
}

#[unsafe(no_mangle)]
pub extern "C" fn jiro_rules_version() -> u32 {
    u32::from(RULES_VERSION)
}

#[repr(C)]
pub struct ReplayArgs {
    pub level: *const u8,
    pub level_len: usize,
    pub journal: *const u8,
    pub journal_len: usize,
    pub weight_bp: u32,
    /// OUTCOME_LEN bytes, all zero when replay returns an error.
    pub outcome: *mut u8,
    /// `trace_cap` state hashes of 32 bytes, or null.
    pub trace: *mut u8,
    pub trace_cap: usize,
    /// keccak256 of the whole trace, as a vector's `traceKeccak`.
    pub trace_keccak: *mut u8,
    /// The state hash of the last processed tick.
    pub last_hash: *mut u8,
    /// Out: the ticks processed.
    pub ticks: u32,
}

/// Replays a journal on a level (README §6.3): 0 and the Outcome, or the error code.
///
/// # Safety
/// Every pointer of `args` is valid for its length.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn jiro_replay(args: *mut ReplayArgs) -> u32 {
    // SAFETY: the caller's contract.
    let args = unsafe { &mut *args };
    let level = unsafe { slice::from_raw_parts(args.level, args.level_len) };
    let journal = unsafe { slice::from_raw_parts(args.journal, args.journal_len) };
    let outcome_out = unsafe { &mut *args.outcome.cast::<[u8; OUTCOME_LEN]>() };
    let trace: &mut [u8] = if args.trace.is_null() {
        &mut []
    } else {
        unsafe { slice::from_raw_parts_mut(args.trace, args.trace_cap.saturating_mul(HASH_LEN)) }
    };
    let Ok(weight_bp) = u16::try_from(args.weight_bp) else {
        return MISUSE;
    };
    let mut slots = trace.as_chunks_mut::<HASH_LEN>().0.iter_mut();
    let mut keccak = Keccak::v256();
    let mut last = [0; HASH_LEN];
    let mut ticks: u32 = 0;
    let result = replay_observed(level, journal, weight_bp, |game| {
        let hash = state_hash(game);
        keccak.update(&hash);
        if let Some(slot) = slots.next() {
            *slot = hash;
        }
        last = hash;
        ticks = ticks.saturating_add(1);
    });
    keccak.finalize(unsafe { &mut *args.trace_keccak.cast::<[u8; HASH_LEN]>() });
    unsafe { *args.last_hash.cast::<[u8; HASH_LEN]>() = last };
    args.ticks = ticks;
    *outcome_out = result.as_ref().map_or([0; OUTCOME_LEN], encode_outcome);
    code(result.map(|_| ()))
}

/// The marker that `jiro_recorder_start` writes after a Recorder, in the word before it, and
/// that every other entry point checks: "JIRO".
const LIVE_MARKER: u32 = 0x4a49_524f;

/// The caller's recorder buffer: the marker, then the Recorder. A buffer that no successful
/// `jiro_recorder_start` wrote fails the marker check, so its bytes are never read as a Recorder.
#[repr(C)]
pub struct Live {
    marker: u32,
    recorder: MaybeUninit<Recorder>,
}

/// The Recorder of a buffer that `jiro_recorder_start` wrote, or `None`.
///
/// # Safety
/// `live` is valid for `jiro_recorder_size()` bytes, aligned to `jiro_recorder_align()`.
unsafe fn recorder<'a>(live: *const Live) -> Option<&'a Recorder> {
    // SAFETY: the marker is a u32, valid for any bytes; the Recorder is read only behind it.
    unsafe { ((*live).marker == LIVE_MARKER).then(|| (*live).recorder.assume_init_ref()) }
}

/// `recorder`, mutable.
///
/// # Safety
/// As `recorder`.
unsafe fn recorder_mut<'a>(live: *mut Live) -> Option<&'a mut Recorder> {
    unsafe { ((*live).marker == LIVE_MARKER).then(|| (*live).recorder.assume_init_mut()) }
}

#[unsafe(no_mangle)]
pub extern "C" fn jiro_recorder_size() -> usize {
    size_of::<Live>()
}

#[unsafe(no_mangle)]
pub extern "C" fn jiro_recorder_align() -> usize {
    align_of::<Live>()
}

#[repr(C)]
pub struct StartArgs {
    /// `jiro_recorder_size()` bytes aligned to `jiro_recorder_align()`.
    pub recorder: *mut Live,
    pub level: *const u8,
    pub level_len: usize,
    /// 32 bytes.
    pub run_nonce: *const u8,
    pub mode: u32,
    pub level_index: u32,
}

/// Starts a level for live play through the Recorder (README §6.2): 0, or the level's or the
/// header's error code. On 0 the recorder buffer holds a Recorder and the marker; on any other
/// code it holds no marker.
///
/// # Safety
/// Every pointer of `args` is valid for its length.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn jiro_recorder_start(args: *mut StartArgs) -> u32 {
    let args = unsafe { &mut *args };
    let live = args.recorder;
    // SAFETY: the buffer is valid for a Live; only raw places are written, never read.
    unsafe { (&raw mut (*live).marker).write(0) };
    let level_bytes = unsafe { slice::from_raw_parts(args.level, args.level_len) };
    let run_nonce = unsafe { *args.run_nonce.cast::<[u8; HASH_LEN]>() };
    let mode = match args.mode {
        0 => Mode::Training,
        1 => Mode::Challenge,
        _ => return MISUSE,
    };
    let Ok(level_index) = u8::try_from(args.level_index) else {
        return MISUSE;
    };
    let header = Header {
        mode,
        run_nonce,
        level_id: level_id(level_bytes),
        level_index,
    };
    // SAFETY: the buffer is valid for a Live, and a MaybeUninit holds any bytes.
    let place = unsafe { &mut (*live).recorder };
    match level::decode(level_bytes).and_then(|level| Recorder::new_in(place, &level, &header)) {
        Ok(_) => {
            unsafe { (&raw mut (*live).marker).write(LIVE_MARKER) };
            0
        }
        Err(error) => u32::from(error.code()),
    }
}

#[repr(C)]
pub struct StepArgs {
    pub recorder: *mut Live,
    /// The raw input mask of this tick (README §6.1).
    pub mask: u32,
}

/// One tick: 0 while the level runs, its end reason (1 to 4) once it ended, STEP_ERROR plus an
/// error code, or MISUSE for a mask out of range or a buffer without a Recorder.
///
/// # Safety
/// `recorder` is valid for `jiro_recorder_size()` bytes, aligned.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn jiro_recorder_step(args: *mut StepArgs) -> u32 {
    let args = unsafe { &mut *args };
    let Some(recorder) = (unsafe { recorder_mut(args.recorder) }) else {
        return MISUSE;
    };
    let Some(mask) = u8::try_from(args.mask).ok().and_then(InputMask::new) else {
        return MISUSE;
    };
    match recorder.step(mask) {
        Ok(None) => 0,
        Ok(Some(end)) => end as u32,
        Err(error) => STEP_ERROR.saturating_add(u32::from(error.code())),
    }
}

#[repr(C)]
pub struct ViewArgs {
    pub recorder: *const Live,
    /// Out: the journal so far, inside the recorder buffer.
    pub journal: *const u8,
    pub journal_len: usize,
    /// 32 bytes for the state hash after the last tick (README §6.4), or null to skip it.
    pub state_hash: *mut u8,
    /// Out: ticks processed.
    pub ticks: u32,
}

/// The journal so far and the state hash: 0, or MISUSE for a buffer without a Recorder.
///
/// # Safety
/// `recorder` is valid for `jiro_recorder_size()` bytes, aligned; `state_hash` is null or 32
/// bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn jiro_recorder_view(args: *mut ViewArgs) -> u32 {
    let args = unsafe { &mut *args };
    let Some(recorder) = (unsafe { recorder(args.recorder) }) else {
        return MISUSE;
    };
    let journal = recorder.journal();
    args.journal = journal.as_ptr();
    args.journal_len = journal.len();
    if !args.state_hash.is_null() {
        unsafe { *args.state_hash.cast::<[u8; HASH_LEN]>() = state_hash(recorder.game()) };
    }
    args.ticks = u32::from(recorder.game().ticks_used);
    0
}

/// The u16 fields `jiro_status` writes, in this order: end reason (0 while the level runs, else
/// 1 to 4), ticks used, time limit, lives, shield charges, invulnerable ticks, cutting (0 or 1),
/// claim and target in basis points, player x and y, boss x, y and size B, then for each of the
/// 3 minion slots its state (0 unused, 1 live, 2 destroyed), x and y (0 unless live).
pub const STATUS_LEN: usize = 23;
const HEAD_FIELDS: usize = 14;
const MINION_FIELDS: usize = 3;
const _: () = assert!(HEAD_FIELDS + MINION_SLOTS * MINION_FIELDS == STATUS_LEN);

#[unsafe(no_mangle)]
pub extern "C" fn jiro_status_len() -> usize {
    STATUS_LEN
}

#[repr(C)]
pub struct StatusArgs {
    pub recorder: *const Live,
    /// STATUS_LEN u16 values.
    pub out: *mut u16,
}

/// What the renderer draws besides the grid: 0, an error code, or MISUSE for a buffer without a
/// Recorder.
///
/// # Safety
/// `recorder` is valid for `jiro_recorder_size()` bytes, aligned; `out` for STATUS_LEN u16.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn jiro_status(args: *mut StatusArgs) -> u32 {
    let args = unsafe { &mut *args };
    let Some(recorder) = (unsafe { recorder(args.recorder) }) else {
        return MISUSE;
    };
    let game = recorder.game();
    let claim_bp = match game.claim_bp() {
        Ok(claim_bp) => claim_bp,
        Err(error) => return u32::from(error.code()),
    };
    let (player, boss) = (&game.player, &game.boss);
    let mut fields = [0; STATUS_LEN];
    let head: [u16; HEAD_FIELDS] = [
        game.end.map_or(0, |end| end as u16),
        game.ticks_used,
        game.time_limit,
        u16::from(player.lives),
        u16::from(player.shield),
        u16::from(player.invulnerable),
        u16::from(player.cutting),
        claim_bp,
        game.target_bp,
        u16::from(player.pos.x),
        u16::from(player.pos.y),
        u16::from(boss.pos.x),
        u16::from(boss.pos.y),
        u16::from(boss.size),
    ];
    let minions = game.minions.slots.iter().flat_map(|slot| match slot {
        None => [0; MINION_FIELDS],
        Some(minion) => match minion.state {
            State::Live { pos, .. } => [1, u16::from(pos.x), u16::from(pos.y)],
            State::Destroyed { .. } => [2, 0, 0],
        },
    });
    for (field, value) in fields.iter_mut().zip(head.into_iter().chain(minions)) {
        *field = value;
    }
    unsafe { *args.out.cast::<[u16; STATUS_LEN]>() = fields };
    0
}

#[repr(C)]
pub struct ScoreArgs {
    pub recorder: *const Live,
    pub weight_bp: u32,
    /// Out: the level score.
    pub score: u32,
}

/// The ended level's score with `weight_bp`, as the Outcome gives it (README §8.4): 0, an error
/// code, or MISUSE for a level still running, a weight above 65,535 or a buffer without a
/// Recorder.
///
/// # Safety
/// `recorder` is valid for `jiro_recorder_size()` bytes, aligned.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn jiro_score(args: *mut ScoreArgs) -> u32 {
    let args = unsafe { &mut *args };
    let Some(game) = (unsafe { recorder(args.recorder) }).map(Recorder::game) else {
        return MISUSE;
    };
    let (Some(end), Ok(weight_bp)) = (game.end, u16::try_from(args.weight_bp)) else {
        return MISUSE;
    };
    match game.claim_bp() {
        Ok(claim_bp) => {
            args.score = level_score(claim_bp, weight_bp, end);
            0
        }
        Err(error) => u32::from(error.code()),
    }
}

/// The wedgie's framebuffer: 240 × 240 pixels of 4 bits, two to a byte, the left one in the high
/// bits, 120 bytes a row. A cell is 4 × 4 pixels.
pub const FRAME_BYTES: usize = 28_800;
const ROW_BYTES: usize = 120;
const CELL_PX: u16 = 4;

#[unsafe(no_mangle)]
pub extern "C" fn jiro_frame_bytes() -> usize {
    FRAME_BYTES
}

#[unsafe(no_mangle)]
pub extern "C" fn jiro_shadow_size() -> usize {
    size_of::<Shadow>()
}

#[repr(C)]
pub struct PaintArgs {
    pub recorder: *const Live,
    /// `jiro_shadow_size()` bytes, any values.
    pub shadow: *mut Shadow,
    /// FRAME_BYTES bytes.
    pub frame: *mut u8,
    /// The 4-bit color of each cell state (README §4.1), in state order.
    pub colors: *const [u8; 4],
    /// Out: the pixel rectangle (x, y, width, height) that holds the painted cells.
    pub rect: *mut [u16; 4],
    /// Not 0: paint every cell, as if the shadow matched no grid.
    pub full: u32,
    /// Out: the number of cells painted.
    pub count: u32,
}

/// Paints the cells that changed since the shadow was last updated (jiro-shell's diff), or every
/// cell, into the framebuffer: 0, or MISUSE for a color above 15 or a buffer without a Recorder.
///
/// # Safety
/// Every pointer of `args` is valid for its length; `recorder` is aligned.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn jiro_paint(args: *mut PaintArgs) -> u32 {
    let args = unsafe { &mut *args };
    let Some(recorder) = (unsafe { recorder(args.recorder) }) else {
        return MISUSE;
    };
    let colors = unsafe { *args.colors };
    if colors.iter().any(|&color| color > 15) {
        return MISUSE;
    }
    let shadow = unsafe { &mut *args.shadow };
    if args.full != 0 {
        *shadow = Shadow::new();
    }
    let frame = unsafe { slice::from_raw_parts_mut(args.frame, FRAME_BYTES) };
    let (mut count, mut x0, mut y0, mut x1, mut y1) = (0, u16::from(W), u16::from(W), 0, 0);
    shadow.update(&recorder.game().grid, |cell, state| {
        let Pos { x, y } = cell.pos();
        let (x, y) = (u16::from(x), u16::from(y));
        paint_cell(frame, x, y, colors[state as usize]);
        (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x + 1), y1.max(y + 1));
        count += 1;
    });
    let rect = if count == 0 {
        [0; 4]
    } else {
        [x0, y0, x1 - x0, y1 - y0].map(|cells| cells * CELL_PX)
    };
    unsafe { *args.rect = rect };
    args.count = count;
    0
}

#[repr(C)]
pub struct RestoreArgs {
    pub recorder: *const Live,
    /// FRAME_BYTES bytes.
    pub frame: *mut u8,
    pub colors: *const [u8; 4],
    /// The cell rectangle to repaint from the grid, clipped to it.
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// Repaints a rectangle of cells from the grid, under an entity that moved: 0, or MISUSE for a
/// color above 15 or a buffer without a Recorder.
///
/// # Safety
/// Every pointer of `args` is valid for its length; `recorder` is aligned.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn jiro_restore(args: *mut RestoreArgs) -> u32 {
    let args = unsafe { &mut *args };
    let Some(recorder) = (unsafe { recorder(args.recorder) }) else {
        return MISUSE;
    };
    let colors = unsafe { *args.colors };
    if colors.iter().any(|&color| color > 15) {
        return MISUSE;
    }
    let frame = unsafe { slice::from_raw_parts_mut(args.frame, FRAME_BYTES) };
    let grid = &recorder.game().grid;
    let clip = |start: u32, len: u32| {
        let end = start.saturating_add(len).min(u32::from(W));
        (start.min(end) as u8)..(end as u8)
    };
    for y in clip(args.y, args.height) {
        for x in clip(args.x, args.width) {
            if let Some(index) = CellIndex::new(Pos { x, y }) {
                let color = colors[grid.get(index) as usize];
                paint_cell(frame, u16::from(x), u16::from(y), color);
            }
        }
    }
    0
}

/// Fills cell (x, y), inside the grid, with a 4-bit color.
fn paint_cell(frame: &mut [u8], x: u16, y: u16, color: u8) {
    let pair = color << 4 | color;
    for row in 0..CELL_PX {
        let start = usize::from((y * CELL_PX + row) * ROW_BYTES as u16 + x * 2);
        if let Some(bytes) = frame.get_mut(start..start + 2) {
            bytes.fill(pair);
        }
    }
}

fn code(result: Result<(), Error>) -> u32 {
    result.map_or_else(|error| u32::from(error.code()), |()| 0)
}

fn encode_outcome(outcome: &Outcome) -> [u8; OUTCOME_LEN] {
    let fields: [&[u8]; 11] = [
        &outcome.journal_hash,
        &outcome.level_id,
        &outcome.run_nonce,
        &[outcome.level_index],
        &[outcome.draw_slot],
        &[outcome.mode as u8],
        &outcome.rules_version.to_be_bytes(),
        &[outcome.end_reason as u8],
        &outcome.claim_bp.to_be_bytes(),
        &outcome.ticks_used.to_be_bytes(),
        &outcome.level_score.to_be_bytes(),
    ];
    let mut out = [0; OUTCOME_LEN];
    for (slot, byte) in out.iter_mut().zip(fields.into_iter().flatten()) {
        *slot = *byte;
    }
    out
}
