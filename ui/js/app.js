// Deck's shell: navigation, the search box, live refresh and events.

// Sidebar: every item is its own page, grouped. [page, sub, label, icon]
const NAV = [
  [null, [["home", null, "Home", "home"], ["boost", null, "Game boost", "zap"]]],
  ["Everyday", [["clipboard", null, "Clipboard", "clipboard"], ["audio", null, "Sound", "volume"], ["power", "timers", "Timers & breaks", "timer"]]],
  ["Power", [["power", "awake", "Keep awake", "coffee"], ["power", "plan", "Shut down when…", "power"]]],
  ["Hardware", [["cpu", null, "CPU", "chip"], ["gpu", null, "GPU", "gpu"], ["fans", null, "Fans", "fan"], ["lighting", null, "Lighting", "bulb"], ["lcd", null, "Cooler display", "monitor"], ["sensors", null, "Sensors", "gauge"], ["thispc", null, "This PC", "info"]]],
  ["Screen", [["windows", null, "Windows", "windows"], ["system", "display", "Display", "monitor"]]],
  ["PC", [["system", "processes", "Processes", "cpu"], ["system", "startup", "Startup apps", "rocket"], ["system", "network", "Network", "wifi"], ["files", "space", "Disk space", "disk"], ["files", "cleanup", "Clean up", "broom"], ["tools", null, "Tools", "tools"]]],
];
/** Hardware pages only show when this PC has something for them (the backend says which). */
const HW_PAGES = ["cpu", "gpu", "fans", "lighting", "lcd", "sensors", "thispc"];
const navShown = (page) => !HW_PAGES.includes(page) || !S.state?.hw || S.state.hw.pages.includes(page);
const NAV_END = [["phone", null, "Phone", "smartphone"], ["settings", null, "Settings", "gear"]];
/** Which part of a page opens when only the page is given. */
const DEFAULT_SUB = { windows: "open", files: "space", system: "processes", power: "awake", tools: "screen" };
const curSub = () => S.sub || DEFAULT_SUB[S.page] || null;

function navBadge(page, sub) {
  const p = S.live?.power;
  if (page === "power" && sub === "timers" && p?.timers?.length) return h("span", { class: "badge" }, p.timers.length);
  if (page === "power" && sub === "awake" && p?.awake_active) return h("span", { class: "badge" }, "On");
  if (page === "power" && sub === "plan" && p?.plan) return h("span", { class: "badge warn" }, "Set");
  if (page === "phone" && S.state?.remote?.clients) return h("span", { class: "badge" }, S.state.remote.clients);
  if (page === "boost" && S.live?.boost?.on) return h("span", { class: "badge" }, "On");
  if ((page === "cpu" || page === "gpu") && S.live?.profiles?.[page]) return h("span", { class: "badge" }, S.live.profiles[page]);
  return null;
}

const navOn = ([page, sub]) => !S.query && S.page === page && (!sub || curSub() === sub);

let sideSig = "";
function renderSide() {
  if (holdRedraw(renderSide)) return;
  const side = document.getElementById("side");
  const list = side.querySelector(".nav-scroll");
  const scroll = list ? list.scrollTop : 0;
  const item = ([page, sub, label, ic]) => {
    const on = navOn([page, sub]);
    return h("button", { class: "nav" + (on ? " on" : ""), "aria-current": on ? "page" : null, onclick: () => go(page, sub) }, I(ic), h("span", null, label), navBadge(page, sub));
  };
  const parts = [
    h("div", { class: "brand" }, h("div", { class: "mark" }, I("logo")), h("b", null, "Deck")),
    h("div", { class: "nav-scroll" }, NAV.map(([group, items]) => {
      const shown = items.filter(([page]) => navShown(page));
      if (!shown.length) return [];
      const folded = group && (S.state?.settings?.nav_folded || []).includes(group);
      const visible = folded ? shown.filter(navOn) : shown;
      const head = group ? h("button", { class: "nav-group" + (folded ? " shut" : ""), title: folded ? "Show" : "Hide", "aria-expanded": folded ? "false" : "true", onclick: () => foldGroup(group) },
        h("span", { class: "grow" }, group), folded ? h("span", { class: "nav-count" }, shown.length) : null, I("chevron", "sm")) : null;
      return [head, visible.map(item)];
    }).flat(2)),
    h("div", { class: "nav-end" }, NAV_END.map(item))];
  // nothing changed (the usual case on the 1-second tick): keep the buttons that are there, so
  // a click in progress always lands
  const sig = parts.map((x) => x.outerHTML).join("");
  if (sig === sideSig && side.firstChild) return;
  sideSig = sig;
  side.replaceChildren(...parts);
  const ns = side.querySelector(".nav-scroll");
  ns.scrollTop = scroll;
  const more = () => ns.classList.toggle("more", ns.scrollTop + ns.clientHeight < ns.scrollHeight - 4);
  more();
  ns.addEventListener("scroll", more, { passive: true });
}

async function foldGroup(group) {
  const cur = S.state.settings.nav_folded || [];
  const next = cur.includes(group) ? cur.filter((g) => g !== group) : [...cur, group];
  S.state.settings.nav_folded = next;
  renderSide();
  try { await invoke("set_setting", { key: "nav_folded", value: next }); } catch (e) { console.error(e); }
}

const appMode = () => S.state?.settings?.mode === "app";

function renderTop() {
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
    h("button", { class: "icon-btn pin" + (S.state?.pinned ? " on" : ""), title: "Keep Deck open when I click elsewhere", "aria-pressed": S.state?.pinned ? "true" : "false", onclick: async (e) => {
      S.state.pinned = !S.state.pinned;
      await invoke("set_pinned", { on: S.state.pinned });
      e.currentTarget.classList.toggle("on", S.state.pinned);
      e.currentTarget.setAttribute("aria-pressed", S.state.pinned ? "true" : "false");
      note(S.state.pinned ? "Deck stays open" : "Deck hides when you click elsewhere");
    } }, I("pin")),
    h("button", { class: "icon-btn close-x", title: "Close", onclick: () => invoke("hide") }, I("x")));
}

/** Where keyboard focus is inside `root`, so a redraw can put it back on the same control. */
function focusSpot(root) {
  const a = document.activeElement;
  if (!a || a === document.body || !root.contains(a)) return null;
  const path = [];
  for (let n = a; n !== root; n = n.parentElement) path.unshift([...n.parentElement.children].indexOf(n));
  return { path, sig: focusSig(a) };
}
const focusSig = (el) => el.tagName + "|" + (el.getAttribute("aria-label") || el.title || el.textContent || "").trim().slice(0, 40);
function refocus(root, spot) {
  if (!spot) return;
  let n = root;
  for (const i of spot.path) n = n && n.children[i];
  if (!n || focusSig(n) !== spot.sig) n = [...root.querySelectorAll(spot.sig.split("|")[0])].find((x) => focusSig(x) === spot.sig);
  if (n && typeof n.focus === "function") n.focus({ preventScroll: true });
}

function render() {
  if (holdRedraw(render)) return;
  renderSide();
  renderTop();
  const content = document.getElementById("content");
  const scroll = content.scrollTop;
  const spot = !S.query && S.keepScroll ? focusSpot(content) : null;
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
  if (!S.query && S.keepScroll) content.scrollTop = scroll;
  S.keepScroll = true;
  nameControls(content);
  refocus(content, spot);
}

// A mouse button held down on the page: a redraw now would swap the button under the pointer
// and the click would be lost, so live redraws wait until it is released.
// (A release that never arrives, say outside the window, stops counting after 5 s.)
// Every whole-page redraw (the tick, backend events, anything else) goes through render() or
// renderSide(), and both wait here, so no caller can swap a button out from under a click.
let pressedAt = 0;
const heldRedraws = new Set();
const releasePress = () => {
  pressedAt = 0;
  // after the click event of this press has run
  if (heldRedraws.size) setTimeout(() => { if (mouseHeld()) return; const fns = [...heldRedraws]; heldRedraws.clear(); fns.forEach((f) => f()); }, 0);
};
document.addEventListener("pointerdown", () => (pressedAt = Date.now()), true);
for (const ev of ["pointerup", "pointercancel"]) document.addEventListener(ev, releasePress, true);
window.addEventListener("blur", releasePress);
const mouseHeld = () => !!pressedAt && Date.now() - pressedAt < 5000;
/** True (and `fn` runs once the button is released) while a mouse button is held. */
function holdRedraw(fn) {
  if (!mouseHeld()) return false;
  heldRedraws.add(fn);
  return true;
}
/** True while a redraw would get in the way: dragging, a held button, a dialog, or typing. */
const uiBusy = () => S.interacting || mouseHeld() || !!document.querySelector(".modal-back") || ["INPUT", "TEXTAREA", "SELECT"].includes(document.activeElement?.tagName);

async function go(page, sub) {
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

let ticking = false;
async function tick() {
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

async function refreshSlow() {
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

async function onShown(payload) {
  document.body.classList.toggle("style-window", payload?.style === "window");
  document.body.classList.toggle("mode-app", payload?.mode === "app");
  document.body.classList.add("shown");
  S.state = await invoke("get_state");
  applyAccent();
  applySideWidth();
  if (payload?.page) await go(payload.page);
  await Promise.all([tick(), refreshSlow()]);
  render();
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

async function boot() {
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
  S.state = await invoke("get_state");
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
      S.state = await invoke("get_state");
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
      if (topic === "hw") S.state = await invoke("get_state");
      const p = Pages[S.page];
      const busy = uiBusy();
      const mine = (topic === "hw" ? HW_PAGES : ["cpu", "gpu", "boost", "settings"]).includes(S.page);
      if (!S.query && mine && p?.load && !busy && !(topic === "hw" && HW.dirty)) { await p.load(); render(); }
      else renderSide();
    } else if (topic === "boost") {
      const b = await invoke("boost_state");
      S.live = { ...(S.live || {}), boost: { on: b.on, busy: b.busy, game: b.active?.game } };
      if (S.page === "boost" && !S.query) { S.cache.boost = b; render(); }
      else renderSide();
    }
  });
  setInterval(tick, 1000);
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

boot();
