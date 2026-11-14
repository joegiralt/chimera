"""Bank 2's room (ADR 0272): free space per region from the ELF's end
symbols, held to ADR 0251's 8 KB floor on the total and to the set's line
in tools/bank2-budget.txt under bank 1's rules (standard library only).

    python3 tools/bank2_budget.py SET ELF [BUDGET]

The line is bank 2's bytes in use; the room is S0-S3 plus S4-S5 and S6-S7
less each slot's 1 KB reserve and 32 B end marker.
"""

import os
import pathlib
import subprocess
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from bank1_budget import GIVE_BACK, SLACK, load, lower  # noqa: E402

BUDGET = pathlib.Path(__file__).resolve().parent / "bank2-budget.txt"
# (region, end symbol, its start, its limit, note)
REGIONS = (
    ("S0-S3", "__bank2_lo_end", 0x08100000, 0x08180000, ""),
    ("S4-S5", "__bank2_s4_end", 0x08180400, 0x081BFFE0, "(1 KB slot, 32 B marker)"),
    ("S6-S7", "__bank2_s6_end", 0x081C0400, 0x081FFFE0, "(1 KB slot, 32 B marker)"),
)
ROOM = sum(hi - lo for _, _, lo, hi, _ in REGIONS)
# ADR 0251: bank 2 keeps 8 KB free, now over all three regions.
FLOOR = 8192


def run(name, nm, budget_text):
    """(exit code, report) for set `name` from `rust-nm`'s output."""
    syms = {}
    for l in nm.splitlines():
        f = l.split()
        if len(f) == 3:
            syms[f[2]] = int(f[0], 16)
    out, free = [], 0
    for region, sym, lo, hi, note in REGIONS:
        if sym not in syms:
            return 1, f"{sym} missing from the ELF"
        n = hi - syms[sym]
        free += n
        out.append(f"bank 2 {region}   free = {n} B   {note}".rstrip())
    used = ROOM - free
    budgets = load(budget_text)
    if name not in budgets:
        return 1, f"bank 2: no budget for {name}: add its line to tools/bank2-budget.txt (ADR 0272)"
    b = budgets[name]
    out.append(f"bank 2 total   free = {free} B, used {used} B, budget {b} B")
    if free < FLOOR:
        out.append(f"bank 2 of {name} has {free} B free, under ADR 0251's {FLOOR} B floor")
        return 1, "\n".join(out)
    if used > b + SLACK:
        out.append(f"bank 2 of {name} uses {used} B, {used - b} B over its budget of {b} B: raise the line under an ADR")
        return 1, "\n".join(out)
    if used < b - GIVE_BACK:
        out.append(f"bank 2 of {name} uses {used} B, {b - used} B under its budget: lower its line in tools/bank2-budget.txt to {used}")
        return 1, "\n".join(out)
    return 0, "\n".join(out)


def main(argv):
    if len(argv) not in (3, 4):
        print(__doc__, file=sys.stderr)
        return 2
    name, elf = argv[1], argv[2]
    path = pathlib.Path(argv[3] if len(argv) == 4 else BUDGET)
    nm = subprocess.run(["rust-nm", elf], check=True, capture_output=True, text=True).stdout
    text = path.read_text()
    rc, out = run(name, nm, text)
    print(out, file=sys.stderr if rc else sys.stdout)
    if rc and "lower its line" in out and os.environ.get("BANK2_LOWER"):
        used = int(out.split(" uses ")[1].split()[0])
        path.write_text(lower(text, name, used))
        print(f"bank 2 of {name}: its line lowered to {used} B")
        return 0
    return rc


if __name__ == "__main__":
    sys.exit(main(sys.argv))
