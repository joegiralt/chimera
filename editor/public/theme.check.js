// node public/theme.check.js: the theme shim survives blocked storage.
var T = require('./theme.js');
function fail(msg) { console.error('FAIL ' + msg); process.exit(1); }
global.window = {
  localStorage: { getItem: function () { throw new Error('blocked'); }, setItem: function () { throw new Error('blocked'); } },
  matchMedia: function () { return { matches: true }; },
};
var f = T.flags();
if (f.stored !== null || f.dark !== true) fail('blocked storage: ' + JSON.stringify(f));
T.store('night');
var kept = {};
global.window.localStorage = { getItem: function (k) { return kept[k] || null; }, setItem: function (k, v) { kept[k] = v; } };
T.store('night');
if (T.flags().stored !== 'night') fail('a stored choice reads back');
console.log('ok');
