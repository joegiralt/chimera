// The theme the UI starts in: the stored choice and the OS scheme, read
// behind try/catch (storage can be blocked or absent); Elm decides.
var ChimeraTheme = (function () {
  var KEY = 'chimera.theme';
  function flags() {
    var stored = null;
    try { stored = window.localStorage.getItem(KEY); } catch (e) {}
    var dark = false;
    try { dark = window.matchMedia('(prefers-color-scheme: dark)').matches; } catch (e) {}
    return { stored: stored, dark: dark };
  }
  function store(name) {
    try { window.localStorage.setItem(KEY, name); } catch (e) {}
  }
  return { flags: flags, store: store };
})();
if (typeof module !== 'undefined') module.exports = ChimeraTheme;
