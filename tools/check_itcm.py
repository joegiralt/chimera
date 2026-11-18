"""ITCM holds only its listed CPU-only residents (ADR 0266, 0286; standard
library only).

    python3 tools/check_itcm.py ELF

Every symbol in `[__sitcm, __eitcm)` (read from the ELF) must be data
(`b`, `B`, `d`, `D`) named in tools/itcm-residents.txt; a weak, text or any
other kind fails. Absolute symbols (`A`, `a`: linker constants such as
`AXI_MARGIN`) are values, not storage, and are not judged. A listed
resident linked anywhere must be inside, and one whose owner is linked
must be there: `AUDIO` wherever `take_audio` is, not in sd-probe, which
links no synth and leaves ITCM empty. A line of `rust-nm` output this
can't read fails too.
"""

import pathlib
import subprocess
import sys
from typing import NamedTuple

RESIDENTS = pathlib.Path(__file__).resolve().parent / "itcm-residents.txt"
BOUNDS = ("__sitcm", "__eitcm")
DATA = frozenset({"b", "B", "d", "D"})
ABSOLUTE = frozenset({"A", "a"})
# Kinds `rust-nm` prints with no address: undefined, weak undefined.
UNDEFINED = frozenset({"U", "w", "v"})


class Resident(NamedTuple):
    ty: str
    count: int
    owner: str | None


def load(text):
    """{symbol: Resident} from the residents file."""
    out = {}
    for n, raw in enumerate(text.splitlines(), 1):
        line = raw.split("#", 1)[0].strip()
        if not line:
            continue
        f = [x.strip() for x in line.split("|")]
        if len(f) not in (3, 4) or not all(f) or not f[2].isdigit() or int(f[2]) < 1:
            raise ValueError(f"line {n}: want `<symbol> | <type> | <count> [| <owner>]`: {raw!r}")
        if f[0] in out:
            raise ValueError(f"line {n}: {f[0]} twice")
        out[f[0]] = Resident(f[1], int(f[2]), f[3] if len(f) == 4 else None)
    return out


def symbols(nm):
    """(address, kind, name) of each defined symbol in `rust-nm -C` output;
    ValueError on a line that is neither that nor an undefined one."""
    out = []
    for n, raw in enumerate(nm.splitlines(), 1):
        if not raw.strip():
            continue
        f = raw.split(maxsplit=2)
        if len(f) == 3 and len(f[1]) == 1:
            try:
                out.append((int(f[0], 16), f[1], f[2]))
                continue
            except ValueError:
                pass
        elif len(f) == 2 and f[0] in UNDEFINED:
            continue
        raise ValueError(f"nm line {n}: unreadable: {raw!r}")
    return out


def judge(nm, residents):
    """What is wrong with the ELF's ITCM, one line a fault; [] if nothing."""
    try:
        syms = symbols(nm)
    except ValueError as e:
        return [str(e)]
    at = {n: a for a, _, n in syms if n in BOUNDS}
    missing = [b for b in BOUNDS if b not in at]
    if missing:
        return [f"no {b} in the ELF: is itcm.x linked?" for b in missing]
    lo, hi = at["__sitcm"], at["__eitcm"]
    bad, inside, outside = [], set(), set()
    for a, kind, name in syms:
        if name in BOUNDS or kind in ABSOLUTE:
            continue
        if not lo <= a < hi:
            if name in residents:
                outside.add(name)
            continue
        if kind not in DATA:
            bad.append(f"{a:08x} {kind} {name}: not data, in ITCM's state (ADR 0286)")
        elif name not in residents:
            bad.append(f"{a:08x} {kind} {name}: in ITCM but not in tools/itcm-residents.txt (ADR 0286)")
        else:
            inside.add(name)
    linked = {n for _, _, n in syms}
    for name, r in residents.items():
        if name in outside:
            bad.append(f"{name}: linked outside ITCM (ADR 0286)")
        elif r.owner in linked and name not in inside:
            bad.append(f"{name}: not in ITCM, though {r.owner} is linked (ADR 0286)")
    return bad


def main(argv):
    if len(argv) != 2:
        print(__doc__, file=sys.stderr)
        return 2
    nm = subprocess.run(["rust-nm", "-C", argv[1]], check=True, capture_output=True, text=True).stdout
    bad = judge(nm, load(RESIDENTS.read_text()))
    if bad:
        print(f"{argv[1]}: ITCM:", *bad, sep="\n", file=sys.stderr)
        return 1
    print(f"{argv[1]}: ITCM holds only listed residents")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
