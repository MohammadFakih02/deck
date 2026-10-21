// Monitor → Controller: the controller overlay (Controller Overlay, now part of Deck) and the
// battery of every controller. Settings live in settings.pad; the overlay itself is drawn by
// the backend (a native, click-through window), not by this page.
import { I, Pages, S, h, invoke, note, seg, setSetting, settingRow, slider, toggle, confirmBox } from "./core.js";
import { HEADS, tabsHead } from "./pages1.js";
import { DEFAULT_SUB, render } from "./app.js";

Object.assign(HEADS, {
  "controller/overlay": ["Controller", "Your controller's buttons and sticks on screen, over your games. A native overlay: click-through, and it never takes focus."],
  "controller/keys": ["Controller", "Keyboard & mouse mode: pick the keys the overlay shows."],
  "controller/battery": ["Controller", "Every controller that reports its battery. Each gets its own tray icon."],
  "controller/test": ["Controller", "The controller the overlay reads, live: press anything to see it here."],
});
DEFAULT_SUB.controller = "overlay";

const TABS = [["overlay", "Overlay"], ["keys", "Keys"], ["battery", "Battery"], ["test", "Test"]];
const THEMES = [["ds4", "DS4"], ["midnight", "Midnight"], ["dualsense", "DualSense"], ["gold", "Gold"], ["mono", "Mono"]];
const STYLES = [["glass", "Glass"], ["solid", "Solid"], ["minimal", "Buttons only"]];
const LAYOUTS = [["controller", "Controller"], ["compact", "Compact"]];
const LABELS = [["playstation", "PlayStation"], ["xbox", "Xbox"], ["auto", "Auto"]];
const MODES = [["controller", "Controller"], ["kbm", "Keyboard & mouse"]];
const CORNERS = [["top-left", "↖"], ["top-center", "↑"], ["top-right", "↗"], ["bottom-left", "↙"], ["bottom-center", "↓"], ["bottom-right", "↘"]];
const DEFAULTS = { anchor_h: "right", anchor_v: "bottom", off_x: 24, off_y: 24, monitor: "auto", scale: 1, opacity: 0.88, theme: "ds4", style: "glass", layout: "controller", labels: "playstation", fade: true, hide_no_pad: true, mode: "controller", keys: ["1", "2", "3", "4", "tab", "q", "w", "e", "r", "shift", "a", "s", "d", "f", "ctrl", "space"], mouse: true };

const pad = () => S.state.settings.pad || {};
const sub = () => S.sub || "overlay";

/** Changes some of the overlay's settings (the backend checks every value). */
async function setPad(changes) {
  await setSetting("pad", { ...pad(), ...changes });
  render();
}

async function act(what) {
  try {
    await invoke("pad_action", { what });
    setTimeout(async () => { await Pages.controller.load(); render(); }, 200);
  } catch (e) {
    note(String(e), true);
  }
}

function batteryText(r) {
  if (!r) return "";
  if (r.percent != null) return (r.charging ? "Charging · " : "") + r.percent + "%";
  if (r.charging) return "Charging";
  return "Battery " + (r.level || "unknown");
}

function batteryIcon(r) {
  const pct = r.percent != null ? r.percent : { empty: 5, low: 25, medium: 60, full: 100 }[r.level] ?? 100;
  const low = !r.charging && (r.percent != null ? r.percent <= 15 : r.level === "low" || r.level === "empty");
  return h("span", { class: "pad-bat" + (r.charging ? " chg" : low ? " low" : ""), "aria-hidden": "true" },
    h("i", { style: { width: Math.max(6, pct) + "%" } }));
}

// ------------------------------------------------------------------ Overlay tab

function statusLine(st, s) {
  if (!s.on) return "Off. Turn it on to see your controller over your games.";
  if (st.demo) return "Showing demo input (no controller needed).";
  if (s.mode === "kbm") return "Showing keyboard & mouse.";
  if (!st.connected) return st.note ? `“${st.note}” isn't recognised as a controller.` : "Waiting for a controller (USB or Bluetooth).";
  const b = st.battery ? " · " + batteryText(st.battery) : "";
  return `Showing ${st.name}${b}.`;
}

function overlayTab(st) {
  const s = pad();
  const row = settingRow;
  const corner = `${s.anchor_v}-${s.anchor_h}`;
  const monitors = st.monitors || [];
  const monSel = h("select", { "aria-label": "Monitor", onchange: (e) => setPad({ monitor: e.target.value === "auto" ? "auto" : +e.target.value }) },
    h("option", { value: "auto", selected: s.monitor === "auto" }, "The game's monitor"),
    monitors.map((m, i) => h("option", { value: String(i), selected: s.monitor === i }, `Monitor ${i + 1} · ${m.w}×${m.h}${m.primary ? " (main)" : ""}`)));
  const kbm = s.mode === "kbm";
  return h("div", null,
    h("div", { class: "card ov-strip pad-strip" + (s.on ? " on" : "") },
      h("label", { class: "switch" }, h("input", { type: "checkbox", checked: !!s.on, "aria-label": "Show the controller overlay", onchange: (e) => setPad({ on: e.target.checked }) }), h("i")),
      h("div", { class: "grow" },
        h("div", { class: "ov-title" }, s.on ? "Overlay on" : "Overlay off"),
        h("div", { class: "small dim" }, statusLine(st, s))),
      h("button", { class: "btn sm" + (st.edit ? " primary" : ""), onclick: () => act("edit") }, I("expand"), st.edit ? "Done moving" : "Move / resize"),
      h("button", { class: "btn sm" + (st.demo ? " primary" : ""), onclick: () => act("demo") }, I("play"), st.demo ? "Demo on" : "Demo input")),

    h("div", { class: "section" }, h("h2", null, "Look")),
    h("div", { class: "settings-card" },
      row("Show", "A controller, or keyboard & mouse (pick the keys in the Keys tab).", seg(MODES, s.mode, (v) => setPad({ mode: v }))),
      row("Theme", null, seg(THEMES, s.theme, (v) => setPad({ theme: v }))),
      row("Style", "Glass lets the game show through the body; Buttons only drops the body.", seg(STYLES, s.style, (v) => setPad({ style: v }))),
      kbm ? null : row("Layout", "A full controller, or a small strip of buttons.", seg(LAYOUTS, s.layout, (v) => setPad({ layout: v }))),
      kbm ? null : row("Button labels", "Auto picks from the controller's name.", seg(LABELS, s.labels, (v) => setPad({ labels: v }))),
      row("Size", `${Math.round(s.scale * 100)}%, on top of Windows' display scaling.`,
        h("div", { style: { width: "220px" } }, slider(Math.round(s.scale * 100), 50, 300, 5, null, (v) => setPad({ scale: v / 100 })))),
      row("Opacity", `${Math.round(s.opacity * 100)}%`,
        h("div", { style: { width: "220px" } }, slider(Math.round(s.opacity * 100), 15, 100, 1, null, (v) => setPad({ opacity: v / 100 }))))),

    h("div", { class: "section" }, h("h2", null, "Position")),
    h("div", { class: "settings-card" },
      row("Corner", "Move / resize lets you drag it anywhere instead.", seg(CORNERS, corner, (v) => {
        const [av, ah] = v.split("-");
        setPad({ anchor_v: av, anchor_h: ah, off_x: ah === "center" ? 0 : 24, off_y: 24 });
      })),
      row("Monitor", "The game's monitor follows whatever you're playing.", monSel),
      row("Move / resize", "Drag the overlay, scroll over it to resize, right-click it when you're done.",
        h("button", { class: "btn sm", onclick: () => act("edit") }, st.edit ? "Done" : "Move / resize"))),

    h("div", { class: "section" }, h("h2", null, "Behaviour")),
    h("div", { class: "settings-card" },
      row("Fade when idle", "After 6 seconds without input it fades to 30%.", toggle(!!s.fade, (v) => setPad({ fade: v }))),
      kbm ? null : row("Hide without a controller", "Nothing on screen until a controller connects.", toggle(!!s.hide_no_pad, (v) => setPad({ hide_no_pad: v }))),
      row("Hide from screenshots and recordings", "Off: it shows up in recordings and screen shares too. On: NVIDIA Instant Replay (ShadowPlay) turns itself off while the overlay is up, because it takes a hidden window for protected video.", toggle(!!s.capture_hidden, (v) => setPad({ capture_hidden: v }))),
      row("Reset the overlay", "Look, position and keys back to how they started.",
        h("button", { class: "btn sm", onclick: async () => { if (await confirmBox("Reset the overlay?", "Its look, position and keys go back to how they started.", "Reset")) setPad(DEFAULTS); } }, I("undo"), "Reset"))),
    h("p", { class: "small dim pad-foot" }, "The tray menu has the same switches under Controller overlay. Some games in exclusive fullscreen hide every outside overlay; borderless or windowed mode shows it."));
}

// ------------------------------------------------------------------ Keys tab

function keysTab(st) {
  const s = pad();
  const keys = st.key_list || [];
  const on = new Set(s.keys || []);
  const unit = 46;
  const rows = Math.max(0, ...keys.map((k) => k.row)) + 1;
  const board = h("div", { class: "pad-kb", style: { height: rows * unit + "px", width: 8 * unit + "px" }, role: "group", "aria-label": "Keys to show" },
    keys.map((k) => h("button", {
      class: "pad-key" + (on.has(k.id) ? " on" : ""),
      "aria-pressed": on.has(k.id) ? "true" : "false",
      style: { left: k.x * unit + "px", top: k.row * unit + "px", width: k.w * unit - 6 + "px" },
      onclick: () => setPad({ keys: on.has(k.id) ? s.keys.filter((x) => x !== k.id) : [...s.keys, k.id] }),
    }, k.label)));
  return h("div", null,
    h("div", { class: "section" }, h("h2", null, "Keys")),
    h("div", { class: "card" }, h("p", { class: "small dim" }, "Click a key to show or hide it. They keep their keyboard positions, so any set still looks like the corner of a keyboard."), board),
    h("div", { class: "settings-card", style: { marginTop: "12px" } },
      settingRow("Show the mouse", "Its buttons, the wheel and which way it's moving. Read through Windows Raw Input, which only listens: nothing is changed or blocked.", toggle(!!s.mouse, (v) => setPad({ mouse: v }))),
      s.mode !== "kbm" ? settingRow("Keyboard & mouse mode is off", "The overlay shows your controller now.", h("button", { class: "btn sm", onclick: () => setPad({ mode: "kbm" }) }, "Switch to keyboard & mouse")) : null));
}

// ------------------------------------------------------------------ Battery tab

function batteryTab(st) {
  const s = pad();
  const list = st.controllers || [];
  return h("div", null,
    h("div", { class: "section" }, h("h2", null, "Controllers")),
    list.length ? h("div", { class: "settings-card" }, list.map((c) => settingRow(
      h("span", { class: "pad-name" }, batteryIcon(c.reading), c.name),
      `${batteryText(c.reading)} · read from: ${c.source}`,
      null))) : h("div", { class: "card empty" }, s.battery_tray || s.on ? "No controller with a battery reading right now. Switch one on, or plug in its receiver." : "Battery reading is off (no tray icons and no overlay)."),
    h("div", { class: "section" }, h("h2", null, "Settings")),
    h("div", { class: "settings-card" },
      settingRow("A tray icon per controller", "Its battery like a phone's: the percentage inside, green while charging, red when low.", toggle(!!s.battery_tray, (v) => setPad({ battery_tray: v }))),
      settingRow("Warn when a battery gets low", "Once per charge, at 15%.", toggle(!!s.battery_warn, (v) => setPad({ battery_warn: v })))),
    h("p", { class: "small dim pad-foot" }, "Works with GameSir dongles, PlayStation controllers (read without changing anything on them), Xbox controllers and other pads Windows knows. The level a controller reports drops while you play and bounces back at rest, so like a phone the shown level only goes down until it charges."));
}

// ------------------------------------------------------------------ Test tab

const BTN_NAMES = { cross: "Cross / A", circle: "Circle / B", square: "Square / X", triangle: "Triangle / Y", up: "Up", down: "Down", left: "Left", right: "Right", l1: "L1 / LB", r1: "R1 / RB", l3: "L3", r3: "R3", create: "Share / View", options: "Options / Menu", ps: "PS / Guide", touchpad: "Touchpad" };
let testTimer = null;

function testTab(st) {
  const live = h("div", { class: "pad-live" });
  const draw = (v) => {
    live.replaceChildren(
      h("div", { class: "card pad-test-head" },
        h("b", null, v.connected ? v.name : "No controller"),
        h("div", { class: "small dim" }, v.connected
          ? `${v.vid ? "ID " + v.vid.toString(16).padStart(4, "0") + ":" + v.pid.toString(16).padStart(4, "0") + " · " : ""}${v.sdl ? "read through SDL" : "read through XInput"}${v.battery ? " · " + batteryText(v.battery) : ""}`
          : v.note ? `“${v.note}” is plugged in but isn't recognised as a controller.` : "Connect one by USB or Bluetooth.")),
      h("div", { class: "pad-btns" }, Object.entries(BTN_NAMES).map(([id, label]) => h("span", { class: "chip" + ((v.buttons || []).includes(id) ? " on" : "") }, label))),
      h("div", { class: "pad-axes" }, Object.entries(v.axes || {}).map(([a, x]) => h("div", { class: "pad-axis" },
        h("span", null, a.toUpperCase()),
        h("div", { class: "pad-bar" + (a.endsWith("2") ? " trig" : "") }, h("i", { style: a.endsWith("2") ? { width: x * 100 + "%" } : { left: Math.min(50, 50 + x * 50) + "%", width: Math.abs(x) * 50 + "%" } })),
        h("code", null, x.toFixed(2))))));
  };
  draw(st);
  clearInterval(testTimer);
  // a few times a second while this tab is on screen (no frame loop)
  testTimer = setInterval(async () => {
    if (!live.isConnected) { clearInterval(testTimer); testTimer = null; return; }
    if (document.visibilityState === "hidden") return;
    try { draw(await invoke("pad_state", { test: true })); } catch { /* Deck is busy: next time */ }
  }, 120);
  return h("div", null, live, h("p", { class: "small dim pad-foot" }, "The overlay doesn't need to be on for this. A controller that shows here but not on the overlay: check Hide without a controller and the monitor."));
}

// ------------------------------------------------------------------ the page

Pages.controller = {
  async load() {
    S.cache.pad = await invoke("pad_state", { test: sub() === "test" });
  },
  async tick() {
    if (sub() === "test") return false; // the Test tab updates itself
    const before = JSON.stringify(S.cache.pad);
    await Pages.controller.load();
    return JSON.stringify(S.cache.pad) !== before;
  },
  render() {
    const st = S.cache.pad || {};
    const t = sub();
    const el = h("div", { class: "page-narrow pad" }, tabsHead("controller", "Controller", TABS, t));
    el.append(t === "keys" ? keysTab(st) : t === "battery" ? batteryTab(st) : t === "test" ? testTab(st) : overlayTab(st));
    return el;
  },
};
