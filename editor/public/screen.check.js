// node public/screen.check.js: screen.wasm (built by `just editor-screen`)
// painted as <chimera-screen> paints it, through the `spec` Elm builds,
// without a browser; then the paint cost, replayed from INIT (as before
// the live mirrors) and live.
var fs = require('fs');
var path = require('path');
var S = require('./screen.js');

function fail(msg) { console.error('FAIL ' + msg); process.exit(1); }

// The spec text Elm builds (contract § 1).
function spec(o) {
  return JSON.stringify({ engine: o.engine, page: o.page, region: o.region || 'screen',
                          focus: o.focus == null ? null : o.focus, pad: o.pad == null ? null : o.pad,
                          vals: o.vals || [] });
}

function differ(a, b) {
  var n = 0;
  for (var i = 0; i < a.rgba.length; i++) if (a.rgba[i] !== b.rgba[i]) n++;
  return n;
}

// `ed describe`'s schema fixture: each page's addresses, and a Sound of
// seeded random values, in describe order (a choice a whole option, which
// another variant may lack: that one is refused, the rest land).
function schema(name) {
  var text = fs.readFileSync(path.join(__dirname, '../tests/fixtures/' + name + '-schema.txt'), 'utf8');
  var pages = [], sound = [], seen = {}, page = null, seed = 12345;
  var rnd = function () { seed = (seed * 1103515245 + 12345) % 2147483648; return seed / 2147483648; };
  text.split('\n').forEach(function (l) {
    var w = l.split(' ');
    if (w[0] === 'page') { page = []; pages[Number(w[1])] = page; }
    if (w[0] === 'param') {
      var t = l.slice(l.lastIndexOf('" ') + 2).split(' ');
      var min = Number(t[2]), max = Number(t[3]), step = Number(t[4]);
      if (page.indexOf(w[1]) < 0) page.push(w[1]);
      if (seen[w[1]]) return;
      seen[w[1]] = true;
      // On the step's grid, as the unit stores it.
      var v = t[0] === 'cont' ? min + rnd() * (max - min) : Math.floor(rnd() * (max + 1));
      if (step > 0) v = Math.min(max, min + Math.round((v - min) / step) * step);
      sound.push([w[1], Math.fround(v)]);
    }
  });
  return { pages: pages, sound: sound };
}

S.load(fs.readFileSync(path.join(__dirname, 'screen.wasm'))).then(function (x) {
  var names = S.engines(x);
  var fixture = fs.readFileSync(path.join(__dirname, '../tests/fixtures/engines.txt'), 'utf8').trim().split('\n');
  if (JSON.stringify(names) !== JSON.stringify(fixture)) fail('engines ' + names + ' are not engines.txt\'s ' + fixture);
  var reset = function (e) { if (x.reset(names.indexOf(e)) !== 0) fail('reset ' + e); };
  var paint = function (o) { return S.paintSpec(x, names, spec(o)); };

  // A bad spec says why.
  [['', 'spec is no JSON'], ['[]', 'spec is no object'],
   [spec({ engine: 'NOPE', page: 0 }), 'no engine "NOPE"'],
   [spec({ engine: 'VA', page: -1 }), 'page -1'],
   [spec({ engine: 'VA', page: 0, region: 'side' }), 'no region "side"'],
   [spec({ engine: 'VA', page: 0, focus: 6 }), 'focus 6'],
   [spec({ engine: 'DRUM', page: 0, pad: 13 }), 'pad 13'],
   [spec({ engine: 'DRUM', page: 0, region: 'padicon' }), 'padicon needs a pad'],
   [spec({ engine: 'VA', page: 0, vals: [['VA.X', NaN]] }), 'vals[0] ["VA.X",null]'],
   [spec({ engine: 'VA', page: 0, vals: [['NOPE.X', 1]] }), 'vals refused whole (-1)']
  ].forEach(function (c) {
    var got = S.paintSpec(x, names, c[0]);
    if (got.error !== c[1]) fail('spec ' + c[0] + ': ' + JSON.stringify(got.error) + ' not ' + c[1]);
  });

  // ALGO's algorithm page morphs its chart with MORPH focused (2), as Elm
  // sends it; on ALG A (no focus) it shows ALG A.
  var at = function (morph, focus, region) {
    return paint({ engine: 'ALGO', page: 0, region: region, focus: focus, vals: [['ALGO.MORPH', morph]] });
  };
  var a = at(0, 2), b = at(100, 2);
  if (!a.rect || a.rect.w !== 240 || a.rect.h !== 68) fail('ALG screen rect: ' + JSON.stringify(a.rect));
  if (differ(a, b) < 50) fail('MORPH moved ' + differ(a, b) + ' channels with focus 2');
  if (differ(at(0, null), at(100, null)) !== 0) fail('MORPH moved ALG A\'s chart');
  // The same spec, the same pixels, whatever the mirror held before.
  if (differ(at(0, 2), a) !== 0) fail('a repaint differs');
  var g = at(64, 2, 'gauge');
  if (!g.rgba || g.rgba.length !== g.rect.w * g.rect.h * 4) fail('gauge');
  if (!paint({ engine: 'VA', page: 999 }).hidden) fail('a page past the end paints');
  names.forEach(function (e) {
    if (!paint({ engine: e, page: 0, region: 'full' }).rgba) fail(e + ' page 0');
  });

  // DRUM: a pad's icon is its machine's; KIT draws the spec's pad.
  var icon = function (m) { return paint({ engine: 'DRUM', page: 0, region: 'padicon', pad: 3, vals: [['PAD3.MACHINE', m]] }); };
  var i0 = icon(0), i5 = icon(5);
  if (i0.rect.w !== 80 || i0.rect.h !== 48) fail('pad icon rect ' + JSON.stringify(i0.rect));
  if (differ(i0, i5) === 0) fail('a pad icon ignores its machine');
  var kit = function (pad) { return paint({ engine: 'DRUM', page: 9, region: 'screen', pad: pad }); };
  if (differ(kit(5), kit(1)) === 0) fail('KIT ignores the spec\'s pad');
  if (differ(kit(5), kit(5)) !== 0) fail('KIT pad 5 twice differs');

  // A selector after the params it resets: the same as before them.
  var mch = function (vals) { reset('DRUM'); return paint({ engine: 'DRUM', page: 11, vals: vals }); };
  var knob = ['PAD1.KNOB_A', 3], machine = ['PAD1.MACHINE', 4];
  if (differ(mch([knob, machine]), mch([machine, knob])) !== 0) fail('a MACHINE after its knob resets it');

  // The paint cost of one value change, on every page of the engine as a
  // screen: before, each paint replays the Part's every value from INIT and
  // every screen carries them all, so each repaints; live, each screen
  // carries its own page's values, and its mirror writes the diff alone.
  function cost(engine, file, addr, to) {
    var sc = schema(file);
    var sound = sc.sound;
    if (!sound.some(function (v) { return v[0] === addr; })) fail(addr + ' not in ' + file);
    var changed = sound.map(function (v) { return v[0] === addr ? [addr, to] : v; });
    var pages = sc.pages.map(function (_, n) { return n; });
    var e = names.indexOf(engine);
    var time = function (f) { var t = process.hrtime.bigint(); f(); return Number(process.hrtime.bigint() - t) / 1e6; };
    // Before: every screen repaints, each from INIT with every value.
    var before = { paints: 0, ms: 0, writes: 0 };
    pages.forEach(function (n) {
      before.ms += time(function () {
        x.reset(e);
        S.apply(x, e, changed);
        x.show(e, n, 1, -1, 0);
      });
      before.writes += x.writes();
      before.paints++;
    });
    // Live: a screen repaints only if its page's values moved.
    reset(engine);
    var own = function (vals, n) { return vals.filter(function (v) { return sc.pages[n].indexOf(v[0]) >= 0; }); };
    pages.forEach(function (n) { S.apply(x, e, own(sound, n)); x.show(e, n, 1, -1, 0); });
    var live = { paints: 0, ms: 0, writes: 0 };
    pages.forEach(function (n) {
      if (JSON.stringify(own(sound, n)) === JSON.stringify(own(changed, n))) return;
      live.ms += time(function () { S.apply(x, e, own(changed, n)); x.show(e, n, 1, -1, 0); });
      live.writes += x.writes();
      live.paints++;
    });
    if (live.paints === 0 || live.writes !== live.paints) fail(engine + ': ' + JSON.stringify(live));
    console.log('screen.check: ' + engine + ' ' + addr + ' over ' + pages.length + ' screens: before ' +
      before.paints + ' paints, ' + (before.ms / before.paints).toFixed(2) + ' ms each (' + before.ms.toFixed(1) +
      ' ms), ' + Math.round(before.writes / before.paints) + ' values written each; live ' + live.paints + ' paint(s), ' + (live.ms / live.paints).toFixed(2) + ' ms each, ' +
      live.writes + ' value(s) written');
  }
  cost('ALGO', 'algo', 'ALGO.MORPH', 64);
  cost('DRUM', 'drum', 'PAD3.LEVEL', 40);
  console.log('screen.check: ok');
}).catch(function (e) { fail(e.stack || e); });
