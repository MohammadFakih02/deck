// Notification pop-ups in the bottom-right corner (their own window, which
// never takes focus from a game).
const T = window.__TAURI__;
const invoke = (c, a) => T.core.invoke(c, a || {});
const stack = document.getElementById("stack");
const timers = {};
const KIND_ICON = { timer: "timer", plan: "power", break: "coffee", file: "download", info: "bell" };

function fit() {
  requestAnimationFrame(() => invoke("toast_resize", { height: stack.children.length ? stack.offsetHeight : 0 }));
}

let audioCtx;
function chime() {
  try {
    audioCtx = audioCtx || new AudioContext();
    const now = audioCtx.currentTime;
    [880, 1318.5].forEach((f, i) => {
      const o = audioCtx.createOscillator();
      const g = audioCtx.createGain();
      o.type = "sine";
      o.frequency.value = f;
      g.gain.setValueAtTime(0, now + i * 0.16);
      g.gain.linearRampToValueAtTime(0.18, now + i * 0.16 + 0.02);
      g.gain.exponentialRampToValueAtTime(0.0001, now + i * 0.16 + 0.9);
      o.connect(g).connect(audioCtx.destination);
      o.start(now + i * 0.16);
      o.stop(now + i * 0.16 + 1);
    });
  } catch (e) {}
}

// While the mouse is over the notifications nothing moves under it: removals (timeouts, the
// backend dismissing one) and new toasts wait until it leaves (at most 15 s). A toast you click
// yourself still goes at once.
let over = false, overSince = 0;
const held = { remove: new Map(), show: [] };
document.addEventListener("mousemove", () => { if (!over) { over = true; overSince = Date.now(); } });
document.addEventListener("mouseleave", () => { over = false; flushHeld(); });
window.addEventListener("blur", () => { over = false; flushHeld(); });
setInterval(() => { if (over && Date.now() - overSince > 15000) { over = false; flushHeld(); } }, 1000);
function flushHeld() {
  const rm = [...held.remove], sh = held.show.splice(0);
  held.remove.clear();
  rm.forEach(([id, animate]) => remove(id, animate));
  sh.forEach(show);
}

function remove(id, animate, force) {
  if (over && !force) { held.remove.set(id, animate); return; }
  const el = stack.querySelector(`[data-id="${CSS.escape(id)}"]`);
  clearTimeout(timers[id]);
  if (!el) return;
  if (animate) {
    el.classList.add("out");
    setTimeout(() => { el.remove(); fit(); }, 180);
  } else {
    el.remove();
    fit();
  }
}

function show(t) {
  // the shutdown / sleep warning never waits: its 60 seconds are already running
  if (over && t.kind !== "plan") { held.show.push(t); return; }
  remove(t.id, false, true);
  const el = document.createElement("div");
  el.className = "t kind-" + t.kind;
  el.dataset.id = t.id;
  const ms = t.sticky ? 0 : 6500;
  el.innerHTML = `<div class="ic">${icon(KIND_ICON[t.kind] || "bell")}</div>
    <div><div class="title"></div><div class="body"></div><div class="acts"></div></div>
    <button class="x" title="Dismiss">${icon("x")}</button>
    ${ms ? `<div class="bar"><i style="animation-duration:${ms}ms"></i></div>` : ""}`;
  el.querySelector(".title").textContent = t.title;
  el.querySelector(".body").textContent = t.body;
  const acts = el.querySelector(".acts");
  if (!t.actions.length) acts.remove();
  t.actions.forEach(([id, label], i) => {
    const b = document.createElement("button");
    b.className = "b" + (i === t.actions.length - 1 ? " p" : "");
    b.textContent = label;
    b.onclick = () => { invoke("toast_action", { id: t.id, action: id }); remove(t.id, true, true); };
    acts.append(b);
  });
  el.querySelector(".x").onclick = () => { invoke("toast_action", { id: t.id, action: "dismiss" }); remove(t.id, true, true); };
  stack.append(el);
  while (stack.children.length > 4) stack.firstElementChild.remove();
  if (ms) timers[t.id] = setTimeout(() => remove(t.id, true), ms);
  if (t.sound) chime();
  fit();
}

// The shutdown countdown ticks down by itself.
setInterval(() => {
  const el = stack.querySelector('[data-id="plan"] .title');
  if (!el) return;
  const m = el.textContent.match(/^(.*) in (\d+) s$/);
  if (m && +m[2] > 0) el.textContent = `${m[1]} in ${+m[2] - 1} s`;
}, 1000);

T.event.listen("toast", (e) => show(e.payload));
T.event.listen("toast-dismiss", (e) => remove(e.payload.id, true));
T.event.listen("changed", async (e) => {
  if (e.payload?.topic === "settings") {
    const s = await invoke("get_state");
    document.documentElement.style.setProperty("--acc", s.settings.accent);
  }
});
invoke("get_state").then((s) => document.documentElement.style.setProperty("--acc", s.settings.accent));
