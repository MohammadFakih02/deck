// Panels you arrange yourself: Customize on a page lets you drag panels to new places, drag a
// corner to resize them, hide them and bring them back. Each page's arrangement is saved in
// settings.panels[page] = { order: [id…], hidden: [id…], size: { id: [columns of 12, height px] },
// pos: { id: [column, row of 10 px] } } (a panel without a place goes in the first free spot).
// The sidebar can be made wider or narrower by dragging its edge.

const PG_MIN_W = 3; // narrowest panel, in twelfths of the page

/** The saved arrangement of `defs` ({ id, name, w, build }) on `page`, with new panels added at the end. */
function layoutLoad(page, defs) {
  const saved = (S.state.settings.panels || {})[page] || {};
  const ids = defs.map((d) => d.id);
  const order = (saved.order || []).filter((id) => ids.includes(id));
  return {
    order: [...order, ...ids.filter((id) => !order.includes(id))],
    hidden: (saved.hidden || []).filter((id) => ids.includes(id)),
    size: { ...(saved.size || {}) },
    pos: { ...(saved.pos || {}) },
  };
}

function layoutSave(page, L) {
  return setSetting("panels", { ...(S.state.settings.panels || {}), [page]: L });
}

/** Home's arrangement until you arrange it yourself: places [column, row] and sizes [columns, height px]. */
const HOME_DEFAULT = {
  pos: { clock: [0, 0], audio: [3, 0], network: [6, 0], system: [9, 0], profiles: [6, 18], quick: [0, 32], media: [3, 32], power: [6, 40], pinned: [3, 60], clipboard: [0, 70] },
  size: { clock: [3, 300], audio: [3, 300], network: [3, 166], system: [3, 700], profiles: [3, 201], quick: [3, 360], media: [3, 261], power: [3, 250], pinned: [3, 250], clipboard: [3, 450] },
};

/** Home keeps its tile list in settings.tiles (the Settings page edits it too); sizes live with the others. */
const HOME_STORE = {
  load(defs) {
    const ids = defs.map((d) => d.id);
    const on = (S.state.settings.tiles || []).filter((t) => ids.includes(t));
    const mine = (S.state.settings.panels || {}).home;
    // never arranged (new install, or an update from before arranging): the default arrangement
    const saved = mine && mine.pos && Object.keys(mine.pos).length ? mine : { ...(mine || {}), ...HOME_DEFAULT };
    const off = [...(saved.order || []).filter((id) => ids.includes(id) && !on.includes(id)), ...ids.filter((id) => !on.includes(id) && !(saved.order || []).includes(id))];
    return { order: [...on, ...off], hidden: off, size: { ...(saved.size || {}) }, pos: { ...(saved.pos || {}) } };
  },
  async save(L) {
    await setSetting("tiles", L.order.filter((id) => !L.hidden.includes(id)));
    await layoutSave("home", L);
  },
};

const pgEditing = (page) => S.cache.layoutEdit === page;

/** The Customize / Done button for a page's head. */
function layoutButton(page) {
  const on = pgEditing(page);
  return h("button", { class: "btn" + (on ? " primary" : " ghost"), "aria-pressed": on ? "true" : "false", title: on ? "Stop arranging" : "Move, resize, hide and show the panels on this page",
    onclick: () => { S.cache.layoutEdit = on ? null : page; render(); } }, I(on ? "check" : "layout"), on ? "Done" : "Customize");
}

/** A live redraw built `fresh` to replace `old`: it keeps the panel's place and size. */
function pgKeep(old, fresh) {
  fresh.dataset.panel = old.dataset.panel;
  fresh.classList.add("pg-panel");
  fresh.style.setProperty("--span", old.style.getPropertyValue("--span"));
  fresh.style.minHeight = old.style.minHeight;
  fresh.style.setProperty("--ht", old.style.getPropertyValue("--ht"));
  fresh.style.gridColumn = old.style.gridColumn;
  fresh.style.gridRow = old.style.gridRow;
  // folded as the old one was, so its height doesn't jump for a frame (the neighbours would bob)
  const fold = +old.dataset.fold || 0;
  if (fold) { pgFold(fresh, fold); fresh.dataset.fold = fold; }
  for (const c of ["pg-wide", "pg-tall"]) fresh.classList.toggle(c, old.classList.contains(c));
}


/** A panel's content height (its own, not the row's or its set height), with `level` parts folded. */
function pgContentHeight(el, level) {
  pgFold(el, level);
  const keep = el.style.getPropertyValue("--ht");
  el.style.setProperty("--ht", "0px");
  el.classList.add("pg-measure");
  const ht = el.getBoundingClientRect().height;
  el.classList.remove("pg-measure");
  el.style.setProperty("--ht", keep);
  return ht;
}

/** Folds a panel's parts up to `level`: data-drop="n" parts hide from step n, data-alt="n" parts show from it. */
function pgFold(el, level) {
  el.querySelectorAll("[data-drop]").forEach((e) => { e.hidden = +e.dataset.drop <= level; });
  el.querySelectorAll("[data-alt]").forEach((e) => { e.hidden = level < +e.dataset.alt; });
}

/** The panel's fold steps (0 = nothing folds). */
const pgLevels = (el) => Math.max(0, ...[...el.querySelectorAll("[data-drop],[data-alt]")].map((e) => +(e.dataset.drop || e.dataset.alt)));

/** A panel set shorter than its content folds its least needed parts, one step at a time, until it
 *  fits (nothing is ever cut): the clock loses its calendar, profiles become dropdowns, and so on. */
function pgFit(el) {
  const ht = parseFloat(el.style.getPropertyValue("--ht")) || 0;
  const max = pgLevels(el);
  let level = 0;
  if (ht && max) while (level < max && pgContentHeight(el, level) > ht + 1) level++;
  pgFold(el, level);
  el.dataset.fold = level;
}

const PG_ROW = 10, PG_GAP = 14, PG_COLS = 12; // grid row step (px), space under a panel, columns
const PG_FREE_MIN = 860; // narrower than this, panels stack in order instead (app.css .flow)
const PG_SNAP = 14; // px: a resized panel this close to another's bottom edge lines up with it

/** Rows of 10 px a panel needs for its height (its own: the set height, or its content). */
const pgRows = (el) => Math.max(1, Math.ceil((el.getBoundingClientRect().height + PG_GAP) / PG_ROW));

const pgHits = (a, x, y, w, rows) => a.x < x + w && x < a.x + a.w && a.y < y + rows && y < a.y + a.rows;

/** Places panels so none overlap: each keeps its own spot unless it would land on one placed
 *  before it, then it moves down just enough. `first` (the one being dragged) is placed first. */
function pgResolve(list, first) {
  list.sort((a, b) => (a.id === first ? -1 : b.id === first ? 1 : a.y - b.y || a.x - b.x));
  const placed = [];
  for (const it of list) {
    let hit;
    while ((hit = placed.find((p) => pgHits(p, it.x, it.y, it.w, it.rows)))) it.y = hit.y + hit.rows;
    placed.push(it);
  }
  return placed;
}

/** The first free spot for a panel that has none yet: top to bottom, left to right. */
function pgSpot(placed, w, rows) {
  for (let y = 0; y < 10000; y++) for (let x = 0; x + w <= PG_COLS; x++) if (!placed.some((p) => pgHits(p, x, y, w, rows))) return [x, y];
  return [0, 0];
}

/**
 * The free grid: every panel sits where it was put (`bx`, `by` in columns and rows) at its own size,
 * and moves only when another would land on it (then it slides down just enough). Heights come from
 * the panels themselves and are measured again whenever one changes, so nothing ever overlaps.
 * On a narrow page the panels stack in order instead (`flow`), and the arrangement comes back
 * when there is room again.
 */
function pgEngine(grid, wrap, items) {
  let free = true, queued = false, active = null;
  const layout = (first) => {
    // while a panel is held, every pass (the pointer's, or a size change's) keeps it on top
    first = first || active;
    const list = [...items.values()].filter((it) => it.el.isConnected);
    if (!list.length) return;
    free = wrap.clientWidth >= PG_FREE_MIN;
    grid.classList.toggle("flow", !free);
    // a panel just put on the page (a redraw, Done, a resize ending) folds to its height before
    // it's measured: otherwise it would show at full size
    for (const it of list) if (!it.el.__fitted) { pgFit(it.el); it.el.__fitted = true; }
    for (const it of list) it.rows = pgRows(it.el);
    if (!free) {
      for (const it of list) { it.el.style.gridColumn = ""; it.el.style.gridRow = "span " + it.rows; }
      return;
    }
    const known = list.filter((it) => it.bx != null);
    for (const it of known) { it.w = Math.min(it.w, PG_COLS); it.x = Math.max(0, Math.min(it.bx, PG_COLS - it.w)); it.y = Math.max(0, it.by); }
    const placed = pgResolve(known, first);
    for (const it of list.filter((it) => it.bx == null)) {
      [it.x, it.y] = pgSpot(placed, Math.min(it.w, PG_COLS), it.rows);
      placed.push(it);
    }
    for (const it of list) {
      it.el.style.gridColumn = `${it.x + 1} / span ${Math.min(it.w, PG_COLS)}`;
      it.el.style.gridRow = `${it.y + 1} / span ${it.rows}`;
    }
  };
  const soon = () => { if (!queued) { queued = true; requestAnimationFrame(() => { queued = false; layout(); }); } };
  // a panel's height changes (live content, folding, a resize) → its rows change → place again
  const ro = new ResizeObserver(soon);
  const watch = (el) => {
    const it = items.get(el.dataset?.panel);
    if (!it || !el.classList.contains("pg-panel")) return;
    it.el = el;
    if (el.isConnected) { pgFit(el); el.__fitted = true; }
    ro.observe(el);
    soon();
  };
  [...grid.children].forEach(watch);
  // a live redraw swaps a panel for a fresh one: fold it to its height and keep it in its place
  new MutationObserver((ms) => ms.forEach((m) => m.addedNodes.forEach(watch))).observe(grid, { childList: true });
  new ResizeObserver(soon).observe(wrap);
  return {
    layout,
    free: () => free,
    hold(id) { active = id; },
    /** Every panel's current place becomes its own (before a drag, and to save). */
    freeze() { for (const it of items.values()) if (it.x != null) { it.bx = it.x; it.by = it.y; } },
  };
}

/** Sets a panel's width (twelfths) and height (px, 0 = as tall as its content). A set height is the
 *  panel's least height: it never cuts the content (more content grows it); a shorter one folds its
 *  least needed parts (pgFit). */
function pgApplySize(el, w, ht) {
  el.style.setProperty("--span", w);
  el.classList.toggle("pg-wide", w >= 8);
  el.style.minHeight = "";
  el.style.height = "";
  el.style.setProperty("--ht", ht ? ht + "px" : "0px");
  el.classList.toggle("pg-tall", !!ht);
}

/**
 * A grid of panels. `defs`: [{ id, name, w (default width in twelfths), build() → element }].
 * `store`: { load(defs), save(L) }, the saved arrangement (defaults to settings.panels[page]).
 */
function panelGrid(page, defs, store, cls) {
  store = store || { load: (d) => layoutLoad(page, d), save: (L) => layoutSave(page, L) };
  const L = store.load(defs);
  L.pos = L.pos || {};
  const editing = pgEditing(page);
  const byId = new Map(defs.map((d) => [d.id, d]));
  const grid = h("div", { class: "pgrid" + (cls ? " " + cls : "") + (editing ? " editing" : ""), "data-page": page });
  const sizeOf = (id) => L.size[id] || [byId.get(id).w || 4, 0];
  const visible = () => L.order.filter((id) => !L.hidden.includes(id));
  const items = new Map();
  const save = () => {
    for (const it of items.values()) if (it.bx != null) L.pos[it.id] = [it.bx, it.by];
    return store.save(L);
  };

  for (const id of visible()) {
    const d = byId.get(id);
    let el;
    try { el = d.build(); } catch (e) { console.error(e); el = h("div", { class: "card" }, d.name); }
    el.dataset.panel = id;
    el.classList.add("pg-panel");
    const [w, ht] = sizeOf(id);
    pgApplySize(el, w, ht);
    const p = L.pos[id];
    items.set(id, { id, el, w, bx: p ? p[0] : null, by: p ? p[1] : null, rows: 1 });
    grid.append(el);
  }
  // the wrapper is the size container the narrow-page rules measure
  const wrapped = h("div", { class: "pgrid-wrap" }, grid);
  const eng = pgEngine(grid, wrapped, items);
  if (editing) for (const it of items.values()) it.el.append(...pgChrome(it, byId.get(it.id), L, grid, eng, items, save));
  if (!editing) return wrapped;

  const hidden = L.hidden.map((id) => byId.get(id)).filter(Boolean);
  return h("div", { class: "pg-wrap" },
    h("div", { class: "pg-help" }, I("info", "sm"),
      h("span", null, "Drag a panel by its bar to put it anywhere, and drag its corner to resize it. Panels in the way slide down. Press Done when it looks right."),
      h("span", { class: "grow" }),
      h("button", { class: "btn sm ghost", onclick: async () => {
        if (!(await confirmBox("Put the panels back", "Every panel on this page goes back to its first place and size, and hidden ones come back.", "Put back"))) return;
        L.order = defs.map((x) => x.id); L.hidden = []; L.size = {}; L.pos = {};
        await store.save(L); render();
      } }, I("undo", "sm"), "Put everything back")),
    wrapped,
    h("div", { class: "pg-tray" },
      h("b", null, hidden.length ? "Hidden panels" : "Nothing is hidden"),
      hidden.length ? h("span", { class: "dim small" }, "Click one to show it again: it goes in the first free spot") : h("span", { class: "dim small" }, "Hide a panel with the eye on its bar"),
      h("div", { class: "row wrap", style: { gap: "6px" } }, hidden.map((d) => h("button", { class: "chip", onclick: async () => {
        eng.freeze();
        L.hidden = L.hidden.filter((x) => x !== d.id);
        delete L.pos[d.id];
        L.order = [...L.order.filter((x) => x !== d.id), d.id];
        await save(); render();
      } }, I("plus", "sm"), d.name)))));
}

/** The bar, buttons and resize corner a panel gets while arranging. */
function pgChrome(it, d, L, grid, eng, items, save) {
  const id = d.id, el = it.el;
  const sizeOf = () => L.size[id] || [d.w || 4, 0];
  const setSize = async (w, ht) => {
    eng.freeze();
    L.size[id] = [w, ht];
    it.w = w;
    pgApplySize(it.el, w, ht);
    pgFit(it.el);
    eng.layout(id);
    eng.freeze();
    await save();
  };
  const widen = (by) => {
    const [w, ht] = sizeOf();
    const steps = [3, 4, 6, 8, 12];
    const k = steps.findIndex((s) => s >= w);
    const next = steps[Math.max(0, Math.min(steps.length - 1, (k < 0 ? steps.length - 1 : k) + by))];
    if (next !== w) setSize(next, ht).then(render);
  };
  const [w] = sizeOf();
  const bar = h("div", { class: "pg-bar", title: "Drag to put it anywhere" },
    h("span", { class: "pg-grip", "aria-hidden": "true" }),
    h("b", { class: "ell" }, d.name),
    h("span", { class: "grow" }),
    h("button", { class: "icon-btn sm pg-size", title: "Narrower", "aria-label": "Make " + d.name + " narrower", disabled: w <= PG_MIN_W, onclick: () => widen(-1) }, I("minus", "sm")),
    h("button", { class: "icon-btn sm pg-size", title: "Wider", "aria-label": "Make " + d.name + " wider", disabled: w >= 12, onclick: () => widen(1) }, I("plus", "sm")),
    sizeOf()[1] ? h("button", { class: "icon-btn sm", title: "Back to its own height", "aria-label": "Give " + d.name + " its own height", onclick: () => setSize(sizeOf()[0], 0).then(render) }, I("undo", "sm")) : null,
    h("button", { class: "icon-btn sm", title: "Hide", "aria-label": "Hide " + d.name, onclick: async () => {
      eng.freeze();
      L.hidden = [...L.hidden, id];
      items.delete(id);
      delete L.pos[id];
      await save(); render();
    } }, I("eyeoff", "sm")));

  // drag by the bar: the panel follows the pointer cell by cell and lands exactly where it's let go
  bar.addEventListener("pointerdown", (e) => {
    if (e.button !== 0 || e.target.closest("button")) return;
    if (!eng.free()) { note("Make the window wider to place panels freely"); return; }
    e.preventDefault();
    bar.setPointerCapture(e.pointerId);
    S.interacting++;
    eng.freeze();
    const r = it.el.getBoundingClientRect(), gx = e.clientX - r.left, gy = e.clientY - r.top;
    it.el.classList.add("pg-moving");
    eng.hold(id);
    const move = (ev) => {
      const g = grid.getBoundingClientRect(), col = (g.width + PG_GAP) / PG_COLS;
      const bx = Math.max(0, Math.min(PG_COLS - Math.min(it.w, PG_COLS), Math.round((ev.clientX - gx - g.left) / col)));
      const by = Math.max(0, Math.round((ev.clientY - gy - g.top) / PG_ROW));
      if (bx === it.bx && by === it.by) return;
      it.bx = bx;
      it.by = by;
      // the others go back to their own places each time, then make room for this one
      const keep = new Map([...items.values()].map((x) => [x.id, [x.bx, x.by]]));
      eng.layout(id);
      for (const [k, [bx, by]] of keep) { const x = items.get(k); x.bx = bx; x.by = by; }
    };
    const up = async () => {
      bar.removeEventListener("pointermove", move);
      it.el.classList.remove("pg-moving");
      S.interacting = Math.max(0, S.interacting - 1);
      eng.layout(id);
      eng.hold(null);
      eng.freeze();
      await save();
    };
    bar.addEventListener("pointermove", move);
    bar.addEventListener("pointerup", up, { once: true });
    bar.addEventListener("pointercancel", up, { once: true });
  });

  // drag the corner: width in twelfths of the page, height in 10 px steps; it keeps exactly that size
  const corner = h("span", { class: "pg-resize", title: "Drag to resize", "aria-hidden": "true" });
  corner.addEventListener("pointerdown", (e) => {
    if (e.button !== 0) return;
    e.preventDefault();
    corner.setPointerCapture(e.pointerId);
    S.interacting++;
    eng.freeze();
    const el = it.el, r0 = el.getBoundingClientRect();
    const col = (grid.clientWidth + PG_GAP) / PG_COLS;
    el.classList.add("pg-sizing");
    // as short as it can get: every part that can fold away folded (nothing is ever cut)
    const least = pgContentHeight(el, pgLevels(el));
    pgFit(el);
    let [w, ht] = sizeOf();
    const base = new Map([...items.values()].map((x) => [x.id, [x.bx, x.by]]));
    eng.hold(id);
    // tidying up: near another panel's bottom edge, a line shows and the height snaps to it
    const guide = h("div", { class: "pg-guide", hidden: true });
    document.body.append(guide);
    let matched = null;
    const align = (want) => {
      const g = grid.getBoundingClientRect();
      let best = null;
      for (const x of items.values()) {
        if (x.id === id || !x.el.isConnected) continue;
        const b = x.el.getBoundingClientRect().bottom;
        const d = Math.abs(r0.top + want - b);
        if (d <= PG_SNAP && b - r0.top >= least && (!best || d < best.d)) best = { d, b, el: x.el };
      }
      if (matched && matched !== best?.el) matched.classList.remove("pg-match");
      matched = best?.el || null;
      guide.hidden = !best;
      if (!best) return want;
      matched.classList.add("pg-match");
      Object.assign(guide.style, { top: best.b + "px", left: g.left + "px", width: g.width + "px" });
      return Math.round(best.b - r0.top);
    };
    const move = (ev) => {
      w = eng.free() ? Math.max(PG_MIN_W, Math.min(12, Math.round((ev.clientX - r0.left + PG_GAP) / col))) : w;
      ht = align(Math.max(Math.ceil(least), Math.round((ev.clientY - r0.top) / 10) * 10));
      it.w = w;
      pgApplySize(el, w, ht);
      pgFit(el);
      for (const [k, [bx, by]] of base) { const x = items.get(k); if (x) { x.bx = bx; x.by = by; } }
      eng.layout(id);
      for (const f of HW.updaters || []) { try { f(); } catch { /* a graph mid-redraw */ } }
    };
    const up = async () => {
      corner.removeEventListener("pointermove", move);
      el.classList.remove("pg-sizing");
      guide.remove();
      matched?.classList.remove("pg-match");
      S.interacting = Math.max(0, S.interacting - 1);
      L.size[id] = [w, ht];
      eng.layout(id);
      eng.hold(null);
      eng.freeze();
      await save();
      render();
    };
    corner.addEventListener("pointermove", move);
    corner.addEventListener("pointerup", up, { once: true });
    corner.addEventListener("pointercancel", up, { once: true });
  });
  return [bar, corner];
}

// ------------------------------------------------------------------- the sidebar's width

function applySideWidth() {
  const w = S.state?.settings?.nav_width || 0;
  document.documentElement.style.setProperty("--side-w", (w ? Math.max(180, Math.min(360, w)) : 180) + "px");
  sideGrip();
}

/** The thin handle on the sidebar's edge: drag it to change the width, double-click for the default. */
function sideGrip() {
  const panel = document.querySelector(".panel");
  if (!panel || panel.querySelector(".side-grip")) return;
  const grip = h("div", { class: "side-grip", role: "separator", "aria-orientation": "vertical", "aria-label": "Sidebar width", title: "Drag to change the sidebar's width · double-click for the default", tabindex: "0" });
  const set = (w) => document.documentElement.style.setProperty("--side-w", Math.max(180, Math.min(360, Math.round(w))) + "px");
  const now = () => parseFloat(getComputedStyle(document.documentElement).getPropertyValue("--side-w")) || 180;
  const keep = () => setSetting("nav_width", Math.round(now()));
  grip.addEventListener("pointerdown", (e) => {
    if (e.button !== 0) return;
    e.preventDefault();
    grip.setPointerCapture(e.pointerId);
    const x0 = e.clientX, w0 = now();
    grip.classList.add("on");
    const move = (ev) => set(w0 + ev.clientX - x0);
    grip.addEventListener("pointermove", move);
    grip.addEventListener("pointerup", () => { grip.removeEventListener("pointermove", move); grip.classList.remove("on"); keep(); }, { once: true });
  });
  grip.addEventListener("dblclick", () => { set(180); setSetting("nav_width", 0); });
  grip.addEventListener("keydown", (e) => {
    if (e.key !== "ArrowLeft" && e.key !== "ArrowRight") return;
    e.preventDefault();
    set(now() + (e.key === "ArrowRight" ? 10 : -10));
    keep();
  });
  panel.append(grip);
}
