// <chimera-screen>: the unit's own screen art, drawn by screen.wasm
// (chimera-screen: the firmware's Renderer over a headless Part).
//
//   <chimera-screen engine="ALGO" page="0" region="screen" focus="2"
//                   vals="ALGO.MORPH=64;OP_A.WAVE=9"></chimera-screen>
//
// engine  ALGO, MODAL, TAPE, VA or DRUM: its INIT Sound first
// page    `ed describe`'s page number
// region  full, screen (the page's art) or gauge (the focus band's glyph)
// focus   optional knob a-f as 0-5: the cell in the focus band
// vals    "ADDR=stored;..." from `ed since`: each set as `ed set` takes it
//
// Every paint starts from INIT, so a paint shows exactly its attributes.
var ChimeraScreen = (function () {
  var W = 240, H = 320;
  // `EngineType::ALL`'s order (chimera-screen's engine_order_is_the_scripts).
  var ENGINES = ['ALGO', 'MODAL', 'TAPE', 'VA', 'DRUM'];
  var REGIONS = { full: 0, screen: 1, gauge: 2 };
  var IN_CAP = 64; // abi::IN_CAP

  // "A=1;B=0.5" -> [["A", 1], ["B", 0.5]]; malformed pairs dropped.
  function parseVals(s) {
    return (s || '').split(';').map(function (p) {
      var i = p.indexOf('=');
      return i > 0 ? [p.slice(0, i).trim(), Number(p.slice(i + 1))] : null;
    }).filter(function (p) { return p && p[0] && isFinite(p[1]); });
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

  // One paint's pixels: `{ rect, rgba }`, or null where the page or region
  // isn't shown on this Sound.
  function draw(x, a) {
    var e = ENGINES.indexOf(String(a.engine || '').toUpperCase());
    var page = parseInt(a.page, 10);
    if (e < 0 || !(page >= 0) || x.init(e) !== 0) return null;
    if (a.focus != null && a.focus !== '' && x.focus(page, parseInt(a.focus, 10)) !== 0) return null;
    var enc = new TextEncoder();
    parseVals(a.vals).forEach(function (kv) {
      var b = enc.encode(kv[0]);
      if (b.length > IN_CAP) return;
      new Uint8Array(x.memory.buffer, x.in_ptr(), b.length).set(b);
      if (x.set(b.length, kv[1]) !== 0) console.debug('chimera-screen: refused', kv[0], kv[1]);
    });
    if (x.render(page) !== 0) return null;
    var ri = REGIONS[a.region || 'screen'];
    var rp = ri == null ? 0 : x.region(ri);
    if (!rp) return null;
    var q = new Uint16Array(x.memory.buffer, rp, 4);
    var rect = { x: q[0], y: q[1], w: q[2], h: q[3] };
    var px = new Uint16Array(x.memory.buffer, x.buf_ptr(), W * H);
    return { rect: rect, rgba: toRgba(px, W, rect, new Uint8ClampedArray(rect.w * rect.h * 4)) };
  }

  function define(url) {
    if (typeof customElements === 'undefined' || customElements.get('chimera-screen')) return;
    var ready = fetch(url).then(function (r) { return r.arrayBuffer(); }).then(load);
    var ATTRS = ['engine', 'page', 'region', 'focus', 'vals'];

    class Screen extends HTMLElement {
      static get observedAttributes() { return ATTRS; }
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
      // Attribute changes in one task paint once.
      schedule() {
        if (this.queued || !this.canvas) return;
        this.queued = true;
        var self = this;
        ready.then(function (x) {
          self.queued = false;
          self.paint(x);
        }, function (err) { console.error('chimera-screen: screen.wasm', err); });
      }
      paint(x) {
        var a = {};
        var self = this;
        ATTRS.forEach(function (n) { a[n] = self.getAttribute(n); });
        var got = draw(x, a);
        this.hidden = !got;
        if (!got) return;
        this.canvas.width = got.rect.w;
        this.canvas.height = got.rect.h;
        this.canvas.getContext('2d').putImageData(new ImageData(got.rgba, got.rect.w, got.rect.h), 0, 0);
      }
    }
    customElements.define('chimera-screen', Screen);
  }

  define('screen.wasm');
  return { parseVals: parseVals, toRgba: toRgba, load: load, draw: draw, ENGINES: ENGINES };
})();

if (typeof module !== 'undefined') module.exports = ChimeraScreen;
