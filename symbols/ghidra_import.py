# Names the functions of LovePlus+ (BOXJ) in a Ghidra program from lp-engine's symbol map (loveplus_plus_boxj.csv):
# each address gets its name as the primary label (and the function's name, where Ghidra has a function there), and
# the note as a plate comment. Static code is labelled in the program's default address space; overlays go to the
# memory blocks named by the pattern you give (default "ov%02d": ov00, ov01, ...), and are skipped if there is none.
#
# Run it from the Script Manager (Jython or PyGhidra). Nothing of the game is in the CSV: only addresses, sizes and
# lp-engine's own names and notes.
# @category LovePlus
# @author lp-engine

import csv

from ghidra.program.model.symbol import SourceType

path = askFile("lp-engine symbol map", "Use this CSV").getAbsolutePath()
pattern = askString("Overlay blocks", "Memory block name for overlay N (Python % pattern)", "ov%02d")
notes = askYesNo("Notes", "Add each function's note as a plate comment?")

mem = currentProgram.getMemory()
st = currentProgram.getSymbolTable()
fm = currentProgram.getFunctionManager()
listing = currentProgram.getListing()
space = currentProgram.getAddressFactory().getDefaultAddressSpace()

done = skipped = 0
missing = set()
with open(path) as f:
    for row in csv.DictReader(f):
        mod = row["module"]
        a = int(row["address"], 16)
        if mod == "static":
            addr = space.getAddress(a)
        else:
            block = mem.getBlock(pattern % int(mod[2:]))
            if block is None:
                missing.add(mod)
                skipped += 1
                continue
            addr = block.getStart().getAddressSpace().getAddress(a)
        if not mem.contains(addr):
            skipped += 1
            continue
        name = row["name"]
        fn = fm.getFunctionAt(addr)
        if fn is not None:
            fn.setName(name, SourceType.IMPORTED)
        else:
            sym = st.createLabel(addr, name, SourceType.IMPORTED)
            sym.setPrimary()
        if notes and row["note"]:
            listing.setComment(addr, 3, "%s [%s, %s]" % (row["note"], row["kind"], row["confidence"]))  # 3 = plate
        done += 1

print("named %d addresses, skipped %d" % (done, skipped))
if missing:
    print("no memory block for: %s (rename the overlay blocks, or change the pattern)" % ", ".join(sorted(missing)))
