// Maintenance → Tweaks: Windows settings with their live state and an exact undo, built-in apps,
// and Store apps without the Microsoft Store. Backend: src-tauri/src/tweaks (design:
// docs/design/specs/2026-10-20-tweaks-design.md).
import { I, Pages, S, confirmBox, fmtBytes, h, invoke, modal, note, run, seg, toggle } from "./core.js";
import { head } from "./pages1.js";
import { go, render } from "./app.js";

const RISK = { safe: null, behaviour: ["Changes behaviour", ""], advanced: ["Advanced", "bad"] };
const AFTER = { explorer: "Restarts Explorer", signout: "Sign out to finish", restart: "Restart to finish" };
const view = () => S.sub || "overview";
const st = () => S.cache.tw || { tweaks: [], cats: [], presets: [], pending: {} };
const shown = (t) => !t.hidden;
let rpAsked = false; // a restore point was offered this session
let openDetails = new Set();

async function reload() {
  S.cache.tw = await invoke("tweaks_state");
  if (S.page === "tweaks" || S.page === "programs") render();
}

// ------------------------------------------------------------------------------------- changing

/** Before the first Advanced change (and app removal) of a session: offer a restore point. */
function restorePointFirst() {
  if (rpAsked || st()?.restore_point?.made) return Promise.resolve(true);
  return new Promise((resolve) => {
    modal("Make a restore point first?", h("div", { class: "col", style: { gap: "8px" } },
      h("p", { class: "dim", style: { margin: 0 } }, "Advanced tweaks and removing apps can break a feature. A System Restore point lets Windows put everything back, even if Deck can't start."),
      h("p", { class: "faint small", style: { margin: 0 } }, "Deck's own Undo still works either way. Windows makes at most one restore point a day.")),
    (close) => [
      h("button", { class: "btn", onclick: () => { close(); resolve(false); } }, "Cancel"),
      h("button", { class: "btn", onclick: () => { rpAsked = true; close(); resolve(true); } }, "Continue without"),
      h("button", { class: "btn primary", onclick: async (e) => {
        e.target.disabled = true; e.target.textContent = "Making one…";
        try {
          const r = await invoke("tweaks_do", { what: "restore_point" });
          note(r.made ? "Restore point made" : "Windows made none (it allows one a day); continuing");
        } catch (err) { note(String(err), true); }
        rpAsked = true; close(); resolve(true);
      } }, "Make one and continue")]);
  });
}

async function apply(changes, list) {
  const advanced = changes.some(([id]) => list.find((t) => t.id === id)?.risk === "advanced");
  if (advanced && !(await restorePointFirst())) return;
  try {
    const r = await invoke("tweaks_set", { changes });
    if (r.failed.length) note(r.failed.length === 1 ? `${r.failed[0].name}: ${r.failed[0].error}` : `${r.failed.length} changes failed: ${r.failed.map((f) => f.name).join(", ")}`, true);
    else if (changes.length > 1) note(`${changes.length} changes made`);
  } catch (e) { note(String(e), true); }
  await reload();
}
const set = (t, opt) => apply([[t.id, opt]], [t]);

async function tdo(what, id, ok) {
  try { const r = await run("tweaks_do", { what, id }, ok); await reload(); return r; } catch { await reload(); return null; }
}

/** A checklist of changes (a preset, the recommended ones): untick any, then apply. */
function checklist(title, what, changes) {
  const list = st().tweaks;
  if (!changes.length) return note("Nothing to change: this PC already matches");
  const picked = new Set(changes.map(([id]) => id));
  const rows = changes.map(([id, opt]) => {
    const t = list.find((x) => x.id === id);
    const to = t.options.find((o) => o.id === opt)?.label || opt;
    const what = t.tick ? h("b", null, opt === t.default ? `Put back: ${t.name}` : t.tick) : [h("b", null, t.name), h("span", { class: "faint" }, " → ", to)];
    return h("label", { class: "tw-check" },
      h("input", { type: "checkbox", checked: true, onchange: (e) => { e.target.checked ? picked.add(id) : picked.delete(id); } }),
      h("span", { class: "grow" }, what), badges(t, true));
  });
  modal(title, h("div", { class: "col", style: { gap: "8px" } }, what ? h("p", { class: "dim", style: { margin: 0 } }, what) : null, h("div", { class: "tw-checks" }, rows)),
    (close) => [h("button", { class: "btn", onclick: close }, "Cancel"),
      h("button", { class: "btn primary", onclick: () => { close(); apply(changes.filter(([id]) => picked.has(id)), list); } }, "Apply")], { wide: changes.length > 8 });
}

// ------------------------------------------------------------------------------------- one tweak

function badges(t, short) {
  const out = [];
  const r = RISK[t.risk];
  if (r) out.push(h("span", { class: "chip " + r[1] }, r[0]));
  if (!short && AFTER[t.after]) out.push(h("span", { class: "chip" }, AFTER[t.after]));
  if (!short && t.partial) out.push(h("span", { class: "chip warn", title: "Something else set part of this (another tool, an old tweak). Flip it to set it completely." }, "Partly · " + t.partial));
  return out;
}

function control(t) {
  if (t.kind === "link") {
    const [p, s] = (t.page || "").split("/");
    return h("button", { class: "btn sm", onclick: () => p === "open:SystemPropertiesPerformance.exe" ? tdo("open", "visualfx") : go(p, s) }, I("external", "sm"), t.label);
  }
  if (t.kind === "action") {
    return t.state === "done"
      ? h("button", { class: "btn sm", onclick: () => tdo("undo", t.id, "Put back") }, I("undo", "sm"), "Undo")
      : h("button", { class: "btn sm", disabled: !!t.blocked, onclick: () => tdo("action", t.id, "Done") }, t.label || "Run");
  }
  if (t.state == null) return h("span", { class: "faint small" }, "Checking…");
  const cur = t.state === "mixed" ? t.lean : t.state;
  if (t.kind === "switch") {
    const el = toggle(cur === "on", (v) => set(t, v ? "on" : "off"));
    if (t.blocked) el.querySelector("input").disabled = true;
    return el;
  }
  const label = (o) => o.id === t.default ? `${o.label} (Windows default)` : o.label;
  // a choice: buttons when they fit, a list otherwise; "Custom" when the PC has a value of its own
  const short = t.options.length <= 4 && t.options.reduce((n, o) => n + o.label.length, 0) <= 38;
  if (short && cur) {
    const el = seg(t.options.map((o) => [o.id, o.label]), cur, (v) => v !== cur && set(t, v));
    if (t.blocked) el.querySelectorAll("button").forEach((b) => (b.disabled = true));
    return el;
  }
  return h("select", { disabled: !!t.blocked, "aria-label": t.name, onchange: (e) => set(t, e.target.value) },
    cur ? null : h("option", { value: "", selected: true, disabled: true }, "Custom"),
    t.options.map((o) => h("option", { value: o.id, selected: o.id === cur }, label(o))));
}

function details(t) {
  return h("div", { class: "tw-details" },
    t.more ? h("p", null, t.more) : null,
    t.details.map((d) => h("div", { class: "tw-opt" }, h("b", null, d.label), d.lines.map((l) => h("code", null, l)))),
    h("div", { class: "faint small" }, "Source: ", t.src, t.at ? ` · changed by Deck ${new Date(t.at * 1000).toLocaleDateString()}` : ""));
}

/** What you gain and what you give up, in plain words. */
function plain(t) {
  if (!t.good && !t.lose) return null;
  return h("div", { class: "tw-plain" },
    t.good ? h("div", { class: "tw-good" }, I("check", "sm"), h("span", null, h("b", null, t.kind === "switch" ? "Why tick it: " : "Why change it: "), t.good)) : null,
    t.lose ? h("div", { class: "tw-lose" }, I("alert", "sm"), h("span", null, h("b", null, "What you lose: "), t.lose)) : null);
}

/** A switch as Winaero shows it: worded as what Deck does, ticked when that's done. Unticked is
 * always Windows' own setting, whichever way round the setting itself goes. */
function tickRow(t) {
  const cur = t.state === "mixed" ? t.lean : t.state;
  const other = t.options.find((o) => o.id !== t.default)?.id;
  const box = h("input", { type: "checkbox", checked: cur != null && cur !== t.default, disabled: !!t.blocked || t.state == null,
    onchange: (e) => set(t, e.target.checked ? other : t.default) });
  if (t.state === "mixed") box.indeterminate = true;
  return h("label", { class: "tw-main tw-tick" }, box,
    h("div", { class: "grow" },
      h("div", { class: "tw-name" }, t.tick, badges(t)),
      h("div", { class: "tw-what" }, t.what),
      plain(t)),
    t.state == null ? h("span", { class: "faint small" }, "Checking…") : null);
}

function row(t) {
  const open = openDetails.has(t.id);
  const notes = [];
  if (t.changed_back) notes.push(h("div", { class: "tw-note warn" }, I("alert", "sm"), h("span", { class: "grow" }, "Windows changed this back (an update, usually)."),
    h("button", { class: "btn sm", onclick: () => set(t, t.applied) }, "Apply again"), h("button", { class: "btn sm ghost", onclick: () => tdo("forget", t.id) }, "Forget")));
  else if (t.changed && t.kind !== "action") notes.push(h("div", { class: "tw-note" }, I("check", "sm"), h("span", { class: "grow" }, "Changed by Deck."),
    h("button", { class: "btn sm ghost", onclick: () => tdo("undo", t.id, "Put back as it was") }, I("undo", "sm"), "Undo")));
  if (t.blocked) notes.push(h("div", { class: "tw-note faint" }, I("lock", "sm"), h("span", null, t.blocked)));
  const def = t.kind === "choice" ? t.options.find((o) => o.id === t.default) : null;
  return h("div", { class: "tw-row" + (t.changed ? " changed" : "") },
    t.kind === "switch" && t.tick ? tickRow(t) : h("div", { class: "tw-main" },
      h("div", { class: "grow" },
        h("div", { class: "tw-name" }, t.name, badges(t)),
        h("div", { class: "tw-what" }, t.what, def ? h("span", { class: "faint" }, ` Windows default: ${def.label}.`) : null),
        plain(t)),
      h("div", { class: "tw-ctl" }, control(t))),
    notes,
    t.kind === "link" ? null : h("button", { class: "tw-more", "aria-expanded": String(open), onclick: () => { open ? openDetails.delete(t.id) : openDetails.add(t.id); render(); } },
      I(open ? "minus" : "plus", "sm"), open ? "Hide details" : "What it changes"),
    open ? details(t) : null);
}

function list(tweaks) {
  const out = [];
  let group;
  for (const t of tweaks) {
    if (t.group && t.group !== group) out.push(h("div", { class: "tw-group" }, t.group));
    group = t.group;
    out.push(row(t));
  }
  return out;
}

// ------------------------------------------------------------------------------------- views

const counts = (cat) => {
  const ts = st().tweaks.filter((t) => t.cat === cat && shown(t));
  return [ts.filter((t) => t.changed).length, ts.length];
};

function nav() {
  const v = view(), x = st();
  const item = (id, icon, label, extra, cls) => h("button", { class: "tw-nav-i" + (v === id ? " on" : "") + (cls ? " " + cls : ""), "aria-current": v === id ? "page" : null, onclick: () => go("tweaks", id) },
    I(icon, "sm"), h("span", { class: "grow" }, label), extra != null ? h("span", { class: "tw-count" }, extra) : null);
  const changed = x.tweaks.filter((t) => t.changed).length, back = x.tweaks.filter((t) => t.changed_back).length;
  return h("nav", { class: "tw-nav", "aria-label": "Tweak categories" },
    item("overview", "grid", "Overview"),
    h("div", { class: "tw-nav-sep" }),
    x.cats.map((c) => { const [n, all] = counts(c.id); return all ? item(c.id, c.icon, c.name, n ? `${n}/${all}` : all) : null; }),
    h("div", { class: "tw-nav-sep" }),
    item("changed", "undo", "Changed by Deck", changed || null),
    back ? item("back", "alert", "Changed back", back, "warn") : null);
}

function filters() {
  const f = S.cache.twFilter || "all";
  return h("div", { class: "row wrap tw-filters" },
    h("input", { type: "search", class: "grow", placeholder: "Search every tweak…", "aria-label": "Search tweaks", value: S.cache.twQ || "",
      oninput: (e) => { S.cache.twQ = e.target.value; render(); const i = document.querySelector(".tw-filters input"); if (i) { i.focus(); i.setSelectionRange(i.value.length, i.value.length); } } }),
    seg([["all", "All"], ["changed", "Changed"], ["safe", "Safe only"]], f, (v) => { S.cache.twFilter = v; render(); }),
    h("div", { class: "faint small", style: { flexBasis: "100%" } }, "Ticked: Deck changed it. Unticked: Windows' own setting. Untick to put it back."));
}

function filtered(ts) {
  const q = (S.cache.twQ || "").trim().toLowerCase(), f = S.cache.twFilter || "all";
  return ts.filter((t) => shown(t)
    && (f !== "changed" || t.changed) && (f !== "safe" || t.risk === "safe")
    && (!q || (t.name + " " + t.what + " " + (t.group || "")).toLowerCase().includes(q)));
}

function catView(cat) {
  const x = st();
  const q = (S.cache.twQ || "").trim();
  // a search covers every category
  const ts = filtered(q ? x.tweaks : x.tweaks.filter((t) => t.cat === cat));
  const hidden = x.tweaks.filter((t) => t.cat === cat && !shown(t)).length;
  const body = [];
  if (q) {
    for (const k of x.cats) {
      const part = ts.filter((t) => t.cat === k.id);
      if (part.length) body.push(h("div", { class: "tw-group" }, k.name), list(part));
    }
  } else body.push(list(ts));
  return [filters(), h("div", { class: "tw-list" }, body.length && ts.length ? body : h("div", { class: "empty" }, q ? "No tweak matches." : "Nothing here with this filter.")),
    !q && hidden ? h("div", { class: "faint small", style: { marginTop: "8px" } }, `${hidden} more ${hidden === 1 ? "isn't" : "aren't"} for this PC or this Windows version.`) : null];
}

function overview() {
  const x = st();
  const ts = x.tweaks.filter(shown);
  const changed = ts.filter((t) => t.changed), back = ts.filter((t) => t.changed_back);
  const rec = ts.filter((t) => t.rec && t.risk === "safe" && t.state && t.state !== t.rec && !t.blocked);
  const card = (icon, title, big, text, ...buttons) => h("div", { class: "card tw-ov" }, h("div", { class: "tile-head" }, I(icon), h("span", null, title)), h("div", { class: "tw-big" }, big), h("div", { class: "dim small" }, text), h("div", { class: "row wrap" }, buttons));
  const presetChanges = (p) => Object.entries(p.set).filter(([id, opt]) => { const t = ts.find((k) => k.id === id); return t && !t.blocked && t.state !== opt; });
  return [
    x.journal_set_aside ? h("div", { class: "tw-note warn", role: "alert" }, I("alert", "sm"), h("span", { class: "grow" },
      "Deck's undo list was changed by another program, so it was set aside (tweaks.changed-outside.json) and isn't used. Tweaks made before can't be undone from here; your PC's settings weren't touched.")) : null,
    h("div", { class: "tw-ov-grid" },
      card("undo", "Changed by Deck", changed.length, changed.length ? "Each can be undone to exactly what it was." : "Nothing yet. Every change can be undone.",
        changed.length ? h("button", { class: "btn sm", onclick: () => go("tweaks", "changed") }, "See them") : null,
        changed.length ? h("button", { class: "btn sm", onclick: () => modal("Undo every tweak?", h("p", { class: "dim" }, `Puts all ${changed.length} back exactly as they were before Deck changed them.`),
          (close) => [h("button", { class: "btn", onclick: close }, "Cancel"), h("button", { class: "btn primary", onclick: async () => { close(); const r = await tdo("undo_all"); if (r) note(r.failed.length ? `${r.undone} undone; ${r.failed.join("; ")}` : `${r.undone} undone`, !!r.failed.length); } }, "Undo all")]) }, I("undo", "sm"), "Undo all") : null),
      card("alert", "Windows changed back", back.length, back.length ? "Usually after a Windows update." : "Deck checks every 6 hours.",
        back.length ? h("button", { class: "btn sm primary", onclick: () => apply(back.map((t) => [t.id, t.applied]), ts) }, "Apply them again") : null),
      card("sparkle", "Recommended", rec.length, rec.length ? "Safe ones this PC doesn't have yet." : "This PC has every recommended safe tweak.",
        rec.length ? h("button", { class: "btn sm primary", onclick: () => checklist("Recommended tweaks", "Only Safe tweaks: nothing breaks, each can be undone. Untick any you want to keep.", rec.map((t) => [t.id, t.rec])) }, "Review and apply") : null)),
    h("h2", { class: "tw-h2" }, "Presets"),
    h("div", { class: "tw-presets" }, x.presets.map((p) => {
      const n = presetChanges(p).length;
      return h("button", { class: "card tw-preset", onclick: () => checklist(p.name, p.what + " Untick any you want to keep.", presetChanges(p)) },
        h("b", null, p.name), h("span", { class: "dim small" }, p.what), h("span", { class: n ? "chip on" : "chip good" }, n ? `${n} change${n > 1 ? "s" : ""}` : "Already set"));
    })),
    h("h2", { class: "tw-h2" }, "Categories"),
    h("div", { class: "tw-cats" }, x.cats.map((c) => { const [n, all] = counts(c.id); return all ? h("button", { class: "card tw-cat", onclick: () => go("tweaks", c.id) }, I(c.icon), h("b", { class: "grow" }, c.name), h("span", { class: "faint small" }, n ? `${n} changed · ${all}` : all)) : null; })),
    h("p", { class: "faint small", style: { marginTop: "14px" } }, "Recipes from Win11Debloat and Chris Titus Tech's WinUtil (MIT), checked one by one, plus Deck's own. Deck never removes Edge or WebView2, turns Windows Update or Defender off, or runs scripts from the internet."),
  ];
}

function changedView(back) {
  const ts = st().tweaks.filter((t) => shown(t) && (back ? t.changed_back : t.changed));
  return h("div", { class: "tw-list" }, ts.length ? list(ts) : h("div", { class: "empty" }, back ? "Windows hasn't changed anything back." : "Deck hasn't changed anything yet."));
}

// ------------------------------------------------------------------------------------- built-in apps

export function appsView() {
  const a = S.cache.twApps;
  if (!a) {
    invoke("tweaks_apps").then((r) => { S.cache.twApps = r; render(); }).catch((e) => note(String(e), true));
    return h("div", { class: "empty" }, "Reading your apps…");
  }
  const showAll = !!S.cache.twAppsAll, q = (S.cache.twAppsQ || "").toLowerCase();
  const apps = a.apps.filter((x) => (showAll || x.rating !== "unsafe") && (!q || (x.name + " " + x.what).toLowerCase().includes(q)));
  const act = async (what, x) => {
    if (what === "remove") {
      if (!(await restorePointFirst())) return;
      const ok = await new Promise((res) => modal(`Remove ${x.name}?`, h("p", { class: "dim" }, x.rating === "unsafe" ? "Windows or other apps use this one: something may stop working." : "It's removed for your account. Reinstall puts it back from Microsoft's servers."),
        (close) => [h("button", { class: "btn", onclick: () => { close(); res(false); } }, "Cancel"), h("button", { class: "btn danger", onclick: () => { close(); res(true); } }, "Remove")]));
      if (!ok) return;
    }
    S.cache.twAppBusy = x.name; render();
    try { await run("tweaks_app", { what, name: x.name }, what === "remove" ? `${x.name} removed` : `Reinstalling ${x.name}: see Get Store apps`); } catch { /* shown */ }
    S.cache.twAppBusy = null;
    S.cache.twApps = await invoke("tweaks_apps");
    render();
  };
  const groups = [["safe", "Safe to remove"], ["optional", "Optional: you may use these"], ["unsafe", "Windows or other apps need these"]];
  const card = (x) => h("div", { class: "card tw-app" + (x.rating === "unsafe" ? " unsafe" : "") },
    h("div", { class: "grow" }, h("b", null, x.name), h("div", { class: "dim small" }, x.what)),
    x.installed
      ? h("button", { class: "btn sm", disabled: S.cache.twAppBusy === x.name, onclick: () => act("remove", x) }, I("trash", "sm"), S.cache.twAppBusy === x.name ? "Removing…" : "Remove")
      : h("button", { class: "btn sm primary", disabled: S.cache.twAppBusy === x.name, onclick: () => act("reinstall", x) }, I("download", "sm"), "Reinstall"));
  return [
    h("div", { class: "row wrap tw-filters" },
      h("input", { type: "search", class: "grow", placeholder: "Search your built-in apps…", "aria-label": "Search built-in apps", value: S.cache.twAppsQ || "",
        oninput: (e) => { S.cache.twAppsQ = e.target.value; render(); const i = document.querySelector(".tw-filters input"); if (i) { i.focus(); i.setSelectionRange(i.value.length, i.value.length); } } }),
      h("label", { class: "row small" }, toggle(showAll, (v) => { S.cache.twAppsAll = v; render(); }), "Show apps Windows needs"),
      h("button", { class: "btn sm ghost", onclick: async () => { S.cache.twApps = null; render(); } }, I("refresh", "sm"), "Refresh")),
    groups.map(([r, title]) => {
      const part = apps.filter((x) => x.rating === r);
      return part.length ? [h("h2", { class: "tw-h2" }, title), h("div", { class: "tw-apps" }, part.map(card))] : null;
    }),
    apps.length ? null : h("div", { class: "empty" }, "None of the listed apps are installed."),
    h("p", { class: "faint small", style: { marginTop: "12px" } }, "Win11Debloat's list (MIT). Edge isn't on it: Deck and many apps run on its WebView2."),

  ];
}

// ------------------------------------------------------------------------------------- Store apps

const fmtTime = (s) => s >= 3600 ? `${Math.floor(s / 3600)} h ${Math.round(s % 3600 / 60)} min` : s >= 60 ? `${Math.floor(s / 60)} min ${s % 60} s` : `${s} s`;
const PHASE = { finding: "Asking Microsoft's servers…", downloading: "Downloading", installing: "Installing…", done: "Done", failed: "Failed", cancelled: "Cancelled", interrupted: "Interrupted" };

function job(j) {
  const pct = j.total ? Math.min(100, (j.done / j.total) * 100) : 0;
  const active = ["finding", "downloading", "installing"].includes(j.phase);
  const files = (j.files || []).filter((f) => !f.have);
  return h("div", { class: "card tw-job" + (j.phase === "failed" ? " failed" : "") },
    h("div", { class: "row" }, h("b", { class: "grow" }, j.title), h("span", { class: "chip " + (j.phase === "done" ? "good" : j.phase === "failed" ? "bad" : "") }, j.phase === "done" && j.mode === "save" ? "Saved" : j.phase === "done" ? "Installed" : PHASE[j.phase] || j.phase),
      active ? h("button", { class: "btn sm ghost", onclick: () => invoke("tweaks_store", { what: "cancel", id: j.id }).then(pollJobs) }, "Cancel")
        // what was downloaded is kept: this carries on from there
        : ["cancelled", "failed", "interrupted"].includes(j.phase) ? h("button", { class: "btn sm", onclick: () => storeAct(j.mode, { id: j.id, title: j.title }) }, I("refresh", "sm"), j.phase === "failed" ? "Try again" : "Carry on") : null),
    j.phase === "interrupted" ? h("div", { class: "small dim" }, `Deck was closed while this was ${j.done ? `downloading (${fmtBytes(j.done)} of ${fmtBytes(j.total)} is kept)` : "starting"}. Carry on picks up from there.`) : null,
    j.phase === "downloading" || (j.phase !== "finding" && j.total) ? h("div", { class: "tw-pbar", role: "progressbar", "aria-valuenow": Math.round(pct), "aria-valuemin": 0, "aria-valuemax": 100, "aria-label": "Download progress" }, h("i", { style: { width: pct + "%" } })) : null,
    j.phase === "downloading" ? h("div", { class: "dim small" }, `${fmtBytes(j.done)} of ${fmtBytes(j.total)} · ${fmtBytes(j.speed)}/s${j.eta != null ? " · " + fmtTime(j.eta) + " left" : ""}`, files.length > 1 ? ` · ${j.file}` : "") : null,
    j.error ? h("div", { class: "small", style: { color: "var(--bad)" } }, j.error) : null,
    j.saved_to ? h("div", { class: "row small" }, h("span", { class: "grow dim" }, j.saved_to), h("button", { class: "btn sm", onclick: () => invoke("reveal", { path: j.saved_to }) }, I("folder", "sm"), "Open folder")) : null);
}

let jobTimer = null;
async function pollJobs() {
  try { S.cache.twJobs = (await invoke("tweaks_store", { what: "jobs" })).jobs; } catch { return; }
  if (S.page === "tweaks" || S.page === "programs") render();
  clearTimeout(jobTimer);
  if (S.cache.twJobs.some((j) => ["finding", "downloading", "installing"].includes(j.phase))) jobTimer = setTimeout(pollJobs, 700);
  else if (S.cache.twQueue?.length) nextUpdate();
}

async function storeAct(what, r) {
  try {
    if (what === "install" || what === "save") {
      await invoke("tweaks_store", { what, id: r.id });
      note(what === "save" ? "Downloading to Downloads\\Store apps" : "Downloading from Microsoft's servers");
      pollJobs();
    } else if (what === "fluxdm") {
      const x = await run("tweaks_store", { what, id: r.id });
      note(`Sent ${x.sent} file${x.sent > 1 ? "s" : ""} to FluxDM`);
    } else if (what === "winget") {
      S.cache.twBusy = r.id; render();
      await run("tweaks_store", { what, id: r.id }, `${r.title} installed`);
    } else if (what === "links") {
      S.cache.twLinks = { id: r.id, loading: true }; render();
      const x = await invoke("tweaks_store", { what, id: r.id });
      S.cache.twLinks = { id: r.id, ...x };
    }
  } catch (e) {
    const msg = String(e);
    if (what === "links") S.cache.twLinks = { id: r.id, error: msg };
    else note(msg, true);
  }
  S.cache.twBusy = null;
  render();
}

function linksBox(r) {
  const L = S.cache.twLinks;
  if (!L || L.id !== r.id) return null;
  if (L.loading) return h("div", { class: "faint small tw-links" }, "Asking Microsoft's servers for the files…");
  if (L.error) return h("div", { class: "tw-links small" }, h("span", { style: { color: "var(--warn)" } }, L.error),
    /desktop app/.test(L.error) ? h("button", { class: "btn sm", style: { marginLeft: "8px" }, onclick: () => storeAct("winget", r) }, "Install with winget") : null);
  return h("div", { class: "tw-links" },
    h("div", { class: "row wrap" }, h("div", { class: "faint small grow" }, "From Microsoft's servers, newest for this PC. Dependencies you already have are marked and not needed. Links work for a while."),
      L.files.some((f) => f.url && !f.have) ? h("button", { class: "btn sm", onclick: () => { const urls = L.files.filter((f) => f.url && !f.have).map((f) => f.url); navigator.clipboard.writeText(urls.join("\n")).then(() => note(`${urls.length} link${urls.length > 1 ? "s" : ""} copied (the files this PC needs)`)); } }, I("copy", "sm"), "Copy all links") : null),
    L.files.map((f) => h("div", { class: "row tw-link" + (f.have ? " have" : "") },
      h("span", { class: "grow ell", title: f.name }, f.main ? h("b", null, f.name) : f.name),
      h("span", { class: "faint small" }, fmtBytes(f.size)),
      f.have ? h("span", { class: "chip good" }, "Already installed") : h("span", { class: "chip" }, f.main ? "App" : "Needed"),
      f.url ? h("button", { class: "icon-btn sm", title: "Copy link", "aria-label": "Copy link", onclick: () => navigator.clipboard.writeText(f.url).then(() => note("Link copied")) }, I("copy", "sm")) : null,
      f.url ? h("button", { class: "icon-btn sm", title: "Open link", "aria-label": "Open link", onclick: () => invoke("open_url", { url: f.url, keep: true }) }, I("external", "sm")) : null)));
}

// ------------------------------------------------------------------------------------- Store app updates

let updTimer = null;
async function pollUpdates() {
  try { S.cache.twUpd = await invoke("tweaks_store", { what: "updates" }); } catch { return; }
  if (S.page === "tweaks" || S.page === "programs") render();
  clearTimeout(updTimer);
  if (S.cache.twUpd.checking) updTimer = setTimeout(pollUpdates, 800);
}

async function checkUpdates() {
  try { await invoke("tweaks_store", { what: "check_updates" }); } catch (e) { return note(String(e), true); }
  pollUpdates();
}

/** Outside the Store: Deck downloads the new version from Microsoft's servers and installs it over
 * the old one. One app at a time (a slow line split ten ways helps nobody); the rest wait in line. */
function updateHere(list) {
  const q = S.cache.twQueue || (S.cache.twQueue = []);
  for (const u of list) if (!q.includes(u.id)) q.push(u.id);
  note(list.length > 1 ? `Updating ${list.length} apps one after another from Microsoft's servers. Progress shows on each app.` : `Updating ${list[0].title}. Progress shows on it.`);
  nextUpdate();
}

const ACTIVE = ["finding", "downloading", "installing"];
async function nextUpdate() {
  const q = S.cache.twQueue || [];
  if (!q.length || (S.cache.twJobs || []).some((j) => ACTIVE.includes(j.phase))) return render();
  const id = q.shift();
  try { await invoke("tweaks_store", { what: "install", id }); } catch (e) {
    const t = S.cache.twUpd?.updates?.find((x) => x.id === id)?.title || id;
    note(`${t} couldn't start: ${e}`, true);
    return nextUpdate();
  }
  pollJobs();
}

/** Stops one update (what's downloaded is kept, so it can carry on later), or every one and the line. */
async function stopUpdate(id) {
  const q = S.cache.twQueue || [];
  if (!id) {
    S.cache.twQueue = [];
    for (const j of (S.cache.twJobs || []).filter((j) => ACTIVE.includes(j.phase))) await invoke("tweaks_store", { what: "cancel", id: j.id }).catch(() => {});
    note("Stopped. What was already downloaded is kept: Carry on picks up from there.");
  } else if (q.includes(id)) {
    q.splice(q.indexOf(id), 1);
  } else {
    await invoke("tweaks_store", { what: "cancel", id }).catch(() => {});
  }
  pollJobs();
}

/** Whether an update is ticked: what the user chose, else ticked unless its version is numbered another way. */
const ticked = (x) => (S.cache.twTick ||= new Map()).has(x.id) ? S.cache.twTick.get(x.id) : !x.odd;

async function storeAll(btn) {
  const u = S.cache.twUpd;
  // the Store updates every app: apps unticked here would be updated too, so ask first
  const skipped = (u?.updates || []).filter((x) => !ticked(x));
  if (skipped.length && !await confirmBox("The Store can't skip apps", `Windows' Store update has no way to leave apps out: it updates every app that has an update, including the ${skipped.length} you unticked (${skipped.map((x) => x.title).join(", ")}). Update them all anyway? To leave those out, use "Update selected here" instead.`, "Update all with the Store")) return;
  if (btn) { btn.disabled = true; btn.textContent = "Asking the Store…"; }
  let r;
  try { r = await invoke("tweaks_store", { what: "store_update_all" }); } catch (e) { r = { error: String(e) }; }
  if (btn) { btn.disabled = false; btn.textContent = "Update all with the Microsoft Store"; }
  const p = (t) => h("p", { class: "dim", style: { margin: 0 } }, t);
  const list = (title, names) => names?.length ? [h("b", null, title), h("ul", { style: { margin: "0 0 0 18px", padding: 0 } }, names.map((n) => h("li", { class: "small" }, n.replace(/^Microsoft\./, ""))))] : null;
  const btns = (close) => [
    h("button", { class: "btn", onclick: () => { close(); storePage(); } }, I("external", "sm"), "Open the Store's Downloads"),
    u?.updates?.length ? h("button", { class: "btn primary", onclick: () => { close(); updateHere(u.updates.filter(ticked)); } }, I("download", "sm"), "Update the ticked ones here instead") : null,
    h("button", { class: "btn ghost", onclick: close }, "Close")];
  let title, body;
  if (r.error) {
    title = "The Store couldn't be asked";
    body = [p(r.error)];
  } else if (r.blocked_by_policy) {
    title = "The Store isn't allowed to update by itself";
    body = [
      p("Your tweak \"Don't update Store apps by themselves\" (Tweaks → Updates) is ticked. It stops the Store from updating apps in the background, so this button can't work while it's on. Nothing was downloaded."),
      p("What you can do:"),
      h("ul", { class: "small", style: { margin: "0 0 0 18px", padding: 0 } },
        h("li", null, "Open the Store's Downloads page and press \"Get updates\": updating by hand still works with the tweak on."),
        h("li", null, "Or update here, without the Store (free apps)."),
        h("li", null, "Or untick that tweak if you want the Store to update apps by itself again."))];
  } else if (!r.known) {
    title = "The Store was asked to update";
    body = [p("Windows accepted the request, but Deck couldn't read the Store's log to see what it did. Check the Store's Downloads page.")];
  } else if (!r.started.length && !r.cancelled.length) {
    title = "The Store found nothing to update";
    body = [p("The Store checked and didn't start any update. If Deck lists updates above, the Store may not offer them yet; update them here instead.")];
  } else {
    title = r.started.length ? `The Store is updating ${r.started.length} app${r.started.length > 1 ? "s" : ""}` : "The Store didn't update anything";
    body = [
      r.started.length ? p("These download and install in the background (no window opens). The Store's Downloads page shows their progress.") : null,
      list("Updating:", r.started),
      r.cancelled.length ? p(`The Store cancelled ${r.cancelled.length} (it does that for apps that are open, or when Windows wants you to confirm). Close them and try again, or update them here:`) : null,
      list("Cancelled by the Store:", r.cancelled)];
  }
  modal(title, h("div", { class: "col", style: { gap: "8px" } }, body), btns);
}

const storePage = (id) => invoke("tweaks_store", { what: "store_page", id: id || "" }).catch((e) => note(String(e), true));

/** Where one app's update is, in words: waiting, finding, x of y MB at a speed, installing, done, or why it failed. */
function updStatus(x, j, queued) {
  if (queued && !(j && ACTIVE.includes(j.phase))) return h("div", { class: "small dim" }, "Waiting: starts when the one before it is done.");
  if (!j) return null;
  const pct = j.total ? Math.min(100, (j.done / j.total) * 100) : 0;
  const bar = h("div", { class: "tw-pbar", role: "progressbar", "aria-valuenow": Math.round(pct), "aria-valuemin": 0, "aria-valuemax": 100, "aria-label": `${x.title} download` }, h("i", { style: { width: pct + "%" } }));
  switch (j.phase) {
    case "finding": return h("div", { class: "small dim" }, "Asking Microsoft's servers for the files…");
    case "downloading": return [bar, h("div", { class: "small dim" }, `Downloading: ${fmtBytes(j.done)} of ${fmtBytes(j.total)} (${Math.round(pct)}%) · ${fmtBytes(j.speed)}/s${j.eta != null ? " · " + fmtTime(j.eta) + " left" : ""}`)];
    case "installing": return [bar, h("div", { class: "small dim" }, "Downloaded. Installing (Windows replaces the old version; the app closes if it's open)…")];
    case "done": return h("div", { class: "small", style: { color: "var(--good, #34c759)" } }, `Updated to ${x.latest}.`);
    case "interrupted": return [j.total ? bar : null, h("div", { class: "small", style: { color: "var(--warn, #ff9f0a)" } }, `Deck was closed while this was ${j.done ? `downloading. ${fmtBytes(j.done)} of ${fmtBytes(j.total)} is kept` : "starting"}: Carry on picks up from there.`)];
    case "cancelled": return [j.total ? bar : null, h("div", { class: "small dim" }, `Stopped${j.done ? ` at ${fmtBytes(j.done)} of ${fmtBytes(j.total)}` : ""}. What was downloaded is kept: Carry on picks up from there.`)];
    case "failed": return [j.total ? bar : null, h("div", { class: "small", style: { color: "var(--warn, #ff9f0a)" } }, `It failed: ${j.error}${j.done ? ` (${fmtBytes(j.done)} downloaded is kept, so Try again continues from there)` : ""}`)];
    default: return null;
  }
}

function updatesBox() {
  const u = S.cache.twUpd;
  if (u === undefined) { S.cache.twUpd = null; pollUpdates(); }
  const running = (id) => (S.cache.twJobs || []).some((j) => j.id === id && ["finding", "downloading", "installing"].includes(j.phase));
  const jobOf = (id) => (S.cache.twJobs || []).find((j) => j.id === id);
  const checked = u && !u.checking && u.at;
  const head = h("div", { class: "row wrap" },
    h("h2", { class: "tw-h2 grow", style: { margin: 0 } }, "Updates for your Store apps"),
    h("button", { class: "btn sm", disabled: !!u?.checking, onclick: checkUpdates }, I("refresh", "sm"), u?.checking ? (u.retrying ? `Asking again for ${u.retrying} app${u.retrying > 1 ? "s" : ""}…` : `Checking ${u.done} of ${u.total || "…"}`) : checked ? "Check again" : "Check for updates"));
  const busy = (S.cache.twQueue || []).length || u?.updates?.some((x) => running(x.id));
  // which updates to do: every one is ticked until the user unticks it (the choice is kept while Deck is open)
  const tick = S.cache.twTick ||= new Map();
  const todo = checked ? u.updates.filter((x) => !running(x.id) && jobOf(x.id)?.phase !== "done") : [];
  const picked = todo.filter(ticked);
  const all = checked && u.updates.length ? h("div", { class: "row wrap", style: { gap: "8px" } },
    todo.length > 1 ? h("span", { class: "small dim", role: "status" }, `${picked.length} of ${todo.length} ticked`) : null,
    todo.length > 1 && picked.length < todo.length ? h("button", { class: "btn sm ghost", onclick: () => { for (const x of todo) tick.set(x.id, true); render(); } }, "Tick all") : null,
    todo.length > 1 && picked.length > 0 ? h("button", { class: "btn sm ghost", onclick: () => { for (const x of todo) tick.set(x.id, false); render(); } }, "Untick all") : null,
    busy ? h("button", { class: "btn sm", onclick: () => stopUpdate() }, I("x", "sm"), "Stop all") : null,
    !busy && u.updates.some((x) => ["interrupted", "cancelled", "failed"].includes(jobOf(x.id)?.phase)) ? h("button", { class: "btn sm primary", onclick: () => updateHere(u.updates.filter((x) => ["interrupted", "cancelled", "failed"].includes(jobOf(x.id)?.phase))) }, I("refresh", "sm"), "Carry on all") : null,
    h("button", { class: "btn sm primary", disabled: !picked.length, title: picked.length ? null : "Tick at least one app", onclick: () => updateHere(picked) }, I("download", "sm"),
      picked.length === todo.length ? "Update all here (without the Store)" : `Update ${picked.length} selected here (without the Store)`),
    h("button", { class: "btn sm", onclick: (e) => storeAll(e.currentTarget), title: "Windows' own Store update: also updates paid and protected apps. It updates every app: it can't skip the unticked ones." }, "Update all with the Microsoft Store")) : null;
  const queued = (id) => (S.cache.twQueue || []).includes(id);
  const rows = checked ? u.updates.map((x) => { const j = jobOf(x.id); const act = running(x.id); return h("div", { class: "card tw-store" }, h("div", { class: "row wrap" },
    todo.length > 1 && todo.includes(x) ? h("input", { type: "checkbox", class: "tw-pick", checked: ticked(x), "aria-label": `Include ${x.title} in the update`, title: "Ticked apps are updated by the update-all button", onchange: (e) => { tick.set(x.id, e.target.checked); render(); } }) : null,
    h("div", { class: "grow" }, h("b", null, x.title), h("div", { class: "dim small" }, `${x.installed} → ${x.latest}`, x.size ? ` · ${fmtBytes(x.size)}` : "",
        x.odd ? h("span", { class: "chip warn", style: { marginLeft: "8px" }, title: `Microsoft lists this one with a different kind of version number than the one you have (${x.installed} vs ${x.latest}). It may be newer, the same build or a different branch of the app, so Deck doesn't tick it for you. Tick it if you want it.` }, I("alert", "sm"), "Different version numbering: not ticked") : null),
      updStatus(x, j, queued(x.id))),
    j?.phase === "done" ? h("span", { class: "chip good" }, I("check", "sm"), "Updated")
      : act || queued(x.id) ? h("button", { class: "btn sm", onclick: () => stopUpdate(x.id), title: act ? "Stops it. What's downloaded is kept, so Carry on picks up from there." : "Takes it out of the line" }, I("x", "sm"), act ? "Stop" : "Don't update")
      : h("button", { class: "btn sm primary", onclick: () => updateHere([x]), title: ["failed", "cancelled", "interrupted"].includes(j?.phase) ? "Picks up where it stopped; what was downloaded is kept" : null }, I(["failed", "cancelled", "interrupted"].includes(j?.phase) ? "refresh" : "download", "sm"),
          j?.phase === "failed" ? "Try again" : ["cancelled", "interrupted"].includes(j?.phase) ? "Carry on" : "Update"),
    h("button", { class: "btn sm ghost", "aria-expanded": String(S.cache.twLinks?.id === x.id), title: "The download links of the new version, to copy or open",
      onclick: () => S.cache.twLinks?.id === x.id ? (S.cache.twLinks = null, render()) : storeAct("links", x) }, I("link", "sm"), "Links"),
    h("button", { class: "btn sm ghost", onclick: () => storePage(x.id), title: "Update it in the Microsoft Store" }, I("external", "sm"), "In Store")),
    linksBox(x)); }) : null;
  return h("div", { class: "card", style: { marginBottom: "14px" } },
    h("div", { class: "col", style: { gap: "10px" } },
      head,
      !u || (!u.checking && !u.at) ? h("div", { class: "dim small" }, "Deck asks Microsoft's servers for the newest version of each Store app you have, the same way the Store does.") : null,
      checked ? h("div", { class: "dim small" }, (() => {
        const ok = u.total - u.failed.length - u.store_only.length - u.not_in_store;
        const parts = [u.updates.length ? `${u.updates.length} app${u.updates.length > 1 ? "s have" : " has"} an update.` : ok > 0 ? `The ${ok} app${ok > 1 ? "s" : ""} Deck could check ${ok > 1 ? "are" : "is"} up to date.` : "No app could be checked."];
        if (u.store_only.length) parts.push(`${u.store_only.length} paid or protected app${u.store_only.length > 1 ? "s" : ""} only the Store can check.`);
        if (u.not_in_store) parts.push(`${u.not_in_store} ${u.not_in_store > 1 ? "aren't" : "isn't"} in the Store's catalog (installed another way, or part of Windows), so ${u.not_in_store > 1 ? "they weren't" : "it wasn't"} checked.`);
        return parts.join(" ");
      })()) : null,
      checked && u.failed.length ? h("div", { class: "tw-note warn", role: "alert" }, I("alert", "sm"), h("span", { class: "grow", style: { userSelect: "text" } },
        `${u.failed.length} app${u.failed.length > 1 ? "s" : ""} couldn't be checked: ${u.failed.slice(0, 8).map((f) => f[0].replace(/^Microsoft\./, "")).join(", ")}${u.failed.length > 8 ? `, and ${u.failed.length - 8} more` : ""}. `,
        h("b", null, u.failed[0][1].replace(/^(Microsoft's update service|The Store catalog) didn't answer: /, "$1 answered with an error: ")), ". ",
        "Deck already tried each one several times, waiting between tries, and went back to them once more at the end. That kind of error is Microsoft's server being busy or turning requests away for a while, not a problem on this PC; the apps may still have updates."),
        h("button", { class: "btn sm", disabled: !!u.checking, onclick: checkUpdates }, I("refresh", "sm"), "Try those again")) : null,
      all, rows,
      h("div", { class: "row wrap faint small", style: { gap: "10px" } },
        h("span", { class: "grow" }, "Here: Deck downloads from Microsoft and installs it, no Store or sign-in needed. With the Store: the Store does it in the background."),
        h("button", { class: "btn sm ghost", onclick: () => storePage() }, I("external", "sm"), "Store's Downloads page"))));
}

export function storeView() {
  const s = S.cache.twStore || {};
  if (!S.cache.twJobs) pollJobs();
  const search = async () => {
    const q = (S.cache.twStoreQ || "").trim();
    if (!q) return;
    S.cache.twStore = { loading: true }; render();
    try { S.cache.twStore = await invoke("tweaks_store", { what: "search", query: q }); } catch (e) { S.cache.twStore = { error: String(e) }; }
    render();
  };
  const jobs = S.cache.twJobs || [];
  return [
    updatesBox(),
    h("p", { class: "dim", style: { marginTop: 0 } }, "Install Store apps without the Microsoft Store, like store.rg-adguard.net: the files come straight from Microsoft's own servers, checked, and install for your account. Free apps only."),
    h("div", { class: "row wrap tw-filters" },
      h("input", { type: "search", class: "grow", placeholder: "Search the Store: Spotify, WhatsApp, Netflix…", "aria-label": "Search the Store", value: S.cache.twStoreQ || "",
        oninput: (e) => { S.cache.twStoreQ = e.target.value; }, onkeydown: (e) => { if (e.key === "Enter") search(); } }),
      h("button", { class: "btn primary", onclick: search }, I("search", "sm"), "Search")),
    jobs.length ? [h("div", { class: "row" }, h("h2", { class: "tw-h2 grow" }, "Downloads"),
      jobs.some((j) => ["done", "failed", "cancelled"].includes(j.phase)) ? h("button", { class: "btn sm ghost", onclick: () => invoke("tweaks_store", { what: "clear" }).then(pollJobs) }, "Clear finished") : null),
      h("div", { class: "col" }, jobs.slice().reverse().map(job))] : null,
    s.loading ? h("div", { class: "empty" }, "Searching…") : s.error ? h("div", { class: "empty" }, s.error) : null,
    s.results ? [h("h2", { class: "tw-h2" }, "Results"), s.results.length ? h("div", { class: "col" }, s.results.map((r) => {
      const paid = r.price > 0;
      return h("div", { class: "card tw-store" },
        h("div", { class: "row wrap" },
          h("div", { class: "grow" }, h("b", null, r.title), h("div", { class: "dim small" }, r.publisher || "", " · ", r.display_price || (paid ? "Paid" : "Free"), h("span", { class: "faint" }, " · " + r.id))),
          paid ? h("button", { class: "btn sm", onclick: () => storePage(r.id) }, I("external", "sm"), "Open in Store")
            : [h("button", { class: "btn sm primary", disabled: S.cache.twBusy === r.id, onclick: () => storeAct("install", r) }, I("download", "sm"), "Install"),
              h("button", { class: "btn sm", onclick: () => storeAct("save", r), title: "Save the package files to Downloads\\Store apps" }, "Download"),
              h("button", { class: "btn sm", onclick: () => storeAct("fluxdm", r), title: "Send the links to FluxDM" }, "FluxDM"),
              h("button", { class: "btn sm ghost", "aria-expanded": String(S.cache.twLinks?.id === r.id), onclick: () => S.cache.twLinks?.id === r.id ? (S.cache.twLinks = null, render()) : storeAct("links", r) }, "Links")]),
        linksBox(r));
    })) : h("div", { class: "empty" }, "No apps found.")] : null,
  ];
}

// ------------------------------------------------------------------------------------- page

function pendingBar() {
  const p = st().pending || {};
  const bits = [];
  if (p.explorer?.length) bits.push(h("div", { class: "row wrap" }, h("span", { class: "grow" }, `${p.explorer.length} change${p.explorer.length > 1 ? "s need" : " needs"} File Explorer restarted (${p.explorer.join(", ")}).`),
    h("button", { class: "btn sm primary", onclick: () => tdo("after", "explorer", "Explorer restarted") }, "Restart Explorer now"), h("button", { class: "btn sm ghost", onclick: () => tdo("after", "later") }, "Later")));
  if (p.signout?.length) bits.push(h("div", { class: "row wrap" }, h("span", { class: "grow" }, `Sign out to finish: ${p.signout.join(", ")}.`), h("button", { class: "btn sm", onclick: () => tdo("after", "signout") }, "Sign out")));
  if (p.restart?.length) bits.push(h("div", { class: "row wrap" }, h("span", { class: "grow" }, `Restart to finish: ${p.restart.join(", ")}.`), h("button", { class: "btn sm", onclick: () => tdo("after", "restart") }, "Restart")));
  return bits.length ? h("div", { class: "tw-pending", role: "status" }, bits) : null;
}

Pages.tweaks = {
  async load() {
    S.cache.tw = await invoke("tweaks_state");
  },
  live: true,
  async tick() {
    if (view() === "store") return false;
    const before = JSON.stringify(S.cache.tw);
    try { S.cache.tw = await invoke("tweaks_state"); } catch { return false; }
    return JSON.stringify(S.cache.tw) !== before && !S.interacting && document.activeElement?.tagName !== "INPUT" && document.activeElement?.tagName !== "SELECT";
  },
  render() {
    const v = view(), x = st();
    if (v === "apps" || v === "store") { setTimeout(() => go("programs", v === "apps" ? "builtin" : "store")); return h("div"); }
    const c = x.cats.find((k) => k.id === v);
    const title = v === "overview" ? "Tweaks" : v === "apps" ? "Built-in apps" : v === "store" ? "Get Store apps" : v === "changed" ? "Changed by Deck" : v === "back" ? "Windows changed back" : c ? c.name : "Tweaks";
    const sub = v === "overview" ? "Set Windows the way you want it. Every switch shows the PC's real setting, and every change can be undone." : null;
    const body = !S.cache.tw ? h("div", { class: "empty" }, "Reading Windows' settings…")
      : v === "overview" ? overview() : v === "apps" ? appsView() : v === "store" ? storeView() : v === "changed" ? changedView(false) : v === "back" ? changedView(true) : catView(v);
    return h("div", { class: "tw" },
      head(title, sub, !x.admin && S.cache.tw ? h("span", { class: "chip warn", title: "Most tweaks need Deck's admin rights (Settings → About → Set up)" }, I("lock", "sm"), "Limited without admin") : null),
      h("div", { class: "tw-layout" }, nav(), h("div", { class: "tw-body" }, body)),
      pendingBar());
  },
};
