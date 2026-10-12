// Panels you arrange yourself: Customize on a page lets you drag panels to new places, drag a
// corner to resize them, hide them and bring them back. Each page's arrangement is saved in
// settings.panels[page] = { order: [id…], hidden: [id…], size: { id: [columns of 12, height px] } }.
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
  };
}

function layoutSave(page, L) {
  return setSetting("panels", { ...(S.state.settings.panels || {}), [page]: L });
}

/** Home keeps its tile list in settings.tiles (the Settings page edits it too); sizes live with the others. */
const HOME_STORE = {
  load(defs) {
    const ids = defs.map((d) => d.id);
    const on = (S.state.settings.tiles || []).filter((t) => ids.includes(t));
    const saved = (S.state.settings.panels || {}).home || {};
    const off = [...(saved.order || []).filter((id) => ids.includes(id) && !on.includes(id)), ...ids.filter((id) => !on.includes(id) && !(saved.order || []).includes(id))];
    return { order: [...on, ...off], hidden: off, size: { ...(saved.size || {}) } };
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
  for (const c of ["pg-wide", "pg-tall"]) fresh.classList.toggle(c, old.classList.contains(c));
}

/** Sets a panel's width (twelfths) and height (px, 0 = as tall as it needs). */
function pgApplySize(el, w, ht) {
  el.style.setProperty("--span", w);
  el.classList.toggle("pg-wide", w >= 8);
  el.style.minHeight = ht ? ht + "px" : "";
  el.classList.toggle("pg-tall", !!ht);
}

/**
 * A grid of panels. `defs`: [{ id, name, w (default width in twelfths), build() → element }].
 * `store`: { load(defs), save(L) }, the saved arrangement (defaults to settings.panels[page]).
 */
function panelGrid(page, defs, store, cls) {
  store = store || { load: (d) => layoutLoad(page, d), save: (L) => layoutSave(page, L) };
  const L = store.load(defs);
  const editing = pgEditing(page);
  const byId = new Map(defs.map((d) => [d.id, d]));
  const grid = h("div", { class: "pgrid" + (cls ? " " + cls : "") + (editing ? " editing" : ""), "data-page": page });
  const save = () => store.save(L);
  const sizeOf = (id) => L.size[id] || [byId.get(id).w || 4, 0];
  const visible = () => L.order.filter((id) => !L.hidden.includes(id));

  for (const id of visible()) {
    const d = byId.get(id);
    let el;
    try { el = d.build(); } catch (e) { console.error(e); el = h("div", { class: "card" }, d.name); }
    el.dataset.panel = id;
    el.classList.add("pg-panel");
    const [w, ht] = sizeOf(id);
    pgApplySize(el, w, ht);
    if (editing) el.append(...pgChrome(el, d, L, grid, save, sizeOf));
    grid.append(el);
  }
  if (!editing) return grid;

  const hidden = L.hidden.map((id) => byId.get(id)).filter(Boolean);
  return h("div", { class: "pg-wrap" },
    h("div", { class: "pg-help" }, I("info", "sm"),
      h("span", null, "Drag a panel by its bar to move it, drag the corner to resize it, or use the buttons. Press Done when it looks right."),
      h("span", { class: "grow" }),
      h("button", { class: "btn sm ghost", onclick: async () => {
        if (!(await confirmBox("Put the panels back", "Every panel on this page goes back to its first place and size, and hidden ones come back.", "Put back"))) return;
        L.order = defs.map((x) => x.id); L.hidden = []; L.size = {};
        await save(); render();
      } }, I("undo", "sm"), "Put everything back")),
    grid,
    h("div", { class: "pg-tray" },
      h("b", null, hidden.length ? "Hidden panels" : "Nothing is hidden"),
      hidden.length ? h("span", { class: "dim small" }, "Click one to show it again") : h("span", { class: "dim small" }, "Hide a panel with the eye on its bar"),
      h("div", { class: "row wrap", style: { gap: "6px" } }, hidden.map((d) => h("button", { class: "chip", onclick: async () => {
        L.hidden = L.hidden.filter((x) => x !== d.id);
        // back at the end, where it's easy to find
        L.order = [...L.order.filter((x) => x !== d.id), d.id];
        await save(); render();
      } }, I("plus", "sm"), d.name)))));
}

/** The bar, buttons and resize corner a panel gets while arranging. */
function pgChrome(el, d, L, grid, save, sizeOf) {
  const id = d.id;
  const vis = () => L.order.filter((x) => !L.hidden.includes(x));
  const swap = async (dir) => {
    const v = vis(), k = v.indexOf(id), other = v[k + dir];
    if (!other) return;
    const a = L.order.indexOf(id), b = L.order.indexOf(other);
    [L.order[a], L.order[b]] = [L.order[b], L.order[a]];
    await save(); render();
  };
  const widen = async (by) => {
    const [w, ht] = sizeOf(id);
    const steps = [3, 4, 6, 8, 12];
    const k = steps.findIndex((s) => s >= w);
    const next = steps[Math.max(0, Math.min(steps.length - 1, (k < 0 ? steps.length - 1 : k) + by))];
    if (next === w) return;
    L.size[id] = [next, ht];
    await save(); render();
  };
  const v = vis(), k = v.indexOf(id), [w] = sizeOf(id);
  const bar = h("div", { class: "pg-bar" },
    h("span", { class: "pg-grip", title: "Drag to move", "aria-hidden": "true" }),
    h("b", { class: "ell" }, d.name),
    h("span", { class: "grow" }),
    h("button", { class: "icon-btn sm", title: "Move earlier", "aria-label": "Move " + d.name + " earlier", disabled: k === 0, onclick: () => swap(-1) }, I("back", "sm")),
    h("button", { class: "icon-btn sm pg-later", title: "Move later", "aria-label": "Move " + d.name + " later", disabled: k === v.length - 1, onclick: () => swap(1) }, I("back", "sm")),
    h("button", { class: "icon-btn sm", title: "Narrower", "aria-label": "Make " + d.name + " narrower", disabled: w <= PG_MIN_W, onclick: () => widen(-1) }, I("minus", "sm")),
    h("button", { class: "icon-btn sm", title: "Wider", "aria-label": "Make " + d.name + " wider", disabled: w >= 12, onclick: () => widen(1) }, I("plus", "sm")),
    sizeOf(id)[1] ? h("button", { class: "icon-btn sm", title: "Back to its own height", "aria-label": "Give " + d.name + " its own height", onclick: async () => { L.size[id] = [sizeOf(id)[0], 0]; await save(); render(); } }, I("undo", "sm")) : null,
    h("button", { class: "icon-btn sm", title: "Hide", "aria-label": "Hide " + d.name, onclick: async () => { L.hidden = [...L.hidden, id]; await save(); render(); } }, I("eyeoff", "sm")));

  // drag by the bar: the panel moves in the grid as the pointer passes over the others
  bar.addEventListener("pointerdown", (e) => {
    if (e.button !== 0 || e.target.closest("button")) return;
    e.preventDefault();
    bar.setPointerCapture(e.pointerId);
    el.classList.add("pg-moving");
    S.interacting++;
    const move = (ev) => {
      const over = document.elementsFromPoint(ev.clientX, ev.clientY).find((x) => x !== el && x.dataset?.panel && x.parentNode === grid);
      if (!over) return;
      const r = over.getBoundingClientRect();
      const after = ev.clientY > r.bottom - r.height / 3 || (ev.clientY > r.top + r.height / 3 && ev.clientX > r.left + r.width / 2);
      const ref = after ? over.nextSibling : over;
      if (ref !== el && el.nextSibling !== ref) grid.insertBefore(el, ref);
    };
    const up = async () => {
      bar.removeEventListener("pointermove", move);
      el.classList.remove("pg-moving");
      S.interacting = Math.max(0, S.interacting - 1);
      const shown = [...grid.children].map((x) => x.dataset.panel).filter(Boolean);
      L.order = [...shown, ...L.order.filter((x) => !shown.includes(x))];
      await save(); render();
    };
    bar.addEventListener("pointermove", move);
    bar.addEventListener("pointerup", up, { once: true });
    bar.addEventListener("pointercancel", up, { once: true });
  });

  // drag the corner: width snaps to twelfths of the page, height to 10 px
  const corner = h("span", { class: "pg-resize", title: "Drag to resize", "aria-hidden": "true" });
  corner.addEventListener("pointerdown", (e) => {
    if (e.button !== 0) return;
    e.preventDefault();
    corner.setPointerCapture(e.pointerId);
    S.interacting++;
    const r0 = el.getBoundingClientRect();
    const gap = parseFloat(getComputedStyle(grid).columnGap) || 14;
    const col = (grid.clientWidth + gap) / 12;
    el.classList.add("pg-sizing");
    el.style.minHeight = "";
    const natural = el.getBoundingClientRect().height;
    let w = sizeOf(id)[0], ht = sizeOf(id)[1];
    pgApplySize(el, w, ht);
    const move = (ev) => {
      w = Math.max(PG_MIN_W, Math.min(12, Math.round((ev.clientX - r0.left + gap) / col)));
      const want = Math.round((ev.clientY - r0.top) / 10) * 10;
      ht = want > natural + 8 ? want : 0; // shorter than its content isn't possible: back to its own height
      pgApplySize(el, w, ht);
      for (const f of HW.updaters || []) { try { f(); } catch { /* a graph mid-redraw */ } }
    };
    const up = async () => {
      corner.removeEventListener("pointermove", move);
      el.classList.remove("pg-sizing");
      S.interacting = Math.max(0, S.interacting - 1);
      L.size[id] = [w, ht];
      await save(); render();
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
  document.documentElement.style.setProperty("--side-w", (w ? Math.max(160, Math.min(360, w)) : 208) + "px");
  sideGrip();
}

/** The thin handle on the sidebar's edge: drag it to change the width, double-click for the default. */
function sideGrip() {
  const panel = document.querySelector(".panel");
  if (!panel || panel.querySelector(".side-grip")) return;
  const grip = h("div", { class: "side-grip", role: "separator", "aria-orientation": "vertical", "aria-label": "Sidebar width", title: "Drag to change the sidebar's width · double-click for the default", tabindex: "0" });
  const set = (w) => document.documentElement.style.setProperty("--side-w", Math.max(160, Math.min(360, Math.round(w))) + "px");
  const now = () => parseFloat(getComputedStyle(document.documentElement).getPropertyValue("--side-w")) || 208;
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
  grip.addEventListener("dblclick", () => { set(208); setSetting("nav_width", 0); });
  grip.addEventListener("keydown", (e) => {
    if (e.key !== "ArrowLeft" && e.key !== "ArrowRight") return;
    e.preventDefault();
    set(now() + (e.key === "ArrowRight" ? 10 : -10));
    keep();
  });
  panel.append(grip);
}
