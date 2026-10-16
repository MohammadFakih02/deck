// The home dashboard: a grid of tiles, refreshed live while Deck is open.

const TILE_INFO = {
  clock: { name: "Clock", w: "" },
  media: { name: "Now playing", w: "" },
  system: { name: "PC", w: "" },
  profiles: { name: "Performance profiles", w: "" },
  audio: { name: "Sound", w: "" },
  pinned: { name: "Pinned & frequent", w: "" },
  quick: { name: "Quick actions", w: "" },
  clipboard: { name: "Clipboard", w: "" },
  power: { name: "Power & timers", w: "" },
  network: { name: "Network", w: "" },
};

function tileHead(ic, title, link, onlink, onset) {
  return h("div", { class: "tile-head" }, I(ic), h("span", null, title), h("span", { class: "grow" }),
    link ? h("button", { class: "link", onclick: onlink }, link) : null,
    onset ? h("button", { class: "link tile-set", title: "Choose what this shows", "aria-label": "Choose what this shows", onclick: onset }, I("sliders")) : null);
}

const ghz = (mhz) => (mhz ? (mhz / 1000).toFixed(1) + " GHz" : null);
const watts = (w) => (w != null ? Math.round(w) + " W" : null);
/** "0.9 / 10.0 GB": both numbers in the unit of the total, so they read together. */
const ofBytes = (a, b) => {
  const y = fmtBytes(b), u = y.split(" ")[1], k = ["B", "KB", "MB", "GB", "TB"].indexOf(u);
  if (k <= 0) return `${fmtBytes(a)} / ${y}`;
  const v = a / 1024 ** k;
  return `${v >= 100 ? v.toFixed(0) : v.toFixed(1)} / ${y}`;
};
const pctOf = (v) => (v != null ? Math.round(v) + "%" : "–");

/** The PC tile's cards: label, big value (usage), details (clock, power), temperature, graph. */
const PC_CARDS = {
  cpu: (s) => {
    const c = S.live?.cpu_now || {};
    // the CPU's clock, power and temperature come from Deck's hardware engine, which needs admin
    const why = S.state.admin ? null : h("button", { class: "vlink", title: "The CPU's clock, power and temperature need Deck to run as admin", onclick: (e) => { e.stopPropagation(); pcAdminSetup(); } }, "Needs admin");
    return { k: "CPU", v: pctOf(s.cpu), l1: c.mhz || c.w != null ? [ghz(c.mhz), watts(c.w)] : [`${s.cores} threads`], l2: c.t != null ? fmtTemp(c.t) : why, hist: s.cpu_hist, color: accent() };
  },
  ram: (s) => ({ k: "Memory", v: pctOf(s.ram_pct), l1: [ofBytes(s.ram_used, s.ram_total)], l2: `${fmtBytes(s.ram_total - s.ram_used)} free`, hist: s.ram_hist, color: "#b98cff" }),
  gpu: (s) => {
    const g = s.gpu, c = S.live?.cpu_now || {};
    if (!g && c.gpu_t == null && c.gpu_l == null) return null; // no graphics card Deck can read
    const t = g?.temp ?? c.gpu_t;
    return { k: "GPU", v: pctOf(g?.util ?? c.gpu_l), l1: [ghz(g?.clock), watts(g?.power_w)], l2: t != null ? fmtTemp(t) : null, hist: s.gpu_hist, color: "#3ddc97" };
  },
  vram: (s) => {
    const g = s.gpu;
    if (!g || !g.vram_total) return null;
    return { k: "VRAM", v: pctOf(g.vram_used * 100 / g.vram_total), l1: [ofBytes(g.vram_used, g.vram_total)], l2: `${fmtBytes(g.vram_total - g.vram_used)} free`, hist: s.vram_hist, color: "#ffb547" };
  },
  net: (s) => ({ k: "Network", v: fmtRate(s.down), small: true, l1: ["down"], l2: `up ${fmtRate(s.up)}`, hist: s.down_hist, max: 0, color: "#4fd1ff" }),
};
/** Sets Deck up to run as admin (the same step as the CPU and GPU pages' Set up button). */
function pcAdminSetup() {
  invoke("elevate_setup").catch((e) => note(String(e), true));
}

const PC_NAMES = { cpu: "CPU", ram: "Memory", gpu: "Graphics card", vram: "Graphics memory", net: "Network" };

/** Which cards the PC tile shows and in what order, and whether the drives show. */
function pcCustomize() {
  const st = S.state.settings;
  let on = [...(st.pc_cards || ["cpu", "ram", "gpu", "vram"])];
  let drives = st.pc_drives !== false;
  const body = h("div", { class: "pc-set" });
  const draw = () => {
    const all = [...on, ...Object.keys(PC_NAMES).filter((id) => !on.includes(id))];
    body.replaceChildren(
      h("div", { class: "dim", style: { marginBottom: "8px" } }, "Pick the cards and their order. Each shows the usage, then clock and power, then the temperature."),
      S.state.admin ? null : h("div", { class: "banner", style: { marginBottom: "8px" } }, I("info"), h("div", { class: "grow" }, "The CPU's clock, power and temperature need Deck to run as admin."), h("button", { class: "btn sm", onclick: pcAdminSetup }, "Set up")),
      ...all.map((id) => {
        const i = on.indexOf(id);
        return h("div", { class: "pc-set-row" },
          toggle(i >= 0, (v) => { on = v ? [...on, id] : on.filter((x) => x !== id); draw(); }),
          h("span", { class: "grow" }, PC_NAMES[id]),
          h("button", { class: "icon-btn", title: "Move up", "aria-label": "Move up", disabled: i <= 0, onclick: () => { [on[i - 1], on[i]] = [on[i], on[i - 1]]; draw(); } }, "↑"),
          h("button", { class: "icon-btn", title: "Move down", "aria-label": "Move down", disabled: i < 0 || i >= on.length - 1, onclick: () => { [on[i + 1], on[i]] = [on[i], on[i + 1]]; draw(); } }, "↓"));
      }),
      h("div", { class: "pc-set-row", style: { marginTop: "6px" } }, toggle(drives, (v) => { drives = v; }), h("span", { class: "grow" }, "Drives and free space")));
  };
  draw();
  modal("PC tile", body, (close) => [
    h("button", { class: "btn ghost", onclick: close }, "Cancel"),
    h("button", { class: "btn primary", onclick: async () => { close(); await setSetting("pc_cards", on); await setSetting("pc_drives", drives); render(); } }, "Save")]);
}


function greeting() {
  const hr = new Date().getHours();
  return hr < 5 ? "Up late" : hr < 12 ? "Good morning" : hr < 18 ? "Good afternoon" : "Good evening";
}

const Tiles = {
  clock() {
    const now = new Date();
    const p = S.live?.power;
    const chips = [];
    if (p?.awake_active) chips.push(h("button", { class: "chip on", onclick: () => go("power", "awake") }, I("coffee"), "Staying awake"));
    if (p?.plan) chips.push(h("button", { class: "chip warn", onclick: () => go("power", "plan") }, I("power"), planLabel(p.plan)));
    for (const t of (p?.timers || []).slice(0, 2)) {
      const left = t.paused_left != null ? t.paused_left / 1000 : (t.end_ms - p.now) / 1000;
      chips.push(h("button", { class: "chip" + (t.done ? " warn" : ""), onclick: () => go("power", "timers") }, I("timer"), t.done ? `${t.label} done` : fmtDur(left)));
    }
    return h("div", { class: "tile clock" },
      h("div", null,
        h("div", { class: "time" }, now.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })),
        h("div", { class: "date" }, now.toLocaleDateString([], { weekday: "long", day: "numeric", month: "long" }))),
      chips.length ? h("div", { class: "chips", "data-drop": "1" }, chips) : h("div", { class: "dim", "data-drop": "1" }, `${greeting()}${S.state?.user ? ", " + S.state.user : ""}.`),
      foldsAt(calendar(now), 2));
  },

  media() {
    const m = S.live?.media?.current;
    const el = h("div", { class: "tile" }, tileHead("play", "Now playing", S.live?.media?.others?.length ? `+${S.live.media.others.length} more` : null, () => go("audio")));
    if (!m || !m.title) {
      el.append(h("div", { class: "empty" }, "Nothing is playing"));
      return el;
    }
    const playing = m.status === "playing";
    const pct = m.len > 0 ? Math.min(100, (m.pos / m.len) * 100) : 0;
    const art = h("div", { class: "art", style: m.art ? { backgroundImage: `url("${m.art}")` } : null }, m.art ? null : I("music", "lg"));
    el.append(
      h("div", { class: "media" }, art,
        h("div", { class: "grow" },
          h("div", { class: "title ell" }, m.title),
          h("div", { class: "artist ell" }, m.artist || m.album || ""),
          h("div", { class: "from" }, m.app))),
      m.len > 0 ? h("div", null,
        h("div", { class: "bar" }, h("i", { style: { width: pct + "%" } })),
        h("div", { class: "row dim num", style: { justifyContent: "space-between", fontSize: "11.5px", marginTop: "5px" } }, h("span", null, fmtDur(m.pos)), h("span", null, fmtDur(m.len)))) : null,
      h("div", { class: "controls" },
        h("button", { class: "icon-btn", title: "Previous", disabled: !m.can_prev, onclick: () => media("prev", m.id) }, I("prev")),
        h("button", { class: "play", title: playing ? "Pause" : "Play", onclick: () => media("toggle", m.id) }, I(playing ? "pause" : "play")),
        h("button", { class: "icon-btn", title: "Next", disabled: !m.can_next, onclick: () => media("next", m.id) }, I("next"))));
    return el;
  },

  system() {
    const s = S.live?.sys;
    const el = h("div", { class: "tile" }, tileHead("activity", "PC", "Processes", () => go("system"), () => pcCustomize()));
    if (!s) return el;
    const st = S.state.settings;
    const cards = (st.pc_cards || ["cpu", "ram", "gpu", "vram"]).map((id) => PC_CARDS[id]?.(s)).filter(Boolean);
    if (cards.length) {
      el.append(h("div", { class: "vitals" }, cards.map((c) => {
        const cv = h("canvas", { "data-drop": "2" });
        requestAnimationFrame(() => spark(cv, c.hist || [], c.max ?? 100, c.color));
        // the same lines on every card: what, how much, the details, then the temperature
        return h("div", { class: "vital" }, h("div", { class: "k" }, c.k), h("div", { class: "v", style: c.small ? { fontSize: "15px" } : null }, c.v),
          h("div", { class: "s" }, c.l1.filter(Boolean).map((t, i) => h("span", { style: { whiteSpace: "nowrap" } }, (i ? " · " : "") + t))),
          h("div", { class: "s" }, c.l2 || " "), cv);
      })));
    }
    if (st.pc_drives !== false) {
      const drives = (s.disks || []).filter((d) => !d.removable && d.total > 0).sort((a, b) => a.mount.localeCompare(b.mount));
      el.append(h("div", { class: "drives", "data-drop": "1" }, drives.map((d) => {
        const used = 1 - d.free / d.total;
        return h("div", { class: "drive" + (used > 0.95 ? " crit" : used > 0.9 ? " full" : "") },
          h("b", null, d.mount),
          h("div", { class: "bar" }, h("i", { style: { width: used * 100 + "%" } })),
          h("span", { class: "dim num" }, `${fmtBytes(d.free)} free`));
      })));
    }
    return el;
  },

  audio() {
    const el = h("div", { class: "tile" }, tileHead("volume", "Sound", "Mixer", () => go("audio")));
    const m = S.live?.master;
    if (!m) return el;
    const outs = S.cache.outputs || [];
    if (outs.length > 1) {
      el.append(h("div", { class: "outputs", "data-drop": "1" }, outs.map((o) =>
        h("button", { class: "chip" + (o.default ? " on" : ""), title: o.name, onclick: () => setOutput(o.id) }, I(deviceIcon(o.name)), shortDevice(o.name)))));
    }
    el.append(volRow(null, "Everything", m.vol, m.muted, m.peak, (v) => invoke("set_master", { vol: v }), () => invoke("set_master", { muted: !m.muted })));
    const apps = (S.live.apps || []).filter((a) => a.key !== "system").slice(0, 3);
    for (const a of apps) {
      el.append(foldsAt(volRow(a, a.name, a.vol, a.muted, a.peak, (v) => invoke("set_app_volume", { key: a.key, vol: v }), () => invoke("set_app_volume", { key: a.key, muted: !a.muted })), 2));
    }
    return el;
  },

  profiles() {
    const pages = S.state.hw?.pages || [];
    const el = h("div", { class: "tile" }, tileHead("gauge", "Performance", "Details", () => go(pages.includes("cpu") ? "cpu" : "gpu")));
    const kinds = [["cpu", "CPU"], ["gpu", "GPU"]].filter(([k]) => pages.includes(k));
    const live = S.live?.profiles || {};
    const shown = kinds.map(([k, label]) => [k, label, S.cache[k]]).filter(([k, , st]) => st?.profiles?.length && (k !== "gpu" || st.found));
    if (!shown.length) {
      el.append(h("div", { class: "empty" }, kinds.length ? "Make a CPU or GPU profile and switch between them here." : "This PC has no CPU or GPU profiles to switch."));
      return el;
    }
    for (const [kind, label, st] of shown) {
      const active = live[kind] ?? st.profiles.find((x) => x.p.id === st.active)?.p.name;
      el.append(h("div", { class: "prof-pick" },
        h("div", { class: "k" }, label),
        h("select", { class: "prof-select", hidden: true, "data-alt": "2", "aria-label": label + " profile", onchange: (e) => homeSwitch(kind, e.target.value) },
          active && !st.profiles.some((x) => x.p.name === active) ? h("option", { value: "", selected: true, disabled: true }, active) : null,
          st.profiles.map((x) => h("option", { value: x.p.id, selected: x.p.name === active }, x.p.name))),
        h("div", { class: "prof-chips", "data-drop": "2" }, st.profiles.map((x) =>
          h("button", { class: "pchip" + (x.p.name === active ? " on" : ""), title: [x.summary, x.p.hotkey ? "Key: " + x.p.hotkey.replaceAll("+", " + ") : null].filter(Boolean).join(" · "), onclick: () => homeSwitch(kind, x.p.id) },
            x.icon_url ? h("img", { src: x.icon_url, alt: "" }) : null, x.p.name))),
        h("div", { class: "faint small ell", "data-drop": "1" }, st.profiles.find((x) => x.p.name === active)?.summary || "Not one of your profiles right now")));
    }
    return el;
  },

  pinned() {
    const el = h("div", { class: "tile" }, tileHead("star", "Pinned & frequent"));
    const pins = S.state.settings.pinned || [];
    const apps = S.cache.applist || [];
    const byId = new Map(apps.map((a) => [a.id, a]));
    const items = pins.map((p) => ({ pin: p }));
    // fill with the most-used apps
    const fre = Object.entries(S.state.settings.frecency || {})
      .filter(([k]) => k.startsWith("app:"))
      .sort((a, b) => b[1][0] - a[1][0])
      .map(([k]) => k.slice(4))
      .filter((id) => byId.has(id) && !pins.some((p) => p.target === id));
    for (const id of fre) { if (items.length >= 8) break; items.push({ app: byId.get(id) }); }
    // A fresh install has no history yet: suggest well-known apps you have.
    const starters = ["Discord", "Steam", "Spotify", "Visual Studio Code", "Google Chrome", "Microsoft Edge", "File Explorer", "Terminal", "Settings", "Epic Games Launcher", "OBS Studio", "Notepad"];
    for (const n of starters) {
      if (items.length >= 8) break;
      const a = apps.find((x) => x.name === n);
      if (a && !items.some((it) => (it.app && it.app.id === a.id) || (it.pin && it.pin.target === a.id))) items.push({ app: a });
    }
    if (!items.length) {
      el.append(h("div", { class: "empty" }, "Right-click an app here to pin it, or just use Deck and your favourites show up here."));
      return el;
    }
    el.append(h("div", { class: "pins" }, items.slice(0, 8).map((it) => {
      if (it.app) return appButton(it.app);
      const p = it.pin;
      if (p.kind === "app") { const a = byId.get(p.target) || { id: p.target, name: p.name }; return appButton(a, true); }
      return h("button", { class: "app", title: p.target, onclick: () => p.kind === "url" ? run("open_url", { url: p.target }) : run("open_path", { path: p.target }) },
        p.kind === "url" ? h("div", { class: "ph" }, I("globe")) : appIcon("file", p.target, p.name),
        h("span", null, p.name));
    })));
    return el;
  },

  quick() {
    const el = h("div", { class: "tile" }, tileHead("zap", "Quick actions"));
    const p = S.live?.power;
    const dark = S.cache.dark;
    const qa = (ic, label, fn, on, tip) => h("button", { class: "qa" + (on ? " on" : ""), title: tip || label, onclick: fn }, I(ic), h("span", null, label));
    el.append(h("div", { class: "quick" },
      qa("coffee", p?.awake_active ? "Staying awake" : "Keep awake", () => invoke("set_awake", { mode: p?.awake_active ? "off" : "on" }), p?.awake_active),
      qa(dark === false ? "sun" : "moon", dark === false ? "Light mode" : "Dark mode", toggleDark, false),
      qa("droplet", "Pick colour", () => tool("color")),
      qa("type", "Copy text", () => tool("text"), false, "Copy text from anywhere on the screen"),
      qa("ruler", "Measure", () => tool("measure"), false, "Measure distances on the screen"),
      qa("monitor", "Screen off", () => invoke("run_power", { action: "screen_off" })),
      qa("lock", "Lock", () => invoke("run_power", { action: "lock" })),
      qa("moon", "Sleep", async () => { if (await confirmBox("Sleep now?", "Your PC goes to sleep right away.", "Sleep")) invoke("run_power", { action: "sleep" }); })));
    return el;
  },

  clipboard() {
    const el = h("div", { class: "tile" }, tileHead("clipboard", "Clipboard", "History", () => go("clipboard")));
    const items = (S.cache.clips || []).slice(0, 5);
    if (!items.length) {
      el.append(h("div", { class: "empty" }, "Things you copy show up here."));
      return el;
    }
    el.append(h("div", { class: "col", style: { gap: "6px" } }, items.map((c) =>
      h("button", { class: "mini-clip", title: "Click to paste", onclick: () => pasteClip(c.id) },
        c.pinned ? I("pin", "sm") : I(c.kind === "image" ? "image" : c.kind === "files" ? "file" : "copy", "sm"),
        h("span", { class: "ell grow" }, c.kind === "image" ? `Image ${c.w}×${c.h}` : c.kind === "files" ? c.files.map((f) => f.split("\\").pop()).join(", ") : c.text.replace(/\s+/g, " ").trim()),
        h("span", { class: "faint", style: { fontSize: "11px" } }, ago(c.ts))))));
    return el;
  },

  power() {
    const el = h("div", { class: "tile" }, tileHead("timer", "Power & timers", "Timers", () => go("power", "timers")));
    const p = S.live?.power;
    if (!p) return el;
    if (p.plan) {
      el.append(h("div", { class: "row" }, h("div", { class: "grow" }, h("div", { style: { fontWeight: 700 } }, planLabel(p.plan)), h("div", { class: "dim", style: { fontSize: "12px" } }, p.plan.progress || planCond(p.plan))),
        h("button", { class: "btn sm", onclick: () => invoke("cancel_plan") }, "Cancel")));
    }
    const timers = p.timers || [];
    if (timers.length) {
      el.append(h("div", { class: "timers" }, timers.slice(0, 2).map((t) => timerRow(t, p.now))));
    }
    el.append(h("div", { class: "presets", "data-drop": "1" },
      [5, 10, 15, 25, 60].map((m) => h("button", { class: "chip", onclick: () => invoke("timer_add", { secs: m * 60, label: "" }) }, I("timer"), `${m} min`))));
    el.append(h("div", { class: "row", style: { marginTop: "auto" }, "data-drop": "2" },
      h("div", { class: "grow" }, h("div", { style: { fontWeight: 600 } }, "Keep awake"), h("div", { class: "dim", style: { fontSize: "12px" } }, p.awake_active ? awakeText(p.awake) : "PC sleeps normally")),
      toggle(!!p.awake_active, (v) => invoke("set_awake", { mode: v ? "on" : "off" }))));
    if (!p.plan) el.append(h("button", { class: "btn sm", "data-drop": "1", onclick: () => go("power", "plan") }, I("power", "sm"), "Shut down or sleep when…"));
    return el;
  },

  network() {
    const s = S.live?.sys;
    const el = h("div", { class: "tile" }, tileHead("wifi", "Network", "Speed test", () => go("system", "network")));
    if (!s) return el;
    const c = h("canvas", { style: { width: "100%", height: "46px" } });
    el.append(h("div", { class: "two" },
      h("div", null, h("div", { class: "label" }, "Download"), h("div", { style: { font: "700 20px var(--display)" } }, fmtRate(s.down))),
      h("div", null, h("div", { class: "label" }, "Upload"), h("div", { style: { font: "700 20px var(--display)" } }, fmtRate(s.up)))), c);
    requestAnimationFrame(() => spark(c, s.down_hist, 0, "#3ddc97"));
    return el;
  },
};

function calendar(now) {
  const first = new Date(now.getFullYear(), now.getMonth(), 1);
  const start = (first.getDay() + 6) % 7; // Monday first
  const days = new Date(now.getFullYear(), now.getMonth() + 1, 0).getDate();
  const prevDays = new Date(now.getFullYear(), now.getMonth(), 0).getDate();
  const cells = ["M", "T", "W", "T", "F", "S", "S"].map((d) => h("b", null, d));
  const total = Math.ceil((start + days) / 7) * 7;
  for (let i = 0; i < total; i++) {
    const d = i - start + 1;
    const out = d < 1 || d > days;
    const label = out ? (d < 1 ? prevDays + d : d - days) : d;
    cells.push(h("span", { class: (out ? "out" : "") + (d === now.getDate() ? " today" : "") + (i % 7 >= 5 && !out ? " we" : "") }, label));
  }
  return h("div", { class: "cal" }, cells);
}

function deviceIcon(n) {
  return /headset|headphone|earphone|hyperx|arctis|airpods|buds/i.test(n) ? "headphones" : /cable|voicemeeter|virtual audio/i.test(n) ? "sliders" : "speaker";
}

function shortDevice(n) {
  const m = n.match(/^(.*?)\s*\((.*)\)$/);
  if (!m) return n;
  const [, a, b] = m;
  if (/^(speakers|headphones|headset earphone|headset|output)$/i.test(a)) return b.replace(/\s*(virtual surround sound|high definition audio|\d+- )/gi, "").trim() || a;
  return a;
}

function awakeText(a) {
  if (!a) return "";
  if (a.mode === "until") return `For ${fmtWords(Math.max(0, a.until - Date.now() / 1000))} more`;
  if (a.mode === "app") return `While ${a.app.replace(/\.exe$/i, "")} runs`;
  if (a.mode === "downloads") return "Until downloads finish";
  return "Until you turn it off";
}

/** Marks a part a short tile folds away at step `n` (layout.js pgFit). */
function foldsAt(el, n) {
  el.dataset.drop = n;
  return el;
}

function volRow(app, name, vol, muted, peak, onvol, onmute) {
  const val = h("span", { class: "val num" }, Math.round(vol * 100));
  const s = slider(Math.round(vol * 100), 0, 100, 1, (v) => { val.textContent = v; onvol(v / 100); });
  const meter = h("div", { class: "meter" }, h("i", { style: { width: Math.min(100, (peak || 0) * 100) + "%" } }));
  return h("div", { class: "vol" },
    h("button", { class: "icon-btn sm", title: muted ? "Unmute" : "Mute", onclick: onmute, style: { width: "26px", height: "26px" } },
      muted ? I("volume-x", "sm") : app?.exe ? appIcon("file", app.exe, name) : I("volume", "sm")),
    h("div", null, h("div", { class: "name" }, h("span", { class: "ell" }, name)), s, meter),
    val);
}

function appButton(a, pinned) {
  return h("button", {
    class: "app", title: a.name,
    onclick: () => run("launch_app", { id: a.id }),
    oncontextmenu: (e) => { e.preventDefault(); appMenu(a, pinned); },
  }, appIcon("app", a.id, a.name), h("span", null, a.name));
}

function appMenu(a, pinned) {
  const pins = S.state.settings.pinned || [];
  const isPinned = pins.some((p) => p.kind === "app" && p.target === a.id);
  modal(a.name, h("div", { class: "dim" }, a.path || a.id), (close) => [
    a.path ? h("button", { class: "btn ghost", onclick: () => { invoke("reveal", { path: a.path }); close(); } }, "Show file") : null,
    h("button", { class: "btn", onclick: () => { togglePin(a, !isPinned); close(); } }, isPinned ? "Unpin" : "Pin to home"),
    h("button", { class: "btn primary", onclick: () => { run("launch_app", { id: a.id }); close(); } }, "Open"),
  ]);
}

function togglePin(a, on) {
  let pins = [...(S.state.settings.pinned || [])];
  if (on) pins.push({ id: "p" + Date.now(), kind: "app", target: a.id, name: a.name });
  else pins = pins.filter((p) => !(p.kind === "app" && p.target === a.id));
  setSetting("pinned", pins);
  render();
}

function timerRow(t, now) {
  const left = t.paused_left != null ? t.paused_left / 1000 : Math.max(0, (t.end_ms - now) / 1000);
  const total = (t.secs || 0) * 1000;
  const frac = t.done ? 1 : total ? Math.min(1, Math.max(0, 1 - (left * 1000) / total)) : 0;
  return h("div", { class: "tmr" + (t.done ? " done" : "") + (t.paused_left != null ? " paused" : "") },
    h("div", null, h("div", { class: "left" }, t.done ? "Done" : fmtDur(left)), h("div", { class: "dim", style: { fontSize: "12px" } }, t.label || (t.secs ? fmtWords(t.secs) + " timer" : ""))),
    h("div", { class: "row", style: { gap: "2px" } },
      t.done ? h("button", { class: "btn sm", onclick: () => invoke("timer_cmd", { id: t.id, cmd: "restart" }) }, I("refresh", "sm"), "Again")
        : h("button", { class: "icon-btn sm", title: t.paused_left != null ? "Resume" : "Pause", onclick: () => invoke("timer_cmd", { id: t.id, cmd: t.paused_left != null ? "resume" : "pause" }) }, I(t.paused_left != null ? "play" : "pause", "sm")),
      h("button", { class: "icon-btn sm danger", title: "Remove", onclick: () => invoke("timer_cmd", { id: t.id, cmd: "remove" }) }, I("x", "sm"))),
    total ? h("div", { class: "tmr-bar" }, h("i", { style: { width: frac * 100 + "%" } })) : null);
}

const ACTION_LABEL = { shutdown: "Shut down", restart: "Restart", sleep: "Sleep", hibernate: "Hibernate", lock: "Lock", signout: "Sign out", screen_off: "Screen off", notify: "Notify me" };
function planLabel(p) {
  if (p.state === "countdown") return `${ACTION_LABEL[p.action] || p.action} in ${Math.max(0, p.countdown_until - Math.floor(Date.now() / 1000))} s`;
  return `${ACTION_LABEL[p.action] || p.action} ${planCondShort(p)}`;
}
function planCondShort(p) {
  switch (p.cond) {
    case "time": return "at " + new Date(p.at * 1000).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
    case "app_closes": return `when ${p.app.replace(/\.exe$/i, "")} closes`;
    case "downloads": return "when downloads finish";
    case "cpu_idle": return "when the PC is idle";
    case "net_idle": return "when the network is quiet";
    case "media_stops": return "when playback stops";
    case "idle": return `after ${p.minutes} min away`;
    default: return "";
  }
}
function planCond(p) {
  switch (p.cond) {
    case "downloads": return "Waiting for browser/Steam downloads to finish";
    case "cpu_idle": return `Waiting for CPU under 12% for ${p.minutes} min`;
    case "net_idle": return `Waiting for the network to be quiet for ${p.minutes} min`;
    case "media_stops": return "Waiting for music/video to stop";
    case "time": return "Scheduled";
    case "app_closes": return `Waiting for ${p.app} to close`;
    case "idle": return `When you've been away ${p.minutes} min`;
    default: return "";
  }
}

async function homeSwitch(kind, id) {
  try { await invoke("profile_switch", { kind, id }); } catch (e) { note(String(e), true); }
  try { S.cache[kind] = await invoke(kind + "_state"); S.live = { ...(S.live || {}), ...(await invoke("live")) }; } catch { /* the next tick catches up */ }
  refreshHome();
  renderSide();
}

async function media(action, id) {
  try { await invoke("media_control", { id: id || "", action }); } catch (e) { note(String(e), true); }
  setTimeout(tick, 250);
}

async function setOutput(id) {
  await run("set_output", { id });
  S.cache.outputs = (await invoke("audio_state")).outputs;
  render();
}

async function toggleDark() {
  const dark = S.cache.dark;
  await run("set_dark", { dark: dark === false });
  S.cache.dark = dark === false;
  render();
}

async function tool(mode) {
  try { await invoke("screen_tool", { mode }); } catch (e) { note(String(e), true); }
}

async function pasteClip(id) {
  const paste = S.state.settings.clip_paste;
  await run("clip_copy", { id, paste });
  if (!paste) note("Copied");
}

function debounce(fn, ms) {
  let t;
  return (...a) => { clearTimeout(t); t = setTimeout(() => fn(...a), ms); };
}

function renderHome() {
  let tiles = S.state.settings.tiles || [];
  // the Performance tile arrived later: add it once for people who already arranged their tiles
  try {
    if (!tiles.includes("profiles") && !localStorage.getItem("deck.profilesTile")) {
      localStorage.setItem("deck.profilesTile", "1");
      const at = tiles.indexOf("system") + 1;
      tiles = [...tiles.slice(0, at), "profiles", ...tiles.slice(at)];
      setSetting("tiles", tiles);
    }
  } catch { /* no storage: the tile can still be turned on in Settings */ }
  const defs = Object.keys(TILE_INFO).filter((id) => Tiles[id]).map((id) => ({ id, name: TILE_INFO[id].name, w: 4, build: () => Tiles[id]() }));
  const editing = pgEditing("home");
  const shown = (S.state.settings.tiles || []).filter((t) => Tiles[t]).length;
  return h("div", { class: "home" },
    h("div", { class: "home-bar" }, editing ? h("b", null, "Arrange Home") : null, h("span", { class: "grow" }), layoutButton("home")),
    shown || editing ? panelGrid("home", defs, HOME_STORE, "tiles") : h("div", { class: "empty" }, "Every panel is hidden. Press Customize to bring some back."));
}

/** Refresh live tiles without rebuilding the ones being used. */
function refreshHome() {
  const grid = document.querySelector(".tiles");
  if (!grid) return;
  if (S.interacting || mouseHeld() || pgEditing("home")) return; // a held button would be swapped out and its click lost
  for (const el of [...grid.children]) {
    const id = el.dataset.panel;
    if (!id || !Tiles[id]) continue;
    if (el.contains(document.activeElement) && document.activeElement.tagName !== "BUTTON") continue;
    const spot = focusSpot(el);
    const fresh = Tiles[id]();
    pgKeep(el, fresh);
    el.replaceWith(fresh);
    nameControls(fresh);
    refocus(fresh, spot);
  }
}
