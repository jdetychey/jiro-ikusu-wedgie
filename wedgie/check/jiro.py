# Jiro core check: replays the golden vectors of jiro_vectors.bin through jiro_core.mpy, the Rust
# core built as a native module, and checks what the referee would compute. The result shows on
# the screen and goes to USB. The core's two buffers come first, while the heap is least
# fragmented: the stack needs 48 KB in one block. The check's code is precompiled in
# jiro_check.mpy, because compiling it from source here would scatter its parse tree over that heap.
import gc

STACK_BYTES = 49152  # replay needs up to 46 KB of stack; the rp2 C stack is 8 KB
RECORDER_BYTES = 18568  # jiro_core.recorder_size(), which jiro_check checks


def run():
    gc.collect()
    free_before = gc.mem_free()
    stack, recorder = bytearray(STACK_BYTES), bytearray(RECORDER_BYTES)
    import jiro_core  # its 15.9 KB of code next, then jiro_check's small objects fill the holes it leaves
    import jiro_check
    import lcd
    screen = lcd.LCD()  # wraps the firmware's framebuffer, which the free RAM above already excludes
    lines, failed = jiro_check.check_all(stack, recorder, free_before, screen)
    del stack, recorder
    jiro_check.show(lines, failed, screen)
