"""Checks ARM object files against the rules of MicroPython v1.29.0's tools/mpy_ld.py for
ARCH=armv6m, without running it (load_object_file, link_objects, do_relocation_text,
do_relocation_data, populate_got, generate_entry_point_jump). Reads ELF32 with its own parser.
Give the objects in the order mpy_ld links them: the entry trampoline is only right when
mpy_init's section is the first section mpy_ld lays out.

Usage: python3 -I check_mpy_ld_rules.py <obj.o>...
Exit status 0 when every rule holds; prints the expected native text size and the relocations
the .mpy would carry.
"""

import collections
import struct
import sys

SHT_PROGBITS, SHT_SYMTAB, SHT_NOBITS, SHT_REL, SHT_RELA = 1, 2, 8, 9, 4
EM_ARM = 40
R_ARM_ABS32, R_ARM_REL32, R_ARM_THM_CALL = 2, 3, 10
R_ARM_BASE_PREL, R_ARM_GOT_BREL, R_ARM_THM_JUMP24, R_ARM_GOT_PREL = 25, 26, 30, 96
NAMES = {2: "ABS32", 3: "REL32", 10: "THM_CALL", 25: "BASE_PREL", 26: "GOT_BREL", 30: "THM_JUMP24", 96: "GOT_PREL"}
LOADED = (".literal", ".text", ".rodata", ".data.rel.ro", ".bss")
FUN_TABLE = {
    "mp_type_type", "mp_type_str", "mp_type_list", "mp_type_dict", "mp_type_fun_builtin_0",
    "mp_type_fun_builtin_1", "mp_type_fun_builtin_2", "mp_type_fun_builtin_3",
    "mp_type_fun_builtin_var", "mp_type_Exception", "mp_stream_read_obj",
    "mp_stream_readinto_obj", "mp_stream_unbuffered_readline_obj", "mp_stream_write_obj",
}
SPECIAL = {"_GLOBAL_OFFSET_TABLE_", "mp_fun_table", "mp_native_qstr_table", "mp_native_obj_table"}
MEM = {"memset", "memcpy", "memmove"}


def cstr(data, off):
    return data[off:data.index(b"\0", off)].decode()


class Obj:
    def __init__(self, path):
        self.path = path
        data = open(path, "rb").read()
        assert data[:4] == b"\x7fELF" and data[4] == 1 and data[5] == 1, f"{path}: not ELF32 LE"
        (e_machine,) = struct.unpack_from("<H", data, 18)
        assert e_machine == EM_ARM, f"{path}: e_machine {e_machine}, not EM_ARM"
        e_shoff, = struct.unpack_from("<I", data, 32)
        e_shentsize, e_shnum, e_shstrndx = struct.unpack_from("<HHH", data, 46)
        raw = [struct.unpack_from("<10I", data, e_shoff + i * e_shentsize) for i in range(e_shnum)]
        strtab_off = raw[e_shstrndx][4]
        self.sections = []
        for name, typ, flags, addr, off, size, link, info, align, entsize in raw:
            self.sections.append(dict(name=cstr(data, strtab_off + name), type=typ, off=off,
                                      size=size, link=link, info=info, align=max(align, 1)))
        self.symbols = []
        for sec in self.sections:
            if sec["type"] == SHT_SYMTAB:
                names = self.sections[sec["link"]]["off"]
                for i in range(sec["size"] // 16):
                    st_name, value, size, info, other, shndx = struct.unpack_from("<IIIBBH", data, sec["off"] + 16 * i)
                    self.symbols.append(dict(name=cstr(data, names + st_name), value=value,
                                             bind=info >> 4, type=info & 15, shndx=shndx))
        self.relocs = collections.defaultdict(list)
        for sec in self.sections:
            if sec["type"] == SHT_RELA:
                raise SystemExit(f"{path}: RELA section {sec['name']}: unexpected on ARM")
            if sec["type"] == SHT_REL:
                for i in range(sec["size"] // 8):
                    r_offset, r_info = struct.unpack_from("<II", data, sec["off"] + 8 * i)
                    self.relocs[sec["info"]].append((r_offset, r_info & 0xFF, r_info >> 8))


def main(paths):
    errors = []
    objs = [Obj(p) for p in paths]
    loaded = {}  # (obj index, shndx) -> section name
    known = {}  # global name -> (obj index, section name)
    first_laid_out = None  # (obj index, shndx) of the section at text offset 0
    mpy_init_at = None
    text_size = bss_size = 0
    kinds = collections.Counter()
    for oi, obj in enumerate(objs):
        for idx, sec in enumerate(obj.sections):
            if sec["type"] not in (SHT_PROGBITS, SHT_NOBITS) or sec["size"] == 0:
                continue
            if sec["name"].startswith(LOADED):
                loaded[(oi, idx)] = sec["name"]
                if first_laid_out is None and not sec["name"].startswith((".bss", ".literal")):
                    first_laid_out = (oi, idx)
                if sec["name"].startswith(".bss"):
                    bss_size = -(-bss_size // sec["align"]) * sec["align"] + sec["size"]
                else:
                    text_size = -(-text_size // sec["align"]) * sec["align"] + sec["size"]
            elif sec["name"].startswith(".data"):
                errors.append(f"{obj.path}: {sec['name']} non-empty")
        for sym in obj.symbols:
            if (oi, sym["shndx"]) in loaded and sym["bind"] in (1, 2):
                if sym["name"] in known:
                    errors.append(f"duplicate symbol: {sym['name']}")
                known[sym["name"]] = (oi, loaded[(oi, sym["shndx"])])
                if sym["name"] == "mpy_init":
                    mpy_init_at = (oi, sym["shndx"])
    unresolved = set()
    for oi, obj in enumerate(objs):
        for sym in obj.symbols:
            if sym["shndx"] == 0 and sym["bind"] == 1 and sym["name"]:
                name = sym["name"]
                if name in SPECIAL or name in known or name in FUN_TABLE or name in MEM:
                    continue
                unresolved.add(name)
    errors += [f"undefined symbol: {n}" for n in sorted(unresolved)]
    if "mpy_init" not in known:
        errors.append("no mpy_init")
    elif mpy_init_at != first_laid_out:
        # v1.29.0 builds the armv6m trampoline before it assigns addresses, from mpy_init's offset in
        # its own section, so it jumps there from text offset 0: the order that froze a wedgie.
        first = f"{objs[first_laid_out[0]].path}:{loaded[first_laid_out]}" if first_laid_out else "nothing"
        errors.append(f"mpy_init is in {objs[mpy_init_at[0]].path}:{known['mpy_init'][1]}, but mpy_ld lays out "
                      f"{first} first; link the object defining mpy_init first")

    def target(oi, sym):
        """The section name a relocation's symbol resolves to, as mpy_ld sees it."""
        if sym["shndx"] != 0:
            return loaded.get((oi, sym["shndx"]), f"<unloaded section {sym['shndx']}>")
        name = sym["name"]
        if name in known:
            return known[name][1]
        if name in MEM:
            return f".internal.{name}"
        if name in FUN_TABLE:
            return ".external.mp_fun_table"
        if name in SPECIAL:
            return f".special.{name}"
        return ".external"

    got = set()
    mpy_relocs = 0
    for oi, obj in enumerate(objs):
        for idx, rels in obj.relocs.items():
            sec_name = loaded.get((oi, idx))
            if sec_name is None:
                continue  # relocations of an ignored section are ignored
            for r_offset, rtype, symi in rels:
                sym = obj.symbols[symi]
                tgt = target(oi, sym)
                where = f"{obj.path}:{sec_name}+{r_offset:#x} -> {sym['name'] or tgt}"
                kinds[(sec_name.split(".")[1] if sec_name.count(".") else sec_name, NAMES.get(rtype, rtype), tgt.split(".")[1] if tgt.startswith(".") else tgt)] += 1
                if sec_name.startswith(".data.rel.ro"):
                    if rtype != R_ARM_ABS32:
                        errors.append(f"{where}: relocation {rtype} in .data.rel.ro (only ABS32)")
                    elif not tgt.startswith((".text", ".rodata", ".data.rel.ro", ".bss", ".external.mp_fun_table")):
                        errors.append(f"{where}: ABS32 to {tgt}")
                    else:
                        mpy_relocs += 1
                    continue
                if rtype in (R_ARM_REL32, R_ARM_THM_CALL, R_ARM_THM_JUMP24):
                    if tgt.startswith(".bss"):
                        errors.append(f"{where}: fixed relocation to bss (bss variables can't be static)")
                    elif tgt.startswith((".external", "<unloaded")):
                        errors.append(f"{where}: fixed relocation to external symbol")
                    elif tgt.startswith(".internal."):
                        mpy_relocs += 0  # the stub carries one relocation, counted below
                elif rtype == R_ARM_BASE_PREL:
                    if sym["name"] != "_GLOBAL_OFFSET_TABLE_":
                        errors.append(f"{where}: BASE_PREL to {sym['name']}")
                elif rtype in (R_ARM_GOT_BREL, R_ARM_GOT_PREL):
                    got.add(sym["name"] if sym["name"] else f"{oi}:{sym['shndx']}+{sym['value']}")
                elif rtype == R_ARM_ABS32:
                    if r_offset % 4:
                        errors.append(f"{where}: unaligned ABS32")
                    if not tgt.startswith((".text", ".rodata", ".data.rel.ro", ".bss", ".external.mp_fun_table")):
                        errors.append(f"{where}: ABS32 to {tgt}")
                    mpy_relocs += 1
                else:
                    errors.append(f"{where}: unsupported relocation type {rtype}")
    mem_stubs = {s["name"] for o in objs for s in o.symbols if s["shndx"] == 0 and s["name"] in MEM and s["name"] not in known}
    text_size += 4 * len(got) + 12 * len(mem_stubs)
    mpy_relocs += len(got) + len(mem_stubs)
    print("relocations (section kind, type, target kind): count")
    for k, v in sorted(kinds.items()):
        print(f"  {k}: {v}")
    print(f"GOT entries: {len(got)} {sorted(got)}")
    print(f"memset/memcpy/memmove stubs to mp_fun_table: {sorted(mem_stubs)}")
    print(f"native text (text + rodata + data.rel.ro + GOT, aligned): {text_size} bytes; bss: {bss_size} bytes")
    print(f"relocations the .mpy carries (fixed at import): {mpy_relocs}")
    for e in errors:
        print("ERROR", e)
    print("mpy_ld rules: " + ("PASS" if not errors else f"FAIL ({len(errors)})"))
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
