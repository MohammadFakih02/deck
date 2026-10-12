// Snap-zone overlay shown while a window is dragged (click-through window).
const T = window.__TAURI__;
const root = document.getElementById("root");
T.core.invoke("get_state").then((s) => document.documentElement.style.setProperty("--acc", s.settings.accent));
T.event.listen("zones", (e) => {
  const v = e.payload;
  root.innerHTML = "";
  if (!v) return;
  const W = innerWidth;
  const H = innerHeight;
  v.zones.forEach((z, i) => {
    const d = document.createElement("div");
    d.className = "z" + (i === v.active ? " on" : "");
    d.style.left = z.x * W + "px";
    d.style.top = z.y * H + "px";
    d.style.width = z.w * W - 12 + "px";
    d.style.height = z.h * H - 12 + "px";
    d.innerHTML = `<span>${i + 1}</span>`;
    root.append(d);
  });
  const n = document.createElement("div");
  n.className = "name";
  n.textContent = `${v.name} · let go over a zone to snap · tap the right mouse button to hide`;
  root.append(n);
});
