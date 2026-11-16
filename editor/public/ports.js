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

  function drop() {
    if (link) { try { link.close(); } catch (e) {} link = null; }
  }

  async function connectSerial() {
    drop();
    if (!('serial' in navigator)) { state('unsupported'); return; }
    var port;
    try {
      port = await navigator.serial.requestPort({ filters: [{ usbVendorId: 0x0483, usbProductId: 0x5740 }] });
      await port.open({ baudRate: 115200, dataBits: 8, stopBits: 1, parity: 'none' });
    } catch (e) {
      // A cancelled chooser is not a fault; a port someone else holds is.
      if (e && e.name === 'NotFoundError') { return; }
      state('busy');
      return;
    }
    var enc = new TextEncoder();
    var writer = port.writable.getWriter();
    var reader = port.readable.getReader();
    var dec = new TextDecoder('utf-8');
    var feed = liner(function (l) { app.ports.lineIn.send(l); });
    var mine = {
      write: function (line) { return writer.write(enc.encode(line + '\n')); },
      close: function () { reader.cancel().catch(function () {}); writer.releaseLock(); port.close().catch(function () {}); },
    };
    link = mine;
    state('open');
    try {
      for (;;) {
        var r = await reader.read();
        if (r.done) break;
        feed(dec.decode(r.value, { stream: true }));
      }
    } catch (e) { /* unplugged: fall through */ }
    if (link === mine) { link = null; state('lost'); }
  }

  function connectSocket(url) {
    drop();
    var ws;
    try { ws = new WebSocket(url); } catch (e) { state('busy'); return; }
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
      if (link === mine) { link = null; state(opened ? 'lost' : 'busy'); }
    };
  }

  function send(line) {
    if (link) link.write(line).catch(function () { state('lost'); });
  }

  return {
    attach: function (a) {
      app = a;
      app.ports.connectSerial.subscribe(connectSerial);
      app.ports.connectSocket.subscribe(connectSocket);
      app.ports.send.subscribe(send);
    },
  };
})();
