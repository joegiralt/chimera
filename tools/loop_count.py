#!/usr/bin/env python3
"""Static loop counts for the filter probes (spec § Cost, ADR 0056).

    python3 tools/loop_count.py <elf> <symbol-substring>

Disassembles the ELF (`llvm-objdump -d -C --mcpu=cortex-m7`), finds the one
function whose name holds the substring, and counts each innermost loop: a
head's backward branches' span, head to last back-edge, that holds no other.
A loop with an `if` in it (the filter's `saturate`) also gets its fast and
slow paths round, the instructions one sample runs with every `if` skipped
or taken. LLVM unrolls some loops, so each loop's stride (samples a time
round) is read from its induction register: a byte offset stepped up by `K`
and compared with an immediate `#I` runs `K` samples a time round if `I` ≤ 64
(a sample index), else `K/4` (a byte offset over `f32`s); one counted down by
`subs rN, #K` from `#I` runs `64·K/I`. With neither, the
stride is unknown and the counts are a time round. Every instruction outside
the loops is `block_instr` (per-block work, which the bill divides by 64). Literal-pool words are not counted.
Prints each loop's instructions, `vdiv`s, `vsqrt`s and cycles:
`ceil5(instr × 1.46 × 1.1 + 12 × (vdiv + vsqrt))`. Standard library only.
"""

import re
from fractions import Fraction
import shutil
import subprocess
import sys

HEAD = re.compile(r"^[0-9a-f]+ <(.*)>:$")
INSN = re.compile(r"^\s*([0-9a-f]+):\s+(\S+)\s*(.*)$")
DATA = re.compile(r"\s\.(?:word|short|byte|long)\b")
TARGET = re.compile(r"^(?:[rR]\d+,\s*)?(?:0x)?([0-9a-f]+)\b")


def cycles(instr, divs):
    """`instr × 1.46 × 1.1 + 12 × divs`, rounded up to 5. Exact (ints or
    `Fraction`s, in thousandths), so 25.000 never rounds to 30 on a float's
    last bit."""
    milli = instr * 1606 + 12000 * divs
    return -(-milli // 5000) * 5


def _function(disasm, symbol):
    found, body = [], None
    for line in disasm.splitlines():
        h = HEAD.match(line.strip())
        if h:
            body = [] if symbol in h.group(1) else None
            if body is not None:
                found.append((h.group(1), body))
            continue
        m = INSN.match(line) if body is not None else None
        if m and not DATA.search(line):
            body.append((int(m.group(1), 16), m.group(2), m.group(3)))
    if len(found) != 1:
        names = ", ".join(n for n, _ in found) or "none"
        raise SystemExit(f"{symbol!r}: want one function, found {names}")
    return found[0]


CALL = re.compile(r"^blx?(?:\.w)?$")
COND = "eq|ne|cs|hs|cc|lo|mi|pl|vs|vc|hi|ls|ge|lt|gt|le"
BRANCH = re.compile(rf"^(?:b({COND})?|cbn?z)(?:\.[wn])?$")


def _branch(op, args):
    """(target, conditional) of a direct branch, or None."""
    m = BRANCH.match(op)
    t = TARGET.match(args) if m else None
    if not t:
        return None
    return int(t.group(1), 16), op.startswith("cb") or m.group(1) is not None


def _ends(op, args):
    """Leaves the function or jumps by table: no path round goes on."""
    regs = args.split(",")[0]
    return (
        op.startswith(("bx", "tbb", "tbh"))
        or (op.startswith("pop") and "pc" in args)
        or (op.startswith(("ldr", "mov")) and regs.strip() == "pc")
    )


def _loops(body):
    """Each innermost loop as (head, last back-edge): a head's back-edges
    are one loop, LLVM's rotated `if`s give it several."""
    lo, hi = body[0][0], body[-1][0]
    edges = {}
    for addr, op, args in body:
        b = _branch(op, args)
        if b and lo <= b[0] < addr <= hi:
            edges[b[0]] = max(edges.get(b[0], addr), addr)
    spans = list(edges.items())
    return sorted(
        s for s in spans
        if not any(o != s and s[0] <= o[0] and o[1] <= s[1] for o in spans)
    )


def _paths(ins, head):
    """Fewest and most instructions from `head` round to a back-edge: the
    loop's paths, each `if` taken or skipped. `ins` is the span in order."""
    at = {a: i for i, (a, _, _) in enumerate(ins)}
    best = [None] * len(ins)
    for i in range(len(ins) - 1, -1, -1):
        a, op, args = ins[i]
        b = _branch(op, args)
        nxt = []
        if _ends(op, args):
            pass
        elif b is None or b[1]:
            nxt.append(i + 1)
        if b is not None:
            nxt.append("back" if b[0] == head else at.get(b[0]))
        ends = [(1, 1) if n == "back" else (1 + best[n][0], 1 + best[n][1])
                for n in nxt if n == "back" or (n is not None and n < len(ins)
                                                 and best[n] is not None)]
        best[i] = (min(e[0] for e in ends), max(e[1] for e in ends)) if ends else None
    return best[0]


IMM = re.compile(r"^(r\d+|lr|ip),\s*(?:\1,\s*)?#(0x[0-9a-f]+|\d+)$")
MOV = re.compile(r"^(r\d+|lr|ip),\s*#(0x[0-9a-f]+|\d+)$")


def _stride(ins, before):
    """Samples a time round, or None if unknown. A register the loop adds an
    immediate `K` to and compares with an immediate `I` counts up to the
    block's end: a sample index if `I` ≤ 64 (`K` a time round), else a byte
    offset over `f32`s (`K/4`). `I` may stop a step or two short of the end
    (LLVM peels the last rounds), so the stride is never `I`'s quotient. A
    register compared with a register (an end pointer) is unknown. One it
    counts down with the last flag-setting `subs` round by `K` and
    nothing compares, from the last `mov` of an immediate `I` before the
    loop (`before`), runs `I/K` times round a block: `64·K/I`."""
    adds, subs, cmps = {}, {}, {}
    for _, op, args in ins:
        base = op.split(".")[0]
        if base == "cmp":
            c = IMM.match(args)
            cmps[args.split(",")[0].strip()] = int(c.group(2), 0) if c else None
        m = IMM.match(args)
        if not m:
            continue
        if base in ("adds", "add"):
            adds[m.group(1)] = int(m.group(2), 0)
        elif base == "subs":
            subs[m.group(1)] = int(m.group(2), 0)
    up = [(k, cmps[r]) for r, k in adds.items() if r in cmps]
    if len(up) == 1:
        k, i = up[0]
        if i is None:
            return None
        if i <= 64:
            return k
        return k // 4 if k % 4 == 0 else None
    # The counter is the last `subs` round: its flags take the back-edge
    # (an earlier one may be a float's bits, e.g. `exp2_poly`'s exponent).
    down = [(r, k) for r, k in subs.items() if r not in cmps]
    if not down:
        return None
    r, k = down[-1]
    for _, op, args in reversed(before):
        m = MOV.match(args) if op.split(".")[0] in ("mov", "movs", "movw") else None
        if m and m.group(1) == r:
            i = int(m.group(2), 0)
            return 64 * k // i if i and (64 * k) % i == 0 else None
        if args.split(",")[0].strip() == r:
            return None  # written some other way: unknown
    return None


def count(disasm, symbol):
    """`loop_instr` (head to last back-edge), `fast` and `slow` (the
    shortest and longest path round), `vdiv`, `vsqrt` and `stride` (samples
    a time round, None if unknown) of the first innermost loop;
    `block_instr` outside every loop; and `loops`, each innermost loop's
    counts in address order. Exits if a loop calls out (`bl`, `blx`):
    the callee's cost wouldn't be counted."""
    name, body = _function(disasm, symbol)
    loops = []
    for head, edge in _loops(body):
        ins = [x for x in body if head <= x[0] <= edge]
        ops = [op for _, op, _ in ins]
        calls = [f"{a:#x}" for a, op, _ in ins if CALL.match(op)]
        if calls:
            raise SystemExit(f"{name}: a call in the loop at {', '.join(calls)}"
                             " (its cost isn't counted)")
        d = sum(op.startswith("vdiv") for op in ops)
        r = sum(op.startswith("vsqrt") for op in ops)
        fast, slow = _paths(ins, head)
        loops.append({
            "head": head, "loop_instr": len(ops), "fast": fast, "slow": slow,
            "vdiv": d, "vsqrt": r,
            "stride": _stride(ins, [x for x in body if x[0] < head]),
        })
    in_loop = sum(l["loop_instr"] for l in loops)
    looped = {a for l in _loops(body) for a, _, _ in body if l[0] <= a <= l[1]}
    block_div = sum(
        op.startswith(("vdiv", "vsqrt")) for a, op, _ in body if a not in looped
    )
    first = loops[0] if loops else {
        "loop_instr": 0, "fast": 0, "slow": 0, "vdiv": 0, "vsqrt": 0, "stride": 1}
    return {
        "name": name,
        **{k: first[k] for k in ("loop_instr", "fast", "slow", "vdiv", "vsqrt", "stride")},
        "block_instr": len(body) - in_loop,
        "block_div": block_div,
        "loops": loops,
    }


def _objdump():
    for tool in ("llvm-objdump", "rust-objdump"):
        if shutil.which(tool):
            return tool
    sysroot = subprocess.run(
        ["rustc", "--print", "sysroot"], capture_output=True, text=True, check=True
    ).stdout.strip()
    import glob
    hits = glob.glob(f"{sysroot}/lib/rustlib/*/bin/llvm-objdump")
    if not hits:
        raise SystemExit("no llvm-objdump: rustup component add llvm-tools")
    return hits[0]


def main(argv):
    if len(argv) != 3:
        raise SystemExit(__doc__)
    dis = subprocess.run(
        [_objdump(), "-d", "-C", "--no-show-raw-insn", "--mcpu=cortex-m7", argv[1]],
        capture_output=True, text=True, check=True,
    ).stdout
    c = count(dis, argv[2])
    print(c["name"])
    for l in c["loops"]:
        n = l["vdiv"] + l["vsqrt"]
        print(
            f"  loop @{l['head']:#x}: {l['loop_instr']} instr, {l['vdiv']} vdiv, "
            f"{l['vsqrt']} vsqrt; cycles = ceil5(instr·1.606 + 12·{n}) = "
            f"{cycles(l['loop_instr'], n)}"
        )
        if l["fast"] != l["slow"]:
            print(
                f"    paths: fast {l['fast']} instr = {cycles(l['fast'], n)}, "
                f"slow {l['slow']} instr = {cycles(l['slow'], n)}"
            )
        k = l["stride"]
        if k is None:
            print("    stride unknown: counts are a time round, not a sample")
        elif k > 1:
            per = [Fraction(x, k) for x in (l["fast"], l["slow"])]
            paths = (f"{float(per[0]):.2f}" if l["fast"] == l["slow"]
                     else f"fast {float(per[0]):.2f}, slow {float(per[1]):.2f}")
            print(f"    unrolled {k} ×: a sample {paths} instr, "
                  f"{float(Fraction(n, k)):.2f} vdiv/vsqrt")
    b, bd = Fraction(c["block_instr"], 64), Fraction(c["block_div"], 64)
    print(
        f"  block: {c['block_instr']} instr, {c['block_div']} vdiv/vsqrt "
        f"(over 64: {float(b):.2f} instr, {float(bd):.3f} div a sample)"
    )
    for l in c["loops"]:
        k = l["stride"] or 1
        n = Fraction(l["vdiv"] + l["vsqrt"], k) + bd
        paths = [l["loop_instr"]] if l["fast"] == l["slow"] else [l["fast"], l["slow"]]
        each = ", ".join(
            f"{float(Fraction(p, k)):.2f} + {float(b):.2f} = {cycles(Fraction(p, k) + b, n)}"
            for p in paths)
        per = "a sample" if l["stride"] else "a time round (stride unknown)"
        print(f"  loop @{l['head']:#x} {per}, with the block: {each}")


if __name__ == "__main__":
    main(sys.argv)
