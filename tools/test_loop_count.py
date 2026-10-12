"""Host tests for loop_count.py (standard library only)."""

import importlib.util
import pathlib
import unittest

_spec = importlib.util.spec_from_file_location(
    "loop_count", pathlib.Path(__file__).resolve().parent / "loop_count.py"
)
lc = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(lc)

# Nine lines: the header, 2 before the loop, 6 in it (one `vdiv`), 1 after.
DIS = """08000100 <chimera::probe_fn>:
 8000100:      \tpush\t{r7, lr}
 8000102:      \tmovs\tr2, #0x40
 8000104:      \tvldmia\tr0!, {s0}
 8000108:      \tvmul.f32\ts0, s0, s1
 800010c:      \tvdiv.f32\ts0, s0, s2
 8000110:      \tvstmia\tr1!, {s0}
 8000114:      \tsubs\tr2, #0x1
 8000116:      \tbne\t0x8000104 <chimera::probe_fn+0x4> @ imm = #-0x16
"""


class LoopCount(unittest.TestCase):
    def test_counts_a_known_loop(self):
        c = lc.count(DIS + " 8000118:      \tpop\t{r7, pc}\n", "probe_fn")
        self.assertEqual(c["loop_instr"], 6)
        self.assertEqual(c["vdiv"], 1)
        self.assertEqual(c["vsqrt"], 0)
        self.assertEqual(c["block_instr"], 3)
        # 6 × 1.606 + 12 = 21.6, rounded up to 25.
        self.assertEqual(lc.cycles(6, 1), 25)

    def test_two_loops_innermost_each(self):
        dis = DIS + (
            " 8000118:      \tvldmia\tr0!, {s0}\n"
            " 800011c:      \tvsqrt.f32\ts0, s0\n"
            " 8000120:      \tsubs\tr2, #0x1\n"
            " 8000122:      \tbne\t0x8000118 <chimera::probe_fn+0x18>\n"
            " 8000124:      \tpop\t{r7, pc}\n"
        )
        c = lc.count(dis, "probe_fn")
        self.assertEqual([l["loop_instr"] for l in c["loops"]], [6, 4])
        self.assertEqual(c["loops"][1]["vsqrt"], 1)
        self.assertEqual(c["block_instr"], 3)

    def test_a_loop_with_two_back_edges_is_one_loop(self):
        # The rotated shape LLVM gives `saturate`: a skippable tail, then a
        # second back-edge. Fast path 5 (the `bls` taken), slow path 7.
        dis = """08000100 <sat_probe>:
 8000100:      \tvldmia\tr0!, {s0}
 8000104:      \tsubs\tr2, #0x1
 8000106:      \tbeq\t0x8000120 <sat_probe+0x20>
 8000108:      \tvabs.f32\ts1, s0
 800010c:      \tbls.w\t0x8000100 <sat_probe>
 8000110:      \tvminnm.f32\ts0, s0, s2
 8000114:      \tb\t0x8000100 <sat_probe>
 8000120:      \tbx\tlr
 8000124: 00 c0 b7 46  \t.word\t0x46b7c000
"""
        c = lc.count(dis, "sat_probe")
        self.assertEqual(len(c["loops"]), 1)
        self.assertEqual(c["loop_instr"], 7)
        self.assertEqual((c["fast"], c["slow"]), (5, 7))
        self.assertEqual(c["block_instr"], 1)  # not the `.word`

    def test_a_divide_outside_the_loop_is_per_block(self):
        dis = DIS.replace("movs\tr2, #0x40", "vdiv.f32\ts1, s1, s2")
        c = lc.count(dis, "probe_fn")
        self.assertEqual(c["vdiv"], 1)
        self.assertEqual(c["block_div"], 1)

    def test_nested_loops_give_the_inner_one(self):
        dis = """08000100 <nest>:
 8000100:      \tmovs\tr3, #0x4
 8000102:      \tmovs\tr2, #0x10
 8000104:      \tvadd.f32\ts0, s0, s1
 8000108:      \tsubs\tr2, #0x1
 800010a:      \tbne\t0x8000104 <nest+0x4>
 800010c:      \tsubs\tr3, #0x1
 800010e:      \tbne\t0x8000102 <nest+0x2>
 8000110:      \tbx\tlr
"""
        c = lc.count(dis, "nest")
        self.assertEqual([l["head"] for l in c["loops"]], [0x8000104])
        self.assertEqual(c["loop_instr"], 3)

    def test_a_pc_write_ends_a_path(self):
        # `pop {pc}` leaves the loop: the only path round is the one past it.
        dis = """08000100 <ret>:
 8000100:      \tvadd.f32\ts0, s0, s1
 8000104:      \tcmp\tr2, #0x0
 8000106:      \tbne\t0x800010e <ret+0xe>
 8000108:      \tpop\t{r7, pc}
 800010a:      \tnop
 800010c:      \tnop
 800010e:      \tsubs\tr2, #0x1
 8000110:      \tb\t0x8000100 <ret>
"""
        c = lc.count(dis, "ret")
        self.assertEqual((c["fast"], c["slow"]), (5, 5))
        for op in ("bx\tlr", "ldr\tpc, [sp], #4", "tbb\t[pc, r2]"):
            c = lc.count(dis.replace("pop\t{r7, pc}", op), "ret")
            self.assertEqual((c["fast"], c["slow"]), (5, 5), op)

    def test_a_call_in_a_loop_fails(self):
        dis = DIS.replace("vmul.f32\ts0, s0, s1", "bl\t0x8000400 <libm::tanf>")
        with self.assertRaises(SystemExit):
            lc.count(dis, "probe_fn")

    def test_a_cold_call_is_off_the_hot_loop(self):
        # The envelope's stage change: out of line after the loop, back into
        # its middle. That span is dropped; its instructions are per block.
        dis = DIS.replace(
            " 8000116:      \tbne\t0x8000104 <chimera::probe_fn+0x4> @ imm = #-0x16\n",
            " 8000116:      \tbne\t0x8000104 <chimera::probe_fn+0x4> @ imm = #-0x16\n"
            " 8000118:      \tb\t0x8000124 <chimera::probe_fn+0x24>\n"
            " 800011a:      \tbl\t0x8000400 <<chimera_engine::dsp::algo::env::OpEnv>::stage_end>\n"
            " 800011e:      \tb\t0x8000108 <chimera::probe_fn+0x8>\n",
        ) + " 8000124:      \tpop\t{r7, pc}\n"
        c = lc.count(dis, "probe_fn")
        self.assertEqual([l["loop_instr"] for l in c["loops"]], [6])
        self.assertEqual(c["block_instr"], 3 + 3)

    def test_a_back_edge_with_no_way_round_is_not_a_loop(self):
        # Entered past its head from outside; from the head every path
        # leaves backwards: an outer loop's tail, not an inner loop.
        dis = DIS + (
            " 8000118:      \tldr\tr0, [sp]\n"
            " 800011a:      \tb\t0x8000100 <chimera::probe_fn>\n"
            " 800011c:      \tnop\n"
            " 8000120:      \tcmp\tr1, r2\n"
            " 8000122:      \tbhs\t0x8000118 <chimera::probe_fn+0x18>\n"
            " 8000124:      \tpop\t{r7, pc}\n"
        )
        c = lc.count(dis, "probe_fn")
        self.assertEqual([l["loop_instr"] for l in c["loops"]], [6])

    def test_an_unrolled_loop_has_its_stride(self):
        # Four samples a time round: a byte offset stepped by 16, compared
        # with 0x100.
        dis = """08000100 <unrolled>:
 8000100:      \tadds\tr3, r1, r2
 8000102:      \tvldr\ts0, [r3]
 8000106:      \tvmul.f32\ts0, s0, s1
 800010a:      \tvstr\ts0, [r3]
 800010e:      \tadds\tr2, #0x10
 8000110:      \tcmp.w\tr2, #0x100
 8000114:      \tbne\t0x8000100 <unrolled>
 8000116:      \tbx\tlr
"""
        c = lc.count(dis, "unrolled")
        self.assertEqual((c["loop_instr"], c["stride"]), (7, 4))
        # `add.w lr, lr, #0x40` against `cmp.w lr, #0x100`: 16 a time.
        wide = dis.replace("adds\tr2, #0x10", "add.w\tlr, lr, #0x40").replace(
            "cmp.w\tr2", "cmp.w\tlr")
        self.assertEqual(lc.count(wide, "unrolled")["stride"], 16)
        # A step that isn't whole `f32`s isn't a stride.
        odd = dis.replace("adds\tr2, #0x10", "adds\tr2, #0x6")
        self.assertIsNone(lc.count(odd, "unrolled")["stride"])
        # A sample index stepped by 4 to 0x40: the compare's immediate says
        # it counts samples, not bytes (4, not 4/4).
        index = dis.replace("adds\tr2, #0x10", "adds\tr2, #0x4").replace(
            "#0x100", "#0x40")
        self.assertEqual(lc.count(index, "unrolled")["stride"], 4)
        # Bytes by 0x10 to 0xf0, the last round peeled: still 4 a time.
        peeled = dis.replace("#0x100", "#0xf0")
        self.assertEqual(lc.count(peeled, "unrolled")["stride"], 4)
        # Compared with a register (an end pointer): unknown.
        self.assertIsNone(
            lc.count(dis.replace("cmp.w\tr2, #0x100", "cmp\tr2, r4"), "unrolled")["stride"])

    def test_a_count_down_loop_has_its_stride(self):
        # DIS counts samples down from 0x40 by 1: one a time round.
        self.assertEqual(lc.count(DIS, "probe_fn")["stride"], 1)
        # Bytes down from 0x100 by 0x10: four a time round.
        bytes_ = DIS.replace("movs\tr2, #0x40", "mov.w\tr2, #0x100").replace(
            "subs\tr2, #0x1", "subs\tr2, #0x10")
        self.assertEqual(lc.count(bytes_, "probe_fn")["stride"], 4)

    def test_the_counter_is_the_last_subs(self):
        # `TapShape::apply`'s shape: a `subs` on a float's bits mid-loop, the
        # counter's `subs.w` by 2 from 0x40 last, before the back-edge.
        dis = DIS.replace("movs\tr2, #0x40", "mov.w\tr12, #0x40").replace(
            "subs\tr2, #0x1", "subs\tr0, #0x40\n 8000115:      \tsubs.w\tr12, r12, #0x2")
        self.assertEqual(lc.count(dis, "probe_fn")["stride"], 2)

    def test_an_unknown_stride_says_so(self):
        # A register-to-register compare: no immediate to read.
        dis = DIS.replace("subs\tr2, #0x1", "cmp\tr2, r3")
        self.assertIsNone(lc.count(dis, "probe_fn")["stride"])
        # A count-down whose start isn't an immediate.
        dis = DIS.replace("movs\tr2, #0x40", "ldr\tr2, [r0]")
        self.assertIsNone(lc.count(dis, "probe_fn")["stride"])

    def test_cycles_land_on_fives(self):
        self.assertEqual(lc.cycles(0, 0), 0)
        # Per-block work over 64: 41 + 96/64 = 42.5 → 68.3 + 24 = 92.3.
        self.assertEqual(lc.cycles(lc.Fraction(41) + lc.Fraction(96, 64), 2), 95)
        self.assertEqual(lc.cycles(50, 2), 105)  # 80.3 + 24 = 104.3


if __name__ == "__main__":
    unittest.main()
