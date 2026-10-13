"""The Justfile never makes a .bin of both banks (standard library only)."""

import pathlib
import re
import unittest

ROOT = pathlib.Path(__file__).resolve().parent.parent


class Bank2Split(unittest.TestCase):
    def test_every_binary_objcopy_drops_bank2(self):
        lines = (ROOT / "Justfile").read_text().splitlines()
        bins = [l for l in lines if re.search(r"rust-objcopy .*-O binary", l)]
        self.assertTrue(bins)
        for l in bins:
            self.assertTrue("-R .bank2" in l or "-j .bank2" in l, l)

    def test_bank2_x_asserts(self):
        src = (ROOT / "chimera-stm32/build.rs").read_text()
        self.assertIn("0x08180000", src)
        self.assertIn("INSERT BEFORE .text", src)


if __name__ == "__main__":
    unittest.main()
