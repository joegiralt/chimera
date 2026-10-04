#!/usr/bin/env python3
"""Worst-case stack depth from a firmware disassembly (ADR 0025).

    llvm-objdump -d --no-show-raw-insn -C <elf> | python3 tools/stack_depth.py <stack bytes>

Each function's frame is every `push`/`vpush` and `sub sp, #imm` in it (an
overestimate where a prologue is duplicated). Depth is the frame plus the
deepest direct call (`bl`); a tail call (`b`/`b.w` to another function's
start) runs after the frame is popped, so it counts alone. A call back into
the chain (recursion) counts 0 and is named. Indirect calls (`blx rN`) are
not followed: the depth is a lower bound by their callees' frames, and the
count of functions holding one is printed.

The thread (`main`) and each interrupt entry (`ISRS`) are summed, each
interrupt with its FPU exception frame, as if all nest at once. Exit 1 if
that passes `stack bytes` less `MARGIN`, or a root (but `OPTIONAL`) is missing.
Standard library only.
"""

import re
import sys

ISRS = ("DMA1_STR0", "USART1", "SysTick", "HardFault")
# A build without MIDI DIN has no USART1; the rest are always there.
OPTIONAL = ("USART1",)
THREAD = "main"  # what `Reset` calls; its own frame is a few words
# An exception's stacked frame with the FPU's lazy state: 26 words, aligned.
EXC_FRAME = 104
MARGIN = 2048

HEADER = re.compile(r"^[0-9a-f]{8} <(.+)>:$")
INSN = re.compile(r"^\s*[0-9a-f]+:\s+(\S+)\s*(.*)$")
TARGET = re.compile(r"<([^<>]+)>")


def regs(operands: str) -> int:
    """Registers in a push list: `{r4, r5, lr}` or `{d8-d15}`."""
    n = 0
    for part in operands.strip("{} ").split(","):
        part = part.strip()
        m = re.fullmatch(r"([rsd])(\d+)-[rsd](\d+)", part)
        n += int(m.group(3)) - int(m.group(2)) + 1 if m else 1
    return n


def parse(lines):
    """{function: (frame bytes, calls, tail calls, indirect)}."""
    funcs, cur = {}, None
    for line in lines:
        line = line.rstrip("\n")
        h = HEADER.match(line)
        if h:
            cur = h.group(1)
            funcs[cur] = [0, set(), set(), False]
            continue
        i = INSN.match(line)
        if not i or cur is None:
            continue
        op, args = i.group(1), i.group(2)
        f = funcs[cur]
        if op in ("push", "push.w"):
            f[0] += 4 * regs(args)
        elif op == "vpush":
            f[0] += (8 if "d" in args else 4) * regs(args)
        elif op in ("sub", "sub.w", "subw"):
            m = re.match(r"sp,\s*(?:sp,\s*)?#(0x[0-9a-f]+|\d+)", args)
            if m:
                f[0] += int(m.group(1), 0)
        elif op == "bl":
            t = TARGET.search(args)
            if t:
                f[1].add(t.group(1))
        elif op in ("b", "b.w"):
            t = TARGET.search(args)
            if t and "+" not in t.group(1) and t.group(1) != cur:
                f[2].add(t.group(1))
        elif op.startswith("blx") and re.match(r"r\d+|lr|ip", args.strip()):
            f[3] = True
    return {k: tuple(v) for k, v in funcs.items()}


def depth(funcs, root):
    """(bytes, deepest chain, recursive calls cut, functions with indirect calls)."""
    memo, cut, indirect = {}, set(), set()

    def go(f, on):
        if f in memo:
            return memo[f]
        if f in on:
            cut.add(f)
            return 0, [f + " (recursion)"]
        frame, calls, tails, ind = funcs.get(f, (0, (), (), False))
        if ind:
            indirect.add(f)
        on = on | {f}
        best = (0, [])
        for c in calls:
            d = go(c, on)
            if d[0] > best[0]:
                best = d
        own = (frame + best[0], [f] + best[1])
        for t in tails:
            d = go(t, on)
            if d[0] > own[0]:
                own = d
        memo[f] = own
        return own

    total, chain = go(root, frozenset())
    return total, chain, cut, indirect


def check(funcs, stack_bytes, out):
    """True if every root is found and the nested sum fits."""
    ok, total = True, 0
    for i, root in enumerate((THREAD,) + ISRS):
        if root not in funcs:
            if root not in OPTIONAL:
                print("stack-depth: no %s in the disassembly" % root, file=out)
                ok = False
            continue
        d, chain, cut, ind = depth(funcs, root)
        d += EXC_FRAME if i else 0
        total += d
        print("%-10s %6d B  %s" % (root, d, " > ".join(chain[:6]) + (" > …" if len(chain) > 6 else "")), file=out)
        if cut:
            print("           recursion cut at: %s" % ", ".join(sorted(cut)), file=out)
        if ind:
            print("           %d functions call through a pointer (not followed)" % len(ind), file=out)
    limit = stack_bytes - MARGIN
    print("nested     %6d B of %d (%d less a %d B margin)" % (total, stack_bytes, limit, MARGIN), file=out)
    if total > limit:
        print("stack-depth: the thread and every interrupt nested pass the stack", file=out)
        ok = False
    return ok


def main(argv) -> int:
    if len(argv) != 1:
        print(__doc__.strip(), file=sys.stderr)
        return 2
    return 0 if check(parse(sys.stdin), int(argv[0], 0), sys.stdout) else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
