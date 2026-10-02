#!/usr/bin/env python3
"""Read the unit over its USB console (or the desktop sim's socket).

    python3 tools/chimera-usb.py <cmd> [<arg>]     help, status, stats, bench, shot [raw], dfu
    python3 tools/chimera-usb.py to-dfu            the unit into ROM DFU, for just flash
    python3 tools/chimera-usb.py wait-console [s]  wait (default 10 s) for the console on the bus

Exit 0 on OK, 1 on ERR (to stderr) or 2 s of silence. Bodies go to stdout;
`shot` writes a PNG and prints its path. Standard library only.

Environment: CHIMERA_USB picks the target (unset: /dev/chimera if present,
else /dev/ttyACM0; `sim`: the desktop sim; `tcp:HOST:PORT`; else a device
path). CHIMERA_SHOTS overrides target/shots. CHIMERA_SYSFS and
CHIMERA_DFU_WAIT (s, default 10) are for to-dfu and wait-console.
"""

import datetime
import enum
import errno
import os
import pathlib
import select
import socket
import struct
import subprocess
import sys
import time
import tty
import zlib
from typing import NamedTuple, Union

SIM = ("127.0.0.1", 7341)  # chimera-desktop/src/console.rs ADDR
SILENCE_S = 2.0
DRAIN_S = 0.05
MAX_SHOT = 1 << 20  # well over 240 x 320 x 2; a garbled header can't ask for gigabytes
UDEV_HINT = "sudo cp tools/70-chimera.rules /etc/udev/rules.d/ && sudo udevadm control --reload && sudo udevadm trigger"


class UsbId(NamedTuple):
    vid: int
    pid: int


CONSOLE_ID = UsbId(0x0483, 0x5740)  # chimera-stm32/src/usb.rs VID_PID
ROM_DFU_ID = UsbId(0x0483, 0xDF11)  # ST's system memory loader


class Tcp(NamedTuple):
    kind: str
    host: str
    port: int


class Tty(NamedTuple):
    kind: str
    path: str


Target = Union[Tcp, Tty]


def target(env) -> Target:
    v = env.get("CHIMERA_USB")
    if v is None:
        return Tty("tty", "/dev/chimera" if os.path.exists("/dev/chimera") else "/dev/ttyACM0")
    if v == "sim":
        return Tcp("tcp", *SIM)
    if v.startswith("tcp:"):
        host, port = v[4:].rsplit(":", 1)
        return Tcp("tcp", host, int(port))
    return Tty("tty", v)


def describe(t: Target) -> str:
    return "%s:%d" % (t.host, t.port) if isinstance(t, Tcp) else t.path


class Status(enum.Enum):
    OK = "OK"
    ERR = "ERR"


class NoAnswer(Exception):
    """2 s passed with no byte from the unit."""


class Vanished(Exception):
    """The port closed under us: the unit reset or was unplugged."""


class Conn:
    """A raw byte stream to the unit: a tty in raw mode or a socket."""

    def __init__(self, t: Target):
        self.sock = None
        if isinstance(t, Tcp):
            self.sock = socket.create_connection((t.host, t.port), timeout=SILENCE_S)
            self.sock.settimeout(None)  # blocking; reads wait in select
            self.fd = self.sock.fileno()
        else:
            self.fd = os.open(t.path, os.O_RDWR | os.O_NOCTTY)
            # Raw: the tty's echo would send the unit's answers back as requests.
            tty.setraw(self.fd)

    def read(self, timeout) -> bytes:
        """Some bytes, or b"" after `timeout` s of silence."""
        if not select.select([self.fd], [], [], timeout)[0]:
            return b""
        try:
            data = os.read(self.fd, 65536)
        except OSError as e:  # EIO: the ACM device went away
            raise Vanished() from e
        if not data:
            raise Vanished()
        return data

    def write(self, data: bytes):
        try:
            while data:
                data = data[os.write(self.fd, data) :]
        except OSError as e:
            raise Vanished() from e

    def close(self):
        if self.sock:
            self.sock.close()
        else:
            os.close(self.fd)


class Reader:
    """Lines and exact byte counts over a Conn, failing after 2 s of silence."""

    def __init__(self, conn: Conn):
        self.conn, self.buf = conn, b""

    def _more(self):
        data = self.conn.read(SILENCE_S)
        if not data:
            raise NoAnswer()
        self.buf += data

    def line(self) -> bytes:
        while b"\n" not in self.buf:
            self._more()
        line, self.buf = self.buf.split(b"\n", 1)
        return line.rstrip(b"\r")

    def exactly(self, n) -> bytes:
        while len(self.buf) < n:
            self._more()
        out, self.buf = self.buf[:n], self.buf[n:]
        return out


def drain(conn: Conn):
    """Drop a stalled earlier answer's tail: read until 50 ms of quiet."""
    deadline = time.monotonic() + SILENCE_S
    while conn.read(DRAIN_S) and time.monotonic() < deadline:
        pass


def send(conn: Conn, line):
    """Drain a stalled tail, then send one request line. Raises Vanished."""
    drain(conn)
    conn.write(line.encode() + b"\n" if isinstance(line, str) else line + b"\n")


def answer(conn: Conn) -> tuple:
    """(Status.OK, body) or (Status.ERR, the ERR line).

    A `SHOT` header line stays in the body, followed by exactly its length
    of pixel bytes. Raises NoAnswer, Vanished or BadShot."""
    rd, body = Reader(conn), b""
    while True:
        ln = rd.line()
        if ln == b"OK":
            return Status.OK, body
        if ln == b"ERR" or ln.startswith(b"ERR "):
            return Status.ERR, ln
        body += ln + b"\n"
        if ln.startswith(b"SHOT "):
            body += rd.exactly(Shot.declared_length(ln))


def request(conn: Conn, line) -> tuple:
    """`send`, then `answer`."""
    send(conn, line)
    return answer(conn)


class BadShot(Exception):
    pass


class Shot(NamedTuple):
    """A screen as the unit sent it: big-endian RGB565, row by row."""

    width: int
    height: int
    pixels: bytes

    @staticmethod
    def declared_length(header: bytes) -> int:
        f = header.split()
        if len(f) != 5 or f[3] != b"rgb565be" or not all(x.isdigit() for x in f[1:3] + f[4:]):
            raise BadShot("bad shot header: %r" % header)
        if int(f[4]) > MAX_SHOT:
            raise BadShot("shot: %s bytes is not a screen" % f[4].decode())
        return int(f[4])

    @classmethod
    def parse(cls, body: bytes) -> "Shot":
        header, pixels = body.split(b"\n", 1)
        n = cls.declared_length(header)
        w, h = int(header.split()[1]), int(header.split()[2])
        if w * h == 0 or w * h * 2 != n or len(pixels) != n:
            raise BadShot("shot: %dx%d needs %d bytes, header says %d" % (w, h, w * h * 2, n))
        return cls(w, h, pixels)


def _widen_lut(scale):
    """Each RGB565 value as `scale` RGB888 pixels; bit replication keeps white 255."""
    out = []
    for v in range(65536):
        r, g, b = v >> 11, (v >> 5) & 0x3F, v & 0x1F
        out.append(bytes((r << 3 | r >> 2, g << 2 | g >> 4, b << 3 | b >> 2)) * scale)
    return out


def _chunk(kind: bytes, data: bytes) -> bytes:
    return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))


def rgb565be_to_png(body: bytes, w: int, h: int, scale=2) -> bytes:
    """An 8-bit RGB PNG of the pixels at `scale`x nearest neighbour."""
    lut = _widen_lut(scale)
    vals = struct.unpack(">%dH" % (w * h), body)
    rows = []
    for y in range(h):
        row = b"\x00" + b"".join(lut[v] for v in vals[y * w : (y + 1) * w])
        rows.extend([row] * scale)
    ihdr = struct.pack(">IIBBBBB", w * scale, h * scale, 8, 2, 0, 0, 0)
    return b"\x89PNG\r\n\x1a\n" + _chunk(b"IHDR", ihdr) + _chunk(b"IDAT", zlib.compress(b"".join(rows), 6)) + _chunk(b"IEND", b"")


def repo_root() -> pathlib.Path:
    here = pathlib.Path(__file__).resolve().parent
    try:
        out = subprocess.run(["git", "rev-parse", "--show-toplevel"], cwd=here, capture_output=True, text=True)
        if out.returncode == 0:
            return pathlib.Path(out.stdout.strip())
    except OSError:
        pass
    return here.parent


def shot_dir() -> pathlib.Path:
    d = os.environ.get("CHIMERA_SHOTS")
    return pathlib.Path(d) if d else repo_root() / "target/shots"


def shot_path(raw: bool, now) -> pathlib.Path:
    return shot_dir() / ("shot-%s%s.png" % ("raw-" if raw else "", now.strftime("%Y%m%d-%H%M%S")))


def write_new(path: pathlib.Path, data: bytes) -> pathlib.Path:
    """Write `data` to `path`, or `<stem>-2…` if two shots share a second."""
    path.parent.mkdir(parents=True, exist_ok=True)
    stem, n = path.stem, 1
    while True:
        try:
            with open(path, "xb") as f:
                f.write(data)
            return path
        except FileExistsError:
            n += 1
            path = path.with_name("%s-%d.png" % (stem, n))


def usb_devices(sysfs) -> set:
    """The (vid, pid) of every USB device the kernel lists."""
    ids = set()
    for d in pathlib.Path(sysfs, "bus/usb/devices").glob("*"):
        try:
            ids.add(UsbId(int((d / "idVendor").read_text(), 16), int((d / "idProduct").read_text(), 16)))
        except (OSError, ValueError):
            pass  # interfaces and hubs without ids, or a device leaving mid-read
    return ids


def fail(msg) -> int:
    print(msg, file=sys.stderr)
    return 1


def open_conn(t: Target) -> Conn:
    """A Conn, or a one-line reason on stderr and SystemExit(1)."""
    try:
        return Conn(t)
    except PermissionError:
        sys.exit(fail("no access to %s: install the udev rule: %s" % (describe(t), UDEV_HINT)))
    except OSError as e:
        sys.exit(fail("no console at %s: %s" % (describe(t), e.strerror or e)))


def to_dfu(t: Target, env) -> int:
    sysfs = env.get("CHIMERA_SYSFS", "/sys")
    ids = usb_devices(sysfs)
    if ROM_DFU_ID in ids:
        return 0
    if CONSOLE_ID not in ids:
        print("no console: bridge BOOT0 on the back and re-plug for DFU")
        return 0
    conn = open_conn(t)  # the kernel lists it, so failing to open it is the fault
    try:
        try:
            send(conn, "dfu")
        except Vanished:
            return fail("%s went away before dfu was sent" % describe(t))
        try:
            status, body = answer(conn)
            if status is Status.ERR:
                return fail(body.decode(errors="replace"))
        except NoAnswer:
            return fail("no answer from %s" % describe(t))
        except Vanished:
            pass  # dfu was sent: the OK was lost to the reset, not refused; DF11 decides
    finally:
        conn.close()
    deadline = time.monotonic() + float(env.get("CHIMERA_DFU_WAIT", "10"))
    while time.monotonic() < deadline:
        if ROM_DFU_ID in usb_devices(sysfs):
            return 0
        time.sleep(0.1)
    return fail("no DFU device after dfu: see U10 (marker clobbered?)")


def wait_console(env, secs) -> int:
    """Exit 0 once the console is on the bus again, 1 after `secs`."""
    sysfs = env.get("CHIMERA_SYSFS", "/sys")
    deadline = time.monotonic() + secs
    while CONSOLE_ID not in usb_devices(sysfs):
        if time.monotonic() >= deadline:
            return fail("console not back %g s after the flash" % secs)
        time.sleep(0.1)
    return 0


def main(argv, env) -> int:
    if not argv or argv[0] in ("-h", "--help"):
        print(__doc__.strip(), file=sys.stderr)
        return 2
    t = target(env)
    if argv[0] == "to-dfu":
        return to_dfu(t, env)
    if argv[0] == "wait-console":
        return wait_console(env, float(argv[1]) if len(argv) > 1 else 10.0)
    conn = open_conn(t)
    try:
        status, body = request(conn, " ".join(argv))
    except NoAnswer:
        return fail("no answer from %s" % describe(t))
    except Vanished:
        return fail("%s went away" % describe(t))
    except BadShot as e:
        return fail(str(e))
    finally:
        conn.close()
    if status is Status.ERR:
        return fail(body.decode(errors="replace"))
    if argv[0].lower() == "shot":
        try:
            shot = Shot.parse(body)
        except BadShot as e:
            return fail(str(e))
        raw = len(argv) > 1 and argv[1].lower() == "raw"
        path = write_new(shot_path(raw, datetime.datetime.now()), rgb565be_to_png(shot.pixels, shot.width, shot.height))
        print(path)
        return 0
    sys.stdout.buffer.write(body)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:], os.environ))
