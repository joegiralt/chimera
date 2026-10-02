"""Host tests for chimera-usb.py against a fake unit (standard library only).

Run: python3 -m unittest discover -s tools -p 'test_*.py'
"""

import importlib.util
import os
import pathlib
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
        import datetime

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
        self.assertIn(b"70-chimera.rules /etc/udev/rules.d/", r.stderr)

    def test_udev_rule_matches_the_firmware_identity(self):
        rule = (ROOT / "tools/70-chimera.rules").read_text()
        usb = (ROOT / "chimera-stm32/src/usb.rs").read_text()
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

    def test_to_dfu_without_a_console_prints_the_jumper(self):
        with FakeSysfs(set()) as fs:
            r = run_tool(["to-dfu"], "tcp:127.0.0.1:9", sysfs=fs.root)
        self.assertEqual(r.returncode, 0)
        self.assertIn(b"bridge BOOT0", r.stdout + r.stderr)

    def test_to_dfu_matches_the_firmware_identity(self):
        self.assertEqual(tool.CONSOLE_ID, (0x0483, 0x5740))
        self.assertEqual(tool.ROM_DFU_ID, (0x0483, 0xDF11))
        just = (ROOT / "Justfile").read_text()
        for recipe in ("flash:", "flash-bench:"):
            body = just.split("\n" + recipe, 1)[1].split("\n\n", 1)[0]
            self.assertLess(body.index("to-dfu"), body.index("dfu-util"), recipe)
            self.assertIn("-s 0x8020000:leave", body, recipe)


if __name__ == "__main__":
    unittest.main()
