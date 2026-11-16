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

function connect() {
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
/** `ed since <gen>` : its lines, the generation held moves on. */
async function since(n = gen) {
  const got = await ask(`ed since ${n}`);
  const end = got.at(-1);
  const m = /^OK (\d+)$/.exec(end);
  if (!m) throw new Error(`ed since ${n}: ${got.join(" | ")}`);
  gen = Number(m[1]);
  return got.slice(0, -1);
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
    const at = (await ask("status")).find((l) => l.startsWith("at "));
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

// ---- the run ----

async function main() {
  await connect();

  // Park on Part 1's page, whatever the sim was left on.
  await onPart1();

  // hello
  const hello = tokens((await ask("ed hello"))[0]);
  if (
    !(hello.length === 7 && hello[0] === "ed" && hello[1] === "hello" && hello[2] === "2" &&
      hello[4] === "part" && hello[5] === "1" && hello[6] === "ALGO")
  ) {
    fail("hello", "ed hello", `${hello.join(" ")}: want protocol 2, part 1 ALGO`);
    throw new Error("not on Part 1 ALGO: cannot go on");
  }
  info(`hello: protocol ${hello[2]}, build ${hello[3]}, part 1 ALGO`);

  // describe every page
  // Pages count from 0: `page 0 ALG of 36` is the first.
  const first = await ask("ed describe ALGO 0");
  const count = Number(/^page 0 \S+ of (\d+)$/.exec(first[0])?.[1]);
  if (!(count > 0)) throw new Error(`describe ALGO 0: ${first.join(" | ")}`);
  // A page's lines are its panels' variants, each with the params it shows:
  // `panel FILTER DIRTY`, then DIRTY's `param` lines. One addr can be several
  // params (FILTER.MODE's names are its TYPE's), so a param belongs to its block.
  const blocks = []; // { panel, variant, params: [{ addr, kind, min, max, opts }] }
  const params = new Map(); // every addr described anywhere
  for (let n = 0; n < count; n++) {
    const got = n === 0 ? first : await ask(`ed describe ALGO ${n}`);
    if (got.at(-1) !== "OK") {
      fail(`page ${n}`, "ed describe", got.join(" | "));
      continue;
    }
    let block = null;
    for (const l of got) {
      const t = tokens(l);
      if (t[0] === "panel" && t.length === 3) {
        block = { panel: t[1], variant: t[2], params: [] };
        blocks.push(block);
      } else if (t[0] === "param" && t.length === 8 && block) {
        const p = { addr: t[1], kind: t[3], min: Number(t[4]), max: Number(t[5]), opts: [] };
        block.params.push(p);
        params.set(p.addr, p);
      } else if (t[0] === "opt" && t.length === 4 && block) {
        block.params.at(-1)?.opts.push([Number(t[2]), t[3]]);
      }
    }
  }
  info(`describe: ${count} pages, ${blocks.length} panel variants, ${params.size} addresses`);

  // The variants the Part shows now.
  const current = async () => {
    const v = new Map();
    for (const l of await since(0)) {
      const t = tokens(l);
      if (t[0] === "panel") v.set(t[1], t[2]);
    }
    return v;
  };

  // every param to its max, then its min, read back
  let sets = 0;
  const done = new Set();
  async function sweep(shown, label) {
    for (const p of shown) {
      const key = JSON.stringify(p);
      if (done.has(key)) continue;
      done.add(key);
      const addr = p.addr;
      let targets; // [text to send, stored it must land on, label]
      if (p.opts.length) {
        const by = [...p.opts].sort((a, b) => a[0] - b[0]);
        targets = [by.at(-1), by[0]].map(([i, name]) => [name, i, `${name} (${i})`]);
      } else {
        targets = [p.max, p.min].map((v) => [String(v), v, String(v)]);
      }
      for (const [text, want, what] of targets) {
        if (/\s/.test(text)) {
          skipped.push(`${addr} = "${text}"`);
          continue;
        }
        const got = await send(`ed set ${addr} ${text}`);
        if (got.at(-1) !== "OK queued") {
          fail(addr, `set ${what}${label}`, got.join(" | "));
          continue;
        }
        const v = await settle(addr, (v) => near(v.stored, want));
        sets++;
        if (!near(v.stored, want)) fail(addr, `set ${what}${label}`, `read back stored ${v.stored}, want ${want} (${v.text})`);
      }
    }
  }
  const shownIn = (cur) =>
    blocks.filter((b) => b.variant === "-" || cur.get(b.panel) === b.variant).flatMap((b) => b.params);
  const start = await current();
  // The selector param goes last, and the filter once per TYPE: what FILTER.MODE
  // takes depends on it. Other panels are swept in the variant the Part shows.
  const hold = (p) => p.addr === "FILTER.KIND";
  const shown = shownIn(start);
  await sweep(shown.filter((p) => !hold(p)), "");
  const kinds = params.get("FILTER.KIND")?.opts.map(([, name]) => name) ?? [];
  for (const kind of kinds) {
    if (kind === start.get("FILTER")) continue;
    await send(`ed set FILTER.KIND ${kind}`);
    const now = await settleVariant("FILTER", kind);
    if (now !== kind) {
      fail("FILTER.KIND", `set ${kind}`, `panel FILTER is ${now}`);
      continue;
    }
    await sweep(blocks.filter((b) => b.panel === "FILTER" && b.variant === kind).flatMap((b) => b.params), ` @${kind}`);
  }
  await sweep(shown.filter(hold), "");
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

  // load a Sound through the browser keys: INIT ALGO, the top row
  await send("ed set FILTER.CUTOFF 500");
  await settle("FILTER.CUTOFF", (v) => near(v.stored, 500));
  await since();
  await key("EDIT+B1");
  await turn(1, -64);
  await turn(1, -64);
  await key("EDIT"); // the edited Sound asks first
  await key("EDIT");
  const load = await sinceUntil((l) => l.startsWith("val FILTER.CUTOFF "));
  const cut = load.map(parseVal).filter(Boolean).find((v) => v.addr === "FILTER.CUTOFF");
  if (!cut || !near(cut.stored, 20000)) fail("FILTER.CUTOFF", "load INIT ALGO", `since: ${load.slice(0, 3).join(" | ")}`);
  else if (!load.every((l) => l.startsWith("val ") || l.startsWith("panel ") || l.startsWith("part "))) {
    fail("load", "since", `unexpected lines: ${load.filter((l) => !/^(val|panel|part) /.test(l)).join(" | ")}`);
  } else info(`load: INIT ALGO -> ${load.length} lines, FILTER.CUTOFF back to ${cut.stored}`);
  await onPart1();

  // a stale number: everything
  await since();
  for (const stale of [0, gen + 1000]) {
    const all = await ask(`ed since ${stale}`);
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
    info(`stale ${stale}: ${all.length - 1} lines, ${vals.length} vals, ${ps.length} panels`);
    gen = Number(/^OK (\d+)$/.exec(all.at(-1))?.[1] ?? gen);
  }
  // and a current one: nothing
  const none = await since();
  if (none.length) fail("since", "current", `${none.length} lines: ${none[0]}`);
}

try {
  await main();
} catch (e) {
  failures.push(String(e.message ?? e));
  console.log(`FAIL ${e.message ?? e}`);
}
if (skipped.length) console.log(`skipped (a name with a space cannot be set): ${skipped.join("; ")}`);
try {
  ws?.close();
} catch {}
console.log(failures.length ? `\n${failures.length} FAILED` : "\nall passed");
process.exit(failures.length ? 1 : 0);
