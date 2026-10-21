"""Bank 1 is frozen (ADR 0198): fail a firmware set whose bank-1 .bin
outgrows its line in tools/bank1-budget.txt (standard library only).

    python3 tools/bank1_budget.py SET BIN [BUDGET]

SET is the set's features as `just check` builds it (`default` for none),
BIN its bank-1 image (`rust-objcopy -R .bank2`).
"""

import os
import pathlib
import re
import sys

BUDGET = pathlib.Path(__file__).resolve().parent / "bank1-budget.txt"
# Alignment and inlining noise: a build may sit this far over its line.
SLACK = 64
# Freed room is given back: this far under, the line must come down.
GIVE_BACK = 1024
ADR = re.compile(r"\bADR \d{4}\b")


def load(text):
    """{set: bytes} from the budget file; every line names its ADR."""
    out = {}
    for n, raw in enumerate(text.splitlines(), 1):
        line, _, note = raw.partition("#")
        if not line.strip():
            continue
        f = line.split()
        if len(f) != 2 or not f[1].isdigit():
            raise ValueError(f"line {n}: want `<set> <bytes>  # ADR NNNN: why`: {raw!r}")
        if not ADR.search(note):
            raise ValueError(f"line {n}: {f[0]}'s budget names no ADR (a raise is an ADR exception): {raw!r}")
        if f[0] in out:
            raise ValueError(f"line {n}: {f[0]} twice")
        out[f[0]] = int(f[1])
    return out


def judge(name, size, budgets):
    """None if `size` keeps `name`'s budget, else why not."""
    if name not in budgets:
        return f"bank 1: no budget for {name}: add its line to tools/bank1-budget.txt (ADR 0198)"
    b = budgets[name]
    if size > b + SLACK:
        return (
            f"bank 1 of {name} is {size} B, {size - b} B over its budget of {b} B.\n"
            "Bank 1 is frozen (ADR 0198): move the new code behind a bank-2 door\n"
            "(ADR 0137; as SemDoor, FuzzDoor or the reverb's), leaving bank 1 a stub.\n"
            "Only an ADR exception raises the budget: edit tools/bank1-budget.txt\n"
            "with a comment naming that ADR."
        )
    if size < b - GIVE_BACK:
        return (
            f"bank 1 of {name} is {size} B, {b - size} B under its budget of {b} B:\n"
            f"lower its line in tools/bank1-budget.txt to {size} so the room stays bank 2's."
        )
    return None


def main(argv):
    if len(argv) not in (3, 4):
        print(__doc__, file=sys.stderr)
        return 2
    name, binp = argv[1], argv[2]
    budgets = load(pathlib.Path(argv[3] if len(argv) == 4 else BUDGET).read_text())
    size = os.stat(binp).st_size
    why = judge(name, size, budgets)
    if why:
        print(why, file=sys.stderr)
        return 1
    print(f"bank 1 of {name}: {size} B, budget {budgets[name]} B")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
