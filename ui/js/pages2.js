// Pages: Files (disk space, cleanup, search index), Power, System.

// ------------------------------------------------------------------ files

const TM_COLORS = ["#3f6fd8", "#7a52d6", "#c24f94", "#d3693c", "#c9a13a", "#3c9d74", "#338aa6", "#5b62c9", "#a24fb8", "#b8584f", "#6a8a3a", "#3a7f9d"];

function squarify(items, x, y, w, hgt) {
  const total = items.reduce((a, b) => a + b.size, 0);
  if (!total || w <= 0 || hgt <= 0) return [];
  const scale = (w * hgt) / total;
  const nodes = items.filter((it) => it.size > 0).map((it) => ({ ...it, area: it.size * scale }));
  const out = [];
  let row = [];
  let rect = { x, y, w, h: hgt };
  const worst = (r, len) => {
    const s = r.reduce((a, b) => a + b.area, 0);
    const mx = Math.max(...r.map((q) => q.area)), mn = Math.min(...r.map((q) => q.area));
    return Math.max((len * len * mx) / (s * s), (s * s) / (len * len * mn));
  };
  const lay = (r, rc) => {
    const s = r.reduce((a, b) => a + b.area, 0);
    if (rc.w >= rc.h) {
      const cw = s / rc.h;
      let cy = rc.y;
      for (const q of r) { const ch = q.area / cw; out.push({ ...q, x: rc.x, y: cy, w: cw, h: ch }); cy += ch; }
      return { x: rc.x + cw, y: rc.y, w: rc.w - cw, h: rc.h };
    }
    const ch = s / rc.w;
    let cx = rc.x;
    for (const q of r) { const cw = q.area / ch; out.push({ ...q, x: cx, y: rc.y, w: cw, h: ch }); cx += cw; }
    return { x: rc.x, y: rc.y + ch, w: rc.w, h: rc.h - ch };
  };
  let i = 0;
  while (i < nodes.length) {
    const len = Math.min(rect.w, rect.h);
    if (!row.length || worst([...row, nodes[i]], len) <= worst(row, len)) { row.push(nodes[i]); i++; }
    else { rect = lay(row, rect); row = []; }
  }
  if (row.length) lay(row, rect);
  return out;
}

Pages.files = {
  async load() {
    S.cache.diskStatus = await invoke("disk_status");
    if (S.sub === "cleanup") {
      S.cache.cleanup = await invoke("cleanup_state");
      if (!S.cache.cleanup.targets.length && !S.cache.cleanup.busy) { invoke("cleanup_scan"); }
    }
    if (S.cache.diskStatus.root && !S.cache.diskStatus.running) {
      S.cache.diskView = await invoke("disk_children", { id: S.cache.diskNode || 0 });
      S.cache.diskBig = await invoke("disk_big");
    }
  },
  render() {
    const sub = S.sub || "space";
    const el = h("div", null, pageHead("files", sub));
    if (sub === "cleanup") el.append(cleanupView());
    else el.append(diskView());
    return el;
  },
  live: true,
  async tick() {
    const st = await invoke(S.sub === "cleanup" ? "cleanup_state" : "disk_status");
    if (S.sub === "cleanup") {
      const was = JSON.stringify(S.cache.cleanup);
      S.cache.cleanup = st;
      return was !== JSON.stringify(st);
    }
    const wasRunning = S.cache.diskStatus?.running;
    S.cache.diskStatus = st;
    if (wasRunning && !st.running && st.root) {
      S.cache.diskNode = 0;
      S.cache.diskView = await invoke("disk_children", { id: 0 });
      S.cache.diskBig = await invoke("disk_big");
    }
    return st.running || wasRunning;
  },
};

function diskView() {
  const st = S.cache.diskStatus || {};
  const el = h("div", null);
  const drives = (S.live?.sys?.disks || []).filter((d) => d.total > 0).sort((a, b) => a.mount.localeCompare(b.mount));
  el.append(h("div", { class: "drives-pick" },
    drives.map((d) => {
      const used = 1 - d.free / d.total;
      return h("button", { class: "dcard" + (st.root === d.mount + "\\" ? " on" : ""), disabled: st.running, onclick: () => startScan(d.mount + "\\") },
        h("div", { class: "row" }, I("disk", "lg"), h("div", { class: "grow" }, h("div", { class: "big" }, `${d.mount} ${d.name || ""}`)), used > 0.9 ? h("span", { class: "chip " + (used > 0.95 ? "bad" : "warn") }, "Almost full") : null),
        h("div", { class: "drive" + (used > 0.95 ? " crit" : used > 0.9 ? " full" : ""), style: { gridTemplateColumns: "1fr" } }, h("div", { class: "bar" }, h("i", { style: { width: used * 100 + "%" } }))),
        h("div", { class: "dim", style: { fontSize: "12px" } }, `${fmtBytes(d.free)} free of ${fmtBytes(d.total)} · click to scan`));
    }),
    h("button", { class: "dcard", disabled: st.running, onclick: async () => { const p = await invoke("pick", { folder: true, title: "Scan a folder" }); if (p) startScan(p); } },
      h("div", { class: "row" }, I("folder", "lg")), h("div", { class: "big" }, "A folder…"), h("div", { class: "dim", style: { fontSize: "12px" } }, "Scan just one folder"))));

  if (st.running) {
    el.append(h("div", { class: "card row", style: { marginTop: "16px" } },
      h("div", { class: "grow" },
        h("div", { style: { fontWeight: 700 } }, `Scanning ${st.target}…`),
        h("div", { class: "dim num" }, `${(st.files || 0).toLocaleString()} files · ${fmtBytes(st.bytes)} · ${Math.round(st.elapsed || 0)} s`)),
      h("button", { class: "btn", onclick: () => invoke("disk_cancel") }, "Stop")));
    return el;
  }
  const v = S.cache.diskView;
  if (!st.root || !v) {
    el.append(h("div", { class: "empty", style: { marginTop: "20px" } }, "Pick a drive to see which folders take the most space. It reads the whole drive, which takes from a few seconds to a couple of minutes."));
    return el;
  }
  el.append(h("div", { class: "section" },
    h("div", { class: "crumbs grow" }, v.crumbs.map((c, i) => [i ? I("chevron", "sm") : null, h("button", { onclick: () => openNode(c.id) }, c.name)])),
    h("span", { class: "dim num" }, `${fmtBytes(v.size)} · ${v.files.toLocaleString()} files · scanned in ${st.took?.toFixed(1)} s`),
    h("button", { class: "icon-btn sm", title: "Scan again", onclick: () => startScan(st.root) }, I("refresh", "sm"))));
  // treemap
  const tm = h("div", { class: "treemap" });
  el.append(tm);
  requestAnimationFrame(() => {
    const W = tm.clientWidth, H = tm.clientHeight;
    const items = v.items.slice(0, 60).map((it, i) => ({ ...it, idx: i }));
    const rects = squarify(items, 0, 0, W, H);
    for (const r of rects) {
      const color = r.dir ? TM_COLORS[r.idx % TM_COLORS.length] : "#4a5266";
      const cell = h("div", { class: "cell", title: `${r.name}\n${fmtBytes(r.size)}`, style: { left: r.x + "px", top: r.y + "px", width: Math.max(0, r.w) + "px", height: Math.max(0, r.h) + "px", background: `linear-gradient(145deg, ${color}, ${color}bb)` },
        onclick: () => r.dir ? openNode(r.id) : invoke("reveal", { path: r.path }) },
        r.w > 60 && r.h > 34 ? [h("b", null, r.name), h("span", null, fmtBytes(r.size))] : null);
      tm.append(cell);
    }
  });
  // list
  const max = v.items[0]?.size || 1;
  el.append(h("div", { class: "section" }, h("h2", null, "In this folder")));
  el.append(h("div", { class: "list" }, v.items.slice(0, 80).map((it) =>
    h("div", { class: "item", ondblclick: () => it.dir ? openNode(it.id) : invoke("open_path", { path: it.path, keep: true }) },
      h("div", { class: "ic" }, I(it.dir ? "folder" : "file")),
      h("div", { class: "grow", style: { minWidth: 0 } },
        h("div", { class: "t ell" }, it.name),
        h("div", { class: "s" }, it.dir ? `${it.files.toLocaleString()} files` : "file")),
      h("div", { class: "sizebar" }, h("div", { class: "bar" }, h("i", { style: { width: (it.size / max) * 100 + "%" } }))),
      h("div", { class: "num", style: { width: "74px", textAlign: "right", fontWeight: 700 } }, fmtBytes(it.size)),
      h("div", { class: "acts" },
        it.dir ? h("button", { class: "icon-btn sm", title: "Open", onclick: () => openNode(it.id) }, I("chevron", "sm")) : null,
        h("button", { class: "icon-btn sm", title: "Show in Explorer", onclick: () => invoke("reveal", { path: it.path }) }, I("folder", "sm")),
        it.keep ? h("span", { class: "icon-btn sm", title: "Part of Windows: Deck won't move it to the Recycle Bin", "aria-label": "Protected" }, I("lock", "sm"))
          : h("button", { class: "icon-btn sm danger", title: "Move to Recycle Bin", onclick: () => recycleItem(it) }, I("trash", "sm")))))));
  const big = S.cache.diskBig || [];
  if (big.length) {
    el.append(h("div", { class: "section" }, h("h2", null, "Biggest files on this scan")));
    el.append(h("div", { class: "list" }, big.slice(0, 25).map((f) =>
      h("div", { class: "item" },
        appIcon("file", f.path, f.name),
        h("div", { class: "grow", style: { minWidth: 0 } }, h("div", { class: "t ell" }, f.name), h("div", { class: "s ell" }, f.path)),
        h("div", { class: "num", style: { fontWeight: 700 } }, fmtBytes(f.size)),
        h("div", { class: "acts" },
          h("button", { class: "icon-btn sm", title: "Show in Explorer", onclick: () => invoke("reveal", { path: f.path }) }, I("folder", "sm")),
          f.keep ? h("span", { class: "icon-btn sm", title: "Part of Windows: Deck won't move it to the Recycle Bin", "aria-label": "Protected" }, I("lock", "sm"))
            : h("button", { class: "icon-btn sm danger", title: "Move to Recycle Bin", onclick: () => recycleItem({ ...f, dir: false }) }, I("trash", "sm")))))));
  }
  return el;
}

async function startScan(root) {
  S.cache.diskView = null;
  await invoke("disk_scan", { root });
  S.cache.diskStatus = await invoke("disk_status");
  render();
}

async function openNode(id) {
  S.cache.diskNode = id;
  S.cache.diskView = await invoke("disk_children", { id });
  render();
  document.querySelector(".content").scrollTo({ top: 0 });
}

async function recycleItem(it) {
  const ok = await confirmBox(`Move "${it.name}" to the Recycle Bin?`, `${fmtBytes(it.size)}${it.dir ? ` · ${it.files.toLocaleString()} files` : ""}. You can restore it from the Recycle Bin. If it's too big for the bin, Windows asks before deleting it for good.`, "Move to Recycle Bin", true);
  if (!ok) return;
  await run("recycle", { paths: [it.path], sizes: [it.size] }, "Moved to the Recycle Bin");
  S.cache.diskView = await invoke("disk_children", { id: S.cache.diskNode || 0 });
  S.cache.diskBig = await invoke("disk_big");
  render();
}

/** Clear RAM (from MoboCtl): trims idle memory and drops the standby cache. Needs admin. */
function clearRamCard() {
  const m = S.cache.ram;
  if (!m) invoke("hw_mem").then((r) => { S.cache.ram = r; if (S.page === "cpu") render(); }).catch(() => {});
  return h("div", { class: "card row", style: { marginBottom: "12px" } }, I("activity"),
    h("div", { class: "grow" }, h("b", null, "Memory", helpTip("cpu.ram")),
      h("div", { class: "dim small" }, m ? `${m.used.toFixed(1)} of ${m.total.toFixed(1)} GB in use · ${m.cached.toFixed(1)} GB standby cache · ${m.free.toFixed(1)} GB free` : "…")),
    h("button", { class: "btn", disabled: !S.state.admin, title: S.state.admin ? "Trims what programs aren't using and empties the standby cache" : "Needs Deck to run as admin", onclick: async (e) => {
      e.currentTarget.disabled = true;
      try { const r = await invoke("hw_clear_ram"); S.cache.ram = r; note(r.msg); } catch (err) { note(String(err), true); }
      render();
    } }, I("broom"), "Clear RAM now"));
}

function cleanupView() {
  const c = S.cache.cleanup || { targets: [], recycle: {} };
  if (!S.cache.cleanSel) S.cache.cleanSel = new Set(["temp", "shader", "crash", "chrome", "edge", "brave", "opera", "firefox", "discord", "thumbs"]);
  const sel = S.cache.cleanSel;
  // your own deleted files: never ticked for you, never called "safe"
  const rows = [{ id: "recycle", name: "Recycle Bin", desc: `${c.recycle?.items || 0} items you deleted earlier. Emptying removes them for good.`, level: "yours", size: c.recycle?.size || 0 }, ...c.targets];
  const total = rows.filter((t) => sel.has(t.id)).reduce((a, t) => a + (t.size || 0), 0);
  const el = h("div", null);
  el.append(h("div", { class: "row", style: { marginBottom: "12px" } },
    h("div", { class: "dim grow" }, c.busy ? "Measuring…" : "Only caches and leftovers that apps rebuild on their own. Files that are in use are skipped."),
    h("button", { class: "btn ghost", disabled: c.busy, onclick: () => invoke("cleanup_scan") }, I("refresh"), "Measure again"),
    h("button", { class: "btn primary lg", disabled: c.busy || !total, onclick: async () => {
      const bin = sel.has("recycle") && (c.recycle?.items || 0) > 0;
      const msg = "The selected caches and leftovers will be deleted. Old installers go to the Recycle Bin." + (bin ? ` The Recycle Bin is emptied too: its ${c.recycle.items} items are gone for good.` : "");
      if (await confirmBox(`Clean ${fmtBytes(total)}?`, msg, "Clean", bin)) invoke("cleanup_run", { ids: [...sel] });
    } }, I("broom"), c.busy ? "Working…" : `Clean ${fmtBytes(total)}`)));
  if (c.last) el.append(h("div", { class: "chip good", style: { marginBottom: "10px" } }, I("check"), `Last clean freed ${fmtBytes(c.last[0])}`));
  const group = (title, lv) => {
    const list = rows.filter((t) => t.level === lv).sort((a, b) => (b.size || 0) - (a.size || 0));
    if (!list.length) return null;
    const biggest = Math.max(1, ...rows.map((t) => t.size || 0));
    return h("div", null, h("div", { class: "section" }, h("h2", null, title)),
      h("div", { class: "col", style: { gap: "8px" } }, list.map((t) => {
        const cb = h("input", { type: "checkbox", checked: sel.has(t.id), onchange: (e) => { e.target.checked ? sel.add(t.id) : sel.delete(t.id); render(); } });
        return h("label", { class: "clean", style: { cursor: "pointer" } }, cb,
          h("div", null, h("div", { class: "n" }, t.name, t.running ? h("span", { class: "chip warn", style: { marginLeft: "8px", height: "20px" } }, `${(t.close_first || "").replace(/\.exe$/i, "")} is open: some files will be skipped`) : null),
            h("div", { class: "d" }, t.desc),
            h("i", { class: "clean-bar" + (sel.has(t.id) ? " on" : "") }, h("b", { style: { width: Math.max(1, ((t.size || 0) / biggest) * 100) + "%" } }))),
          h("div", { class: "sz num" }, t.size == null ? "…" : fmtBytes(t.size)));
      })));
  };
  el.append(group("Safe to clean", "safe"), group("Costs time later (re-downloads or rebuilds)", "careful"), group("Your deleted files (gone for good)", "yours"));
  return el;
}

function indexView() {
  const s = S.state.settings;
  const info = S.cache.indexInfo || {};
  return h("div", null,
    h("div", { class: "settings-card" },
      h("div", { class: "setting" },
        h("div", { class: "grow" }, h("div", { class: "t" }, "Find files from the search box"),
          h("div", { class: "d" }, info.building ? "Reading your folders…" : info.count != null ? `${info.count.toLocaleString()} files and folders, read in ${info.took?.toFixed(1)} s. Refreshes every 30 minutes.` : "Not built yet.")),
        toggle(s.index_on, (v) => setSetting("index_on", v).then(render)))),
    h("div", { class: "section" }, h("h2", null, "Folders search looks in"), h("span", { class: "grow" }),
      h("button", { class: "btn sm", onclick: async () => { const p = await invoke("pick", { folder: true, title: "Add a folder to search" }); if (p && !s.index_roots.includes(p)) { await setSetting("index_roots", [...s.index_roots, p]); render(); } } }, I("plus", "sm"), "Add folder"),
      h("button", { class: "btn sm ghost", onclick: async () => { await invoke("index_rebuild"); note("Re-reading folders"); } }, I("refresh", "sm"), "Refresh now")),
    h("div", { class: "list" }, s.index_roots.map((r) =>
      h("div", { class: "item" }, h("div", { class: "ic" }, I("folder")), h("div", { class: "grow t ell" }, r),
        h("button", { class: "icon-btn sm danger", title: "Remove", onclick: async () => { await setSetting("index_roots", s.index_roots.filter((x) => x !== r)); render(); } }, I("x", "sm"))))),
    h("div", { class: "faint", style: { marginTop: "10px", fontSize: "12px" } }, "Program folders, node_modules, .git, build output and AppData are skipped to keep results useful."));
}

// ------------------------------------------------------------------ power

Pages.power = {
  async load() {
    S.cache.procs = await invoke("processes");
  },
  render() {
    const p = S.live?.power || {};
    const sub = S.sub || "awake";
    const el = h("div", { class: "page-narrow" }, pageHead("power", sub));
    if (sub === "timers") {
      el.append(h("div", { class: "two" }, timersCard(p), stopwatchCard(p)));
      el.append(breaksCard());
    } else if (sub === "plan") {
      el.append(h("div", { class: "pw-cols" }, p.plan ? planNow(p.plan) : planBuilder(), planSide(p)));
    } else {
      el.append(h("div", { class: "pw-cols" }, awakeMain(p), awakeSide(p)));
    }
    return el;
  },
  live: true,
};

// ------------------------------------------------------------------ keep awake

const AWAKE_TIMES = [[30, "30 min"], [60, "1 hour"], [120, "2 hours"], [240, "4 hours"]];

function awakeMain(p) {
  const a = p.awake || { mode: "off", display: true };
  const on = !!p.awake_active;
  const now = Date.now() / 1000;
  const left = a.mode === "until" ? Math.max(0, a.until - now) : 0;
  const set = (args) => invoke("set_awake", { display: a.display, ...args });

  // what is happening now
  let say = "Your PC sleeps as usual", detail = "Pick how long below and it starts right away. Deck doesn't change your power plan; it only asks Windows not to sleep.";
  let big = null, bar = null;
  if (on) {
    say = "Staying awake";
    if (a.mode === "until") {
      big = fmtDur(left);
      detail = `Sleeps normally again at ${new Date(a.until * 1000).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}.`;
      const total = Math.max(left, S.cache.awakeTotal || left);
      S.cache.awakeTotal = total;
      bar = h("div", { class: "pw-bar" }, h("i", { style: { width: `${Math.min(100, (left / total) * 100)}%` } }));
    } else if (a.mode === "app") {
      detail = `While ${a.app.replace(/\.exe$/i, "")} is running. It ends by itself when the app closes.`;
    } else if (a.mode === "downloads") {
      detail = a.dl_seen ? "Downloading. It ends by itself a minute after downloads finish." : "Waiting for a download to start (browser or Steam). It ends by itself a minute after it finishes.";
    } else detail = "Until you turn it off.";
    detail += a.display ? " The screen stays on too." : " The screen can still turn off.";
  }

  const hero = h("div", { class: "card pw-hero" + (on ? " on" : "") },
    h("div", { class: "pw-hero-top" },
      h("span", { class: "pw-hero-ic" }, I("coffee", "lg")),
      h("div", { class: "grow" },
        h("div", { class: "boost-say" }, say),
        h("div", { class: "dim pw-detail" }, detail)),
      on ? h("div", { class: "row", style: { gap: "8px" } },
        a.mode === "until" ? h("button", { class: "btn", onclick: () => set({ mode: "until", minutes: Math.ceil(left / 60) + 30 }).then(() => (S.cache.awakeTotal = 0)) }, I("plus", "sm"), "30 min") : null,
        h("button", { class: "btn lg", onclick: () => set({ mode: "off" }) }, I("x"), "Turn off"))
        : null),
    big ? h("div", { class: "pw-big num" }, big) : null,
    bar);

  // how long: one click starts (or switches) it
  const opt = (sel, ic, label, sub2, fn) => h("button", { class: "pw-opt" + (on && sel ? " on" : ""), "aria-pressed": on && sel ? "true" : "false", onclick: fn },
    h("span", { class: "pw-opt-ic" }, I(ic, "sm")), h("span", { class: "grow" }, h("span", { class: "pw-opt-t" }, label), sub2 ? h("span", { class: "pw-opt-s" }, sub2) : null));
  const mins = h("input", { type: "number", min: 1, max: 1440, placeholder: "90", "aria-label": "Minutes", class: "pw-min" });
  const custom = () => { const m = Math.round(+mins.value); if (!(m > 0)) return note("Type a number of minutes", true); S.cache.awakeTotal = 0; set({ mode: "until", minutes: Math.min(m, 1440) }); };
  mins.addEventListener("keydown", (e) => e.key === "Enter" && custom());
  const apps = (S.cache.procs || []).filter((x) => x.app && !x.protected);
  const appSel = h("select", { "aria-label": "App", class: "grow", onchange: (e) => e.target.value && set({ mode: "app", target: e.target.value }) },
    h("option", { value: "" }, a.mode === "app" && on ? a.app.replace(/\.exe$/i, "") : "Choose a running app…"),
    apps.map((x) => h("option", { value: x.name }, `${x.title ? x.title.slice(0, 40) : x.name} (${x.name})`)));

  const how = h("div", { class: "card col pw-how" },
    h("div", { class: "boost-sub" }, on ? "Change how long" : "How long"),
    h("div", { class: "pw-opts" },
      opt(a.mode === "on", "infinity", "Until I turn it off", null, () => set({ mode: "on" })),
      AWAKE_TIMES.map(([m, label]) => opt(a.mode === "until" && Math.abs(left - m * 60) < 60 && (S.cache.awakeTotal || 0) <= m * 60 + 60, "timer", label, null, () => { S.cache.awakeTotal = 0; set({ mode: "until", minutes: m }); })),
      opt(a.mode === "downloads", "download", "Until downloads finish", "Browser and Steam", () => set({ mode: "downloads" }))),
    h("div", { class: "pw-row" }, h("span", { class: "pw-lbl" }, "For"), mins, h("span", { class: "dim" }, "minutes"), h("button", { class: "btn sm", onclick: custom }, "Start")),
    h("div", { class: "pw-row" }, h("span", { class: "pw-lbl" }, "While"), appSel),
    h("label", { class: "pw-row pw-screen" }, toggle(a.display, (v) => set({ mode: a.mode, display: v })),
      h("span", { class: "grow" }, h("b", null, "Keep the screen on too"), h("span", { class: "dim pw-hint" }, "Turn this off for downloads and renders: the screen can sleep, the PC keeps working."))));
  return h("div", { class: "col pw-main" }, hero, how);
}

function winTimeoutText(s) {
  if (s == null) return "unknown";
  if (s === 0) return "never";
  return fmtWords(s);
}

function awakeSide(p) {
  const t = p.win_timeouts;
  return h("div", { class: "col pw-side" },
    h("div", { class: "card col pw-info" },
      h("div", { class: "boost-sub" }, "Without Deck"),
      t ? h("div", { class: "pw-facts" },
        h("div", null, h("span", { class: "dim" }, "PC sleeps after"), h("b", { class: "num" }, winTimeoutText(t.sleep))),
        h("div", null, h("span", { class: "dim" }, "Screen turns off after"), h("b", { class: "num" }, winTimeoutText(t.screen))))
        : h("div", { class: "dim" }, "Couldn't read Windows' sleep settings."),
      h("div", { class: "faint pw-hint" }, "From your active power plan. Keep awake overrides these while it is on; nothing is changed."),
      h("button", { class: "btn sm ghost", onclick: () => invoke("open_url", { url: "ms-settings:powersleep" }) }, I("gear", "sm"), "Windows sleep settings")),
    h("div", { class: "card col pw-info" },
      h("div", { class: "boost-sub" }, "Goes well with"),
      sideLink("power", "Shut down when downloads finish", "Instead of keeping it awake all night", () => go("power", "plan")),
      sideLink("timer", "Timers & breaks", "A reminder when it's time to stop", () => go("power", "timers"))));
}

function sideLink(ic, title, sub, fn) {
  return h("button", { class: "pw-link", onclick: fn }, h("span", { class: "pw-opt-ic" }, I(ic, "sm")),
    h("span", { class: "grow" }, h("span", { class: "pw-opt-t" }, title), h("span", { class: "pw-opt-s" }, sub)), I("chevron", "sm"));
}

// ------------------------------------------------------------------ shut down when…

const PLAN_ACTIONS = [["shutdown", "power", "Shut down"], ["sleep", "moon", "Sleep"], ["restart", "refresh", "Restart"], ["hibernate", "moon", "Hibernate"], ["lock", "lock", "Lock"], ["signout", "log-out", "Sign out"], ["screen_off", "monitor", "Screen off"], ["notify", "bell", "Just tell me"]];

/** The conditions, each with what it measures right now. */
function planConds() {
  const L = S.live || {};
  const sys = L.sys || {};
  const cur = [L.media?.current, ...(L.media?.others || [])].find((m) => m && m.status === "playing");
  const playing = !!cur;
  return [
    ["downloads", "download", "Downloads finish", "Browser and Steam", sys.down != null ? `${fmtRate(sys.down)} down now` : ""],
    ["in", "timer", "After a while", "In a number of minutes", ""],
    ["time", "clock", "At a time", "Today or tonight", ""],
    ["app_closes", "x", "An app closes", "A game, an export, an install", ""],
    ["media_stops", "play", "Music or video stops", "Waits until something has played", playing ? `Playing: ${cur.title || cur.app || "something"}` : "Nothing playing now"],
    ["cpu_idle", "cpu", "The PC goes idle", "Renders, installs, builds", sys.cpu != null ? `CPU ${Math.round(sys.cpu)}% now` : ""],
    ["net_idle", "wifi", "The network goes quiet", "Uploads, syncing", sys.down != null ? `${fmtRate(sys.down)} now` : ""],
    ["idle", "user", "I've been away", "No mouse or keyboard", ""],
  ];
}

function planBuilder() {
  const f = S.cache.planForm || (S.cache.planForm = { action: "shutdown", cond: "downloads", minutes: 5, at: "", inMin: 30, app: "" });
  const card = h("div", { class: "card col pw-build" });
  card.append(h("div", { class: "boost-sub" }, "Do this"),
    h("div", { class: "pw-acts", role: "group", "aria-label": "Action" }, PLAN_ACTIONS.map(([k, ic, label]) =>
      h("button", { class: "pw-act" + (f.action === k ? " on" : ""), "aria-pressed": f.action === k ? "true" : "false", onclick: () => { f.action = k; render(); } }, I(ic, "sm"), label))));
  card.append(h("div", { class: "boost-sub", style: { marginTop: "6px" } }, "When"),
    h("div", { class: "pw-conds", role: "group", "aria-label": "Condition" }, planConds().map(([k, ic, label, sub, now]) =>
      h("button", { class: "pw-cond" + (f.cond === k ? " on" : ""), "aria-pressed": f.cond === k ? "true" : "false", onclick: () => { f.cond = k; render(); } },
        h("span", { class: "pw-opt-ic" }, I(ic, "sm")),
        h("span", { class: "grow" }, h("span", { class: "pw-opt-t" }, label), h("span", { class: "pw-opt-s" }, now || sub))))));
  // the details the chosen condition needs
  const extra = [];
  const num = (val, set, label) => h("input", { type: "number", min: 1, max: 1440, value: val, "aria-label": label, class: "pw-min", oninput: (e) => set(+e.target.value) });
  if (f.cond === "in") extra.push(h("span", { class: "pw-lbl" }, "In"), num(f.inMin, (v) => { f.inMin = v; refreshSum(); }, "Minutes"), h("span", { class: "dim" }, "minutes"));
  if (f.cond === "time") extra.push(h("span", { class: "pw-lbl" }, "At"), h("input", { type: "time", value: f.at || defaultTime(), "aria-label": "Time", oninput: (e) => { f.at = e.target.value; refreshSum(); } }));
  if (f.cond === "app_closes") {
    const apps = (S.cache.procs || []).filter((x) => x.app && !x.protected);
    extra.push(h("span", { class: "pw-lbl" }, "App"), h("select", { class: "grow", "aria-label": "App", onchange: (e) => { f.app = e.target.value; refreshSum(); } }, h("option", { value: "" }, "Choose a running app…"), apps.map((x) => h("option", { value: x.name, selected: f.app === x.name }, `${x.title ? x.title.slice(0, 40) : x.name} (${x.name})`))));
  }
  if (["cpu_idle", "net_idle", "idle", "downloads"].includes(f.cond)) extra.push(h("span", { class: "pw-lbl" }, f.cond === "idle" ? "Away for" : f.cond === "downloads" ? "After the last one, wait" : "Quiet for"), num(f.minutes, (v) => { f.minutes = v; refreshSum(); }, "Minutes"), h("span", { class: "dim" }, "minutes"));
  if (extra.length) card.append(h("div", { class: "pw-row" }, extra));
  const act = PLAN_ACTIONS.find(([k]) => k === f.action);
  const cond = planConds().find(([k]) => k === f.cond);
  card.append(h("div", { class: "pw-go" },
    h("div", { class: "grow" },
      h("div", { class: "pw-sum" }, planSentence(f, act[2])),
      h("div", { class: "faint pw-hint" }, f.action === "notify" ? "You get a notification; nothing else happens." : "A 60-second warning with Cancel appears first.")),
    h("button", { class: "btn primary lg", onclick: startPlan }, I("check"), "Start")));
  return card;
}

/** The plan that is set: what it waits for, how far along, and Cancel. */
function planNow(plan) {
  const now = Date.now() / 1000;
  const counting = plan.state === "countdown";
  let big = null, bar = null;
  if (counting) big = `${Math.max(0, Math.ceil(plan.countdown_until - now))} s`;
  else if (plan.cond === "time") {
    const left = Math.max(0, plan.at - now);
    big = fmtDur(left);
    const total = Math.max(1, plan.at - (plan.created || plan.at - left));
    bar = h("div", { class: "pw-bar" }, h("i", { style: { width: `${Math.min(100, (1 - left / total) * 100)}%` } }));
  }
  const act = PLAN_ACTIONS.find(([k]) => k === plan.action) || PLAN_ACTIONS[0];
  return h("div", { class: "col pw-main" },
    h("div", { class: "card pw-hero on" + (counting ? " warn" : "") },
      h("div", { class: "pw-hero-top" },
        h("span", { class: "pw-hero-ic" }, I(act[1], "lg")),
        h("div", { class: "grow" },
          h("div", { class: "boost-say" }, counting ? `${act[2]} in a moment` : planLabel(plan)),
          h("div", { class: "dim pw-detail" }, counting ? "The condition was met." : plan.progress || planCond(plan))),
        h("div", { class: "row", style: { gap: "8px" } },
          counting && plan.action !== "notify" ? h("button", { class: "btn", onclick: () => invoke("toast_action", { id: "plan", action: "now" }) }, "Do it now") : null,
          h("button", { class: "btn lg", onclick: () => invoke("cancel_plan") }, I("x"), "Cancel"))),
      big ? h("div", { class: "pw-big num" }, big) : null,
      bar,
      h("div", { class: "faint pw-hint" }, plan.action === "notify" ? "You get a notification; nothing else happens." : "You get 60 seconds to cancel before anything happens.")));
}

function planSide(p) {
  const at = (min) => Math.floor(Date.now() / 1000) + min * 60;
  const go1 = (plan) => run("set_plan", { plan: { at: 0, app: "", minutes: 2, ...plan() } }, "Set. Deck will warn you first.");
  const pick = (ic, label, sub, plan) => h("button", { class: "pw-link", onclick: () => go1(plan) },
    h("span", { class: "pw-opt-ic" }, I(ic, "sm")), h("span", { class: "grow" }, h("span", { class: "pw-opt-t" }, label), h("span", { class: "pw-opt-s" }, sub)));
  return h("div", { class: "col pw-side" },
    h("div", { class: "card col pw-info" },
      h("div", { class: "boost-sub" }, p.plan ? "Quick picks (replace the current one)" : "Quick picks"),
      pick("download", "Shut down when downloads finish", "2 quiet minutes after the last one", () => ({ action: "shutdown", cond: "downloads" })),
      pick("play", "Sleep when music stops", "After it has been quiet a minute", () => ({ action: "sleep", cond: "media_stops" })),
      pick("user", "Sleep when I'm away 20 min", "No mouse or keyboard", () => ({ action: "sleep", cond: "idle", minutes: 20 })),
      pick("moon", "Sleep in 30 min", "", () => ({ action: "sleep", cond: "time", at: at(30) })),
      pick("moon", "Sleep in 1 hour", "", () => ({ action: "sleep", cond: "time", at: at(60) })),
      pick("power", "Shut down in 1 hour", "", () => ({ action: "shutdown", cond: "time", at: at(60) }))),
    h("div", { class: "card col pw-info" },
      h("div", { class: "boost-sub" }, "Good to know"),
      h("div", { class: "dim pw-hint" }, "If the PC is asleep or off when a set time passes, Deck skips it instead of acting the next time it wakes."),
      h("div", { class: "dim pw-hint" }, "You can also type it in search: \"sleep in 30m\", \"shutdown in 2h\".")));
}

/** "Shut down when downloads finish", "Sleep in 30 min", "Lock at 23:00"... for the form. */
function planSentence(f, label) {
  const m = Math.max(1, f.minutes || 5);
  const tail = {
    downloads: "when downloads finish",
    in: `in ${fmtWords(Math.max(1, f.inMin || 30) * 60)}`,
    time: `at ${f.at || defaultTime()}`,
    app_closes: f.app ? `when ${f.app.replace(/\.exe$/i, "")} closes` : "when an app closes",
    media_stops: "when music or video stops",
    cpu_idle: `when the PC has been idle ${m} min`,
    net_idle: `when the network has been quiet ${m} min`,
    idle: `when I've been away ${m} min`,
  }[f.cond] || "";
  return f.action === "notify" ? `Tell me ${tail}` : `${label} ${tail}`;
}

function refreshSum() {
  const f = S.cache.planForm, el = document.querySelector(".pw-sum");
  if (f && el) el.textContent = planSentence(f, (PLAN_ACTIONS.find(([k]) => k === f.action) || PLAN_ACTIONS[0])[2]);
}

function defaultTime() {
  const d = new Date(Date.now() + 60 * 60 * 1000);
  return `${String(d.getHours()).padStart(2, "0")}:00`;
}

async function startPlan() {
  const f = S.cache.planForm;
  const plan = { action: f.action, cond: f.cond, at: 0, app: f.app || "", minutes: Math.max(1, f.minutes || 5) };
  if (f.cond === "in") { plan.cond = "time"; plan.at = Math.floor(Date.now() / 1000) + Math.max(1, f.inMin || 30) * 60; }
  if (f.cond === "time") {
    const [hh, mm] = (f.at || defaultTime()).split(":").map(Number);
    const d = new Date(); d.setHours(hh, mm, 0, 0);
    if (d.getTime() < Date.now()) d.setDate(d.getDate() + 1);
    plan.at = Math.floor(d.getTime() / 1000);
  }
  if (f.cond === "app_closes" && !plan.app) return note("Choose an app first", true);
  await run("set_plan", { plan }, "Set. Deck will warn you first.");
}

function timersCard(p) {
  const input = h("input", { type: "text", placeholder: "e.g. 25m, 1h30, 90s", style: { width: "150px" } });
  const label = h("input", { type: "text", placeholder: "Label (optional)", class: "grow" });
  const start = () => { const secs = parseDuration(input.value); if (!secs) return note("Type a time like 10m or 1:30", true); invoke("timer_add", { secs, label: label.value }); input.value = ""; label.value = ""; };
  input.addEventListener("keydown", (e) => e.key === "Enter" && start());
  const running = p.timers || [];
  return h("div", { class: "card col" },
    h("div", { class: "tile-head" }, I("timer"), h("span", null, "Timers")),
    running.length ? h("div", { class: "timers" }, running.map((t) => timerRow(t, p.now))) : null,
    h("div", { class: "presets" }, [[1, "1m"], [3, "3m"], [5, "5m"], [10, "10m"], [15, "15m"], [25, "25m"], [45, "45m"], [60, "1h"]].map(([m, l]) => h("button", { class: "chip", title: `${m} minutes`, onclick: () => invoke("timer_add", { secs: m * 60, label: "" }) }, l))),
    h("div", { class: "row" }, input, label, h("button", { class: "btn", onclick: start }, I("play"), "Start")),
    running.length ? null : h("div", { class: "faint" }, "When a timer ends you get a sound and a notification, even with Deck closed."));
}

function stopwatchCard(p) {
  const sw = p.stopwatch || { running: false, acc_ms: 0, start_ms: 0, laps: [] };
  const total = sw.acc_ms + (sw.running ? p.now - sw.start_ms : 0);
  const fmt = (ms) => { const s = ms / 1000; const m = Math.floor(s / 60); return `${String(m).padStart(2, "0")}:${(s % 60).toFixed(1).padStart(4, "0")}`; };
  return h("div", { class: "card col" },
    h("div", { class: "tile-head" }, I("clock"), h("span", null, "Stopwatch")),
    h("div", { class: "bigtime", id: "sw-time" }, fmt(total)),
    h("div", { class: "row" },
      h("button", { class: "btn " + (sw.running ? "" : "primary"), onclick: () => invoke("stopwatch", { cmd: sw.running ? "stop" : "start" }) }, I(sw.running ? "pause" : "play"), sw.running ? "Stop" : "Start"),
      h("button", { class: "btn", disabled: !total, onclick: () => invoke("stopwatch", { cmd: "lap" }) }, "Lap"),
      h("button", { class: "btn ghost", disabled: !total, onclick: () => invoke("stopwatch", { cmd: "reset" }) }, "Reset")),
    sw.laps.length ? h("div", { class: "col", style: { gap: "2px", maxHeight: "120px", overflow: "auto" } }, sw.laps.map((l, i) => h("div", { class: "row dim num" }, h("span", { class: "grow" }, `Lap ${i + 1}`), h("span", null, fmt(l - (sw.laps[i - 1] || 0))), h("span", { class: "faint" }, fmt(l)))).reverse()) : null);
}

function breaksCard() {
  const s = S.state.settings;
  return h("div", { class: "settings-card", style: { marginTop: "14px" } },
    h("div", { class: "setting" },
      h("div", { class: "grow" }, h("div", { class: "t" }, "Break reminders"), h("div", { class: "d" }, "A gentle reminder to stand up or look away after a long stretch at the PC. Never pops up over a fullscreen game or video; two quiet minutes count as a break.")),
      toggle(s.breaks_on, (v) => setSetting("breaks_on", v).then(render))),
    s.breaks_on ? h("div", { class: "setting" },
      h("div", { class: "grow" }, h("div", { class: "t" }, "Remind me every")),
      h("select", { onchange: (e) => setSetting("breaks_every_min", +e.target.value) }, [20, 30, 45, 50, 60, 90, 120].map((m) => h("option", { value: m, selected: s.breaks_every_min === m }, `${m} minutes`)))) : null);
}

// ----------------------------------------------------------------- system

Pages.system = {
  async load() {
    const sub = S.sub || "processes";
    if (sub === "processes") S.cache.procs = await invoke("processes");
    if (sub === "startup") S.cache.startup = await invoke("startup_list");
    if (sub === "network") {
      S.cache.net = await invoke("net_info");
      invoke("net_public_ip").then((ip) => { S.cache.publicIp = ip; if (S.page === "system") render(); });
      if (!S.cache.gpGames) S.cache.gpGames = await invoke("game_ping_games").catch(() => []);
      if (!S.cache.gpSel) S.cache.gpSel = S.cache.gpGames[0]?.id;
      S.cache.pingLive = false; // live pinging runs only after Start, and stops when you leave the page
      pingNow().then(() => S.page === "system" && S.sub === "network" && render());
    }
    if (sub === "display") S.cache.display = await invoke("display_state");
  },
  render() {
    const sub = S.sub || "processes";
    const el = h("div", null, pageHead("system", sub));
    if (sub === "startup") el.append(startupView());
    else if (sub === "network") el.append(networkView());
    else if (sub === "display") el.append(displayView());
    else el.append(procView());
    return el;
  },
  live: true,
  async tick() {
    if ((S.sub || "processes") === "processes" && !S.interacting) { S.cache.procs = await invoke("processes"); return true; }
    if (S.sub === "network") { S.cache.net = await invoke("net_info"); netTick(); return true; }
    return false;
  },
};

function procView() {
  const all = S.cache.procs || [];
  const sort = S.cache.procSort || "mem";
  const appsOnly = S.cache.procApps !== false;
  const q = (S.cache.procQ || "").toLowerCase();
  let list = all.filter((p) => (!appsOnly || p.app) && (!q || p.name.toLowerCase().includes(q) || (p.title || "").toLowerCase().includes(q)));
  list = [...list].sort((a, b) => sort === "cpu" ? b.cpu - a.cpu : sort === "name" ? a.name.localeCompare(b.name) : b.mem - a.mem);
  const box = h("input", { type: "search", placeholder: "Filter…", value: S.cache.procQ || "", style: { width: "200px" } });
  box.addEventListener("input", debounce(() => { S.cache.procQ = box.value; render(); const b = document.querySelector('input[type=search]'); b && (b.focus(), b.setSelectionRange(b.value.length, b.value.length)); }, 200));
  const th = (key, label, cls) => h("th", { class: (cls || "") + " sortable" + (sort === key ? " on" : ""), title: "Sort by " + label.toLowerCase(), onclick: () => { S.cache.procSort = key; render(); } }, label, sort === key ? " ↓" : "");
  const topCpu = Math.max(1, ...list.map((p) => p.cpu));
  const ramTotal = S.live?.sys?.ram_total || Math.max(1, ...list.map((p) => p.mem));
  const use = (txt, frac, cls) => h("div", { class: "use " + cls }, h("span", { class: "num" }, txt), h("i", { class: "use-bar" }, h("b", { style: { width: Math.max(2, Math.min(100, frac * 100)) + "%" } })));
  return h("div", null,
    h("div", { class: "row", style: { marginBottom: "10px" } }, box, h("label", { class: "row" }, toggle(appsOnly, (v) => { S.cache.procApps = v; render(); }), h("span", null, "Apps with windows only")), h("span", { class: "grow" }),
      h("span", { class: "dim" }, list.length === all.length ? `${all.length} programs` : `${list.length} of ${all.length} programs`), h("button", { class: "btn ghost", onclick: () => invoke("open_system", { what: "taskmgr" }) }, I("external"), "Task Manager")),
    h("table", { class: "grid" },
      h("thead", null, h("tr", null, th("name", "Program"), th("cpu", "CPU", "r"), th("mem", "Memory", "r"), h("th", { class: "r" }, ""))),
      h("tbody", null, list.slice(0, 150).map((p) => h("tr", null,
        h("td", null, h("div", { class: "row", style: { gap: "10px" } }, appIcon("file", p.exe, p.name, "", ), h("div", { style: { minWidth: 0 } },
          h("div", { class: "ell", style: { fontWeight: 600, maxWidth: "420px" } }, p.title || p.name),
          h("div", { class: "faint", style: { fontSize: "11.5px" } }, [p.name, p.count > 1 ? `${p.count} processes` : null, p.hung ? "NOT RESPONDING" : null].filter(Boolean).join(" · "))))),
        h("td", { class: "r" }, use(p.cpu.toFixed(1) + "%", p.cpu / topCpu, "cpu")),
        h("td", { class: "r" }, use(fmtBytes(p.mem), p.mem / ramTotal, "mem")),
        h("td", { class: "r" }, p.protected ? h("span", { class: "faint", title: "Part of Windows" }, I("lock", "sm")) :
          h("button", { class: "btn sm" + (p.hung ? " danger" : " ghost"), onclick: async () => { if (await confirmBox(`End ${p.name}?`, `${p.count} process${p.count > 1 ? "es" : ""}. Unsaved work in it will be lost.`, "End it", true)) { await run("kill", { name: p.name }, `Ended ${p.name}`); S.cache.procs = await invoke("processes"); render(); } } }, "End")))))));
}

function startupView() {
  const list = [...(S.cache.startup || [])].sort((a, b) => (b.enabled - a.enabled) || a.name.localeCompare(b.name));
  const on = list.filter((e) => e.enabled).length;
  return h("div", null,
    h("div", { class: "row", style: { marginBottom: "12px" } },
      h("b", null, `${on} of ${list.length} start with Windows`), h("span", { class: "grow" }),
      h("span", { class: "faint small" }, "Same switch as Task Manager. Nothing is deleted. A lock means it needs administrator rights.")),
    list.length ? h("div", { class: "col", style: { gap: "8px" } }, list.map((e) =>
      h("div", { class: "win" + (e.enabled ? "" : " off"), style: { gridTemplateColumns: "30px 1fr auto" } },
        appIcon("file", e.exe, e.name),
        h("div", { style: { minWidth: 0 } }, h("div", { class: "t ell" }, e.name), h("div", { class: "s ell", title: e.command }, `${e.source} · ${e.command}`)),
        e.can_change ? toggle(e.enabled, async (v) => { await run("startup_set", { id: e.id, on: v }, v ? `${e.name} will start with Windows` : `${e.name} won't start with Windows`); S.cache.startup = await invoke("startup_list"); render(); })
          : h("span", { class: "chip", title: "Needs administrator rights" }, I("lock"), e.enabled ? "On" : "Off")))) : h("div", { class: "empty" }, "Nothing starts with Windows."));
}

const NET_HIST = 60;   // seconds of download / upload kept for the graph
const PING_KEEP = 60;  // rounds per host in the latency strip
const NET_UP = "#bf5af2";

/** The graph's history: starts from the backend's recent samples, so it isn't empty when the
 * page opens, and never joins points from an earlier visit to new ones. */
function netHist() {
  if (!S.cache.netHist || Date.now() - (S.cache.netHistAt || 0) > 3000) {
    const s = S.live?.sys || {};
    const d = s.down_hist || [], u = s.up_hist || [];
    S.cache.netHist = d.map((v, i) => ({ down: v || 0, up: u[i - (d.length - u.length)] || 0 }));
    S.cache.netHistAt = Date.now();
  }
  return S.cache.netHist;
}

/** Called once a second while the Network page is open: graph history, and a ping round while live pinging is on. */
function netTick() {
  const s = S.live?.sys || {};
  const hist = netHist();
  hist.push({ down: s.down || 0, up: s.up || 0 });
  S.cache.netHistAt = Date.now();
  if (hist.length > NET_HIST) hist.splice(0, hist.length - NET_HIST);
  if (S.cache.pingLive) pingNow();
}

async function pingNow() {
  const hosts = S.state.settings.ping_hosts || [];
  if (S.cache.pinging || !hosts.length) return;
  S.cache.pinging = true;
  try {
    const res = await invoke("net_ping", { hosts });
    const hist = (S.cache.pingHist = S.cache.pingHist || {});
    for (const r of res) {
      const q = (hist[r.host] = hist[r.host] || []);
      q.push(r.ms == null ? null : r.ms);
      if (q.length > PING_KEEP) q.splice(0, q.length - PING_KEEP);
    }
    S.cache.ping = res;
  } catch { /* the next round tries again */ }
  S.cache.pinging = false;
}

const pingTone = (ms) => (ms == null ? "bad" : ms < 40 ? "good" : ms < 90 ? "warn" : "bad");
const gameTone = (ms) => (ms == null ? "bad" : ms < 60 ? "good" : ms < 110 ? "warn" : "bad");

function copyChip(label, value) {
  return h("button", { class: "net-addr", title: value ? "Copy" : "", disabled: !value, onclick: async () => { await navigator.clipboard.writeText(value); note(`Copied ${value}`); } },
    h("span", { class: "faint" }, label), h("span", { class: "num" }, value || "…"), value ? I("copy", "sm") : null);
}

function networkView() {
  const n = S.cache.net || {};
  return h("div", { class: "net" },
    connectionCard(n),
    h("div", { class: "net-cols" },
      h("div", { class: "col", style: { gap: "14px" } }, speedCard(n.speed || {}), pingCard()),
      gamePingView()));
}

function connectionCard(n) {
  const s = S.live?.sys || {};
  const g = h("canvas", { class: "net-graph" });
  requestAnimationFrame(() => {
    const hist = netHist();
    const mb = (v) => (netBytes() ? v / 1024 ** 2 : (v * 8) / 1e6); // graph in the chosen unit
    const top = Math.max(5, mb(s.down || 0), mb(s.up || 0), ...hist.map((x) => Math.max(mb(x.down), mb(x.up))));
    const step = top <= 5 ? 1 : top <= 20 ? 5 : top <= 100 ? 25 : top <= 500 ? 100 : 250;
    const acc = getComputedStyle(document.documentElement).getPropertyValue("--acc").trim();
    lineGraph(g, [{ values: hist.map((x) => mb(x.down)), color: acc }, { values: hist.map((x) => mb(x.up)), color: NET_UP }],
      { min: 0, max: Math.ceil(top / step) * step, n: NET_HIST });
  });
  return h("div", { class: "card net-conn" },
    h("div", { class: "net-rates" },
      h("div", null, h("div", { class: "net-k" }, h("i", { class: "net-key", style: { background: "var(--acc)" } }), "Download"), h("div", { class: "net-v num" }, fmtRate(s.down))),
      h("div", null, h("div", { class: "net-k" }, h("i", { class: "net-key", style: { background: NET_UP } }), "Upload"), h("div", { class: "net-v num" }, fmtRate(s.up)))),
    h("div", { class: "net-graph-wrap" }, g, h("span", { class: "net-graph-cap faint" }, (netBytes() ? "MB/s" : "Mb/s") + " · last minute")),
    h("div", { class: "net-addrs" },
      copyChip("This PC", n.lan),
      copyChip("Internet", S.cache.publicIp),
      n.host ? h("span", { class: "faint net-host" }, n.host) : null));
}

function speedCard(sp) {
  const phase = sp.phase || "idle";
  const running = ["latency", "download", "upload"].includes(phase);
  const metric = (key, label, val, unit, extra) => h("div", { class: "net-m" + (running && phase === key ? " live" : "") },
    h("div", { class: "net-k" }, label),
    h("div", { class: "net-big num" }, ...(unit === "Mb/s" ? ((p) => [p[0], h("span", { class: "dim" }, " " + p[1])])(mbpsParts(val)) : [val != null ? val.toFixed(0) : "–", h("span", { class: "dim" }, " " + unit)]), extra ? h("span", { class: "faint net-extra" }, extra) : null),
    running && phase === key ? h("div", { class: "bar" }, h("i", { style: { width: (sp.progress || 0) * 100 + "%" } })) : null);
  const when = running ? `Measuring ${phase === "latency" ? "ping" : phase}…` : sp.at ? `Tested ${ago(sp.at)} · Cloudflare` : "About 100 MB of data · Cloudflare";
  return h("div", { class: "card col" },
    h("div", { class: "tile-head" }, I("zap"), h("span", null, "Speed test"), h("span", { class: "grow" }),
      h("span", { class: "net-when" }, when),
      running ? h("button", { class: "btn sm", onclick: () => { invoke("net_speed_cancel"); note("Speed test cancelled"); } }, I("x", "sm"), "Cancel")
        : h("button", { class: "btn sm" + (sp.at ? "" : " primary"), onclick: () => invoke("net_speed") }, sp.at ? "Run again" : "Run")),
    h("div", { class: "net-ms" },
      metric("download", "Download", sp.down_mbps, "Mb/s"),
      metric("upload", "Upload", sp.up_mbps, "Mb/s"),
      metric("latency", "Ping", sp.ping_ms, "ms", sp.jitter_ms != null ? `± ${sp.jitter_ms.toFixed(1)}` : null)),
    sp.error ? h("div", { class: "chip bad", style: { alignSelf: "start" } }, sp.error) : null);
}

function pingCard() {
  const hosts = S.state.settings.ping_hosts || [];
  const res = S.cache.ping || [];
  const hist = S.cache.pingHist || {};
  const live = !!S.cache.pingLive;
  const add = h("input", { type: "text", class: "net-add", placeholder: "Add an address or name, e.g. riotgames.com", spellcheck: false });
  add.addEventListener("keydown", async (e) => {
    const v = add.value.trim();
    if (e.key !== "Enter" || !v) return;
    if (hosts.includes(v)) { note(`${v} is already in the list`); return; }
    await setSetting("ping_hosts", [...hosts, v]);
    add.value = "";
    add.blur();
    render();
    pingNow().then(() => S.sub === "network" && render());
  });
  const strip = (q) => {
    const top = Math.max(60, ...q.filter((x) => x != null));
    return h("div", { class: "pstrip" }, Array.from({ length: PING_KEEP }, (_, i) => {
      const v = q[i - (PING_KEEP - q.length)];
      if (i < PING_KEEP - q.length) return h("i", { class: "none" });
      return h("i", { class: v == null ? "lost" : pingTone(v), style: { height: v == null ? "100%" : Math.max(12, (v / top) * 100) + "%" }, title: v == null ? "No reply" : `${v.toFixed(0)} ms` });
    }));
  };
  return h("div", { class: "card col net-ping" },
    h("div", { class: "tile-head" }, I("activity"), h("span", null, "Ping"), h("span", { class: "grow" }),
      h("span", { class: "net-when" + (live ? " live" : "") }, live ? "Live" : S.cache.ping ? "Stopped" : ""),
      hosts.length ? h("button", { class: "btn sm" + (live ? "" : " primary"), "aria-pressed": live ? "true" : "false",
        onclick: () => { S.cache.pingLive = !live; if (!live) { pingNow().then(() => S.page === "system" && S.sub === "network" && render()); } render(); } },
        I(live ? "pause" : "play", "sm"), live ? "Stop" : "Start") : null),
    hosts.length ? h("div", { class: "prows" }, hosts.map((host) => {
      const r = res.find((x) => x.host === host);
      const q = hist[host] || [];
      const ok = q.filter((x) => x != null);
      const avg = ok.length ? ok.reduce((a, b) => a + b, 0) / ok.length : null;
      const lost = q.length ? q.filter((x) => x == null).length / q.length : 0;
      return h("div", { class: "prow" },
        h("div", { class: "ell prow-host", title: host }, host),
        strip(q),
        h("div", { class: "prow-v" },
          r ? h("span", { class: "num tone-" + pingTone(r.ms) }, r.ms == null ? "No reply" : `${r.ms.toFixed(0)} ms`) : h("span", { class: "faint" }, "…"),
          avg != null ? h("span", { class: "faint num" }, `avg ${avg.toFixed(0)}${lost > 0 ? ` · ${Math.round(lost * 100)}% lost` : ""}`) : null),
        h("button", { class: "icon-btn sm prow-x", title: `Remove ${host}`, onclick: async () => { await setSetting("ping_hosts", hosts.filter((x) => x !== host)); render(); } }, I("x", "sm")));
    })) : h("div", { class: "faint" }, "Add an address to watch its ping."),
    add);
}

const GP_NOTE = {
  valve: "Valve's relay servers per city; matchmaking connects you through these.",
  aws: "UDP to Amazon's game ping servers, the way the game measures it.",
};

async function gpTest(g) {
  S.cache.gp = S.cache.gp || {};
  const res = S.cache.gp[g.id] || {};
  if (res.running) return;
  S.cache.gp[g.id] = { ...res, running: true, error: null };
  if (S.sub === "network") render();
  try { S.cache.gp[g.id] = { rows: await invoke("game_ping", { game: g.id }), at: Date.now() }; }
  catch (e) { S.cache.gp[g.id] = { ...res, running: false, error: String(e) }; }
  if (S.page === "system" && S.sub === "network") render();
}

function gamePingView() {
  const games = S.cache.gpGames || [];
  if (!games.length) return h("div");
  const sel = games.find((g) => g.id === S.cache.gpSel) || games[0];
  S.cache.gp = S.cache.gp || {};
  const res = S.cache.gp[sel.id] || {};
  // the first look at a game tests it (about 3 seconds); after that only on request
  if (!res.rows && !res.running && !res.error) setTimeout(() => gpTest(sel), 0);
  const rows = res.rows || [];
  const shown = S.cache.gpAll ? rows : rows.slice(0, 9);
  const best = rows[0]?.ms;
  return h("div", { class: "card col net-gp" },
    h("div", { class: "tile-head" }, I("gamepad"), h("span", null, "Game servers"), h("span", { class: "grow" }),
      res.at && !res.running ? h("span", { class: "net-when" }, `Tested ${ago(res.at)}`) : null,
      h("button", { class: "btn sm", disabled: res.running, onclick: () => gpTest(sel) }, I("refresh", "sm"), res.running ? "Testing…" : "Test again")),
    h("div", { class: "gp-games" }, games.map((g) =>
      h("button", { class: "chip" + (g.id === sel.id ? " on" : "") + (g.installed ? "" : " gp-off"), title: g.installed ? "Installed on this PC" : "Not installed", onclick: () => { S.cache.gpSel = g.id; S.cache.gpAll = false; render(); } },
        g.name))),
    h("div", { class: "faint small" }, GP_NOTE[sel.kind]),
    res.error ? h("div", { class: "chip bad", style: { alignSelf: "start" } }, res.error) : null,
    rows.length ? h("div", { class: "gp-rows" + (res.running ? " busy" : "") }, shown.map((r, i) => h("div", { class: "gp-row" + (i === 0 && r.ms != null ? " best" : "") },
      h("div", { class: "ell" }, h("span", { class: "gp-name" }, r.name), r.sub ? h("span", { class: "faint small" }, "  " + r.sub) : null),
      h("div", { class: "bar gp-bar " + gameTone(r.ms) }, h("i", { style: { width: r.ms == null ? "0" : Math.max(3, Math.min(100, (r.ms / Math.max(150, best * 4 || 150)) * 100)) + "%" } })),
      h("span", { class: "num gp-ms tone-" + gameTone(r.ms) }, r.ms == null ? "No reply" : `${r.ms.toFixed(0)} ms`, r.loss > 0.01 ? h("span", { class: "faint" }, ` · ${Math.round(r.loss * 100)}% lost`) : null))))
      : res.error ? null : h("div", { class: "gp-rows gp-wait" }, Array.from({ length: 6 }, () => h("div", { class: "gp-row skel" }, h("i"), h("i"), h("i")))),
    rows.length > 9 ? h("button", { class: "btn sm ghost", style: { alignSelf: "start" }, onclick: () => { S.cache.gpAll = !S.cache.gpAll; render(); } }, S.cache.gpAll ? "Show fewer" : `Show all ${rows.length}`) : null);
}

function displayView() {
  const d = S.cache.display || { monitors: [] };
  const s = S.state.settings;
  const el = h("div", null);
  for (const m of d.monitors) {
    const br = h("span", { class: "dim num", style: { width: "40px", textAlign: "right" } }, m.brightness != null ? m.brightness + "%" : "");
    el.append(h("div", { class: "card col", style: { marginBottom: "12px" } },
      h("div", { class: "row" }, I("monitor", "lg"), h("div", { class: "grow" }, h("div", { style: { fontWeight: 700, fontSize: "15px" } }, m.name, m.primary ? h("span", { class: "chip", style: { marginLeft: "8px", height: "20px" } }, "Main") : null), h("div", { class: "dim" }, `${m.width} × ${m.height} · ${m.hz} Hz`))),
      m.rates.length > 1 ? h("div", null, h("div", { class: "label", style: { marginBottom: "6px" } }, "Refresh rate"),
        h("div", { class: "row wrap", style: { gap: "6px" } }, m.rates.map((r) => h("button", { class: "chip" + (r === m.hz ? " on" : ""), onclick: async () => { await run("set_refresh", { device: m.device, hz: r }, `${r} Hz`); S.cache.display = await invoke("display_state"); render(); } }, `${r} Hz`))),
        m.hz < Math.max(...m.rates) ? h("div", { class: "row hz-hint" }, I("alert", "sm"), h("span", { class: "dim" }, `Running at ${m.hz} Hz. This monitor can do ${Math.max(...m.rates)} Hz.`),
          h("button", { class: "btn sm", onclick: async () => { const top = Math.max(...m.rates); await run("set_refresh", { device: m.device, hz: top }, `${top} Hz`); S.cache.display = await invoke("display_state"); render(); } }, `Use ${Math.max(...m.rates)} Hz`)) : null) : null,
      m.brightness != null ? h("div", null, h("div", { class: "label" }, "Brightness"),
        h("div", { class: "row" }, I("sun", "sm"), slider(m.brightness, 0, 100, 1, (v) => { br.textContent = v + "%"; }, (v) => run("set_brightness", { device: m.device, pct: v })), br)) : h("div", { class: "faint", style: { fontSize: "12px" } }, "Brightness: this monitor doesn't allow it from Windows (turn on DDC/CI in its menu)."),
      m.hdr_supported ? h("label", { class: "row" }, toggle(m.hdr_on, async (v) => { await run("set_hdr", { device: m.device, on: v }, v ? "HDR on" : "HDR off"); S.cache.display = await invoke("display_state"); render(); }), h("span", null, "HDR")) : null));
  }
  el.append(h("div", { class: "section" }, h("h2", null, "Light & dark mode")));
  el.append(h("div", { class: "settings-card" },
    h("div", { class: "setting" }, h("div", { class: "grow" }, h("div", { class: "t" }, "Windows theme")),
      seg([["dark", "Dark"], ["light", "Light"]], d.dark ? "dark" : "light", async (v) => { await run("set_dark", { dark: v === "dark" }); S.cache.dark = v === "dark"; S.cache.display = await invoke("display_state"); render(); })),
    h("div", { class: "setting" }, h("div", { class: "grow" }, h("div", { class: "t" }, "Switch automatically"),
      h("div", { class: "d" }, d.schedule ? `Light at ${d.schedule.light}, dark at ${d.schedule.dark}${s.theme_mode === "sun" && s.place ? ` (${s.place})` : ""}` : s.theme_mode === "sun" ? "Needs your location for sunrise and sunset." : "")),
      toggle(s.theme_auto, (v) => setSetting("theme_auto", v).then(render))),
    s.theme_auto ? h("div", { class: "setting" }, h("div", { class: "grow" }, h("div", { class: "t" }, "When")),
      seg([["times", "Set times"], ["sun", "Sunrise & sunset"]], s.theme_mode, async (v) => { await setSetting("theme_mode", v); S.cache.display = await invoke("display_state"); render(); })) : null,
    s.theme_auto && s.theme_mode === "times" ? h("div", { class: "setting" }, h("div", { class: "grow" }, h("div", { class: "t" }, "Times")),
      h("span", { class: "dim" }, "Light"), h("input", { type: "time", value: s.light_at, onchange: (e) => setSetting("light_at", e.target.value) }),
      h("span", { class: "dim" }, "Dark"), h("input", { type: "time", value: s.dark_at, onchange: (e) => setSetting("dark_at", e.target.value) })) : null,
    s.theme_auto && s.theme_mode === "sun" ? h("div", { class: "setting" }, h("div", { class: "grow" }, h("div", { class: "t" }, "Location"), h("div", { class: "d" }, s.place || "Not set")),
      h("button", { class: "btn", onclick: async () => { const r = await run("locate"); S.state = await invoke("get_state"); S.cache.display = await invoke("display_state"); note(`Location: ${r.place}`); render(); } }, I("target"), "Use my location")) : null));
  return el;
}
