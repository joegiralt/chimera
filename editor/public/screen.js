// <chimera-screen>: the unit's own screen art, drawn by screen.wasm
// (chimera-screen: the firmware's Renderer over a headless Part).
//
//   <chimera-screen spec='{"engine":"ALGO","page":0,"region":"screen",
//                          "focus":2,"pad":null,"vals":[["ALGO.MORPH",64]]}'>
//
// engine  a name from screen.wasm's list (the firmware's PLAYABLE, which
//         editor/tests/fixtures/engines.txt holds)
// page    `ed describe`'s page number
// region  "screen" (the page's art), "gauge" (the focus band's glyph),
//         "full", or "padicon" (pad `pad`'s machine icon alone)
// focus   null (slot a, the unit's until a turn) or knob a-f as 0-5.
//         ALGO's algorithm page (0) morphs its chart with MORPH only with
//         focus 2 (MORPH's own knob); on ALG A it shows ALG A
// pad     null or 1-12: DRUM's selected pad (KIT draws it selected, a pad's
//         page shows it); "padicon" needs it
// vals    [["ADDR", stored], ...] as `ed since` reports them, in any order
//
// One live mirror per engine keeps every value it was sent: a paint writes
// only the values the mirror doesn't hold, so a spec with the page's own
// values is that page's diff. A spec decodes once a change; a bad one
// sets data-error="<reason>" and draws a small mark (each reason warned
// once).
var ChimeraScreen = (function () {
  var W = 240, H = 320;
  var REGIONS = { full: 0, screen: 1, gauge: 2, padicon: 3 }; // `Region`
  // `abi::Status`.
  var STATUS = { ok: 0, refused: 1, badBatch: -1, noSuchEngine: -2, notShown: -3,
                 noSuchCell: -4, noSuchRegion: -5, noRegion: -6, noSuchPad: -7 };
  var enc = new TextEncoder();

  // The engines' names, in the wasm's numbering. Each pointer is taken
  // before the memory is viewed: a call may grow it, detaching old views.
  function engines(x) {
    var p = x.engines_ptr(), n = x.engines_len();
    return new TextDecoder().decode(new Uint8Array(x.memory.buffer, p, n)).split('\n');
  }

  function int(v, lo, hi) {
    return typeof v === 'number' && Number.isInteger(v) && v >= lo && v <= hi;
  }

  // The `spec` text: `{ spec }`, or `{ error }` saying what is wrong.
  function decode(text, names) {
    var s;
    try { s = JSON.parse(text); } catch (e) { return { error: 'spec is no JSON' }; }
    if (!s || typeof s !== 'object' || Array.isArray(s)) return { error: 'spec is no object' };
    var engine = names.indexOf(s.engine);
    if (engine < 0) return { error: 'no engine ' + JSON.stringify(s.engine) };
    if (!int(s.page, 0, 0xffff)) return { error: 'page ' + JSON.stringify(s.page) };
    if (!Object.prototype.hasOwnProperty.call(REGIONS, s.region)) return { error: 'no region ' + JSON.stringify(s.region) };
    var focus = s.focus == null ? -1 : s.focus;
    if (focus !== -1 && !int(focus, 0, 5)) return { error: 'focus ' + JSON.stringify(s.focus) };
    var pad = s.pad == null ? 0 : s.pad;
    if (pad !== 0 && !int(pad, 1, 12)) return { error: 'pad ' + JSON.stringify(s.pad) };
    if (s.region === 'padicon' && pad === 0) return { error: 'padicon needs a pad' };
    if (!Array.isArray(s.vals)) return { error: 'vals is no list' };
    for (var i = 0; i < s.vals.length; i++) {
      var v = s.vals[i];
      if (!Array.isArray(v) || v.length !== 2 || typeof v[0] !== 'string' || v[0] === '' ||
          v[0].indexOf('\n') >= 0 || typeof v[1] !== 'number' || !isFinite(v[1])) {
        return { error: 'vals[' + i + '] ' + JSON.stringify(v) };
      }
    }
    return { spec: { engine: engine, page: s.page, region: REGIONS[s.region], focus: focus, pad: pad, vals: s.vals } };
  }

  // RGB565 pixels `px` (row-major, `stride` wide) cropped to `r` into RGBA,
  // each channel shifted up as the desktop simulator's window does.
  function toRgba(px, stride, r, out) {
    var o = 0;
    for (var y = r.y; y < r.y + r.h; y++) {
      for (var x = r.x; x < r.x + r.w; x++) {
        var p = px[y * stride + x];
        out[o++] = (p >> 11 & 31) << 3;
        out[o++] = (p >> 5 & 63) << 2;
        out[o++] = (p & 31) << 3;
        out[o++] = 255;
      }
    }
    return out;
  }

  function load(bytes) {
    return WebAssembly.instantiate(bytes, {}).then(function (r) { return r.instance.exports; });
  }

  // The engine's mirror takes `vals`; the wasm's status. Only the values it
  // doesn't hold are written (`writes()` says how many).
  function apply(x, engine, vals) {
    var text = enc.encode(vals.map(function (v) { return v[0]; }).join('\n'));
    if (vals.length > 1024 || text.length > 32 * 1024) return STATUS.badBatch; // abi::VALS_CAP, TEXT_CAP
    var tp = x.text_ptr(), vp = x.vals_ptr();
    new Uint8Array(x.memory.buffer, tp, text.length).set(text);
    var f = new Float32Array(x.memory.buffer, vp, vals.length);
    for (var i = 0; i < vals.length; i++) f[i] = vals[i][1];
    return x.apply(engine, vals.length, text.length);
  }

  // One decoded spec's pixels: `{ rect, rgba }`, `{ hidden }` where the
  // Sound doesn't show the page or region, or `{ error }`.
  function draw(x, s) {
    var got = apply(x, s.engine, s.vals);
    if (got === STATUS.refused) console.debug('chimera-screen: ' + x.refused() + ' values refused');
    else if (got !== STATUS.ok) return { error: 'vals refused whole (' + got + ')' };
    got = x.show(s.engine, s.page, s.region, s.focus, s.pad);
    if (got === STATUS.notShown || got === STATUS.noRegion) return { hidden: true };
    if (got !== STATUS.ok) return { error: 'show refused (' + got + ')' };
    var rp = x.rect_ptr(), bp = x.buf_ptr();
    var q = new Uint16Array(x.memory.buffer, rp, 4);
    var rect = { x: q[0], y: q[1], w: q[2], h: q[3] };
    var px = new Uint16Array(x.memory.buffer, bp, W * H);
    return { rect: rect, rgba: toRgba(px, W, rect, new Uint8ClampedArray(rect.w * rect.h * 4)) };
  }

  // A spec text's paint: decoded, then drawn.
  function paintSpec(x, names, text) {
    var d = decode(text, names);
    return d.error ? d : draw(x, d.spec);
  }

  var warned = {};
  function warn(reason) {
    if (warned[reason]) return;
    warned[reason] = true;
    console.warn('chimera-screen: ' + reason);
  }

  // The error mark: a small red cross on black, 8x8.
  function mark(canvas) {
    canvas.width = 8;
    canvas.height = 8;
    var g = canvas.getContext('2d');
    g.fillStyle = '#000';
    g.fillRect(0, 0, 8, 8);
    g.fillStyle = '#e0302a';
    for (var i = 1; i < 7; i++) { g.fillRect(i, i, 1, 1); g.fillRect(7 - i, i, 1, 1); }
  }

  function define(url) {
    if (typeof customElements === 'undefined' || customElements.get('chimera-screen')) return;
    var ready = fetch(url).then(function (r) { return r.arrayBuffer(); }).then(load)
      .then(function (x) { return { x: x, names: engines(x) }; });

    class Screen extends HTMLElement {
      static get observedAttributes() { return ['spec']; }
      connectedCallback() {
        if (!this.canvas) {
          this.canvas = document.createElement('canvas');
          this.canvas.style.imageRendering = 'pixelated';
          this.canvas.style.display = 'block';
          this.canvas.style.width = '100%';
          this.appendChild(this.canvas);
        }
        this.schedule();
      }
      attributeChangedCallback() { this.schedule(); }
      // Changes in one task paint once; the same spec twice paints once.
      schedule() {
        if (this.queued || !this.canvas) return;
        this.queued = true;
        var self = this;
        ready.then(function (w) {
          self.queued = false;
          var text = self.getAttribute('spec');
          if (text === self.painted) return;
          self.painted = text;
          self.paint(paintSpec(w.x, w.names, text || ''));
        }, function (err) {
          self.queued = false;
          self.paint({ error: 'screen.wasm: ' + err });
        });
      }
      paint(got) {
        if (got.error) {
          warn(got.error);
          this.hidden = false;
          this.setAttribute('data-error', got.error);
          this.removeAttribute('data-painted');
          mark(this.canvas);
          return;
        }
        this.removeAttribute('data-error');
        this.hidden = !!got.hidden;
        // Painted: the crop Elm put inside gives way to the canvas.
        if (got.hidden) { this.removeAttribute('data-painted'); return; }
        this.setAttribute('data-painted', '');
        this.canvas.width = got.rect.w;
        this.canvas.height = got.rect.h;
        this.canvas.getContext('2d').putImageData(new ImageData(got.rgba, got.rect.w, got.rect.h), 0, 0);
      }
    }
    customElements.define('chimera-screen', Screen);
  }

  define('screen.wasm');
  return { decode: decode, engines: engines, toRgba: toRgba, load: load, apply: apply, draw: draw,
           paintSpec: paintSpec, REGIONS: REGIONS, STATUS: STATUS };
})();

if (typeof module !== 'undefined') module.exports = ChimeraScreen;
