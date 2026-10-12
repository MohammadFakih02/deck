// Pages: Tools, Phone, Settings.

// ------------------------------------------------------------------ tools

Pages.tools = {
  async load() {},
  render() {
    const s = S.state.settings;
    const sub = S.sub || "screen";
    const el = h("div", null, tabsHead("tools", "Tools", [["screen", "Screen tools"], ["locks", "Locked files"], ["panels", "Windows panels"]], sub));
    if (sub === "locks") return el.append(locksView()), el;
    if (sub === "panels") return el.append(panelsView()), el;
    const big = (ic, title, text, fn) => h("button", { class: "dcard", onclick: fn },
      h("div", { class: "row" }, h("div", { class: "ic", style: { width: "40px", height: "40px", borderRadius: "11px", background: "var(--acc-soft)", color: "var(--acc)", display: "grid", placeItems: "center" } }, I(ic, "lg"))),
      h("div", { class: "big", style: { fontSize: "15px" } }, title), h("div", { class: "dim", style: { fontSize: "12.5px" } }, text));
    el.append(h("div", { class: "three" },
      big("droplet", "Pick a colour", "Click anywhere on screen; the colour code is copied.", () => tool("color")),
      big("type", "Copy text from the screen", "Drag over text in a picture, video or game and it's copied as text.", () => tool("text")),
      big("ruler", "Measure", "Drag a box to measure it in pixels.", () => tool("measure"))));

    // colours
    if ((s.colors || []).length) {
      el.append(h("div", { class: "section" }, h("h2", null, "Colours you picked"), h("span", { class: "grow" }),
        h("button", { class: "btn sm ghost", onclick: () => setSetting("colors", []).then(render) }, "Clear")));
      el.append(h("div", { class: "swatches" }, s.colors.map((c) => {
        const r = parseInt(c.slice(1, 3), 16), g = parseInt(c.slice(3, 5), 16), b = parseInt(c.slice(5, 7), 16);
        return h("button", { class: "swatch", title: `${c}  ·  rgb(${r}, ${g}, ${b})\nClick: copy hex · Right-click: copy rgb`,
          onclick: () => copyAnswer(c), oncontextmenu: (e) => { e.preventDefault(); copyAnswer(`rgb(${r}, ${g}, ${b})`); } },
          h("i", { style: { background: c } }), h("span", null, c));
      })));
    }
    return el;
  },
};

function locksView() {
  const el = h("div", null);
  el.append(h("div", { class: "row", style: { marginBottom: "12px" } },
    h("button", { class: "btn primary", onclick: () => lockCheck(false) }, I("file"), "Pick a file"),
    h("button", { class: "btn", onclick: () => lockCheck(true) }, I("folder"), "Pick a folder")));
  const lk = S.cache.lock;
  el.append(h("div", { class: "card" }, !lk ? h("div", { class: "dim" }, "\"Can't delete, the file is open in another program\"? Pick the file or folder and Deck shows which program is holding it.")
    : lk.holders.length === 0 ? h("div", null, h("b", null, lk.path), h("div", { class: "dim" }, "Nothing is using it right now. You can move or delete it."))
      : h("div", { class: "col" }, h("div", null, h("b", null, lk.path), h("div", { class: "dim" }, `In use by ${lk.holders.length} program${lk.holders.length > 1 ? "s" : ""}:`)),
        h("div", { class: "list" }, lk.holders.map((x) => h("div", { class: "item" }, appIcon("file", x.exe, x.name), h("div", { class: "grow" }, h("div", { class: "t" }, x.name), h("div", { class: "s" }, `${x.exe || ""} · PID ${x.pid}`)),
          h("button", { class: "btn sm danger", onclick: async () => { if (await confirmBox(`End ${x.name}?`, "Unsaved work in it will be lost.", "End it", true)) { await invoke("kill_pid", { pid: x.pid }); lockCheck(null, lk.path); } } }, "End it")))))));
  return el;
}

function panelsView() {
  const el = h("div", null);
  const sys = (ic, label, what, url) => h("button", { class: "qa", onclick: () => url ? invoke("open_url", { url }) : invoke("open_system", { what }) }, I(ic), h("span", null, label));
  el.append(h("div", { class: "quick", style: { gridTemplateColumns: "repeat(5, 1fr)" } },
    sys("activity", "Task Manager", "taskmgr"), sys("volume", "Sound devices", "sound"), sys("wifi", "Network adapters", "network"),
    sys("power", "Power options", "power"), sys("trash", "Uninstall programs", "programs"), sys("terminal", "Environment variables", "envvars"),
    sys("terminal", "Terminal", "terminal"), sys("download", "Downloads", "downloads"), sys("image", "Snipping Tool", null, "ms-screenclip:"), sys("gear", "Windows Settings", null, "ms-settings:")));
  return el;
}

async function lockCheck(folder, path) {
  const p = path || await invoke("pick", { folder, title: folder ? "Which folder?" : "Which file?" });
  if (!p) return;
  try {
    const holders = await invoke("who_uses", { path: p });
    S.cache.lock = { path: p, holders };
  } catch (e) { note(String(e), true); }
  render();
}

// ------------------------------------------------------------------ phone

Pages.phone = {
  async load() {
    S.cache.remote = await invoke("remote_info");
  },
  render() {
    const r = S.cache.remote || {};
    const s = S.state.settings;
    const el = h("div", null, head("Phone", "Control your PC from your phone (media, volume, power, timers) and pass text, links and files between them."));
    const left = h("div", { class: "card col", style: { alignItems: "center", textAlign: "center", gap: "12px" } },
      h("label", { class: "row", style: { alignSelf: "stretch", justifyContent: "space-between" } },
        h("b", null, "Allow phone connection"),
        toggle(s.remote_on, async (v) => { await setSetting("remote_on", v); setTimeout(async () => { S.cache.remote = await invoke("remote_info"); render(); }, 400); })),
      r.enabled && r.qr ? h("div", { class: "qr", html: r.qr }) : h("div", { class: "empty" }, r.error || "The phone connection is off."),
      r.enabled ? h("div", { class: "dim" }, "Scan with your phone's camera. Phone and PC must be on the same Wi-Fi.") : null,
      // while it's off there's nothing to connect to or re-code
      r.enabled ? h("div", { class: "row" }, h("span", { class: "chip " + (r.clients ? "good" : "") }, I("smartphone"), r.clients ? `${r.clients} phone${r.clients > 1 ? "s" : ""} connected` : "No phone connected")) : null,
      !r.enabled ? null : h("button", { class: "btn sm ghost", title: "Old QR codes stop working", onclick: async () => { if (await confirmBox("Make a new code?", "Phones using the old code are disconnected and need to scan again.", "New code")) { S.cache.remote = await invoke("remote_reset_token"); render(); } } }, I("refresh", "sm"), "Make a new code"));
    const install = h("details", { class: "card col fold-card", style: { gap: "8px" } },
      h("summary", { style: { fontWeight: 700, cursor: "pointer" } }, "Put it in your app drawer (Android)"),
      h("ol", { class: "dim", style: { margin: 0, paddingLeft: "18px", lineHeight: 1.7 } },
        h("li", null, "In Chrome on the phone open ", h("b", null, "chrome://flags")),
        h("li", null, "Find ", h("b", null, "Insecure origins treated as secure"), ", add ", h("b", { style: { userSelect: "text" } }, r.origin || "the address"), ", Enabled, Relaunch"),
        h("li", null, "Scan the code, then ", h("b", null, "⋮ → Install app"))),
      h("div", { class: "faint", style: { fontSize: "12px" } }, "Chrome only offers Install for secure pages, and this page is plain HTTP on your home network; that is what the flag is for."));
    // send box
    const ta = h("textarea", { rows: 3, placeholder: "Text or a link to send to your phone…", style: { width: "100%" } });
    const send = h("div", { class: "card col" },
      h("div", { class: "tile-head" }, I("send"), h("span", null, "Send to your phone")),
      ta,
      h("div", { class: "row wrap" },
        h("button", { class: "btn primary", onclick: async () => { if (!ta.value.trim()) return; await run("share_to_phone", { kind: "text", text: ta.value }, "Sent"); ta.value = ""; S.cache.remote = await invoke("remote_info"); render(); } }, I("send"), "Send text"),
        h("button", { class: "btn", onclick: async () => { await run("share_to_phone", { kind: "file" }); S.cache.remote = await invoke("remote_info"); render(); } }, I("upload"), "Send a file"),
        h("span", { class: "grow" }),
        h("button", { class: "btn ghost", onclick: () => invoke("open_system", { what: "inbox" }) }, I("folder"), "Files from phone")));
    const shared = [...(r.shared || [])].reverse();
    const inbox = h("div", { class: "card col" },
      h("div", { class: "tile-head" }, I("swap"), h("span", null, "Passed between phone and PC")),
      shared.length ? h("div", { class: "list" }, shared.map((x) => h("div", { class: "item" },
        h("div", { class: "ic" }, I(x.kind === "file" ? "file" : x.kind === "link" ? "link" : "note")),
        h("div", { class: "grow", style: { minWidth: 0 } },
          h("div", { class: "t ell" }, x.kind === "file" ? x.name : x.text),
          h("div", { class: "s" }, `${x.from === "phone" ? "From phone" : "To phone"} · ${ago(x.ts)}${x.kind === "file" ? " · " + fmtBytes(x.size) : ""}`)),
        h("div", { class: "row", style: { gap: "2px" } },
          x.kind === "file" && x.from === "phone" ? h("button", { class: "icon-btn sm", title: "Show in folder", onclick: () => invoke("reveal", { path: x.path }) }, I("folder", "sm")) : null,
          x.kind === "link" ? h("button", { class: "icon-btn sm", title: "Open", onclick: () => invoke("open_url", { url: x.text, keep: true }) }, I("external", "sm")) : null,
          x.kind !== "file" ? h("button", { class: "icon-btn sm", title: "Copy", onclick: () => copyAnswer(x.text) }, I("copy", "sm")) : null,
          h("button", { class: "icon-btn sm danger", title: "Remove from list", onclick: async () => { await invoke("shared_remove", { id: x.id }); S.cache.remote = await invoke("remote_info"); render(); } }, I("x", "sm")))))) : h("div", { class: "faint" }, "Nothing yet. Things you send from the phone are copied to the PC clipboard; files land in Downloads\\From phone."));
    el.append(h("div", { style: { display: "grid", gridTemplateColumns: "300px minmax(0, 1fr)", gap: "14px", alignItems: "start" } },
      h("div", { class: "col", style: { gap: "14px" } }, left, install),
      h("div", { class: "col", style: { gap: "14px" } }, send, inbox)));
    return el;
  },
  live: true,
  async tick() {
    const r = await invoke("remote_info");
    const changed = JSON.stringify(r) !== JSON.stringify(S.cache.remote);
    S.cache.remote = r;
    return changed && !S.interacting && document.activeElement?.tagName !== "TEXTAREA";
  },
};

// --------------------------------------------------------------- settings

const ACCENTS = ["#4f8cff", "#7b61ff", "#b35cff", "#ff4f9a", "#ff6b4f", "#ffb020", "#2fbf71", "#1fb5c9"];
const ENGINES = [["https://www.google.com/search?q=", "Google"], ["https://duckduckgo.com/?q=", "DuckDuckGo"], ["https://www.bing.com/search?q=", "Bing"], ["https://search.brave.com/search?q=", "Brave"], ["https://www.youtube.com/results?search_query=", "YouTube"]];

Pages.settings = {
  async load() {
    S.cache.indexInfo = await invoke("index_info");
  },
  render() {
    const s = S.state.settings;
    const row = settingRow;
    const set = (k) => (v) => setSetting(k, v).then(() => { if (k === "accent") applyAccent(); render(); });
    const tab = S.cache.setTab || "general";
    const tabs = seg([["general", "General"], ["home", "Home"], ["clip", "Clipboard & search"], ["profiles", "Profiles"], ["about", "About"]], tab, (v) => { S.cache.setTab = v; render(); });
    const el = h("div", { class: "page-narrow" }, head("Settings", null, tabs));
    let cur = "general";
    const add = (...nodes) => { if (cur === tab) el.append(...nodes); };

    cur = "general";
    add(h("div", { class: "section" }, h("h2", null, "Opening Deck")));
    const panel = s.mode !== "app";
    const modeCard = (id, icon, title, desc) => h("button", { class: "mode-card" + (s.mode === id ? " on" : ""), onclick: () => s.mode !== id && set("mode")(id) },
      I(icon), h("div", null, h("b", null, title), h("div", { class: "dim small" }, desc)));
    add(h("div", { class: "mode-cards" },
      modeCard("panel", "zap", "Quick panel", "Pops up when you rest the mouse in a corner or press your shortcut, and gets out of the way when you click somewhere else."),
      modeCard("app", "windows", "App window", "A normal window with a taskbar button that stays open until you close it. Closing it keeps Deck running in the tray.")));
    add(h("div", { class: "settings-card" },
      row("Keyboard shortcut", panel ? "Opens and closes the panel." : "Brings the window to the front (or minimizes it when it already is).",
        hotkeyField(s.open_hotkey, "open", (k) => set("open_hotkey")(k))),
      panel ? row("Hot corner", "Rest the mouse in this corner of the screen to open or close Deck.",
        seg([["off", "Off"], ["top-left", "↖"], ["top-right", "↗"], ["bottom-left", "↙"], ["bottom-right", "↘"]], s.hot_corner, set("hot_corner"))) : null,
      panel && s.hot_corner !== "off" ? row("How long to rest there", `${s.hot_corner_ms} ms. Longer means fewer accidental opens.`,
        h("div", { style: { width: "200px" } }, slider(s.hot_corner_ms, 0, 1000, 50, null, set("hot_corner_ms")))) : null,
      panel && s.hot_corner !== "off" ? row(h("span", null, "Also over fullscreen games and videos", helpTip("set.fullscreen")), "Off: the corner does nothing while a game or video is fullscreen, so you can't open Deck by accident mid-match.", toggle(s.hot_corner_fullscreen, set("hot_corner_fullscreen"))) : null,
      panel ? row("Hide when I click somewhere else", null, toggle(s.hide_on_blur, set("hide_on_blur"))) : null,
      row("Other ways in", "Click the tray icon, the Start menu entry, or the phone. Right-click the tray icon for quick tools.", null),
      row("Start with Windows", "Starts hidden in the tray.", toggle(s.autostart, set("autostart")))));

    cur = "home";
    add(h("div", { class: "section" }, h("h2", null, "Home tiles")));
    const tiles = (s.tiles || []).filter((t) => TILE_INFO[t]); // a removed tile (Notes) may still be saved
    const allTiles = Object.keys(TILE_INFO);
    const ordered = [...tiles, ...allTiles.filter((t) => !tiles.includes(t))];
    add(h("div", { class: "settings-card" }, ordered.map((t, i) => {
      const on = tiles.includes(t);
      return h("div", { class: "setting", style: { padding: "9px 16px" } },
        h("div", { class: "grow t", style: { opacity: on ? 1 : 0.55 } }, TILE_INFO[t].name),
        on ? h("button", { class: "icon-btn sm", title: "Move up", disabled: tiles.indexOf(t) === 0, onclick: () => { const a = [...tiles]; const k = a.indexOf(t); [a[k - 1], a[k]] = [a[k], a[k - 1]]; set("tiles")(a); } }, I("moveup", "sm")) : null,
        toggle(on, (v) => set("tiles")(v ? [...tiles, t] : tiles.filter((x) => x !== t))));
    })));

    cur = "general";
    add(h("div", { class: "section" }, h("h2", null, "Colour")));
    add(h("div", { class: "settings-card" }, row("Accent", null,
      h("div", { class: "row", style: { gap: "8px" } }, ACCENTS.map((c) => h("button", { title: c, onclick: () => set("accent")(c), style: { width: "26px", height: "26px", borderRadius: "50%", background: c, border: s.accent === c ? "2px solid #fff" : "2px solid transparent", boxShadow: s.accent === c ? `0 0 0 3px ${c}55` : "none" } }))))));

    cur = "clip";
    add(h("div", { class: "section" }, h("h2", null, "Clipboard history")));
    add(h("div", { class: "settings-card" },
      row("Keep a clipboard history", "Stored only on this PC. Passwords copied from password managers are skipped.", toggle(s.clip_on, set("clip_on"))),
      row("Keep images too", null, toggle(s.clip_images, set("clip_images"))),
      row(h("span", null, "Clicking an item pastes it", helpTip("set.clip_paste")), "Deck closes and pastes into the app you were using. Off: it only copies.", toggle(s.clip_paste, set("clip_paste"))),
      row("How many to keep", "Pinned items don't count.", h("select", { onchange: (e) => set("clip_max")(+e.target.value) }, [100, 200, 400, 1000, 3000].map((n) => h("option", { value: n, selected: s.clip_max === n }, `${n} items`))))));

    cur = "clip";
    add(h("div", { class: "section" }, h("h2", null, "Search")));
    add(h("div", { class: "settings-card" },
      row("Web search with", null, h("select", { onchange: (e) => set("web_search")(e.target.value) }, ENGINES.map(([u, n]) => h("option", { value: u, selected: s.web_search === u }, n))))));
    add(h("div", { style: { marginTop: "10px" } }, indexView()));

    cur = "profiles";
    add(h("div", { class: "section" }, h("h2", null, "CPU and GPU profiles")));
    add(h("div", { class: "settings-card" },
      row(h("span", null, "Profile keyboard shortcuts", helpTip("set.hotkeys")), "The keys you give CPU and GPU profiles (and the overlay key). They work in games too.", toggle(s.hotkeys_on, set("hotkeys_on"))),
      row("Say when a profile switches", "A small notification, e.g. \"CPU → Eco\".", toggle(s.profile_toast, set("profile_toast"))),
      row("Repair shortcuts", "Makes every profile's desktop and Start menu shortcut again (after moving Deck or deleting one by mistake).",
        h("button", { class: "btn", onclick: async () => { const errs = await run("shortcuts_repair"); note(errs.length ? errs.join(" · ") : "Shortcuts repaired", errs.length > 0); } }, I("refresh"), "Repair"))));

    cur = "about";
    add(h("div", { class: "section" }, h("h2", null, "About")));
    add(h("div", { class: "settings-card" },
      row(`Deck ${S.state.version}`, "Dashboard, launcher, hardware control and Windows power tools in one tray app." + (S.state.admin ? " Running with admin rights." : ""), null),
      row("Quit Deck", "Stops the hot corner, clipboard history, timers and the phone connection until you start it again. Fans go back to the BIOS.", h("button", { class: "btn danger", onclick: async () => { if (await confirmBox("Quit Deck?", "Timers and shutdown plans stop too.", "Quit", true)) invoke("quit"); } }, I("power"), "Quit"))));
    return el;
  },
};

function applyAccent() {
  const c = S.state?.settings?.accent || "#4f8cff";
  document.documentElement.style.setProperty("--acc", c);
}
