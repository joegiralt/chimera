"""Host tests for stack_depth.py (standard library only)."""

import importlib.util
import io
import os
import pathlib
import subprocess
import sys
import unittest

TOOL = pathlib.Path(__file__).resolve().parent / "stack_depth.py"
_spec = importlib.util.spec_from_file_location("stack_depth", TOOL)
sd = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(sd)

DIS = """
08000000 <main>:
 8000000:      \tpush\t{r4, r5, r6, r7, lr}
 8000002:      \tsub.w\tsp, sp, #0x100
 8000006:      \tbl\t0x8000100 <leaf>        @ imm = #0xf8
 800000a:      \tbl\t0x8000200 <deep>        @ imm = #0x1f4
 800000e:      \tbl\t0x8000800 <<chimera_core::dsp::voice::Voice>::render> @ imm = #0x7f2
 8000012:      \tb\t0x8000006 <main+0x6>    @ imm = #-0x10

08000100 <leaf>:
 8000100:      \tpush\t{r7, lr}
 8000102:      \tpop\t{r7, pc}

08000200 <deep>:
 8000200:      \tpush.w\t{r4, r5, r6, r7, r8, r9, r10, r11, lr}
 8000204:      \tvpush\t{d8, d9, d10, d11, d12, d13, d14, d15}
 8000208:      \tsubw\tsp, sp, #0x800
 800020c:      \tbl\t0x8000200 <deep>        @ imm = #-0xc
 8000210:      \tb.w\t0x8000300 <tail>        @ imm = #0xec

08000300 <tail>:
 8000300:      \tsub\tsp, #0x2000

08000400 <DMA1_STR0>:
 8000400:      \tpush\t{r7, lr}
 8000402:      \tb.w\t0x8000100 <leaf>        @ imm = #-0x302

08000500 <USART1>:
 8000500:      \tpush\t{r7, lr}

08000600 <SysTick>:
 8000600:      \tpush\t{r7, lr}

08000700 <HardFault>:
 8000700:      \tpush\t{r7, lr}

08000780 <DMA2_STR0>:
 8000780:      \tpush\t{r7, lr}

08000800 <<chimera_core::dsp::voice::Voice>::render>:
 8000800:      \tpush\t{r4, lr}
 8000802:      \tsub\tsp, #0x40
 8000804:      \tblx\tr3
 8000806:      \tmovw\tr0, #0x901
 800080a:      \tmovt\tr0, #0x800
 800080e:      \tldr\tr1, [pc, #0x0]
 8000810: 01 0a 00 08  \t.word\t0x08000a01

08000900 <by_movw>:
 8000900:      \tsub\tsp, #0x30

08000a00 <by_word>:
 8000a00:      \tsub\tsp, #0x10

08000b00 <by_data>:
 8000b00:      \tsub\tsp, #0x500
 8000b02:      \tblx\tr2

Contents of section .rodata:
 8000c00 010b0008 00000000                    ........
"""


class StackDepth(unittest.TestCase):
    def setUp(self):
        self.img = sd.parse(io.StringIO(DIS))
        self.funcs = self.img.funcs

    def test_frames_count_pushes_vpushes_and_subs(self):
        self.assertEqual(self.funcs["main"][0], 20 + 0x100)
        self.assertEqual(self.funcs["deep"][0], 36 + 64 + 0x800)
        self.assertEqual(self.funcs["tail"][0], 0x2000)

    def test_a_nested_demangled_name_is_one_call(self):
        self.assertIn("<chimera_core::dsp::voice::Voice>::render", self.funcs["main"][1])
        self.assertTrue(self.funcs["<chimera_core::dsp::voice::Voice>::render"][3])
        self.assertNotIn("main+0x6", self.funcs["main"][2], "a branch inside main")

    def test_pointer_targets_are_the_address_taken(self):
        taken = {f for f, a in self.img.starts.items() if a | 1 in self.img.words}
        self.assertEqual(taken, {"by_movw", "by_word", "by_data"})

    def test_a_pointer_call_costs_the_deepest_target_nested(self):
        out = io.StringIO()
        charge = sd.pointer_charge(self.img, out)
        # by_data (0x500) calls a pointer itself: each level adds 0x500.
        self.assertEqual(charge, sd.POINTER_NEST * 0x500)
        voice = sd.depth(self.funcs, "<chimera_core::dsp::voice::Voice>::render", charge)[0]
        self.assertEqual(voice, 8 + 0x40 + charge)

    def test_nested_frames_sum_and_a_tail_call_replaces(self):
        total, chain, cut, ind = sd.depth(self.funcs, "main")
        # deep's own frame and its tail call's are never live together:
        # the tail's 8 KB wins, on top of main's.
        self.assertEqual(total, 20 + 0x100 + 0x2000)
        self.assertEqual(chain[:2], ["main", "tail"])
        self.assertEqual(cut, {"deep"}, "deep calls itself")
        self.assertEqual(sd.depth(self.funcs, "DMA1_STR0")[0], 8, "a tail call to leaf")

    def test_the_roots_sum_with_exception_frames(self):
        out = io.StringIO()
        self.assertTrue(sd.check(self.img, 64 * 1024, out))
        want = 20 + 0x100 + 0x2000 + 5 * (8 + sd.EXC_FRAME)
        self.assertIn("nested     %6d B" % want, out.getvalue())
        self.assertFalse(sd.check(self.img, want + sd.MARGIN - 1, io.StringIO()))

    def test_dma2_str0_counts(self):
        # The card interrupt nests like the others: its frame and its
        # exception frame are in the sum, and a build without it fails.
        out = io.StringIO()
        self.assertTrue(sd.check(self.img, 64 * 1024, out))
        self.assertIn("DMA2_STR0  %6d B" % (8 + sd.EXC_FRAME), out.getvalue())
        del self.img.funcs["DMA2_STR0"]
        out = io.StringIO()
        self.assertFalse(sd.check(self.img, 1 << 20, out))
        self.assertIn("no DMA2_STR0", out.getvalue())

    def test_a_door_call_costs_its_known_target(self):
        # DMA2_STR0's one pointer call is the board's door entry: charged
        # its target's depth, not every address-taken function's.
        frame, calls, tails, _ = self.funcs["DMA2_STR0"]
        self.funcs["DMA2_STR0"] = (frame, calls, tails, True)
        self.funcs["<chimera_board::card_dma::IrqState>::step"] = (0x60, set(), set(), False)
        doors = {"DMA2_STR0": ("<chimera_board::card_dma::IrqState>::step",)}
        self.assertEqual(sd.depth(self.funcs, "DMA2_STR0", 0x9999, None, doors)[0], 8 + 0x60)
        self.assertEqual(sd.depth(self.funcs, "DMA2_STR0", 0x9999)[0], 8 + 0x9999)
        del self.funcs["<chimera_board::card_dma::IrqState>::step"]
        self.assertEqual(sd.depth(self.funcs, "DMA2_STR0", 0x9999, None, doors)[0], 8 + 0x9999)

    def test_a_missing_root_fails_but_midi_may_be_absent(self):
        del self.img.funcs["USART1"]
        self.assertTrue(sd.check(self.img, 1 << 20, io.StringIO()), "no MIDI DIN")
        del self.img.funcs["DMA1_STR0"]
        out = io.StringIO()
        self.assertFalse(sd.check(self.img, 1 << 20, out))
        self.assertIn("no DMA1_STR0", out.getvalue())

    def test_the_audio_interrupt_reaches_no_small_profile_code(self):
        self.assertEqual(sd.small_reached(self.funcs), [])
        funcs = dict(self.funcs)
        funcs["leaf"] = (8, frozenset({"chimera_board::probe::stack_used"}), frozenset(), False)
        funcs["chimera_board::probe::stack_used"] = (0, frozenset(), frozenset(), False)
        self.assertEqual(sd.small_reached(funcs), ["chimera_board::probe::stack_used"])
        self.img.funcs = funcs
        out = io.StringIO()
        self.assertFalse(sd.check(self.img, 1 << 20, out))
        self.assertIn("reaches chimera_board::probe::stack_used", out.getvalue())

    def test_a_conditional_branch_to_a_start_is_a_tail_call(self):
        img = sd.parse(io.StringIO("08000000 <f>:\n 8000000:      \tbne.w\t0x8000100 <g>        @ imm = #0xfc\n 8000004:      \tbeq\t0x8000002 <f+0x2>\n\n08000100 <g>:\n 8000100:      \tbx\tlr\n"))
        self.assertEqual(img.funcs["f"][2], {"g"})


    def test_a_tbb_table_is_no_pointer(self):
        # `tbb`'s byte offsets disassemble as `.word`s: one that reads as a
        # function's address is not a pointer to it (bench-routing, ADR 0142).
        img = sd.parse(io.StringIO(
            "08000000 <f>:\n"
            " 8000000:      \ttbb\t[pc, r5]\n"
            " 8000004: 01 01 00 08  \t.word\t0x08000101\n"
            " 8000008:      \tmovs\tr2, #0xa\n"
            " 800000a:      \tldr\tr1, [pc, #0x0]\n"
            " 800000c: 01 02 00 08  \t.word\t0x08000201\n"
            "\n08000100 <g>:\n 8000100:      \tbx\tlr\n"
            "\n08000200 <h>:\n 8000200:      \tbx\tlr\n"
        ))
        self.assertNotIn(0x08000101, img.words)
        self.assertIn(0x08000201, img.words, "a literal after the table still counts")

    def cycle(self, light, heavy):
        """main calls both of a cycle: `light` (16 B) and `heavy` (256 B),
        heavy also a 4 KB leaf. Whichever main enters first, the worst
        is light > heavy > leaf (#582)."""
        f = lambda frame, *calls: (frame, frozenset(calls), frozenset(), False)
        return {
            "main": f(8, light, heavy),
            light: f(16, heavy),
            heavy: f(256, light, "leaf"),
            "leaf": f(0x1000),
        }

    def test_a_cycle_costs_its_longest_simple_path_in_any_order(self):
        want = 8 + 16 + 256 + 0x1000
        for light, heavy in (("a", "b"), ("b", "a")):
            total, chain, cut, _ = sd.depth(self.cycle(light, heavy), "main")
            self.assertEqual(total, want, "light %s" % light)
            self.assertEqual(chain, ["main", light, heavy, "leaf"])
            self.assertEqual(cut, {"a", "b"}, "both entries' back edges")

    def test_every_hash_seed_gives_one_answer(self):
        # The cycles below and DIS's: a set's order changes with the seed.
        cycle = (
            "08000180 <ping>:\n 8000180:      \tsub\tsp, #0x10\n 8000182:      \tbl\t0x80001c0 <pong>\n\n"
            "080001c0 <pong>:\n 80001c0:      \tsub\tsp, #0x200\n 80001c2:      \tbl\t0x8000180 <ping>\n"
            " 80001c4:      \tbl\t0x8000300 <tail>\n\n"
        )
        calls = " 8000100:      \tbl\t0x8000180 <ping>\n 8000100:      \tbl\t0x80001c0 <pong>\n"
        dis = DIS.replace("08000100 <leaf>:\n", "08000100 <leaf>:\n" + calls).replace("Contents of", cycle + "Contents of")
        outs = set()
        for seed in range(6):
            env = dict(os.environ, PYTHONHASHSEED=str(seed))
            run = subprocess.run([sys.executable, str(TOOL), "65536"], input=dis, env=env, capture_output=True, text=True)
            outs.add((run.returncode, run.stdout))
        self.assertEqual(len(outs), 1, "\n----\n".join(o for _, o in outs))
        self.assertIn("%-10s %6d B  main > leaf > ping > pong > tail" % ("main", 20 + 0x100 + 8 + 0x10 + 0x200 + 0x2000), outs.pop()[1])


if __name__ == "__main__":
    unittest.main()
