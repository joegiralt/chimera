"""Host tests for bank2_budget.py and its file (standard library only)."""

import importlib.util
import pathlib
import unittest

TOOLS = pathlib.Path(__file__).resolve().parent
_spec = importlib.util.spec_from_file_location("bank2_budget", TOOLS / "bank2_budget.py")
b2 = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(b2)

# rust-nm's lines for the three ends: S0-S3 holds 0x70000 B, S4-S5 0x100,
# S6-S7 nothing.
NM = """08170000 T __bank2_lo_end
08100000 T __bank2_base
08180500 T __bank2_s4_end
081c0400 T __bank2_s6_end
08200000 T __bank2_end
"""
USED = 0x70000 + 0x100
FREE = b2.ROOM - USED


class Lines(unittest.TestCase):
    def test_the_four_lines(self):
        rc, out = b2.run("orbit", NM, f"orbit {USED}  # ADR 0272: test\n")
        self.assertEqual(rc, 0, out)
        lines = out.splitlines()
        self.assertEqual(len(lines), 4, out)
        self.assertRegex(lines[0], r"^bank 2 S0-S3 +free = 65536 B$")
        self.assertRegex(lines[1], r"^bank 2 S4-S5 +free = 260832 B +\(1 KB slot, 32 B marker\)$")
        self.assertRegex(lines[2], r"^bank 2 S6-S7 +free = 261088 B +\(1 KB slot, 32 B marker\)$")
        self.assertRegex(lines[3], rf"^bank 2 total +free = {FREE} B, used {USED} B, budget {USED} B$")

    def test_a_missing_symbol_fails(self):
        rc, out = b2.run("orbit", NM.replace("__bank2_s6_end", "x"), f"orbit {USED}  # ADR 0272\n")
        self.assertEqual(rc, 1)
        self.assertIn("__bank2_s6_end", out)


class Floor(unittest.TestCase):
    def test_under_8_kb_free_fails(self):
        nm = NM.replace("08170000 T __bank2_lo_end", "08180000 T __bank2_lo_end").replace(
            "08180500 T __bank2_s4_end", f"{0x081BFFE0 - 4096:08x} T __bank2_s4_end"
        ).replace("081c0400 T __bank2_s6_end", f"{0x081FFFE0 - 4095:08x} T __bank2_s6_end")
        used = b2.ROOM - 8191
        rc, out = b2.run("orbit", nm, f"orbit {used}  # ADR 0272\n")
        self.assertEqual(rc, 1, out)
        self.assertIn("ADR 0251", out)


class Budget(unittest.TestCase):
    def test_over_the_line_by_65_fails(self):
        rc, _ = b2.run("orbit", NM, f"orbit {USED - 64}  # ADR 0272\n")
        self.assertEqual(rc, 0)
        rc, out = b2.run("orbit", NM, f"orbit {USED - 65}  # ADR 0272\n")
        self.assertEqual(rc, 1)
        self.assertIn("65 B over", out)

    def test_a_kb_freed_must_lower_the_line(self):
        rc, _ = b2.run("orbit", NM, f"orbit {USED + 1024}  # ADR 0272\n")
        self.assertEqual(rc, 0)
        rc, out = b2.run("orbit", NM, f"orbit {USED + 1025}  # ADR 0272\n")
        self.assertEqual(rc, 1)
        self.assertIn("lower its line", out)

    def test_an_unknown_set_fails(self):
        rc, out = b2.run("bench-x", NM, f"orbit {USED}  # ADR 0272\n")
        self.assertEqual(rc, 1)
        self.assertIn("no budget", out)


class TheFile(unittest.TestCase):
    def test_check_measures_each_set_in_the_file(self):
        budgets = b2.load(b2.BUDGET.read_text())
        self.assertEqual(set(budgets), {"default", "orbit", "bank2-gate"})
        self.assertIn("ADR 0272", b2.BUDGET.read_text())


if __name__ == "__main__":
    unittest.main()
