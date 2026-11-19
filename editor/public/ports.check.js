// node public/ports.check.js: the serial teardown order, and the link lifecycle, against fakes.
var P = require('./ports.js');

function fail(msg) { console.error('FAIL ' + msg); process.exit(1); }
function eq(got, want, what) { if (got !== want) fail(what + ': ' + got + ' want ' + want); }
var tick = function () { return new Promise(function (r) { setTimeout(r, 5); }); };

async function teardownOrder() {
  var log = [];
  var step = function (n) { return function () { log.push(n); return Promise.resolve(); }; };
  var port = { close: step('port.close') };
  var reader = { cancel: step('reader.cancel'), releaseLock: function () { log.push('reader.releaseLock'); } };
  var writer = { close: step('writer.close'), releaseLock: function () { log.push('writer.releaseLock'); } };
  await P.release(port, reader, writer);
  eq(log.join(), 'reader.cancel,reader.releaseLock,writer.close,writer.releaseLock,port.close', 'teardown order');
}

// A cut-off answer leaves a fragment; the empty line that opens the next full
// answer ends it, so `part` arrives whole.
function fragmentThenFullAnswer() {
  var got = [];
  var feed = P.liner(function (l) { got.push(l); });
  feed('val A.B 0 0 "te');
  feed('\npart 2 VA\nOK 7\n');
  eq(got.indexOf('part 2 VA') >= 0, true, 'the part line survives a fragment: ' + JSON.stringify(got));
}

// A fake Elm app and a fake Web Serial port.
function fakeApp() {
  var states = [];
  var subs = {};
  var port = function (n) { return { subscribe: function (f) { subs[n] = f; } }; };
  return {
    states: states, subs: subs,
    ports: {
      connectSerial: port('connectSerial'), connectSocket: port('connectSocket'), send: port('send'),
      linkState: { send: function (s) { states.push(s); } },
      lineIn: { send: function () {} },
      recall: port('recall'), keep: port('keep'),
      got: { send: function (g) { (subs.gotLog = subs.gotLog || []).push(g); } },
    },
  };
}

function fakeSerial(opts) {
  var log = [];
  var pending = null; // resolves the current read
  var port = {
    log: log,
    open: function () { return opts.open ? opts.open() : Promise.resolve(); },
    close: function () { log.push('port.close'); return Promise.resolve(); },
    writable: { getWriter: function () { return {
      write: function () { return opts.failWrite ? Promise.reject(new Error('gone')) : Promise.resolve(); },
      close: function () { return Promise.resolve(); }, releaseLock: function () {} }; } },
    readable: { getReader: function () { return {
      read: function () { return new Promise(function (res, rej) { pending = { res: res, rej: rej }; }); },
      cancel: function () { if (pending) pending.res({ done: true }); return Promise.resolve(); },
      releaseLock: function () {} }; } },
    unplug: function () { pending.res({ done: true }); },
  };
  var chooser = 0;
  Object.defineProperty(globalThis, 'navigator', { configurable: true, value: { serial: {
    requestPort: function () { chooser++; return Promise.resolve(port); } } } });
  return { port: port, chooser: function () { return chooser; } };
}

async function serialEnds() {
  var app = fakeApp(); P.attach(app);
  var s = fakeSerial({});
  app.subs.connectSerial(); await tick();
  eq(app.states.join(), 'open', 'serial opens');
  s.port.unplug(); await tick();
  eq(app.states.join(), 'open,lost', 'read loop ends: lost');
  eq(s.port.log.join(), 'port.close', 'read loop ended while current: the port is released');
}

async function failedWriteReleases() {
  var app = fakeApp(); P.attach(app);
  var s = fakeSerial({ failWrite: true });
  app.subs.connectSerial(); await tick();
  app.subs.send('ed hello'); await tick();
  eq(app.states.join(), 'open,lost', 'failed write: lost');
  eq(s.port.log.join(), 'port.close', 'failed write: the port is released');
}

async function quickClicksDoNotInterleave() {
  var app = fakeApp(); P.attach(app);
  var s = fakeSerial({});
  app.subs.connectSerial(); app.subs.connectSerial(); app.subs.connectSocket('ws://x'); await tick();
  eq(s.chooser(), 1, 'one chooser for two quick clicks');
  eq(app.states.join(), 'open', 'one connection, one open');
}

// A fake IndexedDB: open, one store, get and put, each answering on a tick.
function fakeIdb() {
  var data = {};
  function req(f) { var r = {}; setTimeout(function () { try { r.result = f(); r.onsuccess && r.onsuccess(); } catch (e) { r.error = e; r.onerror && r.onerror(); } }, 0); return r; }
  var store = {
    get: function (k) { return req(function () { return data[k]; }); },
    put: function (v, k) { data[k] = v; return {}; },
  };
  var db = {
    createObjectStore: function () { return store; },
    transaction: function () { var t = { objectStore: function () { return store; } }; setTimeout(function () { t.oncomplete && t.oncomplete(); }, 0); return t; },
    close: function () {},
  };
  return { open: function () { var r = req(function () { return db; }); setTimeout(function () { r.onupgradeneeded && r.onupgradeneeded(); }, 0); return r; } };
}

// No IndexedDB (or any failure of it) is a miss, never silence.
async function noStoreIsAMiss() {
  var app = fakeApp(); P.attach(app);
  delete globalThis.indexedDB;
  await P.recall('4/ALGO/0123456789abcdef');
  eq(JSON.stringify(app.subs.gotLog), JSON.stringify([{ key: '4/ALGO/0123456789abcdef', lines: null }]), 'no store: a miss');
}

// What is kept under a key is recalled under it, and nothing under another.
async function keptIsRecalled() {
  var app = fakeApp(); P.attach(app);
  globalThis.indexedDB = fakeIdb();
  await P.keep({ key: '4/MIXER/00000000000000ab', lines: ['page 0 CHR of 1', 'panel CHR -'] });
  await P.recall('4/MIXER/00000000000000ab');
  await P.recall('4/MIXER/00000000000000ac');
  eq(JSON.stringify(app.subs.gotLog), JSON.stringify([
    { key: '4/MIXER/00000000000000ab', lines: ['page 0 CHR of 1', 'panel CHR -'] },
    { key: '4/MIXER/00000000000000ac', lines: null },
  ]), 'kept, recalled');
  delete globalThis.indexedDB;
}

(async function () {
  await teardownOrder();
  await noStoreIsAMiss();
  await keptIsRecalled();
  fragmentThenFullAnswer();
  await serialEnds();
  await failedWriteReleases();
  await quickClicksDoNotInterleave();
  console.log('ok');
  process.exit(0);
})();
