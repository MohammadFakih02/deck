// Hardware pages: Fans, Lighting, Cooler display, Sensors (with the RTSS overlay) and This PC.
// The engine came from MoboCtl. Edits collect in a draft and reach the hardware on Apply.

const HW = {
  state: null,     // hw_state: what was found, lists, the applied configuration
  cfg: null,       // the draft the pages edit
  dirty: false,
  live: null,      // hw_live, once a second
  hist: null,
  table: null,
  sensDev: null,   // Sensors > All readings: the device shown
  sensQ: "",
  curveSel: 0,
  updaters: [],    // in-place live updates for the page on screen (no full redraw)
  lcdColor: "#ff2d55",
};

async function hwLoad(keepDraft) {
  HW.state = await invoke("hw_state");
  if (HW.state.ready && (!keepDraft || !HW.dirty || !HW.cfg)) {
    HW.cfg = structuredClone(HW.state.cfg);
    HW.dirty = false;
  }
}

/** Live values without redrawing the page (inputs keep focus, curves keep their drag). */
async function hwTick() {
  if (!HW.state?.ready) {
    const before = HW.state?.ready;
    await hwLoad(true);
    return HW.state.ready !== before;
  }
  try { HW.live = await invoke("hw_live"); } catch { return false; }
  for (const f of HW.updaters) { try { f(); } catch (e) { console.error(e); } }
  return false;
}

/** A value that follows the live readings. */
const liveText = (fn, cls) => liveSpan(fn, HW.updaters, cls);

function edit(fn) {
  fn(HW.cfg);
  HW.dirty = true;
  render();
}

/** Shown instead of a page when the engine can't run (not admin) or is still starting. */
function hwGate() {
  const st = HW.state;
  if (!st) return h("div", { class: "empty" }, "Loading…");
  if (!st.admin) {
    return h("div", { class: "card col setup-card" },
      h("div", { class: "tile-head" }, I("shield"), h("span", null, "One-time set up")),
      h("div", { class: "boost-say" }, "Hardware control needs admin rights."),
      h("div", { class: "dim" }, "Fans, lighting, the cooler display, CPU power limits and GPU tuning talk to the hardware directly, which Windows only allows with admin rights. One prompt sets Deck up to start with them; after that it never asks again."),
      h("div", null, h("button", { class: "btn primary lg", onclick: async () => {
        try { await invoke("elevate_setup"); note("Restarting Deck with admin rights…"); } catch (e) { note(String(e), true); }
      } }, I("shield"), "Set up")));
  }
  if (!st.ready) return h("div", { class: "empty" }, "Starting the hardware engine… (finding the fan chip and the lights takes a few seconds)");
  return null;
}

function applyBar() {
  if (!HW.dirty) return null;
  return h("div", { class: "hw-bar" },
    I("info"), h("span", { class: "grow" }, "Changes aren't on the hardware yet."),
    h("button", { class: "btn ghost sm", onclick: () => { HW.cfg = structuredClone(HW.state.cfg); HW.dirty = false; render(); } }, "Undo changes"),
    h("button", { class: "btn primary sm", onclick: hwApply }, I("check"), "Apply"));
}

async function hwApply() {
  try {
    await invoke("hw_apply", { cfg: HW.cfg });
    HW.dirty = false;
    await hwLoad();
    note("Applied");
  } catch (e) { note(String(e), true); }
  render();
}

function selectBox(value, options, onpick, attrs) {
  return h("select", { ...attrs, onchange: (e) => onpick(e.target.value) },
    options.map((o) => {
      const [v, label] = Array.isArray(o) ? o : [o, o];
      return h("option", { value: v, selected: String(v) === String(value) }, label);
    }));
}

/** `help`: a HELP key for a "?" after the title. */
function sliderRow(title, value, min, max, step, unit, onset, help) {
  const val = h("b", { class: "num" }, value + unit);
  const s = slider(value, min, max, step, (v) => (val.textContent = v + unit), (v) => onset(v));
  if (typeof title === "string") s.setAttribute("aria-label", title);
  return h("div", { class: "srow" }, h("span", { class: "dim" }, title, help ? helpTip(help) : null), s, val);
}

// ------------------------------------------------------------------- Fans

Pages.fans = {
  async load() { HW.curveUser = false; await hwLoad(true); },
  tick: hwTick,
  render() {
    HW.updaters = [];
    const el = h("div", null, head("Fans", "Case and CPU fans: follow a curve, hold a speed, or leave them to the BIOS. They go back to the BIOS when Deck quits."));
    const gate = hwGate();
    if (gate) return el.append(gate), el;
    const st = HW.state, c = HW.cfg, f = c.fans;
    const info = st.info;
    if (info.control === "readonly") {
      el.append(h("div", { class: "banner warn" }, I("info"), h("div", null, h("b", null, "Read-only: "), info.control_why || "This fan chip can be read but not controlled safely.")));
    } else if (info.control === "experimental" && !f.allow_experimental) {
      el.append(h("div", { class: "banner warn" }, I("info"), h("div", { class: "grow" }, h("b", null, "Not verified on this board. "), "Deck reads the fans but only controls them if you allow it below."), ));
    }
    const curves = f.curves.length ? f.curves : st.lists.default_curves;
    const modes = [...st.lists.fixed, ...curves.map(([n]) => "Curve: " + n)];
    const live = () => HW.live || {};
    const canControl = info.control === "verified" || (info.control === "experimental" && f.allow_experimental);
    if (f.channels.length > 1 && canControl) {
      const same = f.channels.every((ch) => ch.mode === f.channels[0].mode) ? f.channels[0].mode : "";
      el.append(h("div", { class: "card row wrap all-fans" }, I("fan"), h("b", null, "All fans"),
        h("div", { class: "seg" }, modes.filter((m) => m === "Auto (BIOS)" || m.startsWith("Curve: ")).map((m) =>
          h("button", { class: m === same ? "on" : "", onclick: () => edit(() => f.channels.forEach((ch) => (ch.mode = m))) }, m === "Auto (BIOS)" ? "BIOS" : m.slice(7)))),
        h("span", { class: "grow" }),
        selectBox(same.startsWith("Fixed") ? same : "", [["", "Fixed speed…"], ...modes.filter((m) => m.startsWith("Fixed")).map((m) => [m, m])], (v) => v && edit(() => f.channels.forEach((ch) => (ch.mode = v))))));
    }
    // the curve being edited (the fans list marks which fans use it)
    if (!f.curves.length) f.curves = structuredClone(st.lists.default_curves);
    if (!HW.curveUser) {
      const first = f.channels.find((ch) => ch.mode.startsWith("Curve: "));
      const k = first ? f.curves.findIndex(([n]) => n === first.mode.slice(7)) : -1;
      if (k >= 0) HW.curveSel = k;
    }
    HW.curveSel = Math.min(HW.curveSel, f.curves.length - 1);
    const sel = f.curves[HW.curveSel];
    const users = f.channels.filter((ch) => ch.mode === "Curve: " + sel[0]);
    const srcIdx = users[0]?.src ?? 0;
    const tempOf = (src) => live().source_t?.[src]; // the backend's own rule for each source

    // left: one row per fan
    const list = h("div", { class: "card fan-list" }, f.channels.map((ch, i) => {
      const curveName = ch.mode.startsWith("Curve: ") ? ch.mode.slice(7) : null;
      const row = h("div", { class: "fan-row" + (curveName === sel[0] ? " sel" : ""), title: curveName ? `Click to edit the ${curveName} curve` : "",
        onclick: (e) => {
          if (!curveName || e.target.closest("select")) return;
          const k = f.curves.findIndex(([n]) => n === curveName);
          if (k >= 0 && k !== HW.curveSel) { HW.curveSel = k; HW.curveUser = true; render(); }
        } },
        h("span", { class: "fan-ic" }, I("fan")),
        h("div", { class: "fan-name" },
          h("div", { class: "t ell" }, ch.name),
          curveName ? h("label", { class: "fan-follows" }, h("span", null, "follows", helpTip("fans.follows")), selectBox(ch.src, st.sources.map((s, k) => [k, s]), (v) => edit(() => (ch.src = +v)))) : null),
        h("div", { class: "fan-read" },
          h("div", { class: "fan-rpm" }, liveText(() => { const r = live().fan_rpm?.[i]; return r ? r.toLocaleString() : "0"; }, "num"), h("small", null, " RPM")),
          (() => {
            const bar = h("i");
            const pct = h("span", { class: "num faint" });
            const upd = () => { const p = live().fan_pct?.[i]; bar.style.width = (p ?? 0) + "%"; pct.textContent = p == null ? "BIOS" : Math.round(p) + "%"; };
            upd();
            HW.updaters.push(upd);
            return h("div", { class: "fan-bar" }, h("div", { class: "bar" }, bar), pct);
          })()),
        selectBox(ch.mode, modes, (v) => edit(() => (ch.mode = v)), { "aria-label": ch.name + " mode" }));
      // a fan that isn't spinning (nothing on the header, or stopped on purpose) is shown, but quieter
      const idle = () => row.classList.toggle("idle", !live().fan_rpm?.[i]);
      idle();
      HW.updaters.push(idle);
      return row;
    }));

    // right: the curve
    const ed = curveEditor({
      points: sel[1], x: [20, 100], y: [0, 100], marker: () => tempOf(srcIdx),
      onchange: (p) => { sel[1] = p; HW.dirty = true; renderBarOnly(); },
    });
    HW.updaters.push(() => ed.redraw());
    const curveCard = h("div", { class: "card col fan-curve" },
      h("div", { class: "tile-head" }, I("gauge"), h("span", null, "Fan curve"), helpTip("fans.curve"), h("span", { class: "grow" }), h("span", { class: "dim small", style: { textTransform: "none", letterSpacing: 0, fontWeight: 500 } }, "Drag a point · click to add · right-click to remove")),
      h("div", { class: "row wrap", style: { gap: "6px" } },
        f.curves.map(([n], k) => h("button", { class: "chip" + (k === HW.curveSel ? " on" : ""), onclick: () => { HW.curveSel = k; HW.curveUser = true; render(); } }, n)),
        h("span", { class: "grow" }),
        h("button", { class: "icon-btn sm", title: "Save as new, rename, delete, defaults", "aria-label": "Curve actions", onclick: (e) => popMenu(e.currentTarget, [
          ["Save as a new curve…", async () => {
            const n = await promptBox("New curve", "Name", sel[0] + " 2", "Add");
            if (!n || f.curves.some(([x]) => x === n)) return n && note("That name is taken", true);
            edit(() => { f.curves.push([n, structuredClone(sel[1])]); HW.curveSel = f.curves.length - 1; HW.curveUser = true; });
          }],
          ["Rename…", async () => {
            const n = await promptBox("Rename curve", "Name", sel[0], "Rename");
            if (!n || n === sel[0]) return;
            if (f.curves.some(([x]) => x === n)) return note("That name is taken", true);
            edit(() => { for (const ch of f.channels) if (ch.mode === "Curve: " + sel[0]) ch.mode = "Curve: " + n; sel[0] = n; });
          }],
          ["Put Silent, Balanced and Performance back", () => edit(() => {
            for (const [n, p] of st.lists.default_curves) { const ex = f.curves.find(([x]) => x === n); if (ex) ex[1] = structuredClone(p); else f.curves.push([n, structuredClone(p)]); }
          })],
          f.curves.length > 1 ? ["Delete this curve…", async () => {
            if (!(await confirmBox("Delete curve", `Delete "${sel[0]}"? Fans using it go back to the BIOS.`, "Delete", true))) return;
            edit(() => { for (const ch of f.channels) if (ch.mode === "Curve: " + sel[0]) ch.mode = "Auto (BIOS)"; f.curves.splice(HW.curveSel, 1); HW.curveSel = 0; });
          }, true] : null,
        ]) }, I("more"))),
      ed.el,
      h("div", { class: "dim small" }, users.length ? `Used by ${users.map((u) => u.name).join(", ")}. The marker follows ${st.sources[srcIdx]}.` : "No fan uses this curve yet. Pick it as a fan's mode on the left."));
    list.prepend(h("div", { class: "tile-head fan-list-head" }, I("fan"), h("span", null, "Fans"), helpTip("fans.mode")));
    el.append(h("div", { class: "fans-cols" }, list, curveCard));

    // behaviour
    el.append(h("div", { class: "section" }, h("h2", null, "Behaviour")));
    el.append(h("div", { class: "card col" },
      sliderRow("Slow down only after the temperature drops", f.hyst, 0, 10, 1, " °C", (v) => edit(() => (f.hyst = v)), "fans.hyst"),
      sliderRow("Wait between slow-downs", f.delay, 0, 30, 1, " s", (v) => edit(() => (f.delay = v)), "fans.delay"),
      sliderRow("Stop system fans below (0 = never)", f.stop, 0, 70, 1, " °C", (v) => edit(() => (f.stop = v)), "fans.stop"),
      info.control === "experimental" ? h("label", { class: "boost-opt" }, toggle(f.allow_experimental, (v) => edit(() => (f.allow_experimental = v))),
        h("div", null, h("div", { class: "t" }, "Allow control on this unverified board", helpTip("fans.experimental")), h("div", { class: "s" }, `${info.chip}. Deck reads it fine; writing fan speeds hasn't been checked on this board. Fans go back to the BIOS when Deck quits.`))) : null));
    if (HW.dirty) el.append(applyBar());
    return el;
  },
};

/** Updates just the Apply bar (a curve drag must not rebuild its own canvas). */
function renderBarOnly() {
  const content = document.getElementById("content");
  const old = content.querySelector(".hw-bar");
  const bar = applyBar();
  if (old && bar) old.replaceWith(bar);
  else if (!old && bar) content.firstChild.append(bar);
}

// ------------------------------------------------------------------- Lighting

const SWATCHES = ["#ff2d55", "#ff6a00", "#ffd60a", "#34c759", "#00c7be", "#009bde", "#0a84ff", "#5e5ce6", "#bf5af2", "#ff375f", "#ffffff"];

Pages.lighting = {
  async load() { await hwLoad(true); },
  tick: hwTick,
  render() {
    HW.updaters = [];
    const el = h("div", null, head("Lighting", "Board RGB, ARGB strips and RAM: one effect for all of them."));
    const gate = hwGate();
    if (gate) return el.append(gate), el;
    const st = HW.state, l = HW.cfg.lighting, lists = st.lists;
    const off = HW.live?.lights_off ?? st.lights_off;
    el.append(h("div", { class: "card row light-top" },
      h("div", { class: "light-dot", style: { background: off ? "#2a3142" : l.mode === "Rainbow" ? "conic-gradient(red, yellow, lime, cyan, blue, magenta, red)" : l.color } }),
      h("div", { class: "grow" },
        h("b", null, off ? "Lights are off" : `${l.mode}`),
        h("div", { class: "dim small" }, liveText(() => (HW.live?.status || st.status).rgb, "")),
        h("div", { class: "dim small" }, liveText(() => (HW.live?.status || st.status).ram, ""))),
      h("button", { class: "btn" + (off ? " primary" : ""), onclick: async () => {
        await run("hw_lights_off", { on: !off });
        await hwLoad(true); HW.live = await invoke("hw_live"); render();
      } }, I("bulb"), off ? "Turn lights on" : "Lights off")));

    el.append(h("div", { class: "section" }, h("h2", null, "Effect"), helpTip("light.effect")));
    el.append(h("div", { class: "card col" },
      h("div", { class: "mode-grid" }, lists.modes.map((m) => h("button", { class: "chip" + (m === l.mode ? " on" : ""), onclick: () => edit(() => (l.mode = m)) }, m))),
      h("div", { class: "dim small" }, MODE_HELP[l.mode] || ""),
      (l.mode === "Comet" ? (l.comet_colors || "My colour") === "My colour" // a cycling comet has no colour to pick
        : ["Static", "Breathing", "Wave", "Music", "Game"].includes(l.mode) || !["Rainbow", "Color Cycle", "Temperature", "Screen", "Off"].includes(l.mode))
        ? h("div", { class: "row wrap color-row" },
          h("input", { type: "color", "aria-label": "Pick any colour", title: "Pick any colour", value: l.color, onchange: (e) => edit(() => (l.color = e.target.value.toUpperCase())) }),
          SWATCHES.map((c) => h("button", { class: "sw" + (c.toLowerCase() === l.color.toLowerCase() ? " on" : ""), style: { background: c }, title: c, onclick: () => edit(() => (l.color = c.toUpperCase())) })))
        : null,
      sliderRow("Brightness", l.bright, 0, 100, 1, "%", (v) => edit(() => (l.bright = v)), "light.bright"),
      l.mode === "Comet" ? h("div", { class: "row wrap" }, h("span", { class: "dim small" }, "Comet colours"),
        seg((lists.comet_colors || ["My colour", "Cycle", "Rainbow"]).map((c) => [c, c]), l.comet_colors || "My colour", (v) => edit(() => (l.comet_colors = v)))) : null,
      l.mode !== "Static" && l.mode !== "Off" ? sliderRow("Speed", l.speed, 1, 100, 1, "", (v) => edit(() => (l.speed = v)), "light.speed") : null));

    if (l.mode === "Music") {
      el.append(h("div", { class: "section" }, h("h2", null, "Music")));
      el.append(h("div", { class: "card col" },
        h("div", { class: "row wrap" }, h("label", { class: "field" }, h("span", null, "Style"), selectBox(l.music_style, lists.music_styles, (v) => edit(() => (l.music_style = v)))),
          h("label", { class: "boost-opt", style: { alignSelf: "end" } }, toggle(l.music_rainbow, (v) => edit(() => (l.music_rainbow = v))), h("div", { class: "t" }, "Rainbow colours"))),
        sliderRow("Sensitivity", l.music_gain, 25, 400, 5, "%", (v) => edit(() => (l.music_gain = v)), "light.music")));
    }

    el.append(h("div", { class: "section" }, h("h2", null, "Games")));
    el.append(h("div", { class: "card col" },
      h("div", { class: "row" },
        h("label", { class: "boost-opt grow" }, toggle(l.auto_game, (v) => edit(() => (l.auto_game = v))), h("div", null, h("div", { class: "t" }, "Switch to the Game effect while a game is in front", helpTip("light.auto_game")), h("div", { class: "s" }, "Lights react to the game's sound (shots, explosions) and go back afterwards."))),
        liveText(() => (HW.live?.in_game ? "Game in front" : ""), "chip good num hide-empty")),
      h("div", { class: "row wrap" },
        h("label", { class: "field" }, h("span", null, "Game effect sound"), selectBox(l.game_sound, lists.game_sounds, (v) => edit(() => (l.game_sound = v)))),
        h("label", { class: "boost-opt", style: { alignSelf: "end" } }, toggle(l.game_only, (v) => edit(() => (l.game_only = v))), h("div", { class: "t" }, "Only the game's sound (not music or voice chat)", helpTip("light.game_only"))))));

    el.append(h("div", { class: "section" }, h("h2", null, "Where"), helpTip("light.zones")));
    const zones = ["Board 12V RGB", "ARGB header 1", "ARGB header 2", "RAM"];
    el.append(h("div", { class: "card col" },
      // RAM only when its RGB controller answered (a plain kit has nothing to light)
      h("div", { class: "zones" }, zones.map((z, i) => i === 3 && !st.status.ram_ok ? null : h("label", { class: "boost-opt" }, toggle(l.zones[i], (v) => edit(() => (l.zones[i] = v))), h("div", { class: "t" }, z)))),
      h("div", { class: "row wrap" },
        h("label", { class: "field" }, h("span", null, "LEDs on ARGB 1"), h("input", { type: "number", min: 1, max: 120, value: l.n1, onchange: (e) => edit(() => (l.n1 = Math.max(1, Math.min(120, +e.target.value || 60)))) })),
        h("label", { class: "field" }, h("span", null, "LEDs on ARGB 2"), h("input", { type: "number", min: 1, max: 120, value: l.n2, onchange: (e) => edit(() => (l.n2 = Math.max(1, Math.min(120, +e.target.value || 60)))) })),
        h("div", { class: "dim small grow", style: { alignSelf: "end" } }, "Count the LEDs on each strip or fan chain; wave and comet effects use it.", helpTip("light.leds")))));
    if (HW.dirty) el.append(applyBar());
    return el;
  },
};

const MODE_HELP = {
  Static: "One colour.", Breathing: "Fades your colour in and out.", Rainbow: "Every colour, flowing.", "Color Cycle": "All LEDs change colour together.",
  Wave: "Your colour runs along the strips.", Comet: "A light with a tail chases around.", Temperature: "Blue when cool, red when the CPU is hot.",
  Music: "Follows whatever is playing.", Screen: "Matches the colours on your screen.", Game: "Reacts to the game's sound.", Off: "Dark, but the board keeps its settings.",
};

// ------------------------------------------------------------------- Cooler display

Pages.lcd = {
  async load() { await hwLoad(true); },
  async tick() {
    await hwTick();
    return false;
  },
  render() {
    HW.updaters = [];
    const el = h("div", null, head("Cooler display", "The LED display on your cooler: sensors, a clock, text, your own design or a music meter."));
    const gate = hwGate();
    if (gate) return el.append(gate), el;
    const st = HW.state, d = HW.cfg.display, lists = st.lists, L = HW.cfg.lighting;
    const preview = lcdPreview(d, L);
    const offNow = d.off;
    el.append(h("div", { class: "lcd-top" },
      h("div", { class: "card col lcd-card" }, preview.el,
        d.content === "Design" ? h("div", { class: "row wrap" },
          h("span", { class: "dim small" }, "Paint:"),
          h("input", { type: "color", "aria-label": "Paint colour", value: HW.lcdColor, onchange: (e) => (HW.lcdColor = e.target.value) }),
          h("button", { class: "btn sm ghost", onclick: () => edit(() => (d.design = d.design.map(() => "#000000"))) }, "Clear"),
          h("button", { class: "btn sm ghost", onclick: () => edit(() => (d.design = d.design.map(() => HW.lcdColor.toUpperCase()))) }, "Fill"),
          h("button", { class: "btn sm ghost", title: "Start from what the Sensors view shows now", onclick: async () => {
            const g = await run("hw_lcd_grab", { display: d });
            edit(() => (d.design = g));
          } }, "Copy the sensors view")) : null,
        h("div", { class: "dim small" }, liveText(() => (HW.live?.status || st.status).disp, ""))),
      h("div", { class: "card col" },
        h("div", { class: "row" }, h("b", { class: "grow" }, offNow ? "Display is off" : "Display is on"),
          toggle(!offNow, async (v) => { await run("hw_display_off", { on: !v }); d.off = !v; HW.state.cfg.display.off = !v; render(); })),
        h("label", { class: "field" }, h("span", null, "Show", helpTip("lcd.show")), seg(lists.contents.map((c) => [c, c]), d.content, (v) => edit(() => (d.content = v)))),
        lcdContentOptions(d, lists))));

    el.append(h("div", { class: "section" }, h("h2", null, "Colour")));
    el.append(h("div", { class: "card col" },
      h("div", { class: "row wrap" },
        h("label", { class: "field" }, h("span", null, "Colours", helpTip("lcd.colors")), selectBox(d.color_mode, lists.dmodes, (v) => edit(() => (d.color_mode = v)))),
        d.color_mode === "Custom" || d.color_mode === "Breathing" ? h("label", { class: "field" }, h("span", null, "Colour"), h("input", { type: "color", value: d.color, onchange: (e) => edit(() => (d.color = e.target.value.toUpperCase())) })) : null),
      sliderRow("Brightness", d.bright, 0, 100, 1, "%", (v) => edit(() => (d.bright = v)), "lcd.bright"),
      sliderRow("Speed", d.speed, 1, 100, 1, "", (v) => edit(() => (d.speed = v)))));

    el.append(h("div", { class: "section" }, h("h2", null, "Night and alerts")));
    const hours = Array.from({ length: 24 }, (_, i) => [i, `${String(i).padStart(2, "0")}:00`]);
    el.append(h("div", { class: "card col" },
      h("label", { class: "boost-opt" }, toggle(d.night, (v) => edit(() => (d.night = v))), h("div", null, h("div", { class: "t" }, "Dim at night", helpTip("lcd.night")))),
      d.night ? h("div", { class: "row wrap" },
        h("label", { class: "field" }, h("span", null, "From"), selectBox(d.night_from, hours, (v) => edit(() => (d.night_from = +v)))),
        h("label", { class: "field" }, h("span", null, "Until"), selectBox(d.night_to, hours, (v) => edit(() => (d.night_to = +v)))),
        h("div", { class: "grow" }, sliderRow("Night brightness", d.night_bright, 0, 100, 1, "%", (v) => edit(() => (d.night_bright = v))))) : null,
      h("label", { class: "boost-opt" }, toggle(d.flash, (v) => edit(() => (d.flash = v))), h("div", null, h("div", { class: "t" }, "Flash red when the CPU gets hot", helpTip("lcd.flash")))),
      d.flash ? sliderRow("Flash from", d.flash_at, 50, 105, 1, " °C", (v) => edit(() => (d.flash_at = v))) : null));
    if (HW.dirty) el.append(applyBar());
    return el;
  },
};

function lcdContentOptions(d, lists) {
  const t = (key, label) => h("label", { class: "boost-opt" }, toggle(!!d[key], (v) => edit(() => (d[key] = v))), h("div", { class: "t" }, label));
  switch (d.content) {
    case "Sensors":
      return h("div", { class: "zones" },
        ["CPU temperature", "CPU load", "GPU temperature", "GPU load"].map((n, i) => h("label", { class: "boost-opt" }, toggle(d.show[i], (v) => edit(() => (d.show[i] = v))), h("div", { class: "t" }, n))),
        t("fahrenheit", "Fahrenheit"));
    case "Clock":
      return h("div", { class: "row wrap" }, t("clock24", "24-hour"), h("label", { class: "field" }, h("span", null, "Bottom row"), selectBox(d.clock_bottom, lists.clock_bottom, (v) => edit(() => (d.clock_bottom = v)))));
    case "Text":
      return h("div", { class: "col" },
        h("div", { class: "row wrap" },
          h("label", { class: "field grow" }, h("span", null, "Top line"), h("input", { type: "text", maxlength: 40, value: d.text_top, onchange: (e) => edit(() => (d.text_top = e.target.value)) })),
          h("label", { class: "field grow" }, h("span", null, "Bottom line"), h("input", { type: "text", maxlength: 40, value: d.text_bottom, onchange: (e) => edit(() => (d.text_bottom = e.target.value)) }))),
        t("scroll", "Scroll text that doesn't fit"));
    case "Design":
      return h("div", { class: "dim small" }, "Click or drag over the preview to paint. Black means off.");
    case "Animation":
      return h("label", { class: "field" }, h("span", null, "Animation"), seg(lists.anims.map((a) => [a, a]), d.anim, (v) => edit(() => (d.anim = v))));
    case "Music meter":
      return h("label", { class: "field" }, h("span", null, "Style"), seg(lists.music_disp.map((a) => [a, a]), d.music, (v) => edit(() => (d.music = v))));
    case "Rotate":
      return h("div", { class: "col" },
        h("div", { class: "zones" }, lists.contents.filter((c) => c !== "Rotate").map((c) => h("label", { class: "boost-opt" },
          toggle(d.rotate.includes(c), (v) => edit(() => { d.rotate = v ? [...d.rotate, c] : d.rotate.filter((x) => x !== c); })), h("div", { class: "t" }, c)))),
        sliderRow("Each for", d.rotate_secs, 2, 60, 1, " s", (v) => edit(() => (d.rotate_secs = v))));
  }
  return null;
}

/** The 84 LEDs, drawn from lcd::geometry() and refreshed from hw_lcd_preview. Paints in Design mode. */
function lcdPreview(d, lighting) {
  const geo = HW.state.geometry; // [[led, [x, y, w, h]]]
  const W = Math.max(...geo.map(([, r]) => r[0] + r[2])) + 4, H = Math.max(...geo.map(([, r]) => r[1] + r[3])) + 4;
  const cv = h("canvas", { class: "lcd-cv" });
  let colors = d.content === "Design" ? d.design.slice() : [];
  let painting = false;
  const draw = () => {
    const dpr = window.devicePixelRatio || 1;
    const w = cv.clientWidth || 360, ht = (w * H) / W;
    cv.style.height = ht + "px";
    cv.width = w * dpr; cv.height = ht * dpr;
    const c = cv.getContext("2d");
    c.scale((w * dpr) / W, (ht * dpr) / H);
    c.fillStyle = "#07080c"; c.fillRect(0, 0, W, H);
    for (const [i, [x, y, rw, rh]] of geo) {
      const col = colors[i];
      const lit = col && col !== "#000000";
      c.fillStyle = lit ? col : "#151922";
      if (lit) { c.shadowColor = col; c.shadowBlur = 6; } else c.shadowBlur = 0;
      c.beginPath();
      c.roundRect(x, y, rw, rh, Math.min(rw, rh) / 2);
      c.fill();
    }
  };
  const ledAt = (e) => {
    const r = cv.getBoundingClientRect();
    const x = ((e.clientX - r.left) / r.width) * W, y = ((e.clientY - r.top) / r.height) * H;
    const hit = geo.find(([, [gx, gy, gw, gh]]) => x >= gx - 1 && x <= gx + gw + 1 && y >= gy - 1 && y <= gy + gh + 1);
    return hit ? hit[0] : -1;
  };
  const paint = (e) => {
    const i = ledAt(e);
    if (i < 0) return;
    const col = e.buttons === 2 ? "#000000" : HW.lcdColor.toUpperCase();
    if (d.design[i] !== col) { d.design[i] = col; colors[i] = col; HW.dirty = true; draw(); renderBarOnly(); }
  };
  if (d.content === "Design") {
    cv.style.cursor = "crosshair";
    cv.addEventListener("pointerdown", (e) => { painting = true; S.interacting++; cv.setPointerCapture(e.pointerId); paint(e); });
    cv.addEventListener("pointermove", (e) => painting && paint(e));
    const stop = () => { if (painting) { painting = false; S.interacting = Math.max(0, S.interacting - 1); } };
    cv.addEventListener("pointerup", stop);
    cv.addEventListener("pointercancel", stop);
    cv.addEventListener("contextmenu", (e) => e.preventDefault());
  } else {
    // animate from the backend renderer, ~12 fps while the page is open
    let busy = false;
    const timer = setInterval(async () => {
      if (!document.body.contains(cv)) return clearInterval(timer);
      if (busy || !document.body.classList.contains("shown")) return;
      busy = true;
      try { colors = (await invoke("hw_lcd_preview", { display: d, lighting })).map((c) => c || "#000000"); draw(); } catch { /* engine busy */ }
      busy = false;
    }, 80);
  }
  new ResizeObserver(draw).observe(cv);
  return { el: cv };
}

// ------------------------------------------------------------------- Sensors

Pages.sensors = {
  async load() {
    await hwLoad(true);
    S.cache.rtss = await invoke("rtss_state").catch(() => null);
    if (HW.state.ready) [HW.table, HW.hist] = await Promise.all([invoke("hw_table"), invoke("hw_hist")]);
  },
  async tick() {
    if (!HW.state?.ready) return hwTick();
    try {
      [HW.live, HW.table, HW.hist, S.cache.rtss] = await Promise.all([invoke("hw_live"), invoke("hw_table"), invoke("hw_hist"), invoke("rtss_state")]);
    } catch { return false; }
    for (const f of HW.updaters) { try { f(); } catch (e) { console.error(e); } }
    return false;
  },
  render() {
    HW.updaters = [];
    // the in-game overlay has its own page now (Monitor → Overlay)
    // All readings first: every sensor at a glance; the graphs are a click away
    const tab = ["graphs", "readings", "alerts"].includes(S.cache.sensTab) ? S.cache.sensTab : "readings";
    const tabs = seg([["readings", "All readings"], ["graphs", "Graphs"], ["alerts", "Alerts"]], tab, (v) => { S.cache.sensTab = v; render(); });
    const el = h("div", null, head("Sensors", "Live graphs and every reading with min / max / average.", tab === "graphs" && HW.state?.ready ? layoutButton("sensors") : null, tabs));
    const gate = hwGate();
    if (gate) return el.append(gate), el;
    if (tab === "graphs") el.append(sensGraphs());
    else if (tab === "readings") el.append(sensReadings());
    else el.append(alertsCard());
    return el;
  },
};

/** A round number above `v` for a graph's top. */
function niceTop(v, floor) {
  const x = Math.max(floor, v || 0) * 1.15;
  const step = x > 200 ? 100 : x > 50 ? 25 : x > 20 ? 10 : 5;
  return Math.ceil(x / step) * step;
}

function sensGraphs() {
  const PAL = ["#ff9f43", "#bf5af2", "#ffd60a", "#ff6b9a", "#9aa3b5", "#00c7be", "#7c9cff", "#ff453a"];
  const CPU = "#4f8cff", GPU = "#3ddc97";
  const info = HW.state.info;
  // board temperatures (not the CPU's own, already drawn) and fans that spin
  const boardIdx = info.temp_names.map((n, i) => [n, i]).filter(([n]) => !/^CPU/i.test(n)).slice(0, 4);
  const spinning = () => (HW.hist?.rpm || []).map((q, i) => [q, i]).filter(([q]) => q.some((v) => v > 0));
  const l = () => HW.live || {};
  const legend = (label, fn, color) => h("span", { class: "legend" }, h("i", { style: { background: color } }), label, " ", liveText(fn, "num"));
  const last = (q) => { for (let i = (q || []).length - 1; i >= 0; i--) if (q[i] != null) return q[i]; return null; };
  const fmtW = (v) => (v == null ? "–" : Math.round(v) + " W");
  /** A card with a graph that redraws every second (and while it is being resized). */
  const graphCard = (icon, title, help, legends, draw, extra) => {
    const cv = h("canvas", { class: "graph" });
    const redraw = () => { if (cv.isConnected) draw(cv); };
    HW.updaters.push(redraw);
    requestAnimationFrame(redraw);
    new ResizeObserver(redraw).observe(cv);
    return h("div", { class: "card col graph-card" },
      h("div", { class: "tile-head" }, I(icon), h("span", null, title), help ? helpTip(help) : null, h("span", { class: "grow" }), legends || null),
      extra || null, h("div", { class: "graph-box" }, cv));
  };

  const defs = [
    { id: "temp", name: "Temperature", w: 6, build: () => {
      const card = graphCard("thermo", "Temperature · 5 min", null,
        [legend("CPU", () => fmtT(l().cpu_t), CPU), legend("GPU", () => fmtT(l().gpu_t), GPU)],
        (cv) => { const x = HW.hist || {}; const T = (a) => (a || []).map((v) => (v == null ? v : tempNum(v))); lineGraph(cv, [{ values: T(x.cpu_t), color: CPU }, { values: T(x.gpu_t), color: GPU }, ...boardIdx.map(([, i], k) => ({ values: T(x.board?.[i]), color: PAL[k] }))], { min: Math.round(tempNum(20)), max: Math.round(tempNum(100)), unit: "°" }); });
      if (boardIdx.length) card.append(h("div", { class: "row wrap legends" }, boardIdx.map(([n, i], k) => legend(n, () => fmtT(l().board_t?.[i]), PAL[k]))));
      return card;
    } },
    { id: "load", name: "Load", w: 6, build: () => graphCard("gauge", "Load · 5 min", null,
      [legend("CPU", () => fmtP(l().cpu_l), CPU), legend("GPU", () => fmtP(l().gpu_l), GPU)],
      (cv) => { const x = HW.hist || {}; lineGraph(cv, [{ values: x.cpu_l, color: CPU }, { values: x.gpu_l, color: GPU }], { min: 0, max: 100, unit: "%" }); }) },
    { id: "power", name: "Power", w: 6, build: () => graphCard("zap", "Power · 5 min", "sens.power",
      [legend("CPU", () => fmtW(last(HW.hist?.cpu_w)), CPU), legend("GPU", () => fmtW(last(HW.hist?.gpu_w)), GPU)],
      (cv) => {
        const x = HW.hist || {};
        const top = niceTop(Math.max(0, ...[...(x.cpu_w || []), ...(x.gpu_w || [])].filter((v) => v != null)), 50);
        lineGraph(cv, [{ values: x.cpu_w, color: CPU }, { values: x.gpu_w, color: GPU }], { min: 0, max: top, unit: "W" });
      }) },
    { id: "frames", name: "Frametimes", w: 6, build: () => {
      const r = () => S.cache.rtss || {};
      const fig = (label, key, tone) => h("div", { class: "ft-fig" + (tone ? " " + tone : "") }, h("span", { class: "dim small" }, label),
        liveText(() => { const v = r().stats?.[key]; return v == null ? "–" : Math.round(v) + ""; }, "num"));
      const figs = h("div", { class: "ft-figs" },
        h("div", { class: "ft-fig" }, h("span", { class: "dim small" }, "Now"), liveText(() => (r().fps == null ? "–" : Math.round(r().fps) + ""), "num")),
        fig("Average", "avg"), fig("1% low", "low1", "lo"), fig("0.1% low", "low01", "lo"), fig("Lowest", "min"), fig("Highest", "max"));
      const note = h("div", { class: "dim small ft-note" });
      const card = graphCard("activity", "Frametimes", "sens.frames",
        [liveText(() => (r().app ? r().app : ""), "chip num hide-empty"),
          h("button", { class: "btn sm ghost", title: "Start the average, lows, lowest and highest over" + (S.state.settings.osd_reset_hotkey ? " (" + S.state.settings.osd_reset_hotkey + ")" : ""),
            onclick: async () => { await invoke("rtss_reset_stats"); S.cache.rtss = await invoke("rtss_state"); for (const f of HW.updaters) { try { f(); } catch { /* redrawn next tick */ } } } }, I("refresh", "sm"), "Reset")],
        (cv) => {
          const ft = r().ft_recent || [];
          note.textContent = ft.length ? `The last ${ft.length} frames in ms (lower is smoother). Figures in FPS over the last ${fmtSecs(r().stats?.secs)}.`
            : !r().installed ? "RivaTuner Statistics Server isn't installed, so there are no frames to count."
            : !r().running ? "Start RivaTuner (Sensors → Overlay) and a game to see every frame here."
            : "No game is drawing right now. Start one and its frames appear here.";
          const top = niceTop(Math.max(0, ...ft.slice(-600)), 20);
          lineGraph(cv, [{ values: ft, color: "#ffd60a" }], { min: 0, max: top, unit: "", n: 600 });
        }, figs);
      card.append(note);
      return card;
    } },
  ];
  if (spinning().length) {
    const fans = spinning();
    defs.push({ id: "fans", name: "Fan speeds", w: 12, build: () => {
      const card = graphCard("fan", "Fan speeds · 5 min · RPM", null, null, (cv) => {
        const list = spinning();
        const top = Math.max(1000, ...list.flatMap(([q]) => q.filter((v) => v != null)));
        lineGraph(cv, list.map(([q, i]) => ({ values: q, color: PAL[i % PAL.length] })), { min: 0, max: Math.ceil(top / 500) * 500, unit: "" });
      });
      card.append(h("div", { class: "row wrap legends" }, fans.map(([, i]) => legend(info.fan_names[i] || "Fan " + (i + 1), () => (l().fan_rpm?.[i] ? l().fan_rpm[i].toLocaleString() : "0"), PAL[i % PAL.length]))));
      return card;
    } });
  }
  return panelGrid("sensors", defs);
}

const fmtSecs = (s) => (!s ? "minute" : s % 60 ? s + " s" : s === 60 ? "minute" : s / 60 + " minutes");

function alertsCard() {
  const a = HW.cfg.alerts;
  return h("div", { class: "card col" },
    h("div", { class: "tile-head" }, I("bell"), h("span", null, "Alerts and logging")),
    h("label", { class: "boost-opt" }, toggle(a.on, (v) => edit(() => (a.on = v))), h("div", null, h("div", { class: "t" }, "Warn me when it gets hot", helpTip("sens.alerts")), h("div", { class: "s" }, "A notification at most every 5 minutes per sensor."))),
    a.on ? sliderRow("CPU above", a.cpu, 60, 105, 1, " °C", (v) => edit(() => (a.cpu = v))) : null,
    a.on ? sliderRow("GPU above", a.gpu, 60, 100, 1, " °C", (v) => edit(() => (a.gpu = v))) : null,
    h("div", { class: "row" },
      h("label", { class: "boost-opt grow" }, toggle(a.log_csv, (v) => edit(() => (a.log_csv = v))), h("div", null, h("div", { class: "t" }, "Log every sensor to a CSV file", helpTip("sens.csv")), h("div", { class: "s" }, "Once a second to monitor.csv (starts over at 50 MB)."))),
      h("button", { class: "btn sm ghost", onclick: () => invoke("hw_open_folder") }, I("folder", "sm"), "Open folder")),
    applyBar());
}

// --------------------------------------------------------- All readings
// One device at a time (picked on the left). Rows that repeat per core or per fan
// ("P-core 3 temperature", "Pump duty") fold into one grid; the rest group by what they measure.

const SR_KINDS = [
  ["°C", "Temperatures"], ["°F", "Temperatures"], ["W", "Power"], ["MHz", "Clocks"], ["GHz", "Clocks"], ["%", "Load"], ["V", "Voltages"],
  ["RPM", "Fan speed"], ["GB", "Memory"], ["MB", "Memory"], ["MB/s", "Transfer"], ["KB/s", "Transfer"], ["Mb/s", "Transfer"],
];
const SR_METRICS = [["temperature", "Temp", "°C"], ["clock", "Clock", "MHz"], ["load", "Load", "%"], ["speed", "Speed", "RPM"], ["duty", "Duty", "%"]];
/** The grid column units, in the chosen units. */
const srMetricUnit = (u) => (u === "°C" ? tempUnit() : u === "MHz" && useGhz() ? "GHz" : u);

/** Rows as the engine sends them ("64.0 °C", "4800 MHz", "512.0 KB/s"), turned into the chosen units.
 * Warning colours convert back to °C (srTone). */
function srUnits(rows) {
  const conv = (r) => {
    const net = r.group.startsWith("Network") && r.unit === "KB/s";
    if (r.unit === "°C" && useF()) return ["°F", (v) => v * 9 / 5 + 32, 0];
    if (r.unit === "MHz" && useGhz()) return ["GHz", (v) => v / 1000, 2];
    if (net && !netBytes()) return ["Mb/s", (v) => (v * 1024 * 8) / 1e6, 1];
    if (net && netBytes()) return ["MB/s", (v) => v / 1024, 2];
    return null;
  };
  return rows.map((r) => {
    const c = conv(r);
    if (!c) return r;
    const [unit, f, dec] = c;
    const one = (s) => { const v = srNum(s); return v == null ? s : f(v).toFixed(dec) + " " + unit; };
    return { ...r, unit, cur: one(r.cur), min: one(r.min), max: one(r.max), avg: one(r.avg) };
  });
}
const SR_ICON = { CPU: "chip", GPU: "gpu", Motherboard: "fan", Memory: "gauge", Drive: "disk", Network: "wifi" };

const srNum = (s) => { const v = parseFloat(s); return Number.isFinite(v) ? v : null; };
const srBare = (s, unit) => (unit && s.endsWith(" " + unit) ? s.slice(0, -unit.length - 1) : s);

function srGroups(rows) {
  const out = [];
  for (const r of rows) {
    let g = out.find((x) => x.name === r.group);
    if (!g) {
      const [kind, ...rest] = r.group.split(": ");
      out.push((g = { name: r.group, kind, label: rest.join(": ") || kind, rows: [] }));
    }
    g.rows.push(r);
  }
  return out;
}

/** The two numbers a device shows in the list: how hot, and how busy. */
function srHeadline(g) {
  const temps = g.rows.filter((r) => (r.unit === "°C" || r.unit === "°F") && srNum(r.cur) != null);
  const hot = g.kind === "Motherboard" ? temps.sort((a, b) => srNum(b.cur) - srNum(a.cur))[0] : temps[0];
  const busy = g.rows.find((r) => /^(Total load|Core load|Physical memory load|Total activity|Download rate)$/.test(r.name));
  const fans = g.rows.filter((r) => r.unit === "RPM" && srNum(r.cur) > 0).length;
  return [hot ? Math.round(srNum(hot.cur)) + "°" : null, busy ? (busy.unit === "%" ? Math.round(srNum(busy.cur)) + "%" : busy.cur) : fans ? `${fans} fans` : null];
}

/** Splits a device's rows into per-item grids and plain sections. */
function srLayout(rows) {
  const ents = new Map();
  for (const r of rows) {
    const m = r.name.match(/^(.+) (temperature|clock|load|speed|duty)$/);
    if (!m) continue;
    if (!ents.has(m[1])) ents.set(m[1], {});
    ents.get(m[1])[m[2]] = r;
  }
  // a grid needs at least three items sharing two or more readings each
  const bySig = new Map();
  for (const [name, ms] of ents) {
    const keys = Object.keys(ms);
    if (keys.length < 2) continue;
    const sig = SR_METRICS.map(([k]) => k).filter((k) => keys.includes(k)).join(",");
    if (!bySig.has(sig)) bySig.set(sig, []);
    bySig.get(sig).push([name, ms]);
  }
  const grids = [...bySig].filter(([, items]) => items.length >= 3).map(([sig, items]) => ({ cols: sig.split(","), items }));
  const used = new Set(grids.flatMap((g) => g.items.flatMap(([, ms]) => Object.values(ms))));
  const secs = [];
  for (const r of rows) {
    if (used.has(r)) continue;
    const title = (SR_KINDS.find(([u]) => u === r.unit) || [, "Other"])[1];
    let s = secs.find((x) => x.title === title);
    if (!s) secs.push((s = { title, rows: [] }));
    s.rows.push(r);
  }
  const order = [...new Set(SR_KINDS.map(([, t]) => t)), "Other"];
  secs.sort((a, b) => order.indexOf(a.title) - order.indexOf(b.title));
  return { grids, secs };
}

function srTone(r, v) {
  if ((r?.unit !== "°C" && r?.unit !== "°F") || v == null) return "";
  if (r.unit === "°F") v = (v - 32) * 5 / 9; // limits are in °C
  return v >= 95 ? " tone-bad" : v >= 85 ? " tone-warn" : "";
}

function srGrid(g) {
  const title = g.items.some(([n]) => /core/i.test(n)) ? "Cores" : g.cols.includes("speed") ? "Fans" : "Per item";
  const meta = g.cols.map((c) => SR_METRICS.find(([k]) => k === c));
  return h("div", { class: "sr-block sr-wide" },
    h("div", { class: "sr-h" }, h("b", null, title), h("span", { class: "faint" }, "now  ·  highest")),
    h("table", { class: "sr-t sr-grid" },
      h("thead", null, h("tr", null, h("th", null, ""), meta.map(([, label, unit]) => h("th", { class: "r" }, label, h("span", { class: "faint" }, " " + srMetricUnit(unit)))))),
      h("tbody", null, g.items.map(([name, ms]) => h("tr", null,
        h("td", { class: "nm" }, name),
        meta.map(([k]) => {
          const r = ms[k];
          if (!r) return h("td");
          const v = srNum(r.cur);
          return h("td", { class: "r num", title: `min ${r.min} · avg ${r.avg}` },
            h("span", { class: "sr-now" + srTone(r, v) }, srBare(r.cur, r.unit)), h("span", { class: "sr-max" + srTone(r, srNum(r.max)) }, srBare(r.max, r.unit)));
        }))))));
}

function srSection(s) {
  const unit = s.rows.every((r) => r.unit === s.rows[0].unit) ? s.rows[0].unit : "";
  const cell = (r, v, cls) => h("td", { class: "r num " + cls }, unit ? srBare(v, r.unit) : v);
  return h("div", { class: "sr-block" },
    h("div", { class: "sr-h" }, h("b", null, s.title), unit ? h("span", { class: "faint" }, unit) : null),
    h("table", { class: "sr-t" },
      h("thead", null, h("tr", null, h("th", null, ""), ["Now", "Min", "Max", "Avg"].map((x) => h("th", { class: "r" }, x)))),
      h("tbody", null, s.rows.map((r) => h("tr", null,
        h("td", { class: "nm ell", title: r.name }, r.name),
        h("td", { class: "r num sr-now" + srTone(r, srNum(r.cur)) }, unit ? srBare(r.cur, r.unit) : r.cur),
        cell(r, r.min, "dim"), h("td", { class: "r num dim" + srTone(r, srNum(r.max)) }, unit ? srBare(r.max, r.unit) : r.max), cell(r, r.avg, "dim"))))));
}

function sensReadings() {
  const search = h("input", { type: "search", placeholder: "Find a reading…", value: HW.sensQ || "", spellcheck: false });
  const rail = h("div", { class: "sr-rail" });
  const detail = h("div", { class: "sr-detail" });
  const since = () => {
    const m = Math.max(0, Math.floor((Date.now() / 1000 - (HW.table?.since || 0)) / 60));
    return `Collecting for ${Math.floor(m / 60)} h ${String(m % 60).padStart(2, "0")} min`;
  };
  const fill = () => {
    const gs = srGroups(srUnits(HW.table?.rows || []));
    if (!gs.some((g) => g.name === HW.sensDev)) HW.sensDev = gs[0]?.name;
    const q = (HW.sensQ || "").trim().toLowerCase();
    rail.replaceChildren(...gs.map((g) => {
      const [a, b] = srHeadline(g);
      const hits = q ? g.rows.filter((r) => (g.name + " " + r.name).toLowerCase().includes(q)).length : null;
      return h("button", { class: "sr-dev" + (!q && g.name === HW.sensDev ? " on" : "") + (q && !hits ? " off" : ""), title: g.name,
        onmousedown: () => { HW.sensDev = g.name; HW.sensQ = ""; search.value = ""; fill(); } },
        h("span", { class: "sr-ic" }, I(SR_ICON[g.kind] || "gauge", "sm")),
        h("span", { class: "sr-dn" }, h("span", { class: "sr-kind" }, g.kind), g.label !== g.kind ? h("span", { class: "ell" }, g.label) : null),
        q ? h("span", { class: "sr-hl faint num" }, hits || "") : h("span", { class: "sr-hl num" }, a ? h("b", null, a) : null, b ? h("span", { class: "dim" }, b) : null));
    }));
    if (!gs.length) { detail.replaceChildren(h("div", { class: "empty" }, "Collecting readings…")); return; }
    const top = (title, ...right) => h("div", { class: "sr-top" }, h("h2", null, title), h("span", { class: "grow" }), ...right,
      h("span", { class: "faint small" }, since()),
      h("button", { class: "btn sm ghost", title: "Start min / max / average over from now", onclick: async () => { await run("hw_table_reset"); HW.table = await invoke("hw_table"); fill(); } }, I("refresh", "sm"), "Reset"));
    if (q) {
      const hits = gs.flatMap((g) => g.rows.filter((r) => (g.name + " " + r.name).toLowerCase().includes(q)).map((r) => [g, r]));
      detail.replaceChildren(top(`${hits.length} match${hits.length === 1 ? "" : "es"}`),
        hits.length ? h("div", { class: "sr-block sr-wide" }, h("table", { class: "sr-t" },
          h("thead", null, h("tr", null, h("th", null, ""), ["Now", "Min", "Max", "Avg"].map((x) => h("th", { class: "r" }, x)))),
          h("tbody", null, hits.slice(0, 120).map(([g, r]) => h("tr", null,
            h("td", { class: "nm" }, h("div", { class: "ell sr-hit", title: `${r.name} · ${g.name}` }, r.name, h("span", { class: "faint" }, "  " + (g.kind === "Drive" || g.kind === "Network" ? g.label.replace(/ \(Disk \d+\)$/, "") : g.kind)))),
            h("td", { class: "r num sr-now" + srTone(r, srNum(r.cur)) }, r.cur), h("td", { class: "r num dim" }, r.min), h("td", { class: "r num dim" }, r.max), h("td", { class: "r num dim" }, r.avg))))))
          : h("div", { class: "empty" }, `No reading has "${HW.sensQ.trim()}" in its name.`));
      return;
    }
    const g = gs.find((x) => x.name === HW.sensDev);
    const { grids, secs } = srLayout(g.rows);
    detail.replaceChildren(top(g.label, g.kind !== g.label ? h("span", { class: "chip" }, g.kind) : null),
      ...grids.map(srGrid),
      h("div", { class: "sr-secs" }, secs.map(srSection)));
  };
  search.addEventListener("input", () => { HW.sensQ = search.value; fill(); });
  search.addEventListener("keydown", (e) => { if (e.key === "Escape" && search.value) { e.stopPropagation(); HW.sensQ = ""; search.value = ""; fill(); } });
  fill();
  HW.updaters.push(fill);
  return h("div", { class: "sr" }, h("div", { class: "sr-side" }, search, rail), detail);
}

const fmtT = (v) => fmtTemp(v, true);
const fmtP = (v) => (v == null ? "–" : Math.round(v) + "%");

// ------------------------------------------------------------------- This PC

// ------------------------------------------------------------------- drive health (S.M.A.R.T.)

const DRIVE_STATUS = { good: ["good", "Good"], caution: ["warn", "Caution"], bad: ["bad", "Bad"], unknown: ["", "No health data"] };

/** Every drive's health, like CrystalDiskInfo: verdict, life left, temperature, hours, the reasons and the raw attributes. */
function drivesCard() {
  const list = S.cache.drives;
  const refresh = h("button", { class: "btn sm ghost", title: "Read the drives again", onclick: async () => {
    refresh.disabled = true;
    S.cache.drives = await invoke("drive_health", { refresh: true }).catch(() => S.cache.drives);
    render();
  } }, I("refresh", "sm"), "Check now");
  const card = h("div", { class: "card col drives" },
    h("div", { class: "tile-head" }, I("disk"), h("span", null, "Drives"), helpTip("pc.drives"), h("span", { class: "grow" }), refresh));
  if (!list) return card.append(h("div", { class: "dim" }, "Reading the drives…")), card;
  if (!list.length) return card.append(h("div", { class: "dim" }, "No drives could be read. Drive health needs Deck to run as admin.")), card;
  for (const d of list) {
    const [tone, word] = DRIVE_STATUS[d.status] || DRIVE_STATUS.unknown;
    const kind = [d.bus, d.status === "unknown" && !d.attrs.length ? null : d.ssd ? "SSD" : "HDD", d.size ? fmtBytes(d.size) : null].filter(Boolean).join(" · ");
    const facts = [
      d.temp != null ? fmtTemp(d.temp) : null,
      d.hours != null ? `${d.hours.toLocaleString()} h on` : null,
      d.cycles != null ? `${d.cycles.toLocaleString()} starts` : null,
      d.written != null ? `${fmtBytes(d.written)} written` : null,
    ].filter(Boolean);
    const open = S.cache.driveOpen === d.disk;
    card.append(h("div", { class: "drive" },
      h("div", { class: "drive-top" },
        h("i", { class: "dot " + (tone || "off") }),
        h("div", { class: "grow" }, h("div", { class: "drive-name" }, d.model), h("div", { class: "dim small" }, kind)),
        h("span", { class: "chip " + tone }, word + (d.health != null ? ` ${d.health}%` : ""))),
      facts.length ? h("div", { class: "drive-facts dim small" }, facts.join("  ·  ")) : null,
      d.reasons.length ? h("ul", { class: "drive-why " + tone }, d.reasons.map((r) => h("li", null, r))) : null,
      (d.history || []).length ? h("ul", { class: "drive-why past" }, d.history.map((r) => h("li", null, "Earlier (fine now): " + r))) : null,
      d.status === "unknown" ? h("div", { class: "dim small" }, d.bus === "USB" ? "USB enclosures usually hide the drive's health data." : "This drive's controller doesn't pass its health data to Windows.") : null,
      d.attrs.length ? h("button", { class: "btn sm ghost drive-more", onclick: () => { S.cache.driveOpen = open ? null : d.disk; render(); } }, open ? "Hide the raw values" : "Show the raw values") : null,
      open ? h("div", { class: "drive-attrs-wrap" }, h("table", { class: "sr-t drive-attrs" },
        h("thead", null, h("tr", null, ["ID", "Attribute", "Value", "Worst", "Limit", "Raw"].map((x, i) => h("th", { class: i > 1 ? "r" : null }, x)))),
        h("tbody", null, d.attrs.map((a) => h("tr", { class: a.threshold && a.threshold < 254 && a.current <= a.threshold ? "bad" : null },
          h("td", { class: "num" }, a.id.toString(16).toUpperCase().padStart(2, "0")), h("td", null, a.name),
          h("td", { class: "r num" }, a.current), h("td", { class: "r num" }, a.worst), h("td", { class: "r num" }, a.threshold || "–"), h("td", { class: "r num" }, a.raw.toLocaleString())))))) : null));
  }
  return card;
}

Pages.drives = {
  load() {
    // reading the drives can take a moment (a sleeping hard drive spins up): the page draws first
    invoke("drive_health").then((d) => { S.cache.drives = d; if (S.page === "drives") render(); }).catch(() => (S.cache.drives = []));
  },
  render() {
    const on = !!S.state?.settings?.drive_alerts;
    return h("div", null, head("Drives", "Each drive's health, like CrystalDiskInfo. Deck checks the drives every 10 minutes."),
      h("div", { class: "card", style: { marginBottom: "14px" } },
        ovSwitch("Drive notifications", "A notification when a drive gets worse (new bad sectors, read errors, health dropping), and once about problems a drive had in the past. Off: the health only shows here.", on,
          (v) => setSetting("drive_alerts", v).then(render))),
      drivesCard());
  },
};

/** Save report: asks first whether to add the lights test (it takes the lights over for ~15 s). */
async function saveReport() {
  const st = HW.state?.status;
  const lit = HW.state?.ready && (st?.rgb_ok || st?.ram_ok);
  let lights = false;
  if (lit) {
    const pick = await new Promise((resolve) => modal("Add a lights test?",
      h("div", { class: "dim" }, "For lighting problems: every light turns red, green and blue for about 15 seconds, and the report gets what each lighting controller answered. Your lighting comes back afterwards."),
      (close) => [
        h("button", { class: "btn ghost", onclick: () => { close(); resolve(null); } }, "Cancel"),
        h("button", { class: "btn", onclick: () => { close(); resolve(false); } }, "Save without it"),
        h("button", { class: "btn primary", onclick: () => { close(); resolve(true); } }, "Add the test"),
      ], { onDismiss: () => resolve(null) }));
    if (pick == null) return;
    lights = pick;
  }
  if (lights) note("Testing the lights… (about 15 s)");
  await run("support_report", { save: true, lights }, "Report saved");
}

/** Copy / save the support report: everything needed to fix a problem, with or without admin. */
function reportButtons() {
  return [
    h("button", { class: "btn", onclick: async () => { const t = await run("support_report", { save: false }); await navigator.clipboard.writeText(t); note("Report copied: paste it in the chat"); } }, I("copy"), "Copy report"),
    h("button", { class: "btn", onclick: saveReport }, I("save"), "Save report"),
  ];
}

Pages.thispc = {
  async load() {
    await hwLoad(true);
    S.cache.mem = await invoke("hw_mem").catch(() => null);
  },
  tick: hwTick,
  render() {
    HW.updaters = [];
    const el = h("div", null, head("This PC", "What Deck found and what it can control. Something not working? Copy the report and paste it in the chat: it has what's needed to fix it.", ...reportButtons()));
    const gate = hwGate();
    if (gate) return el.append(gate), el;
    const st = HW.state, i = st.info, s = st.status;
    const row = (ok, title, value, sub) => h("div", { class: "pc-row" },
      h("i", { class: "dot " + (ok === true ? "good" : ok === false ? "warn" : "off") }),
      h("div", { class: "k" }, title), h("div", { class: "grow" }, h("div", { class: "v" }, value), sub ? h("div", { class: "dim small" }, sub) : null));
    const board = `${i.board_vendor.replace("Micro-Star International Co., Ltd.", "MSI")} ${i.board_name}`.trim();
    el.append(h("div", { class: "card pc-list" },
      row(true, "Motherboard", board || "Unknown", i.bios ? "BIOS " + i.bios : null),
      row(!i.cpu_note, "Processor", i.cpu || "Unknown", i.cpu_note || null),
      row(!/no supported/i.test(i.gpu), "Graphics", i.gpu),
      row(i.control === "verified" ? true : i.control === "experimental" ? null : false, "Fans", i.fan_names.length ? `${i.fan_names.length} fans on ${i.chip}` : i.chip,
        i.control === "verified" ? "Control checked on this board" : i.control === "experimental" ? "Readable; control not verified on this board" : i.control_why),
      row(s.rgb_ok, "Board lighting", s.rgb),
      row(s.ram_ok, "RAM lighting", s.ram),
      row(s.disp_ok, "Cooler display", s.disp),
      row(true, "Memory", i.dimms.length ? [...i.dimms.reduce((m, [a, b]) => m.set(`${a} ${b}`.trim(), (m.get(`${a} ${b}`.trim()) || 0) + 1), new Map())].map(([n, c]) => (c > 1 ? `${c} × ${n}` : n)).join(" · ") : "–", S.cache.mem ? `${S.cache.mem.used.toFixed(1)} of ${S.cache.mem.total.toFixed(1)} GB in use` : null),
      row(st.admin, "Admin rights", st.admin ? "Deck runs with admin rights" : "Not admin")));
    el.append(h("div", { class: "row wrap", style: { marginTop: "14px" } },
      st.moboctl ? h("button", { class: "btn", onclick: async () => {
        if (!(await confirmBox("Import from MoboCtl", "Copy MoboCtl's fan curves, fan modes, lighting and display settings into Deck? Deck's current hardware settings are replaced.", "Import"))) return;
        try { note(await invoke("hw_import_moboctl")); await hwLoad(); render(); } catch (e) { note(String(e), true); }
      } }, I("download"), "Import MoboCtl settings") : null));
    return el;
  },
};

// ------------------------------------------------------------------- hotkey capture (shared)

/** Click, press the keys; Esc cancels, Backspace clears. `id` is the profile (for duplicate checks). */
function hotkeyField(value, id, onset) {
  // Esc cancels, Backspace clears
  const b = h("button", { class: "hk" + (value ? "" : " empty") }, value ? value.replaceAll("+", " + ") : "None");
  b.addEventListener("click", () => {
    b.classList.add("rec");
    b.textContent = "Press the keys…";
    const key = (e) => {
      e.preventDefault(); e.stopPropagation();
      if (e.key === "Escape") return done(null);
      if (e.key === "Backspace") return done("");
      if (["Control", "Alt", "Shift", "Meta"].includes(e.key)) return;
      const mods = [e.ctrlKey && "Ctrl", e.altKey && "Alt", e.shiftKey && "Shift", e.metaKey && "Win"].filter(Boolean);
      done([...mods, keyName(e)].join("+"));
    };
    const done = async (k) => {
      window.removeEventListener("keydown", key, true);
      b.classList.remove("rec");
      if (k === null) { b.textContent = value ? value.replaceAll("+", " + ") : "None"; return; }
      if (k === "") return onset("");
      const r = await invoke("hotkey_check", { key: k, id });
      if (!r.ok) { note(r.why, true); b.textContent = value ? value.replaceAll("+", " + ") : "None"; return; }
      onset(r.key);
    };
    window.addEventListener("keydown", key, true);
  });
  return b;
}

function keyName(e) {
  const c = e.code;
  // A numpad key that sends a navigation key (Num Lock off, or Fn on a laptop's built-in numpad:
  // Fn + 9 = Page Up) is that key to Windows, so the shortcut must be the key it sends, not "Num9".
  // (Fn itself never reaches Windows; the keyboard turns Fn + key into another key.)
  const nav = { PageUp: "PageUp", PageDown: "PageDown", Home: "Home", End: "End", Insert: "Insert", Delete: "Delete", ArrowUp: "Up", ArrowDown: "Down", ArrowLeft: "Left", ArrowRight: "Right" };
  if (/^Numpad/.test(c) && nav[e.key]) return nav[e.key];
  if (/^Key[A-Z]$/.test(c)) return c.slice(3);
  if (/^Digit\d$/.test(c)) return c.slice(5);
  if (/^F\d{1,2}$/.test(c)) return c;
  if (/^Numpad\d$/.test(c)) return "Num" + c.slice(6);
  const map = { BracketLeft: "[", BracketRight: "]", Backslash: "\\", Semicolon: ";", Quote: "'", Comma: ",", Period: ".", Slash: "/", Minus: "-", Equal: "=", Backquote: "`",
    PageUp: "PageUp", PageDown: "PageDown", Home: "Home", End: "End", Insert: "Insert", Delete: "Delete", ArrowUp: "Up", ArrowDown: "Down", ArrowLeft: "Left", ArrowRight: "Right",
    Space: "Space", Pause: "Pause", ScrollLock: "ScrollLock", Tab: "Tab", Enter: "Enter", NumpadMultiply: "Num*", NumpadAdd: "Num+", NumpadSubtract: "Num-", NumpadDecimal: "Num.", NumpadDivide: "Num/" };
  return map[c] || e.key.toUpperCase();
}
