// node tests/gen-fixture.js: the unit's `ed describe ALGO` answer (fixtures/algo-schema.txt) and its
// engines (fixtures/engines.txt, EngineType::ALL) as an Elm
// module, since elm-test can't read files. The output is generated and ignored by git.
var fs = require('fs');
var path = require('path');
var txt = fs.readFileSync(path.join(__dirname, 'fixtures', 'algo-schema.txt'), 'utf8');
var lines = txt.split('\n').filter(function (l) { return l !== ''; });
var out = 'module FixtureData exposing (engines, lines, partRefusal)\n\n\nlines : List String\nlines =\n    [ ' +
  lines.map(function (l) { return JSON.stringify(l); }).join('\n    , ') + '\n    ]\n';
var refusal = fs.readFileSync(path.join(__dirname, 'fixtures', 'part-refusal.txt'), 'utf8').trim();
out += '\n\npartRefusal : String\npartRefusal =\n    ' + JSON.stringify(refusal) + '\n';
var engines = fs.readFileSync(path.join(__dirname, 'fixtures', 'engines.txt'), 'utf8').split('\n').filter(function (l) { return l !== ''; });
out += '\n\nengines : List String\nengines =\n    [ ' + engines.map(function (l) { return JSON.stringify(l); }).join('\n    , ') + '\n    ]\n';
fs.writeFileSync(path.join(__dirname, 'FixtureData.elm'), out);
