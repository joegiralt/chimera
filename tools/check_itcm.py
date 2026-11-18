"""ITCM holds only its listed CPU-only residents (ADR 0266, 0286; standard
library only).

    python3 tools/check_itcm.py ELF

Every symbol in `[__sitcm, __eitcm)` (read from the ELF) must be data
(`b`/`B`/`d`/`D`) named in tools/itcm-residents.txt; a weak, text or any
other kind fails. Absolute symbols (`A`/`a`, linker constants such as
`AXI_MARGIN`) are values, not storage, and are not judged. A resident with
a third field must be in ITCM wherever that symbol is linked: `AUDIO`
wherever the synth is (`take_audio`), not in sd-probe, which has neither.
"""

import pathlib
import subprocess
import sys
from typing import NamedTuple

RESIDENTS = pathlib.Path(__file__).resolve().parent / "itcm-residents.txt"
BOUNDS = ("__sitcm", "__eitcm")


class Resident(NamedTuple):
    ty: str
    needed_with: str | None


def load(text):
    """{symbol: Resident} from the residents file."""
    out = {}
    for n, raw in enumerate(text.splitlines(), 1):
        line = raw.split("#", 1)[0].strip()
        if not line:
            continue
        f = [x.strip() for x in line.split("|")]
        if len(f) not in (2, 3) or not all(f):
            raise ValueError(f"line {n}: want `<symbol> | <type> [| <symbol>]`: {raw!r}")
        if f[0] in out:
            raise ValueError(f"line {n}: {f[0]} twice")
        out[f[0]] = Resident(f[1], f[2] if len(f) == 3 else None)
    return out


def symbols(nm):
    """(address, kind, name) of each defined symbol in `rust-nm -C` output."""
    for raw in nm.splitlines():
        f = raw.split(maxsplit=2)
        if len(f) == 3:
            yield int(f[0], 16), f[1], f[2]


def judge(nm, residents):
    """What is wrong with the ELF's ITCM, one line a fault; [] if nothing."""
    syms = list(symbols(nm))
    at = {n: a for a, _, n in syms if n in BOUNDS}
    missing = [b for b in BOUNDS if b not in at]
    if missing:
        return [f"no {b} in the ELF: is itcm.x linked?" for b in missing]
    lo, hi = at["__sitcm"], at["__eitcm"]
    bad, inside = [], set()
    for a, kind, name in syms:
        if not lo <= a < hi or name in BOUNDS or kind in "Aa":
            continue
        if kind not in "bBdD":
            bad.append(f"{a:08x} {kind} {name}: not data, in ITCM's state (ADR 0286)")
        elif name not in residents:
            bad.append(f"{a:08x} {kind} {name}: in ITCM but not in tools/itcm-residents.txt (ADR 0286)")
        else:
            inside.add(name)
    linked = {n for _, _, n in syms}
    for name, r in residents.items():
        if r.needed_with in linked and name not in inside:
            bad.append(f"{name}: not in ITCM, though {r.needed_with} is linked (ADR 0286)")
    return bad


def main(argv):
    if len(argv) != 2:
        print(__doc__, file=sys.stderr)
        return 2
    nm = subprocess.run(["rust-nm", "-C", argv[1]], check=True, capture_output=True, text=True).stdout
    residents = load(RESIDENTS.read_text())
    bad = judge(nm, residents)
    if bad:
        print(f"{argv[1]}: ITCM:", *bad, sep="\n", file=sys.stderr)
        return 1
    print(f"{argv[1]}: ITCM holds only listed residents")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
