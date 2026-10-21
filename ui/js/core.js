// Shared helpers and state for Deck's pages.
import { icon } from "./icons.js";
import { HELP } from "./help.js";

export const T = window.__TAURI__;
const rawInvoke = (cmd, args) => T.core.invoke(cmd, args || {});

/** Into Deck's log (the page has none of its own). The same message at most once a minute;
 *  never throws. Commands are named, their arguments never sent. */
const logged = new Map();
export function logUi(level, msg) {
  try {
    const text = String(msg).slice(0, 600), now = Date.now();
    if (now - (logged.get(text) || 0) < 60000) return;
    if (logged.size > 300) logged.clear();
    logged.set(text, now);
    rawInvoke("ui_log", { level, msg: text }).catch(() => {});
  } catch { /* logging must never break the page */ }
}

const errText = (e) => (e instanceof Error ? e.message : typeof e === "string" ? e : JSON.stringify(e) || String(e));

/** A command; one that fails is logged (refusals like "already used" too: info, not warn). */
export const invoke = (cmd, args) => rawInvoke(cmd, args).catch((e) => {
  if (cmd !== "ui_log") logUi("info", `${cmd} failed: ${errText(e)}`);
  throw e;
});
export const listen = (ev, fn) => T.event.listen(ev, fn);

// script errors anywhere on the page; a rejected command was logged above (its error is a string)
window.addEventListener("error", (e) => logUi("error", `${e.message} at ${(e.filename || "").split("/").pop()}:${e.lineno}:${e.colno}`));
window.addEventListener("unhandledrejection", (e) => {
  if (typeof e.reason === "string") return;
  logUi("error", `unhandled: ${errText(e.reason)}${e.reason?.stack ? " | " + e.reason.stack.split("\n").slice(1, 3).join(" | ").trim() : ""}`);
});

export const S = {
  state: null,      // settings + static info
  live: null,       // vitals, media, volumes, power (refreshed every second while open)
  page: "home",
  query: "",
  results: null,
  sel: 0,
  busy: {},
  cache: {},        // per-page data
  interacting: 0,   // > 0 while a slider is being dragged
};

/** Every page: Pages[name] = { load, render, … }; each page file adds its own. */
export const Pages = {};

export const ICON_BASE = window.ICON_BASE || "http://icon.localhost/";
export const iconSrc = (kind, key) => (window.__iconSrc ? window.__iconSrc(kind, key) : ICON_BASE + kind + "/" + encodeURIComponent(key));

export function h(tag, props, ...kids) {
  const el = document.createElement(tag);
  if (props) {
    for (const [k, v] of Object.entries(props)) {
      if (v == null || v === false) continue;
      if (k === "class") el.className = v;
      else if (k === "style" && typeof v === "object") Object.assign(el.style, v);
      else if (k === "html") el.innerHTML = v;
      else if (k.startsWith("on") && typeof v === "function") el.addEventListener(k.slice(2), v);
      else if (k === "value" || k === "checked" || k === "disabled" || k === "selected") el[k] = v;
      else el.setAttribute(k, v === true ? "" : v);
    }
  }
  for (const c of kids.flat(Infinity)) {
    if (c == null || c === false || c === "") continue;
    el.append(c instanceof Node ? c : document.createTextNode(String(c)));
  }
  return el;
}

export function I(name, cls) {
  const t = document.createElement("template");
  t.innerHTML = icon(name, cls);
  return t.content.firstChild;
}

/** App/file icon with a letter placeholder if Windows has none. */
export function appIcon(kind, key, name, cls) {
  const letter = (name || "?").replace(/[^\p{L}\p{N}]/gu, "").slice(0, 1).toUpperCase() || "?";
  const ph = () => h("div", { class: "ph " + (cls || "") }, letter);
  if (!key) return ph();
  const img = h("img", { class: cls || "", src: iconSrc(kind, key), alt: "", draggable: "false", loading: "lazy" });
  img.onerror = () => img.replaceWith(ph());
  return img;
}

export function fmtBytes(b) {
  if (b == null) return "–";
  const u = ["B", "KB", "MB", "GB", "TB"];
  let i = 0;
  let v = b;
  while (v >= 1024 && i < u.length - 1) { v /= 1024; i++; }
  return i === 0 ? `${b} B` : `${v >= 100 ? v.toFixed(0) : v.toFixed(1)} ${u[i]}`;
}

// ---- units chosen in Settings > General > Units. Only shown readings convert; sliders,
// curves and offsets you set stay in MHz and °C (what the hardware and other tools use).
export const units = () => S.state?.settings || {};
export const useGhz = () => units().unit_clock === "GHz";
export const useF = () => units().unit_temp === "F";
export const netBytes = () => units().unit_net === "MB/s";

/** A clock speed given in MHz, in the chosen unit. */
export function fmtClock(mhz) {
  if (mhz == null || !Number.isFinite(+mhz)) return "–";
  return useGhz() ? (mhz / 1000).toFixed(2) + " GHz" : Math.round(mhz).toLocaleString() + " MHz";
}
/** A temperature given in °C, as a number in the chosen unit. */
export const tempNum = (c) => (c == null ? null : useF() ? c * 9 / 5 + 32 : c);
export const tempUnit = () => (useF() ? "°F" : "°C");
/** A temperature given in °C, in the chosen unit ("64 °C"; `short` gives "64°"). */
export function fmtTemp(c, short) {
  if (c == null || !Number.isFinite(+c)) return "–";
  return Math.round(tempNum(c)) + (short ? "°" : " " + tempUnit());
}

/** A network speed given in bytes per second: bits (Mb/s) or bytes (MB/s), as chosen. */
export function fmtRate(bps) {
  if (netBytes()) {
    const b = bps || 0;
    if (b >= 1024 ** 3) return (b / 1024 ** 3).toFixed(1) + " GB/s";
    if (b >= 1024 ** 2) return (b / 1024 ** 2).toFixed(1) + " MB/s";
    if (b >= 1024) return (b / 1024).toFixed(0) + " KB/s";
    return b.toFixed(0) + " B/s";
  }
  const bits = (bps || 0) * 8;
  if (bits >= 1e9) return (bits / 1e9).toFixed(1) + " Gb/s";
  if (bits >= 1e6) return (bits / 1e6).toFixed(1) + " Mb/s";
  if (bits >= 1e3) return (bits / 1e3).toFixed(0) + " kb/s";
  return bits.toFixed(0) + " b/s";
}
/** A speed already in megabits per second (speed tests), in the chosen unit: [number text, unit]. */
export const mbpsParts = (mbps) => (mbps == null ? ["–", netBytes() ? "MB/s" : "Mb/s"] : netBytes() ? [(mbps / 8).toFixed(1), "MB/s"] : [(+mbps).toFixed(mbps >= 100 ? 0 : 1), "Mb/s"]);

export function fmtDur(s) {
  s = Math.max(0, Math.round(s));
  const hh = Math.floor(s / 3600), mm = Math.floor((s % 3600) / 60), ss = s % 60;
  if (hh) return `${hh}:${String(mm).padStart(2, "0")}:${String(ss).padStart(2, "0")}`;
  return `${mm}:${String(ss).padStart(2, "0")}`;
}

export function fmtWords(s) {
  s = Math.round(s);
  if (s < 60) return `${s} s`;
  const m = Math.round(s / 60);
  if (m < 60) return `${m} min`;
  const hh = Math.floor(m / 60), mm = m % 60;
  return mm ? `${hh} h ${mm} min` : `${hh} h`;
}

export function ago(ms) {
  const s = (Date.now() - ms) / 1000;
  if (s < 60) return "just now";
  if (s < 3600) return `${Math.floor(s / 60)} min ago`;
  if (s < 86400) return `${Math.floor(s / 3600)} h ago`;
  const d = Math.floor(s / 86400);
  return d === 1 ? "yesterday" : `${d} days ago`;
}

export let toastTimer;
export function note(msg, err) {
  const t = document.getElementById("toast");
  t.textContent = msg;
  t.className = "toast-in show" + (err ? " err" : "");
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => (t.className = "toast-in" + (err ? " err" : "")), err ? 4200 : 2200);
}

/** invoke with the error shown to the user. */
export async function run(cmd, args, okMsg) {
  try {
    const r = await invoke(cmd, args);
    if (okMsg) note(okMsg);
    return r;
  } catch (e) {
    note(String(e), true);
    throw e;
  }
}

/** Settings this page wrote itself recently: { key: [value, when done (0 = still being written)] }.
 *  A "settings changed" reload can answer from before the write landed (two writes in a row: the
 *  reload of the first can miss the second); these values win over such a stale answer. */
export const localWrites = new Map();

export async function setSetting(key, value) {
  const before = S.state.settings[key];
  S.state.settings[key] = value;
  localWrites.set(key, [value, 0]);
  try {
    await invoke("set_setting", { key, value });
  } catch (e) {
    note(String(e), true);
    // not applied: a redraw must show what is really in effect
    if (localWrites.get(key)?.[0] === value) {
      localWrites.delete(key);
      S.state.settings[key] = before;
    }
    return;
  } finally {
    if (localWrites.get(key)?.[0] === value) localWrites.set(key, [value, Date.now()]);
  }
}

/** A reloaded state, with this page's own recent writes kept over what the reload says. */
export function withLocalWrites(state) {
  for (const [k, [v, done]] of localWrites) {
    if (done && Date.now() - done > 3000) localWrites.delete(k);
    else if (state?.settings) state.settings[k] = v;
  }
  return state;
}

export function toggle(checked, onchange) {
  return h("label", { class: "switch" },
    h("input", { type: "checkbox", checked, onchange: (e) => onchange(e.target.checked) }),
    h("i"));
}

/** Range input with the filled part coloured. `oninput` fires live. */
export function slider(value, min, max, step, oninput, onchange) {
  const el = h("input", { type: "range", min, max, step, value });
  const paint = () => el.style.setProperty("--p", ((el.value - min) / (max - min)) * 100 + "%");
  paint();
  el.addEventListener("pointerdown", () => S.interacting++);
  const up = () => { S.interacting = Math.max(0, S.interacting - 1); };
  el.addEventListener("pointerup", up);
  el.addEventListener("pointercancel", up);
  el.addEventListener("input", () => { paint(); oninput && oninput(+el.value); });
  el.addEventListener("change", () => onchange && onchange(+el.value));
  return el;
}

export function seg(options, current, onpick) {
  return h("div", { class: "seg", role: "group" }, options.map(([v, label]) =>
    h("button", { class: v === current ? "on" : "", "aria-pressed": v === current ? "true" : "false", onclick: () => onpick(v) }, label)));
}

/** A "?" that explains a setting: hover or focus shows the card, a click keeps it open.
 *  `key` names an entry in HELP: { t: title, what, why, change, warn }. */
export function helpTip(key) {
  const x = typeof HELP !== "undefined" && HELP[key];
  if (!x) return null;
  const btn = h("button", { type: "button", class: "help-q", "aria-label": "About " + x.t, "aria-expanded": "false" }, "?");
  let card = null, pinned = false;
  const hide = () => { if (card) { card.remove(); card = null; } btn.setAttribute("aria-expanded", "false"); };
  const show = () => {
    if (card) return;
    document.querySelectorAll(".help-card").forEach((c) => c.__hide && c.__hide());
    const part = (label, text) => text ? h("div", { class: "help-part" }, h("b", null, label), h("p", null, text)) : null;
    card = h("div", { class: "help-card", role: "tooltip" },
      h("div", { class: "help-t" }, x.t),
      part("What it does", x.what), part("Why use it", x.why), part("When you change it", x.change),
      x.warn ? h("div", { class: "help-warn" }, I("alert", "sm"), h("span", null, x.warn)) : null);
    card.__hide = () => { pinned = false; hide(); };
    card.__btn = btn;
    document.body.append(card);
    btn.setAttribute("aria-expanded", "true");
    const r = btn.getBoundingClientRect(), c = card.getBoundingClientRect();
    const left = Math.max(8, Math.min(r.left + r.width / 2 - c.width / 2, innerWidth - c.width - 8));
    const below = r.bottom + 8 + c.height < innerHeight - 8;
    card.style.left = left + "px";
    card.style.top = (below ? r.bottom + 8 : Math.max(8, r.top - c.height - 8)) + "px";
  };
  btn.addEventListener("mouseenter", show);
  btn.addEventListener("mouseleave", () => { if (!pinned) hide(); });
  btn.addEventListener("focus", show);
  btn.addEventListener("blur", () => { pinned = false; hide(); });
  btn.addEventListener("keydown", (e) => { if (e.key === "Escape" && card) { e.stopPropagation(); pinned = false; hide(); } });
  // inside a label a click would also flip the switch next to it
  btn.addEventListener("click", (e) => { e.preventDefault(); e.stopPropagation(); pinned = !pinned; pinned ? show() : hide(); });
  return btn;
}
// a redraw that removes a "?" takes its card along
new MutationObserver(() => document.querySelectorAll(".help-card").forEach((c) => c.__btn && !c.__btn.isConnected && c.__hide()))
  .observe(document.documentElement, { childList: true, subtree: true });

export const FOCUSABLE = "button:not([disabled]), [href], input:not([disabled]):not([type=hidden]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex='-1'])";
export let modalSeq = 0;

/** A dialog. `opts.wide`: the large scrolling editor layout; `opts.onDismiss`: runs when it is
 *  closed without a button (click outside, Escape, Deck hiding). `opts.keep`: stays open while
 *  Deck is hidden (an editor you alt-tab away from, e.g. to try a profile in a game). Returns close().
 *  Focus moves into the dialog, Tab stays inside it, and focus goes back to the opener on close. */
export function modal(title, body, actions, opts = {}) {
  const opener = document.activeElement;
  const back = h("div", { class: "modal-back", onclick: (e) => { if (e.target === back) back.__close(); } });
  const close = () => {
    back.remove();
    if (opener && opener.isConnected && typeof opener.focus === "function") opener.focus({ preventScroll: true });
  };
  back.__dismiss = opts.onDismiss; // Escape / Deck hiding remove modals from outside: they call __close
  back.__close = () => { close(); opts.onDismiss && opts.onDismiss(); };
  back.__keep = !!opts.keep;
  const titleId = "modal-t" + ++modalSeq;
  const box = h("div", { class: "modal" + (opts.wide ? " wide" : ""), role: "dialog", "aria-modal": "true", "aria-labelledby": titleId },
    h("h3", { id: titleId }, title),
    opts.wide ? h("div", { class: "modal-body" }, body) : body,
    h("div", { class: "actions" }, actions(close)));
  box.addEventListener("keydown", (e) => {
    if (e.key !== "Tab") return;
    const f = [...box.querySelectorAll(FOCUSABLE)].filter((x) => x.offsetParent !== null);
    if (!f.length) return;
    const i = f.indexOf(document.activeElement);
    if (e.shiftKey && i <= 0) { e.preventDefault(); f[f.length - 1].focus(); }
    else if (!e.shiftKey && i === f.length - 1) { e.preventDefault(); f[0].focus(); }
  });
  back.append(box);
  document.body.append(back);
  // a field if there is one, otherwise the first button (Cancel comes first, so Enter never confirms by accident)
  const first = box.querySelector("input, textarea, select") || box.querySelector(".actions button") || box.querySelector(FOCUSABLE);
  if (first) first.focus({ preventScroll: true });
  return close;
}

/** Gives switches, sliders and icon-only buttons a name screen readers can say, from the text next
 *  to them, when the page didn't give one. Runs after every redraw (see app.js). */
export function nameControls(root) {
  for (const i of root.querySelectorAll(".switch input")) {
    if (!i.hasAttribute("role")) i.setAttribute("role", "switch");
    if (!i.hasAttribute("aria-label") && !hasLabelText(i)) { const t = nearText(i.closest(".switch")); if (t) i.setAttribute("aria-label", t); }
  }
  for (const i of root.querySelectorAll("input[type=range]:not([aria-label])")) {
    if (!hasLabelText(i)) { const t = nearText(i); if (t) i.setAttribute("aria-label", t); }
  }
  for (const s of root.querySelectorAll("select:not([aria-label])")) {
    if (!hasLabelText(s)) { const t = nearText(s); if (t) s.setAttribute("aria-label", t); }
  }
  for (const b of root.querySelectorAll("button:not([aria-label])")) {
    if (!b.textContent.trim() && b.title) b.setAttribute("aria-label", b.title);
  }
}
export const hasLabelText = (i) => i.labels && [...i.labels].some((l) => l.textContent.trim());
/** The first words near `el` that aren't a number or a unit, looking a few levels up. */
export function nearText(el) {
  const skip = /^[\d\s.,:%°+\-–/×x]*(°C|%|MHz|GHz|W|RPM|ms|s|GB|MB|KB|mV|V|dB|Hz)?[\d\s.,:%°+\-–/]*$/i;
  for (let n = el.parentElement, depth = 0; n && depth < 4; n = n.parentElement, depth++) {
    const walk = document.createTreeWalker(n, NodeFilter.SHOW_TEXT, { acceptNode: (t) => (t.parentElement.closest("button, select, option, input, .switch, svg") ? NodeFilter.FILTER_REJECT : NodeFilter.FILTER_ACCEPT) });
    for (let t = walk.nextNode(); t; t = walk.nextNode()) {
      const s = t.textContent.trim();
      if (s && !skip.test(s)) return s.slice(0, 80);
    }
  }
  return "";
}

/** Text that follows live readings: `list` collects the updaters the page's tick runs. */
export function liveSpan(fn, list, cls) {
  const el = h("span", { class: cls ?? "num" }, fn() ?? "–");
  list.push(() => { el.textContent = fn() ?? "–"; });
  return el;
}

/** Title + description on the left, a control on the right (settings-style row). */
export function settingRow(title, desc, control) {
  // the row's title names its switch or slider for screen readers
  const field = control instanceof Element ? (control.matches("input, select") ? control : control.querySelector("input, select")) : null;
  const name = typeof title === "string" ? title : title instanceof Element ? title.firstChild?.textContent : null; // a title can carry a "?"
  if (field && name && !field.hasAttribute("aria-label")) field.setAttribute("aria-label", name);
  return h("div", { class: "setting" }, h("div", { class: "grow" }, h("div", { class: "t" }, title), desc ? h("div", { class: "d" }, desc) : null), control);
}

export function confirmBox(title, text, okLabel, danger) {
  return new Promise((resolve) => {
    modal(title, h("div", { class: "dim" }, text), (close) => [
      h("button", { class: "btn ghost", onclick: () => { close(); resolve(false); } }, "Cancel"),
      h("button", { class: "btn " + (danger ? "danger" : "primary"), onclick: () => { close(); resolve(true); } }, okLabel),
    ], { onDismiss: () => resolve(false) });
  });
}

export function promptBox(title, label, value, okLabel) {
  return new Promise((resolve) => {
    const input = h("input", { type: "text", value: value || "" });
    modal(title, h("label", { class: "field" }, h("span", null, label), input), (close) => {
      const ok = () => { close(); resolve(input.value.trim()); };
      input.addEventListener("keydown", (e) => { if (e.key === "Enter") ok(); });
      return [
        h("button", { class: "btn ghost", onclick: () => { close(); resolve(null); } }, "Cancel"),
        h("button", { class: "btn primary", onclick: ok }, okLabel || "Save"),
      ];
    }, { onDismiss: () => resolve(null) });
  });
}

/** Sparkline on a canvas. */
export function spark(canvas, values, max, color) {
  const dpr = window.devicePixelRatio || 1;
  const w = canvas.clientWidth || 120, hgt = canvas.clientHeight || 26;
  canvas.width = w * dpr; canvas.height = hgt * dpr;
  const c = canvas.getContext("2d");
  c.scale(dpr, dpr);
  if (!values || values.length < 2) return;
  const m = max || Math.max(...values, 1);
  const n = 60;
  const step = w / (n - 1);
  const off = n - values.length;
  c.beginPath();
  values.forEach((v, i) => {
    const x = (i + off) * step, y = hgt - 2 - (Math.min(v, m) / m) * (hgt - 4);
    i ? c.lineTo(x, y) : c.moveTo(x, y);
  });
  c.strokeStyle = color; c.lineWidth = 1.6; c.lineJoin = "round"; c.stroke();
  c.lineTo((values.length - 1 + off) * step, hgt); c.lineTo(off * step, hgt); c.closePath();
  const g = c.createLinearGradient(0, 0, 0, hgt);
  g.addColorStop(0, color + "44"); g.addColorStop(1, color + "00");
  c.fillStyle = g; c.fill();
}

export function accent() {
  return getComputedStyle(document.documentElement).getPropertyValue("--acc").trim() || "#4f8cff";
}

export function parseDuration(s) {
  // "10m", "1h30", "90s", "25", "1:30"
  s = (s || "").trim().toLowerCase();
  if (!s) return 0;
  if (/^\d+:\d{1,2}$/.test(s)) { const [a, b] = s.split(":"); return (+a) * 60 + (+b); }
  let total = 0, found = false;
  const re = /(\d+(?:\.\d+)?)\s*(h|hr|hrs|hour|hours|m|min|mins|minute|minutes|s|sec|secs|second|seconds)?/g;
  let m;
  while ((m = re.exec(s))) {
    found = true;
    const v = parseFloat(m[1]);
    const u = m[2] || "m";
    total += u.startsWith("h") ? v * 3600 : u.startsWith("s") ? v : v * 60;
  }
  return found ? Math.round(total) : 0;
}
