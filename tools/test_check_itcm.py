"""Host tests for check_itcm.py and tools/itcm-residents.txt (standard library only)."""

import importlib.util
import pathlib
import unittest

TOOLS = pathlib.Path(__file__).resolve().parent
_spec = importlib.util.spec_from_file_location("check_itcm", TOOLS / "check_itcm.py")
ci = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(ci)

RESIDENTS = ci.load((TOOLS / "itcm-residents.txt").read_text())

# `rust-nm -C` of a synth build: ITCM's bounds, its residents, and an
# absolute symbol (a linker constant) whose value falls in the range.
SYNTH = """\
00000400 B __sitcm
00009e58 B __eitcm
00000400 b chimera_board::shared::ED
00000978 b chimera_board::shared::KITS
0000203c b chimera_board::shared::AUDIO
00007a7c b chimera_engine::dsp::drum::view::KIT_VIEW
00008000 A AXI_MARGIN
08031000 T chimera_board::shared::take_audio
24000000 B chimera_board::shared::POOL_ARENA
         U some_undefined
"""

# sd-probe's: no synth, so no `take_audio`, and `.itcm` empty.
PROBE = """\
00000400 B __sitcm
00000400 B __eitcm
00008000 A AXI_MARGIN
24000000 B chimera_board::display::FRAMEBUFFER
"""


class Load(unittest.TestCase):
    def test_the_file_lists_the_four(self):
        self.assertEqual(
            sorted(RESIDENTS),
            sorted(
                [
                    "chimera_board::shared::AUDIO",
                    "chimera_board::shared::ED",
                    "chimera_board::shared::KITS",
                    "chimera_engine::dsp::drum::view::KIT_VIEW",
                ]
            ),
        )

    def test_audio_is_required_where_the_synth_is_linked(self):
        self.assertEqual(
            RESIDENTS["chimera_board::shared::AUDIO"].needed_with,
            "chimera_board::shared::take_audio",
        )

    def test_malformed_and_twice(self):
        with self.assertRaises(ValueError):
            ci.load("just_a_symbol\n")
        with self.assertRaisesRegex(ValueError, "twice"):
            ci.load("a::B | T\na::B | T\n")


class Judge(unittest.TestCase):
    def test_a_synth_build_passes(self):
        self.assertEqual(ci.judge(SYNTH, RESIDENTS), [])

    def test_sd_probe_passes_with_itcm_empty(self):
        self.assertEqual(ci.judge(PROBE, RESIDENTS), [])

    def test_an_unlisted_symbol_fails(self):
        bad = ci.judge(SYNTH + "00009000 b chimera_board::sd::DMA_BUF\n", RESIDENTS)
        self.assertEqual(len(bad), 1)
        self.assertIn("DMA_BUF", bad[0])

    def test_weak_and_other_kinds_in_range_fail(self):
        for kind in "VWvwRrTtCSG?":
            bad = ci.judge(SYNTH + f"00009000 {kind} sneaky\n", RESIDENTS)
            self.assertEqual(len(bad), 1, kind)
            self.assertIn("sneaky", bad[0])

    def test_audio_missing_where_the_synth_is_linked_fails(self):
        moved = SYNTH.replace("0000203c b chimera_board::shared::AUDIO", "24048388 b chimera_board::shared::AUDIO")
        bad = ci.judge(moved, RESIDENTS)
        self.assertEqual(len(bad), 1)
        self.assertIn("AUDIO", bad[0])

    def test_the_range_comes_from_the_elf(self):
        # Past `__eitcm` is not ITCM's state: not judged.
        self.assertEqual(ci.judge(SYNTH + "0000a000 b beyond\n", RESIDENTS), [])
        # A wider `.itcm` takes it in, and it fails.
        wide = SYNTH.replace("00009e58 B __eitcm", "0000b000 B __eitcm")
        self.assertEqual(len(ci.judge(wide + "0000a000 b beyond\n", RESIDENTS)), 1)

    def test_no_bounds_fails(self):
        bad = ci.judge(SYNTH.replace("00000400 B __sitcm\n", ""), RESIDENTS)
        self.assertTrue(any("__sitcm" in b for b in bad))


if __name__ == "__main__":
    unittest.main()
