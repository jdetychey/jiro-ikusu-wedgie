@ uint32_t jiro_call_on_stack(void *arg, void *stack_top, uint32_t (*fn)(void *))
@ Calls fn(arg) on the process stack, with psp = stack_top (8-byte aligned), then returns to the
@ main stack, whose sp the call left as it was. The rp2 port's C stack is 8 KB; jiro-core needs
@ more, so the glue runs it on a heap buffer.
@
@ Exception handlers always run on the main stack, so an interrupt during the call stays on the C
@ stack and puts only its 32-byte frame on the heap buffer. The wedgie firmware's hard key IRQ
@ runs a Python callback, whose stack check fails on any other stack; printing that failure
@ escapes the handler and leaves the GC and the scheduler locked, which froze the app.
@ Thread mode must be privileged and on the main stack (CONTROL = 0), as on the rp2 and qemu ports.
    .syntax unified
    .cpu cortex-m0
    .thumb
    .section .text.jiro_call_on_stack,"ax",%progbits
    .global jiro_call_on_stack
    .type jiro_call_on_stack, %function
    .thumb_func
jiro_call_on_stack:
    push {r4, lr}                @ r4 keeps the main stack 8-byte aligned
    msr psp, r1
    movs r3, #2
    msr control, r3
    isb
    blx r2
    movs r3, #0
    msr control, r3
    isb
    pop {r4, pc}
    .size jiro_call_on_stack, .-jiro_call_on_stack

@ void jiro_glue_panic(void), which jiro-core's panic handler calls: back on the main stack, which
@ is the C stack below jiro_call_on_stack's frame when the core runs on the heap buffer, it
@ raises to the MicroPython caller.
    .section .text.jiro_glue_panic,"ax",%progbits
    .global jiro_glue_panic
    .type jiro_glue_panic, %function
    .thumb_func
jiro_glue_panic:
    movs r0, #0
    msr control, r0
    isb
    bl jiro_raise_panic
    .size jiro_glue_panic, .-jiro_glue_panic
