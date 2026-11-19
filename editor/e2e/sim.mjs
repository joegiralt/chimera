// End to end: the editor's protocol against the simulator, over the bridge.
// `just editor-sim` first; then `just editor-e2e`. Node 22, no dependencies.
// Exits 1 on any mismatch, naming the address and the step.

const URL = process.env.ED_URL ?? "ws://127.0.0.1:7342";
const TIMEOUT_MS = 5000; // one answer
const SETTLE_MS = 4000; // a queued set or a key's effect to show in the values

const failures = [];
const skipped = [];
const fail = (where, step, msg) => {
  failures.push(`${where} [${step}]: ${msg}`);
  console.log(`FAIL ${where} [${step}]: ${msg}`);
};
const info = (m) => console.log(m);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// ---- the wire: one line a message, one command out, lines until OK / ERR ----

let ws;
let inbox = [];
let waiter = null;

// ED_SERIAL=/dev/ttyACM0: the unit's console instead of the bridge.
async function connectSerial(path) {
  const { execFileSync } = await import("node:child_process");
  const fs = await import("node:fs");
  execFileSync("stty", ["-F", path, "raw", "-echo", "115200"]);
  const fd = fs.openSync(path, "r+");
  let buf = "";
  const tty = await import("node:tty"); // a tty handle, not a pool read: exit isn't held
  new tty.ReadStream(fd).on("data", (d) => {
    buf += d.toString("utf8");
    let i;
    while ((i = buf.indexOf("\n")) >= 0) {
      inbox.push(buf.slice(0, i).replace(/\r$/, ""));
      buf = buf.slice(i + 1);
      if (waiter) waiter(true);
    }
  });
  ws = { send: (c) => fs.writeSync(fd, c + "\n"), close: () => {} }; // the script's exit closes the fd
}

function connect() {
  if (process.env.ED_SERIAL) return connectSerial(process.env.ED_SERIAL);
  return new Promise((resolve, reject) => {
    ws = new WebSocket(URL);
    ws.onopen = () => resolve();
    ws.onerror = () =>
      reject(new Error(`no bridge at ${URL}: run \`just editor-sim\` first`));
    ws.onclose = () => {
      if (waiter) waiter(null);
    };
    ws.onmessage = (e) => {
      inbox.push(String(e.data).replace(/\r$/, ""));
      if (waiter) waiter(true);
    };
  });
}

const terminal = (l) => l === "OK" || l.startsWith("OK ") || l.startsWith("ERR");

/** Send `cmd`; its lines up to and including the OK / ERR that ends them. */
async function ask(cmd) {
  inbox = [];
  ws.send(cmd);
  const lines = [];
  const until = Date.now() + TIMEOUT_MS;
  for (;;) {
    while (inbox.length) {
      const l = inbox.shift();
      lines.push(l);
      if (terminal(l)) return lines;
    }
    const left = until - Date.now();
    if (left <= 0) throw new Error(`no answer to \`${cmd}\` in ${TIMEOUT_MS} ms`);
    await new Promise((r) => {
      const t = setTimeout(() => ((waiter = null), r()), left);
      waiter = () => (clearTimeout(t), (waiter = null), r());
    });
  }
}

/** `ask`, but an `ERR busy` is the queue being full: retry. */
async function send(cmd) {
  for (let i = 0; i < 50; i++) {
    const got = await ask(cmd);
    if (got.at(-1) !== "ERR busy") return got;
    await sleep(100);
  }
  throw new Error(`\`${cmd}\` stayed busy`);
}

// ---- the grammar (Protocol.elm): words, a "quoted" run is one ----

function tokens(s) {
  const out = [];
  const re = /"((?:[^"]|"(?! |$))*)"(?= |$)|(\S+)/g;
  for (let m; (m = re.exec(s)); ) out.push(m[1] ?? m[2]);
  return out;
}

function parseVal(line) {
  const t = tokens(line);
  if (t.length !== 5 || t[0] !== "val") return null;
  return { addr: t[1], norm: Number(t[2]), stored: Number(t[3]), text: t[4] };
}

const near = (a, b) => Math.abs(a - b) <= 1e-4 * Math.max(1, Math.abs(b)) + 1e-6;

/** `ed get`'s value of `addr`. */
async function get(addr) {
  const got = await ask(`ed get ${addr}`);
  const v = got.length === 2 ? parseVal(got[0]) : null;
  if (!v || v.addr !== addr) throw new Error(`ed get ${addr}: ${got.join(" | ")}`);
  return v;
}

/** `ed get` until `ok(value)`, or the settle time: the last value seen. */
async function settle(addr, ok) {
  const until = Date.now() + SETTLE_MS;
  for (;;) {
    const v = await get(addr);
    if (ok(v) || Date.now() > until) return v;
    await sleep(40);
  }
}

// ---- `since`: the generation a client holds ----

let gen = 0;
/** `ed since <gen>` : its lines, the generation held moves on. An answer
 * that ends `OK <m> more` owes sections: asked again at once, its lines
 * joined (ADR 0289). */
async function since(n = gen) {
  const lines = [];
  for (let ask_ = n; ; ) {
    const got = await ask(`ed since ${ask_}`);
    const end = got.at(-1);
    const m = /^OK (\d+)( more)?$/.exec(end);
    if (!m) throw new Error(`ed since ${ask_}: ${got.join(" | ")}`);
    gen = Number(m[1]);
    lines.push(...got.slice(0, -1));
    if (!m[2]) return lines;
    ask_ = gen;
  }
}

/** `since` until a line matches, or the settle time: every line seen meanwhile. */
async function sinceUntil(pred) {
  const seen = [];
  const until = Date.now() + SETTLE_MS;
  for (;;) {
    seen.push(...(await since()));
    if (seen.some(pred) || Date.now() > until) return seen;
    await sleep(40);
  }
}

async function key(k) {
  const got = await ask(`key ${k}`);
  if (got.at(-1) !== "OK") throw new Error(`key ${k}: ${got.join(" | ")}`);
}
/** B1 is Part 1's page, and on it the mixer: press until `status` says Part 1's. */
async function onPart1() {
  for (let i = 0; i < 3; i++) {
    await key("B1");
    await sleep(500); // the unit's UI takes a frame or more to turn the page
    const at =(await ask("status")).find((l) => l.startsWith("at "));
    if (/^at PART 1 /.test(at ?? "")) return;
  }
  throw new Error("cannot get to a Part 1 page with B1");
}
async function turn(enc, d) {
  const got = await ask(`turn ${enc} ${d}`);
  if (got.at(-1) !== "OK") throw new Error(`turn ${enc} ${d}: ${got.join(" | ")}`);
}

/** `panel <name> <variant>` until it says `variant`, or the settle time: what it says. */
async function settleVariant(panel, variant) {
  const until = Date.now() + SETTLE_MS;
  for (;;) {
    const lines = await since(0);
    const now = lines.map(tokens).find((t) => t[0] === "panel" && t[1] === panel)?.[2];
    if (now === variant || Date.now() > until) return now;
    await sleep(40);
  }
}

// ---- describe ----

/** Every page of `engine`: its panels' variants, each with the params it
 * shows (`panel FILTER DIRTY`, then DIRTY's `param` lines). One addr can be
 * several params (FILTER.MODE's names are its TYPE's), so a param belongs
 * to its block. Pages count from 0: `page 0 ALG of 36` is the first. */
async function describeAll(engine) {
  const first = await ask(`ed describe ${engine} 0`);
  const count = Number(/^page 0 \S+ of (\d+)$/.exec(first[0])?.[1]);
  if (!(count > 0)) throw new Error(`describe ${engine} 0: ${first.join(" | ")}`);
  const blocks = []; // { panel, variant, params: [{ addr, kind, scale, min, max, opts }] }
  const params = new Map(); // every addr described anywhere
  const texts = []; // each page's lines, for the hash
  for (let n = 0; n < count; n++) {
    const got = n === 0 ? first : await ask(`ed describe ${engine} ${n}`);
    if (got.at(-1) !== "OK") {
      fail(`${engine} page ${n}`, "ed describe", got.join(" | "));
      continue;
    }
    texts.push(got.slice(0, -1));
    let block = null;
    for (const l of got) {
      const t = tokens(l);
      if (t[0] === "panel" && t.length === 3) {
        block = { panel: t[1], variant: t[2], params: [] };
        blocks.push(block);
      } else if (t[0] === "param" && t.length === 10 && block) {
        if (t[4] !== "lin" && t[4] !== "oct") fail(t[1], "describe", `scale ${t[4]}`);
        if (!["-", "m", "d", "md"].includes(t[9])) fail(t[1], "describe", `flags ${t[9]}`);
        const p = { addr: t[1], kind: t[3], scale: t[4], min: Number(t[5]), max: Number(t[6]), opts: [] };
        block.params.push(p);
        params.set(p.addr, p);
      } else if (t[0] === "param") {
        fail(t[1] ?? "?", "describe", `not a protocol 4 param line: ${l}`);
      } else if (t[0] === "opt" && t.length === 4 && block) {
        block.params.at(-1)?.opts.push([Number(t[2]), t[3]]);
      }
    }
  }
  info(`describe ${engine}: ${count} pages, ${blocks.length} panel variants, ${params.size} addresses`);
  await checkHash(engine, texts);
  return { blocks, params, count };
}

/** FNV-1a 64 of every byte `describe` wrote, as `ed schema` hashes it. */
function fnv64(text) {
  let h = 0xcbf29ce484222325n;
  for (const b of Buffer.from(text, "utf8")) h = BigInt.asUintN(64, (h ^ BigInt(b)) * 0x100000001b3n);
  return h.toString(16).padStart(16, "0");
}

/** `ed schema <S>`: its pages and the hash of describe's own bytes. */
async function checkHash(schema, texts) {
  const got = await ask(`ed schema ${schema}`);
  const t = tokens(got[0] ?? "");
  const want = fnv64(texts.map((lines) => lines.map((l) => l + "\n").join("")).join(""));
  if (t[0] !== "schema" || t[1] !== schema || Number(t[2]) !== texts.length || t[3] !== want)
    fail(schema, "ed schema", `${got.join(" | ")}: want ${texts.length} pages, ${want}`);
}

/** The variants the Part shows now. */
async function current() {
  const v = new Map();
  for (const l of await since(0)) {
    const t = tokens(l);
    if (t[0] === "panel") v.set(t[1], t[2]);
  }
  return v;
}

/** Every param of `shown` to its max, then its min (a choice by its
 * number, `#n`), read back. */
let sets = 0;
async function sweep(shown, label, done, lenient = () => false) {
  for (const p of shown) {
    const key = JSON.stringify(p);
    if (done.has(key)) continue;
    done.add(key);
    const addr = p.addr;
    let targets; // [text to send, stored it must land on, label]
    if (p.opts.length) {
      const by = [...p.opts].sort((a, b) => a[0] - b[0]);
      targets = [by.at(-1), by[0]].map(([i, name]) => [`#${i}`, i, `${name} (${i})`]);
    } else {
      targets = [p.max, p.min].map((v) => [String(v), v, String(v)]);
    }
    for (const [text, want, what] of targets) {
      const got = await send(`ed set ${addr} ${text}`);
      if (got.at(-1) !== "OK queued") {
        fail(addr, `set ${what}${label}`, got.join(" | "));
        continue;
      }
      const v = await settle(addr, (v) => near(v.stored, want));
      sets++;
      if (near(v.stored, want)) continue;
      // A DRUM knob its pad's machine lacks, or a CHOKE past 8 groups
      // (ADR 0280), is refused: the unit says so through `since`.
      if (lenient(addr)) skipped.push(`${addr} ${what}${label}: stored ${v.stored} (${v.text})`);
      else fail(addr, `set ${what}${label}`, `read back stored ${v.stored}, want ${want} (${v.text})`);
    }
  }
}

/** Part 1 to INIT `engine` through the browser keys: EDIT+B1 opens it on
 * the Part's row; the INIT rows are first, in `EngineType::PLAYABLE`'s order. */
const INIT_ROW = { ALGO: 0, MODAL: 1, VA: 2, TAPE: 3, DRUM: 4 };
async function loadInit(engine) {
  const want = `part 1 ${engine}`;
  // A full answer names the Part's engine too: it settles a load whose own lines were missed.
  await browse(INIT_ROW[engine], (l) => l === want, async () => (await since(0)).includes(want));
  await onPart1();
}

/** Load the Sound on row `row` of Part 1's browser, until a `since` line
 * passes `took` (or `settled` says the load took). The unit's UI takes its time over each key (the browser reads the
 * card before a turn lands), so each key waits, and a load that didn't take
 * is tried again from the Part's page, waiting twice as long. */
async function browse(row, took, settled = async () => false) {
  const tries = [];
  for (let wait = 250; wait <= 2000; wait *= 2) {
    await since();
    await key("B1");
    await sleep(wait);
    await key("EDIT+B1");
    await sleep(wait * 2);
    await turn(1, -64);
    await sleep(wait);
    await turn(1, -64);
    await sleep(wait);
    if (row > 0) {
      await turn(1, row);
      await sleep(wait);
    }
    await key("EDIT"); // an edited Sound asks first
    await sleep(wait * 2);
    await key("EDIT");
    const seen = await sinceUntil(took);
    if (seen.some(took) || (await settled())) return seen;
    tries.push(wait);
  }
  throw new Error(`load of row ${row}: not taken after waits of ${tries.join(", ")} ms`);
}

/** One engine end to end: INIT loaded, described, every param in the
 * variants the Part shows set and read back, a full answer only of
 * described addresses. */
let mixerAddrs = new Set();
async function enginePass(engine) {
  await loadInit(engine);
  const { blocks, params } = await describeAll(engine);
  const cur = await current();
  const shown = blocks.filter((b) => b.variant === "-" || cur.get(b.panel) === b.variant).flatMap((b) => b.params);
  // Selectors last: what the rest shows depends on them.
  const selector = (p) => /\.(KIND|MODE|MODEL|MACHINE|TYPE)$/.test(p.addr);
  const done = new Set();
  const before = sets;
  const refusable = (a) => /\.KNOB_|\.CHOKE$/.test(a);
  await sweep(shown.filter((p) => !selector(p)), ` ${engine}`, done, refusable);
  await sweep(shown.filter(selector), ` ${engine}`, done, refusable);
  const all = await since(0);
  const vals = all.map(parseVal).filter(Boolean).filter((v) => !mixerAddrs.has(v.addr));
  const extra = vals.filter((v) => !params.has(v.addr)).map((v) => v.addr);
  if (extra.length) fail(engine, "since 0", `answered but not described: ${extra.slice(0, 5).join(" ")}`);
  if (all[1] !== `part 1 ${engine}`) fail(engine, "since 0", `opens ${JSON.stringify(all.slice(0, 2))}`);
  if (engine === "TAPE" && !all.some((l) => l.startsWith("source \""))) fail(engine, "since 0", "no source line");
  info(`${engine}: ${sets - before} sets read back, a full answer of ${vals.length} vals`);
}

// ---- the run ----

async function main() {
  await connect();

  // Park on Part 1's page, whatever the sim was left on.
  await onPart1();

  // hello
  const hello = tokens((await ask("ed hello"))[0]);
  if (
    !(hello.length === 7 && hello[0] === "ed" && hello[1] === "hello" && hello[2] === "4" &&
      hello[4] === "part" && hello[5] === "1" && hello[6] === "ALGO")
  ) {
    fail("hello", "ed hello", `${hello.join(" ")}: want protocol 4, part 1 ALGO`);
    throw new Error("not on Part 1 ALGO: cannot go on");
  }
  info(`hello: protocol ${hello[2]}, build ${hello[3]}, part 1 ALGO`);

  const { blocks, params: algo, count } = await describeAll("ALGO");
  // The mixer (ADR 0289): Part 3's own cells and the buses, set and read
  // back while the box follows Part 1.
  const mixer = await describeAll("MIXER");
  const params = new Map([...algo, ...mixer.params]);
  mixerAddrs = new Set(mixer.params.keys());
  const mixerShown = mixer.blocks
    .filter((b) => b.variant === "-" || b.panel === "REVERB")
    .flatMap((b) => b.params)
    .filter((p) => /^(PART3(_SND)?|CHORUS|DELAY|REVERB|TAPE|COMP)\./.test(p.addr) && p.addr !== "REVERB.TYPE");
  await sweep(mixerShown, " MIXER", new Set(), (a) => /\.(RESERVE|MAX_VOICES)$/.test(a));

  // The matrix (ADR 0289): every source into FREQ, set, read back and removed.
  const mtx = await ask("ed describe MTX-ALGO 0");
  await checkHash("MTX-ALGO", [mtx.slice(0, -1)]);
  const srcs = mtx.map(tokens).filter((t) => t[0] === "src").map((t) => t[1]);
  if (srcs.length !== 13) fail("MTX-ALGO", "describe", `${srcs.length} sources`);
  for (const s of srcs) {
    const cell = `${s}:FILTER.CUTOFF`;
    for (const [to, want] of [["-90", "route " + cell + " -90 "], ["off", null]]) {
      const got = await ask(`ed route ${cell} ${to}`);
      if (got.at(-1) !== "OK queued") { fail(cell, `route ${to}`, got.join(" | ")); continue; }
      const until = Date.now() + SETTLE_MS;
      let ok = false;
      while (!ok && Date.now() < until) {
        const lines = await since(0);
        const line = lines.find((l) => l.startsWith(`route ${cell} `));
        ok = want ? line?.startsWith(want) : !line;
        if (!ok) await sleep(40);
      }
      if (!ok) fail(cell, `route ${to}`, "since never said so");
    }
  }
  info(`routes: ${srcs.length} sources into FREQ, set and removed`);

  // every param to its max, then its min, read back
  const done = new Set();
  const shownIn = (cur) =>
    blocks.filter((b) => b.variant === "-" || cur.get(b.panel) === b.variant).flatMap((b) => b.params);
  const start = await current();
  // The selector param goes last, and the filter once per TYPE: what FILTER.MODE
  // takes depends on it. Other panels are swept in the variant the Part shows.
  const hold = (p) => p.addr === "FILTER.KIND";
  const shown = shownIn(start);
  await sweep(shown.filter((p) => !hold(p)), "", done);
  const kinds = params.get("FILTER.KIND")?.opts.map(([, name]) => name) ?? [];
  for (const kind of kinds) {
    if (kind === start.get("FILTER")) continue;
    await send(`ed set FILTER.KIND #${params.get("FILTER.KIND").opts.find(([, n]) => n === kind)[0]}`);
    const now = await settleVariant("FILTER", kind);
    if (now !== kind) {
      fail("FILTER.KIND", `set ${kind}`, `panel FILTER is ${now}`);
      continue;
    }
    await sweep(blocks.filter((b) => b.panel === "FILTER" && b.variant === kind).flatMap((b) => b.params), ` @${kind}`, done);
  }
  await sweep(shown.filter(hold), "", done);
  info(`set/get: ${sets} sets read back, ${skipped.length} skipped`);

  // a knob: the simulator's own, seen in `since`
  await onPart1();
  await since(); // catch up
  await turn(1, 3);
  await sleep(1000); // three detents, at a hand's pace
  const knob = await sinceUntil((l) => l.startsWith("val "));
  const moved = knob.map(parseVal).filter(Boolean);
  if (!moved.length) fail("knob", "turn 1 3", "no val line in since");
  for (const m of moved) {
    if (!params.has(m.addr)) fail(m.addr, "knob", "a val for an address no page describes");
  }
  if (moved.length) {
    const last = moved.at(-1);
    const g = await settle(last.addr, (v) => near(v.stored, last.stored));
    if (!near(g.stored, last.stored)) fail(last.addr, "knob", `since said ${last.stored}, get says ${g.stored}`);
  }
  info(`knob: turn 1 3 -> ${moved.length} val lines (${moved.map((m) => m.addr).slice(0, 4).join(", ")})`);

  // Part switch
  await since();
  await key("B2");
  const sw = await sinceUntil((l) => /^part 2 \S+$/.test(l));
  if (!sw.some((l) => /^part 2 \S+$/.test(l))) fail("part", "key B2", `no "part 2 <engine>": ${sw.join(" | ")}`);
  else info(`part: key B2 -> ${sw.find((l) => l.startsWith("part "))}`);
  await onPart1();
  const back = await sinceUntil((l) => l === "part 1 ALGO");
  if (!back.includes("part 1 ALGO")) fail("part", "key B1", `no "part 1 ALGO": ${back.join(" | ")}`);

  // The editor's Part buttons: `ed part`, as the keys
  await since();
  for (const [n, want] of [[2, /^part 2 \S+$/], [1, /^part 1 ALGO$/]]) {
    const got = await ask(`ed part ${n}`);
    if (got.at(-1) !== "OK") fail("part", `ed part ${n}`, got.join(" | "));
    const seen = await sinceUntil((l) => want.test(l));
    if (!seen.some((l) => want.test(l))) fail("part", `ed part ${n}`, `no ${want}: ${seen.join(" | ")}`);
    else info(`part: ed part ${n} -> ${seen.find((l) => want.test(l))}`);
  }
  const bad = await ask("ed part 7");
  if (bad.at(-1) !== "ERR ed part takes 1-6") fail("part", "ed part 7", bad.join(" | "));

  // load a Sound through the browser keys: INIT ALGO, the top row
  await send("ed set FILTER.CUTOFF 500");
  await settle("FILTER.CUTOFF", (v) => near(v.stored, 500));
  const load = await browse(0, (l) => l.startsWith("val FILTER.CUTOFF "));
  const cut = load.map(parseVal).filter(Boolean).find((v) => v.addr === "FILTER.CUTOFF");
  if (!cut || !near(cut.stored, 20000)) fail("FILTER.CUTOFF", "load INIT ALGO", `since: ${load.slice(0, 3).join(" | ")}`);
  else if (!load.every((l) => l.startsWith("val ") || l.startsWith("panel ") || l.startsWith("part "))) {
    fail("load", "since", `unexpected lines: ${load.filter((l) => !/^(val|panel|part) /.test(l)).join(" | ")}`);
  } else info(`load: INIT ALGO -> ${load.length} lines, FILTER.CUTOFF back to ${cut.stored}`);
  await onPart1();

  // a stale number: everything
  await since();
  for (const stale of [0, gen + 1000]) {
    const all = await since(stale);
    const vals = all.map(parseVal).filter(Boolean);
    const addrs = new Set(vals.map((v) => v.addr));
    if (all[0] !== "" || all[1] !== "part 1 ALGO") fail("since", `stale ${stale}`, `first lines ${JSON.stringify(all.slice(0, 2))}: a full answer opens with an empty line`);
    if (addrs.size !== vals.length) fail("since", `stale ${stale}`, "an address twice");
    for (const a of params.keys()) if (!addrs.has(a)) fail(a, `stale ${stale}`, "missing from the full answer");
    // `since` answers only what `describe` lists.
    const extra = [...addrs].filter((a) => !params.has(a));
    if (extra.length) fail("since", `stale ${stale}`, `answered but not described: ${extra.join(" ")}`);
    const ps = all.filter((l) => l.startsWith("panel ")).map((l) => tokens(l)[1]);
    if (ps.length === 0) fail("since", `stale ${stale}`, "no panel lines");
    if (!all.includes("mixer")) fail("since", `stale ${stale}`, "no mixer section");
    info(`stale ${stale}: ${all.length} lines, ${vals.length} vals, ${ps.length} panels`);
  }
  // and a current one: nothing
  const none = await since();
  if (none.length) fail("since", "current", `${none.length} lines: ${none[0]}`);

  // every other engine (ADR 0285), then back to ALGO
  for (const engine of ["MODAL", "VA", "TAPE", "DRUM"]) {
    try {
      await enginePass(engine);
    } catch (e) {
      fail(engine, "pass", String(e.message ?? e));
    }
  }
  await loadInit("ALGO");
}

try {
  await main();
} catch (e) {
  failures.push(String(e.message ?? e));
  console.log(`FAIL ${e.message ?? e}`);
}
if (skipped.length) console.log(`read back otherwise (a refused or clamped set, ADR 0276): ${skipped.length}\n  ${skipped.slice(0, 20).join("\n  ")}`);
try {
  ws?.close();
} catch {}
console.log(failures.length ? `\n${failures.length} FAILED` : "\nall passed");
process.exit(failures.length ? 1 : 0);
