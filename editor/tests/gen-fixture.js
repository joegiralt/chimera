// node tests/gen-fixture.js: the unit's `ed describe` answers (fixtures/<engine>-schema.txt,
// MIXER's, fixtures/mixer-schema.txt, SETTINGS', fixtures/settings-schema.txt, and MAP's,
// fixtures/map-schema.txt), each
// engine's INIT panels (fixtures/<engine>-init.txt, `ed since 0` on the sim) and its
// engines (fixtures/engines.txt, EngineType::ALL) as an Elm
// module, since elm-test can't read files. The output is generated and ignored by git.
var fs = require('fs');
var path = require('path');
var txt = fs.readFileSync(path.join(__dirname, 'fixtures', 'algo-schema.txt'), 'utf8');
var lines = txt.split('\n').filter(function (l) { return l !== ''; });
function list(name, ls) {
  return '\n\n' + name + ' : List String\n' + name + ' =\n    [ ' + ls.map(function (l) { return JSON.stringify(l); }).join('\n    , ') + '\n    ]\n';
}
function schema(engine) {
  return fs.readFileSync(path.join(__dirname, 'fixtures', engine + '-schema.txt'), 'utf8').split('\n').filter(function (l) { return l !== ''; });
}
var out = 'module FixtureData exposing (drum, drumInit, engines, lines, linesInit, map, mixer, modal, modalInit, partRefusal, settings, tape, tapeInit, va, vaInit)\n' + list('lines', lines) +
  ['va', 'modal', 'tape', 'drum', 'mixer', 'settings', 'map'].map(function (e) { return list(e, schema(e)); }).join('');
// Each engine's INIT Sound as `ed since 0` reports its panels (the sim, Part 1).
out += ['algo', 'va', 'modal', 'tape', 'drum'].map(function (e) { return list((e === 'algo' ? 'lines' : e) + 'Init', fs.readFileSync(path.join(__dirname, 'fixtures', e + '-init.txt'), 'utf8').split('\n').filter(function (l) { return l !== ''; })); }).join('');
var refusal = fs.readFileSync(path.join(__dirname, 'fixtures', 'part-refusal.txt'), 'utf8').trim();
out += '\n\npartRefusal : String\npartRefusal =\n    ' + JSON.stringify(refusal) + '\n';
var engines = fs.readFileSync(path.join(__dirname, 'fixtures', 'engines.txt'), 'utf8').split('\n').filter(function (l) { return l !== ''; });
out += '\n\nengines : List String\nengines =\n    [ ' + engines.map(function (l) { return JSON.stringify(l); }).join('\n    , ') + '\n    ]\n';
fs.writeFileSync(path.join(__dirname, 'FixtureData.elm'), out);
