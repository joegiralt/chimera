"""Host tests for bank1_budget.py and its file (standard library only)."""

import importlib.util
import pathlib
import re
import unittest

TOOLS = pathlib.Path(__file__).resolve().parent
_spec = importlib.util.spec_from_file_location("bank1_budget", TOOLS / "bank1_budget.py")
bb = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(bb)


class Load(unittest.TestCase):
    def test_reads_sets_and_comments(self):
        b = bb.load("# head\n\ndefault 900000  # ADR 0198: frozen\nbench-algo,bank2 880000 # ADR 0201: x\n")
        self.assertEqual(b, {"default": 900000, "bench-algo,bank2": 880000})

    def test_a_line_without_an_adr_is_refused(self):
        with self.assertRaisesRegex(ValueError, "names no ADR"):
            bb.load("default 900000  # raised for NOISE\n")
        with self.assertRaisesRegex(ValueError, "names no ADR"):
            bb.load("default 900000\n")

    def test_malformed_and_twice(self):
        with self.assertRaises(ValueError):
            bb.load("default 9e5  # ADR 0198\n")
        with self.assertRaisesRegex(ValueError, "twice"):
            bb.load("orbit 1  # ADR 0198\norbit 2  # ADR 0198\n")


class Judge(unittest.TestCase):
    B = {"orbit": 900_000}

    def test_within_slack_passes(self):
        for n in (900_000 - bb.GIVE_BACK, 900_000, 900_000 + bb.SLACK):
            self.assertIsNone(bb.judge("orbit", n, self.B), n)

    def test_growth_past_slack_says_move_it_behind_a_door(self):
        why = bb.judge("orbit", 900_000 + bb.SLACK + 1, self.B)
        self.assertIn("65 B over", why)
        self.assertIn("behind a bank-2 door", why)
        self.assertIn("ADR 0198", why)

    def test_freed_room_must_be_given_back(self):
        why = bb.judge("orbit", 900_000 - bb.GIVE_BACK - 1, self.B)
        self.assertIn("lower its line", why)

    def test_an_unknown_set_fails(self):
        self.assertIn("no budget", bb.judge("bench-x", 1, self.B))

    def test_main_reads_the_bin(self):
        d = pathlib.Path(__file__).resolve().parent.parent / "target" / "bank1-test"
        d.mkdir(parents=True, exist_ok=True)
        (d / "b.txt").write_text("default 100  # ADR 0198\n")
        (d / "a.bin").write_bytes(b"\0" * 100)
        self.assertEqual(bb.main(["", "default", str(d / "a.bin"), str(d / "b.txt")]), 0)
        (d / "a.bin").write_bytes(b"\0" * 200)
        self.assertEqual(bb.main(["", "default", str(d / "a.bin"), str(d / "b.txt")]), 1)


class TheFile(unittest.TestCase):
    """The checked-in budgets cover exactly the bank-2 sets `just check` builds."""

    def test_every_line_names_an_adr_and_check_measures_each_set(self):
        budgets = bb.load(bb.BUDGET.read_text())
        just = (TOOLS.parent / "Justfile").read_text()
        bench = re.search(r'^bench_sets := "([^"]*)"', just, re.M).group(1).split()
        self.assertEqual(set(budgets), {"default", "orbit", "bank2-gate", *bench})
        check = just[just.index("\ncheck:") : just.index("\nstack-check:")]
        for s in ("default", "orbit", "bank2-gate"):
            self.assertIn(f"just check-bank1 {s}\n", check)
        self.assertIn('just check-bank1 "$f"', check)

    def test_bank1_size_is_the_bin_without_bank2(self):
        just = (TOOLS.parent / "Justfile").read_text()
        body = just[just.index("\ncheck-bank1 ") :].split("\n\n")[0]
        self.assertIn("-R .bank2 -O binary", body)
        self.assertIn("tools/bank1_budget.py", body)


if __name__ == "__main__":
    unittest.main()
