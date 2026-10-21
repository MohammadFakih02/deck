// Your other PCs: Transfer (files, folders, installers, apps and setups between your PCs) and
// Workspace Sync for Claude Code (sessions, memory, settings, project files and programs kept the same on your PCs).
import { I, Pages, S, ago, confirmBox, fmtBytes, h, invoke, note, run, seg, toggle } from "./core.js";
import { copyAnswer } from "./search.js";
import { head, plural } from "./pages1.js";
import { curSub, go, render } from "./app.js";


export const dur = (s) => { s = Math.max(0, Math.round(s)); return s >= 3600 ? `${Math.floor(s / 3600)} h ${Math.floor(s % 3600 / 60)} min` : s >= 60 ? `${Math.floor(s / 60)} min ${s % 60} s` : `${s} s`; };
/** append() writes "null" for an empty slot: this leaves empty slots out (and flattens lists). */
export const addTo = (el, ...kids) => el.append(...kids.flat(Infinity).filter((k) => k != null && k !== false && k !== ""));
export const agoMs = (ms) => (ms < 0 ? "never" : ago(Date.now() - ms));
export const row = (label, ...rest) => h("div", { class: "rd-row" }, h("span", { class: "dim" }, label), ...rest);
export const swRow = (label, on, fn, tip) => h("label", { class: "rd-row", title: tip || "" }, h("span", { class: "grow" }, label), toggle(on, fn));

/** A text box that saves when it loses focus or Enter is pressed. */
export function saveBox(value, save, attrs = {}) {
  const el = h("input", { type: "text", value: value ?? "", ...attrs });
  const done = () => { if (el.value !== (value ?? "")) save(el.value.trim()); };
  el.addEventListener("change", done);
  el.addEventListener("keydown", (e) => e.key === "Enter" && el.blur());
  return el;
}

// ------------------------------------------------------------------------------------------- Transfer

/** How long a PC may send without asking. */
export const TX_TRUST = [["7", "for a week"], ["30", "for a month"], ["182", "for 6 months"], ["always", "always"]];
export const txUntil = (u) => (u === 0 ? "always" : `until ${new Date(u).toLocaleDateString(undefined, { day: "numeric", month: "short", year: "numeric" })}`);

export const TX_TABS = [["send", "Send"], ["share", "Share link"], ["apps", "Apps"], ["setup", "Setup"], ["received", "Received"], ["settings", "Settings"], ["log", "Log"]];

export async function tx(what, args = {}, ok) {
  try { S.cache.tx = await run("transfer_do", { what, args }, ok); } catch { /* run() showed it */ }
  render();
}

export function txPeer(t) {
  const peers = t.peers || [];
  return peers.find((p) => p.id === S.cache.txPeer) || peers[0] || null;
}

/** Offers and requests waiting for an answer (also shown as notifications). */
export function txPrompts(t) {
  return [
    ...(t.reqs || []).map((r) => h("div", { class: "card col tx-ask" },
      h("b", null, `${r.peer} wants to ${r.what}`),
      h("div", { class: "faint" }, `Declines by itself in ${r.left} s`),
      h("div", { class: "row wrap" },
        h("button", { class: "btn primary", onclick: () => tx("answer", { id: r.id, answer: "allow" }) }, "Allow"),
        h("button", { class: "btn", onclick: () => tx("answer", { id: r.id, answer: "always" }) }, "Always allow this PC"),
        h("button", { class: "btn ghost", onclick: () => tx("answer", { id: r.id, answer: "deny" }) }, "Deny")))),
    ...(t.pending || []).map((p) => h("div", { class: "card col tx-ask" },
      h("b", null, `${p.peer} wants to send you ${p.label}`),
      p.deletes ? h("div", { class: "chip warn" }, I("alert", "sm"), `Also deletes ${plural(p.deletes, "file")} here that ${p.peer} no longer has`) : null,
      h("div", { class: "dim" }, `${plural(p.files, "file")}, ${fmtBytes(p.total)}${p.install ? ", and install it on this PC" : ""}`),
      h("div", { class: "row wrap" },
        h("button", { class: "btn primary", onclick: () => tx("answer", { id: p.id, answer: "accept" }) }, p.install ? "Accept and install" : "Accept"),
        h("button", { class: "btn ghost", onclick: () => tx("answer", { id: p.id, answer: "decline" }) }, "Decline"),
        h("span", { class: "grow" }),
        h("select", { "aria-label": `Accept and trust ${p.peer}`, title: `Accept, and let ${p.peer} send without asking for a while`, onchange: (e) => e.target.value && tx("answer", { id: p.id, answer: e.target.value }, `${p.peer} is trusted`) },
          h("option", { value: "" }, `Accept and trust ${p.peer}…`),
          TX_TRUST.map(([v, l]) => h("option", { value: "trust_" + v }, l)))))),
  ];
}

export function txPeers(t) {
  const peers = t.peers || [];
  if (!peers.length) return h("div", { class: "card col" },
    h("div", { class: "row" }, h("span", { class: "chip warn" }, I("search", "sm"), "Looking for your other PC…")),
    h("div", { class: "dim" }, "Open Deck on the other PC and switch PC to PC on there too. Both PCs need the same pairing code (Settings tab); on the same network they then find each other by themselves. Away from home, they find each other over Tailscale."),
    t.tailscale ? csTailscale(t.tailscale, () => tx("tailscale_open")) : null,
    row("This PC's code", h("b", { class: "num grow", style: { userSelect: "text" } }, t.code), h("button", { class: "icon-btn sm", title: "Copy", "aria-label": "Copy", onclick: () => copyAnswer(t.code) }, I("copy", "sm"))),
    row("Code from the other PC", saveBox("", (v) => v && tx("set", { key: "code", value: v }, "Code saved. Looking for the other PC…"), { placeholder: "XXXX-XXXX-XXXX-XXXX-XXXX", class: "grow mono" })));
  const cur = txPeer(t);
  return h("div", { class: "tx-peers" }, peers.map((p) => h("button", {
    class: "tx-peer" + (cur && cur.id === p.id ? " on" : ""), onclick: () => { S.cache.txPeer = p.id; render(); },
  }, I("monitor"), h("span", { class: "grow col", style: { gap: "2px", alignItems: "flex-start" } }, h("b", null, p.name), h("small", { class: "faint" }, `${p.ip} · ${p.tailscale ? "over Tailscale" : "home network"}`)),
    h("span", { class: "btn sm", role: "button", title: "See this PC's shared folders and copy from them", onclick: (e) => { e.stopPropagation(); S.cache.txPeer = p.id; tx("browse", { peer: p.id, path: "" }); } }, I("folder", "sm"), "Browse"))));
}

export function txProgress(x) {
  const pct = x.total ? Math.min(100, (x.done / x.total) * 100) : 0;
  const dir = x.send ? `to ${x.peer}` : `from ${x.peer}`;
  let line, cls = "dim";
  if (x.state === "waiting") line = x.send ? `Waiting for ${x.peer} to accept…` : "Waiting for your answer…";
  else if (x.state === "active") line = `${fmtBytes(x.done)} of ${fmtBytes(x.total)} · ${fmtBytes(x.speed)}/s${x.speed > 1 ? ` · ${dur((x.total - x.done) / x.speed)} left` : ""}`;
  else if (x.state === "done") { line = `Done: ${fmtBytes(x.total)} in ${dur(x.secs)} (${fmtBytes(x.total / Math.max(0.01, x.secs))}/s)`; cls = "good-t"; }
  else if (x.state === "failed") { line = `Failed: ${x.error}`; cls = "bad-t"; }
  else if (x.state === "cancelled") { line = x.resumable ? `Cancelled. ${fmtBytes(x.done)} of ${fmtBytes(x.total)} arrived and is kept: Carry on sends the rest.` : "Cancelled"; cls = "warn-t"; }
  else { line = x.send ? `${x.peer} declined it` : "You declined it"; cls = "warn-t"; }
  const live = x.state === "waiting" || x.state === "active";
  return h("div", { class: "item tx-x" },
    h("div", { class: "ic" }, I(x.send ? "upload" : "download")),
    h("div", { class: "grow col", style: { gap: "4px", minWidth: 0 } },
      h("div", { class: "row", style: { gap: "6px" } }, h("b", { class: "ell" }, x.label), h("span", { class: "faint" }, dir)),
      x.state === "active" ? h("div", { class: "bar" }, h("i", { style: { width: pct + "%" } })) : null,
      x.note ? h("div", { class: "warn-t", style: { fontSize: "12px" } }, x.note) : null,
      h("div", { class: cls, style: { fontSize: "12px" } }, line)),
    live ? h("button", { class: "btn sm ghost", title: "Stops it. What already arrived is kept on the other PC, so Carry on sends only the rest.", onclick: () => tx("cancel", { id: x.id }) }, "Cancel")
      : [x.resumable ? h("button", { class: "btn sm primary", title: `Sends it again; ${x.peer} keeps what it already got, so only the rest goes`, onclick: () => tx("resume", { id: x.id }) }, I("refresh", "sm"), "Carry on") : null,
        h("button", { class: "icon-btn sm", title: "Remove from list", "aria-label": "Remove from list", onclick: () => tx("dismiss", { id: x.id }) }, I("x", "sm"))]);
}

/** A folder waiting for Send: what goes, and the options. */
export function txStaged(t, sc) {
  const cur = txPeer(t);
  const o = (S.cache.txOpts ||= {})[sc.path] ||= { skip: t.skip_build, sync: false, del: false, ok: false };
  const paths = sc.path.split("|");
  const name = paths.length === 1 ? paths[0].split("\\").pop() : `${paths.length} items`;
  const [all, kept] = sc.scan || [];
  const shown = sc.scan ? (o.skip ? kept : all) : null;
  const ready = sc.scan && cur && (!o.del || o.ok);
  const chk = (key, label, tip) => h("label", { class: "row", title: tip || "" }, h("input", { type: "checkbox", checked: o[key], onchange: (e) => { o[key] = e.target.checked; render(); } }), label);
  return h("div", { class: "card col" },
    h("div", { class: "tile-head" }, I("folder"), h("span", { class: "grow" }, `Send ${name}${cur ? ` to ${cur.name}` : ""}`)),
    shown ? h("div", null, `${plural(shown[0], "file")}, ${fmtBytes(shown[1])}`) : h("div", { class: "faint" }, "Counting files…"),
    sc.scan && o.skip && all[0] > kept[0] ? h("div", { class: "faint" }, `${plural(all[0] - kept[0], "build-output file")} (${fmtBytes(all[1] - kept[1])}) left out`) : null,
    chk("skip", "Skip build output", "Leaves out what's listed under Settings (target, node_modules, .git, …)"),
    chk("sync", "Only send what's new or changed", "Compares size and date, then a checksum: a second copy of a project takes seconds"),
    chk("del", "Delete files on the other PC that no longer exist here"),
    o.del ? h("div", { class: "col", style: { gap: "6px" } }, h("div", { class: "warn-t" }, `This removes files from ${cur?.name || "the other PC"}'s copy. It asks its user before deleting anything.`), chk("ok", "Yes, delete them")) : null,
    h("div", { class: "row" },
      h("button", { class: "btn primary", disabled: !ready, onclick: () => tx("send", { peer: cur.id, paths, confirmed: true, skip: o.skip, sync: o.sync, delete_extra: o.del && o.ok, install: !!S.cache.txInstall }) }, I("send"), "Send"),
      h("button", { class: "btn ghost", onclick: () => tx("unstage", { path: sc.path }) }, "Cancel")));
}

export function txBrowse(t) {
  const b = t.browse;
  if (!b) return null;
  const peer = (t.peers || []).find((p) => p.id === b.peer);
  const sel = (S.cache.txSel ||= new Set());
  const join = (n) => (b.path ? `${b.path}/${n}` : n);
  const o = (S.cache.txReqOpts ||= { skip: t.skip_build, sync: false });
  const items = b.items;
  let body;
  if (!items) body = h("div", { class: "faint" }, `Waiting for ${peer?.name || "the other PC"}… it may ask its user first (up to 60 seconds).`);
  else if (items.error) body = h("div", { class: "bad-t" }, items.error);
  else if (!items.length) body = h("div", { class: "faint" }, b.path ? "Empty folder" : "Nothing is shared on that PC yet (its PC to PC page → Settings → Shared folders).");
  else body = h("div", { class: "list tx-files" }, items.map((it) => {
    const full = join(it.name);
    return h("div", { class: "item" },
      h("input", { type: "checkbox", checked: sel.has(full), onchange: (e) => { e.target.checked ? sel.add(full) : sel.delete(full); render(); } }),
      h("div", { class: "ic" }, I(it.dir ? "folder" : "file")),
      it.dir ? h("button", { class: "link grow ell", onclick: () => { sel.clear(); tx("browse", { peer: b.peer, path: full }); } }, it.name) : h("span", { class: "grow ell" }, it.name),
      it.dir ? null : h("span", { class: "faint num" }, fmtBytes(it.size)));
  }));
  const typed = h("input", { type: "text", placeholder: "or type a path, e.g. Projects/app/src", class: "grow" });
  const chk = (key, label) => h("label", { class: "row" }, h("input", { type: "checkbox", checked: o[key], onchange: (e) => { o[key] = e.target.checked; } }), label);
  return h("div", { class: "card col" },
    h("div", { class: "tile-head" }, I("folder"), h("span", { class: "grow" }, `Files on ${peer?.name || "the other PC"}`),
      h("button", { class: "icon-btn sm", title: "Close", "aria-label": "Close", onclick: () => { sel.clear(); tx("browse_close"); } }, I("x", "sm"))),
    h("div", { class: "row" },
      h("button", { class: "btn sm", disabled: !b.path, onclick: () => { sel.clear(); tx("browse", { peer: b.peer, path: b.path.includes("/") ? b.path.slice(0, b.path.lastIndexOf("/")) : "" }); } }, I("moveup", "sm"), "Up"),
      h("code", { class: "grow ell" }, b.path || "Shared folders")),
    body,
    h("div", { class: "row" }, typed),
    h("div", { class: "row wrap" }, chk("skip", "Skip build output"), chk("sync", "Only new or changed files")),
    h("div", { class: "row" },
      h("button", { class: "btn primary", onclick: () => {
        const want = [...sel, ...(typed.value.trim() ? [typed.value.trim()] : [])];
        if (!want.length) return note("Tick something, or type a path", true);
        sel.clear();
        tx("request", { peer: b.peer, items: want, skip: o.skip, sync: o.sync }, `Requested from ${peer?.name || "the other PC"}`);
      } }, I("download"), sel.size ? `Copy ${sel.size} to this PC` : "Copy to this PC")));
}

export function txSendTab(t) {
  const cur = txPeer(t);
  const ready = !!cur;
  const pickFiles = async () => { const ps = await invoke("pick_many", { title: "Files or installers to send" }); if (ps?.length) tx("send", { peer: cur.id, paths: ps, install: !!S.cache.txInstall }, `Sending to ${cur.name}`); };
  const pickFolder = async () => { const p = await invoke("pick", { folder: true, title: "A folder to send" }); if (p) tx("send", { peer: cur.id, paths: [p] }); };
  const ta = h("textarea", { rows: 2, "aria-label": "Text or a link to send", placeholder: "Text or a link to send…", style: { width: "100%" } });
  const xfers = t.xfers || [];
  return [
    txPeers(t),
    txBrowse(t),
    ...(t.scans || []).map((sc) => txStaged(t, sc)),
    h("div", { class: "card col" },
      h("div", { class: "tile-head" }, I("send"), h("span", { class: "grow" }, cur ? `Send to ${cur.name}` : "Send")),
      h("div", { class: "row wrap" },
        h("button", { class: "btn primary lg", disabled: !ready, onclick: pickFiles }, I("file"), "Files or installers"),
        h("button", { class: "btn lg", disabled: !ready, onclick: pickFolder }, I("folder"), "A folder"),
        h("label", { class: "row", title: "Starts the installer on the other PC when it has arrived: silent when the installer supports it, otherwise its setup window opens. The other PC asks its user first unless it trusts this one." },
          h("input", { type: "checkbox", checked: !!S.cache.txInstall, onchange: (e) => (S.cache.txInstall = e.target.checked) }), "Install on arrival")),
      ta,
      h("div", { class: "row" }, h("button", { class: "btn", disabled: !ready, onclick: () => { if (ta.value.trim()) tx("send_text", { peer: cur.id, text: ta.value }, "Text sent"); } }, I("note"), "Send text"))),
    xfers.length ? h("div", { class: "card col" },
      h("div", { class: "tile-head" }, I("swap"), h("span", { class: "grow" }, "Transfers"),
        xfers.some((x) => !["waiting", "active"].includes(x.state)) ? h("button", { class: "link", onclick: () => tx("clear_done") }, "Clear finished") : null),
      h("div", { class: "list" }, xfers.map(txProgress))) : null,
  ];
}

export function txAppsTab(t) {
  const a = S.cache.txApps;
  const cur = txPeer(t);
  const el = h("div", { class: "card col" },
    h("div", { class: "tile-head" }, I("grid"), h("span", { class: "grow" }, "Send apps and install them there")),
    h("div", { class: "dim" }, "Pick apps installed on this PC. The other PC installs them itself with winget (no installer files to find or send) and skips the ones it already has."));
  if (!a || a.busy) return (addTo(el, h("div", { class: "faint" }, "Listing installed apps (a few seconds)…")), el);
  if (a.error) return (addTo(el, h("div", { class: "bad-t" }, a.error), h("button", { class: "btn sm", onclick: () => { S.cache.txApps = null; tx("apps_scan"); } }, "Try again")), el);
  const sel = (S.cache.txAppSel ||= new Set());
  const q = (S.cache.txAppQ || "").toLowerCase();
  const shown = a.apps.filter((x) => (S.cache.txAppRt || !x.runtime) && (!q || x.name.toLowerCase().includes(q) || x.id.toLowerCase().includes(q)));
  const search = h("input", { type: "text", placeholder: "Search apps", value: S.cache.txAppQ || "", oninput: (e) => { S.cache.txAppQ = e.target.value; render(); } });
  addTo(el, 
    h("div", { class: "row wrap" }, search,
      h("label", { class: "row" }, h("input", { type: "checkbox", checked: !!S.cache.txAppRt, onchange: (e) => { S.cache.txAppRt = e.target.checked; render(); } }), "Show runtimes and system parts"),
      h("button", { class: "btn sm ghost", onclick: () => { S.cache.txApps = null; tx("apps_scan"); } }, I("refresh", "sm"), "Rescan")),
    h("div", { class: "row wrap" },
      h("button", { class: "btn sm", onclick: () => { shown.forEach((x) => sel.add(x.id)); render(); } }, "Select all shown"),
      h("button", { class: "btn sm ghost", onclick: () => { sel.clear(); render(); } }, "Select none"),
      h("span", { class: "faint" }, `${sel.size} of ${a.apps.length} selected`),
      h("span", { class: "grow" }),
      h("button", { class: "btn primary", disabled: !sel.size || !cur, onclick: async () => { await tx("apps_send", { peer: cur.id, ids: [...sel] }, `Sending ${plural(sel.size, "app")} to ${cur.name}`); sel.clear(); S.sub = "send"; render(); } },
        I("send"), !cur ? "Waiting for the other PC" : sel.size ? `Send ${plural(sel.size, "app")} to ${cur.name}` : "Pick apps to send")),
    h("div", { class: "list tx-apps" }, shown.map((x) => h("label", { class: "item", title: x.id },
      h("input", { type: "checkbox", checked: sel.has(x.id), onchange: (e) => { e.target.checked ? sel.add(x.id) : sel.delete(x.id); render(); } }),
      h("span", { class: "grow ell" }, x.name), h("span", { class: "faint num" }, x.version)))),
    h("div", { class: "faint", style: { fontSize: "12px" } }, "Apps winget doesn't know can still go as installer files from the Send tab."));
  return el;
}

export function txSetupTab(t) {
  const p = S.cache.txSetup;
  const cur = txPeer(t);
  const o = (S.cache.txSetupOpts ||= { look: true, wallpaper: true, windhawk: true });
  const chk = (key, label, on = true, sub) => h("label", { class: "col", style: { gap: "2px", opacity: on ? 1 : 0.5 } },
    h("span", { class: "row" }, h("input", { type: "checkbox", disabled: !on, checked: on && o[key], onchange: (e) => { o[key] = e.target.checked; render(); } }), label),
    sub ? h("small", { class: "faint", style: { marginLeft: "24px" } }, sub) : null);
  const el = h("div", { class: "card col" },
    h("div", { class: "tile-head" }, I("layout"), h("span", { class: "grow" }, "Make the other PC look like this one")),
    h("div", { class: "dim" }, "Packs this PC's look and feel and Windhawk mods into a snapshot and sends it. On the other PC you pick what to apply; it backs up first, and you can undo."));
  if (!p) return (addTo(el, h("div", { class: "faint" }, "Checking…")), el);
  addTo(el, 
    chk("look", `Windows look and feel (${p.look} settings${p.tweaks ? `, ${p.tweaks} tweak${p.tweaks > 1 ? "s" : ""}` : ""})`, true, "Dark/light mode, accent colour, taskbar alignment and buttons, Explorer options, search box, and the tweaks you set in Maintenance → Tweaks (the other PC can undo them one by one there). Never display scaling, drivers or power settings."),
    chk("wallpaper", "Wallpaper", p.wallpaper),
    chk("windhawk", p.windhawk_installed ? `Windhawk ${p.windhawk || ""}: ${plural(p.mods.length, "mod")}${p.mods.length ? " (" + p.mods.join(", ") + ")" : ""}` : "Windhawk mods (Windhawk isn't installed here)", p.windhawk_installed,
      "Both PCs need the same Windhawk version. Mods are added or updated there, never removed."),
    t.setup_msg ? h("div", { class: "warn-t" }, t.setup_msg) : null,
    h("div", { class: "row" },
      h("button", { class: "btn primary", disabled: !cur || !(o.look || (o.wallpaper && p.wallpaper) || (o.windhawk && p.windhawk_installed)), onclick: async () => {
        await tx("setup_send", { peer: cur.id, look: o.look, wallpaper: o.wallpaper && p.wallpaper, windhawk: o.windhawk && p.windhawk_installed });
        S.sub = "send"; render();
      } }, I("send"), cur ? `Send setup to ${cur.name}` : "Waiting for the other PC"),
      h("button", { class: "btn ghost", onclick: async () => { S.cache.txSetup = null; render(); S.cache.txSetup = await invoke("transfer_setup_preview"); render(); } }, I("refresh"), "Refresh")));
  return el;
}

export function txApplyPanel(it) {
  const e = it.extra || {};
  const o = (S.cache.txApply ||= {})[it.dir] ||= { look: true, wallpaper: true, windhawk: true, restart: true };
  const chk = (key, label, on = true) => h("label", { class: "row", style: { opacity: on ? 1 : 0.5 } }, h("input", { type: "checkbox", disabled: !on, checked: on && o[key], onchange: (ev) => { o[key] = ev.target.checked; } }), label);
  return h("div", { class: "col tx-apply" },
    h("b", null, "Choose what to apply on this PC"),
    chk("look", `Windows look and feel (${e.look || 0} settings${e.tweaks ? `, ${e.tweaks} tweak${e.tweaks > 1 ? "s" : ""}` : ""})`, e.look > 0 || e.tweaks > 0),
    chk("wallpaper", "Wallpaper", !!e.wallpaper),
    chk("windhawk", e.windhawk ? `Windhawk ${e.windhawk}: ${plural(e.mods || 0, "mod")} with settings` : "Windhawk (not in this snapshot)", !!e.windhawk),
    chk("restart", "Restart Explorer afterwards so the taskbar updates (closes open folder windows)"),
    h("div", { class: "row" },
      h("button", { class: "btn primary", onclick: () => { S.cache.txApplyOpen = null; tx("setup_apply", { dir: it.dir, look: o.look && (e.look > 0 || e.tweaks > 0), wallpaper: o.wallpaper && !!e.wallpaper, windhawk: o.windhawk && !!e.windhawk, restart: o.restart }); } }, "Apply now"),
      h("button", { class: "btn ghost", onclick: () => { S.cache.txApplyOpen = null; render(); } }, "Cancel")));
}

export function txReceivedTab(t) {
  const items = t.inbox || [];
  const el = h("div", { class: "card col" },
    h("div", { class: "tile-head" }, I("download"), h("span", { class: "grow" }, "Received"),
      h("button", { class: "link", onclick: () => tx("open_inbox") }, "Open folder")));
  if (!items.length) return (addTo(el, h("div", { class: "faint" }, "Nothing yet. What your other PC sends shows up here, ready to install.")), el);
  addTo(el, h("div", { class: "list" }, items.map((it) => {
    const btns = [];
    if (!it.gone) {
      if (it.kind === "apps") btns.push(h("button", { class: "btn sm primary", onclick: () => tx("apps_install", { dir: it.dir }) }, /failed/.test(it.note) ? "Retry failed" : "Install these apps"));
      if (it.kind === "setup") {
        btns.push(h("button", { class: "btn sm primary", onclick: () => { S.cache.txApplyOpen = it.dir; render(); } }, "Apply setup…"));
        if (it.extra?.backup) btns.push(h("button", { class: "btn sm", onclick: () => tx("setup_undo", { dir: it.dir }) }, I("undo", "sm"), "Undo"));
      }
      if (it.entry) {
        if (it.kind === "zip") btns.push(h("button", { class: "btn sm primary", onclick: () => tx("install", { dir: it.dir, silent: false }) }, "Extract"));
        else {
          btns.push(h("button", { class: "btn sm primary", onclick: () => tx("install", { dir: it.dir, silent: false }) }, "Install"));
          if (it.silent) btns.push(h("button", { class: "btn sm", onclick: () => tx("install", { dir: it.dir, silent: true }) }, "Install silently"));
        }
      }
      btns.push(h("button", { class: "btn sm ghost", onclick: () => tx("show", { dir: it.dir }) }, I("folder", "sm"), "Show files"));
    }
    const del = !it.gone ? h("button", { class: "icon-btn sm danger", title: "Delete the files", "aria-label": "Delete the files", onclick: async () => {
      if (await confirmBox("Delete these files?", `${it.name} (${fmtBytes(it.size)}) is deleted from this PC.`, "Delete", true)) tx("inbox_delete", { dir: it.dir }, "Deleted");
    } }, I("trash", "sm")) : null;
    return h("div", { class: "item tx-in" },
      h("div", { class: "ic" }, I(it.kind === "setup" ? "layout" : it.kind === "apps" ? "grid" : it.entry ? "download" : "folder")),
      h("div", { class: "grow col", style: { gap: "4px", minWidth: 0 } },
        h("div", { class: "row", style: { gap: "8px" } }, h("b", { class: "ell" }, it.name), it.kind_label ? h("span", { class: "chip on" }, it.kind_label) : null),
        h("div", { class: "faint", style: { fontSize: "12px" } }, `from ${it.from} · ${ago(it.time)} · ${plural(it.files, "file")} · ${fmtBytes(it.size)}`),
        it.extra?.apps ? h("div", { class: "dim ell", style: { fontSize: "12px" } }, it.extra.apps.slice(0, 8).join(", ") + (it.extra.apps.length > 8 ? ` and ${it.extra.apps.length - 8} more` : "")) : null,
        it.note ? h("div", { class: "warn-t", style: { fontSize: "12px" } }, it.note) : null,
        it.gone ? h("div", { class: "bad-t", style: { fontSize: "12px" } }, "The files were moved or deleted") : null,
        S.cache.txApplyOpen === it.dir && !it.gone ? txApplyPanel(it) : null,
        btns.length ? h("div", { class: "row wrap", style: { gap: "6px" } }, btns) : null),
      h("div", { class: "row", style: { gap: "2px", alignSelf: "flex-start" } }, del,
        h("button", { class: "icon-btn sm", title: "Remove from list", "aria-label": "Remove from list", onclick: () => tx("inbox_remove", { dir: it.dir }) }, I("x", "sm"))));
  })));
  return el;
}

/** Trusted PCs: what they send comes in without asking, until a date or always. */
export function txTrustCard(t, set) {
  const known = new Map();
  (t.policies || []).forEach((p) => known.set(p.id, { id: p.id, name: p.name, until: null }));
  (t.trusted || []).forEach((x) => known.set(x.id, x));
  const list = [...known.values()];
  return h("div", { class: "card col" },
    h("div", { class: "tile-head" }, I("shield"), h("span", null, "Trusted PCs")),
    h("div", { class: "dim" }, "What a trusted PC sends comes in without asking. Installers and deleting files here are always asked. Trust a PC from its Accept prompt, or here."),
    list.map((x) => row(x.name, h("span", { class: (x.until == null ? "faint" : "good-t") + " grow" }, x.until == null ? "asks first" : "trusted " + txUntil(x.until)),
      h("select", { "aria-label": `Trust ${x.name}`, onchange: (e) => set("trust", { id: x.id, name: x.name, days: e.target.value }, e.target.value ? `${x.name} is trusted` : `${x.name} asks first again`) },
        h("option", { value: "", selected: x.until == null }, "Ask first"),
        TX_TRUST.map(([v, l]) => h("option", { value: v }, "Trust " + l))))),
    !list.length ? h("div", { class: "faint" }, "Your other PC shows up here once it has connected.") : null);
}

/** One-time share links: a file or folder for someone who isn't one of your PCs (any browser). */
export async function shareDo(cmd, args, ok) {
  try { S.cache.share = await run(cmd, args, ok); } catch { /* run() showed it */ }
  render();
}

export function txShareTab() {
  const sh = S.cache.share || { shares: [] };
  const o = (S.cache.shareOpts ||= { hours: 24, max: 1, internet: false });
  const make = async (folder) => {
    const p = await invoke("pick", { folder, title: folder ? "A folder to share (it is zipped)" : "A file to share" });
    if (p) shareDo("share_create", { path: p, hours: o.hours, max: o.max, internet: o.internet }, "Link made: copy it and send it");
  };
  const left = (ms) => { const m = Math.max(0, (ms - Date.now()) / 60000); return m >= 1440 ? `${Math.round(m / 1440)} days` : m >= 60 ? `${Math.round(m / 60)} h` : `${Math.max(1, Math.round(m))} min`; };
  const link = (label, url) => url ? h("div", { class: "rd-row" }, h("span", { class: "dim" }, label), h("code", { class: "grow ell", style: { userSelect: "text" }, title: url }, url),
    h("button", { class: "icon-btn sm", title: "Copy the link", "aria-label": "Copy the link", onclick: () => copyAnswer(url) }, I("copy", "sm"))) : null;
  return [
    h("div", { class: "card col" },
      h("div", { class: "tile-head" }, I("link"), h("span", null, "Share with someone else")),
      h("div", { class: "dim" }, "For a friend or anyone who isn't one of your PCs: they get a link and download in their browser, no Deck needed. Nothing else on this PC is reachable, and the link ends by itself."),
      row("Link works for", h("span", { class: "grow" }), seg([[1, "1 hour"], [24, "1 day"], [168, "7 days"]], o.hours, (v) => { o.hours = v; render(); })),
      row("Downloads", h("span", { class: "grow" }), seg([[1, "Once"], [0, "Until it ends"]], o.max, (v) => { o.max = v; render(); })),
      swRow("Over the internet (Tailscale Funnel)", o.internet, (v) => { o.internet = v; render(); },
        "Off: the link works on this Wi-Fi only. On: a secure internet address through Tailscale, open only while the link is"),
      o.internet && !sh.tailscale ? h("div", { class: "warn-t" }, "Needs Tailscale on this PC (Phone page → Away)") : null,
      h("div", { class: "row wrap" },
        h("button", { class: "btn primary", onclick: () => make(false) }, I("file"), "Share a file"),
        h("button", { class: "btn", onclick: () => make(true) }, I("folder"), "Share a folder"))),
    sh.funnel_error ? h("div", { class: "card col" }, h("div", { class: "warn-t", style: { userSelect: "text" } }, sh.funnel_error),
      /https:\/\/login\.tailscale\.com\S+/.test(sh.funnel_error) ? h("div", { class: "row" }, h("button", { class: "btn sm", onclick: () => invoke("open_url", { url: sh.funnel_error.match(/https:\/\/login\.tailscale\.com\S+/)[0], keep: true }) }, I("external", "sm"), "Allow Funnel"),
        h("span", { class: "faint" }, "then make the link again")) : null) : null,
    sh.shares.length ? h("div", { class: "card col" },
      h("div", { class: "tile-head" }, I("link"), h("span", null, "Open links")),
      h("div", { class: "list" }, sh.shares.map((s) => h("div", { class: "item tx-in" },
        h("div", { class: "ic" }, I(s.name.endsWith(".zip") ? "folder" : "file")),
        h("div", { class: "grow col", style: { gap: "4px", minWidth: 0 } },
          h("b", { class: "ell" }, s.name),
          h("div", { class: "faint", style: { fontSize: "12px" } }, s.preparing ? "Zipping the folder…" : `${fmtBytes(s.size)} · ${s.max === 1 ? "one download" : s.max ? `${s.max - s.done} downloads left` : `${s.done} download${s.done === 1 ? "" : "s"} so far`} · ends in ${left(s.expires)}`),
          s.error ? h("div", { class: "bad-t" }, s.error) : null,
          s.preparing ? null : link("This Wi-Fi", s.home),
          s.preparing ? null : s.internet ? (s.web ? link("Internet", s.web) : h("div", { class: "faint", style: { fontSize: "12px" } }, "Internet address: waiting for Tailscale Funnel…")) : null),
        h("button", { class: "btn sm ghost", onclick: () => shareDo("share_stop", { token: s.token }, "Link stopped") }, "Stop"))))) : null,
  ];
}

export function txSettingsTab(t) {
  const set = (key, value, ok) => tx("set", { key, value }, ok);
  const policy = (p) => h("select", { onchange: (e) => set("policy", { id: p.id, name: p.name, mode: e.target.value }) },
    [["ask", "Ask me each time"], ["always", "Always (trusted)"], ["off", "Never"]].map(([v, l]) => h("option", { value: v, selected: p.mode === v }, l)));
  return [
    h("div", { class: "card col" },
      h("div", { class: "tile-head" }, I("monitor"), h("span", null, "This PC")),
      row("Name the other PC sees", saveBox(t.name, (v) => set("name", v, "Saved"))),
      row("Save received files in", h("code", { class: "grow ell", title: t.inbox_dir }, t.inbox_dir),
        h("button", { class: "btn sm", onclick: async () => { const p = await invoke("pick", { folder: true, title: "Where should received files go?" }); if (p) set("inbox_dir", p, "Saved"); } }, "Change…")),
      swRow("Ask me before anything comes in", t.ask_before_receive, (v) => set("ask_before_receive", v), "On by default: every file or folder waits for your Accept, unless it comes from a PC you trust"),
      h("div", { class: "faint" }, "Installers from another PC always wait for your Accept here, trusted or not, and start through Windows' own admin prompt.")),
    txTrustCard(t, set),
    h("div", { class: "card col" },
      h("div", { class: "tile-head" }, I("lock"), h("span", null, "Pairing")),
      h("div", { class: "dim" }, "Both PCs need the same code. It also encrypts everything they send each other."),
      row("Pairing code", h("b", { class: "num grow", style: { userSelect: "text" } }, t.code), h("button", { class: "icon-btn sm", title: "Copy", "aria-label": "Copy", onclick: () => copyAnswer(t.code) }, I("copy", "sm"))),
      row("Use the other PC's code", saveBox("", (v) => v && set("code", v, "Code saved"), { placeholder: "XXXX-XXXX-XXXX-XXXX-XXXX", class: "grow" })),
      row("Other PC's address", saveBox((t.manual_peers || []).join(", "), (v) => set("manual_peers", v, "Saved"), { placeholder: "only if it isn't found by itself, e.g. LAPTOP or 192.168.1.20", class: "grow" })),
      h("div", { class: "faint", style: { fontSize: "12px" } }, "Away from home: your PCs find each other over Tailscale."),
      t.tailscale ? csTailscale(t.tailscale, () => tx("tailscale_open")) : null,
      h("div", { class: "row" }, h("button", { class: "btn sm ghost", onclick: async () => { if (await confirmBox("Make a new pairing code?", "The other PC stops finding this one until you give it the new code too.", "New code")) set("new_code", true, "New code made"); } }, I("refresh", "sm"), "Make a new code"))),
    h("div", { class: "card col" },
      h("div", { class: "tile-head" }, I("folder"), h("span", null, "Shared folders")),
      h("div", { class: "dim" }, "Your other PC may browse and copy from these folders (nothing outside them, ever). Nothing is shared until you add a folder."),
      (t.shared_folders || []).length ? h("div", { class: "list" }, t.shared_folders.map((f) => h("div", { class: "item" }, h("div", { class: "ic" }, I("folder")), h("span", { class: "grow ell", title: f }, f),
        h("button", { class: "icon-btn sm", title: "Stop sharing", "aria-label": "Stop sharing", onclick: () => set("share_remove", f) }, I("x", "sm"))))) : null,
      h("div", { class: "row" }, h("button", { class: "btn sm", onclick: async () => { const p = await invoke("pick", { folder: true, title: "A folder your other PC may copy from" }); if (p) set("share_add", p, "Shared"); } }, I("plus", "sm"), "Add a folder")),
      (t.policies || []).length ? h("div", { class: "col", style: { gap: "6px", marginTop: "6px" } }, h("b", null, "Who may copy from this PC"), t.policies.map((p) => row(p.name, h("span", { class: "grow" }), policy(p)))) : null),
    h("div", { class: "card col" },
      h("div", { class: "tile-head" }, I("list"), h("span", null, "Folders and projects")),
      swRow("Skip build output when sending or copying a folder", t.skip_build, (v) => set("skip_build", v)),
      row("Skipped names", saveBox(t.excludes, (v) => set("excludes", v, "Saved"), { class: "grow", title: "Comma separated. A name matches any folder or file called that; *.pdb matches by pattern." })),
      row("A received folder already exists", h("span", { class: "grow" }), h("select", { onchange: (e) => set("dest_exists", e.target.value) },
        [["merge", "Merge (newer file wins)"], ["replace", "Replace same-named files"], ["both", "Keep both (new folder)"]].map(([v, l]) => h("option", { value: v, selected: t.dest_exists === v }, l))))),
    h("div", { class: "card col" },
      h("div", { class: "tile-head" }, I("terminal"), h("span", null, "Claude Code")),
      swRow("Let Claude Code use PC to PC", !!t.claude_skill, (v) => tx("claude_skill", { on: v }, v ? "Claude Code can now send to your other PCs" : "Removed from Claude Code"),
        "Adds a small skill to Claude Code so it can send files and projects (dependencies included) to your other PCs when you ask it to"),
      h("div", { class: "faint", style: { fontSize: "12px" } }, "Claude runs Deck's command line (deck.exe --pc-send …). The other PC still asks before anything comes in, unless you trust this PC there.")),
    h("div", { class: "faint", style: { fontSize: "12px" } }, `Ports UDP 47833 and TCP ${t.port || 47834}. Settings and history: ${t.data}`),
  ];
}

Pages.transfer = {
  async load() {
    S.cache.tx = await invoke("transfer_state");
    if (curSub() === "share") S.cache.share = await invoke("share_state");
    // the installed apps are listed once (a few seconds of winget), then on Rescan
    if (S.cache.tx.on && curSub() === "apps" && !S.cache.txAppsAsked) {
      S.cache.txAppsAsked = true;
      await invoke("transfer_do", { what: "apps_scan", args: {} });
    }
    if (curSub() === "apps") S.cache.txApps = await invoke("transfer_apps");
    if (curSub() === "setup" && !S.cache.txSetup) invoke("transfer_setup_preview").then((p) => { S.cache.txSetup = p; if (S.page === "transfer") render(); });
  },
  render() {
    const t = S.cache.tx || {};
    const el = h("div", null, head("PC to PC", "Send files, folders, installers, apps and your setup between your PCs. Both run Deck with the same pairing code.",
      h("label", { class: "row" }, h("span", { class: "dim" }, t.on ? "On" : "Off"), toggle(!!t.on, (v) => tx("on", { on: v })))));
    if (!t.on) {
      addTo(el, h("div", { class: "card col", style: { maxWidth: "640px" } },
        h("div", { class: "tile-head" }, I("swap"), h("span", null, "PC to PC is off")),
        h("div", { class: "dim" }, "Switch it on to find your other PC on this network and send things to it. Everything sent between your PCs is encrypted with your pairing code."),
        h("div", { class: "row" }, h("button", { class: "btn primary", onclick: () => tx("on", { on: true }) }, "Switch on"))));
      return el;
    }
    const sub = curSub();
    const waiting = (t.pending || []).length + (t.reqs || []).length;
    addTo(el, h("div", { class: "row wrap", style: { marginBottom: "14px" } },
      seg(TX_TABS.map(([v, l]) => [v, v === "received" && (t.inbox || []).length ? `${l} (${t.inbox.length})` : l]), sub, (v) => go("transfer", v)),
      waiting ? h("span", { class: "chip warn" }, I("bell", "sm"), `${waiting} waiting for you`) : null));
    const body = sub === "share" ? txShareTab() : sub === "apps" ? [txAppsTab(t)] : sub === "setup" ? [txSetupTab(t)] : sub === "received" ? [txReceivedTab(t)] : sub === "settings" ? txSettingsTab(t)
      : sub === "log" ? [h("div", { class: "card" }, h("pre", { class: "log" }, (t.logs || []).join("\n") || "Nothing yet."))] : txSendTab(t);
    addTo(el, h("div", { class: "col", style: { gap: "14px", maxWidth: "860px" } }, txPrompts(t), body));
    return el;
  },
  live: true,
  async tick() {
    const t = await invoke("transfer_state");
    if (curSub() === "share") {
      const sh = await invoke("share_state");
      const ch = JSON.stringify(sh) !== JSON.stringify(S.cache.share);
      S.cache.share = sh;
      if (ch && !S.interacting) return true;
    }
    const strip = (x) => JSON.stringify({ ...x, logs: curSub() === "log" ? x.logs : 0, reqs: (x.reqs || []).map((r) => ({ ...r, left: Math.ceil(r.left / 5) })) });
    const changed = strip(t) !== strip(S.cache.tx || {});
    S.cache.tx = t;
    if (curSub() === "apps" && (!S.cache.txApps || S.cache.txApps.busy)) {
      const a = await invoke("transfer_apps");
      if (JSON.stringify(a) !== JSON.stringify(S.cache.txApps)) { S.cache.txApps = a; return !S.interacting; }
    }
    return changed && !S.interacting && !["INPUT", "TEXTAREA", "SELECT"].includes(document.activeElement?.tagName);
  },
};

// ---------------------------------------------------------------------------------------- Workspace Sync (Claude Code)

export const CS_TABS = [["status", "Status"], ["sessions", "Sessions"], ["projects", "Projects"], ["settings", "Settings"], ["log", "Log"]];

export async function cs(what, args = {}, ok) {
  try { S.cache.cs = await run("claude_sync_do", { what, args }, ok); } catch { /* run() showed it */ }
  render();
}

export const csWhere = (alias) => (alias === "~" ? "~ (home folder)" : alias);

export function csStatusTab(c) {
  const cfg = c.cfg;
  let txt, cls;
  if (cfg.paused) { txt = "Paused"; cls = "warn"; }
  else if (c.last_error && c.phase === "Idle") { txt = `${c.last_result}, with problems`; cls = "warn"; }
  else if (c.phase === "Idle") { txt = c.last_result || "Up to date"; cls = "good"; }
  else { txt = `${c.phase}…`; cls = ""; }
  const versions = c.machines.filter((m) => m.cmp !== 0).map((m) => h("div", { class: "warn-t" }, m.same ? `${m.name} runs a different build of Deck ${m.version} (built ${m.cmp < 0 ? "earlier" : "later"} than this PC's).` : m.cmp < 0 ? `${m.name} runs an older Deck (${m.version}; this PC has ${c.version}). Update it there.` : `${m.name} runs a newer Deck (${m.version}). Update this PC to match.`));
  const sid8 = (s) => s.slice(0, 8);
  const sess = (name, sid, alias) => `${name ? `${name} (${sid8(sid)})` : sid8(sid)}${alias ? ` in ${csWhere(alias)}` : ""}`;
  return [
    h("div", { class: "card col" },
      h("div", { class: "row wrap" }, h("span", { class: "chip " + cls }, I(cls === "good" ? "check" : cls === "warn" ? "alert" : "refresh", "sm"), txt), h("span", { class: "faint" }, `Last sync ${agoMs(c.last_cycle)}`), h("span", { class: "grow" }),
        h("button", { class: "btn sm", onclick: () => cs("sync_now", {}, "Syncing") }, I("refresh", "sm"), "Sync now"),
        h("button", { class: "btn sm ghost", onclick: () => cs("pause", { paused: !cfg.paused }) }, I(cfg.paused ? "play" : "pause", "sm"), cfg.paused ? "Resume" : "Pause")),
      versions,
      (c.held || []).length ? h("div", { class: "col", role: "alert" },
        h("div", { class: "warn-t" }, `Waiting for your OK: ${c.held.map((x) => x.path).join(", ")} from ${c.held[0].from}. These would change what Claude Code runs or may do on this PC (hooks, permissions, plugins). Apply only if you made this change.`),
        h("div", { class: "row" },
          h("button", { class: "btn sm", onclick: () => cs("held", { apply: true }, "Applied") }, I("check", "sm"), "Apply"),
          h("button", { class: "btn sm ghost", onclick: () => cs("held", { apply: false }, "Kept this PC's") }, "Keep this PC's"))) : null,
      c.last_error ? h("div", { class: "bad-t" }, c.last_error) : null,
      h("div", { class: "cs-grid" },
        [["Sessions", cfg.sessions ? c.local_files[0] : "not synced"], ["Memory files", cfg.memory ? c.local_files[1] : "not synced"], ["Settings, skills, plugins", cfg.config ? c.local_files[2] : "not synced"], ["Project files", cfg.code ? c.local_files[3] : "not copied"],
        ["Total size", fmtBytes(c.local_bytes)], ["Network", !cfg.lan ? "off" : c.lan_port ? `port ${c.lan_port}` : "not listening"], ["Away from home", !cfg.lan || !cfg.tailscale ? (cfg.hub ? "cloud folder only" : "off") : tsShort(c.tailscale)], ["Cloud folder", cfg.hub ? cfg.hub_dir : "off"],
        ["Received since start", `${plural(c.received, "file")}, ${plural(c.conflicts, "conflict")}`]].map(([k, v]) => h("div", null, h("small", { class: "faint" }, k), h("b", { class: "ell", title: String(v) }, String(v)))))),
    h("div", { class: "card col" },
      h("div", { class: "tile-head" }, I("monitor"), h("span", null, "Your other PCs")),
      c.machines.length ? h("div", { class: "list" }, c.machines.map((m) => h("div", { class: "item" },
        h("span", { class: "dot " + (m.online ? "good" : "") }), h("b", { class: "grow" }, m.name),
        h("span", { class: "faint" }, `${m.via} · changed ${agoMs(m.ago)} · ${plural(m.files, "file")} · Deck ${m.version}${m.arch ? ` · ${m.arch === "ARM64" ? "ARM" : m.arch === "AMD64" ? "Intel/AMD" : m.arch}` : ""}`),
        m.other_arch && cfg.code && cfg.build ? h("span", { class: "warn-t" }, "Other processor kind: its build files aren't copied here") : null)))
        : h("div", { class: "dim" }, "None yet. Switch Workspace Sync on in Deck on the other PC and give it the same sync key (Settings tab). On the same network they find each other; away from home use Tailscale or the same cloud folder on both (Settings tab).")),
    cfg.tools ? csToolsCard(c) : null,
    h("div", { class: "card col" },
      h("div", { class: "tile-head" }, I("terminal"), h("span", null, "Open Claude Code sessions")),
      !cfg.session_lock ? h("div", { class: "faint" }, "Session lock is off (Settings): the same session can be continued on two PCs at once.") : null,
      c.hook_note ? h("div", { class: "warn-t" }, c.hook_note) : null,
      !c.open_here.length && !c.open_elsewhere.length ? h("div", { class: "faint" }, "No Claude Code sessions are open on any PC.") : null,
      h("div", { class: "list" },
        c.open_here.map((o) => h("div", { class: "item" }, h("span", { class: "dot good" }), h("span", { class: "grow ell" }, `Here: ${sess(o.name, o.sid, o.alias)}`),
          o.pending ? h("span", { class: "warn-t" }, `${o.pending} has newer messages; leave it here to get them`) : null)),
        c.open_elsewhere.map((o) => h("div", { class: "item" }, h("span", { class: "dot warn" }), h("span", { class: "grow ell" }, `On ${o.machine}: ${sess(o.name, o.sid, o.alias)}`),
          o.taken ? [h("span", { class: "chip on" }, "Taken over here"), h("button", { class: "btn sm ghost", onclick: () => cs("take_back", { sid: o.sid }) }, "Undo")]
            : h("button", { class: "btn sm", title: "Switches off the safety lock for this session. If both PCs then add messages, they can't be merged and you get two copies. Safer: close it there and /resume it here.",
              onclick: async () => { if (await confirmBox("Take this session over?", `It stays open on ${o.machine}. If both PCs add messages to it, Workspace Sync keeps two separate copies. Safer: close it there, then /resume it here.`, "Take over")) cs("take_over", { sid: o.sid }); } }, "Take over"))))),
  ];
}

/** Programs another PC has that this one doesn't, with a button that installs them. */
export function csToolsCard(c) {
  const t = c.tools || { missing: [] };
  return h("div", { class: "card col" },
    h("div", { class: "tile-head" }, I("download"), h("span", { class: "grow" }, "Programs"),
      h("button", { class: "btn sm ghost", onclick: () => cs("tools_refresh", {}, "Checking this PC") }, I("refresh", "sm"), "Check again")),
    !t.ready ? h("div", { class: "faint" }, "Checking what this PC has…")
      : !t.missing.length ? h("div", { class: "faint" }, `This PC has every program your other PCs have (${plural(t.count, "item")} checked: Rust, cargo tools, Node, npm, Python, pipx, Git).`)
      : t.missing.map((m) => h("div", { class: "col", style: { gap: "6px" } },
        h("div", { class: "row" }, h("b", { class: "grow" }, `${m.name} has, this PC doesn't:`),
          h("button", { class: "btn sm", onclick: async () => {
            if (await confirmBox(`Install ${plural(m.items.length, "program")}?`, `A PowerShell window opens and installs: ${m.items.map((x) => x.label).join(", ")}. It runs with your normal rights; nothing else is changed.`, "Install")) cs("tools_install", { machine: m.id }, "Installing in a new window");
          } }, I("download", "sm"), "Install all")),
        h("div", { class: "list" }, m.items.map((x) => h("div", { class: "item" }, h("span", { class: "grow ell" }, x.label),
          h("button", { class: "btn sm ghost", onclick: () => cs("tools_install", { machine: m.id, keys: [x.key] }, "Installing in a new window") }, "Install")))))));
}

/** One line for a status grid: Tailscale's state. */
export const tsShort = (t) => !t ? "…" : !t.installed ? "Tailscale not installed" : !t.on ? "Tailscale not connected" : `Tailscale, ${plural((t.pcs || []).filter((p) => p.online).length, "other PC")} online`;

/** Tailscale's state under its switch: get it, connect it, or which PCs it reaches. */
export function csTailscale(t, open = () => cs("tailscale_open")) {
  const small = { fontSize: "12px" };
  if (!t.installed) return h("div", { class: "row" }, h("span", { class: "faint grow", style: small }, "Tailscale (free) connects your PCs from anywhere. Install it here and on your other PC and sign in with the same account."),
    h("button", { class: "btn sm", onclick: open }, I("download", "sm"), "Get Tailscale"));
  if (!t.on) return h("div", { class: "row" }, h("span", { class: "warn-t grow", style: small }, "Tailscale is installed but not connected."),
    h("button", { class: "btn sm", onclick: open }, "Open Tailscale"));
  const pcs = t.pcs || [];
  return h("div", { class: "faint", style: small },
    `Connected${t.account ? ` as ${t.account}` : ""}${t.ip ? ` (${t.ip})` : ""}. `,
    pcs.length ? `Your other PCs on it: ${pcs.map((p) => p.online ? p.name : `${p.name} (offline)`).join(", ")}.` : "No other PC is on it yet: install Tailscale on the other PC and sign in with the same account.");
}

export function csSessionsTab(c) {
  const q = (S.cache.csQ || "").trim().toLowerCase();
  const rows = c.sessions.filter((r) => !q || [r.title, r.first, r.last, r.alias, r.sid, ...r.elsewhere].some((s) => (s || "").toLowerCase().includes(q)));
  const shown = rows.slice(0, S.cache.csMore || 60);
  const search = h("input", { type: "text", placeholder: "Search: title, words, project or id", value: S.cache.csQ || "", oninput: (e) => { S.cache.csQ = e.target.value; S.cache.csMore = 60; render(); }, style: { flex: 1 } });
  return [
    h("div", { class: "row" }, search, h("span", { class: "faint" }, `${rows.length} of ${c.sessions.length}`)),
    c.pending ? h("div", { class: "chip warn" }, I("alert", "sm"), `${plural(c.pending, "session")} open here ${c.pending === 1 ? "has" : "have"} newer messages from another PC. Close ${c.pending === 1 ? "it" : "each"} (and its VS Code tab) to get them.`) : null,
    !c.sessions.length ? h("div", { class: "empty" }, "Nothing here yet. The list fills in after the first sync.") : null,
    h("div", { class: "col", style: { gap: "8px" } }, shown.map((r) => {
      const title = r.title || r.first || "(empty session)";
      const del = async (everywhere) => {
        const names = [...(r.here ? ["this PC"] : []), ...r.elsewhere];
        const text = everywhere ? `Deletes it on ${names.join(" and ")}. A PC that is off, or has it open, deletes it once it syncs and it's closed. It can be recovered from each PC's Workspace Sync trash folder.`
          : `Deletes it on this PC only${r.elsewhere.length ? `; it stays on ${r.elsewhere.join(" and ")}` : ""}. It won't be received here again. It can be recovered from the Workspace Sync trash folder.`;
        if (await confirmBox("Delete this session?", text, "Delete", true)) cs("session_delete", { sid: r.sid, everywhere }, "Deleted");
      };
      return h("div", { class: "card col cs-sess" },
        h("div", { class: "row", style: { gap: "8px" } }, h("b", { class: "grow ell", title }, title), r.alias ? h("span", { class: "faint" }, `in ${csWhere(r.alias)}`) : null),
        h("div", { class: "row wrap", style: { gap: "10px", fontSize: "12px" } },
          r.here ? h("span", { class: "row", style: { gap: "5px" } }, h("span", { class: "dot good" }), "This PC") : null,
          r.elsewhere.map((m, i) => h("span", { class: "row", style: { gap: "5px" } }, h("span", { class: "dot " + (r.online[i] ? "good" : "") }), r.online[i] ? m : `${m} (offline)`)),
          r.open_here ? h("span", { class: "warn-t" }, "open in Claude Code here") : null,
          r.open_there ? h("span", { class: "warn-t" }, `open on ${r.open_there}`) : null),
        h("div", { class: "faint", style: { fontSize: "12px" } }, `${plural(r.prompts, "prompt")} · ${fmtBytes(r.size)} · last used ${agoMs(r.ago)} · id ${r.sid.slice(0, 8)}`),
        r.first ? h("div", { class: "dim ell", style: { fontSize: "12px" } }, "First: " + r.first) : null,
        r.last && r.last !== r.first ? h("div", { class: "dim ell", style: { fontSize: "12px" } }, "Last: " + r.last) : null,
        r.waiting ? h("div", { class: "row wrap" }, h("span", { class: "warn-t grow" }, `${r.waiting} added newer messages. Close the session in Claude Code and its tab; it updates within seconds. Then /resume it.`),
          h("button", { class: "btn sm", onclick: () => cs("sync_now", {}, "Checking") }, "Get latest")) : null,
        r.open_here ? h("div", { class: "faint", style: { fontSize: "12px" } }, "Close it in Claude Code to delete it.")
          : h("div", { class: "row" },
            r.here ? h("button", { class: "btn sm ghost", onclick: () => del(false) }, I("trash", "sm"), r.elsewhere.length ? "Delete here…" : "Delete…") : null,
            r.elsewhere.length ? h("button", { class: "btn sm ghost", onclick: () => del(true) }, I("trash", "sm"), "Delete on all PCs…") : null));
    })),
    rows.length > shown.length ? h("button", { class: "btn", onclick: () => { S.cache.csMore = (S.cache.csMore || 60) + 60; render(); } }, `Show more (${rows.length - shown.length})`) : null,
  ];
}

export function csProjectsTab(c) {
  return [
    h("div", { class: "dim" }, "Each project has a name shared by all your PCs and its own folder on each one. Paths inside synced sessions are rewritten to this PC's folder, so /resume works here. Moving a project moves its sessions: close Claude Code in that project first."),
    ...c.projects.map((p) => h("div", { class: "card col" },
      h("div", { class: "row" }, h("b", { class: "grow" }, csWhere(p.alias)), h("span", { class: "faint" }, plural(p.sessions, "session") + " here")),
      row("Folder on this PC", h("code", { class: "grow ell", title: p.path }, p.path),
        p.home ? null : h("button", { class: "btn sm", onclick: async () => { const d = await invoke("pick", { folder: true, title: `Where is ${p.alias} on this PC?` }); if (d) cs("remap", { alias: p.alias, path: d }, "Moving…"); } }, "Move…")),
      p.auto ? h("div", { class: "row wrap" }, h("span", { class: "warn-t grow" }, `Guessed from ${p.origin}. Keep this folder, or Move it to where the project lives on this PC.${c.cfg.code ? " Its files arrive once you choose." : ""}`),
        h("button", { class: "btn sm", onclick: () => cs("remap", { alias: p.alias, path: p.path }, "Kept") }, "Keep this folder"))
        : !p.exists ? h("div", { class: "warn-t" }, "That folder doesn't exist on this PC") : null,
      p.elsewhere.map((e) => h("div", { class: "faint", style: { fontSize: "12px" } }, `On ${e.machine}: ${e.path}`)),
      swRow("Sync this project", p.sync, (v) => cs("project_sync", { alias: p.alias, on: v })),
      !p.sync ? h("div", { class: "faint", style: { fontSize: "12px" } }, "Not syncing: its sessions, memory and files stay on this PC only, and nothing arrives for it.") : null,
      p.sync ? swRow("Its sessions (conversations)", p.sessions_on, (v) => cs("project_sessions", { alias: p.alias, on: v })) : null,
      p.sync && !p.sessions_on ? h("div", { class: "faint", style: { fontSize: "12px" } }, "Its memory (and files, if copied) still sync, so you can start a new session on the other PC and carry on with the project.") : null,
      p.sync && c.cfg.code && !p.home ? swRow("Its files (the project folder)", p.code_on, (v) => cs("project_code", { alias: p.alias, on: v })) : null,
      p.sync && c.cfg.code && p.code_on && p.code_skipped ? h("div", { class: "warn-t" }, `Files not copied: ${p.code_skipped}.`) : null,
      p.sync && c.cfg.code && p.code_on && p.exists && !p.has_git && p.git_from ? h("div", { class: "row wrap" },
        h("span", { class: "faint grow", style: { fontSize: "12px" } }, `Git history isn't here (it isn't copied with the files). ${p.git_from.machine} gets it from ${p.git_from.url}, branch ${p.git_from.branch}.`),
        h("button", { class: "btn sm", onclick: async () => { if (await confirmBox("Set up git here?", `A PowerShell window connects this folder to ${p.git_from.url} and downloads its history. Your files stay as they are; git then shows what differs.`, "Set up")) cs("git_setup", { alias: p.alias }, "Setting up git in a new window"); } }, "Get git history")) : null,
      p.home ? null : h("div", { class: "row" }, h("button", { class: "btn sm ghost", onclick: async () => {
        if (await confirmBox(`Delete ${p.alias} everywhere?`, "Its Claude Code sessions and memory are deleted on this PC and every other connected one. Your project files aren't touched. Close Claude Code in this project first. Deleted items go to Workspace Sync's trash folder.", "Delete everywhere", true)) cs("project_delete", { alias: p.alias }, "Deleting…");
      } }, I("trash", "sm"), "Delete everywhere…")))),
  ];
}

export function csSettingsTab(c) {
  const cfg = c.cfg;
  const set = (key, value, ok) => cs("set", { key, value }, ok);
  return [
    h("div", { class: "card col" },
      h("div", { class: "tile-head" }, I("monitor"), h("span", null, "This PC")),
      row("Name", saveBox(cfg.name, (v) => set("name", v, "Saved"))),
      row("Sync key", h("b", { class: "num grow", style: { userSelect: "text" } }, S.cache.csShowKey ? cfg.key : "•".repeat(Math.min(24, cfg.key.length))),
        h("button", { class: "icon-btn sm", title: S.cache.csShowKey ? "Hide" : "Show", "aria-label": S.cache.csShowKey ? "Hide" : "Show", onclick: () => { S.cache.csShowKey = !S.cache.csShowKey; render(); } }, I(S.cache.csShowKey ? "eyeoff" : "eye", "sm")),
        h("button", { class: "icon-btn sm", title: "Copy", "aria-label": "Copy", onclick: () => copyAnswer(cfg.key) }, I("copy", "sm"))),
      row("Use the other PC's key", saveBox("", (v) => v && set("key", v, "Key saved"), { class: "grow", placeholder: "paste it here" })),
      h("div", { class: "row" }, h("div", { class: "faint grow", style: { fontSize: "12px" } }, "Type the same key on every PC. It encrypts everything they send each other."),
        h("button", { class: "btn sm ghost", onclick: async () => { if (await confirmBox("Make a new sync key?", "Your other PCs stop syncing until you give them the new key too.", "New key")) { S.cache.csShowKey = true; set("new_key", true, "New key made"); } } }, I("refresh", "sm"), "New key"))),
    h("div", { class: "card col" },
      h("div", { class: "tile-head" }, I("wifi"), h("span", null, "How your PCs reach each other")),
      swRow("Directly, on the same network", cfg.lan, (v) => set("lan", v)),
      cfg.lan ? swRow("Over Tailscale, from anywhere (free; both PCs need to be on)", cfg.tailscale, (v) => set("tailscale", v)) : null,
      cfg.lan && cfg.tailscale ? csTailscale(c.tailscale || {}) : null,
      row("Other PC's address", saveBox(cfg.manual_peers, (v) => set("manual_peers", v, "Saved"), { class: "grow", placeholder: "only if they don't find each other, e.g. LAPTOP or 192.168.1.20" })),
      swRow("Also through a cloud folder (the other PC catches up later, even if this one is off)", cfg.hub, (v) => set("hub", v)),
      cfg.hub ? row("Cloud folder", h("code", { class: "grow ell", title: cfg.hub_dir }, cfg.hub_dir || "not chosen"),
        c.onedrive ? h("button", { class: "btn sm ghost", onclick: () => set("hub_dir", c.onedrive, "Saved") }, "Use OneDrive") : null,
        h("button", { class: "btn sm", onclick: async () => { const d = await invoke("pick", { folder: true, title: "A folder OneDrive, Google Drive or Dropbox syncs" }); if (d) set("hub_dir", d, "Saved"); } }, "Choose…")) : null,
      cfg.hub ? h("div", { class: "faint", style: { fontSize: "12px" } }, "Any folder a sync app keeps up to date works: OneDrive, Google Drive for desktop, Dropbox, MEGA, pCloud or Syncthing. A Deck-ClaudeSync-hub folder is made inside it. Pick the same one on every PC; what's in it is encrypted with the sync key.") : null),
    h("div", { class: "card col" },
      h("div", { class: "tile-head" }, I("list"), h("span", null, "What to sync")),
      swRow("Sessions (conversations, subagents, file history for /rewind)", cfg.sessions, (v) => set("sessions", v)),
      swRow("Memory (each project's memory folder)", cfg.memory, (v) => set("memory", v)),
      swRow("Settings, CLAUDE.md, skills, agents, commands, plugins, MCP servers", cfg.config, (v) => set("config", v)),
      h("div", { class: "faint", style: { fontSize: "12px" } }, "Sign-in details and per-PC files (.credentials.json, settings.local.json, caches) are never synced. Hooks, permissions, plugins and MCP servers from another PC wait for your OK."),
      swRow("Project files (each project's folder, so the code is there too)", cfg.code, (v) => set("code", v)),
      cfg.code ? h("div", { class: "faint", style: { fontSize: "12px" } }, "Leaves out .git, Claude Code's settings.local.json and build output (target, node_modules, .venv, venv, __pycache__, dist, build, obj, .next…). Very big folders (over 40,000 files or 3 GB) aren't copied. Changes to a project's .claude settings, hooks or .mcp.json wait for your OK. Turn single projects off under Projects. Switching this off deletes nothing.") : null,
      cfg.code ? swRow("Also build files (target, node_modules, .venv…)", cfg.build, (v) => set("build", v)) : null,
      cfg.code && cfg.build ? h("div", { class: "warn-t" }, "Build folders can be several GB and change on every build. Usually it's quicker to build again on the other PC.") : null,
      swRow("Programs: show what the other PCs have that this one is missing (Rust, Node, Python, Git…)", cfg.tools, (v) => set("tools", v)),
      row("Check every", h("span", { class: "grow" }), h("select", { onchange: (e) => set("interval", +e.target.value) },
        [[10, "10 seconds"], [20, "20 seconds"], [60, "minute"], [300, "5 minutes"]].map(([v, l]) => h("option", { value: v, selected: cfg.interval === v }, l))))),
    h("div", { class: "card col" },
      h("div", { class: "tile-head" }, I("terminal"), h("span", null, "Claude Code")),
      swRow("Don't let a session open on one PC be continued on another", cfg.session_lock, (v) => set("session_lock", v)),
      h("div", { class: "faint", style: { fontSize: "12px" } }, "Adds a small hook to Claude Code: a message in a session that's open elsewhere is stopped with an explanation. Close it there, then /resume it here."),
      swRow("Tell Claude which PC it's on at the start of every session", cfg.machine_note, (v) => set("machine_note", v)),
      h("div", { class: "faint", style: { fontSize: "12px" } }, "Uses the name above, and says when a session was used on another PC, so Claude doesn't mix up hardware, files and paths.")),
    h("div", { class: "row" }, h("span", { class: "faint grow", style: { fontSize: "12px" } }, `Claude folder: ${c.claude_dir}. Ports UDP 47835 and TCP ${c.lan_port || 47836}.`),
      h("button", { class: "btn sm ghost", onclick: () => cs("open_data") }, I("folder", "sm"), "Data folder")),
  ];
}

Pages.claudesync = {
  async load() { S.cache.cs = await invoke("claude_sync_state"); },
  render() {
    const c = S.cache.cs || {};
    const el = h("div", null, head("Workspace Sync", "For Claude Code. Keeps your sessions, memory, settings, project files and programs the same on your PCs, so you can carry on where you left off.",
      h("label", { class: "row" }, h("span", { class: "dim" }, c.on ? "On" : "Off"), toggle(!!c.on, (v) => cs("on", { on: v }, v ? "Workspace Sync is on" : "Workspace Sync is off")))));
    if (!c.on) {
      addTo(el, h("div", { class: "card col", style: { maxWidth: "680px" } },
        h("div", { class: "tile-head" }, I("terminal"), h("span", null, "Workspace Sync is off")),
        h("div", { class: "dim" }, "For people who use Claude Code on more than one PC. When it's on, Deck syncs your Claude Code conversations, memory and settings with your other PCs that run Deck (directly on your network, or through a cloud folder), and rewrites project paths so /resume works on each PC. It can also copy your project folders and tell you which programs (Rust, Node, Python…) a PC is missing."),
        h("ul", { class: "dim", style: { margin: 0, paddingLeft: "18px", lineHeight: 1.7 } },
          h("li", null, "Encrypted with a sync key only your PCs know"),
          h("li", null, "Works at home, from anywhere with Tailscale, or through a cloud folder (OneDrive, Google Drive, Dropbox…)"),
          h("li", null, "Sign-in details are never synced"),
          h("li", null, "Adds two small hooks to Claude Code's settings (removed again when you switch it off)")),
        c.standalone ? h("div", { class: "chip warn" }, I("alert", "sm"), "The ClaudeSync app is running here. Close it first (tray icon → Exit).") : null,
        h("div", { class: "row" }, h("button", { class: "btn primary", onclick: () => cs("on", { on: true }, "Workspace Sync is on") }, "Switch on"))));
      return el;
    }
    const sub = curSub();
    addTo(el, h("div", { class: "row wrap", style: { marginBottom: "14px" } }, seg(CS_TABS, sub, (v) => go("claudesync", v))));
    const body = sub === "sessions" ? csSessionsTab(c) : sub === "projects" ? csProjectsTab(c) : sub === "settings" ? csSettingsTab(c)
      : sub === "log" ? [h("div", { class: "card" }, h("pre", { class: "log" }, (c.logs || []).join("\n") || "Nothing yet."))] : csStatusTab(c);
    addTo(el, h("div", { class: "col", style: { gap: "14px", maxWidth: "860px" } }, body));
    return el;
  },
  live: true,
  async tick() {
    const c = await invoke("claude_sync_state");
    const strip = (x) => JSON.stringify({ ...x, logs: curSub() === "log" ? x.logs : 0 });
    const changed = strip(c) !== strip(S.cache.cs || {});
    S.cache.cs = c;
    return changed && !S.interacting && !["INPUT", "TEXTAREA", "SELECT"].includes(document.activeElement?.tagName);
  },
};
