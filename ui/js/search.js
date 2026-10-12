// The search box: apps, Deck's own actions, calculator, conversions, games,
// files, clipboard and the web, with arrow keys + Enter inside Deck.

/** Everything Deck can do, as searchable actions. */
function actions() {
  const out = [];
  const a = (title, sub, ic, keys, fn) => out.push({ title, sub, ic, keys: (keys || "").toLowerCase(), fn });
  // every sidebar page
  for (const [group, items] of [...NAV, [null, NAV_END]]) {
    for (const [page, sub, label, ic] of items) a(label, group ? `Open page · ${group}` : "Open page", ic, `${label} ${page} ${sub || ""} page`, () => go(page, sub));
  }
  a("Pick a colour from the screen", "Colour picker", "droplet", "color colour picker eyedropper hex", () => tool("color"));
  a("Copy text from the screen", "Reads text in images, videos, games (OCR)", "type", "ocr text extract copy screen read", () => tool("text"));
  a("Measure something on screen", "Pixel ruler", "ruler", "measure ruler pixels size", () => tool("measure"));
  a("Keep the PC awake", "Until you turn it off", "coffee", "awake caffeine sleep stay on", () => invoke("set_awake", { mode: "on" }).then(() => note("Staying awake")));
  a("Let the PC sleep normally", "Turn keep awake off", "coffee", "awake off caffeine", () => invoke("set_awake", { mode: "off" }).then(() => note("Normal sleep")));
  a("Dark mode / light mode", "Switch the Windows theme", "moon", "dark light theme mode night", toggleDark);
  a("Lock the PC", "", "lock", "lock", () => invoke("run_power", { action: "lock" }));
  a("Turn the screen off", "", "monitor", "screen off monitor display", () => invoke("run_power", { action: "screen_off" }));
  a("Sleep", "", "moon", "sleep suspend", async () => { if (await confirmBox("Sleep now?", "Your PC goes to sleep right away.", "Sleep")) invoke("run_power", { action: "sleep" }); });
  a("Restart", "", "refresh", "restart reboot", async () => { if (await confirmBox("Restart now?", "Unsaved work in other apps may be lost.", "Restart", true)) invoke("run_power", { action: "restart" }); });
  a("Shut down", "", "power", "shutdown shut down turn off", async () => { if (await confirmBox("Shut down now?", "Unsaved work in other apps may be lost.", "Shut down", true)) invoke("run_power", { action: "shutdown" }); });
  a("Shut down when downloads finish", "Browser and Steam downloads", "download", "shutdown when download finish done", () => go("power", "plan"));
  a("Clean up disk space", "Caches, temp files, old installers", "broom", "clean cleanup junk temp cache free space", () => go("files", "cleanup"));
  a("What's using my disk?", "Map of folder sizes", "disk", "disk space usage big files wiztree treesize", () => go("files"));
  a("What's locking this file?", "Find the program holding a file open", "lock", "locked file in use locksmith unlock delete", () => go("tools", "locks"));
  a("Empty the Recycle Bin", "", "trash", "recycle bin empty trash", async () => { if (await confirmBox("Empty the Recycle Bin?", "Everything in it is deleted for good.", "Empty", true)) run("cleanup_run", { ids: ["recycle"] }); });
  a("Run a speed test", "Download, upload and ping", "zap", "speed test internet bandwidth", () => { go("system", "network"); invoke("net_speed"); });
  a("Startup apps", "What starts with Windows", "rocket", "startup boot autostart", () => go("system", "startup"));
  a("Processes", "See and end running programs", "cpu", "process task kill end frozen", () => go("system", "processes"));
  a("Display settings", "Refresh rate, brightness, HDR", "monitor", "refresh rate hz brightness hdr monitor display", () => go("system", "display"));
  a("Workspaces", "Put your apps back where they belong", "layout", "workspace layout scene", () => go("windows", "workspaces"));
  a("Send to phone", "Text, a link or a file", "send", "phone send share transfer", () => go("phone"));
  a("Task Manager", "Windows", "activity", "task manager taskmgr", () => invoke("open_system", { what: "taskmgr" }));
  a("Sound control panel", "Windows", "volume", "sound control panel playback recording", () => invoke("open_system", { what: "sound" }));
  a("Network connections", "Windows", "wifi", "network adapters ncpa", () => invoke("open_system", { what: "network" }));
  a("Uninstall programs", "Windows", "trash", "uninstall programs features appwiz remove app", () => invoke("open_system", { what: "programs" }));
  a("Environment variables", "Windows", "terminal", "env environment variables path", () => invoke("open_system", { what: "envvars" }));
  a("Terminal", "Windows", "terminal", "terminal cmd powershell console", () => invoke("open_system", { what: "terminal" }));
  a("Downloads folder", "", "download", "downloads folder", () => invoke("open_system", { what: "downloads" }));
  a("Windows Settings", "", "gear", "windows settings", () => invoke("open_url", { url: "ms-settings:" }));
  a("Snipping Tool", "Screenshot", "image", "screenshot snip capture", () => invoke("open_url", { url: "ms-screenclip:" }));
  for (const o of S.cache.outputs || []) {
    if (!o.default) a(`Switch sound to ${shortDevice(o.name)}`, o.name, "headphones", "sound output audio switch speakers headset " + o.name, () => setOutput(o.id));
  }
  for (const ws of S.state.settings.workspaces || []) {
    a(`Open workspace: ${ws.name}`, `${ws.windows.length} windows`, "layout", "workspace " + ws.name, () => run("ws_open", { id: ws.id }));
  }
  return out;
}

/** Commands typed in the box: "timer 10m", "kill discord", "sleep in 30m", URLs. */
function commands(q) {
  const out = [];
  const lower = q.toLowerCase().trim();
  let m;
  if ((m = lower.match(/^(timer|t)\s+(.+)$/))) {
    const secs = parseDuration(m[2]);
    if (secs > 0) out.push({ title: `Start a ${fmtWords(secs)} timer`, sub: "Timer", ic: "timer", fn: () => invoke("timer_add", { secs, label: "" }).then(() => note(`Timer: ${fmtWords(secs)}`)) });
  }
  if ((m = lower.match(/^(shutdown|shut down|sleep|restart|lock)\s+(in|after)\s+(.+)$/))) {
    const secs = parseDuration(m[3]);
    const action = m[1].replace(" ", "");
    if (secs > 0) out.push({ title: `${ACTION_LABEL[action] || action} in ${fmtWords(secs)}`, sub: "Power plan (you can cancel it)", ic: "power", fn: () => invoke("set_plan", { plan: { action, cond: "time", at: Math.floor(Date.now() / 1000) + secs, app: "", minutes: 0 } }).then(() => note("Scheduled")) });
  }
  if ((m = lower.match(/^(awake|keep awake|caffeine|stay awake)\s+(?:for\s+)?(.+)$/))) {
    const secs = parseDuration(m[2]);
    if (secs > 0) out.push({ title: `Keep the PC awake for ${fmtWords(secs)}`, sub: "Keep awake", ic: "coffee", fn: () => invoke("set_awake", { mode: "until", minutes: Math.max(1, Math.round(secs / 60)) }).then(() => note(`Staying awake for ${fmtWords(secs)}`)) });
  }
  const plan = S.live?.power?.plan;
  if (plan && /^(cancel|stop)(\s+(the\s+)?(shutdown|shut down|sleep|restart|plan|timer))?$/.test(lower)) {
    out.push({ title: `Cancel: ${planLabel(plan)}`, sub: "Shut down when…", ic: "x", fn: () => invoke("cancel_plan").then(() => note("Cancelled")) });
  }
  if ((m = lower.match(/^(kill|close|end)\s+(.+)$/))) {
    const name = m[2].trim();
    const procs = S.cache.procs || [];
    const hits = procs.filter((p) => !p.protected && (p.name.toLowerCase().includes(name) || (p.title || "").toLowerCase().includes(name))).slice(0, 3);
    // same question as the Processes page: Enter alone must never end a program
    const end = async (exe, label, detail) => { if (await confirmBox(`End ${label}?`, `${detail}Unsaved work in it will be lost.`, "End it", true)) run("kill", { name: exe }, `Ended ${label}`); };
    for (const p of hits) out.push({ title: `End ${p.name}`, sub: `${p.count} process${p.count > 1 ? "es" : ""} · ${fmtBytes(p.mem)}`, ic: "x", fn: () => end(p.name, p.name, `${p.count} process${p.count > 1 ? "es" : ""}. `) });
    if (!hits.length) out.push({ title: `End "${name}"`, sub: "Looks for a running program with that name", ic: "x", fn: () => end(name.endsWith(".exe") ? name : name + ".exe", `"${name}"`, "") });
  }
  if (/^(https?:\/\/|www\.)\S+$/i.test(q.trim()) || /^[\w-]+(\.[\w-]+)+(\/\S*)?$/.test(q.trim()) && !/\s/.test(q)) {
    const url = /^https?:\/\//i.test(q.trim()) ? q.trim() : "https://" + q.trim();
    out.push({ title: `Open ${q.trim()}`, sub: "Website", ic: "globe", fn: () => invoke("open_url", { url }) });
  }
  return out;
}

function score(name, q) {
  const n = name.toLowerCase(), t = q.toLowerCase().trim();
  if (!t) return 0;
  if (n === t) return 1000;
  if (n.startsWith(t)) return 800 - n.length;
  const words = n.split(/[^\p{L}\p{N}]+/u).filter(Boolean);
  if (words.some((w) => w.startsWith(t))) return 650 - n.length;
  if (n.includes(t)) return 450 - n.length;
  return -1;
}

let searchSeq = 0;
/** The backend search for the newest query, while it runs. */
let searchPending = null;
async function doSearch(q) {
  S.query = q;
  const seq = ++searchSeq;
  if (!q.trim()) {
    S.results = null;
    render();
    return;
  }
  // Local results right away, backend results when they arrive.
  const acts = actions()
    .map((x) => ({ ...x, s: Math.max(score(x.title, q), x.keys.split(" ").some((k) => k && k.startsWith(q.toLowerCase().trim())) ? 500 : -1, score(x.keys, q)) }))
    .filter((x) => x.s >= 0)
    .sort((a, b) => b.s - a.s)
    .slice(0, 6);
  // keep the previous lists for a moment so they don't flicker, but never an answer that
  // belongs to the previous query
  const prev = S.results?.q && q.startsWith(S.results.q.slice(0, 1)) ? S.results.be : null;
  S.results = { q, acts, cmds: commands(q), be: prev && { ...prev, calc: null, convert: null }, fresh: false };
  S.sel = 0;
  render();
  const call = invoke("search", { q });
  searchPending = call;
  try {
    const be = await call;
    if (seq !== searchSeq) return;
    S.results.be = be;
    S.results.fresh = true;
    render();
  } catch (e) {
    console.error(e);
  } finally {
    if (searchPending === call) searchPending = null;
  }
}

/** Flat list of result rows in display order: [{group, el-builder, run}] */
function resultRows() {
  const r = S.results;
  if (!r) return [];
  const rows = [];
  const be = r.be || {};
  const push = (group, item) => rows.push({ group, ...item });
  if (be.calc) push("Calculator", { answer: true, title: be.calc.value, sub: `${r.q.trim()} =`, ic: "calc", fn: () => copyAnswer(be.calc.value.replace(/,/g, "")) });
  if (be.convert) push("Conversion", { answer: true, title: be.convert.value, sub: be.convert.detail, ic: "swap", fn: () => copyAnswer(be.convert.value) });
  for (const c of r.cmds) push("Do it", c);
  // best app first, then actions, then the other apps
  const apps = (be.apps || []).map((x) => x.app);
  const topApp = apps[0];
  const topAct = r.acts[0];
  if (topApp && (!topAct || (be.apps[0].score >= topAct.s))) {
    push("Apps", appRow(topApp));
    for (const x of apps.slice(1, 6)) push("Apps", appRow(x));
    for (const x of r.acts) push("Deck", x);
  } else {
    for (const x of r.acts) push("Deck", x);
    for (const x of apps.slice(0, 6)) push("Apps", appRow(x));
  }
  for (const g of be.games || []) push("Games", { title: g.game.name, sub: g.game.store === "steam" ? "Steam game" : "Epic game", ic: "gamepad", fn: () => invoke("open_url", { url: g.game.launch }) });
  for (const f of be.files || []) push("Files", { title: f.name, sub: f.path, file: f, fn: () => invoke("open_path", { path: f.path }), alt: () => invoke("reveal", { path: f.path }) });
  for (const c of be.clips || []) push("Clipboard", { title: c.text.replace(/\s+/g, " ").slice(0, 90), sub: "From your clipboard history", ic: "clipboard", fn: () => pasteClip(c.id) });
  const q = r.q.trim();
  push("Web", { title: `Search the web for "${q}"`, sub: "Google", ic: "globe", fn: () => invoke("open_url", { url: (S.state.settings.web_search || "https://www.google.com/search?q=") + encodeURIComponent(q) }) });
  return rows;
}

function appRow(a) {
  return { title: a.name, sub: a.store ? "Store app" : a.path ? a.path.split("\\").slice(-2, -1)[0] || "App" : "App", app: a, fn: () => run("launch_app", { id: a.id }), alt: a.path ? () => invoke("reveal", { path: a.path }) : null };
}

async function copyAnswer(text) {
  await run("set_clipboard", { text, paste: false });
  note(`Copied ${text}`);
}

function renderResults() {
  const rows = resultRows();
  const wrap = h("div", { class: "results" });
  let group = null;
  rows.forEach((r, i) => {
    if (r.group !== group && !r.answer) {
      group = r.group;
      wrap.append(h("div", { class: "rgroup" }, group));
    }
    const sel = i === S.sel;
    if (r.answer) {
      wrap.append(h("button", { class: "answer" + (sel ? " sel" : ""), onclick: r.fn, onmouseenter: () => hover(i) },
        h("div", { class: "grow" }, h("div", { class: "s" }, r.sub), h("div", { class: "big" }, r.title)),
        h("span", { class: "kbd" }, "Enter to copy")));
      return;
    }
    const lead = r.app ? appIcon("app", r.app.id, r.app.name) : r.file ? appIcon("file", r.file.path, r.file.name) : h("div", { class: "ic" }, I(r.ic || "sparkle"));
    wrap.append(h("button", { class: "item" + (sel ? " sel" : ""), "data-i": i, onclick: r.fn, onmouseenter: () => hover(i), oncontextmenu: (e) => { if (r.app) { e.preventDefault(); appMenu(r.app); } } },
      lead,
      h("div", { class: "grow" }, h("div", { class: "t ell" }, r.title), r.sub ? h("div", { class: "s ell" }, r.sub) : null),
      h("div", { class: "end" },
        r.alt ? h("span", { class: "acts" }, h("span", { class: "icon-btn sm", title: "Show in folder", onclick: (e) => { e.stopPropagation(); r.alt(); } }, I("folder", "sm"))) : null,
        sel ? h("span", { class: "kbd" }, "Enter") : null)));
  });
  return wrap;
}

function hover(i) {
  if (S.sel === i) return;
  S.sel = i;
  document.querySelectorAll(".results .sel").forEach((x) => x.classList.remove("sel"));
  const rows = document.querySelectorAll(".results .item, .results .answer");
  rows[i] && rows[i].classList.add("sel");
}

function searchKey(e) {
  const rows = resultRows();
  if (e.key === "ArrowDown" || e.key === "ArrowUp") {
    if (!rows.length) return;
    e.preventDefault();
    S.sel = (S.sel + (e.key === "ArrowDown" ? 1 : rows.length - 1)) % rows.length;
    render();
    const el = document.querySelector(".results .sel");
    el && el.scrollIntoView({ block: "nearest" });
  } else if (e.key === "Enter") {
    e.preventDefault();
    // results for what was typed last may still be on their way: wait for them, so Enter
    // never runs a result that belonged to the previous letters
    if (S.results && !S.results.fresh && searchPending) {
      const q = S.results.q;
      searchPending.catch(() => {}).finally(() => { if (S.results?.q === q) { const r = resultRows()[S.sel]; r && r.fn(); } });
      return;
    }
    const r = rows[S.sel];
    if (r) r.fn();
  } else if (e.key === "Escape") {
    e.preventDefault();
    if (S.query) { e.target.value = ""; doSearch(""); } else invoke("hide");
  }
}
