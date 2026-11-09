# Builds jiro_core.mpy, an armv6m native module, with MicroPython v1.29.0's py/dynruntime.mk and
# tools/mpy_ld.py, from the C glue and jiro-natmod's object. `bash scripts/bootstrap.sh wedgie`
# installs MicroPython and pyelftools under JIRO_WEDGIE_HOME.
JIRO_WEDGIE_HOME ?= $(HOME)/.cache/jiro-wedgie
MPY_DIR = $(JIRO_WEDGIE_HOME)/micropython
MOD = jiro_core
ARCH = armv6m
BUILD = build
SRC = jiro.c call_on_stack.S
include $(MPY_DIR)/py/dynruntime.mk
PYTHON = $(JIRO_WEDGIE_HOME)/venv/bin/python3
SCRIPTS = ../../scripts
RUST_O = $(BUILD)/jiro_rust.o

# The Rust object goes after the glue's objects, which GNU make lists first because their rule
# holds the recipe: mpy_ld v1.29.0 builds the armv6m entry trampoline from mpy_init's offset in
# its own section, before it lays out sections, so the object defining mpy_init must come first.
$(BUILD)/$(MOD).mpy: $(RUST_O)
# The mpy_ld rules check reads the very object list mpy_ld gets ($^), in its order; `private`
# keeps the prerequisites' recipes on the plain MPY_LD.
$(BUILD)/$(MOD).mpy: private MPY_LD = python3 -I $(SCRIPTS)/check_mpy_ld_rules.py $^ && $(PYTHON) $(MPY_DIR)/tools/mpy_ld.py

# Cargo decides what to rebuild.
.PHONY: $(RUST_O) checks
$(RUST_O):
	bash $(SCRIPTS)/build-core.sh $@

all: checks
checks: $(MOD).mpy
	python3 -I $(SCRIPTS)/check_mpy_header.py $(MOD).mpy
