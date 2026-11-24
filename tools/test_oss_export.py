# Copyright (C) 2026 Joe Giralt
# SPDX-License-Identifier: GPL-3.0-or-later

"""oss_export leaves the private paths out, rewrites home paths, and its scan
fails on each kind of leak; HEAD's export scans clean (standard library
only). Every planted secret is built at run time, so this file scans clean
itself."""

import importlib.util
import pathlib
import subprocess
import tempfile
import unittest

spec = importlib.util.spec_from_file_location(
    "oss_export", pathlib.Path(__file__).with_name("oss_export.py")
)
oss = importlib.util.module_from_spec(spec)
spec.loader.exec_module(oss)

ROOT = pathlib.Path(__file__).resolve().parent.parent
AT = "@"


def rules(path, data):
    return {r for r, _ in oss.scan_file(path, data)}


def rules_with(path, data, words):
    return {r for r, _ in oss.scan_file(path, data, words)}


class Private(unittest.TestCase):
    def test_private_paths_stay_out(self):
        for p in (
            "reports/a.md",
            "research_notes/n.md",
            ".claude/projects/m/MEMORY.md",
            ".claude/settings.json",
            "a/.superpowers/ledgers/x.txt",
            "chimera.bin",
            "target/chimera.bk2",
            "tools/factory-sounds/private/p.toml",
        ):
            self.assertTrue(oss.private(p), p)

    def test_public_paths_go_out(self):
        for p in (
            "README.md",
            "LICENSE",
            "chimera-engine/src/lib.rs",
            "docs/adr/0330-open-the-source-under-gplv3-with-0-4.md",
            "docs/superpowers/specs/2026-10-02-usb-console-design.md",
            "docs/superpowers/plans/2026-10-02-usb-console.md",
            "chimera-desktop/fuzz/seeds/read_wav/float32.wav",
        ):
            self.assertFalse(oss.private(p), p)

    def test_every_kept_file_exists(self):
        for p in oss.KEEP:
            self.assertTrue((ROOT / p).is_file(), p)


class Rewrite(unittest.TestCase):
    def test_home_paths(self):
        h = "/" + "home/someone"
        got = oss.rewrite(f"a {h}/dev/x and `{h}` and /home/|x\n".encode())
        self.assertEqual(got, b"a ~/dev/x and `~` and /home/|x\n")

    def test_binaries_pass(self):
        b = b"\0/" + b"home/someone/x"
        self.assertEqual(oss.rewrite(b), b)


class Scan(unittest.TestCase):
    def test_clean_text(self):
        self.assertEqual(rules("a.rs", b"fn main() {}\n// emilie.o.gillet@gmail.com\n"), set())

    def test_each_leak(self):
        cases = {
            "email": f"mail me{AT}example.org\n",
            "home path": "/" + "home/someone/x\n",
            "private key": "-" * 5 + "BEGIN OPENSSH PRIVATE KEY" + "-" * 5 + "\n",
            "GitHub token": "gh" + "p_" + "a1" * 18 + "\n",
            "AWS key": "AK" + "IA" + "ABCDEFGHIJKLMNOP\n",
            "Slack token": "xo" + "xb-" + "1234567890abc\n",
            "API key": "sk-" + "ant-" + "x" * 30 + "\n",
            "secret assignment": "pass" + 'word = "' + "hunter2hunter2" + '"\n',
            "private address": "192." + "168.1.20\n",
            "local hostname": "ssh pi" + AT + "studio" + ".local\n",
            "agent path": "see /tmp/" + "claude-1000/x\n",
            "flattened home path": "projects/" + "-home-" + "someone-dev-x\n",
        }
        for rule, text in cases.items():
            self.assertIn(rule, rules("f.txt", text.encode()), rule)

    def test_denied_words_from_the_deny_file(self):
        with tempfile.TemporaryDirectory() as d:
            f = pathlib.Path(d) / "deny.txt"
            f.write_text("# a comment\nZorblax\nsome-host\n")
            words = oss.deny_words(f)
            self.assertEqual(words, {"zorblax", "some-host"})
            self.assertTrue(oss.denied("met at some-host today", words))
            self.assertTrue(oss.denied("x.zorblax.com", words))
            self.assertFalse(oss.denied("some host", words))
            self.assertIn("denied word", rules_with("f.txt", b"ZORBLAX\n", words))
            self.assertEqual(oss.deny_words(pathlib.Path(d) / "absent.txt"), frozenset())

    def test_a_missing_deny_list_fails_loudly(self):
        with tempfile.TemporaryDirectory() as d:
            absent = pathlib.Path(d) / "absent.txt"
            with self.assertRaises(SystemExit) as e:
                oss.require_deny_words(env=str(absent), default=absent, allow_none=False)
            self.assertIn(str(absent), str(e.exception.code))
            # $OSS_DENY_FILE set but missing fails even with --no-deny.
            with self.assertRaises(SystemExit):
                oss.require_deny_words(env=str(absent), default=absent, allow_none=True)
            # No list anywhere: only an explicit --no-deny passes.
            with self.assertRaises(SystemExit):
                oss.require_deny_words(env=None, default=absent, allow_none=False)
            self.assertEqual(oss.require_deny_words(env=None, default=absent, allow_none=True), frozenset())
            f = pathlib.Path(d) / "deny.txt"
            f.write_text("zorblax\n")
            self.assertEqual(oss.require_deny_words(env=str(f), default=absent, allow_none=False), {"zorblax"})
            self.assertEqual(oss.require_deny_words(env=None, default=f, allow_none=False), {"zorblax"})

    def test_the_real_deny_list_when_present(self):
        words = oss.deny_words(oss.DENY_FILE)
        if not words:
            self.skipTest("no deny file: the rule is skipped")
        w = sorted(words)[0]
        self.assertIn("denied word", rules_with("f.txt", f"x {w} y\n".encode(), words))

    def test_binary_strings_are_scanned(self):
        blob = b"\x00\x01\xff" + b"/" + b"home/someone/x" + b"\x00\x02" + f"a{AT}example.org".encode() + b"\x00"
        got = rules("blob.bin", blob)
        self.assertIn("home path", got)
        self.assertIn("email", got)

    def test_data_files(self):
        self.assertIn("audio or bank data", rules("bank.syx", b"x"))
        self.assertIn("audio", rules("song.wav", b"RIFF"))
        self.assertIn("audio", rules("a/fuzz/seeds/w/big.wav", b"R" * 5000))
        self.assertEqual(rules("a/fuzz/seeds/w/tiny.wav", b"RIFF"), set())
        self.assertIn("sysex dump", rules("dump.bin", b"\xf0\x43\x00\x01\xf7"))


class Head(unittest.TestCase):
    """The tree HEAD would publish: exported and scanned clean."""

    def test_head_exports_clean(self):
        if not (ROOT / ".git").exists():
            self.skipTest("not a git checkout")
        with tempfile.TemporaryDirectory() as d:
            files = oss.export(ROOT, pathlib.Path(d) / "out")
            self.assertTrue(files)
            self.assertFalse([f for f in files if oss.private(f)])
            self.assertEqual(oss.scan(pathlib.Path(d) / "out"), [])

    def test_drop_matches_whole_segments(self):
        self.assertTrue(oss.dropped("editor/src/Main.elm", ["editor/"]))
        self.assertTrue(oss.dropped("editor/src/Main.elm", ["editor"]))
        self.assertFalse(oss.dropped("editor-kit/x", ["editor"]))
        self.assertFalse(oss.dropped("chimera-editor/x", ["editor/"]))

    def test_public_edits(self):
        (old, new), = oss.EDITS["CLAUDE.md"]
        got = oss.rewrite(f"go in the {old}; by URL".encode(), "CLAUDE.md")
        self.assertEqual(got, f"go in the {new}; by URL".encode())
        self.assertIn(b"joegiralt/chimera-private", got)
        # The public repo exports itself: the edit is already made.
        self.assertEqual(oss.rewrite(got, "CLAUDE.md"), got)
        with self.assertRaises(SystemExit):
            oss.rewrite(b"nothing to edit", "CLAUDE.md")

    def test_tracker_links_become_plain_text(self):
        # Built at run time, so the export leaves this file's input alone.
        gh = "https://github.com/joegiralt/" + "chimera"
        got = oss.rewrite(
            f"{gh}/issues/303 and {gh}/pull/1113, [#345]({gh}/issues/345), [the bug]({gh}/issues/9), "
            f"<{gh}/issues/710>, joegiralt/" f"chimera#565, joegiralt/chimera-releases#6, {gh}/tree/preenfm3-v0.4.0".encode()
        )
        self.assertEqual(
            got.decode(),
            "private issue #303 and private PR #1113, private issue #345, the bug (private issue #9), "
            f"private issue #710, private issue #565, joegiralt/chimera-releases#6, {gh}/tree/preenfm3-v0.4.0",
        )
        self.assertEqual(oss.rewrite(got), got)

    def test_docs_lose_private_pages_and_paths(self):
        page = "https://claude.ai/" + "artifact/B1ydzBo2"
        home = "~" + "/dev"
        ledgers = ".superpowers/" + "ledgers"
        doc = (
            f"mocked at 240x320\n({page}): A refined\nA/B: {page}; spike\nmockup ({page}, the panel)\n"
            f"Design: `{home}/designs/key.md`; run `{home}/build-slot.sh just check`, see {ledgers}/kobol/a.txt. Step ({ledgers}/r.md) and (`{home}/x`).\n"
            f"Its comment ({home}/x/y.md S1, S2); demo (`{home}/c/{ledgers}/fb/`, branch `x`); KOBOL_DIR=…/{ledgers}/kobol cargo test.\n"
            f"Path: `{home}/c/{ledgers}/f/<s>/r<R>_<a|b>.wav`.\n"
        )
        want = (
            "mocked at 240x320: A refined\nA/B: an unpublished page; spike\nmockup (the panel)\n"
            "Design: (private notes); run `just check`, see (private notes). Step (private notes) and (private notes).\n"
            "Its comment (private notes S1, S2); demo (private notes, branch `x`); KOBOL_DIR=<dir> cargo test.\n"
            "Path: (private notes).\n"
        )
        self.assertEqual(oss.rewrite(doc.encode(), "docs/x.md").decode(), want)
        # Code keeps its own words: only docs are rewritten.
        self.assertEqual(oss.rewrite(doc.encode(), "x.rs").decode(), doc)

    def test_plans_lose_the_agent_banner_and_attribution_rule(self):
        plan = (
            "# A plan\n\n> **For agentic workers:** REQUIRED SUB-SKILL: Use one.\n\n**Goal:** x.\n"
            "- Commits: terse, no type prefix, no Claude" " attribution. Stage by path.\n"
            "- Terse comments. No type prefixes in commits, no Claude" " attribution, and never stage `chimera.bin`.\n"
            "- Commit messages are terse. **No attribution" " or `Co-Authored-By` lines.**\n"
            "- coefficients copied with attribution.\n"
        )
        want = (
            "# A plan\n\n**Goal:** x.\n"
            "- Commits: terse, no type prefix. Stage by path.\n"
            "- Terse comments. No type prefixes in commits, never stage `chimera.bin`.\n"
            "- Commit messages are terse.\n"
            "- coefficients copied with attribution.\n"
        )
        self.assertEqual(oss.rewrite(plan.encode(), "docs/superpowers/plans/p.md").decode(), want)
        self.assertEqual(oss.rewrite(plan.encode(), "docs/superpowers/specs/p.md").decode(), plan)

    def test_mit_history_edits_match(self):
        for path, edits in oss.EDITS.items():
            text = (ROOT / path).read_text(encoding="utf-8")
            for old, new in edits:
                self.assertTrue(old in text or new in text, (path, old))

    def test_the_root_overlay_lands_at_the_root(self):
        self.assertEqual(oss.public_path("tools/oss-root/.github/workflows/ci.yml"), ".github/workflows/ci.yml")
        self.assertEqual(oss.public_path("tools/oss_export.py"), "tools/oss_export.py")

    def test_head_exports_the_public_repo_files(self):
        if not (ROOT / ".git").exists():
            self.skipTest("not a git checkout")
        with tempfile.TemporaryDirectory() as d:
            files = oss.export(ROOT, pathlib.Path(d) / "out")
            for f in ("README.md", "CONTRIBUTING.md", "LICENSE", "THIRD_PARTY.md", ".github/workflows/ci.yml"):
                self.assertIn(f, files)
            self.assertFalse([f for f in files if f.startswith(oss.ROOT_OVERLAY)])
            text = "\n".join(
                (pathlib.Path(d) / "out" / f).read_text(encoding="utf-8") for f in files if f.endswith((".md", ".rs"))
            )
            self.assertNotRegex(text, r"github\.com/joegiralt/chimera/(?:issues|pull)/\d")
            docs = "\n".join((pathlib.Path(d) / "out" / f).read_text(encoding="utf-8") for f in files if f.endswith(".md"))
            self.assertNotIn("claude.ai/" "artifact", docs)
            self.assertNotIn("~" "/dev/", docs)
            self.assertNotIn(".superpowers/" "ledgers", docs)

    def test_drop_leaves_a_prefix_out(self):
        if not (ROOT / ".git").exists():
            self.skipTest("not a git checkout")
        with tempfile.TemporaryDirectory() as d:
            files = oss.export(ROOT, pathlib.Path(d) / "out", drop=["editor/"])
            self.assertFalse([f for f in files if f.startswith("editor/")])
            self.assertIn("README.md", files)

    def test_refuses_a_foreign_dir(self):
        with tempfile.TemporaryDirectory() as d:
            (pathlib.Path(d) / "mine.txt").write_text("x")
            with self.assertRaises(SystemExit):
                oss.export(ROOT, d)
            self.assertTrue((pathlib.Path(d) / "mine.txt").is_file())


if __name__ == "__main__":
    unittest.main()
