"""The Justfile never makes a .bin of both banks (standard library only)."""

import pathlib
import re
import unittest

ROOT = pathlib.Path(__file__).resolve().parent.parent


def recipes():
    """Each recipe's name and body, by the Justfile's layout: a header at
    column 0, its body indented."""
    out, name = {}, None
    for l in (ROOT / "Justfile").read_text().splitlines():
        m = re.match(r"([a-z][\w-]*)( [^:=]*)?:( |$)", l)
        if m:
            name = m.group(1)
            out[name] = []
        elif l.startswith((" ", "\t")) and name:
            out[name].append(l)
        elif l.strip() and not l.startswith("#"):
            name = None
    return out


class Bank2Gate(unittest.TestCase):
    def test_release_bin_runs_the_guard(self):
        body = recipes()["release-bin"]
        self.assertTrue(any("just check-no-gate " in l for l in body), body)

    def test_the_guard_greps_both(self):
        body = "\n".join(recipes()["check-no-gate"])
        self.assertIn("BANK2-GATE-BUILD", body)
        self.assertIn("PULL NOW", body)
        self.assertIn("set -euo pipefail", body)

    def test_split_guards_all_but_the_gate(self):
        body = "\n".join(recipes()["bank2-split"])
        self.assertIn("!= chimera-gate ]; then just check-no-gate", body)

    def test_no_release_or_flash_recipe_builds_the_gate(self):
        r = recipes()
        shipping = [n for n in r if n.startswith(("release", "flash", "firmware"))]
        self.assertIn("release-bin", shipping)
        self.assertIn("flash", shipping)
        for n in shipping:
            if n == "firmware-gate":
                continue
            for l in r[n]:
                self.assertNotIn("bank2-gate", l, n)
                self.assertNotIn("firmware-gate", l, n)

    def test_the_gate_recipe_never_flashes(self):
        body = "\n".join(recipes()["firmware-gate"])
        self.assertIn("--features bank2-gate", body)
        self.assertIn("target/chimera-gate", body)
        for w in ("dfu-util", "chimera-usb", "just flash", "probe-rs"):
            self.assertNotIn(w, body)


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


class OneUpdate(unittest.TestCase):
    """An update is one thing (Task 9c): a zip a variant, and every route
    flashes and installs through `chimera-usb.py update`."""

    def test_flash_goes_through_update(self):
        body = "\n".join(recipes()["flash"])
        self.assertIn("chimera-usb.py update target/chimera.bin target/chimera.bk2", body)
        self.assertNotIn("dfu-util", body)
        self.assertNotIn("chimera-usb.py bank2", body)

    def test_release_ships_a_zip_a_variant(self):
        body = "\n".join(recipes()["release"])
        for want in (
            'SHA256SUMS chimera-$V-orbit.zip chimera-$V.zip chimera-usb.py ',
            "zip -qX",
            "bank2-seal --check",
            "check-bin-size",
            "check-bin-paths",
            "sha256sum -c --quiet SHA256SUMS",
            "README.txt",
        ):
            self.assertIn(want, body)
        # The zip's members, exactly.
        self.assertIn('"README.txt SHA256SUMS $n.bin $n.bk2"', body)
        # No .bk2 named .bin: a .bin never starts with the .bk2 magic.
        self.assertIn("CHIMERA2", body)
        self.assertNotIn('"$out/chimera-$V.bin"', body)


if __name__ == "__main__":
    unittest.main()
