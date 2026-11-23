# Copyright (C) 2026 Joe Giralt
# SPDX-License-Identifier: GPL-3.0-or-later

"""Copyright and licence headers on every published source file (ADR 0330):
Rust, Python, shell, linker scripts, build scripts and the Justfile. Files
derived from others' work carry their authors' lines above ours. The name
line and the licence ID are one constant each: change one, run `just
headers`, and every header follows. A generated file is exempt if its first
lines say `@generated`. Standard library only.

    python3 -I tools/headers.py           # insert or rewrite headers
    python3 -I tools/headers.py --check   # list files without theirs; exit 1
"""

import importlib.util
import pathlib
import subprocess
import sys

OWNER = "Copyright (C) 2026 Joe Giralt"
LICENCE = "GPL-3.0-or-later"

ROOT = pathlib.Path(__file__).resolve().parent.parent

_spec = importlib.util.spec_from_file_location("oss_export", pathlib.Path(__file__).with_name("oss_export.py"))
oss = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(oss)

GILLET_RINGS = "Copyright 2014-2015 Emilie Gillet (Mutable Instruments Rings, MIT; notice below)"
GILLET_BEADS = "Copyright 2017 Emilie Gillet (Mutable Instruments Beads, MIT; notice below)"
GILLET_PLAITS = "Copyright 2014-2016 Emilie Gillet (Mutable Instruments Plaits and stmlib, MIT; notice below)"
SCIPY = "Copyright (c) 2001-2002 Enthought, Inc., 2003 SciPy Developers (SciPy, BSD-3-Clause; notice below)"
REVERB = [
    "ROOM, BLOOM and LOOP start from delay and coefficient values derived from",
    "thement/midiverb_emulator (GPL-3.0). The parameterizable engine built",
    "around them is original to Chimera.",
]
CC_BY = "(CC BY 4.0; see THIRD_PARTY.md)"

# Upstream lines, above ours, by path (THIRD_PARTY.md has the full credits).
UPSTREAM = {
    "chimera-engine/src/dsp/modal/mod.rs": [GILLET_RINGS],
    "chimera-engine/src/dsp/modal/rings.rs": [GILLET_RINGS],
    "chimera-engine/src/dsp/modal/dispersion.rs": [GILLET_RINGS],
    "chimera-engine/src/dsp/modal/chords.rs": ["Copyright 2015 Emilie Gillet (Mutable Instruments Rings, MIT; notice below)"],
    "chimera-engine/src/dsp/grain/core.rs": [GILLET_BEADS],
    "chimera-engine/src/dsp/delay/grain.rs": ["Copyright 2017 Emilie Gillet (Mutable Instruments Beads, MIT; see dsp/grain/core.rs)"],
    "chimera-engine/src/dsp/mod.rs": ["Copyright 2012 Emilie Gillet (stmlib's SoftClip in fast_tanh, MIT; notice: see dsp/modal/mod.rs)"],
    "chimera-engine/src/dsp/filter/coef.rs": ["Copyright 2014 Emilie Gillet (stmlib's FREQUENCY_ACCURATE, MIT; notice: see dsp/modal/mod.rs)"],
    "chimera-engine/src/dsp/lpg.rs": [GILLET_PLAITS],
    "chimera-circuits/src/plaits_lpg/mod.rs": [GILLET_PLAITS],
    "chimera-engine/src/dsp/delay/tape.rs": [
        "Copyright (C) 1993 Sun Microsystems, Inc.; Copyright (c) 2005-2020 Rich Felker, et al.;",
        "Copyright (c) 2018 Jorge Aparicio (tanhf, expm1f via musl and libm; notices below)",
    ],
    "chimera-utils/src/sig.rs": [SCIPY],
    "chimera-circuits/src/util/sig.rs": [SCIPY],
    "chimera-utils/src/stim.rs": [
        "Copyright (c) 2005-2024 NumPy Developers; Copyright (c) 2019 Kevin Sheppard (NumPy, BSD-3-Clause)",
        "Copyright 2014 Melissa O'Neill (PCG64, Apache-2.0 or MIT; notices below)",
    ],
    "chimera-engine/src/dsp/rev/room.rs": REVERB,
    "chimera-engine/src/dsp/rev/bloom.rs": REVERB,
    "chimera-engine/src/dsp/rev/loop_engine.rs": REVERB,
    "chimera-board/src/display.rs": ["Copyright (c) 2017 martnak (the ILI9341 init list, martnak/STM32-ILI9341, MIT, via PreenFM3; notice below)"],
    "chimera-engine/src/dsp/va/lbfm.rs": [f"After Tamara Smyth, Loopback Frequency Modulation, DAFx-26 {CC_BY}"],
    "chimera-engine/src/dsp/filter/sem.rs": [f"After L. Köper, M. Holters, F. Esqueda, J. D. Parker, DAFx20in22 {CC_BY}"],
    "chimera-circuits/src/sem1a/mod.rs": [f"After L. Köper, M. Holters, F. Esqueda, J. D. Parker, DAFx20in22 {CC_BY}"],
    "chimera-circuits/src/vtl5c3/mod.rs": [f"After J. Najnudel, R. Müller, T. Hélie, D. Roze, DAFx-23 {CC_BY}"],
    "chimera-circuits/src/buchla292b/mod.rs": [f"After J. Najnudel, R. Müller, T. Hélie, D. Roze, DAFx-23 {CC_BY}"],
}


# Files holding others' permissive code carry their licence too, REUSE-style.
MIT = "MIT"
BSD = "BSD-3-Clause"
EXTRA_LICENCE = {
    "chimera-engine/src/dsp/modal/mod.rs": MIT,
    "chimera-engine/src/dsp/modal/rings.rs": MIT,
    "chimera-engine/src/dsp/modal/dispersion.rs": MIT,
    "chimera-engine/src/dsp/modal/chords.rs": MIT,
    "chimera-engine/src/dsp/grain/core.rs": MIT,
    "chimera-engine/src/dsp/delay/grain.rs": MIT,
    "chimera-engine/src/dsp/mod.rs": MIT,
    "chimera-engine/src/dsp/filter/coef.rs": MIT,
    "chimera-engine/src/dsp/lpg.rs": MIT,
    "chimera-circuits/src/plaits_lpg/mod.rs": MIT,
    "chimera-engine/src/dsp/delay/tape.rs": "MIT AND SunPro",
    "chimera-utils/src/sig.rs": BSD,
    "chimera-circuits/src/util/sig.rs": BSD,
    "chimera-utils/src/stim.rs": "BSD-3-Clause AND (Apache-2.0 OR MIT)",
    "chimera-board/src/display.rs": MIT,
}


def spdx(path):
    extra = EXTRA_LICENCE.get(path)
    return f"{LICENCE} AND {extra}" if extra else LICENCE


def style(path):
    """The comment form for `path`: (line prefix, line suffix), or None."""
    name = path.rsplit("/", 1)[-1]
    if path.endswith(".rs"):
        return ("// ", "")
    if path.endswith((".py", ".sh")) or name == "Justfile":
        return ("# ", "")
    if path.endswith(".x"):
        return ("/* ", " */")
    return None


def block(path):
    pre, suf = style(path)
    lines = UPSTREAM.get(path, []) + [OWNER, f"SPDX-License-Identifier: {spdx(path)}"]
    return [f"{pre}{l}{suf}" for l in lines]


def generated(text):
    return any("@generated" in l for l in text.splitlines()[:5])


def apply(path, text):
    """`text` with `path`'s header on top: inserted, or the old one rewritten."""
    if generated(text):
        return text
    pre, suf = style(path)
    lines = text.split("\n")
    head = [lines.pop(0)] if lines and lines[0].startswith("#!") and not lines[0].startswith("#![") else []
    # The header this tool wrote: comment lines from the top through the
    # first SPDX line, then one blank line.
    for i, l in enumerate(lines[:12]):
        if not (l.startswith(pre.rstrip()) and l.endswith(suf)):
            break
        if "SPDX-License-Identifier:" in l:
            lines = lines[i + 1 :]
            if lines and lines[0] == "":
                lines.pop(0)
            break
    return "\n".join(head + block(path) + [""] + lines)


def published(root=ROOT):
    """The source files the export publishes, repo-relative."""
    if (root / ".git").exists():
        out = subprocess.run(["git", "-C", str(root), "ls-files", "-z"], check=True, capture_output=True).stdout
        paths = [p for p in out.decode().split("\0") if p]
    else:
        paths = [p.relative_to(root).as_posix() for p in root.rglob("*") if p.is_file()]
    return sorted(p for p in paths if style(p) and not oss.private(p) and (root / p).is_file())


def missing(root=ROOT):
    """Published source files whose header is absent or stale."""
    bad = []
    for p in published(root):
        text = (root / p).read_text(encoding="utf-8")
        if apply(p, text) != text:
            bad.append(p)
    return bad


def main(argv):
    if argv == ["--check"]:
        bad = missing()
        for p in bad:
            print(f"no header: {p} (just headers)", file=sys.stderr)
        return 1 if bad else 0
    n = 0
    for p in published():
        f = ROOT / p
        text = f.read_text(encoding="utf-8")
        new = apply(p, text)
        if new != text:
            f.write_text(new, encoding="utf-8")
            n += 1
    print(f"headers: {n} files written")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
