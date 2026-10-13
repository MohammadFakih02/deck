// Pages: Clipboard, Sound, Windows.

const Pages = {};

function head(title, sub, ...right) {
  return h("div", { class: "page-head" }, h("div", { class: "grow" }, h("h1", null, title), sub ? h("p", null, sub) : null), right);
}

/** Title and one-line explanation of each sidebar page. */
const HEADS = {
  "windows/open": ["Windows", "Every open window: pin it on top, make it see-through, snap it to a zone or close it."],
  "windows/layouts": ["Snap zones", "Drag a window and tap the right mouse button to drop it into a zone. Pick or make a layout here."],
  "windows/workspaces": ["Workspaces", "Save where your windows are, then put everything back in one click."],
  "files/space": ["Disk space", "See what fills your drives and send big things to the Recycle Bin."],
  "files/cleanup": ["Clean up", "Safe junk: caches, temp files, crash dumps and old installers."],
  "system/processes": ["Processes", "Running programs, grouped by app. End the ones that are stuck."],
  "system/startup": ["Startup apps", "What starts with Windows."],
  "system/network": ["Network", "Speed test, ping to game servers and your addresses."],
  "system/display": ["Display", "Refresh rate, brightness, HDR and dark mode."],
  "power/awake": ["Keep awake", "Stop the PC and the screen from going to sleep."],
  "power/plan": ["Shut down when…", "Shut down, sleep or restart when a download finishes, an app closes, music stops or at a set time."],
  "power/timers": ["Timers & breaks", "Timers, a stopwatch and reminders to take a break."],
  "tools/screen": ["Screen tools", "Pick a colour, copy text off the screen, or measure something."],
  "tools/locks": ["Locked files", "Find which program is holding a file you can't delete or move."],
  "tools/panels": ["Windows panels", "Straight to the Windows settings and tools you actually need."],
};
function pageHead(page, sub, ...right) {
  const [t, d] = HEADS[`${page}/${sub}`] || [page, ""];
  return head(t, d, ...right);
}
/** One sidebar item holding several parts: one title, the open tab's own explanation, and the tabs. */
function tabsHead(page, title, tabs, sub, ...right) {
  const [, d] = HEADS[`${page}/${sub}`] || [];
  return head(title, d, ...right, seg(tabs, sub, (v) => go(page, v)));
}

// -------------------------------------------------------------- clipboard

const TRANSFORMS = [
  ["Plain text", (t) => t],
  ["UPPERCASE", (t) => t.toUpperCase()],
  ["lowercase", (t) => t.toLowerCase()],
  ["Title Case", (t) => t.toLowerCase().replace(/(^|[\s\-_(["'])(\p{L})/gu, (m, a, b) => a + b.toUpperCase())],
  ["Trim spaces", (t) => t.split("\n").map((l) => l.trim()).join("\n").replace(/[ \t]{2,}/g, " ").trim()],
  ["One line", (t) => t.replace(/\s*\n\s*/g, " ").trim()],
  ["Clean link", (t) => t.replace(/https?:\/\/\S+/g, (u) => {
    try {
      const url = new URL(u);
      [...url.searchParams.keys()].forEach((k) => { if (/^(utm_|fbclid|gclid|si$|igshid|ref$|ref_src|mc_|_hs|yclid|spm|feature$)/i.test(k)) url.searchParams.delete(k); });
      return url.toString().replace(/\?$/, "");
    } catch { return u; }
  })],
  ["Pretty JSON", (t) => { try { return JSON.stringify(JSON.parse(t), null, 2); } catch { throw new Error("That isn't JSON"); } }],
];

const CLIP_ICON = { text: "type", link: "link", image: "image", files: "file" };
const clipKind = (c) => (c.kind === "text" && /^https?:\/\/\S+$/i.test(c.text.trim()) ? "link" : c.kind);
const CLIP_GROUPS = ["Pinned", "Today", "Yesterday", "Earlier"];
function clipGroup(c) {
  if (c.pinned) return "Pinned";
  const d = new Date();
  d.setHours(0, 0, 0, 0);
  return c.ts >= d.getTime() ? "Today" : c.ts >= d.getTime() - 86400e3 ? "Yesterday" : "Earlier";
}
const plural = (n, one, many) => `${n} ${n === 1 ? one : many || one + "s"}`;

Pages.clipboard = {
  async load() {
    S.cache.clipAll = await invoke("clip_list", { q: "", limit: 1000 });
  },
  render() {
    const all = S.cache.clipAll || [];
    const f = S.cache.clipFilter || "all";
    const q = (S.cache.clipQ || "").toLowerCase();
    const matches = (c, v) => v === "all" || (v === "pinned" ? c.pinned : c.kind === v);
    const found = all.filter((c) => matches(c, f) &&
      (!q || c.text.toLowerCase().includes(q) || c.files.some((x) => x.toLowerCase().includes(q)) || (c.app || "").toLowerCase().includes(q)));
    // newest first inside each day, pinned on top
    const items = CLIP_GROUPS.flatMap((g) => found.filter((c) => clipGroup(c) === g));
    S.cache.clipView = items;
    const sel = items.find((c) => c.id === S.cache.clipSel) || items[0];
    const box = h("input", { type: "search", placeholder: "Search history…", value: S.cache.clipQ || "", style: { width: "240px" } });
    box.addEventListener("input", debounce(() => { S.cache.clipQ = box.value; render(); setTimeout(() => { const b = document.querySelector("input[type=search]"); b && (b.focus(), b.setSelectionRange(b.value.length, b.value.length)); }, 0); }, 200));
    const s = S.state.settings;
    const list = [];
    const groups = new Set(items.map(clipGroup));
    let last = null;
    for (const c of items) {
      const g = clipGroup(c);
      if (groups.size > 1 && g !== last) list.push(h("div", { class: "clip-group" }, g));
      last = g;
      list.push(clipCard(c, sel && c.id === sel.id));
    }
    const left = h("div", { class: "clips scroll", style: { maxHeight: "calc(94vh - 250px)", overflowY: "auto", paddingRight: "4px" } },
      items.length ? list : h("div", { class: "empty" }, !s.clip_on ? "Clipboard history is off (Settings)." : all.length ? "Nothing matches." : "Nothing here yet. Copy something!"));
    const right = sel ? clipDetail(sel) : h("div");
    const el = h("div", null,
      head("Clipboard", `${plural(all.length, "item")}. Click one to look at it, double-click to ${s.clip_paste ? "paste it into the app you were using" : "copy it"}. Passwords from password managers are never kept.`, box,
        h("button", { class: "btn ghost", onclick: async () => { if (await confirmBox("Clear clipboard history?", "Pinned items are kept.", "Clear", true)) { await invoke("clip_clear"); await Pages.clipboard.load(); render(); } } }, I("trash"), "Clear")),
      h("div", { class: "row", style: { marginBottom: "14px", gap: "6px" } },
        [["all", "All"], ["text", "Text"], ["image", "Images"], ["files", "Files"], ["pinned", "Pinned"]].map(([v, l]) => {
          const n = all.filter((c) => matches(c, v)).length;
          return h("button", { class: "chip" + (f === v ? " on" : ""), onclick: () => { S.cache.clipFilter = v; render(); } }, l, n ? h("span", { class: "chip-n" }, n) : null);
        }),
        h("span", { class: "grow" }),
        items.length ? h("span", { class: "faint small" }, "↑ ↓ to move · Enter to paste · Del to remove") : null),
      h("div", { style: { display: "grid", gridTemplateColumns: "minmax(0,1.2fr) minmax(0,1fr)", gap: "16px", alignItems: "start" } }, left, right));
    requestAnimationFrame(() => document.querySelector(".clip.sel")?.scrollIntoView({ block: "nearest" }));
    return el;
  },
};

// arrow keys, Enter and Delete work on the list without clicking into it
document.addEventListener("keydown", async (e) => {
  if (S.page !== "clipboard" || S.query || !["ArrowDown", "ArrowUp", "Enter", "Delete"].includes(e.key)) return;
  if (["INPUT", "TEXTAREA", "SELECT", "BUTTON"].includes(document.activeElement?.tagName) || document.querySelector(".modal-back")) return;
  const items = S.cache.clipView || [];
  const i = Math.max(0, items.findIndex((c) => c.id === S.cache.clipSel));
  const cur = items[i];
  if (!cur) return;
  e.preventDefault();
  if (e.key === "Enter") return pasteClip(cur.id);
  if (e.key === "Delete") {
    await invoke("clip_remove", { id: cur.id });
    S.cache.clipSel = (items[i + 1] || items[i - 1])?.id;
    await Pages.clipboard.load();
    return render();
  }
  S.cache.clipSel = items[Math.min(items.length - 1, Math.max(0, i + (e.key === "ArrowDown" ? 1 : -1)))].id;
  render();
});

function clipCard(c, sel) {
  const body = c.kind === "image"
    ? h("img", { class: "thumb", src: iconSrc("clip", c.image), alt: "" })
    : c.kind === "files"
      ? h("div", { class: "txt" }, c.files.map((f) => f.split("\\").pop()).join("\n"))
      : h("div", { class: "txt" }, c.text);
  const k = clipKind(c);
  return h("button", { class: "clip" + (sel ? " sel" : ""), onclick: () => { S.cache.clipSel = c.id; render(); }, ondblclick: () => pasteClip(c.id) },
    h("span", { class: "clip-ic" }, I(CLIP_ICON[k] || "copy", "sm")),
    h("div", { class: "body" }, body,
      h("div", { class: "meta" },
        c.pinned ? h("span", { class: "pin-on" }, "Pinned") : null,
        h("span", null, ago(c.ts)),
        c.app ? h("span", null, c.app) : null,
        k === "link" ? h("span", null, (() => { try { return new URL(c.text.trim()).host; } catch { return ""; } })()) : null,
        c.kind === "text" ? h("span", null, plural(c.text.length, "char")) : c.kind === "image" ? h("span", null, `${c.w}×${c.h}`) : null)));
}

function clipDetail(c) {
  const el = h("div", { class: "card col", style: { position: "sticky", top: "0", gap: "12px" } });
  // with "paste on click" off the main button already copies, so the second Copy would be a twin
  const pasting = S.state.settings.clip_paste;
  el.append(h("div", { class: "row wrap clip-acts" },
    h("button", { class: "btn primary", onclick: () => pasteClip(c.id) }, I(pasting ? "clipboard" : "copy"), pasting ? "Paste" : "Copy"),
    pasting ? h("button", { class: "btn", onclick: () => run("clip_copy", { id: c.id, paste: false }, "Copied") }, I("copy"), "Copy") : null,
    h("span", { class: "grow" }),
    h("button", { class: "icon-btn" + (c.pinned ? " on" : ""), title: c.pinned ? "Unpin" : "Pin (kept forever)", onclick: async () => { await invoke("clip_pin", { id: c.id, on: !c.pinned }); await Pages.clipboard.load(); render(); } }, I("pin")),
    c.kind === "text" ? h("button", { class: "icon-btn", title: "Send to phone", onclick: () => run("share_to_phone", { kind: "text", text: c.text }, "Sent to your phone") }, I("send")) : null,
    c.kind === "files" && c.files.length === 1 ? h("button", { class: "icon-btn", title: "Send to phone", onclick: () => run("share_to_phone", { kind: "path", text: c.files[0] }, "Sent to your phone") }, I("send")) : null,
    h("button", { class: "icon-btn danger", title: "Delete", onclick: async () => { await invoke("clip_remove", { id: c.id }); await Pages.clipboard.load(); render(); } }, I("trash"))));
  if (c.kind === "image") {
    el.append(h("img", { src: iconSrc("clip", c.image), style: { maxWidth: "100%", borderRadius: "10px" } }));
  } else if (c.kind === "files") {
    el.append(h("div", { class: "list" }, c.files.map((f) => h("button", { class: "item", onclick: () => invoke("reveal", { path: f }) }, appIcon("file", f, f.split("\\").pop()), h("div", { class: "grow" }, h("div", { class: "t ell" }, f.split("\\").pop()), h("div", { class: "s ell" }, f))))));
  } else {
    el.append(h("pre", { style: { margin: 0, whiteSpace: "pre-wrap", wordBreak: "break-word", maxHeight: "240px", overflow: "auto", background: "var(--bg2)", border: "1px solid var(--line)", borderRadius: "10px", padding: "10px 12px", fontFamily: "Consolas, monospace", fontSize: "12.5px", userSelect: "text" } }, c.text));
    el.append(h("div", { class: "label" }, "Paste as"));
    el.append(h("div", { class: "row wrap", style: { gap: "6px" } }, TRANSFORMS.map(([name, fn]) =>
      h("button", { class: "chip", onclick: async () => {
        try { await run("set_clipboard", { text: fn(c.text), paste: S.state.settings.clip_paste }); if (!S.state.settings.clip_paste) note("Copied"); }
        catch (e) { note(e.message || String(e), true); }
      } }, name))));
    const words = (c.text.match(/\S+/g) || []).length;
    const lines = c.text.split("\n").length;
    el.append(h("div", { class: "faint", style: { fontSize: "12px" } }, `${plural(c.text.length, "character")} · ${plural(words, "word")} · ${plural(lines, "line")}`));
  }
  return el;
}

// ------------------------------------------------------------------ audio

Pages.audio = {
  async load() {
    const a = await invoke("audio_state");
    S.cache.outputs = a.outputs;
  },
  render() {
    const l = S.live || {};
    const m = l.master;
    const el = h("div", null, head("Sound", "Switch where sound plays and set a volume for each app. Deck never changes your microphone or recording devices."));
    // where sound plays
    const outs = S.cache.outputs || [];
    el.append(h("div", { class: "section", style: { marginTop: "6px" } }, h("h2", null, "Play sound through")));
    el.append(h("div", { class: "dev-grid" }, outs.map((o) =>
      h("button", { class: "dev" + (o.default ? " on" : ""), title: o.name, "aria-pressed": o.default ? "true" : "false", onclick: () => setOutput(o.id) },
        h("span", { class: "dev-ic" }, I(deviceIcon(o.name))),
        h("div", { style: { minWidth: 0 } }, h("div", { class: "t ell" }, shortDevice(o.name)), h("div", { class: "s ell" }, o.name)),
        o.default ? h("span", { class: "dev-on", title: "In use" }, I("check", "sm")) : null))));
    // the master volume, then one card per app
    el.append(h("div", { class: "section" }, h("h2", null, "Volume")));
    if (m) el.append(masterCard(m));
    const apps = l.apps || [];
    if (apps.length) el.append(h("div", { class: "mix-grid" }, apps.map(appMixCard)));
    else el.append(h("div", { class: "empty" }, "No app is making sound right now."));
    // what is playing
    const sessions = [l.media?.current, ...(l.media?.others || [])].filter((x) => x && x.title);
    if (sessions.length) {
      el.append(h("div", { class: "section" }, h("h2", null, "Playing")));
      el.append(h("div", { class: "col", style: { gap: "8px" } }, sessions.map((s) =>
        h("div", { class: "ws" },
          h("div", { class: "art", style: { width: "38px", height: "38px", borderRadius: "9px", backgroundImage: s.art ? `url("${s.art}")` : null } }, s.art ? null : I("music", "sm")),
          h("div", { class: "grow" }, h("div", { style: { fontWeight: 700 }, class: "ell" }, s.title), h("div", { class: "dim ell", style: { fontSize: "12px" } }, [s.artist, s.app, s.status].filter(Boolean).join(" · "))),
          h("div", { class: "row", style: { gap: "2px" } },
            h("button", { class: "icon-btn sm", title: "Previous", disabled: !s.can_prev, onclick: () => media("prev", s.id) }, I("prev", "sm")),
            h("button", { class: "icon-btn sm", title: s.status === "playing" ? "Pause" : "Play", onclick: () => media("toggle", s.id) }, I(s.status === "playing" ? "pause" : "play", "sm")),
            h("button", { class: "icon-btn sm", title: "Next", disabled: !s.can_next, onclick: () => media("next", s.id) }, I("next", "sm")))))));
    }
    return el;
  },
  live: true,
};

/** The volume of everything: big, because it is the one you reach for. */
function masterCard(m) {
  const val = h("b", { class: "num mix-big" }, Math.round(m.vol * 100) + "%");
  const s = slider(Math.round(m.vol * 100), 0, 100, 1, (v) => { val.textContent = v + "%"; invoke("set_master", { vol: v / 100 }); });
  return h("div", { class: "card mix-master" + (m.muted ? " muted" : "") },
    h("button", { class: "mix-mute", title: m.muted ? "Unmute" : "Mute", "aria-label": m.muted ? "Unmute" : "Mute", onclick: () => invoke("set_master", { muted: !m.muted }) }, I(m.muted ? "volume-x" : "volume", "lg")),
    h("div", { class: "col grow", style: { gap: "9px", minWidth: 0 } },
      h("div", { class: "row" }, h("b", null, "Everything"), m.device ? h("span", { class: "faint small ell" }, shortDevice(m.device)) : null, h("span", { class: "grow" }), val),
      s,
      h("div", { class: "meter" }, h("i", { style: { width: Math.min(100, (m.peak || 0) * 100) + "%" } }))));
}

/** One app's volume: name and mute on top, the slider under it. */
function appMixCard(a) {
  const val = h("span", { class: "num dim mix-pct" }, Math.round(a.vol * 100));
  const s = slider(Math.round(a.vol * 100), 0, 100, 1, (v) => { val.textContent = v; invoke("set_app_volume", { key: a.key, vol: v / 100 }); });
  return h("div", { class: "mix-app" + (a.muted ? " muted" : "") },
    h("div", { class: "row", style: { gap: "10px" } },
      a.exe ? appIcon("file", a.exe, a.name) : I("volume", "sm"),
      h("span", { class: "t ell grow", title: a.name }, a.name),
      val,
      h("button", { class: "icon-btn sm", title: a.muted ? "Unmute" : "Mute", "aria-label": (a.muted ? "Unmute " : "Mute ") + a.name, onclick: () => invoke("set_app_volume", { key: a.key, muted: !a.muted }) }, I(a.muted ? "volume-x" : "volume", "sm"))),
    s,
    h("div", { class: "meter" }, h("i", { style: { width: Math.min(100, (a.peak || 0) * 100) + "%" } })));
}

// ---------------------------------------------------------------- windows

Pages.windows = {
  async load() {
    S.cache.windows = await invoke("windows_list");
  },
  render() {
    const sub = S.sub || "open";
    const el = h("div", null, tabsHead("windows", "Windows", [["open", "Open windows"], ["layouts", "Snap zones"], ["workspaces", "Workspaces"]], sub,
      sub === "open" ? h("button", { class: "btn ghost", onclick: async () => { await Pages.windows.load(); render(); } }, I("refresh"), "Refresh") : null));
    if (sub === "layouts") el.append(layoutsView());
    else if (sub === "workspaces") el.append(workspacesView());
    else el.append(windowsView());
    return el;
  },
};

function currentLayout() {
  const s = S.state.settings;
  return s.layouts.find((l) => l.id === s.layout) || s.layouts[0];
}

function zoneMap(layout, onpick, w = 150, hh = 64) {
  const map = h("div", { class: "zmap", style: { width: w + "px", height: hh + "px" } });
  layout.zones.forEach((z, i) => {
    map.append(h("button", {
      title: `Snap to zone ${i + 1}`,
      style: { left: z.x * w + 1 + "px", top: z.y * hh + 1 + "px", width: z.w * w - 2 + "px", height: z.h * hh - 2 + "px" },
      onclick: () => onpick(i),
    }, String(i + 1)));
  });
  return map;
}

function windowsView() {
  const wins = S.cache.windows || [];
  if (!wins.length) return h("div", { class: "empty" }, "No app windows open.");
  const lay = currentLayout();
  const label = (cls, text, tip) => h("span", { class: cls, title: tip || "" }, text);
  const header = h("div", { class: "win wins-head", "aria-hidden": "true" }, h("span"), h("span"),
    h("div", { class: "tools" }, label("t-pin", "Top", "Keep on top of everything"), label("t-see", "See-through"), label("t-snap", "Snap to zone"), label("t-act", "Show · centre · hide · close")));
  return h("div", { class: "wins" }, header, wins.map((w) => {
    const op = h("span", { class: "dim num", style: { width: "36px", fontSize: "12px", textAlign: "right" } }, Math.round((w.opacity / 255) * 100) + "%");
    return h("div", { class: "win" },
      appIcon("file", w.exe, w.app),
      h("div", { style: { minWidth: 0 } },
        h("div", { class: "t ell" }, w.title),
        h("div", { class: "s ell" }, w.app, w.hung ? h("span", { class: "chip bad sm-chip" }, "Not responding") : null, w.topmost ? h("span", { class: "chip on sm-chip" }, "On top") : null)),
      h("div", { class: "tools" },
        h("button", { class: "icon-btn sm" + (w.topmost ? " on" : ""), title: w.topmost ? "Stop keeping on top" : "Keep on top of everything", onclick: async () => { await run("window_action", { hwnd: w.hwnd, action: "top", value: w.topmost ? 0 : 1 }); w.topmost = !w.topmost; render(); } }, I("pin", "sm")),
        h("span", { title: "See-through", class: "row t-see", style: { gap: "4px" } }, I("eye", "sm"),
          slider(Math.round((w.opacity / 255) * 100), 20, 100, 1, (v) => { op.textContent = v + "%"; invoke("window_action", { hwnd: w.hwnd, action: "opacity", value: (v / 100) * 255 }); }), op),
        zoneMap(lay, (i) => run("snap_window", { hwnd: w.hwnd, layout: lay.id, zone: i }), 96, 40),
        h("button", { class: "icon-btn sm", title: "Show", onclick: () => invoke("window_action", { hwnd: w.hwnd, action: "focus" }).then(() => invoke("hide")) }, I("external", "sm")),
        h("button", { class: "icon-btn sm", title: "Center on screen", onclick: () => invoke("window_action", { hwnd: w.hwnd, action: "center" }) }, I("target", "sm")),
        h("button", { class: "icon-btn sm", title: "Minimize", onclick: () => invoke("window_action", { hwnd: w.hwnd, action: "min" }) }, I("minus", "sm")),
        h("button", { class: "icon-btn sm danger", title: "Close", "aria-label": "Close " + w.title, onclick: async () => { await invoke("window_action", { hwnd: w.hwnd, action: "close" }); setTimeout(async () => { await Pages.windows.load(); render(); }, 500); } }, I("x", "sm"))));
  }));
}

function layoutsView() {
  const s = S.state.settings;
  const el = h("div", null);
  el.append(h("div", { class: "settings-card" },
    h("div", { class: "setting" },
      h("div", { class: "grow" }, h("div", { class: "t" }, "Snap windows into zones while dragging"),
        h("div", { class: "d" }, s.zones_trigger === "always"
          ? "Drag a window by its title bar: the zones appear and it snaps into the one under your mouse."
          : "Drag a window by its title bar, then tap the right mouse button: the zones appear. Let go over a zone to snap. Tap again to hide them.")),
      toggle(s.zones_on, (v) => setSetting("zones_on", v).then(render))),
    h("div", { class: "setting" },
      h("div", { class: "grow" }, h("div", { class: "t" }, "When to show the zones")),
      seg([["rbutton", "Tap right mouse button"], ["always", "Every drag"]], s.zones_trigger, (v) => setSetting("zones_trigger", v).then(render)))));
  el.append(h("div", { class: "section" }, h("h2", null, "Layout"), h("span", { class: "grow" }), h("span", { class: "dim" }, "Used for dragging and the snap buttons")));
  el.append(h("div", { class: "layouts" }, s.layouts.map((l) =>
    h("button", { class: "lay" + (l.id === s.layout ? " on" : ""), onclick: () => setSetting("layout", l.id).then(render) },
      h("div", { class: "pv" }, l.zones.map((z) => h("i", { style: { left: z.x * 100 + "%", top: z.y * 100 + "%", width: `calc(${z.w * 100}% - 4px)`, height: `calc(${z.h * 100}% - 4px)` } }))),
      h("b", null, l.name)))));
  return el;
}

/** What opening a workspace does besides placing windows. */
function wsChips(ws) {
  return [ws.close.length ? ["x", `Closes ${ws.close.map((c) => c.replace(/\.exe$/i, "")).join(", ")}`] : null,
    ws.audio_out ? ["volume", `Sound to ${shortDevice(ws.audio_out)}`] : null,
    ws.awake ? ["coffee", "Keeps the PC awake"] : null].filter(Boolean);
}

function workspacesView() {
  const s = S.state.settings;
  const el = h("div", null);
  el.append(h("div", { class: "row", style: { marginBottom: "14px" } },
    h("div", { class: "dim grow" }, "Arrange your apps how you like them, save, and bring the whole setup back with one click. A workspace can also close apps (like game launchers) and switch your sound output."),
    h("button", { class: "btn primary", onclick: async () => {
      const name = await promptBox("Save a workspace", "Name (e.g. Coding, Gaming, Chill)", "", "Save");
      if (!name) return;
      await run("ws_capture", { name, icon: "layout" }, "Saved: " + name);
      S.state = await invoke("get_state");
      render();
    } }, I("plus"), "Save open windows")));
  const list = s.workspaces || [];
  if (!list.length) el.append(h("div", { class: "card col setup-card" }, h("div", { class: "boost-say" }, "No workspaces yet."),
    h("div", { class: "dim" }, "Open the apps you use for something (say coding or gaming), put them where you want them, then press Save open windows. Next time, one click brings it all back.")));
  for (const ws of list) {
    el.append(h("div", { class: "ws", style: { marginBottom: "8px" } },
      h("div", { class: "ic" }, I("layout")),
      h("div", { style: { minWidth: 0 } },
        h("div", { style: { fontWeight: 700 } }, ws.name),
        h("div", { class: "ws-apps" },
          [...new Map(ws.windows.map((w) => [w.exe, w])).values()].slice(0, 7).map((w) => h("span", { title: w.exe.split("\\").pop().replace(/\.exe$/i, "") }, appIcon("file", w.exe, w.exe.split("\\").pop()))),
          h("span", { class: "dim small" }, plural(ws.windows.length, "window"))),
        wsChips(ws).length ? h("div", { class: "row wrap", style: { gap: "6px", marginTop: "6px" } }, wsChips(ws).map(([ic, text]) => h("span", { class: "chip" }, I(ic, "sm"), text))) : null),
      h("div", { class: "row", style: { gap: "4px" } },
        h("button", { class: "icon-btn sm", title: "Edit", onclick: () => editWorkspace(ws) }, I("edit", "sm")),
        h("button", { class: "icon-btn sm", title: "Replace with the windows open now", onclick: async () => { await run("ws_recapture", { id: ws.id }, "Updated"); S.state = await invoke("get_state"); render(); } }, I("refresh", "sm")),
        h("button", { class: "icon-btn sm danger", title: "Delete", onclick: async () => { if (await confirmBox(`Delete ${ws.name}?`, "Only the saved layout is deleted.", "Delete", true)) { await setSetting("workspaces", list.filter((x) => x.id !== ws.id)); render(); } } }, I("trash", "sm")),
        h("button", { class: "btn sm primary", onclick: () => run("ws_open", { id: ws.id }) }, I("play", "sm"), "Open"))));
  }
  return el;
}

function editWorkspace(ws) {
  const name = h("input", { type: "text", value: ws.name });
  const close = h("input", { type: "text", value: ws.close.join(", "), placeholder: "e.g. EpicGamesLauncher.exe, Steam.exe" });
  const audio = h("select", null, h("option", { value: "" }, "Leave it"), (S.cache.outputs || []).map((o) => h("option", { value: o.name, selected: o.name === ws.audio_out }, o.name)));
  const awake = h("input", { type: "checkbox", checked: ws.awake });
  modal("Edit workspace", h("div", { class: "col" },
    h("label", { class: "field" }, h("span", null, "Name"), name),
    h("label", { class: "field" }, h("span", null, "Close these apps when opening (they get the chance to save first)"), close),
    h("label", { class: "field" }, h("span", null, "Switch sound to"), audio),
    h("label", { class: "row" }, awake, h("span", null, "Keep the PC awake"))), (done) => [
    h("button", { class: "btn ghost", onclick: done }, "Cancel"),
    h("button", { class: "btn primary", onclick: async () => {
      const list = S.state.settings.workspaces.map((x) => x.id === ws.id ? { ...x, name: name.value.trim() || x.name, close: close.value.split(",").map((s) => s.trim()).filter(Boolean).map((s) => /\.exe$/i.test(s) ? s : s + ".exe"), audio_out: audio.value, awake: awake.checked } : x);
      await setSetting("workspaces", list);
      done();
      render();
    } }, "Save"),
  ]);
}
