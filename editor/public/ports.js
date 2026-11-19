// Web Serial and WebSocket for the Elm editor: lines in, lines out, link state.
// One link at a time; both kinds look the same to Elm.
var ChimeraPorts = (function () {
  var link = null; // { write(line) -> Promise, close() }
  var app = null;

  function state(s) { app.ports.linkState.send(s); }

  // Split a stream of text into lines on \n, CR stripped.
  function liner(emit) {
    var buf = '';
    return function (chunk) {
      buf += chunk;
      var i;
      while ((i = buf.indexOf('\n')) >= 0) {
        emit(buf.slice(0, i).replace(/\r$/, ''));
        buf = buf.slice(i + 1);
      }
    };
  }

  // Release a serial port in the only order that lets it close: cancel the
  // reader (ends its read loop), release its lock, close the writer (drains
  // pending writes), release it, and only then close the port. A port closed
  // while a stream is still locked rejects, stays open, and the next
  // requestPort/open fails.
  async function release(port, reader, writer) {
    try { await reader.cancel(); } catch (e) {}
    try { reader.releaseLock(); } catch (e) {}
    try { await writer.close(); } catch (e) {}
    try { writer.releaseLock(); } catch (e) {}
    try { await port.close(); } catch (e) {}
  }

  async function drop() {
    var old = link;
    link = null;
    if (old) { try { await old.close(); } catch (e) {} }
  }

  // True while a connect is between its click and its link: a second click
  // then does nothing, so two connects never interleave their awaits.
  var connecting = false;

  async function connect(setup) {
    if (connecting) return;
    connecting = true;
    var run;
    try { run = await setup(); } finally { connecting = false; }
    if (run) await run();
  }

  // Open the port and make it the link; resolves to the read loop, or nothing.
  async function openSerial() {
    await drop();
    if (!('serial' in navigator)) { state('unsupported'); return null; }
    var port;
    try {
      port = await navigator.serial.requestPort({ filters: [{ usbVendorId: 0x0483, usbProductId: 0x5740 }] });
      await port.open({ baudRate: 115200, dataBits: 8, stopBits: 1, parity: 'none' });
    } catch (e) {
      // A cancelled chooser is not a fault; a port someone else holds is.
      if (e && e.name === 'NotFoundError') { return null; }
      state('busy');
      return null;
    }
    var enc = new TextEncoder();
    var writer = port.writable.getWriter();
    var reader = port.readable.getReader();
    var dec = new TextDecoder('utf-8');
    var feed = liner(function (l) { app.ports.lineIn.send(l); });
    var mine = {
      write: function (line) { return writer.write(enc.encode(line + '\n')); },
      close: function () { return release(port, reader, writer); },
    };
    link = mine;
    state('open');
    return async function () {
      try {
        for (;;) {
          var r = await reader.read();
          if (r.done) break;
          feed(dec.decode(r.value, { stream: true }));
        }
      } catch (e) { /* unplugged: fall through */ }
      // Still current: nobody dropped it, so release it here, or a reconnect to the same port finds it held.
      if (link === mine) { link = null; state('lost'); await mine.close(); }
    };
  }

  function connectSerial() { return connect(openSerial); }

  async function openSocket(url) {
    await drop();
    var ws;
    try { ws = new WebSocket(url); } catch (e) { state('absent'); return null; }
    var mine = {
      write: function (line) { ws.send(line); return Promise.resolve(); },
      close: function () { ws.close(); },
    };
    var opened = false;
    link = mine;
    ws.onopen = function () { opened = true; state('open'); };
    // The bridge sends one line per message; tolerate a CR or trailing \n.
    ws.onmessage = function (m) {
      String(m.data).split('\n').forEach(function (l, i, a) {
        if (i < a.length - 1 || l !== '') app.ports.lineIn.send(l.replace(/\r$/, ''));
      });
    };
    ws.onclose = function () {
      if (link === mine) { link = null; state(opened ? 'lost' : 'absent'); }
    };
    return null;
  }

  function connectSocket(url) { return connect(function () { return openSocket(url); }); }

  function send(line) {
    var l = link;
    if (l) l.write(line).catch(function () {
      // A write that fails ends the link; release it so a reconnect can open the same port.
      if (link === l) { link = null; state('lost'); Promise.resolve(l.close()).catch(function () {}); }
    });
  }

  return {
    release: release,
    liner: liner,
    attach: function (a) {
      app = a;
      app.ports.connectSerial.subscribe(connectSerial);
      app.ports.connectSocket.subscribe(connectSocket);
      app.ports.send.subscribe(send);
    },
  };
})();

if (typeof module !== 'undefined') module.exports = ChimeraPorts;
if (typeof Elm !== 'undefined' && typeof document !== 'undefined') {
  var theme = typeof ChimeraTheme !== 'undefined' ? ChimeraTheme : { flags: function () { return { stored: null, dark: false }; }, store: function () {} };
  var main = Elm.Main.init({ node: document.getElementById('app'), flags: theme.flags() });
  if (main.ports.storeTheme) main.ports.storeTheme.subscribe(theme.store);
  ChimeraPorts.attach(main);
}
