// Deck's shell: navigation, the search box, live refresh and events.
import { I, Pages, S, h, invoke, listen, nameControls, note, withLocalWrites } from "./core.js";
import { applySideWidth } from "./layout.js";
import { refreshHome, renderHome } from "./home.js";
import { doSearch, renderResults, searchKey } from "./search.js";
import { head } from "./pages1.js";
import { applyAccent, themeWelcome } from "./pages3.js";
import { HW } from "./hw.js";


// Sidebar: every item is its own page, grouped. [page, sub, label, icon]
export const NAV = [
  [null, [["home", null, "Home", "home"], ["boost", null, "Game boost", "zap"]]],
  ["Hardware", [["cpu", null, "CPU", "chip"], ["gpu", null, "GPU", "gpu"], ["fans", null, "Fans", "fan"], ["lighting", null, "Lighting", "bulb"], ["lcd", null, "Cooler display", "monitor"], ["thispc", null, "This PC", "info"]]],
  ["Monitor", [["sensors", null, "Sensors", "gauge"], ["overlay", null, "Overlay", "gamepad"], ["controller", null, "Controller", "joystick"], ["deaths", null, "Deaths", "skull"], ["drives", null, "Drives", "disk"], ["system", "processes", "Processes", "cpu"], ["system", "network", "Network", "wifi"]]],
  ["Desktop", [["windows", null, "Windows", "windows"], ["system", "display", "Display", "monitor"], ["clipboard", null, "Clipboard", "clipboard"], ["audio", null, "Sound", "volume"], ["claudesync", null, "Claude Code sync", "terminal"]]],
  ["Maintenance", [["files", "space", "Disk space", "disk"], ["files", "cleanup", "Clean up", "broom"], ["system", "startup", "Startup apps", "rocket"], ["tweaks", null, "Tweaks", "sliders"], ["tools", null, "Tools", "tools"]]],
  ["Power & time", [["power", "awake", "Keep awake", "coffee"], ["power", "timers", "Timers & breaks", "timer"], ["power", "plan", "Shut down when…", "power"]]],
];
/** Hardware pages only show when this PC has something for them (the backend says which). */
export const HW_PAGES = ["cpu", "gpu", "fans", "lighting", "lcd", "sensors"];
export const navShown = (page) => page === "claudesync" ? !!(S.state?.settings?.claude_sync_nav || S.state?.settings?.claude_sync_on)
  : !HW_PAGES.includes(page) || !S.state?.hw || S.state.hw.pages.includes(page);
/** Each group's own icon, in the same column as the pages' icons. */
export const NAV_GROUP_ICON = { Hardware: "sliders", Monitor: "activity", Desktop: "layout", Maintenance: "shield", "Power & time": "clock" };
export const NAV_END = [["transfer", null, "PC to PC", "swap"], ["phone", null, "Phone", "smartphone"], ["settings", null, "Settings", "gear"]];
/** Which part of a page opens when only the page is given. */
export const DEFAULT_SUB = { transfer: "send", claudesync: "status", windows: "open", files: "space", system: "processes", power: "awake", tools: "screen", tweaks: "overview" };
export const curSub = () => S.sub || DEFAULT_SUB[S.page] || null;

/** A small bubble on the page's icon: a count, or a dot for a state (its word is the tooltip and what a
 *  screen reader says). On the icon it never pushes the page's name out of a narrow sidebar. */
export function navBadge(page, sub) {
  const p = S.live?.power;
  const n = (v) => h("span", { class: "badge" }, v);
  const dot = (word, warn) => h("span", { class: "badge dot" + (warn ? " warn" : ""), title: word }, word);
  if (page === "power" && sub === "timers" && p?.timers?.length) return n(p.timers.length);
  if (page === "power" && sub === "awake" && p?.awake_active) return dot("On");
  if (page === "power" && sub === "plan" && p?.plan) return dot("Set", true);
  if (page === "phone" && S.state?.remote?.clients) return n(S.state.remote.clients);
  if (page === "transfer" && S.cache.tx && ((S.cache.tx.pending || []).length + (S.cache.tx.reqs || []).length)) return dot("Waiting", true);
  if (page === "boost" && S.live?.boost?.on) return dot("On");
  return null;
}

export const navOn = ([page, sub]) => !S.query && S.page === page && (!sub || curSub() === sub);

export let sideSig = "";
/** Redraws the sidebar even when nothing it shows seems to have changed (after its order changed). */
export function resetSide() {
  sideSig = "";
  renderSide();
}
export function renderSide() {
  if (holdRedraw(renderSide)) return;
  const side = document.getElementById("side");
  const list = side.querySelector(".nav-scroll");
  const scroll = list ? list.scrollTop : 0;
  const item = ([page, sub, label, ic]) => {
    const on = navOn([page, sub]);
    // CPU / GPU: the active profile on its own line under the name, so a long name has room
    const prof = (page === "cpu" || page === "gpu") && S.live?.profiles?.[page];
    if (prof) return h("button", { class: "nav nav-2l" + (on ? " on" : ""), "aria-current": on ? "page" : null, title: `${label}: ${prof}`, onclick: () => go(page, sub) },
      I(ic), h("span", { class: "nav-txt" }, h("span", null, label), h("small", { class: "nav-sub" }, prof)));
    return h("button", { class: "nav" + (on ? " on" : ""), "aria-current": on ? "page" : null, onclick: () => go(page, sub) }, I(ic), h("span", null, label), navBadge(page, sub));
  };
  const parts = [
    h("div", { class: "brand" }, h("div", { class: "mark" }, I("logo")), h("b", null, "Deck")),
    h("div", { class: "nav-scroll" }, navOrdered().map(([group, items]) => {
      const shown = items.filter(([page]) => navShown(page));
      if (!shown.length) return [];
      const folded = group && (S.state?.settings?.nav_folded || []).includes(group);
      const visible = folded ? shown.filter(navOn) : shown;
      const head = group ? h("button", { class: "nav-group" + (folded ? " shut" : ""), title: (folded ? "Show" : "Hide") + " · drag to move the group", "aria-expanded": folded ? "false" : "true", draggable: "true", "data-grp": group, onclick: () => foldGroup(group) },
        I(NAV_GROUP_ICON[group] || "grid"), h("span", { class: "grow" }, group), folded ? h("span", { class: "nav-count" }, shown.length) : null, h("span", { class: "ng-ic" }, I("chevron", "sm"))) : null;
      return [head, visible.map((it) => { const b = item(it); b.draggable = true; b.dataset.g = group || ""; b.dataset.k = navKey(it); return b; })];
    }).flat(2)),
    h("div", { class: "nav-end" }, NAV_END.map(item))];
  // nothing changed (the usual case on the 1-second tick): keep the buttons that are there, so
  // a click in progress always lands
  const sig = parts.map((x) => x.outerHTML).join("");
  if (sig === sideSig && side.firstChild) return;
  sideSig = sig;
  side.replaceChildren(...parts);
  navDnd(side);
  const ns = side.querySelector(".nav-scroll");
  ns.scrollTop = scroll;
  const more = () => ns.classList.toggle("more", ns.scrollTop + ns.clientHeight < ns.scrollHeight - 4);
  more();
  ns.addEventListener("scroll", more, { passive: true });
}

// ---- the user's own sidebar order: drag a group's header, or a page up and down inside its group
export const navKey = (it) => it[0] + "/" + (it[1] || "");

/** NAV in the saved order. Pages and groups the saved order doesn't know (added in an update) keep their default place at the end. */
export function navOrdered() {
  const o = S.state?.settings?.nav_order || {};
  const at = (list, k) => { const i = (list || []).indexOf(k); return i < 0 ? 1e6 : i; };
  const groups = NAV.map(([g, items]) => [g, [...items].sort((a, b) => at(o.items?.[g || ""], navKey(a)) - at(o.items?.[g || ""], navKey(b)))]);
  // the untitled group (Home, Game boost) stays at the top
  return [...groups.filter((g) => !g[0]), ...groups.filter((g) => g[0]).sort((a, b) => at(o.groups, a[0]) - at(o.groups, b[0]))];
}

export let navDrag = null;
/** Drag and drop on the sidebar (listeners set once on the sidebar itself; its buttons are redrawn). */
export function navDnd(side) {
  if (side.dataset.dnd) return;
  side.dataset.dnd = "1";
  const clear = () => side.querySelectorAll(".drop-above, .drop-below").forEach((e) => e.classList.remove("drop-above", "drop-below"));
  const target = (e) => {
    if (!navDrag) return null;
    const el = e.target.closest?.(navDrag.type === "group" ? ".nav-group[data-grp]" : ".nav[data-k]");
    if (!el || el === navDrag.el || (navDrag.type === "item" && el.dataset.g !== navDrag.g)) return null;
    const r = el.getBoundingClientRect();
    return { el, below: e.clientY > r.top + r.height / 2 };
  };
  side.addEventListener("dragstart", (e) => {
    const grp = e.target.closest?.(".nav-group[data-grp]"), it = e.target.closest?.(".nav[data-k]");
    if (grp) navDrag = { type: "group", g: grp.dataset.grp, el: grp };
    else if (it) navDrag = { type: "item", g: it.dataset.g, k: it.dataset.k, el: it };
    else return;
    e.dataTransfer.effectAllowed = "move";
    e.dataTransfer.setData("text/plain", navDrag.g);
    navDrag.el.classList.add("dragging");
  });
  side.addEventListener("dragover", (e) => {
    const t = target(e);
    clear();
    if (!t) return;
    e.preventDefault();
    t.el.classList.add(t.below ? "drop-below" : "drop-above");
  });
  side.addEventListener("dragleave", (e) => { if (!side.contains(e.relatedTarget)) clear(); });
  side.addEventListener("drop", (e) => {
    const t = target(e);
    clear();
    if (!t) return;
    e.preventDefault();
    navMove(navDrag, t.el, t.below);
  });
  side.addEventListener("dragend", () => { clear(); navDrag?.el.classList.remove("dragging"); navDrag = null; });
}

export function navMove(d, el, below) {
  const cur = navOrdered();
  const o = { groups: cur.filter((g) => g[0]).map((g) => g[0]), items: Object.fromEntries(cur.map(([g, items]) => [g || "", items.map(navKey)])) };
  const place = (list, moving, before) => { const l = list.filter((x) => x !== moving); l.splice(l.indexOf(before) + (below ? 1 : 0), 0, moving); return l; };
  if (d.type === "group") o.groups = place(o.groups, d.g, el.dataset.grp);
  else o.items[d.g] = place(o.items[d.g], d.k, el.dataset.k);
  S.state.settings.nav_order = o;
  sideSig = "";
  renderSide();
  invoke("set_setting", { key: "nav_order", value: o }).catch((e) => console.error(e));
}

export async function foldGroup(group) {
  const cur = S.state.settings.nav_folded || [];
  const next = cur.includes(group) ? cur.filter((g) => g !== group) : [...cur, group];
  S.state.settings.nav_folded = next;
  renderSide();
  try { await invoke("set_setting", { key: "nav_folded", value: next }); } catch (e) { console.error(e); }
}

export const appMode = () => S.state?.settings?.mode === "app";

export function renderTop() {
  const top = document.getElementById("top");
  if (top.dataset.built) {
    const pin = top.querySelector(".pin");
    pin.classList.toggle("on", !!S.state?.pinned);
    pin.setAttribute("aria-pressed", S.state?.pinned ? "true" : "false");
    return;
  }
  top.dataset.built = "1";
  const input = h("input", { id: "q", type: "text", placeholder: "Search apps, files, settings… or try 12*8, 5 km to mi, timer 10m", "aria-label": "Search", autocomplete: "off", spellcheck: "false" });
  input.addEventListener("input", () => doSearch(input.value));
  input.addEventListener("keydown", searchKey);
  top.append(
    h("div", { class: "searchbox" }, I("search"), input),
    // the pin: the app window stays above other windows; the quick panel stays open when you click elsewhere
    h("button", { class: "icon-btn pin" + (S.state?.pinned ? " on" : ""), title: appMode() ? "Keep Deck on top of other windows" : "Keep Deck open and on top while you use other apps", "aria-pressed": S.state?.pinned ? "true" : "false", onclick: async (e) => {
      const btn = e.currentTarget;
      S.state.pinned = !S.state.pinned;
      // the quick panel floats where it is while pinned: the panel's place goes to the backend
      const r = document.querySelector(".panel").getBoundingClientRect(), dpr = window.devicePixelRatio || 1;
      const rect = [r.left, r.top, r.width, r.height].map((v) => Math.round(v * dpr));
      if (!appMode()) document.body.classList.toggle("pin-float", S.state.pinned);
      await invoke("set_pinned", { on: S.state.pinned, rect });
      btn.classList.toggle("on", S.state.pinned);
      btn.setAttribute("aria-pressed", S.state.pinned ? "true" : "false");
      applyAccent(); // its own opacity with the pin on or off
      note(appMode() ? (S.state.pinned ? "Deck stays on top of other windows" : "Deck no longer stays on top") : (S.state.pinned ? "Deck floats on top: use other apps while it stays" : "Deck hides when you click elsewhere"));
    } }, I("pin")),
    h("button", { class: "icon-btn close-x", title: "Close", onclick: () => invoke("hide") }, I("x")));
}

/** Where keyboard focus is inside `root`, so a redraw can put it back on the same control. */
export function focusSpot(root) {
  const a = document.activeElement;
  if (!a || a === document.body || !root.contains(a)) return null;
  const path = [];
  for (let n = a; n !== root; n = n.parentElement) path.unshift([...n.parentElement.children].indexOf(n));
  return { path, sig: focusSig(a) };
}
export const focusSig = (el) => el.tagName + "|" + (el.getAttribute("aria-label") || el.title || el.textContent || "").trim().slice(0, 40);
export function refocus(root, spot) {
  if (!spot) return;
  let n = root;
  for (const i of spot.path) n = n && n.children[i];
  if (!n || focusSig(n) !== spot.sig) n = [...root.querySelectorAll(spot.sig.split("|")[0])].find((x) => focusSig(x) === spot.sig);
  if (n && typeof n.focus === "function") n.focus({ preventScroll: true });
}

export let lastView = null;
export const foldTitle = (d) => d.querySelector(":scope > summary")?.textContent || "";
export const openFolds = (root) => new Set([...root.querySelectorAll("details[open]")].map(foldTitle).filter(Boolean));

export function render() {
  if (holdRedraw(render)) return;
  renderSide();
  renderTop();
  const content = document.getElementById("content");
  const scroll = content.scrollTop;
  const spot = !S.query && S.keepScroll ? focusSpot(content) : null;
  // fold cards (<details>) the user opened stay open through a redraw of the same page (found by their title)
  const view = S.query ? "?" : `${S.page}/${S.sub || ""}`;
  const opened = view === lastView ? openFolds(content) : new Set();
  lastView = view;
  let body;
  try {
    if (S.query) body = renderResults();
    else if (S.page === "home") body = renderHome();
    else body = (Pages[S.page] || Pages.settings).render();
  } catch (e) {
    console.error(e);
    body = h("div", { class: "empty" }, "Something went wrong drawing this page: " + e.message);
  }
  content.replaceChildren(body);
  if (opened.size) for (const d of content.querySelectorAll("details")) if (opened.has(foldTitle(d))) d.open = true;
  if (!S.query && S.keepScroll) content.scrollTop = scroll;
  S.keepScroll = true;
  nameControls(content);
  refocus(content, spot);
}

// A mouse button held down on the page: a redraw now would swap the button under the pointer
// and the click would be lost, so live redraws wait until it is released.
// (A release that never arrives, say outside the window, stops counting after 5 s, or 2 min while dragging.)
// Every whole-page redraw (the tick, backend events, anything else) goes through render() or
// renderSide(), and both wait here, so no caller can swap a button out from under a click.
export let pressedAt = 0;
export const heldRedraws = new Set();
export const releasePress = () => {
  pressedAt = 0;
  // after the click event of this press has run
  if (heldRedraws.size) setTimeout(() => { if (mouseHeld()) return; const fns = [...heldRedraws]; heldRedraws.clear(); fns.forEach((f) => f()); }, 0);
};
document.addEventListener("pointerdown", () => (pressedAt = Date.now()), true);
for (const ev of ["pointerup", "pointercancel"]) document.addEventListener(ev, releasePress, true);
window.addEventListener("blur", releasePress);
// a drag (moving or resizing a panel, a slider) can take longer than a click: it holds for up to 2 min
export const mouseHeld = () => !!pressedAt && Date.now() - pressedAt < (S.interacting ? 120000 : 5000);
/** True (and `fn` runs once the button is released) while a mouse button is held. */
export function holdRedraw(fn) {
  if (!mouseHeld()) return false;
  heldRedraws.add(fn);
  return true;
}
/** True while a redraw would get in the way: dragging, a held button, a dialog, or typing. */
export const uiBusy = () => S.interacting || mouseHeld() || !!document.querySelector(".modal-back") || ["INPUT", "TEXTAREA", "SELECT"].includes(document.activeElement?.tagName);

export async function go(page, sub) {
  const changed = page !== S.page || (sub || null) !== (S.sub || null);
  S.page = page;
  S.sub = sub || null;
  if (S.query) {
    S.query = "";
    const q = document.getElementById("q");
    if (q) q.value = "";
    S.results = null;
  }
  if (changed) S.keepScroll = false;
  render();
  const p = Pages[page];
  if (p && p.load) {
    try {
      await p.load();
    } catch (e) {
      console.error(e);
    }
    if (S.page === page) render();
  }
}

// ---------------------------------------------------------- live updates

export let ticking = false;
export async function tick() {
  if (ticking || !document.body.classList.contains("shown") || document.visibilityState === "hidden") return;
  ticking = true;
  try {
    S.live = await invoke("live");
    if (S.query) {
      // leave search results alone
    } else if (S.page === "home") {
      refreshHome();
      renderSide();
    } else {
      const p = Pages[S.page];
      let redraw = false;
      if (p?.tick) redraw = await p.tick();
      else if (p?.live) redraw = true;
      // a change that arrives while the user is busy is drawn on the next free tick, not dropped
      if (redraw) S.redrawDue = S.page;
      if (S.redrawDue === S.page && !uiBusy()) { S.redrawDue = null; render(); }
      else renderSide();
    }
  } catch (e) {
    console.error(e);
  } finally {
    ticking = false;
  }
}

export async function refreshSlow() {
  // things that change rarely: fetched on open
  try {
    const [audio, clips, disp] = await Promise.all([invoke("audio_state"), invoke("clip_list", { q: "", limit: 8 }), invoke("display_state")]);
    S.cache.outputs = audio.outputs;
    S.cache.clips = clips;
    S.cache.dark = disp.dark;
    S.cache.display = disp;
  } catch (e) {
    console.error(e);
  }
  for (const k of ["cpu", "gpu"]) {
    if (S.state?.hw?.pages?.includes(k)) invoke(k + "_state").then((x) => { S.cache[k] = x; if (S.page === "home" && !S.query) refreshHome(); }).catch(() => {});
  }
  if (!S.cache.applist) {
    invoke("apps_list", { force: false }).then((l) => { S.cache.applist = l; if (S.page === "home" && !S.query) render(); });
  }
  invoke("processes").then((p) => (S.cache.procs = p)).catch(() => {});
}

export async function onShown(payload) {
  document.body.classList.toggle("style-window", payload?.style === "window");
  document.body.classList.toggle("mode-app", payload?.mode === "app");
  document.body.classList.toggle("pin-float", !!payload?.float);
  document.body.classList.add("shown");
  // the first frame drawn tells the app the window isn't blank (no repaint nudge needed)
  requestAnimationFrame(() => invoke("ui_heartbeat").catch(() => {}));
  S.state = withLocalWrites(await invoke("get_state"));
  applyAccent();
  applySideWidth();
  if (payload?.page) await go(payload.page);
  await Promise.all([tick(), refreshSlow()]);
  render();
  themeWelcome();
  // an editor left open (kept while hidden) gets the focus back, not the search box
  const kept = [...document.querySelectorAll(".modal-back")].pop();
  if (kept) {
    (kept.querySelector("input, select, textarea, button") || kept).focus({ preventScroll: true });
    return;
  }
  const q = document.getElementById("q");
  if (q) {
    q.focus();
    q.select();
  }
}

export async function boot() {
  // listen before asking for the state, so a show in between is either seen here or in the state
  await Promise.all([
    listen("shown", (e) => onShown(e.payload)),
    listen("hiding", () => {
      document.body.classList.remove("shown");
      document.querySelectorAll(".modal-back").forEach((m) => m.__keep || (m.__close ? m.__close() : m.remove()));
      document.querySelectorAll(".pop").forEach((p) => p.remove());
      document.querySelectorAll(".help-card").forEach((c) => c.__hide());
    }),
  ]);
  S.state = withLocalWrites(await invoke("get_state"));
  applyAccent();
  applySideWidth();
  document.body.classList.toggle("style-window", S.state.settings.mode === "app");
  document.body.classList.toggle("mode-app", S.state.settings.mode === "app");
  render();
  // a "shown" that fired while this page was still loading is lost, and the page would sit frozen
  // (no ticks) until Deck is hidden and opened again: whenever the page isn't marked shown, ask
  const catchUp = async () => {
    if (document.body.classList.contains("shown")) return;
    const st = await invoke("get_state");
    if (st.visible && !document.body.classList.contains("shown")) onShown({ mode: st.settings.mode, style: st.settings.mode === "app" ? "window" : "dim" });
  };
  catchUp();
  setTimeout(catchUp, 1500);
  window.addEventListener("focus", catchUp);
  document.addEventListener("visibilitychange", () => document.visibilityState === "visible" && catchUp());
  listen("changed", async (e) => {
    const topic = e.payload?.topic;
    if (!document.body.classList.contains("shown")) return;
    if (topic === "settings" || topic === "remote") {
      S.state = withLocalWrites(await invoke("get_state"));
      applyAccent();
      if (S.page === "settings" || S.page === "phone" || topic === "remote") renderSide();
    } else if (topic === "clipboard") {
      S.cache.clips = await invoke("clip_list", { q: "", limit: 8 });
      if (S.page === "clipboard") { await Pages.clipboard.load(); if (!S.interacting) render(); }
      else if (S.page === "home" && !S.query) refreshHome();
    } else if (topic === "power") {
      S.live = { ...(S.live || {}), power: (await invoke("live")).power };
      if (!S.query && (S.page === "home" || S.page === "power") && !S.interacting && !["INPUT", "TEXTAREA", "SELECT"].includes(document.activeElement?.tagName)) S.page === "home" ? refreshHome() : render();
      renderSide();
    } else if (topic === "disk" && S.page === "files") {
      await Pages.files.load(); render();
    } else if (topic === "cleanup" && S.page === "files" && S.sub === "cleanup") {
      S.cache.cleanup = await invoke("cleanup_state"); render();
    } else if (topic === "net" && S.page === "system") {
      S.cache.net = await invoke("net_info"); render();
    } else if (topic === "audio") {
      S.cache.outputs = (await invoke("audio_state")).outputs;
    } else if (topic === "display") {
      S.cache.display = await invoke("display_state"); S.cache.dark = S.cache.display.dark;
    } else if (topic === "profiles" || topic === "hotkeys" || topic === "hw") {
      if (topic === "hw") S.state = withLocalWrites(await invoke("get_state"));
      const p = Pages[S.page];
      const busy = uiBusy();
      const mine = (topic === "hw" ? [...HW_PAGES, "thispc"] : ["cpu", "gpu", "boost", "settings"]).includes(S.page);
      if (!S.query && mine && p?.load && !busy && !(topic === "hw" && HW.dirty)) { await p.load(); render(); }
      else renderSide();
    } else if (topic === "boost") {
      const b = await invoke("boost_state");
      S.live = { ...(S.live || {}), boost: { on: b.on, busy: b.busy, game: b.active?.game } };
      if (S.page === "boost" && !S.query) { S.cache.boost = b; render(); }
      else renderSide();
    }
  });
  // low-spec mode: live readings every 3 s
  let lowSkip = 0;
  setInterval(() => { if (S.state?.settings?.low_spec && ++lowSkip % 3) return; tick(); }, 1000);
  // clicking the dimmed area closes Deck
  document.getElementById("backdrop").addEventListener("mousedown", () => invoke("hide"));
  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && document.activeElement?.id !== "q") {
      const m = [...document.querySelectorAll(".modal-back")].pop();
      if (m) m.__close ? m.__close() : m.remove();
      else if (!appMode()) invoke("hide");
    }
    // typing anywhere goes to the search box (not Space: that presses the focused button; not inside a dialog)
    if (e.key.length === 1 && e.key !== " " && !e.ctrlKey && !e.altKey && !e.metaKey && !["INPUT", "TEXTAREA", "SELECT"].includes(document.activeElement?.tagName) && !document.querySelector(".modal-back")) {
      const q = document.getElementById("q");
      if (q) q.focus();
    }
  });
  document.addEventListener("contextmenu", (e) => { if (!e.target.closest("input, textarea, pre")) e.preventDefault(); });
}

