#!/usr/bin/env python3
"""The A/B stimuli (reverb TYPEs spec § A/B): 48 kHz mono float WAVs, each
followed by 12 s of silence, from seed 343. Writes <dir>/<name>.wav. The
drums are synthesised here (Chimera has no drum engine yet); the strings,
the pad and the IDM phrase are played on Chimera's own engines by
`reverb_render_test`'s `stimuli`, which reads the kick, snare and hat."""

import math
import random
import struct
import sys
from pathlib import Path

SR = 48_000
TAIL = 12 * SR


def db(x):
    return 10 ** (x / 20)


def peak_to(x, dbfs):
    g = db(dbfs) / max(abs(s) for s in x)
    return [s * g for s in x]


def impulse(_rng):
    return [db(-6)]


def pink(rng):
    # Kellet's economy filter, 0.5 s at −12 dBFS RMS.
    b0 = b1 = b2 = 0.0
    x = []
    for _ in range(SR // 2):
        w = rng.uniform(-1, 1)
        b0 = 0.99765 * b0 + w * 0.0990460
        b1 = 0.96300 * b1 + w * 0.2965164
        b2 = 0.57000 * b2 + w * 1.0526913
        x.append(b0 + b1 + b2 + w * 0.1848)
    g = db(-12) / math.sqrt(sum(s * s for s in x) / len(x))
    return [s * g for s in x]


def snare(rng):
    # A dry snare: a falling 190 → 160 Hz body and a noise rattle, 300 ms.
    n = int(0.3 * SR)
    x, ph = [], 0.0
    for i in range(n):
        t = i / SR
        ph += 2 * math.pi * (160 + 30 * math.exp(-t / 0.02)) / SR
        body = math.sin(ph) * math.exp(-t / 0.05)
        rattle = rng.uniform(-1, 1) * math.exp(-t / 0.07)
        x.append(0.6 * body + 0.8 * rattle)
    return peak_to(x, -12)


def kick(_rng):
    # A dry kick: a sine falling 150 → 45 Hz, 350 ms, and a 2 ms click.
    n = int(0.35 * SR)
    x, ph = [], 0.0
    for i in range(n):
        t = i / SR
        ph += 2 * math.pi * (45 + 105 * math.exp(-t / 0.03)) / SR
        click = math.exp(-t / 0.002) * 0.5
        x.append(math.sin(ph) * math.exp(-t / 0.12) + click)
    return peak_to(x, -12)


def hat(rng):
    # A closed hat: white noise, differenced twice (a crude high-pass), 50 ms.
    n = int(0.05 * SR)
    w = [rng.uniform(-1, 1) for _ in range(n + 2)]
    x = [(w[i + 2] - 2 * w[i + 1] + w[i]) * math.exp(-i / SR / 0.012) for i in range(n)]
    return peak_to(x, -18)


def saw_chord(_rng):
    # C3, E♭3, G3, C4: band-limited saws, 0.6 s with 5 ms edges.
    n = int(0.6 * SR)
    hz = [130.81, 155.56, 196.0, 261.63]
    x = []
    for i in range(n):
        t = i / SR
        env = min(1.0, t / 0.005, (n - i) / SR / 0.005)
        s = 0.0
        for f in hz:
            for k in range(1, int(SR / 2 / f)):
                s += math.sin(2 * math.pi * k * f * t) / k
        x.append(env * s)
    return peak_to(x, -12)


STIMULI = {
    "impulse": impulse,
    "pink": pink,
    "snare": snare,
    "kick": kick,
    "hat": hat,
    "saw_chord": saw_chord,
}


def write(path, x):
    data = b"".join(struct.pack("<f", s) for s in x)
    head = b"RIFF" + struct.pack("<I", 36 + len(data)) + b"WAVEfmt "
    head += struct.pack("<IHHIIHH", 16, 3, 1, SR, SR * 4, 4, 32)
    path.write_bytes(head + b"data" + struct.pack("<I", len(data)) + data)


def main():
    out = Path(sys.argv[1] if len(sys.argv) > 1 else "target/ab/stimuli")
    out.mkdir(parents=True, exist_ok=True)
    for name, make in STIMULI.items():
        x = make(random.Random(343)) + [0.0] * TAIL
        write(out / f"{name}.wav", x)


if __name__ == "__main__":
    main()
