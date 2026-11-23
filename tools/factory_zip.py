# Copyright (C) 2026 Joe Giralt
# SPDX-License-Identifier: GPL-3.0-or-later

"""`factory-v1.zip`: the card's CHIMERA folder as a tester copies it over
the bootloader's SD card access. Every factory Sound (the public set's
and TAPE's) in SOUNDS, the TAPE sources in TAPES, the demo projects in
PROJECTS, the factory controller maps (`just suggest-map`) in MAPS, one
CHIMERA root and nothing beside it. Refuses a pack whose counts aren't
the two builds' manifests', that lacks a source a TAPE Sound plays, or
that holds no map. Standard library only.

    python3 -I tools/factory_zip.py <factory-sounds/public> <tapes-v1> <factory-maps> <out.zip>
"""

import json
import pathlib
import re
import sys
import zipfile

# The 16 TX81Z-inspired ports ship public (ADR 0138).
PORTS = re.compile(r"CHIMERA/SOUNDS/S91000(1[0-9]|2[0-5])\.A$")


def entries(public, tapes, maps):
    """Each zip path and the file it holds, and the two manifests."""
    pm = json.loads((public / "manifest.json").read_text())
    tm = json.loads((tapes / "manifest.json").read_text())
    out = {}
    for sub in ("SOUNDS", "PROJECTS"):
        for p in sorted((public / "CHIMERA" / sub).glob("*.A")):
            out[f"CHIMERA/{sub}/{p.name}"] = p
    for p in sorted((maps / "CHIMERA" / "MAPS").glob("*.A")):
        out[f"CHIMERA/MAPS/{p.name}"] = p
    for s in tm["sounds"]:
        out[f"CHIMERA/SOUNDS/{s['file']}"] = tapes / s["file"]
    for s in tm["sources"]:
        out[f"CHIMERA/TAPES/{s['crc']}.SRC"] = tapes / f"{s['crc']}.SRC"
    return out, pm, tm


def problems(names, pm, tm):
    """What's wrong with a zip of `names`, against the manifests."""
    bad = []
    sounds = [n for n in names if re.fullmatch(r"CHIMERA/SOUNDS/S\d{7}\.A", n)]
    projects = [n for n in names if re.fullmatch(r"CHIMERA/PROJECTS/P\d{7}\.A", n)]
    srcs = {n for n in names if re.fullmatch(r"CHIMERA/TAPES/[0-9A-F]{8}\.SRC", n)}
    maps = [n for n in names if re.fullmatch(r"CHIMERA/MAPS/[0-9A-Z]{5,8}\.A", n)]
    if not maps:
        bad.append("no controller maps")
    want = len(pm["sounds"]) + len(tm["sounds"])
    if len(sounds) != want:
        bad.append(f"{len(sounds)} Sounds, not {want}")
    if len(projects) != len(pm["projects"]):
        bad.append(f"{len(projects)} projects, not {len(pm['projects'])}")
    if not srcs:
        bad.append("no TAPE sources")
    elif len(srcs) != len(tm["sources"]):
        bad.append(f"{len(srcs)} TAPE sources, not {len(tm['sources'])}")
    for s in tm["sounds"]:
        if f"CHIMERA/TAPES/{s['source']}.SRC" not in srcs:
            bad.append(f"{s['file']} plays {s['source']}.SRC, not in the zip")
    if sum(1 for n in names if PORTS.search(n)) != 16:
        bad.append("not all 16 ports")
    stray = len(names) - len(sounds) - len(projects) - len(srcs) - len(maps)
    if stray:
        bad.append(f"{stray} files outside SOUNDS, PROJECTS, TAPES and MAPS")
    return bad


def pack(public, tapes, maps, dst):
    files, pm, tm = entries(public, tapes, maps)
    bad = problems(list(files), pm, tm)
    bad += [f"{n} has no {p}" for n, p in sorted(files.items()) if not p.is_file()]
    if bad:
        sys.exit("factory zip: " + "; ".join(bad))
    with zipfile.ZipFile(dst, "w", zipfile.ZIP_DEFLATED) as z:
        for n in sorted(files):
            i = zipfile.ZipInfo(n, (2026, 1, 1, 0, 0, 0))
            i.compress_type = zipfile.ZIP_DEFLATED
            i.external_attr = 0o644 << 16
            z.writestr(i, files[n].read_bytes())
    # What was written, checked again.
    with zipfile.ZipFile(dst) as z:
        names = z.namelist()
    bad = problems(names, pm, tm)
    if bad:
        sys.exit("factory zip: " + "; ".join(bad))
    for d in ("SOUNDS", "TAPES", "PROJECTS", "MAPS"):
        print(f"CHIMERA/{d}: {sum(1 for n in names if n.startswith(f'CHIMERA/{d}/'))}")


if __name__ == "__main__":
    if len(sys.argv) != 5:
        sys.exit(__doc__)
    pack(*(pathlib.Path(a) for a in sys.argv[1:4]), sys.argv[4])
