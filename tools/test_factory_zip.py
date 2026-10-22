"""factory_zip packs the whole card set, or refuses (standard library only)."""

import importlib.util
import json
import pathlib
import tempfile
import unittest
import zipfile

spec = importlib.util.spec_from_file_location(
    "factory_zip", pathlib.Path(__file__).with_name("factory_zip.py")
)
factory_zip = importlib.util.module_from_spec(spec)
spec.loader.exec_module(factory_zip)


def builds(root, sources=("0BADF00D", "12345678")):
    """A public set of 18 Sounds (the 16 ports among them) and 2 projects,
    and a TAPE set of 2 Sounds playing `sources`."""
    pub, tapes = root / "public", root / "tapes"
    for d in ("SOUNDS", "PROJECTS"):
        (pub / "CHIMERA" / d).mkdir(parents=True)
    for n in range(10, 28):
        (pub / "CHIMERA/SOUNDS" / f"S91000{n:02}.A").write_bytes(b"s")
    for n in (1, 2):
        (pub / "CHIMERA/PROJECTS" / f"P000000{n}.A").write_bytes(b"p")
    (pub / "manifest.json").write_text(json.dumps({"sounds": [{}] * 18, "projects": [{}] * 2}))
    tapes.mkdir()
    for c in sources:
        (tapes / f"{c}.SRC").write_bytes(b"x")
    snd = [{"file": f"S900000{i + 1}.A", "source": c} for i, c in enumerate(sources)]
    for s in snd:
        (tapes / s["file"]).write_bytes(b"t")
    man = {"sources": [{"crc": c} for c in sources], "sounds": snd}
    (tapes / "manifest.json").write_text(json.dumps(man))
    return pub, tapes


class FactoryZip(unittest.TestCase):
    def test_one_chimera_root_with_every_sound_source_and_project(self):
        with tempfile.TemporaryDirectory() as d:
            root = pathlib.Path(d)
            pub, tapes = builds(root)
            factory_zip.pack(pub, tapes, root / "f.zip")
            names = zipfile.ZipFile(root / "f.zip").namelist()
        self.assertTrue(all(n.startswith("CHIMERA/") for n in names), names)
        self.assertEqual(sum(n.startswith("CHIMERA/SOUNDS/") for n in names), 20)
        self.assertEqual(sum(n.startswith("CHIMERA/PROJECTS/") for n in names), 2)
        self.assertIn("CHIMERA/TAPES/0BADF00D.SRC", names)
        self.assertIn("CHIMERA/TAPES/12345678.SRC", names)

    def test_a_missing_source_refuses(self):
        with tempfile.TemporaryDirectory() as d:
            root = pathlib.Path(d)
            pub, tapes = builds(root)
            man = json.loads((tapes / "manifest.json").read_text())
            man["sources"].pop()
            (tapes / "manifest.json").write_text(json.dumps(man))
            with self.assertRaises(SystemExit) as e:
                factory_zip.pack(pub, tapes, root / "f.zip")
        self.assertIn("12345678.SRC, not in the zip", str(e.exception))

    def test_a_wrong_sound_count_refuses(self):
        names = [f"CHIMERA/SOUNDS/S91000{n:02}.A" for n in range(10, 26)]
        bad = factory_zip.problems(names, {"sounds": [{}] * 17, "projects": []}, {"sources": [], "sounds": []})
        self.assertIn("16 Sounds, not 17", bad)
        self.assertIn("no TAPE sources", bad)

    def test_the_release_packs_through_it(self):
        body = (pathlib.Path(__file__).parent.parent / "Justfile").read_text()
        self.assertIn("tools/factory_zip.py", body)


if __name__ == "__main__":
    unittest.main()
