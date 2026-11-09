// MicroPython dynamic native module "jiro_core": jiro-core (natmod/rust, linked in as
// build/jiro_rust.o) behind a Python API. The caller owns all memory: a stack buffer, of at least
// 24 KB for `start`, `step` and `state_hash` and 48 KB for `replay` (the rp2 port's C stack is
// 8 KB; starting a level needs about 19 KB and a replay about 46 KB, since replay builds its game
// on the stack, as the referees do), and the Recorder buffer, both bytearrays made once, first,
// while the heap is whole. `start`, `step`, `state_hash` and `replay` run on that stack; the other
// calls only read the Recorder or paint cells, in small frames, on the C stack. A recorder buffer
// holds a marker after a successful `start`, and every other call that takes it raises ValueError
// without one. The buffers of one call must not overlap, since the core writes some while it
// reads others: ValueError.
//
//   rules_version() -> int
//   recorder_size() -> int
//   shadow_size() -> int, the bytes of a shadow for `paint`
//   start(recorder, level, mode, run_nonce, level_index, stack) -> code (0 or README §5/§6.3)
//   step(recorder, mask, stack) -> 0 running, 1-4 end reason, 0x100 + code on error
//   journal(recorder[, start[, end]]) -> bytes, the journal so far, or its slice [start:] or
//                            [start:end]
//   journal_equals(recorder, buffer) -> bool, whether the journal so far equals the buffer,
//                                       without copying it
//   state_hash(recorder, stack) -> bytes (32)
//   status(recorder, out) -> None; writes the 23 u16 fields of jiro_status (natmod/rust) into
//                            out, e.g. array('H', 23): end reason, ticks, lives, claim, entities
//   score(recorder, weight_bp) -> int, the ended level's score (README §8.4)
//   paint(recorder, shadow, frame, colors, rect[, full]) -> int, the cells that changed since
//                            the shadow was last painted (jiro-shell's diff), or with full true
//                            every cell, painted into the 4-bit frame buffer with colors[state]
//                            (4 bytes); their pixel rectangle (x, y, w, h) goes to rect, e.g.
//                            array('H', 4)
//   restore(recorder, frame, colors, x, y, width, height) -> None; repaints a rectangle of cells
//                            from the grid; ValueError for a negative x, y, width or height
//   replay(level, journal, weight_bp, stack[, trace]) -> (code, outcome, ticks, trace_keccak,
//                                                        last_hash)

#include "py/dynruntime.h"
#include "jiro_api.h"

#define PLAY_STACK_MIN (24 * 1024)
#define REPLAY_STACK_MIN (48 * 1024)

void jiro_raise_panic(void) {
    mp_raise_msg(&mp_type_RuntimeError, MP_ERROR_TEXT("jiro-core panic"));
}

static void *stack_top(mp_obj_t stack_obj, size_t min_len) {
    mp_buffer_info_t stack;
    mp_get_buffer_raise(stack_obj, &stack, MP_BUFFER_RW);
    if (stack.len < min_len) {
        mp_raise_ValueError(MP_ERROR_TEXT("stack buffer too small for this call"));
    }
    return (void *)(((uintptr_t)stack.buf + stack.len) & ~(uintptr_t)7);
}

// Raises for a code other than 0: ValueError for a misuse, RuntimeError(code) for a core error.
static void check(uint32_t code) {
    if (code == JIRO_MISUSE) {
        mp_raise_ValueError(MP_ERROR_TEXT("no started level in the recorder buffer, or an argument out of range"));
    }
    if (code != 0) {
        mp_raise_type_arg(&mp_type_RuntimeError, mp_obj_new_int(code));
    }
}

// A writable buffer of at least `len` bytes, aligned to `align`.
static void *rw_buf(mp_obj_t obj, size_t len, uintptr_t align) {
    mp_buffer_info_t buffer;
    mp_get_buffer_raise(obj, &buffer, MP_BUFFER_RW);
    if (buffer.len < len || ((uintptr_t)buffer.buf & (align - 1))) {
        mp_raise_ValueError(MP_ERROR_TEXT("buffer too small or misaligned"));
    }
    return buffer.buf;
}

static const uint8_t *colors_buf(mp_obj_t obj) {
    mp_buffer_info_t colors;
    mp_get_buffer_raise(obj, &colors, MP_BUFFER_READ);
    if (colors.len != 4) {
        mp_raise_ValueError(MP_ERROR_TEXT("colors is 4 bytes"));
    }
    return colors.buf;
}

// The bytes of a buffer that the core reads or writes.
typedef struct {
    const void *buf;
    size_t len;
} region_t;

// Raises ValueError if two of the regions overlap.
static void check_disjoint(const region_t *regions, size_t count) {
    for (size_t i = 0; i < count; i++) {
        for (size_t j = i + 1; j < count; j++) {
            uintptr_t a = (uintptr_t)regions[i].buf, b = (uintptr_t)regions[j].buf;
            if (a < b + regions[j].len && b < a + regions[i].len) {
                mp_raise_ValueError(MP_ERROR_TEXT("buffers overlap"));
            }
        }
    }
}

static void *recorder_buf(mp_obj_t recorder_obj) {
    mp_buffer_info_t recorder;
    mp_get_buffer_raise(recorder_obj, &recorder, MP_BUFFER_RW);
    if (recorder.len < jiro_recorder_size() || ((uintptr_t)recorder.buf & (jiro_recorder_align() - 1))) {
        mp_raise_ValueError(MP_ERROR_TEXT("recorder buffer too small or misaligned"));
    }
    return recorder.buf;
}

static mp_obj_t rules_version(void) {
    return mp_obj_new_int(jiro_rules_version());
}
static MP_DEFINE_CONST_FUN_OBJ_0(rules_version_obj, rules_version);

static mp_obj_t recorder_size(void) {
    return mp_obj_new_int(jiro_recorder_size());
}
static MP_DEFINE_CONST_FUN_OBJ_0(recorder_size_obj, recorder_size);

static mp_obj_t shadow_size(void) {
    return mp_obj_new_int(jiro_shadow_size());
}
static MP_DEFINE_CONST_FUN_OBJ_0(shadow_size_obj, shadow_size);

static mp_obj_t start(size_t n_args, const mp_obj_t *args) {
    mp_buffer_info_t level, nonce;
    mp_get_buffer_raise(args[1], &level, MP_BUFFER_READ);
    mp_get_buffer_raise(args[3], &nonce, MP_BUFFER_READ);
    if (nonce.len != JIRO_HASH_LEN) {
        mp_raise_ValueError(MP_ERROR_TEXT("run_nonce is 32 bytes"));
    }
    jiro_start_args_t start_args = {
        .recorder = recorder_buf(args[0]),
        .level = level.buf,
        .level_len = level.len,
        .run_nonce = nonce.buf,
        .mode = mp_obj_get_int(args[2]),
        .level_index = mp_obj_get_int(args[4]),
    };
    uint32_t code = jiro_call_on_stack(&start_args, stack_top(args[5], PLAY_STACK_MIN), (jiro_entry_t)jiro_recorder_start);
    return mp_obj_new_int(code);
}
static MP_DEFINE_CONST_FUN_OBJ_VAR_BETWEEN(start_obj, 6, 6, start);

static mp_obj_t step(mp_obj_t recorder_obj, mp_obj_t mask_obj, mp_obj_t stack_obj) {
    jiro_step_args_t step_args = {
        .recorder = recorder_buf(recorder_obj),
        .mask = mp_obj_get_int(mask_obj),
    };
    uint32_t code = jiro_call_on_stack(&step_args, stack_top(stack_obj, PLAY_STACK_MIN), (jiro_entry_t)jiro_recorder_step);
    if (code == JIRO_MISUSE) {
        check(code);
    }
    return mp_obj_new_int(code);
}
static MP_DEFINE_CONST_FUN_OBJ_3(step_obj, step);

static mp_obj_t journal(size_t n_args, const mp_obj_t *args) {
    jiro_view_args_t view = {.recorder = recorder_buf(args[0]), .state_hash = NULL};
    check(jiro_recorder_view(&view));
    size_t start = 0, end = view.journal_len;
    if (n_args > 1) {
        mp_int_t from = mp_obj_get_int(args[1]);
        mp_int_t to = n_args == 3 ? mp_obj_get_int(args[2]) : MP_INT_MAX;
        if (from < 0 || to < from) {
            mp_raise_ValueError(MP_ERROR_TEXT("journal slice out of range"));
        }
        start = (size_t)from < end ? (size_t)from : end;
        end = (size_t)to < end ? (size_t)to : end;
    }
    return mp_obj_new_bytes(view.journal + start, end - start);
}
static MP_DEFINE_CONST_FUN_OBJ_VAR_BETWEEN(journal_obj, 1, 3, journal);

static mp_obj_t journal_equals(mp_obj_t recorder_obj, mp_obj_t buffer_obj) {
    mp_buffer_info_t buffer;
    mp_get_buffer_raise(buffer_obj, &buffer, MP_BUFFER_READ);
    jiro_view_args_t view = {.recorder = recorder_buf(recorder_obj), .state_hash = NULL};
    check(jiro_recorder_view(&view));
    const uint8_t *expected = buffer.buf;
    bool equal = view.journal_len == buffer.len;
    for (size_t i = 0; equal && i < buffer.len; i++) {
        equal = view.journal[i] == expected[i];
    }
    return mp_obj_new_bool(equal);
}
static MP_DEFINE_CONST_FUN_OBJ_2(journal_equals_obj, journal_equals);

static mp_obj_t state_hash(mp_obj_t recorder_obj, mp_obj_t stack_obj) {
    uint8_t hash[JIRO_HASH_LEN];
    jiro_view_args_t view = {.recorder = recorder_buf(recorder_obj), .state_hash = hash};
    check(jiro_call_on_stack(&view, stack_top(stack_obj, PLAY_STACK_MIN), (jiro_entry_t)jiro_recorder_view));
    return mp_obj_new_bytes(hash, JIRO_HASH_LEN);
}
static MP_DEFINE_CONST_FUN_OBJ_2(state_hash_obj, state_hash);

static mp_obj_t status(mp_obj_t recorder_obj, mp_obj_t out_obj) {
    size_t out_len = jiro_status_len() * sizeof(uint16_t);
    jiro_status_args_t status_args = {
        .recorder = recorder_buf(recorder_obj),
        .out = rw_buf(out_obj, out_len, sizeof(uint16_t)),
    };
    region_t regions[] = {{status_args.recorder, jiro_recorder_size()}, {status_args.out, out_len}};
    check_disjoint(regions, MP_ARRAY_SIZE(regions));
    check(jiro_status(&status_args));
    return mp_const_none;
}
static MP_DEFINE_CONST_FUN_OBJ_2(status_obj, status);

static mp_obj_t score(mp_obj_t recorder_obj, mp_obj_t weight_obj) {
    jiro_score_args_t score_args = {.recorder = recorder_buf(recorder_obj), .weight_bp = mp_obj_get_int(weight_obj)};
    check(jiro_score(&score_args));
    return mp_obj_new_int(score_args.score);
}
static MP_DEFINE_CONST_FUN_OBJ_2(score_obj, score);

static mp_obj_t paint(size_t n_args, const mp_obj_t *args) {
    jiro_paint_args_t paint_args = {
        .recorder = recorder_buf(args[0]),
        .shadow = rw_buf(args[1], jiro_shadow_size(), 1),
        .frame = rw_buf(args[2], jiro_frame_bytes(), 1),
        .colors = colors_buf(args[3]),
        .rect = rw_buf(args[4], 4 * sizeof(uint16_t), sizeof(uint16_t)),
        .full = n_args > 5 && mp_obj_is_true(args[5]),
    };
    region_t regions[] = {
        {paint_args.recorder, jiro_recorder_size()},
        {paint_args.shadow, jiro_shadow_size()},
        {paint_args.frame, jiro_frame_bytes()},
        {paint_args.rect, 4 * sizeof(uint16_t)},
    };
    check_disjoint(regions, MP_ARRAY_SIZE(regions));
    check(jiro_paint(&paint_args));
    return mp_obj_new_int(paint_args.count);
}
static MP_DEFINE_CONST_FUN_OBJ_VAR_BETWEEN(paint_obj, 5, 6, paint);

static mp_obj_t restore(size_t n_args, const mp_obj_t *args) {
    jiro_restore_args_t restore_args = {
        .recorder = recorder_buf(args[0]),
        .frame = rw_buf(args[1], jiro_frame_bytes(), 1),
        .colors = colors_buf(args[2]),
        .x = mp_obj_get_int(args[3]),
        .y = mp_obj_get_int(args[4]),
        .width = mp_obj_get_int(args[5]),
        .height = mp_obj_get_int(args[6]),
    };
    // A negative int has the sign bit set as a uint32_t.
    if ((restore_args.x | restore_args.y | restore_args.width | restore_args.height) >> 31) {
        mp_raise_ValueError(MP_ERROR_TEXT("restore takes no negative cells"));
    }
    region_t regions[] = {{restore_args.recorder, jiro_recorder_size()}, {restore_args.frame, jiro_frame_bytes()}};
    check_disjoint(regions, MP_ARRAY_SIZE(regions));
    check(jiro_restore(&restore_args));
    return mp_const_none;
}
static MP_DEFINE_CONST_FUN_OBJ_VAR_BETWEEN(restore_obj, 7, 7, restore);

static mp_obj_t replay(size_t n_args, const mp_obj_t *args) {
    mp_buffer_info_t level, journal_buf, trace = {.buf = NULL, .len = 0};
    mp_get_buffer_raise(args[0], &level, MP_BUFFER_READ);
    mp_get_buffer_raise(args[1], &journal_buf, MP_BUFFER_READ);
    if (n_args > 4 && args[4] != mp_const_none) {
        mp_get_buffer_raise(args[4], &trace, MP_BUFFER_RW);
    }
    uint8_t outcome[JIRO_OUTCOME_LEN], keccak[JIRO_HASH_LEN], last[JIRO_HASH_LEN];
    jiro_replay_args_t replay_args = {
        .level = level.buf,
        .level_len = level.len,
        .journal = journal_buf.buf,
        .journal_len = journal_buf.len,
        .weight_bp = mp_obj_get_int(args[2]),
        .outcome = outcome,
        .trace = trace.buf,
        .trace_cap = trace.len / JIRO_HASH_LEN,
        .trace_keccak = keccak,
        .last_hash = last,
    };
    uint32_t code = jiro_call_on_stack(&replay_args, stack_top(args[3], REPLAY_STACK_MIN), (jiro_entry_t)jiro_replay);
    mp_obj_t items[5] = {
        mp_obj_new_int(code),
        mp_obj_new_bytes(outcome, JIRO_OUTCOME_LEN),
        mp_obj_new_int(replay_args.ticks),
        mp_obj_new_bytes(keccak, JIRO_HASH_LEN),
        mp_obj_new_bytes(last, JIRO_HASH_LEN),
    };
    return mp_obj_new_tuple(5, items);
}
static MP_DEFINE_CONST_FUN_OBJ_VAR_BETWEEN(replay_obj, 4, 5, replay);

mp_obj_t mpy_init(mp_obj_fun_bc_t *self, size_t n_args, size_t n_kw, mp_obj_t *args) {
    MP_DYNRUNTIME_INIT_ENTRY

    mp_store_global(MP_QSTR_rules_version, MP_OBJ_FROM_PTR(&rules_version_obj));
    mp_store_global(MP_QSTR_recorder_size, MP_OBJ_FROM_PTR(&recorder_size_obj));
    mp_store_global(MP_QSTR_start, MP_OBJ_FROM_PTR(&start_obj));
    mp_store_global(MP_QSTR_step, MP_OBJ_FROM_PTR(&step_obj));
    mp_store_global(MP_QSTR_journal, MP_OBJ_FROM_PTR(&journal_obj));
    mp_store_global(MP_QSTR_journal_equals, MP_OBJ_FROM_PTR(&journal_equals_obj));
    mp_store_global(MP_QSTR_state_hash, MP_OBJ_FROM_PTR(&state_hash_obj));
    mp_store_global(MP_QSTR_replay, MP_OBJ_FROM_PTR(&replay_obj));
    mp_store_global(MP_QSTR_shadow_size, MP_OBJ_FROM_PTR(&shadow_size_obj));
    mp_store_global(MP_QSTR_status, MP_OBJ_FROM_PTR(&status_obj));
    mp_store_global(MP_QSTR_score, MP_OBJ_FROM_PTR(&score_obj));
    mp_store_global(MP_QSTR_paint, MP_OBJ_FROM_PTR(&paint_obj));
    mp_store_global(MP_QSTR_restore, MP_OBJ_FROM_PTR(&restore_obj));

    MP_DYNRUNTIME_INIT_EXIT
}
