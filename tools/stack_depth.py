#!/usr/bin/env python3
"""Worst-case stack depth from a firmware disassembly (ADR 0025).

    { llvm-objdump -d -C <elf>; llvm-objdump -s -j .rodata -j .data <elf>; } \
        | python3 tools/stack_depth.py <stack bytes>

Each function's frame is every `push`/`vpush` and `sub sp, #imm` in it (an
overestimate where a prologue is duplicated). Depth is the frame plus the
deepest direct call (`bl`); a tail call (`b`/`b.w` to another function's
start) runs after the frame is popped, so it counts alone. A call back into
the chain (recursion) counts 0 and is named. Indirect calls (`blx rN`) are
each charged the deepest function whose address the image holds (in
`.word` literals, `movw`/`movt` pairs, or the data sections' words when the
`-s` dump follows the disassembly): any pointer's target is one of them.
Pointer calls are assumed to nest at most `POINTER_NEST` deep, and a
recursive direct call to run once (the sorts' and `core::str`'s).

The thread (`main`) and each interrupt entry (`ISRS`) are summed, each
interrupt with its FPU exception frame, as if all nest at once. Exit 1 if
that passes `stack bytes` less `MARGIN`, or a root (but `OPTIONAL`) is missing,
or if the audio interrupt reaches, by direct or tail calls, code of a
small-profile crate (`SMALL`, ADR 0108): a name holding one, its own
function or a generic instantiated with its types.
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
# The small-profile crates (ADR 0108): built for size, so off the audio path.
AUDIO = "DMA1_STR0"
SMALL = ("chimera_board",)
ROOTS = ("Reset", "__stext", "main", "DefaultHandler", "DefaultHandler_") + ISRS
# Pointer calls nest at most this deep: a dyn body (a `Store` write's)
# calling a dyn sink calling `core::fmt`'s writer. An assumption, not a
# proof: the firmware formats only flat values, and no dyn call recurses.
POINTER_NEST = 3

HEADER = re.compile(r"^([0-9a-f]{8}) <(.+)>:$")
# The mnemonic follows the address (and raw bytes, if shown) after a tab.
INSN = re.compile(r"^\s*[0-9a-f]+:[^\t]*\t(\S+)\s*(.*)$")
# A branch's target: the whole demangled name, nested `<…>` and all.
TARGET = re.compile(r"0x[0-9a-f]+ <(.+)>(?:\s+@.*)?$")
# A target inside a function: `<name+0x1c>`.
OFFSET = re.compile(r"\+0x[0-9a-f]+$")


def regs(operands: str) -> int:
    """Registers in a push list: `{r4, r5, lr}` or `{d8-d15}`."""
    n = 0
    for part in operands.strip("{} ").split(","):
        part = part.strip()
        m = re.fullmatch(r"([rsd])(\d+)-[rsd](\d+)", part)
        n += int(m.group(3)) - int(m.group(2)) + 1 if m else 1
    return n


DATA_ROW = re.compile(r"^ ([0-9a-f]+) ((?:[0-9a-f]{8} ?){1,4})")


class Image:
    """Functions as (frame bytes, calls, tail calls, calls through a
    pointer), their start addresses, and every word the image holds."""

    def __init__(self):
        self.funcs, self.starts, self.words = {}, {}, set()


def parse(lines) -> Image:
    """`llvm-objdump -d` output, optionally followed by `-s` of the data
    sections: their words are where function pointers live."""
    img, cur, data, lo = Image(), None, False, {}
    for line in lines:
        line = line.rstrip("\n")
        if line.startswith("Contents of section"):
            data = True
            continue
        if data:
            m = DATA_ROW.match(line)
            if m:
                for g in m.group(2).split():
                    if len(g) == 8:
                        img.words.add(int.from_bytes(bytes.fromhex(g), "little"))
            continue
        h = HEADER.match(line)
        if h:
            cur = h.group(2)
            img.funcs[cur] = [0, set(), set(), False]
            img.starts[cur] = int(h.group(1), 16)
            lo = {}
            continue
        i = INSN.match(line)
        if not i or cur is None:
            continue
        op, args = i.group(1), i.group(2)
        f = img.funcs[cur]
        if op == ".word":
            img.words.add(int(args.split()[0], 0))
        elif op in ("push", "push.w"):
            f[0] += 4 * regs(args)
        elif op == "vpush":
            f[0] += (8 if "d" in args else 4) * regs(args)
        elif op in ("sub", "sub.w", "subw"):
            m = re.match(r"sp,\s*(?:sp,\s*)?#(0x[0-9a-f]+|\d+)", args)
            if m:
                f[0] += int(m.group(1), 0)
        elif op in ("movw", "movt"):
            m = re.match(r"(r\d+|ip|lr),\s*#(0x[0-9a-f]+|\d+)", args)
            if m and op == "movw":
                lo[m.group(1)] = int(m.group(2), 0)
            elif m and m.group(1) in lo:
                img.words.add(int(m.group(2), 0) << 16 | lo.pop(m.group(1)))
        elif op == "bl":
            t = TARGET.search(args)
            if t and not OFFSET.search(t.group(1)):
                f[1].add(t.group(1))
        elif op in ("b", "b.w"):
            t = TARGET.search(args)
            if t and not OFFSET.search(t.group(1)) and t.group(1) != cur:
                f[2].add(t.group(1))
        elif op.startswith("blx") or (op == "bx" and args.strip() != "lr"):
            if re.match(r"(r\d+|ip|lr)\b", args.strip()):
                f[3] = True
    img.funcs = {k: tuple(v) for k, v in img.funcs.items()}
    return img


def depth(funcs, root, charge=0):
    """(bytes, deepest chain, recursive calls cut, functions with pointer
    calls). A pointer call costs `charge`."""
    memo, cut, indirect = {}, set(), set()

    def go(f, on):
        if f in memo:
            return memo[f]
        if f in on:
            cut.add(f)
            return 0, [f + " (recursion)"]
        frame, calls, tails, ind = funcs.get(f, (0, (), (), False))
        best = (0, [])
        if ind:
            indirect.add(f)
            best = (charge, ["(a pointer call, %d B)" % charge])
        on = on | {f}
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


def pointer_charge(img, out):
    """What one pointer call may cost: the deepest function whose address
    the image holds (Thumb bit set), with its own pointer calls charged one
    level less, `POINTER_NEST` levels in all (the innermost charged 0).
    A fixed point doesn't exist: address-taken functions make pointer
    calls themselves (a `Store` body calling a sink), so the nesting is the
    stated assumption."""
    taken = [f for f, a in img.starts.items() if (a | 1) in img.words and f not in ROOTS]
    charge, deepest = 0, "-"
    for _ in range(POINTER_NEST):
        charge, deepest = max(((depth(img.funcs, f, charge)[0], f) for f in taken), default=(0, "-"))
    print(
        "pointers   %6d B  each, %d deep, over %d address-taken functions; deepest %s"
        % (charge, POINTER_NEST, len(taken), short(deepest)),
        file=out,
    )
    return charge


def small_reached(funcs, root=AUDIO):
    """The functions `root` reaches by direct and tail calls whose names
    hold a small-profile crate's, sorted."""
    seen, todo = set(), [root]
    while todo:
        f = todo.pop()
        if f in seen or f not in funcs:
            continue
        seen.add(f)
        todo.extend(funcs[f][1] | funcs[f][2])
    return sorted(f for f in seen if any(c in f for c in SMALL))


def short(name, n=90):
    return name if len(name) <= n else name[: n - 1] + "…"


def check(img, stack_bytes, out):
    """True if every root is found and the nested sum fits."""
    charge = pointer_charge(img, out)
    ok, total = True, 0
    for i, root in enumerate((THREAD,) + ISRS):
        if root not in img.funcs:
            if root not in OPTIONAL:
                print("stack-depth: no %s in the disassembly" % root, file=out)
                ok = False
            continue
        d, chain, cut, ind = depth(img.funcs, root, charge)
        d += EXC_FRAME if i else 0
        total += d
        print("%-10s %6d B  %s" % (root, d, " > ".join(short(c, 60) for c in chain[:6]) + (" > …" if len(chain) > 6 else "")), file=out)
        if cut:
            print("           recursion cut at: %s" % ", ".join(sorted(short(c, 60) for c in cut)), file=out)
        if ind:
            print("           %d functions call through a pointer, each charged above" % len(ind), file=out)
    for f in small_reached(img.funcs):
        print("stack-depth: the audio interrupt reaches %s, built for size" % short(f), file=out)
        ok = False
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
