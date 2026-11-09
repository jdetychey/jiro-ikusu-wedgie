// A stand-in, on MicroPython v1.29.0's qemu port, for the wedgie firmware's hard key IRQ: its
// power.py registers Pin.irq(_touch, Pin.IRQ_FALLING, hard=True) on every key, and the rp2 port
// calls mp_irq_dispatch(handler, parent, true) from the GPIO interrupt, on whatever stack the
// interrupted code is on. Here the port's SysTick interrupt, which ld's --wrap routes through
// __wrap_SysTick_Handler, makes that call at every tick once a callback is armed, and counts the
// ticks that interrupted jiro_core: the thread on the process stack (jiro_call_on_stack), or the
// interrupt's own sp outside the C stack (a stack switch that moved the main stack).
//
//   irqsim.hard(callback): from the next tick on, every tick calls callback(None) as a hard IRQ.
//   irqsim.stats() -> (hard IRQs dispatched, those that interrupted jiro_core, those that failed)
#include "py/runtime.h"

// The qemu port does not build shared/runtime/mpirq.c, which defines mp_irq_dispatch.
#include "shared/runtime/mpirq.c"

MP_REGISTER_ROOT_POINTER(mp_obj_t irqsim_callback);

static volatile uint32_t dispatched, in_jiro_core, failed;

void __real_SysTick_Handler(void);

void __wrap_SysTick_Handler(void) {
    uint32_t exc_return = (uint32_t)__builtin_return_address(0);
    __real_SysTick_Handler();
    mp_obj_t callback = MP_STATE_PORT(irqsim_callback);
    if (callback == MP_OBJ_NULL) {
        return;
    }
    char here;
    bool on_process_stack = (exc_return & 0xf) == 0xd;
    bool off_c_stack = (size_t)(MP_STATE_THREAD(stack_top) - &here) >= MP_STATE_THREAD(stack_limit);
    dispatched++;
    in_jiro_core += on_process_stack || off_c_stack;
    if (mp_irq_dispatch(callback, mp_const_none, true) < 0) {
        failed++;
    }
}

static mp_obj_t irqsim_hard(mp_obj_t callback) {
    dispatched = in_jiro_core = failed = 0;
    MP_STATE_PORT(irqsim_callback) = callback;
    return mp_const_none;
}
static MP_DEFINE_CONST_FUN_OBJ_1(irqsim_hard_obj, irqsim_hard);

static mp_obj_t irqsim_stats(void) {
    mp_obj_t counts[3] = {
        mp_obj_new_int_from_uint(dispatched),
        mp_obj_new_int_from_uint(in_jiro_core),
        mp_obj_new_int_from_uint(failed),
    };
    return mp_obj_new_tuple(3, counts);
}
static MP_DEFINE_CONST_FUN_OBJ_0(irqsim_stats_obj, irqsim_stats);

static const mp_rom_map_elem_t irqsim_module_globals_table[] = {
    { MP_ROM_QSTR(MP_QSTR___name__), MP_ROM_QSTR(MP_QSTR_irqsim) },
    { MP_ROM_QSTR(MP_QSTR_hard), MP_ROM_PTR(&irqsim_hard_obj) },
    { MP_ROM_QSTR(MP_QSTR_stats), MP_ROM_PTR(&irqsim_stats_obj) },
};
static MP_DEFINE_CONST_DICT(irqsim_module_globals, irqsim_module_globals_table);

const mp_obj_module_t irqsim_module = {
    .base = { &mp_type_module },
    .globals = (mp_obj_dict_t *)&irqsim_module_globals,
};

MP_REGISTER_MODULE(MP_QSTR_irqsim, irqsim_module);
