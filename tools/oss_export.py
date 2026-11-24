# Copyright (C) 2026 Joe Giralt
# SPDX-License-Identifier: GPL-3.0-or-later

"""The public tree (ADR 0330): HEAD's files, without git history, less what
stays private, with home paths rewritten, then scanned. Any scan hit fails.
Standard library only.

    python3 -I tools/oss_export.py export <dir> [--rev REV] [--drop PREFIX ...] [--no-deny]
    python3 -I tools/oss_export.py scan <dir> [--no-deny]
    python3 -I tools/oss_export.py refs <dir>

`export` refuses a non-empty <dir> unless an earlier export wrote it (its
`.oss-export` marker). The tree is the public repo joegiralt/chimera's root
(ADR 0339): files under tools/oss-root/ land at its root, as its CI
workflow, which would run here too if it sat at .github/ in this repo. `refs` counts what points into the private tracker. The scan's deny list
is read from outside the tree (`$OSS_DENY_FILE` or
~/.config/chimera/oss-deny.txt).
"""

import io
import os
import pathlib
import re
import shutil
import subprocess
import sys
import tarfile

MARKER = ".oss-export"

# Never published: ledgers, reports, research notes, agent memory, build
# output, private factory data. docs/superpowers/ (the specs and plans) is
# published as design history (ADR 0330).
EXCLUDE_PREFIXES = (
    "reports/",
    "research_notes/",
    "target/",
    "tools/factory-sounds/private/",
    "editor/node_modules/",
    "editor/elm-stuff/",
    "editor/dist/",
)
EXCLUDE_DIRS = {".claude", ".superpowers"}
# Build output, and the private audit against stock (it grades someone else's firmware).
EXCLUDE_FILES = {"chimera.bin", "chimera.bk2", "chimera.map", "docs/parity.md"}
EXCLUDE_SUFFIXES = (".elf", ".bk2")
# Published although under an excluded prefix.
KEEP = set()
# The public repo's own files: tools/oss-root/<path> is published at <path>.
ROOT_OVERLAY = "tools/oss-root/"

HOME = re.compile(r"/home/[A-Za-z0-9._-]+(?![A-Za-z0-9._-])")


def private(path):
    """Whether `path` (repo-relative, '/'-separated) stays out."""
    if path in KEEP:
        return False
    parts = path.split("/")
    return (
        path.startswith(EXCLUDE_PREFIXES)
        or any(p in EXCLUDE_DIRS for p in parts[:-1])
        or parts[-1] in EXCLUDE_DIRS
        or path in EXCLUDE_FILES
        or path.endswith(EXCLUDE_SUFFIXES)
    )


def is_text(data):
    if b"\0" in data:
        return False
    try:
        data.decode("utf-8")
    except UnicodeDecodeError:
        return False
    return True


# Text that differs in the public tree, by path: (private, public). Each
# must be found (or its public text already there, as when the public repo
# exports itself), so a source edit can't silently drop the rewrite.
THEN = "GPL-3.0-or-later since ADR 0330"
EDITS = {
    # Records written while the workspace was MIT say so as history.
    "docs/adr/0032-modal-code-provenance.md": [
        ("while Chimera is MIT, so", f"while Chimera was then MIT (it is {THEN}), so"),
        ("relicenses it to Chimera under MIT.", f"relicenses it to Chimera under MIT (Chimera's licence then; {THEN})."),
    ],
    "docs/adr/0085-the-master-reverb-picks-a-type.md": [
        ("in an MIT workspace.", f"in what was then an MIT workspace ({THEN})."),
    ],
    "docs/adr/0095-every-reverb-type-matches-its-midiverb-ii-presets.md": [
        ("and the workspace is MIT.", f"and the workspace was then MIT ({THEN})."),
    ],
    "docs/adr/0098-chimera-utils-host-reference-tool.md": [
        ("Chimera's own\ncode is MIT.", f"Chimera's own\ncode was then MIT ({THEN})."),
    ],
    "docs/superpowers/specs/2026-10-04-patch-importer-design.md": [
        ("The repo is MIT.", f"The repo was then MIT ({THEN})."),
    ],
    "docs/superpowers/specs/2026-10-04-reverb-types-design.md": [
        ("and this workspace is MIT (`Cargo.toml`).", f"and this workspace was then MIT (`Cargo.toml`; {THEN})."),
    ],
    "CLAUDE.md": [
        (
            "GitHub issue tracker (joegiralt/chimera), not in repo files",
            "private tracker (joegiralt/chimera-private), not in repo files"
            " (public bug reports come in at joegiralt/chimera-releases' issues)",
        )
    ],
}

# The tracker stays private (ADR 0339) while the public repo takes the name
# joegiralt/chimera, so a link into it would land on the public repo's
# issues: each becomes plain text, "private issue #N" (or PR).
_TRACKER_URL = r"https?://github\.com/joegiralt/chimera/(issues|pull)/(\d+)"
TRACKER_LINK = re.compile(r"\[([^\]\n]*)\]\(" + _TRACKER_URL + r"\)")
TRACKER_URL = re.compile(r"<" + _TRACKER_URL + r">|" + _TRACKER_URL)
TRACKER_SHORT = re.compile(r"\bjoegiralt/chimera#(\d+)")


def _private(kind, n):
    return f"private {'PR' if kind == 'pull' else 'issue'} #{n}"


def _tracker_link(m):
    text, kind, n = m.groups()
    plain = _private(kind, n)
    return plain if text.strip() in (f"#{n}", n) else f"{text} ({plain})"


def untrack(text):
    """Links into the private tracker as plain text."""
    text = TRACKER_LINK.sub(_tracker_link, text)
    text = TRACKER_URL.sub(lambda m: _private(m.group(1) or m.group(3), m.group(2) or m.group(4)), text)
    return TRACKER_SHORT.sub(lambda m: _private("issues", m.group(1)), text)


# In the docs (.md) only: pages on claude.ai (private artifacts) and paths
# into private notes (~/dev/…, .superpowers/ledgers/…) go; the text around
# them stays. Code and the tools' tests say these things in their own words.
_CLAUDE_PAGE = r"https://claude\.ai/artifact/[A-Za-z0-9]+"
CLAUDE_PAREN = re.compile(r"[ \t]*\n?[ \t]*\(" + _CLAUDE_PAGE + r"\)")
CLAUDE_LEAD = re.compile(r"\(" + _CLAUDE_PAGE + r",\s*")
CLAUDE_BARE = re.compile(_CLAUDE_PAGE)
_NOTE_AT = r"(?:~/dev/|(?:[\w.-]+/)*\.superpowers/ledgers\b/?)"
_NOTE_PATH = _NOTE_AT + r"[^\s`)\]|\"']*"
# The machine's build-slot wrapper: dropped from a command, which runs as is.
BUILD_SLOT = re.compile(r"[ \t]*~/dev/build-slot\.sh(?=`)|~/dev/build-slot\.sh[ \t]+")
NOTE_ENV = re.compile(r"\b([A-Z][A-Z0-9_]*=)(?:…/)?" + _NOTE_PATH)
NOTE_TICKED = re.compile(r"`" + _NOTE_AT + r"[^`\s]*`")
NOTE_PAREN = re.compile(r"\(`?" + _NOTE_PATH + r"`?(?=[),;\s])")
NOTE_BARE = re.compile(_NOTE_PATH)


def _note(m):
    tail = re.search(r"[.,;:]+$", m.group(0))
    return "(private notes)" + (tail.group(0) if tail else "")


def unprivate_doc(text):
    """A doc's claude.ai pages and private-note paths, as plain text: a
    command's build-slot wrapper goes, a variable set to a private dir reads
    `<dir>`, and a path reads "(private notes)", once in parentheses."""
    text = CLAUDE_PAREN.sub("", text)
    text = CLAUDE_LEAD.sub("(", text)
    text = CLAUDE_BARE.sub("an unpublished page", text)
    text = BUILD_SLOT.sub("", text)
    text = NOTE_ENV.sub(r"\1<dir>", text)
    text = NOTE_PAREN.sub("(private notes", text)
    text = NOTE_TICKED.sub("(private notes)", text)
    return NOTE_BARE.sub(_note, text)


# In the plans only: the agent banner each opens with, and the rule against
# attribution in commit messages (the README says Chimera is AI-assisted).
BANNER = re.compile(r"(?m)^> (?:\*\*)?For agentic workers:.*\n(?:[ \t]*\n)?")
_ATTR_END = r"(?:attribution|co-authored-by|claude|lines?|trailer|anywhere|ai)"
ATTR_CLAUSE = re.compile(
    r"(?i)\*{0,2}\b(?:no|never)\b(?:(?!\bno\b|\bnever\b)[^.;:,(])*?\b(?:attribution|co-authored-by|claude)\b`?"
    r"(?:(?:[ /`]|\bor\b|\band\b|\bother\b|\bany\b)*\b" + _ATTR_END + r"\b`?)*(?:\*{2}|\.\*{2})?"
)
ATTR_RULE = re.compile(r"(?i)\b(?:no|never)\b[^.;\n]*\b(?:attribution|co-authored)")
CUT = "\0"


def _unattribute(line):
    if not ATTR_RULE.search(line):
        return line
    s = ATTR_CLAUSE.sub(CUT, line)
    s = re.sub(r"\(\s*\0\s*\)", CUT, s)
    s = re.sub(r"(?:,\s*|\s+and\s+|\s+)\0", CUT, s)
    s = re.sub(r"\0,?\s*and\s+", ", ", s)
    s = s.replace(CUT, "")
    s = re.sub(r"\s+([.;,])", r"\1", s)
    s = re.sub(r",\s*\.", ".", s)
    s = re.sub(r"(?:,| and)\s*$", "", s)
    s = re.sub(r"\.\s*\.", ".", s)
    return s


def unagent_plan(text):
    """A plan without its agent banner and attribution rule."""
    text = BANNER.sub("", text)
    return "\n".join(_unattribute(l) for l in text.split("\n"))


def rewrite(data, path=""):
    """`/home/<user>` becomes `~` in a text file, links into the tracker
    plain text, a doc's private pages and paths go, and `path`'s EDITS
    apply; binaries pass."""
    if not is_text(data):
        return data
    text = HOME.sub("~", data.decode("utf-8"))
    text = untrack(text)
    if path.endswith(".md"):
        text = unprivate_doc(text)
    if path.startswith("docs/superpowers/plans/") and path.endswith(".md"):
        text = unagent_plan(text)
    for old, new in EDITS.get(path, []):
        if old not in text and new not in text:
            sys.exit(f"{path}: the public edit no longer matches {old!r}")
        text = text.replace(old, new)
    return text.encode("utf-8")


def public_path(path):
    """Where `path` lands in the public tree: its own path, or the root for
    tools/oss-root/'s files."""
    return path[len(ROOT_OVERLAY) :] if path.startswith(ROOT_OVERLAY) else path


def dropped(path, drop):
    """Whether `path` is under a dropped part, by whole path segments."""
    return any(path == d.strip("/") or path.startswith(d.strip("/") + "/") for d in drop)


def export(repo, dest, rev="HEAD", drop=()):
    """Write `rev`'s public files into `dest`, less any path under a `drop`
    prefix (a part that lives elsewhere in the public home, as `editor/`);
    returns the paths written."""
    dest = pathlib.Path(dest)
    if dest.exists() and any(dest.iterdir()):
        if not (dest / MARKER).is_file():
            sys.exit(f"{dest} is not empty and holds no {MARKER}: refusing")
        shutil.rmtree(dest)
    dest.mkdir(parents=True, exist_ok=True)
    tar = subprocess.run(
        ["git", "-C", str(repo), "archive", "--format=tar", rev],
        check=True,
        capture_output=True,
    ).stdout
    written = []
    with tarfile.open(fileobj=io.BytesIO(tar)) as t:
        members = [m for m in t.getmembers() if m.isfile() and not private(m.name) and not dropped(m.name, drop)]
        names = {m.name for m in members}
        for m in members:
            name = public_path(m.name)
            if name != m.name and name in names:
                sys.exit(f"{m.name} would overwrite {name}")
            out = dest / name
            out.parent.mkdir(parents=True, exist_ok=True)
            out.write_bytes(rewrite(t.extractfile(m).read(), name))
            out.chmod(0o755 if m.mode & 0o111 else 0o644)
            written.append(name)
    (dest / MARKER).write_text(f"exported from {rev}\n")
    return written


# --- The scan -------------------------------------------------------------

# Addresses that may appear: upstream authors' notices and package metadata.
EMAIL_OK = {
    "emilie.o.gillet@gmail.com",
    "oneill@pcg-random.org",
    "i@izs.me",
    "ian@cygnus.com",
    "noreply@anthropic.com",
}
EMAIL = re.compile(r"\b[A-Za-z0-9._%+-]+@[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)*\.[A-Za-z]{2,}\b")

# Words that must never appear, one a line, from a file kept outside the
# tree: $OSS_DENY_FILE, else ~/.config/chimera/oss-deny.txt. Without one
# the scan fails unless --no-deny is given. Matched against every
# lowercased token and its pieces.
DENY_FILE = pathlib.Path.home() / ".config/chimera/oss-deny.txt"


def deny_words(path):
    p = pathlib.Path(path)
    if not p.is_file():
        return frozenset()
    lines = (l.strip().lower() for l in p.read_text(encoding="utf-8").splitlines())
    return frozenset(l for l in lines if l and not l.startswith("#"))


def require_deny_words(env=None, default=DENY_FILE, allow_none=False):
    """The deny list, or exit naming the path tried: `$OSS_DENY_FILE` (`env`)
    must exist when set; with no list at all only `allow_none` (`--no-deny`)
    goes on, and says so."""
    if env:
        if not pathlib.Path(env).is_file():
            sys.exit(f"scan: OSS_DENY_FILE={env} does not exist")
        return deny_words(env)
    if pathlib.Path(default).is_file():
        return deny_words(default)
    if not allow_none:
        sys.exit(f"scan: no deny list at {default} (set OSS_DENY_FILE, or pass --no-deny to skip that rule)")
    print(f"scan: --no-deny: no deny list at {default}, that rule skipped", file=sys.stderr)
    return frozenset()


TOKEN = re.compile(r"[a-z0-9][a-z0-9._%+@-]*")
PIECE = re.compile(r"[._%+@-]")

PATTERNS = {
    "home path": re.compile(r"/home/[A-Za-z0-9._-]+|/Users/[A-Za-z][A-Za-z0-9._-]*/|[Cc]:\\\\?Users\\\\?[A-Za-z]"),
    "private key": re.compile(r"-{5}BEGIN [A-Z0-9 ]*PRIVATE KEY-{5}"),
    "GitHub token": re.compile(r"\b(?:gh[pousr]_[A-Za-z0-9]{36,}|github_pat_[A-Za-z0-9_]{22,})"),
    "AWS key": re.compile(r"\b(?:AKIA|ASIA)[0-9A-Z]{16}\b"),
    "Slack token": re.compile(r"\bxox[abprs]-[A-Za-z0-9-]{10,}"),
    "API key": re.compile(r"\bsk-(?:ant-)?[A-Za-z0-9_-]{24,}|\bAIza[0-9A-Za-z_-]{35}\b"),
    "secret assignment": re.compile(
        r"(?i)\b(?:api[_-]?key|secret|passw(?:or)?d|auth[_-]?token|access[_-]?token)\b\s*[:=]\s*[\"'][^\"'\s]{12,}[\"']"
    ),
    "private address": re.compile(r"\b(?:192\.168|10\.\d{1,3}|172\.(?:1[6-9]|2\d|3[01]))\.\d{1,3}\.\d{1,3}\b"),
    "local hostname": re.compile(r"(?:@|//)[A-Za-z0-9-]+\.(?:local|lan|home\.arpa)\b"),
    "agent path": re.compile(r"/tmp/claude[-]"),
    "flattened home path": re.compile(r"-home-[a-z]"),
}

# Audio, banks and dumps: never published. WAVs only as tiny fuzz seeds.
DATA_SUFFIXES = (".syx", ".bnk", ".mix", ".aif", ".aiff", ".flac", ".mp3", ".ogg", ".m4a", ".zip", ".7z")
SEED_WAV = re.compile(r"(^|/)fuzz/seeds/[^/]+/[^/]+\.wav$")
SEED_WAV_MAX = 4096


def denied(text, words):
    if not words:
        return False
    for tok in TOKEN.findall(text.lower()):
        if tok in words or any(w in words for w in PIECE.split(tok)):
            return True
    return False


STRINGS = re.compile(rb"[\x20-\x7e]{6,}")


def strings(data):
    """A binary's printable runs, as `strings` finds them."""
    return [m.group(0).decode("ascii") for m in STRINGS.finditer(data)]


def scan_file(path, data, words=frozenset()):
    """The hits in one file: (rule, detail) pairs."""
    hits = []
    low = path.lower()
    if low.endswith(DATA_SUFFIXES):
        hits.append(("audio or bank data", path))
    if low.endswith(".wav") and not (SEED_WAV.search(path) and len(data) <= SEED_WAV_MAX):
        hits.append(("audio", path))
    if len(data) >= 2 and data[0] == 0xF0 and data.rstrip(b"\0")[-1:] == b"\xf7":
        hits.append(("sysex dump", path))
    if denied(path, words):
        hits.append(("denied word", path))
    lines = data.decode("utf-8").splitlines() if is_text(data) else strings(data)
    for n, line in enumerate(lines, 1):
        for e in EMAIL.findall(line):
            if e.lower() not in EMAIL_OK:
                hits.append(("email", f"{path}:{n}: {e}"))
        for rule, rx in PATTERNS.items():
            m = rx.search(line)
            if m:
                hits.append((rule, f"{path}:{n}: {m.group(0)[:60]}"))
        if denied(line, words):
            hits.append(("denied word", f"{path}:{n}"))
    return hits


def walk(root):
    root = pathlib.Path(root)
    for p in sorted(root.rglob("*")):
        if p.is_file() and p.name != MARKER:
            yield p.relative_to(root).as_posix(), p.read_bytes()


def scan(root, words=None):
    """Every hit in the tree at `root`."""
    words = require_deny_words(os.environ.get("OSS_DENY_FILE"), DENY_FILE, True) if words is None else words
    return [h for path, data in walk(root) for h in scan_file(path, data, words)]


# --- What points into the private tracker (reported, never failed) -------

REFS = {
    "private issue/PR URLs": re.compile(r"github\.com/joegiralt/chimera/(?:issues|pull)/\d+"),
    "claude.ai pages": re.compile(r"claude\.ai/artifact/"),
    "private-note paths": re.compile(r"~/dev/|\.superpowers/ledgers"),
    "bare #nnn references": re.compile(r"(?<![\w/&])#\d{2,4}\b"),
    '"the owner"': re.compile(r"(?i)\bthe owner\b|\bthe owner's\b"),
    "ADR references": re.compile(r"\bADRs? \d{4}"),
}


def refs(root):
    counts = {k: [0, 0] for k in REFS}  # mentions, files
    for path, data in walk(root):
        if not is_text(data):
            continue
        text = data.decode("utf-8")
        for k, rx in REFS.items():
            n = len(rx.findall(text))
            if n:
                counts[k][0] += n
                counts[k][1] += 1
    return counts


def main(argv):
    if len(argv) < 2 or argv[0] not in ("export", "scan", "refs"):
        sys.exit(__doc__)
    cmd, dest = argv[0], argv[1]
    if cmd == "export":
        rev = argv[argv.index("--rev") + 1] if "--rev" in argv else "HEAD"
        drop = [argv[i + 1] for i, a in enumerate(argv) if a == "--drop"]
        repo = pathlib.Path(__file__).resolve().parent.parent
        files = export(repo, dest, rev, drop)
        size = sum((pathlib.Path(dest) / f).stat().st_size for f in files)
        print(f"exported {len(files)} files, {size} B, to {dest}")
    if cmd == "refs":
        for k, (n, f) in refs(dest).items():
            print(f"{k}: {n} in {f} files")
        return 0
    words = require_deny_words(os.environ.get("OSS_DENY_FILE"), DENY_FILE, "--no-deny" in argv)
    if words:
        print(f"scan: deny list {len(words)} words", file=sys.stderr)
    hits = scan(dest, words)
    for rule, where in hits:
        print(f"scan: {rule}: {where}", file=sys.stderr)
    if hits:
        print(f"scan: {len(hits)} hits: not publishable", file=sys.stderr)
        return 1
    print("scan: clean")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
