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
08031100 t chimera_board::shared::take_ui
080b9358 t <chimera_engine::dsp::drum::view::KitWriter>::publish_all
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

    def test_audio_is_three_snapshots_owned_by_take_audio(self):
        self.assertEqual(
            RESIDENTS["chimera_board::shared::AUDIO"],
            ci.Resident("AudioShared", 3, "chimera_board::shared::take_audio"),
        )

    def test_malformed_and_twice(self):
        for bad in ["just_a_symbol\n", "a::B | T\n", "a::B | T | x3\n", "a::B | T | 0\n", "a::B | | 1\n"]:
            with self.assertRaises(ValueError, msg=bad):
                ci.load(bad)
        with self.assertRaisesRegex(ValueError, "twice"):
            ci.load("a::B | T | 1\na::B | T | 1\n")


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

    def test_any_resident_moved_out_fails(self):
        for name in ["ED", "KITS", "AUDIO"]:
            kept = [l for l in SYNTH.splitlines(keepends=True) if not l.endswith(f"b chimera_board::shared::{name}\n")]
            moved = "".join(kept).replace(
                "24000000 B chimera_board::shared::POOL_ARENA",
                f"24000000 B chimera_board::shared::POOL_ARENA\n24001000 b chimera_board::shared::{name}",
            )
            bad = ci.judge(moved, RESIDENTS)
            self.assertTrue(any(name in b for b in bad), (name, bad))
        moved = SYNTH.replace("00007a7c b chimera_engine", "24007a7c b chimera_engine")
        self.assertTrue(any("KIT_VIEW" in b for b in ci.judge(moved, RESIDENTS)))

    def test_a_resident_gone_where_its_owner_is_linked_fails(self):
        gone = SYNTH.replace("00000978 b chimera_board::shared::KITS\n", "")
        bad = ci.judge(gone, RESIDENTS)
        self.assertEqual(len(bad), 1)
        self.assertIn("take_ui", bad[0])

    def test_a_malformed_nm_line_fails(self):
        for junk in ["zzzz b broken\n", "00009000 bB two_letter_kind\n", "lonely\n", "         T no_address\n"]:
            bad = ci.judge(SYNTH + junk, RESIDENTS)
            self.assertEqual(len(bad), 1, junk)
            self.assertIn("unreadable", bad[0])

    def test_no_bounds_fails(self):
        bad = ci.judge(SYNTH.replace("00000400 B __sitcm\n", ""), RESIDENTS)
        self.assertTrue(any("__sitcm" in b for b in bad))


if __name__ == "__main__":
    unittest.main()
