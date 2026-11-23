#!/usr/bin/env python3
# Copyright (C) 2026 Joe Giralt
# SPDX-License-Identifier: GPL-3.0-or-later

"""Worst-case stack depth from a firmware disassembly (ADR 0025).

    { llvm-objdump -d --mcpu=cortex-m7 -C <elf>; llvm-objdump -s -j .rodata -j .data -j .bank2 <elf>; } \
        | python3 tools/stack_depth.py <stack bytes>

Each function's frame is every `push`/`vpush` and `sub sp, #imm` in it (an
overestimate where a prologue is duplicated). Depth is the frame plus the
deepest direct call (`bl`); a tail call (`b`/`b.w` to another function's
start) runs after the frame is popped, so it counts alone. A call back into
the chain (recursion) counts 0 and is named. Indirect calls (`blx rN`) are
each charged the deepest function whose address the image holds (in
`.word` literals but a `tbb`/`tbh`'s table, `movw`/`movt` pairs, or the data sections' words when the
`-s` dump follows the disassembly, less any word inside a disassembled
function: `.bank2` holds the door's table, ADR 0137, among bank 2's code,
whose instruction bytes are not pointers): any pointer's target is one of
them.
Pointer calls are assumed to nest at most `POINTER_NEST` deep, and a
recursive direct call to run once (the sorts' and `core::str`'s).

The thread (`main`) and each interrupt entry (`ISRS`) are summed, each
interrupt with its FPU exception frame, as if all nest at once; the
interrupts of one priority (`ONE_LEVEL`) never nest each other, so that
level is charged its deepest. Exit 1 if
that passes `stack bytes` less `MARGIN`, or a root (but `OPTIONAL`) is missing,
or if the audio interrupt reaches, by direct or tail calls, code of a
small-profile crate (`SMALL`, ADR 0108): a name holding one, its own
function or a generic instantiated with its types.
Standard library only.
"""

import bisect
import re
import sys

ISRS = ("DMA1_STR0", "USART1", "OTG_FS", "DMA2_STR0", "SysTick", "HardFault")
# A build without MIDI DIN has no USART1, one without `midi-usb` no OTG_FS;
# the rest are always there (the card's DMA2_STR0 in every build but the SD
# probe, which has no check).
OPTIONAL = ("USART1", "OTG_FS")
# Interrupts at one priority: they never nest each other (MIDI's, ADR 0217
# and the USB MIDI spec § 5).
ONE_LEVEL = (("USART1", "OTG_FS"),)
THREAD = "main"  # what `Reset` calls; its own frame is a few words
# An exception's stacked frame with the FPU's lazy state: 26 words, aligned.
EXC_FRAME = 104
MARGIN = 2048
# The small-profile crates (ADR 0108): built for size, so off the audio and
# MIDI interrupts' paths. Calls through a pointer are not followed: a target
# can't be told from the disassembly (the audio tree has two, a `dyn Block`
# and `TapShape`'s, both into `chimera_engine` today), so the check holds
# for direct and tail calls only. A small crate's generic compiled in the
# binary still carries its own path: it passes only while it is inlined.
FAST_ROOTS = ("DMA1_STR0", "USART1")
SMALL = ("chimera_board", "chimera_core")
ROOTS = ("Reset", "__stext", "main", "DefaultHandler", "DefaultHandler_") + ISRS
# Pointer calls nest at most this deep: a dyn body (a `Store` write's)
# calling a dyn sink calling `core::fmt`'s writer. An assumption, not a
# proof: the firmware formats only flat values, and no dyn call recurses.
POINTER_NEST = 3
# A root whose pointer calls are door entries (ADR 0175) whose targets are
# known: charged the deepest target's depth, not every address-taken
# function's. More pointer calls in such a root than named entries, or a
# named entry missing while the root makes a pointer call, fail the check. The card interrupt's shim calls `IrqState::step` through the
# board's slot; the DIN interrupt calls MIDI's `feed` and `idle` through
# bank 2's door (ADR 0217). USB's one poll calls bank 2's through the
# board's slot, and USB MIDI's descriptor shim the writer its class holds.
DOOR_CALLS = {
    "DMA2_STR0": ("<chimera_board::card_dma::IrqState>::step",),
    "USART1": (
        "<chimera_engine::midi_in::DinPipe>::door_feed",
        "<chimera_engine::midi_in::DinPipe>::door_idle",
    ),
    "<chimera_board::usb::UsbCore>::poll_all": ("chimera_board::usb_midi::door::poll",),
    # `midi_in::feed`'s door call, if out of line.
    "<chimera_engine::bank2::DoorCell>::with::<(), chimera_engine::midi_in::feed::{closure#0}>": (
        "<chimera_engine::midi_in::DinPipe>::door_feed",
    ),
    "<chimera_board::usb_midi::MidiClass as usb_device::class::UsbClass<synopsys_usb_otg::bus::UsbBus<chimera_board::usb::Otg2>>>::get_configuration_descriptors": (
        "chimera_board::usb_midi::door::descriptors",
    ),
    "<chimera_board::usb_midi::MidiClass as usb_device::class::UsbClass<synopsys_usb_otg::bus::UsbBus<chimera_board::usb::Otg2>>>::get_string": (
        "chimera_midi::usb_class::jack_name",
    ),
}
# The USB device's poll calls its classes through `dyn UsbClass`: each such
# pointer call is charged the deepest class method linked (a name holding
# `CLASS`), not every address-taken function. No class method linked while
# the poll makes a pointer call fails the check.
CLASS_CALLS = ("<usb_device::device::UsbDevice<synopsys_usb_otg::bus::UsbBus<chimera_board::usb::Otg2>>>::poll",)
CLASS = " as usb_device::class::UsbClass<"
# A door entry that only its named callers reach (ADR 0194), its address
# held by the door's table alone: out of the address-taken set, so no other
# pointer call is charged it; each caller's pointer calls are charged the
# deeper of it and the usual charge. The bench's entry, read from its slot
# in `main`'s `bench`, which the link inlines into `synth`. With the entry
# linked but no named caller making a pointer call, the check fails: the
# call moved where it would go uncharged.
DOOR_ONLY = {
    "chimera_stm32::bench::enter": ("chimera_board::synth", "chimera_stm32::bench"),
}

# Functions only ever called directly, never through a pointer, whose
# address a plain data word has matched by chance: a `BlockDef`'s packed
# bytes read 0x08050401 when the link put bank 2's install at 0x08050400
# (bug batch F's merge). Out of the address-taken set by name prefix (the
# generic instance's full name follows). Grep the code before adding one:
# its address must never be taken.
# ponytail: a name list is the ceiling: each link may collide anew. The
# real fix counts only words in relocated function-pointer tables or
# sections, not any data word that happens to match.
DIRECT_ONLY = ("chimera_board::bank2::boot_install::<",)

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


# A conditional branch, `bne.w` and kin: one to another function's start is a
# conditional tail call.
COND_B = re.compile(r"^b(?:eq|ne|cs|hs|cc|lo|mi|pl|vs|vc|hi|ls|ge|lt|gt|le)(?:\.w)?$")

DATA_ROW = re.compile(r"^ ([0-9a-f]+) ((?:[0-9a-f]{8} ?){1,4})")


class Image:
    """Functions as (frame bytes, calls, tail calls, calls through a
    pointer), their start addresses, and every word the image holds."""

    def __init__(self):
        self.funcs, self.starts, self.words = {}, {}, set()
        # Each function's end: past its last instruction.
        self.ends = {}


def parse(lines) -> Image:
    """`llvm-objdump -d` output, optionally followed by `-s` of the data
    sections: their words are where function pointers live."""
    img, cur, data, lo, table = Image(), None, False, {}, False
    dumped = []
    for line in lines:
        line = line.rstrip("\n")
        if line.startswith("Contents of section"):
            data = True
            continue
        if data:
            m = DATA_ROW.match(line)
            if m:
                at = int(m.group(1), 16)
                for k, g in enumerate(m.group(2).split()):
                    if len(g) == 8:
                        dumped.append((at + 4 * k, int.from_bytes(bytes.fromhex(g), "little")))
            continue
        h = HEADER.match(line)
        if h:
            cur = h.group(2)
            img.funcs[cur] = [0, set(), set(), 0]
            img.starts[cur] = int(h.group(1), 16)
            lo, table = {}, False
            continue
        i = INSN.match(line)
        if not i or cur is None:
            continue
        img.ends[cur] = int(line.split(":", 1)[0], 16) + 4
        op, args = i.group(1), i.group(2)
        f = img.funcs[cur]
        # A `tbb`/`tbh` table's offsets, up to the next instruction, are
        # data the disassembler shows as `.word`s: never a pointer.
        if op in ("tbb", "tbh"):
            table = True
        elif not op.startswith("."):
            table = False
        if op == ".word":
            if not table:
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
        elif op in ("b", "b.w") or COND_B.match(op):
            t = TARGET.search(args)
            if t and not OFFSET.search(t.group(1)) and t.group(1) != cur:
                f[2].add(t.group(1))
        elif op.startswith("blx") or (op == "bx" and args.strip() != "lr"):
            if re.match(r"(r\d+|ip|lr)\b", args.strip()):
                f[3] += 1
    # A dumped word inside a function is its instruction bytes (bank 2's
    # code shares `.bank2` with the door's table): its literals came in as
    # `.word`s above, the rest is never a pointer.
    spans = sorted((img.starts[f], img.ends.get(f, img.starts[f])) for f in img.starts)
    starts = [a for a, _ in spans]
    for at, w in dumped:
        k = bisect.bisect_right(starts, at) - 1
        if k < 0 or at >= spans[k][1]:
            img.words.add(w)
    img.funcs = {k: tuple(v) for k, v in img.funcs.items()}
    return img


def components(funcs):
    """Each function's strongly connected component (Tarjan, iterative) as a
    frozenset: the functions it can recurse through, itself alone if none."""
    index, low, stack, on, comp = {}, {}, [], set(), {}
    edges = {f: sorted(set(v[1]) | set(v[2])) for f, v in funcs.items()}
    for root in sorted(funcs):
        if root in index:
            continue
        work = [(root, 0)]
        while work:
            f, i = work.pop()
            if i == 0:
                index[f] = low[f] = len(index)
                stack.append(f)
                on.add(f)
            out = edges.get(f, ())
            if i < len(out):
                work.append((f, i + 1))
                c = out[i]
                if c not in index:
                    work.append((c, 0))
                elif c in on:
                    low[f] = min(low[f], index[c])
                continue
            if work:
                parent = work[-1][0]
                low[parent] = min(low[parent], low[f])
            if low[f] == index[f]:
                members = []
                while True:
                    m = stack.pop()
                    on.discard(m)
                    members.append(m)
                    if m == f:
                        break
                for m in members:
                    comp[m] = frozenset(members)
    return comp


def depth(funcs, root, charge=0, comp=None, doors=None, only=None):
    """(bytes, deepest chain, recursive calls cut, functions with pointer
    calls). A pointer call costs `charge`. A call back into the chain costs
    0, so a recursion's worst case is its longest simple path, every
    function in it live once. That depends on which of the cycle's
    functions are already on the chain, so a result is kept per function
    and those (the rest of the chain can't be reached again); calls go in
    name order and only a deeper one replaces the best: one answer, one
    chain, whatever the hash seed. A function in `doors` charges its
    pointer call its named targets' depth instead, where they are found;
    one in `only`'s callers charges the deeper of its target and `charge`."""
    doors = doors or {}
    also = {}
    for t, callers in (only or {}).items():
        for c in callers:
            also.setdefault(c, []).append(t)
    comp = components(funcs) if comp is None else comp
    memo, cut, indirect = {}, set(), set()

    def go(f, on):
        if f in on:
            cut.add(f)
            return 0, [f + " (recursion)"]
        key = (f, on & comp.get(f, frozenset()))
        if key in memo:
            return memo[key]
        frame, calls, tails, ind = funcs.get(f, (0, (), (), False))
        best = (0, [])
        known = [t for t in doors.get(f, ()) if t in funcs]
        if ind and not known:
            indirect.add(f)
            best = (charge, ["(a pointer call, %d B)" % charge])
        on = on | {f}
        for t in sorted((known or [t for t in also.get(f, ()) if t in funcs]) if ind else ()):
            d = go(t, on)
            if d[0] > best[0]:
                best = (d[0], ["(the door's) " + d[1][0]] + d[1][1:])
        for c in sorted(calls):
            d = go(c, on)
            if d[0] > best[0]:
                best = d
        own = (frame + best[0], [f] + best[1])
        for t in sorted(tails):
            d = go(t, on)
            if d[0] > own[0]:
                own = d
        memo[key] = own
        return own

    total, chain = go(root, frozenset())
    return total, chain, cut, indirect


def pointer_charge(img, out, comp=None):
    """What one pointer call may cost: the deepest function whose address
    the image holds (Thumb bit set), with its own pointer calls charged one
    level less, `POINTER_NEST` levels in all (the innermost charged 0).
    A fixed point doesn't exist: address-taken functions make pointer
    calls themselves (a `Store` body calling a sink), so the nesting is the
    stated assumption."""
    taken = sorted(
        f
        for f, a in img.starts.items()
        if (a | 1) in img.words
        and f not in ROOTS
        and f not in DOOR_ONLY
        and not f.startswith(DIRECT_ONLY)
    )
    comp = comp if comp is not None else components(img.funcs)
    charge, deepest = 0, "-"
    for _ in range(POINTER_NEST):
        charge, deepest = max(((depth(img.funcs, f, charge, comp, None, DOOR_ONLY)[0], f) for f in taken), default=(0, "-"))
    print(
        "pointers   %6d B  each, %d deep, over %d address-taken functions; deepest %s"
        % (charge, POINTER_NEST, len(taken), short(deepest)),
        file=out,
    )
    return charge


def small_reached(funcs, roots=FAST_ROOTS):
    """The functions `roots` reach by direct and tail calls whose names
    hold a small-profile crate's, sorted."""
    seen, todo = set(), list(roots)
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
    comp = components(img.funcs)
    charge = pointer_charge(img, out, comp)
    ok, total = True, 0
    doors = dict(DOOR_CALLS)
    classes = tuple(sorted(f for f in img.funcs if CLASS in f))
    for f in CLASS_CALLS:
        if f in img.funcs and img.funcs[f][3]:
            if not classes:
                print("stack-depth: %s calls through a pointer, but no class method is linked" % short(f), file=out)
                ok = False
            doors[f] = classes
    level = {r: g for g in ONE_LEVEL for r in g}
    deepest = {}
    for i, root in enumerate((THREAD,) + ISRS):
        if root not in img.funcs:
            if root not in OPTIONAL:
                print("stack-depth: no %s in the disassembly" % root, file=out)
                ok = False
            continue
        d, chain, cut, ind = depth(img.funcs, root, charge, comp, doors, DOOR_ONLY)
        d += EXC_FRAME if i else 0
        g = level.get(root)
        if g is None:
            total += d
        elif d > deepest.get(g, 0):
            total += d - deepest.get(g, 0)
            deepest[g] = d
        print("%-10s %6d B  %s" % (root, d, " > ".join(short(c, 60) for c in chain[:6]) + (" > …" if len(chain) > 6 else "")), file=out)
        if cut:
            print("           recursion cut at: %s" % ", ".join(sorted(short(c, 60) for c in cut)), file=out)
        if ind:
            print("           %d functions call through a pointer, each charged above" % len(ind), file=out)
    for root, entries in DOOR_CALLS.items():
        if root in img.funcs and img.funcs[root][3] > len(entries):
            print("stack-depth: %s has %d pointer calls, its door's %d only" % (root, img.funcs[root][3], len(entries)), file=out)
            ok = False
        # A renamed or dropped entry would charge the pointer call the
        # usual charge in silence: every named one must be linked.
        if root in img.funcs and img.funcs[root][3]:
            for t in entries:
                if t not in img.funcs:
                    print("stack-depth: %s's door entry %s is not in the disassembly" % (root, short(t)), file=out)
                    ok = False
    for t, callers in DOOR_ONLY.items():
        if t in img.funcs and not any(img.funcs.get(c, (0, (), (), 0))[3] for c in callers):
            print("stack-depth: no pointer call in %s enters %s" % (" or ".join(callers), t), file=out)
            ok = False
    for f in small_reached(img.funcs):
        print("stack-depth: an audio or MIDI interrupt reaches %s, built for size" % short(f), file=out)
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
