// Tells the app this page is really drawing frames, so a window that silently stops painting can be
// nudged back to life (see webview_guard.rs). Beats stop on their own while the page is hidden.
(() => {
  const T = window.__TAURI__;
  if (!T || !T.core) return;
  let lastFrame = performance.now();
  const frame = () => { lastFrame = performance.now(); requestAnimationFrame(frame); };
  requestAnimationFrame(frame);
  setInterval(() => {
    if (!document.hidden && performance.now() - lastFrame < 3000) T.core.invoke("ui_heartbeat").catch(() => {});
  }, 4000);
})();
