#!/usr/bin/env python3
"""Talk to your Chimera over USB: status for bug reports, screenshots, files onto the card.

    python3 chimera-usb.py <cmd> [<arg>]     help, status, stats, bench, card [clean|seed=N], shot [raw], dfu, sweep, flash,
                                                   ls [tapes|sounds|maps], rm|get /CHIMERA/SOUNDS/S####### or
                                                   /CHIMERA/MAPS/<stem>; rm also /CHIMERA/TUNINGS/<NAME>.SCL|KBM
    python3 chimera-usb.py get <remote> <local>    the file `get` reads, checked and written to <local>
    python3 chimera-usb.py put <local> <remote> [--over]
                                                   a project file onto the card as /CHIMERA/PROJECTS/P#######.A,
                                                   a library Sound as /CHIMERA/SOUNDS/S#######.A (--over replaces one there),
                                                   or a TAPE source as /CHIMERA/TAPES/<CRC>.SRC, in checked chunks,
                                                   or the bench's check table as /CHIMERA/BENCH/BEDS.CHK (9 KiB at most),
                                                   or a controller map as /CHIMERA/MAPS/<stem>.A (4 KiB at most)
                                                   (<remote> ending in / takes the local name; /CHIMERA/SOUNDS/ takes
                                                   `ls sounds`' top user id + 1; /CHIMERA/MAPS/ the stem of the
                                                   map's name)
    python3 chimera-usb.py tuning <file.scl> [<file.kbm>]
                                                   a Scala tuning onto the card under its own name,
                                                   /CHIMERA/TUNINGS/<NAME>.SCL, with its keyboard map as <NAME>.KBM
                                                   (4 KiB each at most; the name 1-8 of A-Z and 0-9): past MTS,
                                                   SETTINGS > AUDIO > TUNING lists it; `rm` takes
                                                   either off again (a Part on it then plays 12-TET)
    python3 chimera-usb.py update <file.bin> <file.bk2>
                                                   flash a build's pair in one go (developers, `just flash`): the
                                                   .bk2 onto the card, the .bin by DFU, and the first boot
                                                   installs the .bk2
    python3 chimera-usb.py bank2 <file.bk2>  install a .bk2 under the running firmware (developers)
    python3 chimera-usb.py bank2 erase       empty bank 2 (a `just firmware-gate` build only)
    python3 chimera-usb.py flash             the flash controller's registers and bank 2's first bytes, read only
    python3 chimera-usb.py key <name>[+<name>] [hold_ms]
                                                   press a button as a hand would (a tap without hold_ms):
                                                   B1-B6 MENU MINUS PLUS MIX EDIT SEQ PUSH1-6; key MIX+SEQ
                                                   holds MIX while it taps SEQ. OK once queued (8 at most)
    python3 chimera-usb.py turn <encoder 1-6> <delta -64..64>
                                                   detents every 25 ms, so acceleration builds as by hand
    python3 chimera-usb.py card-wait <bench> [N|kind]
                                                   a bench build's `card dma <bench>`, then `card dma report`
                                                   every 10 s until its result, which it prints
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
the flash if the project has unsaved changes or the .bk2 can't go onto the card, waits up to 2 minutes
for the unit to answer after the flash, and exits 0 only once
`status` shows the pair's seal open (`OK updated <version>`);
it needs dfu-util. An install that opens bank 2 restarts the unit (its USB
MIDI and audio come up at boot): `bank2` waits up to a minute for it, then
exits 0 on OK (or `OK already installed`), 2 if
the file belongs to another firmware (`staged: needs firmware <version>`: it
installs at the next boot of that firmware), 1 on any other ERR.
`--version` prints this tool's release.

The port: CHIMERA_USB picks it. Unset, on Linux /dev/chimera (the udev
rule's link), else the tty whose USB id is the console's, one named Chimera
before a stock PreenFM3's (one found only by its id must answer `status` as
Chimera); elsewhere pyserial's, else the first /dev/cu.usbmodem* (macOS);
else give a device path, or `tcp:HOST:PORT`. On Linux, if it says "no access", either
add yourself to the dialout group (then log out and in again), or run the
`sudo tee` line it prints once. Shots are saved in the current directory;
CHIMERA_SHOTS picks another. Where there is no /sys (macOS, Windows),
to-dfu, wait-console and update find the unit with pyserial (pip install
pyserial) and dfu-util -l. CHIMERA_SYSFS and CHIMERA_DFU_WAIT (s,
default 10) are for to-dfu and wait-console; CHIMERA_POLL_S (s, default
10) is card-wait's.
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
import re
import select
import socket
import struct
import subprocess
import sys
import tempfile
import time
import zlib
from typing import NamedTuple, Optional, Union

try:
    import termios
    import tty
except ImportError:  # Windows: the console goes through pyserial
    termios = tty = None

TermiosError = termios.error if termios else OSError

SIM = ("127.0.0.1", 7341)  # the desktop simulator (developers)
SILENCE_S = 2.0
CARD_POLL_S = 10.0  # card-wait's `card dma report`, a bench's minutes apart enough
CARD_WAIT_S = 60.0  # `card` measures before it answers: ~5 s, ~15 s writing its test file
LS_WAIT_S = 15.0  # a cold card acquire, then one listing under the card's 10 s cap
SWEEP_WAIT_S = 15.0  # a cold acquire, then the mark's one write; the unit restarts after OK
PUT_WAIT_S = 125.0  # the unit's put deadline (120 s for a 1 MB .bk2) plus 5 s
BANK2_WAIT_S = 120.0  # the unit holds its UI loop for the whole write
LATER = " (it installs at that firmware's next boot)"
BANK2_REMOTE = "/CHIMERA/SYSTEM/BANK2.BK2"
BANK2_SEAL = slice(0x18, 0x20)  # the .bk2 header's seal
BK2_HEADER = 256
BK2_MAX = 1 << 20  # bank 2's slots: 1 MB
UPDATE_BOOT_WAIT_S = 120.0  # the first boot installs the .bk2 behind DON'T POWER OFF, then the console comes up
RESTART_WAIT_S = 60.0  # `bank2 install` opened the door: the unit restarts so USB lays out MIDI and audio (#1064)
# Firmware from before bank 2 refuses the path (its `put` usage, exactly): the
# .bk2 then goes after the flash.
OLD_PUT_REFUSALS = (
    b"ERR put takes /CHIMERA/PROJECTS/P#######.A, /CHIMERA/SOUNDS/S#######.A [--over] or /CHIMERA/TAPES/<CRC>.SRC, its size and CRC-32 in hex",
    b"ERR put takes /CHIMERA/PROJECTS/P#######.A or /CHIMERA/SOUNDS/S#######.A [--over], its size and CRC-32 in hex",
)
FOREIGN_SLOTS = {b"mixer": ("MIXER", "0x08180000"), b"edit": ("EDIT", "0x081C0000")}


def foreign_slot(line: str):
    """What `bank2 slot foreign <slot>` (`status`'s line, `bank2 install`'s
    after ERR) means for the user; None for a slot it doesn't name."""
    slot = FOREIGN_SLOTS.get(line.split()[-1].encode())
    return slot and "the unit's %s slot (%s) holds other code; bank 2 not installed" % slot


def old_put_refusal(body: bytes, bk2_len: int) -> bool:
    """Firmware from before bank 2 refuses the path: its usage line exactly,
    or (0.2, 0.3 word it differently) any `put takes` line naming BANK2.BK2
    when the .bk2 is over the old 512 KB."""
    return body in OLD_PUT_REFUSALS or (body.startswith(b"ERR put takes") and b"BANK2.BK2" in body and bk2_len > 512 * 1024)


BK2_FORMAT = slice(8, 10)  # the .bk2 header's format, little-endian
BK2_FORMAT_NOW = 2  # 0.4's: the 1 MB image with the slots (0.3's is 1)
FW_MAX = 917_504  # the stock bootloader erases itself at this size
FLASH_AT = "0x8020000:leave"  # the firmware area, never the bootloader
PUT_MAX = 1 << 20  # the unit's limit
PUT_LINE = 48  # bytes a data line carries: 64 base64 characters
PUT_CHUNK = 256 * 1024  # a TAPE source goes in chunks this big
PUT_BEDS_MAX = 9 * 1024  # the bench's check table
TAPES = "/CHIMERA/TAPES/"
BEDS = "/CHIMERA/BENCH/BEDS.CHK"
TUNINGS = "/CHIMERA/TUNINGS/"
PUT_TUNING_MAX = 4 * 1024  # what the unit reads of a tuning file
SOUNDS = "/CHIMERA/SOUNDS/"
MAPS = "/CHIMERA/MAPS/"
MAP_KIND = 7  # a Chimera file's header byte 6: a controller map
USER_MAX = 8_999_999  # highest user Sound id
BUSY_TRIES = 5
BUSY_WAIT_S = 0.2  # doubling after each `ERR busy`
DRAIN_S = 0.05
MAX_SHOT = 1 << 20  # well over 240 x 320 x 2; a garbled header can't ask for gigabytes
UDEV_HINT = (
    "echo 'SUBSYSTEM==\"tty\", ATTRS{idVendor}==\"0483\", ATTRS{idProduct}==\"5740\", ENV{ID_MM_DEVICE_IGNORE}=\"1\", TAG+=\"uaccess\", SYMLINK+=\"chimera\"'"
    " | sudo tee /etc/udev/rules.d/70-chimera.rules && sudo udevadm control --reload && sudo udevadm trigger"
)


ACCESS_HINT = "add yourself to the dialout group (sudo usermod -aG dialout $USER, then log out and in), or on Linux run: " + UDEV_HINT


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
        if not has_sysfs(env):
            found = console_ports()
            if len(found) > 1:
                sys.exit(fail("several units on USB (%s): set CHIMERA_USB=<port> to pick one" % ", ".join(found)))
            if found:
                return Tty("tty", found[0])
            for p in sorted(glob.glob("/dev/cu.usbmodem*")):
                return Tty("tty", p)
            return Tty("tty", "/dev/ttyACM0")
        return default_port(env)
    if v == "sim":
        return Tcp("tcp", *SIM)
    if v.startswith("tcp:"):
        host, port = v[4:].rsplit(":", 1)
        return Tcp("tcp", host, int(port))
    return Tty("tty", v)


UDEV_LINK = "/dev/chimera"  # the udev rule's symlink (UDEV_HINT)


def default_port(env) -> Tty:
    """Linux, no CHIMERA_USB: the udev link; else the tty whose USB id is
    the console's, one named Chimera before others with that id (a stock
    PreenFM3 shares it), never a guessed /dev/ttyACM0 (#314). One found
    only by its id must answer `status` as Chimera first."""
    if os.path.exists(UDEV_LINK):
        return Tty("tty", UDEV_LINK)
    ports = unit_ports(env)
    named = [p for p in ports if "chimera" in (p.product or "").lower()]
    pool = named or ports
    if len(pool) > 1:
        sys.exit(fail("several units on USB (%s): set CHIMERA_USB=<port> to pick one" % ", ".join(p.path for p in pool)))
    if not pool:
        return Tty("tty", UDEV_LINK)  # opening it says there is no console
    t = Tty("tty", pool[0].path)
    if not named and not is_chimera(t):
        sys.exit(fail("%s has the console's USB id but doesn't answer as Chimera: set CHIMERA_USB=<port>" % t.path))
    return t


def is_chimera(t: Target) -> bool:
    """`t` answers `status` with Chimera's `firmware` line."""
    try:
        conn = Conn(t)
    except (OSError, TermiosError):
        return False
    try:
        status, body = request(conn, "status")
    except (NoAnswer, Vanished):
        return False
    finally:
        conn.close()
    return status is Status.OK and any(ln.startswith(b"firmware ") for ln in body.splitlines())


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
        self.sock = self.ser = None
        if isinstance(t, Tty) and tty is None:
            try:
                import serial
            except ImportError:
                sys.exit(fail("the console on this system needs pyserial: pip install pyserial"))
            self.ser = serial.Serial(t.path, timeout=0)
        elif isinstance(t, Tcp):
            self.sock = socket.create_connection((t.host, t.port), timeout=SILENCE_S)
            self.sock.settimeout(None)  # blocking; reads wait in select
            self.fd = self.sock.fileno()
        else:
            self.fd = os.open(t.path, os.O_RDWR | os.O_NOCTTY)
            # Raw: the tty's echo would send the unit's answers back as requests.
            try:
                tty.setraw(self.fd)
            except TermiosError:  # not a terminal: don't leak the fd
                os.close(self.fd)
                raise

    def read(self, timeout) -> bytes:
        """Some bytes, or b"" after `timeout` s of silence."""
        if self.ser:
            try:
                self.ser.timeout = timeout
                first = self.ser.read(1)
                return first + self.ser.read(self.ser.in_waiting) if first else b""
            except OSError as e:  # serial.SerialException: the port went away
                raise Vanished() from e
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
            if self.ser:
                self.ser.write(data)
                return
            while data:
                data = data[os.write(self.fd, data) :]
        except OSError as e:
            raise Vanished() from e

    def close(self):
        if self.ser:
            self.ser.close()
        elif self.sock:
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


LISTS = ("ls",)  # answers whose rows are `<name> <size>`: a file may be named OK or ERR
ROW = re.compile(rb"\S+ [0-9]+")


def answer(conn: Conn, first=SILENCE_S, listing=False) -> tuple:
    """(Status.OK, body) or (Status.ERR, the ERR line), the first byte
    waited for up to `first` s. An `OK <words>` line ends the body too;
    in a `listing`, a `<name> <size>` row never does, whatever its name.

    A `SHOT` header line stays in the body, followed by exactly its length
    of pixel bytes. Raises NoAnswer, Vanished or BadShot."""
    rd, body = Reader(conn, first), b""
    while True:
        ln = rd.line()
        if listing and ROW.fullmatch(ln):
            body += ln + b"\n"
            continue
        if ln == b"OK":
            return Status.OK, body
        # `OK queued`, `ed since`'s `OK <gen> [more]`: the line is the news.
        if ln.startswith(b"OK "):
            return Status.OK, body + ln + b"\n"
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
        word = (text.lower().split() or [""])[0]
        return answer(conn, first_wait(text.lower()), word in LISTS)

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


def map_stem(data: bytes) -> str:
    """The stem a controller map's file goes under: up to four of its
    name's letters and digits, upper case, then a 16-bit FNV-1a of the name
    in hex (the unit's `midi_map::ctl::stem`). Raises PutRefused."""
    if len(data) < 28 or data[:4] != b"CHIM" or data[6] != MAP_KIND:
        raise PutRefused("put: not a controller map")
    name = data[12:28].split(b"\0", 1)[0]
    if not name:
        raise PutRefused("put: the map has no name")
    h = 0x811C9DC5
    for c in name:
        h = ((h ^ c) * 0x01000193) & 0xFFFFFFFF
    letters = bytes(c for c in name if chr(c).isalnum())[:4].upper() or b"M"
    return letters.decode() + "%04X" % ((h ^ h >> 16) & 0xFFFF)


def get_file(t: Target, remote: str, local: str) -> int:
    """`get <remote>`, its CRC checked, written to `local`: 0 on OK, 1 with
    the reason. `local` is never overwritten."""
    conn = open_conn(t)
    try:
        status, body = request(conn, "get " + remote)
    except NoAnswer:
        return fail("get: no answer from %s" % describe(t))
    except Vanished:
        return fail("get: %s went away" % describe(t))
    finally:
        conn.close()
    if status is Status.ERR:
        return fail(body.decode(errors="replace"))
    lines = body.split(b"\n")
    try:
        size = int(lines[0].split(b" ")[1]) if lines[0].startswith(b"GET ") else -1
        crc = [ln for ln in lines if ln.startswith(b"CRC ")]
        data = base64.b64decode(b"".join(ln for ln in lines[1:] if ln and not ln.startswith(b"CRC ")))
        want = int(crc[0][4:], 16) if crc else -1
    except (ValueError, IndexError):
        return fail("get: a garbled answer")
    if len(data) != size or zlib.crc32(data) != want:
        return fail("get: the file came through damaged")
    try:
        with open(local, "xb") as f:
            f.write(data)
    except OSError as e:
        return fail("get: %s: %s" % (local, e.strerror or e))
    print("%s %d" % (local, len(data)))
    return 0


def put(conn: Conn, data: bytes, remote: str, progress=lambda sent, total: None, over=False) -> tuple:
    """`put <remote> <size> <crc32>`, the data once the unit says READY,
    then its OK or ERR as `answer` gives it; (OK, b"PRESENT") for a source
    already on the card. A source goes in `PUT_CHUNK` chunks, each ended by
    `end <crc32>` and waited on for its `ACK <bytes so far>`.
    `progress(sent, total)` after each batch of lines. Raises NoAnswer,
    Vanished or PutRefused."""
    up = remote.upper()
    cap = PUT_BEDS_MAX if up == BEDS else PUT_TUNING_MAX if up.startswith((TUNINGS, MAPS)) else PUT_MAX
    if len(data) > cap:
        raise PutRefused("%d bytes: put takes at most %d" % (len(data), cap))
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
        elif remote.upper() == MAPS:
            remote = MAPS + map_stem(data) + ".A"
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


def sysfs_devices(sysfs) -> set:
    """The (vid, pid) of every USB device the kernel lists."""
    ids = set()
    for d in pathlib.Path(sysfs, "bus/usb/devices").glob("*"):
        try:
            ids.add(UsbId(int((d / "idVendor").read_text(), 16), int((d / "idProduct").read_text(), 16)))
        except (OSError, ValueError):
            pass  # interfaces and hubs without ids, or a device leaving mid-read
    return ids


def dfu_listing() -> list:
    """One `dfu-util -l` line per distinct 0483:df11 device (a device lists
    one line per alt setting), [] if there is no dfu-util."""
    try:
        out = subprocess.run(["dfu-util", "-l"], capture_output=True, text=True, timeout=10).stdout
    except (OSError, subprocess.TimeoutExpired):
        return []
    seen = {}
    for ln in out.splitlines():
        if "[0483:df11]" in ln.lower():
            seen.setdefault(re.sub(r"alt=\d+, name=.*?(, serial=)", r"\1", ln.strip()), ln.strip())
    return list(seen)


def console_ports() -> list:
    """The device paths of pyserial's ports with the console's id, [] without pyserial."""
    try:
        from serial.tools import list_ports
    except ImportError:
        return []
    return sorted(p.device for p in list_ports.comports() if (p.vid, p.pid) == tuple(CONSOLE_ID))


def port_devices(dfu) -> set:
    """macOS, Windows, Linux without sysfs: pyserial's serial ports (the
    console) and, if `dfu`, `dfu-util -l` (the ROM loader is no serial port)."""
    try:
        from serial.tools import list_ports
    except ImportError:
        sys.exit(fail("no /sys here, so finding the unit needs pyserial: pip install pyserial"))
    ids = {UsbId(p.vid, p.pid) for p in list_ports.comports() if p.vid is not None}
    if dfu and dfu_listing():
        ids.add(ROM_DFU_ID)
    return ids


def has_sysfs(env) -> bool:
    return pathlib.Path(env.get("CHIMERA_SYSFS", "/sys"), "bus/usb/devices").is_dir()


def usb_devices(env, dfu=True) -> set:
    """The (vid, pid) of the USB devices we care about: sysfs where the
    kernel has it, else pyserial and dfu-util."""
    if has_sysfs(env):
        return sysfs_devices(env.get("CHIMERA_SYSFS", "/sys"))
    return port_devices(dfu)


def fail(msg) -> int:
    print(msg, file=sys.stderr)
    return 1


_fail = fail


def open_conn(t: Target) -> Conn:
    """A Conn, or a one-line reason on stderr and SystemExit(1)."""
    try:
        return Conn(t)
    except PermissionError:
        sys.exit(fail("no access to %s. %s" % (describe(t), ACCESS_HINT[0].upper() + ACCESS_HINT[1:])))
    except OSError as e:
        sys.exit(fail("no console at %s: %s. Is the unit on, running Chimera, and connected by USB? CHIMERA_USB=<port> picks another port." % (describe(t), e.strerror or e)))


def to_dfu(t: Target, env) -> int:
    ids = usb_devices(env)
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
        if ROM_DFU_ID in usb_devices(env):
            return 0
        time.sleep(0.1)
    return fail("no DFU device after dfu: marker clobbered? re-plug and try again")


def wait_console(env, secs) -> int:
    """Exit 0 once the console is on the bus again, 1 after `secs`."""
    deadline = time.monotonic() + secs
    while CONSOLE_ID not in usb_devices(env, dfu=False):
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


def bank2_install(t: Target, env, local: str, note: str = "") -> int:
    """Put `local` where the unit stages it and have the unit write it:
    0 on OK, 2 if it is for another firmware or the unit's slot holds other
    code, else 1. `note` leads every failure message (update: \"flashed, but \")."""
    return install_bk2(t, env, local, note)[0]


def install_bk2(t: Target, env, local: str, note: str = "") -> tuple:
    """`bank2_install`'s exit code, and the unit once back from the restart
    an install that opens the door ends in (#1064: USB's MIDI and audio
    are a boot's layout), else None."""
    fail = lambda m: (_fail(note + m), None)
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
        return rc, None
    serial = unit_serial(t, env)  # the unit before its restart, to know it after
    conn = open_conn(t)
    try:
        status, body = request(conn, "bank2")
        state = body.decode(errors="replace").splitlines()[:1]
        if status is Status.OK and state and state[0].startswith("bank2 open ") and state[0].endswith(" seal=" + seal):
            print("OK already installed")
            return 0, None
        status, body = retry_busy(lambda: put(conn, data, BANK2_REMOTE, show_progress))
        print(file=sys.stderr)
        if status is Status.ERR:
            return fail(body.decode(errors="replace"))
        if body.startswith(b"staged:"):
            return stage_only(body, note), None
        for _ in range(BUSY_TRIES):
            send(conn, "bank2 install")
            ln, before = bank2_answer(conn)
            if ln != b"ERR bank2: busy":
                break
            time.sleep(BUSY_WAIT_S)
    except PutRefused as e:
        return fail(str(e))
    except NoAnswer:
        print(file=sys.stderr)
        return fail("bank2: no answer from %s" % describe(t))
    except Vanished:
        print(file=sys.stderr)
        return fail("bank2: %s went away" % describe(t))
    finally:
        conn.close()
    sys.stdout.buffer.write(before)
    text = ln.decode(errors="replace")
    if text.startswith("OK bank2 open "):
        print(text)
        sys.stdout.flush()
        print("restarting: the open door's USB MIDI and audio come up at boot", file=sys.stderr)
        try:
            return 0, back_after_flash(t, env, RESTART_WAIT_S, serial)
        except NotBack as e:
            return fail("bank2: installed, but the unit isn't back %g s after its restart: %s" % (RESTART_WAIT_S, e.reason()))
    if text.startswith("OK"):
        print(text)
        return 0, None
    foreign = text.startswith("ERR bank2 slot foreign ") and foreign_slot(text)
    if foreign:
        _fail(note + foreign)
        return 2, None
    if "staged: needs firmware" in text:
        _fail(note + text + LATER)
        return 2, None
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


def stage_only(body: bytes, note: str = "") -> int:
    fail(note + body.decode(errors="replace") + LATER)
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


def dfu_flash(t: Target, env, local: str) -> int:
    """`local` into the firmware area by DFU; the unit then resets."""
    rc = to_dfu(t, env)
    if rc:
        return rc
    devs = dfu_listing()
    if len(devs) != 1:
        return fail("update: want exactly one DFU device, dfu-util -l shows %d%s" % (len(devs), "".join("\n  " + d for d in devs)))
    try:  # no timeout: killing dfu-util mid-write strands the unit
        r = subprocess.run(["dfu-util", "-a0", "-d", "0483:df11", "-D", local, "-s", FLASH_AT], capture_output=True, text=True)
    except OSError as e:
        return fail("update: dfu-util: %s" % (e.strerror or e))
    sys.stdout.write(r.stdout)
    sys.stderr.write(r.stderr)
    # `:leave` resets the unit under dfu-util, which then exits 74.
    if r.returncode not in (0, 74) or (r.returncode == 74 and "File downloaded successfully" not in r.stdout):
        return fail("update: dfu-util failed (%d)" % r.returncode)
    return 0


class UnitPort(NamedTuple):
    """A tty whose USB device has the console's id, and its USB serial
    (None when the device has none)."""

    path: str
    serial: Optional[str]
    product: Optional[str] = None


def _read(path: pathlib.Path) -> Optional[str]:
    try:
        return path.read_text().strip()
    except OSError:
        return None


def unit_ports(env) -> list:
    """The console's ports now, by USB id: sysfs's ttys where the kernel
    has it, else pyserial's; never a guessed path."""
    if not has_sysfs(env):
        try:
            from serial.tools import list_ports
        except ImportError:
            return []
        return [UnitPort(p.device, getattr(p, "serial_number", None), getattr(p, "product", None)) for p in list_ports.comports() if (p.vid, p.pid) == tuple(CONSOLE_ID)]
    out = []
    for d in sorted(pathlib.Path(env.get("CHIMERA_SYSFS", "/sys"), "class/tty").glob("*")):
        usb = (d / "device").resolve().parent  # the interface's parent: the USB device
        try:
            ids = UsbId(int((usb / "idVendor").read_text(), 16), int((usb / "idProduct").read_text(), 16))
        except (OSError, ValueError):
            continue  # a virtual console, or a device leaving mid-read
        if ids != CONSOLE_ID:
            continue
        serial, product = (_read(usb / f) for f in ("serial", "product"))
        out.append(UnitPort("/dev/" + d.name, serial, product))
    return out


def unit_serial(t: Target, env) -> Optional[str]:
    """The serial of the unit `t` talks to now, so the wait after the flash
    knows it among others; None for a tcp target or when it can't tell."""
    if isinstance(t, Tcp):
        return None
    ports = unit_ports(env)
    if "CHIMERA_USB" in env:
        ports = [p for p in ports if os.path.realpath(p.path) == os.path.realpath(t.path)]
    return ports[0].serial if len(ports) == 1 else None


class Cause(enum.Enum):
    """Why the unit hadn't answered after the flash, at the last try."""

    OFF_BUS = enum.auto()
    IN_DFU = enum.auto()
    NO_PORT = enum.auto()
    NO_ACCESS = enum.auto()
    PORT_FAULT = enum.auto()
    NO_ANSWER = enum.auto()
    BUSY = enum.auto()
    WENT_AWAY = enum.auto()
    SEVERAL = enum.auto()


class NotBack(Exception):
    """The console didn't answer after the flash: the last try's cause,
    the port it was on, and for SEVERAL the units' ports."""

    def __init__(self, cause: Cause, port: Optional[str], ports: tuple = ()):
        super().__init__(cause, port, ports)
        self.cause, self.port, self.ports = cause, port, ports

    def reason(self) -> str:
        port = self.port or "its port"
        return {
            Cause.OFF_BUS: "its console is not on USB",
            Cause.IN_DFU: "it is still in its USB loader",
            Cause.NO_PORT: "its console is on USB but its port hasn't appeared",
            Cause.NO_ACCESS: "no access to %s" % port,
            Cause.PORT_FAULT: "%s won't open as a serial port" % port,
            Cause.NO_ANSWER: "no answer from %s" % port,
            Cause.BUSY: "%s still answers busy" % port,
            Cause.WENT_AWAY: "%s went away" % port,
            Cause.SEVERAL: "more than one unit is on USB (%s)" % ", ".join(self.ports),
        }[self.cause]

    def __str__(self) -> str:
        return self.reason()


class Back(NamedTuple):
    """The unit back from the flash: the port it answered on, and its status."""

    target: Target
    status: Status
    body: bytes


BACK_POLL_S = 0.25
BACK_FIRST_MIN_S = 0.05  # the shortest wait for a first byte, near the deadline
_clock, _sleep = time.monotonic, time.sleep  # the wait's; tests make them fake


def off_cause(env) -> Cause:
    """Why no console port is listed: the console not yet a tty, the ROM
    loader on the bus, or nothing."""
    ids = usb_devices(env, dfu=False)
    if CONSOLE_ID in ids:
        return Cause.NO_PORT
    return Cause.IN_DFU if ROM_DFU_ID in ids else Cause.OFF_BUS


def pick(t: Tty, env, serial: Optional[str]) -> Optional[Tty]:
    """The console's port, by USB id (and `serial`, when known): CHIMERA_USB's
    once it is the console's, else the only one listed; None while there is
    none. Raises NotBack(SEVERAL) when it can't tell units apart."""
    ports = [p for p in unit_ports(env) if serial is None or p.serial == serial]
    if "CHIMERA_USB" in env:
        named = os.path.realpath(t.path)
        return t if any(os.path.realpath(p.path) == named for p in ports) else None
    if len(ports) > 1:
        raise NotBack(Cause.SEVERAL, None, tuple(p.path for p in ports))
    return Tty("tty", ports[0].path) if ports else None


def back_after_flash(t: Target, env, secs, serial: Optional[str] = None, clock=None, sleep=None) -> Back:
    """The unit once back from the flash: its console's port (by its USB
    id, the one `serial` names when known: never a guessed path), open
    (udev makes the node and its access a moment after the kernel lists
    the device) and answering `status` (the first boot installs first;
    `ERR busy` waits too). The port is picked when there is none and
    again only when it goes. Raises NotBack after `secs`."""
    clock, sleep = clock or _clock, sleep or _sleep
    deadline = clock() + secs
    cause, here = Cause.OFF_BUS, None
    while True:
        if isinstance(t, Tcp):
            here = t
        elif here is None:
            here = pick(t, env, serial)
            if here is None:
                cause = off_cause(env)
        if here is not None:
            try:
                conn = Conn(here)
            except PermissionError:
                cause = Cause.NO_ACCESS
            except FileNotFoundError:
                cause, here = Cause.NO_PORT, None
            except (OSError, TermiosError):
                cause = Cause.PORT_FAULT
            else:
                try:
                    send(conn, "status")
                    status, body = answer(conn, max(BACK_FIRST_MIN_S, min(SILENCE_S, deadline - clock())))
                    if not is_busy(status, body):
                        return Back(here, status, body)
                    cause = Cause.BUSY
                except NoAnswer:
                    cause = Cause.NO_ANSWER
                except Vanished:
                    cause, here = Cause.WENT_AWAY, None
                finally:
                    conn.close()
        left = deadline - clock()
        if left <= 0:
            raise NotBack(cause, here and describe(here))
        sleep(min(BACK_POLL_S, left))


STATUS_CMD = "python3 tools/chimera-usb.py status"


def not_back(pkg: Package, e: NotBack, secs) -> str:
    """What a tester does when the unit doesn't answer after the flash, by
    what its screen shows: never a power-off while it installs, never a
    command that deletes anything."""
    text = (
        "update: flashed, but the unit hadn't answered %g s later: %s.\n"
        "What to do depends on the unit's screen:\n"
        "  - DON'T POWER OFF: it is still installing. Leave it on and wait until the screen changes.\n"
        "  - Its usual screen: run\n"
        "        %s\n"
        "    The update is done if that shows `build %s` and `bank2 open ... seal=%s`.\n"
        "    If it shows anything else, run the same update command again.\n"
        "    If it doesn't answer, switch the unit off and on, only if the screen is NOT showing DON'T POWER OFF,\n"
        "    then run status again.\n"
        "  - Blank or dark: it is waiting in its USB loader. Unplug the USB cable, plug it back in,\n"
        "    and run the same update command again.\n"
        "With more than one unit on USB, name this one's port: CHIMERA_USB=<port> %s"
        % (secs, e.reason(), STATUS_CMD, pkg.version, pkg.seal, STATUS_CMD)
    )
    if e.cause is Cause.NO_ACCESS:
        text += "\nNo access to the port: " + ACCESS_HINT
    return text


def stage(t: Target, env, pkg: Package):
    """The .bk2 onto the card under the running firmware: True staged,
    False for firmware from before bank 2 (it refuses the path), else the
    exit code."""
    rc = wait_console(env, float(env.get("CHIMERA_DFU_WAIT", "10")))
    if rc:
        return rc
    conn = open_conn(t)
    try:
        # The flash restarts the unit, and an install after it refuses
        # unsaved changes: they stop the update first (firmware too old to
        # say goes on).
        try:
            status, body = request(conn, "status")
        except NoAnswer:
            status, body = Status.ERR, b""
        if status is Status.OK and b"state MODIFIED" in body.splitlines():
            return fail("update: the project has unsaved changes: save it (or load another), then run update again; nothing flashed")
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
    if old_put_refusal(bytes(body), len(pkg.bk2)):
        print("this firmware can't take the .bk2 yet: it goes after the flash", file=sys.stderr)
        return False
    # A newer firmware judges an older format's .bk2 (0.3's, going back)
    # not a bank-2 image: the .bin it pairs with takes it after the flash.
    if b"not a bank-2 image" in body and int.from_bytes(pkg.bk2[BK2_FORMAT], "little") != BK2_FORMAT_NOW:
        print("this firmware can't take an older .bk2: it goes after the flash", file=sys.stderr)
        return False
    return fail("%s: nothing flashed" % body.decode(errors="replace"))


def updated(t: Target, pkg: Package, got: Optional[Back] = None) -> int:
    """0 once `status` (`got`'s, else asked now) shows the new firmware
    running (its build line's hash, the .bk2 version's) and `pkg`'s seal
    open, else what it shows and 1. The seal, not bank 2's version: a
    release whose .bk2 didn't change keeps the one installed before it,
    and its version."""
    want = "bank2 open %s seal=%s" % (pkg.version, pkg.seal)
    if got is None:
        conn = open_conn(t)
        try:
            got = Back(t, *request(conn, "status"))
        except NoAnswer:
            return fail("update: flashed, but no status from %s" % describe(t))
        except Vanished:
            return fail("update: flashed, but %s went away" % describe(t))
        finally:
            conn.close()
    status, body = got.status, got.body
    text = body.decode(errors="replace")
    build = next((ln for ln in text.splitlines() if ln.startswith("build ")), "")
    hash_ = pkg.version.split()[-1:]
    if status is Status.OK and (not hash_ or build.rstrip("+").split()[-1:] != hash_):
        return fail("update: flashed, but status says %r: the new firmware (%s) isn't the one running" % (build or text.strip(), pkg.version))
    got = next((ln for ln in text.splitlines() if ln.startswith("bank2 ")), text.strip())
    foreign = status is Status.OK and got.startswith("bank2 slot foreign ") and foreign_slot(got)
    if foreign:
        fail("update: flashed, but " + foreign)
        return 2
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
        serial = unit_serial(t, env)  # the unit before the flash, to know it after
        rc = dfu_flash(t, env, str(b))
        if rc:
            return rc
        try:
            back = back_after_flash(t, env, UPDATE_BOOT_WAIT_S, serial)
        except NotBack as e:
            return fail(not_back(pkg, e, UPDATE_BOOT_WAIT_S))
        if staged:
            return updated(back.target, pkg, back)
        # The port it came back on carries on: the install, then status again
        # (after the restart an install that opens the door ends in).
        rc, again = install_bk2(back.target, env, str(k), "update: flashed, but ")
        if rc:
            return rc
    return updated(again.target, pkg, again) if again else updated(back.target, pkg)


def card_wait(t: Target, env, verbs) -> int:
    """`card dma <verbs>`, then its report every `CARD_POLL_S` until the
    result: printed, exit 0. Progress goes to stderr; any ERR exits 1."""
    poll = float(env.get("CHIMERA_POLL_S", CARD_POLL_S))
    conn = open_conn(t)
    try:
        send(conn, "card dma " + " ".join(verbs))
        first = Reader(conn, CARD_WAIT_S).line()
        if not first.startswith(b"OK started"):
            return fail(first.decode(errors="replace"))
        print(first.decode(errors="replace"), file=sys.stderr)
        while True:
            time.sleep(poll)
            send(conn, "card dma report")
            # A pass may hold the console a while (a B6 save, a B5 write).
            status, body = answer(conn, CARD_WAIT_S)
            if status is Status.ERR:
                return fail(body.decode(errors="replace"))
            if not body.startswith(b"running "):
                sys.stdout.buffer.write(body)
                return 0
            print(body.decode(errors="replace").strip(), file=sys.stderr)
    except NoAnswer:
        return fail("no answer from %s" % describe(t))
    except Vanished:
        return fail("%s went away" % describe(t))
    finally:
        conn.close()


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
    if argv[0] == "card-wait":
        return card_wait(t, env, argv[1:]) if len(argv) > 1 else fail("card-wait takes <bench> [N|kind]")
    if argv[0] == "tuning":
        if len(argv) not in (2, 3) or not argv[1].lower().endswith(".scl"):
            return fail("tuning takes <file.scl> [<file.kbm>]")
        stem = os.path.splitext(os.path.basename(argv[1]))[0].upper()
        if not re.fullmatch(r"[A-Z0-9]{1,8}", stem):
            return fail("tuning: %s is not an 8.3 name: rename it to 1-8 of A-Z and 0-9" % stem)
        for local, ext in zip(argv[1:], ("SCL", "KBM")):
            rc = put_file(t, local, "%s%s.%s" % (TUNINGS, stem, ext))
            if rc:
                return rc
        return 0
    if argv[0] == "get" and len(argv) == 3:
        return get_file(t, argv[1], argv[2])
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
