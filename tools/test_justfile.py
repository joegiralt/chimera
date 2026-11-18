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
        m = re.match(r"([a-z][\w-]*)( [^:]*)?:( |$)", l)
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
        self.assertIn("SLOT-GATE-BUILD", body)
        self.assertIn("set -euo pipefail", body)

    def test_the_slot_marker_is_in_the_gate_build(self):
        src = (ROOT / "chimera-board/src/bank2.rs").read_text()
        self.assertIn('*b"SLOT-GATE-BUILD"', src)

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
        for want in ("0x08180000", "0x081C0000", "0x081BFFE0", "0x081FFFE0", "INSERT BEFORE .text"):
            self.assertIn(want, src)
        # lld reads a fill as a 32-bit big-endian value: `=0xFF` is 00 00 00 FF.
        self.assertIn("} > BANK2 =0xFFFFFFFF\n", src)
        self.assertIn("LENGTH = 1M", (ROOT / "chimera-stm32/memory.x").read_text())

    def test_bank2_split_runs_check_slots(self):
        body = "\n".join(recipes()["bank2-split"])
        self.assertIn("just check-slots {{elf}} {{out}}.bk2", body)
        self.assertLess(body.index("bank2-seal {{elf}}"), body.index("just check-slots"))

    def test_check_runs_check_bank2_for_each_budgeted_set(self):
        body = recipes()["check"]
        for s in ("default", "orbit", "bank2-gate"):
            b1 = [i for i, l in enumerate(body) if l.strip() == f"just check-bank1 {s}"]
            b2 = [i for i, l in enumerate(body) if l.strip() == f"just check-bank2 {s}"]
            self.assertEqual(len(b1), 1, s)
            self.assertEqual(b2, [b1[0] + 1], s)

    def test_check_door_placed_bound(self):
        body = "\n".join(recipes()["check-door-placed"])
        self.assertIn("0x08200000", body)
        self.assertNotIn("0x08180000", body)


class Bank2Code(unittest.TestCase):
    """Code behind the door (ADR 0137, 0142) is checked like bank 1's."""

    def test_the_stack_check_reads_the_door_table(self):
        body = "\n".join(recipes()["stack-check"])
        dump = [l for l in body.splitlines() if " -s " in l]
        self.assertTrue(dump)
        for l in dump:
            self.assertIn("-j .bank2", l, "door targets would go uncharged")

    def test_every_bench_area_is_built(self):
        sets = re.search(r'^bench_sets := "([^"]*)"', (ROOT / "Justfile").read_text(), re.M).group(1).split()
        for area in ("bench-algo", "bench-routing", "bench-memory", "bench-stress", "bench-orbit", "bench-tape"):
            self.assertTrue([s for s in sets if area in s.split(",")], area)

    def test_bank_2_is_never_a_firmware_feature(self):
        """ADR 0194: every firmware build links bank 2."""
        toml = (ROOT / "chimera-stm32/Cargo.toml").read_text()
        self.assertNotRegex(toml, r"(?m)^bank2 = ")
        for dep in ("chimera-board", "chimera-core", "chimera-engine"):
            self.assertRegex(toml, rf'(?m)^{dep} = {{.*features = \["bank2"\]')
        self.assertNotIn("CARGO_FEATURE_BANK2", (ROOT / "chimera-stm32/build.rs").read_text())
        split = "\n".join(recipes()["bank2-split"])
        self.assertRegex(split, r"else\n\s+echo .*no bank 2.*exit 1", "an empty .bank2 must fail the split")

    def test_check_runs_the_proxies_with_their_door(self):
        body = "\n".join(recipes()["check"])
        self.assertRegex(body, r"--features bank2,test-support --test tape_proxy_test --test reverb_proxy_test --test va_proxy_test")

    def test_split_checks_the_va_door_is_in_bank_2(self):
        body = "\n".join(recipes()["bank2-split"])
        self.assertIn("just check-door-placed {{elf}}", body)


class OneUpdate(unittest.TestCase):
    """An update is one thing: `flash` installs through `chimera-usb.py
    update`; a release ships one firmware, the ORBIT build's .bin and its
    sealed .bk2, dropped on the card."""

    def test_flash_goes_through_update(self):
        body = "\n".join(recipes()["flash"])
        self.assertIn("chimera-usb.py update target/chimera.bin target/chimera.bk2", body)
        self.assertNotIn("dfu-util", body)
        self.assertNotIn("chimera-usb.py bank2", body)

    def test_release_ships_the_orbit_bin_and_its_bk2(self):
        body = "\n".join(recipes()["release"])
        for want in (
            'CHIMERA.BIN CHIMERA.BK2 SHA256SUMS factory-v1.zip ',
            'cp target/release-bin/chimera-orbit.bin "$out/CHIMERA.BIN"',
            'cp target/release-bin/chimera-orbit.bk2 "$out/CHIMERA.BK2"',
            'bank2-seal --check "$out/CHIMERA.BIN" "$out/CHIMERA.BK2"',
            "check-bin-paths",
        ):
            self.assertIn(want, body)
        # No .bk2 named .bin: a .bin never starts with the .bk2 magic.
        self.assertIn("CHIMERA2", body)
        self.assertIn("`CHIMERA.BIN`", open("docs/alpha/RELEASE-NOTES.md.tmpl").read())
        # No zips a variant, no console tool shipped.
        self.assertNotIn("zip -qX", body)
        self.assertNotIn('"$out/chimera-usb.py"', body)


class Itcm(unittest.TestCase):
    def test_every_split_checks_itcm(self):
        body = "\n".join(recipes()["bank2-split"])
        self.assertIn("just check-itcm {{elf}}", body)

    def test_check_splits_sd_probe_as_its_flash_recipe_does(self):
        # sd-probe links no synth and leaves ITCM empty: its split
        # runs in `check`, so check-itcm can't break `flash-sd-probe` unseen.
        split = "just bank2-split target/thumbv7em-none-eabihf/release/chimera-stm32 target/chimera-sd-probe"
        self.assertIn(split, "\n".join(recipes()["flash-sd-probe"]))
        body = recipes()["check"]
        i = next(n for n, l in enumerate(body) if "--features sd-probe" in l)
        self.assertIn(split, "\n".join(body[i + 1 : i + 3]))

    def test_check_itcm_reads_the_residents_file(self):
        self.assertIn("tools/check_itcm.py", "\n".join(recipes()["check-itcm"]))


if __name__ == "__main__":
    unittest.main()
