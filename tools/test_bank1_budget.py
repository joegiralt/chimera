"""Host tests for bank1_budget.py and its file (standard library only)."""

import importlib.util
import os
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

    def test_bank1_lower_only_ever_lowers(self):
        d = pathlib.Path(__file__).resolve().parent.parent / "target" / "bank1-test"
        d.mkdir(parents=True, exist_ok=True)
        budget = "bench-algo,bank2 5000  # ADR 0198: a\nbench-algo,master-tape,bank2 5000  # ADR 0198: b\n"
        (d / "b.txt").write_text(budget)
        os.environ["BANK1_LOWER"] = "1"
        try:
            (d / "a.bin").write_bytes(b"\0" * 6000)
            self.assertEqual(bb.main(["", "bench-algo,bank2", str(d / "a.bin"), str(d / "b.txt")]), 1)
            self.assertEqual((d / "b.txt").read_text(), budget)
            (d / "a.bin").write_bytes(b"\0" * 3000)
            self.assertEqual(bb.main(["", "bench-algo,bank2", str(d / "a.bin"), str(d / "b.txt")]), 0)
        finally:
            del os.environ["BANK1_LOWER"]
        self.assertEqual(
            bb.load((d / "b.txt").read_text()),
            {"bench-algo,bank2": 3000, "bench-algo,master-tape,bank2": 5000},
        )
        self.assertIn("# ADR 0198: a", (d / "b.txt").read_text())


class TheFile(unittest.TestCase):
    """The checked-in budgets cover exactly the bank-2 sets `just check` builds."""

    # Firmware sets with no bank 2: code moved behind a door lands in them.
    NO_BANK2 = {"no-default", "sd-probe"}

    def test_every_line_names_an_adr_and_check_measures_each_set(self):
        budgets = bb.load(bb.BUDGET.read_text())
        just = (TOOLS.parent / "Justfile").read_text()
        bench = re.search(r'^bench_sets := "([^"]*)"', just, re.M).group(1).split()
        # `check`'s header may carry parameters (`$CHIMERA_STAMP_CLEAN`).
        start = re.search(r"^check(?: [^\n]*)?:$", just, re.M).start()
        check = just[start : just.index("\nstack-check:")]
        # Walk check's firmware builds: each bank-2 build is measured by the
        # next `just check-bank1`, under its own name, before another set is
        # built. A new build line without its guard fails here.
        measured, built, done = set(), None, True
        for line in check.splitlines():
            if "cargo build -p chimera-stm32" in line:
                f = re.search(r'--features "?([^"\s]+)"?', line)
                s = f.group(1) if f else ("no-default" if "--no-default-features" in line else "default")
                if s == "$f":
                    self.assertIn('just check-bank1 "$f"', line)
                    measured.update(bench)
                elif s != built:
                    self.assertTrue(done, f"{built} is built but never held to its budget")
                    built, done = s, s in self.NO_BANK2
                continue
            m = re.search(r"just check-bank1 (\S+)", line)
            if m:
                self.assertEqual(m.group(1), built, "check-bank1 measures a set other than the one just built")
                measured.add(built)
                done = True
        self.assertTrue(done, f"{built} is built but never held to its budget")
        self.assertEqual(set(budgets), measured)

    def test_bank1_size_is_the_bin_without_bank2(self):
        just = (TOOLS.parent / "Justfile").read_text()
        body = just[just.index("\ncheck-bank1 ") :].split("\n\n")[0]
        self.assertIn("-R .bank2 -O binary", body)
        self.assertIn("tools/bank1_budget.py", body)


if __name__ == "__main__":
    unittest.main()
