"""`just put-factory-sounds` against a stubbed console (CHIMERA_USB_CMD):
it refuses over user projects, installs on a card without PROJECTS, and
--replace-demos overrides the refusal. Standard library only."""

import os
import pathlib
import shutil
import subprocess
import sys
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parent.parent

# The stub console: `ls` answers $STUB_LS on stdout, or $STUB_ERR on
# stderr with exit 1 (as chimera-usb.py reports an ERR); `put` logs.
STUB = """
import os, sys
if sys.argv[1] == "ls":
    if os.environ.get("STUB_ERR"):
        print(os.environ["STUB_ERR"], file=sys.stderr)
        sys.exit(1)
    print(os.environ.get("STUB_LS", ""))
else:
    with open(os.environ["STUB_LOG"], "a") as f:
        f.write(" ".join(sys.argv[1:]) + "\\n")
"""


@unittest.skipIf(shutil.which("just") is None, "just is not installed")
class PutFactorySounds(unittest.TestCase):
    def setUp(self):
        self.tmp = pathlib.Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.tmp)
        for d, n in (("SOUNDS", "S9100001.A"), ("PROJECTS", "P0000001.A")):
            (self.tmp / "set" / "CHIMERA" / d).mkdir(parents=True)
            (self.tmp / "set" / "CHIMERA" / d / n).write_bytes(b"x")
        (self.tmp / "stub.py").write_text(STUB)
        self.log = self.tmp / "log"

    def run_recipe(self, *flags, ls="", err=""):
        env = dict(os.environ)
        env.pop("CHIMERA_USB", None)
        env.update(
            CHIMERA_USB_CMD="%s %s" % (sys.executable, self.tmp / "stub.py"),
            STUB_LS=ls,
            STUB_ERR=err,
            STUB_LOG=str(self.log),
        )
        return subprocess.run(
            ["just", "--justfile", str(ROOT / "Justfile"), "--working-directory", str(ROOT),
             "put-factory-sounds", str(self.tmp / "set"), *flags],
            env=env, capture_output=True, text=True,
        )

    def puts(self):
        return self.log.read_text().splitlines() if self.log.exists() else []

    def test_refuses_over_user_projects(self):
        r = self.run_recipe(ls="P0000001.A 1234\nS0000001.A 99")
        self.assertNotEqual(r.returncode, 0)
        self.assertIn("P0000001", r.stderr)
        self.assertEqual(self.puts(), [])

    def test_replace_demos_overrides(self):
        r = self.run_recipe("--replace-demos", ls="P0000001.A 1234")
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(len(self.puts()), 2)

    def test_a_card_without_projects_installs(self):
        r = self.run_recipe(err="ERR ls: FILE NOT FOUND")
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(len(self.puts()), 2)

    def test_another_ls_error_stops(self):
        r = self.run_recipe(err="ERR ls: NO CARD")
        self.assertNotEqual(r.returncode, 0)
        self.assertIn("NO CARD", r.stderr)
        self.assertEqual(self.puts(), [])


if __name__ == "__main__":
    unittest.main()
