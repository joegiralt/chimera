"""Host tests for stack_depth.py (standard library only)."""

import importlib.util
import io
import pathlib
import unittest

_spec = importlib.util.spec_from_file_location(
    "stack_depth", pathlib.Path(__file__).resolve().parent / "stack_depth.py"
)
sd = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(sd)

DIS = """
08000000 <main>:
 8000000:      	push	{r4, r5, r6, r7, lr}
 8000002:      	sub.w	sp, sp, #0x100
 8000006:      	bl	0x8000100 <leaf>        @ imm = #0xf8
 800000a:      	bl	0x8000200 <deep>        @ imm = #0x1f4
 800000e:      	blx	r3
 8000010:      	b	0x8000006 <main+0x6>    @ imm = #-0xe

08000100 <leaf>:
 8000100:      	push	{r7, lr}
 8000102:      	pop	{r7, pc}

08000200 <deep>:
 8000200:      	push.w	{r4, r5, r6, r7, r8, r9, r10, r11, lr}
 8000204:      	vpush	{d8, d9, d10, d11, d12, d13, d14, d15}
 8000208:      	subw	sp, sp, #0x800
 800020c:      	bl	0x8000200 <deep>        @ imm = #-0xc
 8000210:      	b.w	0x8000300 <tail>        @ imm = #0xec

08000300 <tail>:
 8000300:      	sub	sp, #0x2000

08000400 <DMA1_STR0>:
 8000400:      	push	{r7, lr}
 8000402:      	b.w	0x8000100 <leaf>        @ imm = #-0x302

08000500 <USART1>:
 8000500:      	push	{r7, lr}

08000600 <SysTick>:
 8000600:      	push	{r7, lr}

08000700 <HardFault>:
 8000700:      	push	{r7, lr}
"""


class StackDepth(unittest.TestCase):
    def setUp(self):
        self.funcs = sd.parse(io.StringIO(DIS))

    def test_frames_count_pushes_vpushes_and_subs(self):
        self.assertEqual(self.funcs["main"][0], 20 + 0x100)
        self.assertEqual(self.funcs["deep"][0], 36 + 64 + 0x800)
        self.assertEqual(self.funcs["tail"][0], 0x2000)
        self.assertTrue(self.funcs["main"][3], "blx r3 is an indirect call")

    def test_nested_frames_sum_and_a_tail_call_replaces(self):
        total, chain, cut, ind = sd.depth(self.funcs, "main")
        # deep's own frame and its tail call's are never live together:
        # the tail's 8 KB wins, on top of main's.
        self.assertEqual(total, 20 + 0x100 + 0x2000)
        self.assertEqual(chain[:2], ["main", "tail"])
        self.assertEqual(cut, {"deep"}, "deep calls itself")
        self.assertEqual(ind, {"main"})
        self.assertEqual(sd.depth(self.funcs, "DMA1_STR0")[0], 8, "a tail call to leaf")

    def test_the_roots_sum_with_exception_frames(self):
        out = io.StringIO()
        self.assertTrue(sd.check(self.funcs, 64 * 1024, out))
        want = 20 + 0x100 + 0x2000 + 4 * (8 + sd.EXC_FRAME)
        self.assertIn("nested     %6d B" % want, out.getvalue())
        self.assertFalse(sd.check(self.funcs, want + sd.MARGIN - 1, io.StringIO()))

    def test_a_missing_root_fails_but_midi_may_be_absent(self):
        funcs = dict(self.funcs)
        del funcs["USART1"]
        self.assertTrue(sd.check(funcs, 1 << 20, io.StringIO()), "no MIDI DIN")
        del funcs["DMA1_STR0"]
        out = io.StringIO()
        self.assertFalse(sd.check(funcs, 1 << 20, out))
        self.assertIn("no DMA1_STR0", out.getvalue())


if __name__ == "__main__":
    unittest.main()
