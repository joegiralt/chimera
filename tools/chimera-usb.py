#!/usr/bin/env python3
"""Talk to your Chimera over USB: status for bug reports, screenshots, files onto the card.

    python3 chimera-usb.py <cmd> [<arg>]     help, status, stats, bench, card [clean|seed=N], shot [raw], dfu, sweep, flash,
                                                   ls [tapes|sounds], rm|get /CHIMERA/SOUNDS/S#######
    python3 chimera-usb.py put <local> <remote> [--over]
                                                   a project file onto the card as /CHIMERA/PROJECTS/P#######.A,
                                                   a library Sound as /CHIMERA/SOUNDS/S#######.A (--over replaces one there),
                                                   or a TAPE source as /CHIMERA/TAPES/<CRC>.SRC, in checked chunks
                                                   (<remote> ending in / takes the local name; /CHIMERA/SOUNDS/ takes
                                                   `ls sounds`' top user id + 1)
    python3 chimera-usb.py update <file.bin> <file.bk2>
                                                   flash a build's pair in one go (developers, `just flash`): the
                                                   .bk2 onto the card, the .bin by DFU, and the first boot
                                                   installs the .bk2
    python3 chimera-usb.py bank2 <file.bk2>  install a .bk2 under the running firmware (developers)
    python3 chimera-usb.py bank2 erase       empty bank 2 (a `just firmware-gate` build only)
    python3 chimera-usb.py flash             the flash controller's registers and bank 2's first bytes, read only
    python3 chimera-usb.py to-dfu            the unit into ROM DFU, to flash it
    python3 chimera-usb.py wait-console [s]  wait (default 10 s) for the console on the bus

Exit 0 on OK, 1 on ERR (to stderr) or 2 s of silence (60 s before `card`'s
first line: it measures before it answers; 60 s before `bank2`'s install; 15 s before `ls`'s, `rm`'s,
`get`'s and `sweep`'s, 35 s before `put`'s). Bodies go to stdout; `shot` writes a PNG and prints its
path; `put` shows its progress on stderr and never retries a failure (a source
already on the card answers `OK PRESENT`, so a re-run is the retry). `ERR busy`
(the unit's card work pending) is retried: 5 tries, 200 ms apart, doubling.
Python 3 and its standard library only; Linux and macOS (no Windows).
`update` checks the whole pair before it touches the unit, stops before
the flash if the .bk2 can't go onto the card, and exits 0 only once
`status` shows the pair's seal open (`OK updated <version>`);
it needs dfu-util. `bank2` exits 0 on OK (or `OK already installed`), 2 if
the file belongs to another firmware (`staged: needs firmware <version>`: it
installs at the next boot of that firmware), 1 on any other ERR.
`--version` prints this tool's release.

The port: CHIMERA_USB picks it. Unset, the first of /dev/chimera,
/dev/ttyACM0 (Linux) or /dev/cu.usbmodem* (macOS) that exists; else give a
device path, or `tcp:HOST:PORT`. On Linux, if it says "no access", either
add yourself to the dialout group (then log out and in again), or run the
`sudo tee` line it prints once. Shots are saved in the current directory;
CHIMERA_SHOTS picks another. CHIMERA_SYSFS and CHIMERA_DFU_WAIT (s,
default 10) are for to-dfu and wait-console.
"""

VERSION = "dev"  # `just release` stamps the release version here

import base64
import datetime
import enum
import errno
import glob
import hashlib
import os
import pathlib
import select
import socket
import struct
import subprocess
import sys
import tempfile
import time
import zlib
from typing import NamedTuple, Union

try:
    import tty
except ImportError:  # Windows
    sys.exit("chimera-usb.py needs Linux or macOS (Python's tty module is missing)")

SIM = ("127.0.0.1", 7341)  # the desktop simulator (developers)
SILENCE_S = 2.0
CARD_WAIT_S = 60.0  # `card` measures before it answers: ~5 s, ~15 s writing its test file
LS_WAIT_S = 15.0  # a cold card acquire, then one listing under the card's 10 s cap
SWEEP_WAIT_S = 15.0  # a cold acquire, then the mark's one write; the unit restarts after OK
PUT_WAIT_S = 35.0  # the unit's whole put is 30 s (PUT_DEADLINE_MS), card work included
BANK2_WAIT_S = 60.0  # the unit holds its UI loop for the whole write
LATER = " (it installs at that firmware's next boot)"
BANK2_REMOTE = "/CHIMERA/SYSTEM/BANK2.BK2"
BANK2_SEAL = slice(0x18, 0x20)  # the .bk2 header's seal
BK2_HEADER = 256
BK2_MAX = 512 * 1024  # bank 2's four sectors
UPDATE_BOOT_WAIT_S = 90.0  # the first boot installs the .bk2 behind DON'T POWER OFF, then the console comes up
# Firmware from before bank 2 refuses the path (its `put` usage, exactly): the
# .bk2 then goes after the flash.
OLD_PUT_REFUSALS = (
    b"ERR put takes /CHIMERA/PROJECTS/P#######.A, /CHIMERA/SOUNDS/S#######.A [--over] or /CHIMERA/TAPES/<CRC>.SRC, its size and CRC-32 in hex",
    b"ERR put takes /CHIMERA/PROJECTS/P#######.A or /CHIMERA/SOUNDS/S#######.A [--over], its size and CRC-32 in hex",
)
DFU_UTIL_S = 300  # a flash is ~30 s; this is only to not hang forever
FW_MAX = 917_504  # the stock bootloader erases itself at this size
FLASH_AT = "0x8020000:leave"  # the firmware area, never the bootloader
PUT_MAX = 1 << 20  # the unit's limit
PUT_LINE = 48  # bytes a data line carries: 64 base64 characters
PUT_CHUNK = 256 * 1024  # a TAPE source goes in chunks this big
TAPES = "/CHIMERA/TAPES/"
SOUNDS = "/CHIMERA/SOUNDS/"
USER_MAX = 8_999_999  # highest user Sound id
BUSY_TRIES = 5
BUSY_WAIT_S = 0.2  # doubling after each `ERR busy`
DRAIN_S = 0.05
MAX_SHOT = 1 << 20  # well over 240 x 320 x 2; a garbled header can't ask for gigabytes
UDEV_HINT = (
    "echo 'SUBSYSTEM==\"tty\", ATTRS{idVendor}==\"0483\", ATTRS{idProduct}==\"5740\", ENV{ID_MM_DEVICE_IGNORE}=\"1\", TAG+=\"uaccess\", SYMLINK+=\"chimera\"'"
    " | sudo tee /etc/udev/rules.d/70-chimera.rules && sudo udevadm control --reload && sudo udevadm trigger"
)


class UsbId(NamedTuple):
    vid: int
    pid: int


CONSOLE_ID = UsbId(0x0483, 0x5740)  # Chimera and the stock PreenFM3 share this USB id
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
        for p in ["/dev/chimera", "/dev/ttyACM0", *sorted(glob.glob("/dev/cu.usbmodem*"))]:
            if os.path.exists(p):
                return Tty("tty", p)
        return Tty("tty", "/dev/ttyACM0")
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
    """Lines and exact byte counts over a Conn, failing after `first` s of
    silence before the first byte and 2 s after."""

    def __init__(self, conn: Conn, first=SILENCE_S):
        self.conn, self.buf, self.wait = conn, b"", first

    def _more(self):
        data = self.conn.read(self.wait)
        if not data:
            raise NoAnswer()
        self.buf += data
        self.wait = SILENCE_S

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


class ErrLine(bytes):
    """An ERR line; `before` holds the lines the answer sent ahead of it
    (what a `card` measured before it stopped)."""

    before = b""


def answer(conn: Conn, first=SILENCE_S) -> tuple:
    """(Status.OK, body) or (Status.ERR, the ERR line), the first byte
    waited for up to `first` s.

    A `SHOT` header line stays in the body, followed by exactly its length
    of pixel bytes. Raises NoAnswer, Vanished or BadShot."""
    rd, body = Reader(conn, first), b""
    while True:
        ln = rd.line()
        if ln == b"OK":
            return Status.OK, body
        if ln == b"ERR" or ln.startswith(b"ERR "):
            err = ErrLine(ln)
            err.before = body
            return Status.ERR, err
        body += ln + b"\n"
        if ln.startswith(b"SHOT "):
            body += rd.exactly(Shot.declared_length(ln))


def is_busy(status, body) -> bool:
    return status is Status.ERR and body == b"ERR busy"


def retry_busy(ask, sleep=time.sleep) -> tuple:
    """`ask()` until it isn't `ERR busy`: `BUSY_TRIES` tries at most, waiting
    `BUSY_WAIT_S` and doubling between them. The last answer."""
    wait = BUSY_WAIT_S
    for _ in range(BUSY_TRIES - 1):
        status, body = ask()
        if not is_busy(status, body):
            return status, body
        sleep(wait)
        wait *= 2
    return ask()


def first_wait(line: str) -> float:
    """How long a request's first byte may take."""
    waits = {"card": CARD_WAIT_S, "ls": LS_WAIT_S, "rm": LS_WAIT_S, "get": LS_WAIT_S, "sweep": SWEEP_WAIT_S}
    return waits.get((line.split() or [""])[0], SILENCE_S)


def request(conn: Conn, line) -> tuple:
    """`send`, then `answer`, again while the unit says `ERR busy`."""
    text = line if isinstance(line, str) else line.decode(errors="replace")

    def ask():
        send(conn, line)
        return answer(conn, first_wait(text.lower()))

    return retry_busy(ask)


class BadShot(Exception):
    pass


class PutRefused(Exception):
    """The file can't go: the reason, before anything was sent."""


def remote_path(local: str, remote: str) -> str:
    """`remote`, or the local file's name under it if it ends in `/`."""
    return remote + pathlib.Path(local).name.upper() if remote.endswith("/") else remote


def put_lines(data: bytes):
    """The data lines: `PUT_LINE` bytes each, base64, one per line."""
    for i in range(0, len(data), PUT_LINE):
        yield base64.b64encode(data[i : i + PUT_LINE]) + b"\n"


def send_lines(conn: Conn, data: bytes, base: int, total: int, progress):
    """`data`'s lines, `progress(sent, total)` after each batch, counting
    from `base`."""
    lines = list(put_lines(data))
    batch = 256
    for i in range(0, len(lines), batch):
        conn.write(b"".join(lines[i : i + batch]))
        progress(base + min(len(data), (i + batch) * PUT_LINE), total)
    if not lines:
        progress(base, total)


def next_sound(conn: Conn) -> str:
    """`ls sounds`' top user id + 1, as `/CHIMERA/SOUNDS/S#######.A`; the
    first id on a card with no SOUNDS. Raises PutRefused."""
    status, body = request(conn, "ls sounds")
    if status is Status.ERR and body != b"ERR ls: FILE NOT FOUND":
        raise PutRefused(body.decode(errors="replace"))
    top = 0
    for ln in body.decode(errors="replace").splitlines() if status is Status.OK else []:
        name = ln.split(" ", 1)[0].upper()
        stem, _, ext = name.partition(".")
        if len(stem) == 8 and stem[0] == "S" and stem[1:].isdigit() and ext in ("A", "B"):
            n = int(stem[1:])
            if n <= USER_MAX:
                top = max(top, n)
    if top >= USER_MAX:
        raise PutRefused("put: no Sound ids left above S%07d" % top)
    return "%sS%07d.A" % (SOUNDS, top + 1)


def put(conn: Conn, data: bytes, remote: str, progress=lambda sent, total: None, over=False) -> tuple:
    """`put <remote> <size> <crc32>`, the data once the unit says READY,
    then its OK or ERR as `answer` gives it; (OK, b"PRESENT") for a source
    already on the card. A source goes in `PUT_CHUNK` chunks, each ended by
    `end <crc32>` and waited on for its `ACK <bytes so far>`.
    `progress(sent, total)` after each batch of lines. Raises NoAnswer,
    Vanished or PutRefused."""
    if len(data) > PUT_MAX:
        raise PutRefused("%d bytes: put takes at most %d" % (len(data), PUT_MAX))
    send(conn, "put %s %d %08x%s" % (remote, len(data), zlib.crc32(data), " --over" if over else ""))
    rd = Reader(conn, PUT_WAIT_S)
    first = rd.line()
    if first == b"ERR" or first.startswith(b"ERR "):
        return Status.ERR, ErrLine(first)
    if first == b"OK PRESENT":
        return Status.OK, b"PRESENT"
    if first != b"READY":
        raise PutRefused("put: expected READY, got %r" % first)
    if remote.upper().startswith(TAPES):
        for at in range(0, len(data), PUT_CHUNK):
            chunk = data[at : at + PUT_CHUNK]
            send_lines(conn, chunk, at, len(data), progress)
            conn.write(b"end %08x\n" % zlib.crc32(chunk))
            rd.wait = PUT_WAIT_S
            ln = rd.line()
            if ln != b"ACK %d" % (at + len(chunk)):
                return Status.ERR, ErrLine(ln)
    else:
        send_lines(conn, data, 0, len(data), progress)
    rd.wait = PUT_WAIT_S
    ln = rd.line()
    if ln == b"OK":
        return Status.OK, b""
    if ln.startswith(b"OK staged:"):  # a .bk2 sealed for another firmware
        return Status.OK, ln[3:]
    return Status.ERR, ErrLine(ln)


def show_progress(sent, total):
    pct = 100 if total == 0 else sent * 100 // total
    print("\rput: %3d%% %d of %d bytes" % (pct, sent, total), end="", file=sys.stderr, flush=True)


def put_file(t: Target, local: str, remote: str, over=False) -> int:
    """`put` with progress on stderr, again while the unit is busy: 0 on
    OK, 1 with the reason."""
    try:
        data = pathlib.Path(local).read_bytes()
    except OSError as e:
        return fail("put: %s: %s" % (local, e.strerror or e))
    conn = open_conn(t)
    try:
        if remote.upper() == SOUNDS:
            remote = next_sound(conn)
        else:
            remote = remote_path(local, remote)
        status, body = retry_busy(lambda: put(conn, data, remote, show_progress, over))
    except PutRefused as e:
        return fail(str(e))
    except NoAnswer:
        return fail("\nput: no answer from %s" % describe(t))
    except Vanished:
        return fail("\nput: %s went away" % describe(t))
    finally:
        conn.close()
    print(file=sys.stderr)
    if status is Status.ERR:
        return fail(body.decode(errors="replace"))
    print("%s %d%s" % (remote, len(data), " PRESENT" if body == b"PRESENT" else ""))
    if body.startswith(b"staged:"):
        print(body.decode(errors="replace"))
    return 0


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
    if d:
        return pathlib.Path(d)
    root = repo_root()
    return root / "target/shots" if (root / "Cargo.toml").exists() else pathlib.Path.cwd()


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
        sys.exit(fail("no access to %s. Add yourself to the dialout group (sudo usermod -aG dialout $USER, then log out and in), or on Linux run: %s" % (describe(t), UDEV_HINT)))
    except OSError as e:
        sys.exit(fail("no console at %s: %s. Is the unit on, running Chimera, and connected by USB? CHIMERA_USB=<port> picks another port." % (describe(t), e.strerror or e)))


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
    return fail("no DFU device after dfu: marker clobbered? re-plug and try again")


def wait_console(env, secs) -> int:
    """Exit 0 once the console is on the bus again, 1 after `secs`."""
    sysfs = env.get("CHIMERA_SYSFS", "/sys")
    deadline = time.monotonic() + secs
    while CONSOLE_ID not in usb_devices(sysfs):
        if time.monotonic() >= deadline:
            return fail("console not back %g s after the flash" % secs)
        time.sleep(0.1)
    return 0


def bank2_answer(conn: Conn) -> tuple:
    """`bank2 install`'s answer: (final line, the lines before it), the first
    byte waited for `BANK2_WAIT_S`. Its OK lines say more than `OK`.
    Raises NoAnswer or Vanished."""
    rd, body = Reader(conn, BANK2_WAIT_S), b""
    while True:
        ln = rd.line()
        if ln == b"OK" or ln.startswith(b"OK ") or ln == b"ERR" or ln.startswith(b"ERR "):
            return ln, body
        body += ln + b"\n"


def bank2_install(t: Target, env, local: str) -> int:
    """Put `local` where the unit stages it and have the unit write it:
    0 on OK, 2 if it is for another firmware, else 1."""
    if not local.lower().endswith(".bk2"):
        return fail("bank2 takes a .bk2 file, not %s (the .bin goes by DFU)" % local)
    try:
        data = pathlib.Path(local).read_bytes()
    except OSError as e:
        return fail("bank2: %s: %s" % (local, e.strerror or e))
    if len(data) < 0x20 or data[:8] != b"CHIMERA2":
        return fail("bank2: %s is not a .bk2" % local)
    seal = data[BANK2_SEAL].hex()
    rc = wait_console(env, float(env.get("CHIMERA_DFU_WAIT", "10")))
    if rc:
        return rc
    conn = open_conn(t)
    try:
        status, body = request(conn, "bank2")
        state = body.decode(errors="replace").splitlines()[:1]
        if status is Status.OK and state and state[0].startswith("bank2 open ") and state[0].endswith(" seal=" + seal):
            print("OK already installed")
            return 0
        status, body = retry_busy(lambda: put(conn, data, BANK2_REMOTE, show_progress))
        print(file=sys.stderr)
        if status is Status.ERR:
            return fail(body.decode(errors="replace"))
        if body.startswith(b"staged:"):
            return stage_only(body)
        for _ in range(BUSY_TRIES):
            send(conn, "bank2 install")
            ln, before = bank2_answer(conn)
            if ln != b"ERR bank2: busy":
                break
            time.sleep(BUSY_WAIT_S)
    except PutRefused as e:
        return fail(str(e))
    except NoAnswer:
        return fail("\nbank2: no answer from %s" % describe(t))
    except Vanished:
        return fail("\nbank2: %s went away" % describe(t))
    finally:
        conn.close()
    sys.stdout.buffer.write(before)
    text = ln.decode(errors="replace")
    if text.startswith("OK"):
        print(text)
        return 0
    if "staged: needs firmware" in text:
        fail(text + LATER)
        return 2
    return fail(text)


def bank2_erase(t: Target) -> int:
    """`bank2 erase`, a bank2-gate build's: S0 erased for the next power
    pull. 0 on OK, else 1."""
    conn = open_conn(t)
    try:
        send(conn, "bank2 erase")
        ln, before = bank2_answer(conn)
    except NoAnswer:
        return fail("bank2 erase: no answer from %s" % describe(t))
    except Vanished:
        return fail("bank2 erase: %s went away" % describe(t))
    finally:
        conn.close()
    sys.stdout.buffer.write(before)
    text = ln.decode(errors="replace")
    if text.startswith("OK"):
        print(text)
        return 0
    return fail(text)


def stage_only(body: bytes) -> int:
    fail(body.decode(errors="replace") + LATER)
    return 2


class BadPackage(Exception):
    """The update can't go: why, before anything touched the unit."""


class Package(NamedTuple):
    """A release's pair: `name`.bin for DFU and `name`.bk2 for the card."""

    name: str
    bin: bytes
    bk2: bytes

    @property
    def version(self) -> str:
        return self.bk2[0x20:0x40].split(b"\0", 1)[0].decode("ascii", "replace")

    @property
    def seal(self) -> str:
        return self.bk2[BANK2_SEAL].hex()


def check_pair(name: str, bin_data: bytes, bk2: bytes) -> Package:
    """The .bin under the bootloader's limit and no .bk2 in disguise; the
    .bk2 whole (length, CRC-32, its seal its body's SHA-256 prefix) and
    sealed for this .bin (the seal in it once). Raises BadPackage."""
    if bin_data[:8] == b"CHIMERA2":
        raise BadPackage("%s.bin is a .bk2 under a .bin name" % name)
    if len(bin_data) >= FW_MAX:
        raise BadPackage("%s.bin is %d bytes: too big for the firmware area" % (name, len(bin_data)))
    if bk2[:8] != b"CHIMERA2" or not BK2_HEADER <= len(bk2) <= BK2_MAX:
        raise BadPackage("%s.bk2 is not a .bk2" % name)
    body = bk2[BK2_HEADER:]
    if int.from_bytes(bk2[0x0C:0x10], "little") != len(body) or zlib.crc32(body) != int.from_bytes(bk2[0x10:0x14], "little"):
        raise BadPackage("%s.bk2 is damaged" % name)
    seal = bk2[BANK2_SEAL]
    if hashlib.sha256(body).digest()[:8] != seal:
        raise BadPackage("%s.bk2 is damaged (its seal)" % name)
    if bin_data.count(seal) != 1:
        raise BadPackage("%s.bin and its .bk2 are not one build" % name)
    return Package(name, bin_data, bk2)


def read_files(bin_path: str, bk2_path: str) -> Package:
    """A loose pair, as `just flash` builds it. Raises BadPackage."""
    if not bin_path.lower().endswith(".bin") or not bk2_path.lower().endswith(".bk2"):
        raise BadPackage("takes <file.bin> <file.bk2>, in that order")
    try:
        bin_data, bk2 = pathlib.Path(bin_path).read_bytes(), pathlib.Path(bk2_path).read_bytes()
    except OSError as e:
        raise BadPackage("%s: %s" % (e.filename, e.strerror or e))
    return check_pair(pathlib.Path(bin_path).name[:-4], bin_data, bk2)


def dfu_flash(t: Target, env, local: str, wait: float) -> int:
    """`local` into the firmware area by DFU, then the console back within
    `wait` s."""
    rc = to_dfu(t, env)
    if rc:
        return rc
    try:
        r = subprocess.run(["dfu-util", "-a0", "-d", "0483:df11", "-D", local, "-s", FLASH_AT], capture_output=True, text=True, timeout=DFU_UTIL_S)
    except subprocess.TimeoutExpired:
        return fail("update: dfu-util still running after %d s" % DFU_UTIL_S)
    except OSError as e:
        return fail("update: dfu-util: %s" % (e.strerror or e))
    sys.stdout.write(r.stdout)
    sys.stderr.write(r.stderr)
    # `:leave` resets the unit under dfu-util, which then exits 74.
    if r.returncode not in (0, 74) or (r.returncode == 74 and "File downloaded successfully" not in r.stdout):
        return fail("update: dfu-util failed (%d)" % r.returncode)
    return wait_console(env, wait)


def stage(t: Target, env, pkg: Package):
    """The .bk2 onto the card under the running firmware: True staged,
    False for firmware from before bank 2 (it refuses the path), else the
    exit code."""
    rc = wait_console(env, float(env.get("CHIMERA_DFU_WAIT", "10")))
    if rc:
        return rc
    conn = open_conn(t)
    try:
        status, body = retry_busy(lambda: put(conn, pkg.bk2, BANK2_REMOTE, show_progress))
    except PutRefused as e:
        return fail("\nupdate: %s: nothing flashed" % e)
    except NoAnswer:
        return fail("\nupdate: no answer from %s: nothing flashed" % describe(t))
    except Vanished:
        return fail("\nupdate: %s went away: nothing flashed" % describe(t))
    finally:
        conn.close()
    print(file=sys.stderr)
    if status is Status.OK:
        return True
    if bytes(body) in OLD_PUT_REFUSALS:
        print("this firmware can't take the .bk2 yet: it goes after the flash", file=sys.stderr)
        return False
    return fail("%s: nothing flashed" % body.decode(errors="replace"))


def updated(t: Target, pkg: Package) -> int:
    """0 once `status` shows the new firmware running (its build line's
    hash, the .bk2 version's) and `pkg`'s seal open, else what it shows
    and 1. The seal, not bank 2's version: a release whose .bk2 didn't
    change keeps the one installed before it, and its version."""
    want = "bank2 open %s seal=%s" % (pkg.version, pkg.seal)
    conn = open_conn(t)
    try:
        status, body = request(conn, "status")
    except NoAnswer:
        return fail("update: flashed, but no status from %s" % describe(t))
    except Vanished:
        return fail("update: flashed, but %s went away" % describe(t))
    finally:
        conn.close()
    text = body.decode(errors="replace")
    build = next((ln for ln in text.splitlines() if ln.startswith("build ")), "")
    hash_ = pkg.version.split()[-1:]
    if status is Status.OK and (not hash_ or build.rstrip("+").split()[-1:] != hash_):
        return fail("update: flashed, but status says %r: the new firmware (%s) isn't the one running" % (build or text.strip(), pkg.version))
    got = next((ln for ln in text.splitlines() if ln.startswith("bank2 ")), text.strip())
    if status is Status.OK and got.startswith("bank2 open ") and got.endswith(" seal=" + pkg.seal):
        print("OK updated %s" % pkg.version)
        return 0
    return fail("update: flashed, but status says %r, not %r (a staged .bk2 stays on the card for the next boot)" % (got, want))


def update(t: Target, env, pkg: Package) -> int:
    """One update: the .bk2 staged under the running firmware, the .bin by
    DFU, the first boot's install waited for, then `status` checked. Older
    firmware takes the .bk2 after the flash instead."""
    staged = stage(t, env, pkg)
    if staged is not True and staged is not False:
        return staged
    with tempfile.TemporaryDirectory() as d:
        b, k = pathlib.Path(d, pkg.name + ".bin"), pathlib.Path(d, pkg.name + ".bk2")
        b.write_bytes(pkg.bin)
        k.write_bytes(pkg.bk2)
        rc = dfu_flash(t, env, str(b), UPDATE_BOOT_WAIT_S)
        if rc:
            return rc
        if not staged and bank2_install(t, env, str(k)):
            return 1
    return updated(t, pkg)


def main(argv, env) -> int:
    if argv[:1] == ["--version"]:
        print(VERSION)
        return 0
    if not argv or argv[0] in ("-h", "--help"):
        print(__doc__.strip(), file=sys.stderr)
        return 2
    t = target(env)
    if argv[0] == "to-dfu":
        return to_dfu(t, env)
    if argv[0] == "wait-console":
        return wait_console(env, float(argv[1]) if len(argv) > 1 else 10.0)
    if argv[0] == "bank2" and argv[1:] == ["erase"]:
        return bank2_erase(t)
    if argv[0] == "bank2":
        return bank2_install(t, env, argv[1]) if len(argv) == 2 else fail("bank2 takes <file.bk2>")
    if argv[0] == "update":
        try:
            if len(argv) != 3:
                return fail("update takes <file.bin> <file.bk2>")
            pkg = read_files(argv[1], argv[2])
        except BadPackage as e:
            return fail("update: %s" % e)
        return update(t, env, pkg)
    if argv[0] == "put":
        over = argv[3:] == ["--over"]
        if len(argv) != 3 and not over:
            return fail("put takes <local> <remote> [--over]")
        return put_file(t, argv[1], argv[2], over)
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
        sys.stdout.buffer.write(getattr(body, "before", b""))
        sys.stdout.flush()
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
