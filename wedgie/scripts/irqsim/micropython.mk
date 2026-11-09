# The irqsim user C module of build-qemu.sh's irqsim firmware: the qemu port's SysTick interrupt,
# wrapped, dispatches a hard IRQ callback (irqsim.c).
SRC_USERMOD_C += $(USERMOD_DIR)/irqsim.c
LDFLAGS_USERMOD += --wrap=SysTick_Handler
