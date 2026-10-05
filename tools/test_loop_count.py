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

    def test_cycles_land_on_fives(self):
        self.assertEqual(lc.cycles(0, 0), 0)
        # Per-block work over 64: 41 + 96/64 = 42.5 → 68.3 + 24 = 92.3.
        self.assertEqual(lc.cycles(lc.Fraction(41) + lc.Fraction(96, 64), 2), 95)
        self.assertEqual(lc.cycles(50, 2), 105)  # 80.3 + 24 = 104.3


if __name__ == "__main__":
    unittest.main()
