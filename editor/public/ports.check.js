// node public/ports.check.js: the serial teardown order, against a fake port.
var P = require('./ports.js');
var log = [];
var step = function (n) { return function () { log.push(n); return Promise.resolve(); }; };
var port = { close: step('port.close') };
var reader = { cancel: step('reader.cancel'), releaseLock: function () { log.push('reader.releaseLock'); } };
var writer = { close: step('writer.close'), releaseLock: function () { log.push('writer.releaseLock'); } };
P.release(port, reader, writer).then(function () {
  var want = 'reader.cancel,reader.releaseLock,writer.close,writer.releaseLock,port.close';
  if (log.join() !== want) { console.error('FAIL ' + log.join()); process.exit(1); }
  console.log('ok');
});
