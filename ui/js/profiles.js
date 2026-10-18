// CPU and GPU pages: profiles you switch with a click, a keyboard shortcut, a desktop shortcut,
// the tray or the phone. CPU profiles are Windows power plans plus extras; GPU profiles are
// what MSI Afterburner kept in its five slots (power, clocks, curve, voltage, fan).
import { I, Pages, S, accent, ago, confirmBox, fmtClock, fmtTemp, h, helpTip, invoke, liveSpan, modal, note, promptBox, run, seg, setSetting, settingRow, slider, spark, toggle } from "./core.js";
import { watts } from "./home.js";
import { hover } from "./search.js";
import { head } from "./pages1.js";
import { clearRamCard } from "./pages2.js";
import { curveEditor, niceSteps, placeTip } from "./curve.js";
import { hotkeyField, selectBox } from "./hw.js";
import { go, render } from "./app.js";


export const TURBO = [[0, "Off"], [1, "On"], [2, "Aggressive"], [3, "Efficient"], [4, "Efficient aggressive"]];

/** Small menu under a button. items: [label, fn, danger?] */
export function popMenu(anchor, items) {
  document.querySelectorAll(".pop").forEach((p) => p.remove());
  const r = anchor.getBoundingClientRect();
  const pop = h("div", { class: "pop", style: { top: r.bottom + 4 + "px", left: Math.max(8, r.right - 190) + "px" } },
    items.filter(Boolean).map(([label, fn, danger]) => h("button", { class: danger ? "danger" : "", onclick: () => { pop.remove(); fn(); } }, label)));
  document.body.append(pop);
  setTimeout(() => document.addEventListener("pointerdown", function off(e) { if (!pop.contains(e.target)) { pop.remove(); document.removeEventListener("pointerdown", off); } }), 0);
}

export function profileCard(kind, x, active, opts) {
  const p = x.p;
  const hk = p.hotkey ? h("span", { class: "chip sm-chip2" + (x.hotkey_state === "busy" ? " warn" : ""), title: x.hotkey_state === "busy" ? "In use by another program (Deck keeps trying)" : "Keyboard shortcut" }, I("keyboard"), p.hotkey.replaceAll("+", " + ")) : null;
  const more = h("button", { class: "icon-btn sm more", title: "More", "aria-label": "More for " + p.name, onclick: (e) => { e.stopPropagation(); popMenu(e.currentTarget, opts.menu()); } }, I("more"));
  const edit = opts.edit ? h("button", { class: "icon-btn sm more" + (opts.editing ? " on" : ""), title: "Edit", "aria-label": "Edit " + p.name, onclick: (e) => { e.stopPropagation(); opts.edit(); } }, I("edit")) : null;
  return h("div", { class: "prof-card" + (active ? " on" : "") + (opts.editing ? " editing" : ""), onclick: () => opts.switchTo() },
    x.icon_url ? h("img", { class: "prof-ic", src: x.icon_url, alt: "" }) : h("div", { class: "prof-ic ph" }, (p.name || "?")[0]),
    h("div", { class: "grow", style: { minWidth: 0 } },
      h("div", { class: "row", style: { gap: "6px" } }, h("b", { class: "ell" }, p.name), active ? h("span", { class: "chip on sm-chip2" }, "On") : null),
      h("div", { class: "dim small ell", title: x.summary }, x.summary),
      h("div", { class: "row wrap", style: { gap: "4px", marginTop: "6px" } }, hk,
        p.desktop ? h("span", { class: "chip sm-chip2", title: "Desktop shortcut" }, I("monitor")) : null,
        p.start_menu ? h("span", { class: "chip sm-chip2", title: "Start menu shortcut" }, I("grid")) : null,
        x.plan_missing && kind === "cpu" ? h("span", { class: "chip warn sm-chip2" }, "Plan missing") : null,
        x.risky ? h("span", { class: "chip warn sm-chip2", title: "Overclock: not re-applied automatically after an unexpected restart" }, "OC") : null)),
    edit, more);
}

export async function switchProfile(kind, id) {
  try {
    await invoke("profile_switch", { kind, id });
  } catch (e) { note(String(e), true); }
  await Pages[kind].load();
  render();
}

// -------------------------------------------------------------------- icons

export async function iconPicker(current, kind) {
  S.cache.icons = S.cache.icons || (await invoke("icons_list"));
  return new Promise((resolve) => {
    const grid = h("div", { class: "icon-grid" }, S.cache.icons.map((ic) =>
      h("button", { class: "icon-pick" + (ic.name === current ? " on" : ""), title: ic.name.replace(".ico", ""), onclick: () => { close(); resolve({ icon: ic.name, url: ic.url }); } }, h("img", { src: ic.url, alt: "" }))));
    const close = modal("Pick an icon", grid, (cl) => [
      h("button", { class: "btn ghost", onclick: () => { cl(); resolve(null); } }, "Cancel"),
      h("button", { class: "btn", onclick: async () => {
        const f = await invoke("pick", { folder: false, title: "Choose an icon (.ico, .png or .exe)" });
        if (!f) return;
        try { const r = await invoke("icon_custom", { file: f }); cl(); resolve(r); } catch (e) { note(String(e), true); }
      } }, I("folder"), "Choose file…"),
    ]);
  });
}

/** Name, icon, keyboard shortcut and desktop / Start menu shortcuts: the top of both editors. */
export function identityFields(p, kind, iconUrl, redraw) {
  const img = iconUrl ? h("img", { src: iconUrl, alt: "" }) : h("div", { class: "ph" }, "?");
  return h("div", { class: "ident" },
    h("button", { class: "ident-ic", title: "Change the icon", onclick: async () => {
      const r = await iconPicker(p.icon, kind);
      if (r) { p.icon = r.icon; redraw.icon = r.url; redraw(); }
    } }, img),
    h("div", { class: "col grow", style: { gap: "8px" } },
      h("label", { class: "field" }, h("span", null, "Name"), h("input", { type: "text", value: p.name, maxlength: 40, oninput: (e) => (p.name = e.target.value) })),
      h("div", { class: "row wrap" },
        h("label", { class: "field" }, h("span", null, "Keyboard shortcut", helpTip("prof.hotkey")), hotkeyField(p.hotkey, p.id, (k) => { p.hotkey = k; redraw(); })),
        h("label", { class: "boost-opt", style: { alignSelf: "end" } }, toggle(p.desktop, (v) => (p.desktop = v)), h("div", { class: "t" }, "Desktop shortcut")),
        h("label", { class: "boost-opt", style: { alignSelf: "end" } }, toggle(p.start_menu, (v) => (p.start_menu = v)), h("div", { class: "t" }, "Start menu", helpTip("prof.links"))))));
}

/** A slider with a "no limit" switch: value null = off. */
export function optSlider(label, value, min, max, step, unit, offLabel, onset, fmt, help) {
  const on = value != null;
  const v = on ? value : max;
  const f = fmt || ((x) => x + unit);
  const out = h("b", { class: "num" }, on ? f(v) : offLabel);
  const s = slider(v, min, max, step, (x) => (out.textContent = f(x)), (x) => onset(x));
  if (!on) s.disabled = true;
  return h("div", { class: "srow4" }, toggle(on, (x) => onset(x ? v : null)), h("span", { class: "dim" }, label, help ? helpTip(help) : null), s, out);
}

export function plainSlider(label, value, min, max, step, fmt, onset, help) {
  const out = h("b", { class: "num" }, fmt(value));
  return h("div", { class: "srow4" }, h("span"), h("span", { class: "dim" }, label, help ? helpTip(help) : null), slider(value, min, max, step, (x) => (out.textContent = fmt(x)), (x) => onset(x)), out);
}

/** Keeps the last minute of live readings (one value per second) for the little graphs. */
export function pushHist(h, vals) {
  for (const [k, v] of Object.entries(vals)) {
    const q = (h[k] ||= []);
    if (v != null) q.push(v);
    if (q.length > 60) q.shift();
  }
}

// -------------------------------------------------------------------- CPU

Pages.cpu = {
  async load() {
    S.cache.cpu = await invoke("cpu_state");
    S.cache.cpuLive = await invoke("cpu_live").catch(() => null);
    // an open editor whose profile is gone (deleted elsewhere) closes
    const ed = S.cache.cpuEdit;
    if (ed && !S.cache.cpu.profiles.some((x) => x.p.id === ed.id)) S.cache.cpuEdit = null;
  },
  async tick() {
    S.cache.cpuLive = await invoke("cpu_live").catch(() => null);
    pushHist(S.cache.cpuHist ||= {}, { mhz: S.cache.cpuLive?.mhz, temp: S.cache.cpuLive?.temp, load: S.cache.cpuLive?.load, power: S.cache.cpuLive?.power });
    for (const f of S.cache.cpuUpd || []) f();
    if (S.cache.cpuLive && S.cache.cpu && S.cache.cpuLive.active !== S.cache.cpu.active) { await this.load(); return true; }
    return false;
  },
  render() {
    const st = S.cache.cpu;
    S.cache.cpuUpd = [];
    const el = h("div", null, head("CPU", "Click a profile to switch to it. Each one is a Windows power plan plus Deck's extras: power limits, a clock floor, an overclock.",
      st ? h("button", { class: "btn ghost", onclick: adoptPlans }, I("download"), "Use my existing plans") : null,
      st ? helpTip("cpu.adopt") : null,
      st ? h("button", { class: "btn primary", onclick: newCpuProfile }, I("plus"), "New profile") : null));
    if (!st) return el.append(h("div", { class: "empty" }, "Loading…")), el;
    const main = h("div", { class: "cpu-main" });
    if (!st.admin) main.append(h("div", { class: "banner" }, I("info"), h("div", { class: "grow" }, "Plans and caps work now. Power limits, live per-core clocks, the clock floor and overclocking need Deck to run as admin."), h("button", { class: "btn sm", onclick: () => invoke("elevate_setup").catch((e) => note(String(e), true)) }, "Set up")));
    const ed = S.cache.cpuEdit;
    if (!st.profiles.length) {
      main.append(h("div", { class: "card col setup-card" },
        h("div", { class: "boost-say" }, "No CPU profiles yet."),
        h("div", { class: "dim" }, "Turn power plans you already have into profiles, or make a new one from the current plan."),
        h("div", { class: "row" }, h("button", { class: "btn primary", onclick: adoptPlans }, "Use my existing plans"), h("button", { class: "btn", onclick: newCpuProfile }, "New profile"))));
    } else {
      main.append(h("div", { class: "section" }, h("h2", null, "Profiles", helpTip("cpu.profiles"))));
      main.append(h("div", { class: "prof-grid" }, st.profiles.map((x) => profileCard("cpu", x, x.p.id === st.active, {
        editing: ed?.id === x.p.id,
        edit: () => editCpu(x),
        switchTo: () => switchProfile("cpu", x.p.id),
        menu: () => [["Edit", () => editCpu(x)], ["Duplicate", async () => { await run("cpu_duplicate", { id: x.p.id }, "Copied"); await Pages.cpu.load(); render(); }],
          ["Delete…", () => deleteCpu(x), true]],
      }))));
      if (ed) main.append(cpuEditor(st));
      if (st.battery) main.append(...batteryProfiles(st));
    }
    main.append(...clockCheck(st));
    main.append(h("div", { style: { marginTop: "14px" } }, clearRamCard()));
    el.append(h("div", { class: "cpu-page" }, main, cpuRail(st)));
    return el;
  },
};

/** "Held at the power limit": under load, the package draws all the sustained (or burst) watts it may. */
export function heldByPower(live) {
  const l = live?.limits, w = live?.power;
  if (!l || w == null || (live.load ?? 0) < 60 || l.pl1 >= 1000) return false;
  return w >= l.pl1 * 0.95 || (l.pl2 > l.pl1 + 1 && w >= l.pl2 * 0.95);
}

/** The live column: clock, temperature, load, power against the limit in force, and every core. */
function cpuRail(st) {
  const L = () => S.cache.cpuLive || {};
  const lv = (fn, cls) => liveSpan(fn, S.cache.cpuUpd, cls);
  const hist = (S.cache.cpuHist ||= {});
  const up = (f) => (S.cache.cpuUpd.push(f), f);
  const cv = h("canvas", { class: "rail-spark", "aria-hidden": "true" });
  const acc = accent();
  up(() => spark(cv, hist.mhz, st.limits?.turbo || 0, acc));
  requestAnimationFrame(() => spark(cv, hist.mhz, st.limits?.turbo || 0, acc));
  const stat = (k, fn) => h("div", { class: "rail-stat" }, h("div", { class: "k" }, k), h("div", { class: "v" }, lv(fn)));
  // power against the limit in force
  const fill = h("i");
  const bar = h("div", { class: "rail-bar", role: "meter", "aria-label": "Power against the sustained limit", "aria-valuemin": "0" }, fill);
  const held = h("div", { class: "rail-held", hidden: true }, I("alert", "sm"), h("span", null, "Held at the power limit"));
  up(() => {
    const l = L().limits, w = L().power;
    const pct = l && w != null ? Math.min(100, (w / Math.min(l.pl1, 1000)) * 100) : 0;
    fill.style.transform = `scaleX(${pct / 100})`;
    bar.setAttribute("aria-valuemax", String(Math.round(l?.pl1 ?? 0)));
    bar.setAttribute("aria-valuenow", String(Math.round(w ?? 0)));
    const isHeld = heldByPower(L());
    held.hidden = !isHeld;
    bar.classList.toggle("held", isHeld);
  })();
  const limitText = () => {
    const l = L().limits;
    if (!l) return st.admin ? "–" : "needs admin";
    const fw = (x) => (x >= 1000 ? "no limit" : Math.round(x) + " W");
    return Math.abs(l.pl1 - l.pl2) < 1 ? fw(l.pl1) : `${fw(l.pl1)} · ${fw(l.pl2)} burst`;
  };
  const source = () => (!L().limits ? "" : L().limits_by_deck ? "Set by the active profile" : "Set by your motherboard");
  // cores
  const cores = h("div", { class: "rail-cores", role: "list" });
  const coresNote = h("div", { class: "faint small" });
  const drawCores = () => {
    const list = L().cores || [];
    coresNote.textContent = list.length ? "" : st.admin ? "Starting…" : "Per-core clocks need Deck to run as admin.";
    coresNote.hidden = !!list.length;
    if (cores.childElementCount !== list.length) {
      cores.replaceChildren(...list.map((c) => h("div", { class: "rc", role: "listitem" },
        h("span", { class: "rc-n" }, c.n.replace("P-core ", "P").replace("E-core ", "E").replace("Core ", "C")),
        h("span", { class: "rc-bar" }, h("i")),
        h("span", { class: "rc-v num" }))));
    }
    list.forEach((c, i) => {
      const row = cores.children[i];
      row.title = `${c.n}: ${fmtClock(c.mhz)}, ${c.load != null ? Math.round(c.load) + "% load" : "load –"}${c.temp != null ? ", " + fmtTemp(c.temp) : ""}`;
      row.querySelector(".rc-bar i").style.transform = `scaleX(${Math.max(0, Math.min(100, c.load ?? 0)) / 100})`;
      row.querySelector(".rc-v").textContent = c.mhz ? fmtClock(c.mhz) : "–";
    });
  };
  up(drawCores)();
  return h("aside", { class: "cpu-rail", "aria-label": "Live CPU readings" },
    h("div", { class: "rail-card" },
      h("div", { class: "rail-h" }, h("span", null, "Right now"), helpTip("cpu.vitals")),
      h("div", { class: "rail-clock" }, lv(() => (L().mhz ? fmtClock(L().mhz) : "–"), "num")),
      cv,
      h("div", { class: "rail-stats" },
        stat("Temp", () => fmtTemp(L().temp)),
        stat("Load", () => (L().load != null ? Math.round(L().load) + "%" : "–")),
        stat("Power", () => (L().power != null ? Math.round(L().power) + " W" : "–"))),
      h("div", { class: "rail-limit" },
        h("div", { class: "rail-limit-h" }, h("span", null, "Power limit", helpTip("cpu.limit")), lv(limitText, "num")),
        bar,
        held,
        h("div", { class: "rail-src faint small" }, lv(source, "")))),
    h("div", { class: "rail-card" },
      h("div", { class: "rail-h" }, h("span", null, "Cores"), h("span", { class: "faint small" }, "load · clock")),
      coresNote,
      cores));
}

/** In-game clock check: the real clock recorded while a game was in front. */
function clockCheck(st) {
  const c = st.clock;
  return [
    h("div", { class: "section" }, h("h2", null, "In-game clock check", helpTip("cpu.clockcheck"))),
    h("div", { class: "card row" }, I("gauge"),
      c ? h("div", { class: "grow" },
        h("b", null, `${c.on ? "In game" : "Last game"}: average ${fmtClock(c.avg)}`, c.cap ? ` (cap ${fmtClock(c.cap)})` : "", ` · peak ${fmtClock(c.max)}`),
        h("div", { class: "dim small" }, [c.game, c.profile ? "profile " + c.profile : null, c.on ? "now" : c.ago_s != null ? ago(Date.now() - c.ago_s * 1000) : null].filter(Boolean).join(" · "),
          c.cap && c.avg > c.cap * 1.05 ? h("span", { class: "chip warn sm-chip2", style: { marginLeft: "8px" } }, "Above the cap") : null))
        : h("div", { class: "grow dim" }, "Play something with a capped profile on: Deck records the real clock while the game is in front, so you can check the cap without alt-tabbing."),
      st.game_mode_off ? h("span", { class: "chip", title: "Windows Game Mode would lift the cap in games. It comes back when no capped profile is on." }, "Game Mode held off") : null),
  ];
}

/** PCs with a battery: a profile for plugged in and one for battery, switched when the power source changes. */
export function batteryProfiles(st) {
  const s = S.state.settings;
  const opts = [["", "Don't switch"], ...st.profiles.map((x) => [x.p.id, x.p.name])];
  const set = (k) => (v) => setSetting(k, v).then(render);
  return [
    h("div", { class: "section" }, h("h2", null, "Plugged in and on battery", helpTip("cpu.battery"))),
    h("div", { class: "card col" },
      h("div", { class: "dim small" }, st.plugged ? "Plugged in right now." : "On battery right now."),
      settingRow("When plugged in, switch to", "Switches the moment you plug in. A profile you pick yourself stays until the next change.", selectBox(s.cpu_plugged_profile, opts, set("cpu_plugged_profile"))),
      settingRow("On battery, switch to", "Switches the moment you unplug.", selectBox(s.cpu_battery_profile, opts, set("cpu_battery_profile"))),
      settingRow("Profiles also apply on battery", "Off: on battery, Windows keeps each plan's own values (changes you make in Deck still reach them).", toggle(s.cpu_battery_same, set("cpu_battery_same")))),
  ];
}

export async function adoptPlans() {
  const plans = await run("cpu_plans");
  // plans the user made (not Windows' own Balanced / High performance / Power saver / Ultimate)
  const BUILTIN = ["381b4222-f694-41f0-9685-ff5bb260df2e", "8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c", "a1841308-3541-4fab-bc81-f71556f20b4a", "e9a42b02-d5df-448d-aa00-03f14749eb61"];
  const picked = new Set(plans.filter((p) => !p.used && !BUILTIN.includes(p.guid)).map((p) => p.guid));
  const list = h("div", { class: "col", style: { gap: "6px" } }, plans.map((p) => h("label", { class: "plan-pick" + (p.used ? " off" : "") },
    h("input", { type: "checkbox", checked: picked.has(p.guid), disabled: p.used, onchange: (e) => (e.target.checked ? picked.add(p.guid) : picked.delete(p.guid)) }),
    h("div", { class: "grow" }, h("b", null, p.name, p.active ? h("span", { class: "chip on sm-chip2", style: { marginLeft: "6px" } }, "Active") : null), h("div", { class: "dim small" }, p.used ? "Already a profile" : p.summary)))));
  modal("Use my existing plans", h("div", { class: "col" }, h("div", { class: "dim small" }, "The plans stay as they are (same plan, so old shortcuts to them keep working). Deck reads their caps and turbo setting."), list), (close) => [
    h("button", { class: "btn ghost", onclick: close }, "Cancel"),
    h("button", { class: "btn primary", onclick: async () => {
      close();
      const n = await run("cpu_adopt", { guids: [...picked] });
      note(n ? `${n} profile${n === 1 ? "" : "s"} added` : "Nothing new to add");
      await Pages.cpu.load(); render();
    } }, "Add as profiles"),
  ]);
}

export async function newCpuProfile() {
  const name = await promptBox("New CPU profile", "Name (a copy of the active power plan)", "", "Create");
  if (name === null) return;
  const p = await run("cpu_create", { name });
  await Pages.cpu.load();
  render();
  const x = S.cache.cpu.profiles.find((q) => q.p.id === p.id);
  if (x) editCpu(x);
}

export async function deleteCpu(x) {
  let plan = false;
  const body = h("div", { class: "col" }, h("div", { class: "dim" }, `Delete the profile "${x.p.name}" and its shortcuts?`),
    h("label", { class: "boost-opt" }, toggle(false, (v) => (plan = v)), h("div", null, h("div", { class: "t" }, "Also delete the Windows power plan"), h("div", { class: "s" }, x.plan_name || ""))));
  modal("Delete profile", body, (close) => [
    h("button", { class: "btn ghost", onclick: close }, "Cancel"),
    h("button", { class: "btn danger", onclick: async () => { close(); await run("cpu_delete", { id: x.p.id, deletePlan: plan }, "Deleted"); await Pages.cpu.load(); render(); } }, "Delete"),
  ]);
}

/** Opens a profile's settings on the page, below the profiles (a draft until Save). */
export function editCpu(x) {
  const cur = S.cache.cpuEdit;
  if (cur && cur.id !== x.p.id && JSON.stringify(cur.p) !== cur.orig) note(`Unsaved changes to "${cur.name}" were discarded`);
  const p = structuredClone(x.p);
  S.cache.cpuEdit = { id: x.p.id, name: x.p.name, p, orig: JSON.stringify(p), icon: x.icon_url, advanced: S.cache.cpuEdit?.advanced || false };
  if (S.page !== "cpu") go("cpu"); else render();
  requestAnimationFrame(() => document.querySelector(".cpu-editor")?.scrollIntoView({ behavior: "smooth", block: "nearest" }));
}

/** The profile caps the clock: what "Hold the cap in games" protects. */
const isCapped = (p) => p.cap_p != null || p.cap_e != null || p.max_state < 100 || p.turbo === 0;

/** A slider whose one end means "off" ("No cap" at the top, "Off" at the bottom): no separate switch. */
function endSlider({ label, help, value, min, max, step, offAt, offLabel, fmt, set }) {
  const offV = offAt === "max" ? max : min;
  const v = value == null ? offV : Math.max(min, Math.min(max, value));
  const show = (x) => (x === offV ? offLabel : fmt(x));
  const out = h("b", { class: "num" + (value == null ? " off" : "") }, show(v));
  const s = slider(v, min, max, step, (x) => { out.textContent = show(x); out.classList.toggle("off", x === offV); }, (x) => set(x === offV ? null : x));
  s.setAttribute("aria-label", label);
  s.setAttribute("aria-valuetext", show(v));
  s.addEventListener("input", () => s.setAttribute("aria-valuetext", show(+s.value)));
  return h("div", { class: "ce-row" }, h("span", { class: "ce-l" }, label, help ? helpTip(help) : null), s, out);
}

/** A plain slider row (no off position). */
function rangeRow({ label, help, value, min, max, step, fmt, set }) {
  const out = h("b", { class: "num" }, fmt(value));
  const s = slider(value, min, max, step, (x) => (out.textContent = fmt(x)), set);
  s.setAttribute("aria-label", label);
  return h("div", { class: "ce-row" }, h("span", { class: "ce-l" }, label, help ? helpTip(help) : null), s, out);
}

/** A switch row: title, a line of explanation, the switch on the right. */
function switchRow(title, help, desc, on, set) {
  const t = toggle(on, set);
  t.querySelector("input").setAttribute("aria-label", title);
  return h("label", { class: "ce-switch" }, h("div", { class: "grow" }, h("div", { class: "t" }, title, help ? helpTip(help) : null), desc ? h("div", { class: "d" }, desc) : null), t);
}

/** The profile editor on the page: plain settings first, Windows plan details folded under Advanced. */
function cpuEditor(st) {
  const ed = S.cache.cpuEdit;
  const x = st.profiles.find((q) => q.p.id === ed.id);
  const box = h("section", { class: "cpu-editor", "aria-label": `Edit ${ed.name}` });
  const draw = () => {
    const keep = document.activeElement && box.contains(document.activeElement) ? document.activeElement.getAttribute("aria-label") : null;
    box.replaceChildren(...editorBody(st, x, ed, draw));
    if (keep) box.querySelector(`[aria-label="${CSS.escape(keep)}"]`)?.focus({ preventScroll: true });
  };
  draw();
  return box;
}

function editorBody(st, x, ed, draw) {
  const p = ed.p, lim = st.limits;
  const live = S.cache.cpuLive || {};
  const step = 100, lo = lim.min, hi = lim.turbo;
  const set = (k) => (v) => { p[k] = v; draw(); };
  const dirty = JSON.stringify(p) !== ed.orig;
  // the icon picker sets .icon on the function it calls
  const iconRedraw = () => { ed.icon = iconRedraw.icon; draw(); };
  iconRedraw.icon = ed.icon;
  const power = p.power?.mode && p.power.mode !== "balanced" ? p.power.mode : "none";
  const board = live.board;
  const bw = (l) => (l ? (Math.abs(l.pl1 - l.pl2) < 1 ? `${Math.round(l.pl1)} W` : `${Math.round(l.pl1)} W sustained, ${Math.round(l.pl2)} W burst`) : null);
  const warns = [];
  if (p.oc != null && p.cap_p != null && p.cap_p < p.oc) warns.push(`The P-core cap (${fmtClock(p.cap_p)}) stops the CPU before the overclock (${fmtClock(p.oc)}) can take effect.`);
  if (p.min_lock != null && p.cap_p != null && p.min_lock > p.cap_p) warns.push("The clock floor is above the cap: the cap wins.");
  const capRow = (label, key, help) => endSlider({ label, help, value: p[key], min: lo, max: hi, step, offAt: "max", offLabel: "No cap", fmt: fmtClock, set: set(key) });
  const eppText = (v) => (v === 0 ? "0 · speed" : v === 100 ? "100 · saving" : String(v));
  const planEpp = x?.plan_epp;
  const powerNote = {
    none: board ? `Your motherboard's own limits: ${bw(board)}.` : "Your motherboard's own limits.",
    saver: "15 W sustained, 25 W bursts, and Windows set to best power efficiency.",
    performance: `Your motherboard's own limits${board ? ` (${bw(board)})` : ""}, and Windows set to best performance.`,
    custom: board ? `Your motherboard's own: ${bw(board)}. Higher lets the CPU hold its clocks under long all-core loads, and run hotter.` : "Higher lets the CPU hold its clocks under long all-core loads, and run hotter.",
  }[power];
  const plMax = Math.max(253, Math.ceil((board?.pl2 || 0) / 10) * 10);
  const sec = (title, help, ...kids) => h("div", { class: "ce-sec" }, h("h3", null, title, help ? helpTip(help) : null), ...kids);
  return [
    h("div", { class: "ce-head" },
      h("div", { class: "grow" }, h("h2", null, "Edit ", h("span", null, ed.name)), h("div", { class: "faint small" }, x?.plan_name ? "Windows plan: " + x.plan_name : "")),
      dirty ? h("span", { class: "chip warn sm-chip2" }, "Unsaved") : null),
    identityFields(p, "cpu", ed.icon, iconRedraw),
    sec("Speed", null,
      st.hybrid
        ? [capRow("P-cores max", "cap_p", "cpu.cap_p"), capRow("E-cores max", "cap_e", "cpu.cap_e")]
        : endSlider({ label: "Max clock", help: "cpu.cap", value: p.cap_p, min: lo, max: hi, step, offAt: "max", offLabel: "No cap", fmt: fmtClock, set: (v) => { p.cap_p = v; p.cap_e = v; draw(); } }),
      switchRow("Turbo boost", "cpu.turbo", p.turbo === 0 ? "Off: the CPU stays at its base clock. Much cooler, much slower." : "The CPU boosts above its base clock when there's work.", p.turbo !== 0, (v) => { p.turbo = v ? 2 : 0; draw(); }),
      endSlider({ label: "Energy preference", help: "cpu.epp", value: p.epp, min: -5, max: 100, step: 5, offAt: "min", offLabel: planEpp != null ? `Plan's own · ${planEpp}` : "Plan's own", fmt: eppText, set: set("epp") }),
      p.epp == null && planEpp > 0 ? h("div", { class: "ce-hint" }, I("info", "sm"), h("span", null, `The plan leans to saving (${planEpp}): light loads such as games climb to high clocks slower than at 0 · speed.`)) : null),
    sec("Power limits", "cpu.power",
      h("div", { class: "row wrap", style: { gap: "10px" } },
        seg([["none", "Motherboard"], ["saver", "Saver 15 W"], ["performance", "Performance"], ["custom", "Custom"]], power, (v) => {
          p.power = v === "none" ? null : { mode: v, pl1: p.power?.pl1 || Math.round(board?.pl1 || 65), pl2: p.power?.pl2 || Math.round(board?.pl2 || 125) };
          draw();
        }),
        !st.rapl ? h("span", { class: "chip warn sm-chip2" }, "needs admin") : null),
      h("div", { class: "faint small" }, powerNote),
      power === "custom" ? [
        rangeRow({ label: "Sustained (PL1)", help: "cpu.pl1", value: p.power.pl1, min: 5, max: plMax, step: 1, fmt: (v) => v + " W", set: (v) => { p.power.pl1 = v; if (p.power.pl2 < v) p.power.pl2 = v; draw(); } }),
        rangeRow({ label: "Burst (PL2)", help: "cpu.pl2", value: p.power.pl2, min: 5, max: plMax, step: 1, fmt: (v) => v + " W", set: (v) => { p.power.pl2 = Math.max(v, p.power.pl1); draw(); } }),
      ] : null),
    sec("Extras", null,
      endSlider({ label: "Clock floor", help: "cpu.min_lock", value: p.min_lock, min: lo, max: p.cap_p || hi, step, offAt: "min", offLabel: "Off", fmt: fmtClock, set: set("min_lock") }),
      p.min_lock != null ? h("div", { class: "ce-hint" }, I("thermo", "sm"), h("span", null, "Released by itself if the CPU passes 92 °C.")) : null,
      lim.oc ? endSlider({ label: "Overclock turbo to", help: "cpu.oc", value: p.oc, min: lim.turbo, max: lim.oc_max, step: 100, offAt: "min", offLabel: "Off", fmt: fmtClock, set: set("oc") }) : null,
      isCapped(p)
        ? switchRow("Hold the cap in games", "cpu.hold", "Turns Windows Game Mode off while this profile is on (Game Mode lifts power-plan caps in games).", p.hold_in_games, set("hold_in_games"))
        : null,
      warns.map((w) => h("div", { class: "ce-hint warn" }, I("alert", "sm"), h("span", null, w)))),
    h("details", { class: "ce-adv", open: ed.advanced || null, ontoggle: (e) => (ed.advanced = e.target.open) },
      h("summary", null, h("span", { class: "t" }, "Advanced"), h("span", { class: "faint small" }, "Windows plan settings: turbo mode, processor states, parked cores"), h("span", { class: "chev" }, I("chevron", "sm"))),
      h("div", { class: "ce-sec flat" },
        h("div", { class: "ce-row" }, h("span", { class: "ce-l" }, "Turbo mode", helpTip("cpu.turbo_mode")), seg(TURBO, p.turbo, set("turbo"))),
        rangeRow({ label: "Max processor state", help: "cpu.max_state", value: p.max_state, min: 5, max: 100, step: 1, fmt: (v) => v + "%", set: set("max_state") }),
        rangeRow({ label: "Min processor state", help: "cpu.min_state", value: p.min_state, min: 0, max: 100, step: 1, fmt: (v) => v + "%", set: set("min_state") }),
        rangeRow({ label: "Cores kept awake", help: "cpu.min_cores", value: p.min_cores, min: 0, max: 100, step: 5, fmt: (v) => v + "%", set: set("min_cores") }))),
    h("div", { class: "ce-foot" },
      h("span", { class: "faint small grow" }, dirty ? "Changes apply when you save." : "No changes yet."),
      h("button", { class: "btn ghost", onclick: () => { S.cache.cpuEdit = null; render(); } }, dirty ? "Discard" : "Close"),
      h("button", { class: "btn primary", disabled: !dirty || null, onclick: async (e) => {
        e.currentTarget.disabled = true;
        try {
          const r = await invoke("cpu_save", { p });
          if (r.plan_error) note(r.plan_error, true);
          else if (r.link_error) note("Saved, but the shortcut failed: " + r.link_error, true);
          else note("Saved");
          S.cache.cpuEdit = null;
        } catch (err) { note(String(err), true); }
        await Pages.cpu.load(); render();
      } }, I("save"), "Save")),
  ];
}

// -------------------------------------------------------------------- GPU

Pages.gpu = {
  async load() {
    S.cache.gpu = await invoke("gpu_state");
    S.cache.gpuLive = S.cache.gpu.found ? await invoke("gpu_live").catch(() => null) : null;
  },
  async tick() {
    if (!S.cache.gpu?.found) return false;
    S.cache.gpuLive = await invoke("gpu_live").catch(() => null);
    pushHist(S.cache.gpuHist ||= {}, { core: S.cache.gpuLive?.core_mhz, power: S.cache.gpuLive?.power_w, temp: S.cache.gpuLive?.temp_c, load: S.cache.gpuLive?.util });
    for (const f of S.cache.gpuUpd || []) f();
    if (S.cache.vfDraw) S.cache.vfDraw();
    return false;
  },
  render() {
    const st = S.cache.gpu;
    S.cache.gpuUpd = [];
    const el = h("div", null, head("GPU", "Power limit, clocks, voltage curve and fan, in profiles you switch with a click or a key. Replaces MSI Afterburner.",
      st?.found ? helpTip("gpu.stock") : null,
      st?.found ? h("button", { class: "btn", onclick: () => run("gpu_stock").then(async (m) => { note(m); await Pages.gpu.load(); render(); }) }, I("refresh"), "Back to stock") : null,
      st?.found ? h("button", { class: "btn primary", onclick: () => editGpu(null) }, I("plus"), "New profile") : null));
    if (!st) return el.append(h("div", { class: "empty" }, "Loading…")), el;
    if (!st.found) return el.append(h("div", { class: "empty" }, "No NVIDIA graphics card was found.")), el;
    const caps = st.caps;
    if (st.afterburner_tunes) el.append(h("div", { class: "banner warn" }, I("info"),
      h("div", { class: "grow" }, h("b", null, "MSI Afterburner is running. "), "Its startup profile, hotkeys and fan control can override Deck."),
      h("button", { class: "btn sm", onclick: stopAfterburner }, "Stop Afterburner from tuning…")));
    if (st.held_back) el.append(h("div", { class: "banner warn" }, I("shield"),
      h("div", { class: "grow" }, h("b", null, "Started at stock. "), "The PC restarted unexpectedly while an overclock was on, so Deck didn't apply it again."),
      h("button", { class: "btn sm", onclick: async () => { await run("gpu_apply_held", null, "Applied"); await Pages.gpu.load(); render(); } }, "Apply anyway")));
    if (!st.admin) el.append(h("div", { class: "banner" }, I("info"), h("div", { class: "grow" }, "Changing the card's settings needs Deck to run as admin."), h("button", { class: "btn sm", onclick: () => invoke("elevate_setup").catch((e) => note(String(e), true)) }, "Set up")));
    el.append(gpuLivePanel(caps));
    // profiles
    el.append(h("div", { class: "section" }, h("h2", null, "Profiles", helpTip("gpu.profiles")), h("span", { class: "grow" }),
      st.afterburner_installed ? h("button", { class: "btn sm ghost", onclick: importAfterburner }, I("download", "sm"), "Import from Afterburner") : null,
      st.afterburner_installed ? helpTip("gpu.import") : null));
    if (!st.profiles.length) {
      el.append(h("div", { class: "card col setup-card" }, h("div", { class: "boost-say" }, "No GPU profiles yet."),
        h("div", { class: "dim" }, st.afterburner_installed ? "Bring over Afterburner's five profiles (with their curves and Ctrl+Alt keys), or make one." : "Make one: a power limit, clock offsets or a curve, and a fan setting."),
        h("div", { class: "row" }, st.afterburner_installed ? h("button", { class: "btn primary", onclick: importAfterburner }, "Import from Afterburner") : null, h("button", { class: "btn", onclick: () => editGpu(null) }, "New profile"))));
    } else {
      el.append(h("div", { class: "prof-grid" }, st.profiles.map((x) => profileCard("gpu", x, x.p.id === st.active, {
        switchTo: () => switchProfile("gpu", x.p.id),
        menu: () => [["Edit…", () => editGpu(x)], ["Duplicate", async () => {
          const q = structuredClone(x.p); q.id = ""; q.name += " copy"; q.hotkey = ""; q.desktop = false; q.start_menu = false;
          await run("gpu_save", { p: q }, "Copied"); await Pages.gpu.load(); render();
        }], ["Delete", async () => {
          if (!(await confirmBox("Delete profile", `Delete "${x.p.name}" and its shortcuts?`, "Delete", true))) return;
          await run("gpu_delete", { id: x.p.id }, "Deleted"); await Pages.gpu.load(); render();
        }, true]],
      }))));
    }
    // startup
    el.append(h("div", { class: "section" }, h("h2", null, "When Deck starts")));
    el.append(h("div", { class: "card row wrap" },
      h("div", { class: "grow" }, h("div", { class: "t" }, "Apply a profile at start", helpTip("gpu.startup")), h("div", { class: "dim small" }, "Like Afterburner's startup profile. An overclock isn't applied again after an unexpected restart.")),
      selectBox(st.startup, [["", "None (leave the card alone)"], ...st.profiles.map((x) => [x.p.id, x.p.name])], async (v) => { await setSetting("gpu_startup", v); S.cache.gpu.startup = v; })));
    return el;
  },
};

export function gpuLivePanel(caps) {
  const L = () => S.cache.gpuLive || {};
  const lv = (fn, cls) => liveSpan(fn, S.cache.gpuUpd, cls);
  const pct = () => (L().power_w != null && caps.power[2] ? Math.round((L().power_w / caps.power[2]) * 100) + "% of default" : "");
  const hist = (S.cache.gpuHist ||= {});
  const box = (k, v, s, key, color, max) => {
    const cv = key ? h("canvas") : null;
    if (key) {
      const draw = () => spark(cv, hist[key], max, color);
      S.cache.gpuUpd.push(draw);
      requestAnimationFrame(draw);
    }
    return h("div", { class: "vital" }, h("div", { class: "k" }, k), h("div", { class: "v" }, v), s ? h("div", { class: "s" }, s) : null, cv);
  };
  const acc = accent();
  const reason = () => ({ power: "Held back by the power limit", thermal: "Held back by temperature", voltage: "At the voltage limit", idle: "Idle", "": "" })[L().limit] ?? L().limit;
  return h("div", { class: "card col" },
    h("div", { class: "row" }, h("div", { class: "tile-head grow" }, I("gpu"), h("span", null, caps.name), helpTip("gpu.vitals")), h("span", { class: "dim small" }, "Driver " + caps.driver), lv(() => reason(), "chip")),
    h("div", { class: "live-strip in" },
      box("Core", lv(() => fmtClock(L().core_mhz)), null, "core", acc, caps.max_core || 0),
      box("Memory", lv(() => fmtClock(L().mem_mhz))),
      box("Power", lv(() => (L().power_w != null ? Math.round(L().power_w) + " W" : "–")), lv(pct, ""), "power", "#3ddc97", caps.power?.[2] || 0),
      box("Temperature", lv(() => fmtTemp(L().temp_c)), null, "temp", "#ff9f43", 100),
      box("Fan", lv(() => (L().fan_pct?.length ? Math.max(...L().fan_pct) + "%" : "–")), lv(() => (L().fan_manual ? "Deck" : "Driver"), "")),
      box("Voltage", lv(() => (L().voltage_mv ? Math.round(L().voltage_mv) + " mV" : "–"))),
      box("Load", lv(() => (L().util != null ? L().util + "%" : "–")), lv(() => (L().mem_used_mb != null ? `${(L().mem_used_mb / 1024).toFixed(1)} / ${(L().mem_total_mb / 1024).toFixed(0)} GB` : ""), ""), "load", "#b98cff", 100)));
}

export async function stopAfterburner() {
  if (!(await confirmBox("Stop Afterburner from tuning",
    "Deck backs up Afterburner's settings, then clears its startup profile, its profile hotkeys and its fan control, and restarts it. Afterburner keeps working for monitoring. You can undo this.", "Stop it tuning"))) return;
  try {
    const dir = await invoke("ab_stop_tuning");
    modal("Done", h("div", { class: "dim" }, "Afterburner no longer applies profiles or controls the fan. Backup: " + dir), (close) => [
      h("button", { class: "btn ghost", onclick: async () => { close(); await run("ab_undo_stop", null, "Afterburner's settings are back"); } }, "Undo"),
      h("button", { class: "btn primary", onclick: close }, "OK"),
    ]);
  } catch (e) { note(String(e), true); }
}

export async function importAfterburner() {
  let pv;
  try { pv = await invoke("ab_preview"); } catch (e) { return note(String(e), true); }
  let startup = !!pv.startup, osd = pv.osd.length > 0;
  const list = h("div", { class: "col", style: { gap: "6px" } }, pv.profiles.map((p) => h("div", { class: "plan-pick" },
    h("div", { class: "grow" }, h("b", null, p.name, p.id === pv.startup ? h("span", { class: "chip sm-chip2", style: { marginLeft: "6px" } }, "Startup") : null), h("div", { class: "dim small" }, p.summary)),
    p.hotkey ? h("span", { class: "chip sm-chip2" }, p.hotkey) : null)));
  const body = h("div", { class: "col" }, list,
    pv.startup ? h("label", { class: "boost-opt" }, toggle(startup, (v) => (startup = v)), h("div", { class: "t" }, "Apply the startup profile when Deck starts, like Afterburner")) : null,
    pv.osd.length ? h("label", { class: "boost-opt" }, toggle(osd, (v) => (osd = v)), h("div", null, h("div", { class: "t" }, `Copy the overlay items (${pv.osd.length})`), h("div", { class: "s" }, "What Afterburner showed in RivaTuner's overlay."))) : null,
    pv.notes.map((n) => h("div", { class: "dim small" }, n)));
  modal("Import from Afterburner", body, (close) => [
    h("button", { class: "btn ghost", onclick: close }, "Cancel"),
    h("button", { class: "btn primary", onclick: async () => {
      close();
      const n = await run("ab_import", { startup, osd });
      note(`${n} profiles imported`);
      await Pages.gpu.load(); render();
    } }, "Import"),
  ]);
}

/** The editor. `x` = existing card data, or null for a new profile. */
export async function editGpu(x) {
  const st = S.cache.gpu, caps = st.caps;
  const p = structuredClone(x ? x.p : st.blank);
  let vf = null;
  if (caps.curve) vf = await invoke("gpu_curve").catch(() => null);
  let tested = false;
  const body = h("div");
  const redraw = () => body.replaceChildren(gpuForm(p, caps, vf, redraw));
  redraw.icon = x?.icon_url || (S.cache.icons || []).find((i) => i.name === p.icon)?.url || null;
  if (!redraw.icon) invoke("icon_preview", { icon: p.icon }).then((u) => { redraw.icon = u; redraw(); });
  redraw();
  const undoTest = async () => {
    if (!tested) return;
    if (st.active) await invoke("profile_switch", { kind: "gpu", id: st.active }).catch(() => {});
    else await invoke("gpu_stock").catch(() => {});
  };
  modal(x ? "Edit GPU profile" : "New GPU profile", body, (close) => [
    h("button", { class: "btn", title: "Put it on the card now without saving", onclick: async () => {
      try {
        const fields = await invoke("gpu_try", { p });
        tested = true;
        const bad = fields.filter((f) => !f.ok);
        note(bad.length ? bad.map((f) => `${f.what}: ${f.note}`).join(" · ") : "On the card now: " + fields.map((f) => f.note).join(" · "), bad.length > 0);
      } catch (e) { note(String(e), true); }
    } }, I("play"), "Try it"),
    helpTip("gpu.try"),
    h("span", { class: "grow" }),
    h("button", { class: "btn ghost", onclick: async () => { close(); await undoTest(); } }, "Cancel"),
    h("button", { class: "btn primary", onclick: async () => {
      try {
        const r = await invoke("gpu_save", { p });
        close();
        if (r.apply_error) note(r.apply_error, true);
        else if (r.link_error) note("Saved, but the shortcut failed: " + r.link_error, true);
        else note("Saved");
        if (tested && st.active !== r.id) await undoTest();
      } catch (e) { note(String(e), true); return; }
      await Pages.gpu.load(); render();
    } }, "Save"),
  ], { wide: true, keep: true, onDismiss: () => undoTest() });
}

export function gpuForm(p, caps, vf, redraw) {
  const set = (k) => (v) => { p[k] = v; redraw(); };
  const watts = (pct) => Math.round((caps.power[2] * pct) / 100);
  const signed = (v) => (v > 0 ? "+" : "") + v + " MHz";
  const fanMode = p.fan.mode;
  return h("div", { class: "col", style: { gap: "14px" } },
    identityFields(p, "gpu", redraw.icon, redraw),
    h("div", { class: "ed-sec" }, "Power and temperature"),
    plainSlider("Power limit", p.power_pct, caps.power_pct[0], caps.power_pct[1], 1, (v) => `${v}% · ${watts(v)} W`, set("power_pct"), "gpu.power"),
    caps.temp ? optSlider("Temperature limit", p.temp_limit, caps.temp[0], caps.temp[1], 1, " °C", "Card's own", set("temp_limit"), null, "gpu.temp") : null,
    h("div", { class: "ed-sec" }, "Clocks", vf ? helpTip("gpu.clockmode") : null,
      vf ? h("div", { class: "seg sm-seg" }, h("button", { class: p.curve ? "" : "on", onclick: () => { p.curve = null; p.lock_mv = null; redraw(); } }, "Offset"),
        h("button", { class: p.curve ? "on" : "", onclick: () => { p.curve = vf.points.map((q) => [q.mv, p.core_mhz || 0]); p.core_mhz = 0; redraw(); } }, "Curve")) : null),
    p.curve && vf ? vfEditor(p, vf, redraw) : plainSlider("Core clock", p.core_mhz, Math.max(caps.core[0], -1000), Math.min(caps.core[1], 1000), 5, signed, set("core_mhz"), "gpu.core"),
    plainSlider("Memory clock", p.mem_mhz, caps.mem[0], caps.mem[1], 10, signed, set("mem_mhz"), "gpu.mem"),
    caps.volt ? optSlider("Core voltage boost", p.volt_pct, 0, 100, 1, "%", "None", set("volt_pct"), null, "gpu.volt") : null,
    h("div", { class: "ed-sec" }, "Fan", helpTip("gpu.fan")),
    h("div", { class: "row wrap" }, seg([["auto", "Driver (auto)"], ["fixed", "Fixed speed"], ["curve", "Curve"]], fanMode, (v) => { p.fan.mode = v; redraw(); })),
    fanMode === "fixed" ? plainSlider("Speed", p.fan.pct, caps.fan_min, 100, 1, (v) => v + "%", (v) => { p.fan.pct = v; redraw(); }) : null,
    fanMode === "curve" ? curveEditor({ points: p.fan.points, x: [25, 95], y: [0, 100], height: 200, marker: () => S.cache.gpuLive?.temp_c, onchange: (pts) => (p.fan.points = pts) }).el : null,
    fanMode === "curve" ? h("div", { class: "dim small" }, helpTip("gpu.fan_curve"), ` Deck runs this curve every second against the GPU temperature (minimum ${caps.fan_min}%). The fan goes back to the driver when Deck quits.`) : null);
}

export const VF_STATE = new WeakMap();

/** The voltage/frequency curve, edited the way Afterburner's curve editor works:
 *  drag a point · Shift+drag to select a range · drag a selected point to move the range ·
 *  Ctrl+drag moves the whole curve · Alt+drag bends the neighbours along · L locks the card
 *  to the selected point · Ctrl+F flattens from it · Ctrl+D resets · Ctrl+Z / Ctrl+Y undo.
 *  The curve never falls to the right (the driver runs it that way), so raising a point lifts
 *  the points after it and lowering one pulls the points before it down, like Afterburner. */
export function vfEditor(p, vf, redraw) {
  const base = vf.points; // [{mv, base, delta}]
  const n = base.length;
  // the profile's curve as one delta per card point (matched by voltage), then as the card runs it
  let deltas = base.map((b) => {
    let best = null, bd = 99;
    for (const [mv, d] of p.curve) { const dd = Math.abs(mv - b.mv); if (dd < bd) { bd = dd; best = d; } }
    return bd < 3.3 ? best : 0;
  });
  const F = (i) => base[i].base + deltas[i];
  const setF = (i, f) => (deltas[i] = Math.round(Math.min(Math.max(f - base[i].base, -1000), 1000)));
  for (let i = n - 2; i >= 0; i--) if (F(i) > F(i + 1)) setF(i, F(i + 1));
  const save = () => { p.curve = base.map((b, i) => [b.mv, Math.round(deltas[i])]); };
  save();

  // ---- state (kept on the profile object so a form redraw doesn't lose it)
  const keep = VF_STATE.get(p) || { sel: new Set(), cur: -1, undo: [], redo: [] };
  VF_STATE.set(p, keep);
  let sel = keep.sel;       // selected points
  let cur = keep.cur;       // keyboard cursor (last clicked point)
  let hover = -1;
  let drag = null;          // { kind: "point" | "group" | "all" | "bend" | "band", ... }
  const undo = keep.undo, redo = keep.redo;
  const snap = () => ({ d: deltas.slice(), lock: p.lock_mv ?? null });
  const remember = () => { undo.push(snap()); if (undo.length > 100) undo.shift(); redo.length = 0; };
  const restore = (s) => { deltas = s.d.slice(); p.lock_mv = s.lock; changed(); };

  /** Keeps the curve rising: lifts points after the changed ones (dir > 0) or pulls points
   *  before them down (dir < 0). */
  const settle = (dir, lo, hi) => {
    if (dir > 0) for (let j = Math.max(1, lo + 1); j < n; j++) { if (F(j) < F(j - 1)) setF(j, F(j - 1)); }
    if (dir < 0) for (let j = Math.min(n - 2, hi - 1); j >= 0; j--) { if (F(j) > F(j + 1)) setF(j, F(j + 1)); }
  };

  // ---- drawing
  const cv = h("canvas", { class: "curve-cv vf-cv", tabindex: 0, style: { height: "300px" } });
  const tip = h("div", { class: "curve-tip hidden" });
  const wrap = h("div", { class: "curve" }, cv, tip);
  const minMv = base[0].mv, maxMv = base[n - 1].mv;
  const pad = { l: 46, r: 14, t: 14, b: 26 };
  let top = 0;
  const fitTop = () => { if (!drag) top = Math.max(...base.map((b, i) => Math.max(b.base, F(i)))) + 150; };
  fitTop();
  const g = () => { const w = cv.clientWidth || 700, ht = cv.clientHeight || 300; return { w, ht, iw: w - pad.l - pad.r, ih: ht - pad.t - pad.b }; };
  const X = (mv, G) => pad.l + ((mv - minMv) / (maxMv - minMv)) * G.iw;
  const Y = (f, G) => pad.t + (1 - (f - 200) / (top - 200)) * G.ih;
  const fAt = (y, G) => 200 + (1 - (y - pad.t) / G.ih) * (top - 200);
  const lockIdx = () => (p.lock_mv == null ? -1 : base.reduce((k, b, i) => (Math.abs(b.mv - p.lock_mv) < Math.abs(base[k].mv - p.lock_mv) ? i : k), 0));

  const draw = () => {
    keep.sel = sel; keep.cur = cur;
    const G = g(), dpr = window.devicePixelRatio || 1;
    cv.width = G.w * dpr; cv.height = G.ht * dpr;
    const c = cv.getContext("2d"); c.scale(dpr, dpr);
    const css = getComputedStyle(document.documentElement), line = css.getPropertyValue("--line").trim(), faint = css.getPropertyValue("--faint").trim(), acc = accent();
    c.font = "11px Segoe UI, sans-serif"; c.fillStyle = faint; c.strokeStyle = line; c.lineWidth = 1;
    for (const f of niceSteps(200, top, 6)) { const y = Y(f, G); c.beginPath(); c.moveTo(pad.l, y + 0.5); c.lineTo(pad.l + G.iw, y + 0.5); c.stroke(); c.textAlign = "right"; c.fillText(f, pad.l - 6, y + 4); }
    const mvs = niceSteps(minMv, maxMv, 8);
    for (const mv of mvs) { const x = X(mv, G), last = mv === mvs.at(-1); c.beginPath(); c.moveTo(x + 0.5, pad.t); c.lineTo(x + 0.5, pad.t + G.ih); c.stroke(); c.textAlign = last ? "right" : "center"; c.fillText(mv + (last ? " mV" : ""), last ? x + 4 : x, G.ht - 8); }
    // range selection being dragged
    if (drag?.kind === "band") {
      const [a, b] = [Math.min(drag.x0, drag.x1), Math.max(drag.x0, drag.x1)];
      c.fillStyle = acc + "22"; c.fillRect(a, pad.t, b - a, G.ih);
      c.strokeStyle = acc + "88"; c.strokeRect(a + 0.5, pad.t + 0.5, b - a, G.ih);
    }
    // selected range: a soft band behind the points
    if (sel.size > 1) {
      const xs = [...sel].map((i) => X(base[i].mv, G));
      c.fillStyle = acc + "14"; c.fillRect(Math.min(...xs) - 4, pad.t, Math.max(...xs) - Math.min(...xs) + 8, G.ih);
    }
    // stock curve, then yours
    c.beginPath(); base.forEach((b, i) => (i ? c.lineTo : c.moveTo).call(c, X(b.mv, G), Y(b.base, G))); c.strokeStyle = faint; c.setLineDash([3, 4]); c.stroke(); c.setLineDash([]);
    c.beginPath(); base.forEach((b, i) => (i ? c.lineTo : c.moveTo).call(c, X(b.mv, G), Y(F(i), G))); c.strokeStyle = acc; c.lineWidth = 2.2; c.stroke();
    // clock lock (green) and the voltage now (orange, dashed)
    const lk = lockIdx();
    if (lk >= 0) {
      const x = X(base[lk].mv, G);
      c.strokeStyle = "#3ddc97"; c.lineWidth = 1.6; c.beginPath(); c.moveTo(x, pad.t); c.lineTo(x, pad.t + G.ih); c.stroke();
      const lab = `Locked · ${Math.round(Math.min(...base.slice(lk).map((_, j) => F(lk + j))))} MHz`;
      c.font = "600 11px Segoe UI, sans-serif"; const tw = c.measureText(lab).width + 12;
      const lx = Math.min(Math.max(x - tw / 2, pad.l + 2), pad.l + G.iw - tw - 2);
      c.fillStyle = "#3ddc97"; c.beginPath(); c.roundRect(lx, pad.t + 2, tw, 18, 5); c.fill();
      c.fillStyle = "#06120c"; c.textAlign = "left"; c.fillText(lab, lx + 6, pad.t + 15);
    }
    const v = S.cache.gpuLive?.voltage_mv;
    if (v) { const x = X(Math.min(Math.max(v, minMv), maxMv), G); c.setLineDash([4, 4]); c.strokeStyle = "#ffb547"; c.lineWidth = 1.2; c.beginPath(); c.moveTo(x, pad.t); c.lineTo(x, pad.t + G.ih); c.stroke(); c.setLineDash([]); }
    // points: every one is a handle; selected ones filled, the cursor ringed
    base.forEach((b, i) => {
      const on = sel.has(i), big = on || i === hover || i === cur;
      c.beginPath(); c.arc(X(b.mv, G), Y(F(i), G), big ? 4.6 : 2.6, 0, 7);
      c.fillStyle = on ? acc : "#fff"; c.fill(); c.strokeStyle = acc; c.lineWidth = 1.4; c.stroke();
      if (i === cur && document.activeElement === cv) { c.beginPath(); c.arc(X(b.mv, G), Y(F(i), G), 8, 0, 7); c.strokeStyle = acc + "aa"; c.lineWidth = 1.2; c.stroke(); }
    });
  };

  const showTip = (i) => {
    if (i < 0) return tip.classList.add("hidden");
    const G = g(), b = base[i], f = F(i), d = f - b.base;
    tip.textContent = `${Math.round(b.mv)} mV · ${Math.round(f)} MHz (${d >= 0 ? "+" : ""}${Math.round(d)})${i === lockIdx() ? " · locked" : ""}${sel.size > 1 && sel.has(i) ? ` · ${sel.size} selected` : ""}`;
    placeTip(tip, wrap, X(b.mv, G), Y(f, G));
  };
  const changed = () => { save(); fitTop(); draw(); status(); };

  // ---- mouse
  const pos = (e) => { const r = cv.getBoundingClientRect(); return { x: e.clientX - r.left, y: e.clientY - r.top }; };
  const nearest = (q) => {
    const G = g();
    let best = -1, bd = 12;
    base.forEach((b, i) => { const d = Math.hypot(X(b.mv, G) - q.x, Y(F(i), G) - q.y); if (d < bd) { bd = d; best = i; } });
    return best;
  };
  const inBand = (x0, x1) => { const G = g(), [a, b] = [Math.min(x0, x1), Math.max(x0, x1)]; return base.map((bb, i) => [X(bb.mv, G), i]).filter(([x]) => x >= a - 2 && x <= b + 2).map(([, i]) => i); };

  cv.addEventListener("pointerdown", (e) => {
    if (e.button !== 0) return;
    cv.focus({ preventScroll: true });
    const q = pos(e), i = nearest(q);
    cv.setPointerCapture(e.pointerId);
    S.interacting++;
    if (e.shiftKey) { drag = { kind: "band", x0: q.x, x1: q.x, add: e.ctrlKey ? new Set(sel) : new Set() }; draw(); return; }
    if (i < 0) { drag = { kind: "none" }; sel = new Set(); cur = -1; draw(); status(); return; }
    if (e.ctrlKey && !e.altKey) {
      // Ctrl+click toggles a point; Ctrl+drag moves the whole curve
      drag = { kind: "all", i, y0: q.y, from: deltas.slice(), toggle: true };
    } else if (e.altKey) {
      drag = { kind: "bend", i, y0: q.y, from: deltas.slice() };
    } else if (sel.has(i) && sel.size > 1) {
      drag = { kind: "group", i, y0: q.y, from: deltas.slice() };
    } else {
      sel = new Set([i]);
      drag = { kind: "point", i, y0: q.y, from: deltas.slice() };
    }
    cur = i;
    drag.moved = false;
    draw(); showTip(i); status();
  });

  cv.addEventListener("pointermove", (e) => {
    const q = pos(e);
    if (!drag) {
      const i = nearest(q);
      if (i !== hover) { hover = i; draw(); showTip(i); }
      cv.style.cursor = i >= 0 ? (e.ctrlKey ? "ns-resize" : "grab") : "crosshair";
      return;
    }
    if (drag.kind === "none") return;
    if (drag.kind === "band") {
      drag.x1 = q.x;
      sel = new Set([...drag.add, ...inBand(drag.x0, drag.x1)]);
      draw(); status();
      return;
    }
    const G = g();
    if (!drag.moved && Math.abs(q.y - drag.y0) < 3) return;
    if (!drag.moved) { remember(); undo[undo.length - 1].d = drag.from.slice(); drag.moved = true; }
    deltas = drag.from.slice();
    const k = drag.i;
    const want = fAt(q.y, G) - (base[k].base + drag.from[k]); // MHz the grabbed point moves by
    let idx;
    if (drag.kind === "point") idx = [k];
    else if (drag.kind === "group") idx = [...sel];
    else if (drag.kind === "all") idx = base.map((_, i) => i);
    if (idx) for (const i of idx) setF(i, base[i].base + drag.from[i] + want);
    else {
      // bend: the grabbed point moves fully, neighbours less the further they are
      const w = 5;
      base.forEach((_, i) => { const t = Math.exp(-((i - k) ** 2) / (2 * w * w)); if (t > 0.02) setF(i, base[i].base + drag.from[i] + want * t); });
      idx = [k];
    }
    settle(want > 0 ? 1 : -1, Math.min(...idx), Math.max(...idx));
    save(); draw(); showTip(k); status();
  });

  const end = (e) => {
    if (!drag) return;
    const d = drag;
    drag = null;
    S.interacting = Math.max(0, S.interacting - 1);
    if (d.kind === "all" && d.toggle && !d.moved) {
      // Ctrl+click: add / remove one point
      sel.has(d.i) ? sel.delete(d.i) : sel.add(d.i);
    }
    fitTop(); draw(); status();
    if (e?.type === "pointercancel") tip.classList.add("hidden");
  };
  cv.addEventListener("pointerup", end);
  cv.addEventListener("pointercancel", end);
  cv.addEventListener("pointerleave", () => { if (!drag) { hover = -1; tip.classList.add("hidden"); draw(); } });
  cv.addEventListener("focus", draw);
  cv.addEventListener("blur", draw);

  // ---- actions (buttons and keys)
  const targets = () => (sel.size ? [...sel] : cur >= 0 ? [cur] : []);
  const act = {
    nudge(mhz) {
      const t = targets(); if (!t.length) return note("Select a point first", true);
      remember(); for (const i of t) setF(i, F(i) + mhz);
      settle(Math.sign(mhz), Math.min(...t), Math.max(...t)); changed(); showTip(cur >= 0 ? cur : t[0]);
    },
    shiftAll(mhz) { remember(); for (let i = 0; i < n; i++) setF(i, F(i) + mhz); changed(); },
    /** from point k up the card runs at its clock (Afterburner's flat-line undervolt) */
    flattenAt(k, mhz) {
      if (k < 0) return note("Select the point to flatten from", true);
      remember();
      const flat = mhz ?? F(k);
      for (let i = 0; i < n; i++) setF(i, i >= k ? flat : Math.min(F(i), flat));
      changed();
    },
    lock() {
      const k = cur >= 0 ? cur : [...sel][0] ?? -1;
      if (k < 0 && p.lock_mv == null) return note("Select the point to lock to", true);
      remember();
      p.lock_mv = p.lock_mv != null && (k < 0 || k === lockIdx()) ? null : base[k].mv;
      changed();
      note(p.lock_mv != null ? `The card stays at ${Math.round(base[k].mv)} mV once applied (L again to unlock)` : "Unlocked");
    },
    reset() { remember(); deltas.fill(0); p.lock_mv = null; changed(); },
    resetSel() { const t = targets(); if (!t.length) return; remember(); for (const i of t) deltas[i] = 0; settle(-1, Math.min(...t), Math.max(...t)); settle(1, Math.min(...t), Math.max(...t)); changed(); },
    undo() { if (!undo.length) return; redo.push(snap()); restore(undo.pop()); },
    redo() { if (!redo.length) return; undo.push(snap()); restore(redo.pop()); },
    move(step, extend) {
      const k = Math.min(n - 1, Math.max(0, (cur < 0 ? (step > 0 ? -1 : n) : cur) + step));
      if (extend && cur >= 0) { const [a, b] = [Math.min(cur, k), Math.max(cur, k)]; for (let i = a; i <= b; i++) sel.add(i); }
      else sel = new Set([k]);
      cur = k; draw(); showTip(k); status();
    },
  };

  cv.addEventListener("keydown", (e) => {
    const k = e.key, ctrl = e.ctrlKey || e.metaKey;
    const step = e.shiftKey ? 1 : ctrl ? 100 : 15;
    let done = true;
    if (k === "ArrowUp") act.nudge(step);
    else if (k === "ArrowDown") act.nudge(-step);
    else if (k === "ArrowLeft") act.move(-1, e.shiftKey);
    else if (k === "ArrowRight") act.move(1, e.shiftKey);
    else if (k === "Home") act.move(-n, e.shiftKey);
    else if (k === "End") act.move(n, e.shiftKey);
    else if ((k === "l" || k === "L") && !ctrl) act.lock();
    else if (ctrl && (k === "f" || k === "F")) act.flattenAt(cur >= 0 ? cur : [...sel][0] ?? -1);
    else if (ctrl && (k === "d" || k === "D")) act.reset();
    else if (ctrl && (k === "z" || k === "Z") && !e.shiftKey) act.undo();
    else if (ctrl && ((k === "y" || k === "Y") || ((k === "z" || k === "Z") && e.shiftKey))) act.redo();
    else if (ctrl && (k === "a" || k === "A")) { sel = new Set(base.map((_, i) => i)); draw(); status(); }
    else if (k === "Delete" || k === "Backspace") act.resetSel();
    else if (k === "Escape" && (sel.size || cur >= 0)) { sel = new Set(); cur = -1; tip.classList.add("hidden"); draw(); status(); }
    else done = false;
    if (done) { e.preventDefault(); e.stopPropagation(); }
    else if (k.length === 1 && !ctrl) e.stopPropagation(); // typing here doesn't jump to the search box
  });

  new ResizeObserver(draw).observe(cv);
  S.cache.vfDraw = draw; // the live voltage marker (only the newest editor canvas)

  // ---- tools under the graph
  const num = (v, step, w) => h("input", { type: "number", value: v, step, style: { width: w || "72px" } });
  const mvIn = num(850, 25), mhzIn = num(1800, 15), shiftIn = num(15, 15, "60px");
  const at = (mv) => base.reduce((k, b, i) => (Math.abs(b.mv - mv) < Math.abs(base[k].mv - mv) ? i : k), 0);
  const info = h("span", { class: "dim small grow" });
  const lockBtn = h("button", { class: "btn sm ghost lock-btn", title: "Keep the card at the selected point (L)", onclick: () => act.lock() });
  const undoBtn = h("button", { class: "icon-btn sm", title: "Undo (Ctrl+Z)", onclick: () => act.undo() }, I("undo", "sm"));
  const redoBtn = h("button", { class: "icon-btn sm", title: "Redo (Ctrl+Y)", onclick: () => act.redo() }, I("redo", "sm"));
  function status() {
    const t = targets();
    info.textContent = t.length > 1 ? `${t.length} points selected (${Math.round(base[Math.min(...t)].mv)}–${Math.round(base[Math.max(...t)].mv)} mV)` : t.length ? `Point ${Math.round(base[t[0]].mv)} mV · ${Math.round(F(t[0]))} MHz` : "";
    lockBtn.replaceChildren(I("lock", "sm"), p.lock_mv != null ? "Unlock" : "Lock point");
    lockBtn.classList.toggle("on", p.lock_mv != null);
    undoBtn.disabled = !undo.length; redoBtn.disabled = !redo.length;
  }
  const keys = [["Drag", "move a point"], ["Shift+drag", "select a range"], ["Drag a selected point", "move the range"], ["Ctrl+drag", "move the whole curve"], ["Alt+drag", "bend the neighbours along"], ["Ctrl+click", "add / remove a point"],
    ["↑ ↓", "±15 MHz (Shift ±1, Ctrl ±100)"], ["← →", "next point (Shift extends)"], ["L", "lock the card to the point"], ["Ctrl+F", "flatten from the point"], ["Del", "selected points back to stock"], ["Ctrl+D", "reset the curve"], ["Ctrl+Z / Ctrl+Y", "undo / redo"], ["Ctrl+A / Esc", "select all / none"]];
  const help = h("div", { class: "vf-keys hidden" }, keys.map(([k, d]) => h("div", null, h("kbd", null, k), h("span", null, d))));
  status();
  return h("div", { class: "col", style: { gap: "8px" } },
    wrap,
    h("div", { class: "row wrap vf-tools" }, helpTip("gpu.curve"), undoBtn, redoBtn, lockBtn, helpTip("gpu.lock"), info,
      h("button", { class: "btn sm ghost", onclick: (e) => { help.classList.toggle("hidden"); e.currentTarget.classList.toggle("on"); } }, I("keyboard", "sm"), "Shortcuts")),
    help,
    h("div", { class: "row wrap vf-tools" },
      helpTip("gpu.flat"), h("span", { class: "dim small" }, "Hold"), mhzIn, h("span", { class: "dim small" }, "MHz from"), mvIn, h("span", { class: "dim small" }, "mV"),
      h("button", { class: "btn sm", title: "Undervolt: from this voltage up the card runs at this clock (Afterburner's flat-line trick)", onclick: () => act.flattenAt(at(+mvIn.value), +mhzIn.value) }, "Apply"),
      h("button", { class: "btn sm ghost", title: "Keep the clock the curve has at that voltage, from there up", onclick: () => act.flattenAt(at(+mvIn.value)) }, "Flatten there"),
      h("span", { class: "vsep" }),
      h("span", { class: "dim small" }, "Shift"), helpTip("gpu.shift"), shiftIn,
      h("button", { class: "icon-btn sm", title: "Whole curve up", onclick: () => act.shiftAll(+shiftIn.value) }, I("plus", "sm")),
      h("button", { class: "icon-btn sm", title: "Whole curve down", onclick: () => act.shiftAll(-shiftIn.value) }, I("minus", "sm")),
      h("span", { class: "grow" }),
      h("button", { class: "btn sm ghost", title: "Back to the stock curve (Ctrl+D)", onclick: () => act.reset() }, "Reset")),
    h("div", { class: "dim small" }, "Click the graph, then use the keys (Shortcuts lists them). Dashed grey: the card's stock curve. Orange: the voltage now. Green: the locked point."));
}
