// A pinned theme (Light or Dark) applies before the first paint; System follows macOS.
// A file, not an inline script, so the app's CSP allows no inline scripts.
try {
  var t = JSON.parse(localStorage.getItem("wh:theme") || '"system"');
  if (t === "light" || t === "dark") document.documentElement.dataset.mode = t;
} catch (e) {}
