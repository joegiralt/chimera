"""Host tests for chimera-usb.py against a fake unit (standard library only).

Run: python3 -m unittest discover -s tools -p 'test_*.py'
"""

import base64
import contextlib
import datetime
import hashlib
import importlib.util
import io
import os
import pathlib
import re
import shutil
import socket
import struct
import subprocess
import sys
import tempfile
import threading
import time
import unittest
import zlib

ROOT = pathlib.Path(__file__).resolve().parent.parent
TOOL = ROOT / "tools/chimera-usb.py"

_spec = importlib.util.spec_from_file_location("chimera_usb", TOOL)
tool = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(tool)


HANG_UP = object()


class FakeUnit:
    """A console on 127.0.0.1:<free port> that answers canned lines.

    `preamble` goes out as soon as a client connects, as a stalled answer's
    tail would sit in the port. A request with no canned answer gets
    silence; one whose answer is `HANG_UP` drops the connection, as a unit
    resetting would. `on_answer` runs after each answer."""

    def __init__(self, answers, preamble=b"", on_answer=None):
        self.answers = answers
        self.preamble = preamble
        self.on_answer = on_answer
        self.requests = []
        self.server = socket.create_server(("127.0.0.1", 0))
        self.server.settimeout(0.1)
        self.target = "tcp:127.0.0.1:%d" % self.server.getsockname()[1]
        self.stop = threading.Event()
        self.thread = threading.Thread(target=self._serve, daemon=True)

    def _serve(self):
        while not self.stop.is_set():
            try:
                conn, _ = self.server.accept()
            except TimeoutError:
                continue
            with conn:
                conn.settimeout(0.1)
                conn.sendall(self.preamble)
                buf = b""
                while not self.stop.is_set():
                    try:
                        data = conn.recv(4096)
                    except TimeoutError:
                        continue
                    if not data:
                        break
                    buf += data
                    while b"\n" in buf:
                        line, buf = buf.split(b"\n", 1)
                        line += b"\n"
                        self.requests.append(line)
                        if line in self.answers:
                            if self.answers[line] is HANG_UP:
                                return
                            conn.sendall(self.answers[line])
                            if self.on_answer:
                                self.on_answer()

    def __enter__(self):
        self.thread.start()
        return self

    def __exit__(self, *exc):
        self.stop.set()
        self.thread.join()
        self.server.close()


class FakeSysfs:
    """`bus/usb/devices/<n>/idVendor` and `idProduct` under a temp dir,
    lowercase hex as the kernel writes them."""

    def __init__(self, ids):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = self.tmp.name
        self.timers = []
        self._write(ids)

    def _write(self, ids):
        devices = pathlib.Path(self.root, "bus/usb/devices")
        if devices.exists():
            for d in devices.iterdir():
                for f in d.iterdir():
                    f.unlink()
                d.rmdir()
        for n, (vid, pid) in enumerate(sorted(ids)):
            d = devices / ("1-%d" % (n + 1))
            d.mkdir(parents=True)
            (d / "idVendor").write_text("%04x\n" % vid)
            (d / "idProduct").write_text("%04x\n" % pid)
        devices.mkdir(parents=True, exist_ok=True)

    def replace(self, ids, after):
        """The device leaves and `ids` arrive, `after` seconds from now."""
        t = threading.Timer(after, self._write, (ids,))
        self.timers.append(t)
        t.start()

    def __enter__(self):
        return self

    def __exit__(self, *exc):
        for t in self.timers:
            t.cancel()
            if t.is_alive():
                t.join()
        self.tmp.cleanup()


def run_tool(args, target, cwd=None, sysfs=None, env=None):
    e = dict(os.environ)
    e["CHIMERA_USB"] = target
    # Shots land in the test's temp dir, never the repo's target/shots.
    e["CHIMERA_SHOTS"] = cwd or tempfile.gettempdir()
    if sysfs is not None:
        e["CHIMERA_SYSFS"] = sysfs
    e.update(env or {})
    return subprocess.run(
        [sys.executable, str(TOOL), *args], cwd=cwd, env=e, capture_output=True, timeout=30
    )


def read_png(path):
    """(width, height, RGB bytes) of an 8-bit RGB PNG with filter 0 rows."""
    data = path.read_bytes()
    assert data[:8] == b"\x89PNG\r\n\x1a\n"
    pos, idat, w, h = 8, b"", None, None
    while pos < len(data):
        (n,) = struct.unpack(">I", data[pos : pos + 4])
        kind, body = data[pos + 4 : pos + 8], data[pos + 8 : pos + 8 + n]
        (crc,) = struct.unpack(">I", data[pos + 8 + n : pos + 12 + n])
        assert crc == zlib.crc32(kind + body), kind
        if kind == b"IHDR":
            w, h, depth, colour = struct.unpack(">IIBB", body[:10])
            assert (depth, colour) == (8, 2)
        elif kind == b"IDAT":
            idat += body
        pos += 12 + n
    raw = zlib.decompress(idat)
    stride = 1 + 3 * w
    assert len(raw) == stride * h
    assert all(raw[y * stride] == 0 for y in range(h))
    return w, h, b"".join(raw[y * stride + 1 : (y + 1) * stride] for y in range(h))


def rgb_at(rgb, w, x, y):
    i = 3 * (y * w + x)
    return tuple(rgb[i : i + 3])


def widen(v):
    r, g, b = v >> 11, (v >> 5) & 0x3F, v & 0x1F
    return (r << 3 | r >> 2, g << 2 | g >> 4, b << 3 | b >> 2)


class Tool(unittest.TestCase):
    def test_status_prints_the_body_and_exits_0(self):
        with FakeUnit({b"status\n": b"firmware 0.1.0 release\nOK\n"}) as u:
            r = run_tool(["status"], u.target)
        self.assertEqual((r.returncode, r.stdout), (0, b"firmware 0.1.0 release\n"))

    def test_flash_prints_the_registers(self):
        body = b"acr 00000037 latency 7 wrhighfreq 3\npeek 08100000 ffffffff erased\n"
        with FakeUnit({b"flash\n": body + b"OK\n"}) as u:
            r = run_tool(["flash"], u.target)
        self.assertEqual((r.returncode, r.stdout), (0, body))

    def test_help_names_flash(self):
        self.assertIn("chimera-usb.py flash ", tool.__doc__)

    def test_err_goes_to_stderr_and_exits_1(self):
        with FakeUnit({b"stats\n": b"ERR stats is not in this build\n"}) as u:
            r = run_tool(["stats"], u.target)
        self.assertEqual(r.returncode, 1)
        self.assertEqual(r.stdout, b"")
        self.assertIn(b"ERR stats is not in this build", r.stderr)

    def test_silence_fails_in_about_two_seconds(self):
        with FakeUnit({}) as u:  # never answers
            t = time.monotonic()
            r = run_tool(["status"], u.target)
            took = time.monotonic() - t
        self.assertEqual(r.returncode, 1)
        self.assertIn(b"no answer from", r.stderr)
        self.assertTrue(1.8 < took < 4.0, took)

    def test_card_waits_longer_for_its_first_line_only(self):
        class Scripted:
            def __init__(self, chunks):
                self.chunks, self.waits = list(chunks), []

            def read(self, timeout):
                self.waits.append(timeout)
                return self.chunks.pop(0) if self.chunks else b""

        c = Scripted([b"streams 12\n", b"OK\n"])
        self.assertEqual(tool.answer(c, tool.first_wait("card")), (tool.Status.OK, b"streams 12\n"))
        self.assertEqual(c.waits, [tool.CARD_WAIT_S, tool.SILENCE_S])
        self.assertEqual(tool.first_wait("stats"), tool.SILENCE_S)
        self.assertGreaterEqual(tool.CARD_WAIT_S, 30)

    def test_a_card_stopped_short_prints_what_it_measured(self):
        with FakeUnit({b"card seed=7\n": b"seed 7\noverruns 0\nERR card: timeout\n"}) as u:
            r = run_tool(["card", "seed=7"], u.target)
        self.assertEqual((r.returncode, r.stdout), (1, b"seed 7\noverruns 0\n"))
        self.assertIn(b"ERR card: timeout", r.stderr)

    def test_card_prints_its_body(self):
        with FakeUnit({b"card\n": b"streams 12\nOK\n"}) as u:
            r = run_tool(["card"], u.target)
        self.assertEqual((r.returncode, r.stdout), (0, b"streams 12\n"))

    def test_shot_writes_a_png_of_the_pixels(self):
        px = bytes(range(256)) * 600  # 153 600 bytes
        with FakeUnit({b"shot\n": b"SHOT 240 320 rgb565be 153600\n" + px + b"OK\n"}) as u, tempfile.TemporaryDirectory() as d:
            r = run_tool(["shot"], u.target, cwd=d)
            path = pathlib.Path(d, r.stdout.decode().strip())
            self.assertEqual(path.parent, pathlib.Path(d))
            w, h, rgb = read_png(path)
        self.assertEqual((w, h), (480, 640))
        self.assertEqual(rgb_at(rgb, w, 0, 0), widen(px[0] << 8 | px[1]))
        self.assertEqual(rgb_at(rgb, w, 1, 1), widen(px[0] << 8 | px[1]), "2x nearest")
        self.assertEqual(rgb_at(rgb, w, 2, 0), widen(px[2] << 8 | px[3]))
        self.assertEqual(rgb_at(rgb, w, 0, 2), widen(px[480] << 8 | px[481]))
        self.assertEqual(widen(0xFFFF), (255, 255, 255))
        self.assertTrue(path.name.startswith("shot-") and not path.name.startswith("shot-raw-"))

    def test_shot_raw_asks_for_raw_and_names_it(self):
        px = bytes(153600)
        with FakeUnit({b"shot raw\n": b"SHOT 240 320 rgb565be 153600\n" + px + b"OK\n"}) as u, tempfile.TemporaryDirectory() as d:
            r = run_tool(["shot", "raw"], u.target, cwd=d)
        self.assertEqual(r.returncode, 0)
        self.assertIn(b"shot-raw-", r.stdout)

    def test_a_bad_shot_header_is_refused(self):
        with FakeUnit({b"shot\n": b"SHOT 240 320 rgb565be 999\n" + bytes(999) + b"OK\n"}) as u, tempfile.TemporaryDirectory() as d:
            r = run_tool(["shot"], u.target, cwd=d)
            self.assertEqual(os.listdir(d), [], "nothing written")
        self.assertEqual(r.returncode, 1)

    def test_a_shot_over_the_cap_is_refused_before_its_body(self):
        # 1024 x 513 x 2 is consistent but over 1 MiB; no body follows.
        with FakeUnit({b"shot\n": b"SHOT 1024 513 rgb565be 1050624\n"}) as u:
            r = run_tool(["shot"], u.target)
        self.assertEqual(r.returncode, 1)
        self.assertIn(b"is not a screen", r.stderr)

    def test_an_empty_shot_is_refused(self):
        with FakeUnit({b"shot\n": b"SHOT 0 320 rgb565be 0\nOK\n"}) as u, tempfile.TemporaryDirectory() as d:
            r = run_tool(["shot"], u.target, cwd=d)
            self.assertEqual(os.listdir(d), [])
        self.assertEqual(r.returncode, 1)

    def test_a_stalled_tail_is_drained_before_the_next_request(self):
        # The unit first sends the tail of an earlier, stalled shot, then answers.
        with FakeUnit({b"status\n": b"firmware x\nOK\n"}, preamble=b"\x00" * 5000) as u:
            r = run_tool(["status"], u.target)
        self.assertEqual((r.returncode, r.stdout), (0, b"firmware x\n"))

    def test_sim_target_is_the_desktop_console_address(self):
        src = (ROOT / "chimera-desktop/src/console.rs").read_text()
        self.assertIn('pub const ADDR: &str = "127.0.0.1:7341";', src)
        self.assertEqual(tool.target({"CHIMERA_USB": "sim"}), ("tcp", "127.0.0.1", 7341))

    def test_targets(self):
        self.assertEqual(tool.target({"CHIMERA_USB": "tcp:h:9"}), ("tcp", "h", 9))
        self.assertEqual(tool.target({"CHIMERA_USB": "/dev/x"}), ("tty", "/dev/x"))
        self.assertIn(tool.target({}), [("tty", "/dev/chimera"), ("tty", "/dev/ttyACM0")])

    def test_shot_path_names_the_kind_and_time(self):
        now = datetime.datetime(2026, 10, 2, 9, 5, 7)
        self.assertEqual(tool.shot_path(False, now).name, "shot-20261002-090507.png")
        self.assertEqual(tool.shot_path(True, now).name, "shot-raw-20261002-090507.png")

    def test_a_tty_without_permission_names_the_udev_rule(self):
        with tempfile.TemporaryDirectory() as d:
            tty = pathlib.Path(d, "ttyACM9")
            tty.write_bytes(b"")
            tty.chmod(0)
            if os.access(tty, os.R_OK):
                self.skipTest("running as root")
            r = run_tool(["status"], str(tty))
        self.assertEqual(r.returncode, 1)
        self.assertEqual(r.stderr.count(b"\n"), 1, r.stderr)
        self.assertIn(b"dialout", r.stderr)
        self.assertIn(b"/etc/udev/rules.d/70-chimera.rules", r.stderr)

    def test_udev_rule_matches_the_firmware_identity(self):
        rule = (ROOT / "tools/70-chimera.rules").read_text()
        usb = (ROOT / "chimera-board/src/usb.rs").read_text()
        self.assertIn("VID_PID: (u16, u16) = (0x0483, 0x5740)", usb)
        for want in (
            'ATTRS{idVendor}=="0483"',
            'ATTRS{idProduct}=="5740"',
            'ENV{ID_MM_DEVICE_IGNORE}="1"',
            'TAG+="uaccess"',
            'SYMLINK+="chimera"',
        ):
            self.assertIn(want, rule)

    def test_to_dfu_sends_dfu_and_waits_for_the_rom(self):
        with FakeSysfs({(0x0483, 0x5740)}) as fs, FakeUnit(
            {b"dfu\n": b"OK\n"}, on_answer=lambda: fs.replace({(0x0483, 0xDF11)}, after=0.3)
        ) as u:
            r = run_tool(["to-dfu"], u.target, sysfs=fs.root)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(u.requests, [b"dfu\n"])

    def test_to_dfu_counts_a_lost_ok_when_the_rom_comes(self):
        # The port vanishes before OK arrives; DF11 then appears.
        with FakeSysfs({(0x0483, 0x5740)}) as fs, FakeUnit({b"dfu\n": HANG_UP}) as u:
            fs.replace({(0x0483, 0xDF11)}, after=0.5)
            r = run_tool(["to-dfu"], u.target, sysfs=fs.root)
        self.assertEqual(r.returncode, 0, r.stderr)

    def test_to_dfu_refused_stops_the_flash(self):
        with FakeSysfs({(0x0483, 0x5740)}) as fs, FakeUnit({b"dfu\n": b"ERR dfu is not in this build\n"}) as u:
            r = run_tool(["to-dfu"], u.target, sysfs=fs.root)
        self.assertEqual(r.returncode, 1)
        self.assertIn(b"ERR dfu is not in this build", r.stderr)

    def test_to_dfu_times_out_when_the_rom_never_comes(self):
        with FakeSysfs({(0x0483, 0x5740)}) as fs, FakeUnit({b"dfu\n": b"OK\n"}) as u:
            r = run_tool(["to-dfu"], u.target, sysfs=fs.root, env={"CHIMERA_DFU_WAIT": "1"})
        self.assertEqual(r.returncode, 1)
        self.assertIn(b"marker clobbered", r.stderr)

    def test_to_dfu_with_the_jumper_sends_nothing(self):
        with FakeSysfs({(0x0483, 0xDF11)}) as fs, FakeUnit({}) as u:
            r = run_tool(["to-dfu"], u.target, sysfs=fs.root)
        self.assertEqual((r.returncode, u.requests), (0, []))

    def test_to_dfu_with_both_ids_sends_nothing(self):
        with FakeSysfs({(0x0483, 0x5740), (0x0483, 0xDF11)}) as fs, FakeUnit({b"dfu\n": b"OK\n"}) as u:
            r = run_tool(["to-dfu"], u.target, sysfs=fs.root)
        self.assertEqual((r.returncode, u.requests), (0, []))

    def test_to_dfu_fails_at_once_when_the_listed_console_wont_open(self):
        with FakeSysfs({(0x0483, 0x5740)}) as fs:
            t = time.monotonic()
            r = run_tool(["to-dfu"], "tcp:127.0.0.1:9", sysfs=fs.root)
            took = time.monotonic() - t
        self.assertEqual(r.returncode, 1)
        self.assertIn(b"no console at 127.0.0.1:9", r.stderr)
        self.assertLess(took, 1.5)

    def test_to_dfu_silence_after_dfu_is_no_answer(self):
        with FakeSysfs({(0x0483, 0x5740)}) as fs, FakeUnit({}) as u:
            r = run_tool(["to-dfu"], u.target, sysfs=fs.root)
        self.assertEqual(r.returncode, 1)
        self.assertIn(b"no answer from", r.stderr)
        self.assertNotIn(b"marker clobbered", r.stderr)

    def test_wait_console_sees_it_come_back(self):
        with FakeSysfs(set()) as fs:
            fs.replace({(0x0483, 0x5740)}, after=0.3)
            r = run_tool(["wait-console", "5"], "tcp:127.0.0.1:9", sysfs=fs.root)
        self.assertEqual(r.returncode, 0, r.stderr)

    def test_wait_console_gives_up(self):
        with FakeSysfs({(0x0483, 0xDF11)}) as fs:
            r = run_tool(["wait-console", "0.3"], "tcp:127.0.0.1:9", sysfs=fs.root)
        self.assertEqual(r.returncode, 1)
        self.assertIn(b"console not back", r.stderr)

    def test_to_dfu_without_a_console_prints_the_jumper(self):
        with FakeSysfs(set()) as fs:
            r = run_tool(["to-dfu"], "tcp:127.0.0.1:9", sysfs=fs.root)
        self.assertEqual(r.returncode, 0)
        self.assertIn(b"bridge BOOT0", r.stdout + r.stderr)

    def test_to_dfu_matches_the_firmware_identity(self):
        self.assertEqual(tool.CONSOLE_ID, (0x0483, 0x5740))
        self.assertEqual(tool.ROM_DFU_ID, (0x0483, 0xDF11))
        just = (ROOT / "Justfile").read_text()
        # `flash` goes through `update` (its DFU is the tool's).
        for recipe in ("flash-bench area", "flash-fault:"):
            body = just.split("\n" + recipe, 1)[1].split("\n\n", 1)[0]
            self.assertLess(body.index("to-dfu"), body.index("dfu-util"), recipe)
            self.assertIn("-s 0x8020000:leave", body, recipe)
            self.assertIn("set -euo pipefail", body, recipe)
            self.assertIn('"$rc" -eq 74', body, recipe)
            self.assertIn("File downloaded successfully", body, recipe)
        fault = just.split("\nflash-fault:", 1)[1].split("\n\n", 1)[0]
        self.assertIn("wait-console 10", fault)

    def test_the_justfile_never_touches_the_option_bytes(self):
        just = (ROOT / "Justfile").read_text()
        self.assertEqual(re.findall(r"-a\s*1\b|--alt[ =]1\b", just), [])
        alts = re.findall(r"^\s+(?:rc=0 && )?dfu-util (\S+)", just, re.M)
        self.assertGreaterEqual(len(alts), 4)
        self.assertEqual(set(alts), {"-a0"})

    def test_backup_bootloader_refuses_to_overwrite(self):
        just = shutil.which("just")
        if not just:
            self.skipTest("no just")
        with tempfile.TemporaryDirectory() as home, FakeSysfs(set()) as fs:
            bindir = pathlib.Path(home, "bin")
            bindir.mkdir()
            ran = pathlib.Path(home, "dfu-util-ran")
            fake = bindir / "dfu-util"  # never the real one, whatever happens
            fake.write_text("#!/bin/sh\ntouch %s\nexit 99\n" % ran)
            fake.chmod(0o755)
            backups = pathlib.Path(home, "chimera-backups")
            backups.mkdir()
            old = backups / ("preenfm3-bootloader-%s.bin" % datetime.date.today().isoformat())
            old.write_bytes(b"keep me")
            env = dict(os.environ, HOME=home, CHIMERA_SYSFS=fs.root, PATH="%s:%s" % (bindir, os.environ["PATH"]))
            r = subprocess.run(
                [just, "--justfile", str(ROOT / "Justfile"), "--working-directory", str(ROOT), "backup-bootloader"],
                env=env, capture_output=True, timeout=30,
            )
            self.assertNotEqual(r.returncode, 0)
            self.assertIn(b"not overwriting", r.stderr)
            self.assertEqual(old.read_bytes(), b"keep me")
            self.assertFalse(ran.exists())


class PutUnit:
    """A console that speaks `put` as the unit does: `READY` (or `refuse`
    first), base64 lines until the size is in, then `OK` if the CRC
    matches, else `ERR put: CRC mismatch`; `after` replaces that answer.
    `silent` says nothing after READY. Also answers `ls` with `listing`.
    A TAPES path takes its data in chunks, each ended by `end <crc32>` and
    answered `ACK <bytes so far>`, or `ERR put: chunk` at the chunk
    `bad_chunk` names; `present` answers `OK PRESENT` before any data.
    The first `busy` puts answer `ERR busy`; `answers` maps other request
    lines to their answers. `refuse_once`: only the first put is refused."""

    def __init__(self, refuse=None, after=None, silent=False, listing=b"OK\n", present=False, bad_chunk=None, busy=0, answers=None, refuse_once=False):
        self.answers = answers or {}
        self.refuse, self.after, self.silent, self.listing = refuse, after, silent, listing
        self.refuse_once = refuse_once
        self.busy = busy
        self.present, self.bad_chunk = present, bad_chunk
        self.requests, self.data, self.ends = [], b"", []
        self.server = socket.create_server(("127.0.0.1", 0))
        self.server.settimeout(0.1)
        self.target = "tcp:127.0.0.1:%d" % self.server.getsockname()[1]
        self.stop = threading.Event()
        self.thread = threading.Thread(target=self._serve, daemon=True)

    def _lines(self, conn):
        buf = b""
        while not self.stop.is_set():
            while b"\n" in buf:
                line, buf = buf.split(b"\n", 1)
                yield line
            try:
                data = conn.recv(65536)
            except TimeoutError:
                continue
            if not data:
                return
            buf += data

    def _serve(self):
        while not self.stop.is_set():
            try:
                conn, _ = self.server.accept()
            except TimeoutError:
                continue
            with conn:
                conn.settimeout(0.1)
                lines = self._lines(conn)
                for line in lines:
                    self.requests.append(line)
                    words = line.split()
                    if words[:1] == [b"ls"]:
                        conn.sendall(self.listing)
                    if line in self.answers:
                        conn.sendall(self.answers[line])
                    if words[:1] != [b"put"]:
                        continue
                    if self.busy:
                        self.busy -= 1
                        conn.sendall(b"ERR busy\n")
                        continue
                    if self.refuse:
                        conn.sendall(self.refuse)
                        if self.refuse_once:
                            self.refuse = None
                        continue
                    size, crc = int(words[2]), int(words[3], 16)
                    if self.present:
                        conn.sendall(b"OK PRESENT\n")
                        continue
                    conn.sendall(b"READY\n")
                    chunk = tool.PUT_CHUNK if words[1].upper().startswith(b"/CHIMERA/TAPES/") else size
                    bad = False
                    while len(self.data) < size and not bad:
                        start = len(self.data)
                        end = min(start + chunk, size)
                        while len(self.data) < end:
                            ln = next(lines)
                            assert len(ln) <= 64 and len(ln) % 4 == 0, ln
                            self.data += base64.b64decode(ln, validate=True)
                        if chunk == size:
                            break
                        ln = next(lines)
                        self.ends.append(ln)
                        if len(self.ends) == self.bad_chunk or ln != b"end %08x" % zlib.crc32(self.data[start:end]):
                            conn.sendall(b"ERR put: chunk\n")
                            bad = True
                        else:
                            conn.sendall(b"ACK %d\n" % end)
                    if self.silent or bad:
                        continue
                    ok = zlib.crc32(self.data) == crc and len(self.data) == size
                    conn.sendall(self.after or (b"OK\n" if ok else b"ERR put: CRC mismatch\n"))

    def __enter__(self):
        self.thread.start()
        return self

    def __exit__(self, *exc):
        self.stop.set()
        self.thread.join()
        self.server.close()


class Put(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)

    def local(self, name, data):
        p = pathlib.Path(self.tmp.name, name)
        p.write_bytes(data)
        return str(p)

    def test_put_sends_size_crc_and_the_data_once(self):
        data = bytes((i * 7) % 251 for i in range(130_000))
        f = self.local("AVRIL14.A", data)
        with PutUnit() as u:
            r = run_tool(["put", f, "/CHIMERA/PROJECTS/P0000014.A"], u.target)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(u.data, data)
        self.assertEqual(u.requests[0], b"put /CHIMERA/PROJECTS/P0000014.A 130000 %08x" % zlib.crc32(data))
        self.assertEqual(sum(1 for x in u.requests if x.startswith(b"put ")), 1)
        self.assertEqual(r.stdout, b"/CHIMERA/PROJECTS/P0000014.A 130000\n")
        self.assertIn(b"100% 130000 of 130000 bytes", r.stderr)

    def test_a_remote_directory_takes_the_local_name(self):
        f = self.local("avril14.a", b"abc")
        with PutUnit() as u:
            r = run_tool(["put", f, "/CHIMERA/PROJECTS/"], u.target)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertTrue(u.requests[0].startswith(b"put /CHIMERA/PROJECTS/AVRIL14.A 3 "))

    def test_an_empty_file_puts(self):
        f = self.local("E.A", b"")
        with PutUnit() as u:
            r = run_tool(["put", f, "/CHIMERA/PROJECTS/E.A"], u.target)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(u.requests, [b"put /CHIMERA/PROJECTS/E.A 0 00000000"])

    def test_a_refusal_before_ready_sends_no_data(self):
        f = self.local("S.A", b"x" * 1000)
        refusal = b"ERR put: would leave under 1 MiB free\n"
        with PutUnit(refuse=refusal) as u:
            r = run_tool(["put", f, "/CHIMERA/SOUNDS/S.A"], u.target)
        self.assertEqual(r.returncode, 1)
        self.assertIn(b"ERR put: would leave under 1 MiB free", r.stderr)
        self.assertEqual(len(u.requests), 1, "no data, no retry")

    def test_an_err_after_the_data_fails_without_retry(self):
        f = self.local("S.A", b"y" * 5000)
        with PutUnit(after=b"ERR put: CARD TIMEOUT\n") as u:
            r = run_tool(["put", f, "/CHIMERA/SOUNDS/S.A"], u.target)
        self.assertEqual(r.returncode, 1)
        self.assertTrue(r.stderr.endswith(b"ERR put: CARD TIMEOUT\n"), r.stderr)
        self.assertEqual(sum(1 for x in u.requests if x.startswith(b"put ")), 1)

    def test_a_file_over_the_limit_is_refused_here(self):
        f = self.local("BIG.A", bytes(tool.PUT_MAX + 1))
        with PutUnit() as u:
            r = run_tool(["put", f, "/CHIMERA/SOUNDS/BIG.A"], u.target)
        self.assertEqual(r.returncode, 1)
        self.assertIn(b"put takes at most 1048576", r.stderr)
        self.assertEqual(u.requests, [])

    def test_a_missing_local_file_fails(self):
        r = run_tool(["put", str(pathlib.Path(self.tmp.name, "NOPE.A")), "/CHIMERA/SOUNDS/"], "tcp:127.0.0.1:9")
        self.assertEqual(r.returncode, 1)
        self.assertIn(b"put: ", r.stderr)

    def test_silence_after_the_data_is_no_answer(self):
        with PutUnit(silent=True) as u:
            conn = tool.Conn(tool.target({"CHIMERA_USB": u.target}))
            saved = tool.PUT_WAIT_S
            tool.PUT_WAIT_S = 0.3
            try:
                with self.assertRaises(tool.NoAnswer):
                    tool.put(conn, b"z" * 100, "/CHIMERA/SOUNDS/Z.A")
            finally:
                tool.PUT_WAIT_S = saved
                conn.close()

    def test_lines_carry_48_bytes_in_base64(self):
        data = os.urandom(1000)
        lines = list(tool.put_lines(data))
        self.assertTrue(all(len(x) <= 65 and x.endswith(b"\n") for x in lines))
        self.assertEqual(b"".join(base64.b64decode(x) for x in lines), data)

    def test_a_source_goes_in_chunks_each_ended_and_acked(self):
        data = bytes((i * 13) % 251 for i in range(600_000))
        f = self.local("0BADF00D.SRC", data)
        with PutUnit() as u:
            r = run_tool(["put", f, "/CHIMERA/TAPES/"], u.target)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(u.data, data)
        self.assertEqual(u.requests[0], b"put /CHIMERA/TAPES/0BADF00D.SRC 600000 %08x" % zlib.crc32(data))
        c = tool.PUT_CHUNK
        self.assertEqual(u.ends, [b"end %08x" % zlib.crc32(data[i : i + c]) for i in range(0, len(data), c)])
        self.assertEqual(len(u.ends), 3)
        self.assertEqual(r.stdout, b"/CHIMERA/TAPES/0BADF00D.SRC 600000\n")
        self.assertIn(b"100% 600000 of 600000 bytes", r.stderr)

    def test_a_project_never_takes_the_source_protocol(self):
        data = bytes(300_000)
        f = self.local("P.A", data)
        with PutUnit() as u:
            r = run_tool(["put", f, "/CHIMERA/PROJECTS/P0000001.A"], u.target)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(u.ends, [])
        self.assertFalse(any(x.startswith(b"end ") for x in u.requests))

    def test_a_source_on_the_card_answers_present_and_sends_nothing(self):
        f = self.local("0BADF00D.SRC", b"s" * 5000)
        with PutUnit(present=True) as u:
            r = run_tool(["put", f, "/CHIMERA/TAPES/"], u.target)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(r.stdout, b"/CHIMERA/TAPES/0BADF00D.SRC 5000 PRESENT\n")
        self.assertEqual(len(u.requests), 1, "no data")

    def test_a_refused_chunk_stops_the_put_without_retry(self):
        data = bytes(600_000)
        f = self.local("0BADF00D.SRC", data)
        with PutUnit(bad_chunk=2) as u:
            r = run_tool(["put", f, "/CHIMERA/TAPES/"], u.target)
        self.assertEqual(r.returncode, 1)
        self.assertTrue(r.stderr.endswith(b"ERR put: chunk\n"), r.stderr)
        self.assertEqual(len(u.ends), 2, "no third chunk")
        self.assertEqual(sum(1 for x in u.requests if x.startswith(b"put ")), 1)

    def test_ls_prints_the_listing_and_waits_for_the_card(self):
        self.assertEqual(tool.first_wait("ls"), tool.LS_WAIT_S)
        self.assertEqual(tool.first_wait("sweep"), tool.SWEEP_WAIT_S)
        self.assertEqual(tool.first_wait("rm /CHIMERA/SOUNDS/S0000001"), tool.LS_WAIT_S)
        self.assertEqual(tool.first_wait("get /chimera/sounds/s0000001"), tool.LS_WAIT_S)
        self.assertGreaterEqual(tool.SWEEP_WAIT_S, 1.5 + 10)
        self.assertEqual(tool.first_wait("status"), tool.SILENCE_S)
        with PutUnit(listing=b"P0000014.A 130000\nOK\n") as u:
            r = run_tool(["ls"], u.target)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(r.stdout, b"P0000014.A 130000\n")
        self.assertEqual(u.requests, [b"ls"])

    def test_busy_is_retried(self):
        f = self.local("S.A", b"q" * 300)
        with PutUnit(busy=2) as u:
            start = time.monotonic()
            r = run_tool(["put", f, "/CHIMERA/SOUNDS/S0000003.A"], u.target)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(sum(1 for x in u.requests if x.startswith(b"put ")), 3)
        self.assertEqual(u.data, b"q" * 300)
        self.assertGreaterEqual(time.monotonic() - start, 0.2 + 0.4, "200 ms, doubling")

    def test_busy_gives_up_after_five_tries(self):
        f = self.local("S.A", b"q" * 30)
        saved = tool.BUSY_WAIT_S
        tool.BUSY_WAIT_S = 0.01
        try:
            with PutUnit(busy=9) as u:
                conn = tool.Conn(tool.target({"CHIMERA_USB": u.target}))
                try:
                    status, body = tool.retry_busy(lambda: tool.put(conn, b"q" * 30, "/CHIMERA/SOUNDS/S0000003.A"))
                finally:
                    conn.close()
        finally:
            tool.BUSY_WAIT_S = saved
        self.assertIs(status, tool.Status.ERR)
        self.assertEqual(body, b"ERR busy")
        self.assertEqual(sum(1 for x in u.requests if x.startswith(b"put ")), tool.BUSY_TRIES)
        self.assertEqual(tool.BUSY_TRIES, 5)

    def test_a_request_is_retried_while_busy(self):
        replies = [b"ERR busy\n", b"S0000001.A 120\nOK\n"]

        class Next:
            def __contains__(self, _):
                return bool(replies)

            def __getitem__(self, _):
                return replies[0]

        with FakeUnit(Next(), on_answer=lambda: replies.pop(0)) as u:
            r = run_tool(["ls", "sounds"], u.target)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(r.stdout, b"S0000001.A 120\n")
        self.assertEqual(u.requests, [b"ls sounds\n"] * 2)

    def test_a_sound_takes_the_top_user_id_plus_one(self):
        f = self.local("bass.snd", b"s" * 200)
        listing = b"S0000002.A 200\nS0000007.B 200\nS0000007.A 200\nS9000001.A 200\nREADME 2\nOK\n"
        with PutUnit(listing=listing) as u:
            r = run_tool(["put", f, "/chimera/sounds/"], u.target)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(u.requests[0], b"ls sounds")
        self.assertTrue(u.requests[1].startswith(b"put /CHIMERA/SOUNDS/S0000008.A 200 "), u.requests)
        self.assertEqual(r.stdout, b"/CHIMERA/SOUNDS/S0000008.A 200\n")

    def test_a_sound_onto_an_empty_card_is_the_first_id(self):
        f = self.local("bass.snd", b"s" * 20)
        with PutUnit(listing=b"ERR ls: FILE NOT FOUND\n") as u:
            r = run_tool(["put", f, "/CHIMERA/SOUNDS/"], u.target)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertTrue(u.requests[1].startswith(b"put /CHIMERA/SOUNDS/S0000001.A 20 "), u.requests)




SEAL = bytes(range(1, 9))
CONSOLE = {(0x0483, 0x5740)}


class Bank2(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.bk2 = bytearray(b"CHIMERA2" + bytes(16) + SEAL + bytes(224) + b"body" * 100)

    def local(self, name="chimera.bk2", data=None):
        p = pathlib.Path(self.tmp.name, name)
        p.write_bytes(self.bk2 if data is None else data)
        return str(p)

    def run_bank2(self, unit, name="chimera.bk2"):
        with FakeSysfs(CONSOLE) as fs:
            return run_tool(["bank2", self.local(name)], unit.target, sysfs=fs.root)

    def status(self, seal=SEAL, state="open"):
        return {b"bank2": b"bank2 %s 0.2.0 02e440e2 seal=%s\nOK\n" % (state.encode(), seal.hex().encode())}

    def test_refuses_a_name_not_ending_bk2(self):
        with PutUnit() as u:
            r = self.run_bank2(u, "chimera.bin")
        self.assertEqual(r.returncode, 1)
        self.assertIn(b".bk2", r.stderr)
        self.assertEqual(u.requests, [])

    def test_an_open_matching_seal_sends_no_put(self):
        with PutUnit(answers=self.status()) as u:
            r = self.run_bank2(u)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(r.stdout, b"OK already installed\n")
        self.assertEqual(u.requests, [b"bank2"])

    def test_puts_then_installs_and_exits_0_on_open(self):
        answers = self.status(bytes(8), "empty")
        answers[b"bank2 install"] = b"audio overruns 0 -> 0 blocks +40\nOK bank2 open 0.2.0 02e440e2\n"
        with PutUnit(answers=answers) as u:
            r = self.run_bank2(u)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(u.requests[0], b"bank2")
        self.assertTrue(u.requests[1].startswith(b"put /CHIMERA/SYSTEM/BANK2.BK2 %d " % len(self.bk2)))
        self.assertEqual(u.requests[-1], b"bank2 install")
        self.assertEqual(u.data, bytes(self.bk2))
        self.assertEqual(r.stdout, b"audio overruns 0 -> 0 blocks +40\nOK bank2 open 0.2.0 02e440e2\n")

    def test_a_seal_that_differs_installs(self):
        answers = self.status(bytes(range(9, 17)))
        answers[b"bank2 install"] = b"OK bank2 open v\n"
        with PutUnit(answers=answers) as u:
            r = self.run_bank2(u)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertIn(b"bank2 install", u.requests)

    def test_a_file_for_another_firmware_exits_2_at_put(self):
        with PutUnit(answers=self.status(bytes(8), "empty"), after=b"OK staged: needs firmware 0.3.0\n") as u:
            r = self.run_bank2(u)
        self.assertEqual(r.returncode, 2, r.stderr)
        self.assertIn(b"staged: needs firmware 0.3.0", r.stderr)
        self.assertNotIn(b"bank2 install", u.requests)

    def test_a_file_for_another_firmware_exits_2_at_install(self):
        answers = self.status(bytes(8), "empty")
        answers[b"bank2 install"] = b"ERR bank2: staged: needs firmware 0.3.0\n"
        with PutUnit(answers=answers) as u:
            r = self.run_bank2(u)
        self.assertEqual(r.returncode, 2, r.stderr)
        self.assertIn(b"staged: needs firmware 0.3.0", r.stderr)

    def test_an_install_err_exits_1(self):
        answers = self.status(bytes(8), "empty")
        answers[b"bank2 install"] = b"audio overruns 0 -> 0 blocks +40\nERR bank2: flash timeout at S0\n"
        with PutUnit(answers=answers) as u:
            r = self.run_bank2(u)
        self.assertEqual(r.returncode, 1)
        self.assertIn(b"ERR bank2: flash timeout at S0", r.stderr)

    def test_erase_sends_it_alone(self):
        with PutUnit(answers={b"bank2 erase": b"OK bank2 erased\n"}) as u:
            r = run_tool(["bank2", "erase"], u.target)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(r.stdout, b"OK bank2 erased\n")
        self.assertEqual(u.requests, [b"bank2 erase"])

    def test_an_erase_err_exits_1(self):
        with PutUnit(answers={b"bank2 erase": b"ERR bank2: flash timeout\n"}) as u:
            r = run_tool(["bank2", "erase"], u.target)
        self.assertEqual(r.returncode, 1)
        self.assertIn(b"ERR bank2: flash timeout", r.stderr)

    def test_the_install_answer_waits_a_minute(self):
        self.assertEqual(tool.BANK2_WAIT_S, 60.0)

    def test_a_missing_console_fails(self):
        with PutUnit() as u, FakeSysfs(set()) as fs:
            r = run_tool(["bank2", self.local()], u.target, sysfs=fs.root, env={"CHIMERA_DFU_WAIT": "0.2"})
        self.assertEqual(r.returncode, 1)
        self.assertEqual(u.requests, [])

    def test_a_plain_put_of_a_stale_bk2_prints_the_line(self):
        f = self.local("old.bk2")
        with PutUnit(after=b"OK staged: needs firmware 0.3.0\n") as u:
            r = run_tool(["put", f, "/CHIMERA/SYSTEM/BANK2.BK2"], u.target)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertIn(b"staged: needs firmware 0.3.0", r.stdout)

    def test_dfu_util_has_a_timeout(self):
        seen = {}

        def run(cmd, **kw):
            seen.update(kw)
            return subprocess.CompletedProcess(cmd, 0, "", "")

        saved = tool.to_dfu, tool.wait_console, tool.subprocess.run
        tool.to_dfu, tool.wait_console, tool.subprocess.run = (lambda t, e: 0), (lambda e, s: 0), run
        try:
            tool.dfu_flash(None, {}, self.local("x.bin", b"fw"), 10.0)
        finally:
            tool.to_dfu, tool.wait_console, tool.subprocess.run = saved
        self.assertEqual(seen.get("timeout"), tool.DFU_UTIL_S)

    def test_dfu_runs_dfu_util_on_the_firmware_area(self):
        argv = []

        def run(cmd, **kw):
            argv.extend(cmd)
            return subprocess.CompletedProcess(cmd, 74, "File downloaded successfully\n", "")

        saved = tool.to_dfu, tool.wait_console, tool.subprocess.run
        tool.to_dfu, tool.wait_console, tool.subprocess.run = (lambda t, e: 0), (lambda e, s: 0), run
        try:
            f = self.local("x.bin", b"fw")
            self.assertEqual(tool.dfu_flash(None, {}, f, 10.0), 0)
        finally:
            tool.to_dfu, tool.wait_console, tool.subprocess.run = saved
        self.assertEqual(argv[0], "dfu-util")
        self.assertEqual(argv[-2:], ["-s", "0x8020000:leave"])

    def test_the_public_tool_names_nothing_private(self):
        leak = re.compile(r"/home/|/Users/|https?://|ADR|docs/|chimera-(core|board|engine|desktop|stm32)|\.rs\b|joegiralt")
        self.assertIsNone(leak.search(TOOL.read_text()))


VERSION = b"0.2.0 02e440e2"
OLD_PUT = (
    b"ERR put takes /CHIMERA/PROJECTS/P#######.A, /CHIMERA/SOUNDS/S#######.A [--over] "
    b"or /CHIMERA/TAPES/<CRC>.SRC, its size and CRC-32 in hex\n"
)


def make_pair(body=b"door" * 64, version=VERSION):
    """A .bin and its .bk2 as `bank2-split` makes them: the .bk2's header
    seals its body (SHA-256 prefix, CRC-32), the .bin holds the seal once."""
    seal = hashlib.sha256(body).digest()[:8]
    h = bytearray(256)
    h[0:8] = b"CHIMERA2"
    h[8:10] = (1).to_bytes(2, "little")
    h[0x0C:0x10] = len(body).to_bytes(4, "little")
    h[0x10:0x14] = zlib.crc32(body).to_bytes(4, "little")
    h[0x14:0x18] = (0x100).to_bytes(4, "little")
    h[0x18:0x20] = seal
    h[0x20 : 0x20 + len(version)] = version
    h[0xE0:0x100] = (0xB2C0FFEE).to_bytes(4, "little") * 8
    return b"\x7fFW" + seal + b"rest of bank 1" * 50, bytes(h) + body, seal


class Update(unittest.TestCase):
    """`update <file.bin> <file.bk2>`: check the pair, stage the .bk2 under
    the running firmware, flash the .bin, let the boot install, read status."""

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.bin, self.bk2, self.seal = make_pair()
        self.events = []
        self.waits = []
        saved = tool.to_dfu, tool.subprocess.run, tool.wait_console, tool.send
        self.addCleanup(lambda: setattr(tool, "to_dfu", saved[0]) or setattr(tool.subprocess, "run", saved[1]) or setattr(tool, "wait_console", saved[2]) or setattr(tool, "send", saved[3]))
        send = tool.send

        def record_send(conn, line):
            self.events.append(line if isinstance(line, str) else line.decode())
            return send(conn, line)

        def run(cmd, **kw):
            self.events.append("dfu-util")
            return subprocess.CompletedProcess(cmd, 74, "File downloaded successfully\n", "")

        def wait(env, secs):
            self.waits.append(secs)
            return 0

        tool.send = record_send
        tool.to_dfu = lambda t, e: self.events.append("to-dfu") or 0
        tool.subprocess.run = run
        tool.wait_console = wait

    def package(self, bin_data=None, bk2=None, name="chimera-0.2.0"):
        b, k = pathlib.Path(self.tmp.name, name + ".bin"), pathlib.Path(self.tmp.name, name + ".bk2")
        b.write_bytes(self.bin if bin_data is None else bin_data)
        k.write_bytes(self.bk2 if bk2 is None else bk2)
        return [str(b), str(k)]

    def status(self, line, build=VERSION):
        return b"firmware 0.2.0 release\nbuild " + build + b"\nprotocol 1\n" + line + b"\nOK\n"

    def open_line(self, seal=None):
        return b"bank2 open " + VERSION + b" seal=" + (seal or self.seal).hex().encode()

    def update(self, unit, args):
        out = io.TextIOWrapper(io.BytesIO(), encoding="utf-8")
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(io.StringIO()) as err:
            rc = tool.main(["update", *args], {"CHIMERA_USB": unit.target})
        out.flush()
        return rc, out.buffer.getvalue().decode(), err.getvalue()

    def sent(self, prefix):
        return [i for i, e in enumerate(self.events) if e.startswith(prefix)]

    def test_stages_then_flashes_then_reads_status(self):
        answers = {b"status": self.status(self.open_line())}
        with PutUnit(answers=answers, after=b"OK staged: needs firmware 0.2.0 02e440e2\n") as u:
            rc, out, err = self.update(u, self.package())
        self.assertEqual(rc, 0, err)
        self.assertIn("OK updated 0.2.0 02e440e2", out)
        put, dfu, status = self.sent("put /CHIMERA/SYSTEM/BANK2.BK2 "), self.sent("dfu-util"), self.sent("status")
        self.assertEqual((len(put), len(dfu), len(status)), (1, 1, 1), self.events)
        self.assertLess(put[0], dfu[0])
        self.assertLess(dfu[0], status[0])
        self.assertEqual(u.data, self.bk2)
        self.assertEqual(self.sent("bank2"), [], "the boot installs it, not the console")
        self.assertGreaterEqual(max(self.waits), 60.0, "the first boot installs before the console comes up")

    def test_a_plain_ok_stages_too(self):
        with PutUnit(answers={b"status": self.status(self.open_line())}) as u:
            rc, out, err = self.update(u, self.package())
        self.assertEqual(rc, 0, err)
        self.assertEqual(len(self.sent("dfu-util")), 1)

    def test_a_staging_failure_stops_before_any_flash(self):
        for refusal in (b"ERR put: CARD FULL\n", b"ERR put: firmware unsealed\n", b"ERR put: not a bank-2 image\n"):
            self.events.clear()
            with PutUnit(refuse=refusal) as u:
                rc, out, err = self.update(u, self.package())
            self.assertEqual(rc, 1, refusal)
            self.assertIn(refusal.strip().decode(), err)
            self.assertEqual(self.sent("to-dfu") + self.sent("dfu-util"), [], refusal)

    def test_old_firmware_flashes_first_then_puts_and_installs(self):
        answers = {
            b"bank2": b"bank2 empty\nOK\n",
            b"bank2 install": b"audio overruns 0 -> 0 blocks +40\nOK bank2 open 0.2.0 02e440e2\n",
            b"status": self.status(self.open_line()),
        }
        with PutUnit(answers=answers, refuse=OLD_PUT, refuse_once=True) as u:
            rc, out, err = self.update(u, self.package())
        self.assertEqual(rc, 0, err)
        puts, dfu = self.sent("put "), self.sent("dfu-util")
        install, status = self.sent("bank2 install"), self.sent("status")
        self.assertEqual(len(puts), 2, self.events)
        self.assertLess(puts[0], dfu[0])
        self.assertLess(dfu[0], puts[1])
        self.assertLess(puts[1], install[0])
        self.assertLess(install[0], status[0])
        self.assertEqual(u.data, self.bk2)

    def test_old_firmware_without_source_puts_is_deferred_too(self):
        old = b"ERR put takes /CHIMERA/PROJECTS/P#######.A or /CHIMERA/SOUNDS/S#######.A [--over], its size and CRC-32 in hex\n"
        answers = {b"bank2": b"bank2 empty\nOK\n", b"bank2 install": b"OK bank2 open 0.2.0 02e440e2\n", b"status": self.status(self.open_line())}
        with PutUnit(answers=answers, refuse=old, refuse_once=True) as u:
            rc, out, err = self.update(u, self.package())
        self.assertEqual(rc, 0, err)

    def test_a_bad_pair_touches_nothing(self):
        _, other_bk2, _ = make_pair(b"another build" * 20)
        cases = {
            "bk2 posing as bin": (self.bk2, None),
            "too big for the bootloader": (self.bin + bytes(917_504), None),
            "pair from two builds": (None, other_bk2),
            "corrupt bk2 body": (None, self.bk2[:-1] + b"X"),
            "not a bk2": (None, b"PK no"),
        }
        for what, (bin_data, bk2) in cases.items():
            self.events.clear()
            with PutUnit() as u:
                rc, out, err = self.update(u, self.package(bin_data, bk2))
            self.assertEqual(rc, 1, what)
            self.assertEqual(self.events, [], what)
            self.assertEqual(u.requests, [], what)
            self.assertEqual(self.waits, [], what)
        with PutUnit() as u:
            rc, out, err = self.update(u, self.package()[:1])
        self.assertEqual((rc, u.requests), (1, []))

    def test_exit_1_unless_status_is_open_with_the_package_seal(self):
        for line in (b"bank2 stale have 0.1.0", self.open_line(bytes(8)), b"bank2 empty", b"bank2 torn"):
            self.events.clear()
            with PutUnit(answers={b"status": self.status(line)}) as u:
                rc, out, err = self.update(u, self.package())
            self.assertEqual(rc, 1, line)
            self.assertIn(line.decode(), out + err)
            self.assertNotIn("OK updated", out)

    def test_an_unchanged_bk2_keeps_its_older_version(self):
        # Same seal, installed by an earlier release: nothing to install.
        older = b"bank2 open 0.1.0 11111111 seal=" + self.seal.hex().encode()
        with PutUnit(answers={b"status": self.status(older)}) as u:
            rc, out, err = self.update(u, self.package())
        self.assertEqual(rc, 0, err)
        self.assertIn("OK updated", out)

    def test_exit_1_unless_the_new_firmware_runs(self):
        for build in (b"0.1.0 11111111", b"", b"0.2.0"):
            with PutUnit(answers={b"status": self.status(self.open_line(), build)}) as u:
                rc, out, err = self.update(u, self.package())
            self.assertEqual(rc, 1, build)
            self.assertIn("isn't the one running", err)
        # A dirty build's `+` is the same build.
        with PutUnit(answers={b"status": self.status(self.open_line(), VERSION + b"+")}) as u:
            rc, out, err = self.update(u, self.package())
        self.assertEqual(rc, 0, err)

    def test_the_pair_in_the_wrong_order_is_refused(self):
        b, k = self.package()
        with PutUnit() as u:
            rc, out, err = self.update(u, [k, b])
        self.assertEqual((rc, u.requests), (1, []))


if __name__ == "__main__":
    unittest.main()
