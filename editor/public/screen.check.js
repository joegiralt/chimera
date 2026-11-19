// node public/screen.check.js: screen.wasm (built by `just editor-screen`)
// painted as <chimera-screen> paints it, without a browser.
var fs = require('fs');
var path = require('path');
var S = require('./screen.js');

function fail(msg) { console.error('FAIL ' + msg); process.exit(1); }

S.load(fs.readFileSync(path.join(__dirname, 'screen.wasm'))).then(function (x) {
  var vals = S.parseVals('ALGO.MORPH=0; OP_A.WAVE=9;bad;X=nan');
  if (JSON.stringify(vals) !== '[["ALGO.MORPH",0],["OP_A.WAVE",9]]') fail('parseVals: ' + JSON.stringify(vals));

  var at = function (morph, region) {
    return S.draw(x, { engine: 'algo', page: '0', region: region, focus: '2', vals: 'ALGO.MORPH=' + morph });
  };
  var a = at(0, 'screen'), b = at(100, 'screen');
  if (!a || a.rect.w !== 240 || a.rect.h !== 68) fail('ALG screen rect: ' + JSON.stringify(a && a.rect));
  var differ = 0;
  for (var i = 0; i < a.rgba.length; i++) if (a.rgba[i] !== b.rgba[i]) differ++;
  if (differ < 50) fail('MORPH moved ' + differ + ' channels');
  // A paint is its attributes alone: the same twice over.
  if (Buffer.compare(Buffer.from(at(0, 'screen').rgba), Buffer.from(a.rgba)) !== 0) fail('a repaint differs');
  var g = at(64, 'gauge');
  if (!g || g.rgba.length !== g.rect.w * g.rect.h * 4) fail('gauge');
  if (S.draw(x, { engine: 'NOPE', page: '0' }) !== null) fail('an unknown engine paints');
  if (S.draw(x, { engine: 'VA', page: '999' }) !== null) fail('a page past the end paints');
  S.ENGINES.forEach(function (e) {
    if (!S.draw(x, { engine: e, page: '0', region: 'full' })) fail(e + ' page 0');
  });
  console.log('screen.check: ok');
}).catch(function (e) { fail(e.stack || e); });
