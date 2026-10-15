// Sensors → Overlay. What the in-game overlay shows (and in what order), how its lines are laid
// out and coloured, how RivaTuner draws it, and a preview built from the same lines the overlay gets.

const OV_DEF = {
  layout: "grouped", labels: true, units: "small", align: true,
  label_color: "FF9A40", value_color: "FFFFFF", warn_color: "FF5A5A", warm_color: "FFC85A", heat: false, group_colors: false, group_values: false,
  fps_lines: false, columns: false, clock_12h: false, clock_secs: false,
  gpu_color: "76B900", cpu_color: "4FA3FF", ram_color: "C77DFF", fps_color: "FFD60A", info_color: "9AA3B5",
  warn: true, warn_gpu_temp: 85, warn_cpu_temp: 90, warn_fps: 0, cores_per_row: 8, mem_unit: "MB", stats_secs: 60,
  corner: "tr", margin_x: 1, margin_y: 1, zoom: 2, shadow: true, box_on: false, box_opacity: 50, font: "Unispace", bold: false,
  graph: false, graph_w: 32, graph_h: 4, graph_style: 0, graph_max: 0, graph_fit: true, graph_secs: 10, graph_smooth: true, color_bar: false, fit: false, int_fps: true, all_games: true,
};

const OV_FONTS = ["Unispace", "Consolas", "Lucida Console", "Courier New", "Cascadia Mono", "Segoe UI", "Tahoma", "Arial", "Verdana", "Calibri"];
const OV_CSS_FONT = { Unispace: 'Consolas, "Cascadia Mono", monospace', Consolas: "Consolas, monospace", "Lucida Console": '"Lucida Console", monospace', "Courier New": '"Courier New", monospace' };

const OV_THEMES = [
  ["Classic", { label_color: "FF9A40", value_color: "FFFFFF", warn_color: "FF5A5A", warm_color: "FFC85A", group_colors: false }],
  ["By part", { label_color: "FF9A40", value_color: "FFFFFF", warn_color: "FF5A5A", warm_color: "FFC85A", group_colors: true, gpu_color: "76B900", cpu_color: "4FA3FF", ram_color: "C77DFF", fps_color: "FFD60A", info_color: "9AA3B5" }],
  ["Ice", { label_color: "7FD4FF", value_color: "FFFFFF", warn_color: "FF6B8A", warm_color: "FFD479", group_colors: false }],
  ["Neon", { label_color: "FF2BD6", value_color: "00F5FF", warn_color: "FF3B3B", warm_color: "FFE600", group_colors: true, gpu_color: "39FF14", cpu_color: "00E5FF", ram_color: "FF2BD6", fps_color: "FFE600", info_color: "A78BFA" }],
  ["Matrix", { label_color: "00A83C", value_color: "7CFF9B", warn_color: "FF5A5A", warm_color: "E6FF5A", group_colors: false }],
  ["Sunset", { label_color: "FF8A5B", value_color: "FFF1E6", warn_color: "FF4D6D", warm_color: "FFC857", group_colors: false }],
  ["Mono", { label_color: "9AA3B5", value_color: "FFFFFF", warn_color: "FF5A5A", warm_color: "FFC85A", group_colors: false }],
];

const OV_PRESETS = [
  ["Just FPS", ["fps"]],
  ["FPS and lows", ["fps", "frametime", "fps_1low", "fps_01low"]],
  ["Gaming", ["gpu_temp", "gpu_usage", "gpu_mem", "cpu_temp", "cpu_usage", "ram", "fps", "frametime", "fps_1low"]],
  ["Like Afterburner", ["gpu_temp", "gpu_usage", "gpu_mem", "gpu_clock", "gpu_memclock", "gpu_power", "gpu_fan", "cpu_temp", "cpu_usage", "cpu_clock", "cpu_power", "ram", "fps", "frametime"]],
  ["Every core", ["cpu_usage", "cpu_temp", "core_table", "gpu_temp", "gpu_usage", "fps"]],
  ["Everything", null],
];

/** One click: the items and the style together. */
const OV_LOOKS = [
  ["Like MSI Afterburner", "Clock on top, a line per thread, framerate stats on their own lines, columns, graph",
    ["time", "cpu_temp", "cpu_usage", "cpu_clock", "cpu_power", "thread_table", "ram", "proc_ram", "commit", "fps", "fps_min", "fps_avg", "fps_max", "fps_1low", "fps_01low", "gpu_temp", "gpu_usage", "gpu_power_pct", "gpu_clock", "gpu_power", "gpu_fan", "gpu_mem", "gpu_memclock", "frametime"],
    { layout: "grouped", labels: true, align: true, units: "small", columns: true, fps_lines: true, group_colors: true, group_values: false, heat: false,
      label_color: "FF8A1E", value_color: "FF8A1E", warm_color: "FFC85A", warn_color: "FF4D4D", cpu_color: "1E90FF", ram_color: "1E90FF", gpu_color: "2ECC71", fps_color: "E0707A", info_color: "FF8A1E",
      clock_12h: true, clock_secs: true, graph: true, graph_w: 48, graph_h: 2, shadow: true, box_on: false, zoom: 2, corner: "tr", margin_x: 8, margin_y: 8, font: "Unispace", bold: false }],
  ["Just the framerate", "A big FPS number in the corner",
    ["fps"], { layout: "line", labels: false, units: "small", columns: false, group_colors: false, heat: false, zoom: 4, corner: "tl", margin_x: 12, margin_y: 12, graph: false, box_on: true, box_opacity: 45 }],
  ["Compact row", "GPU, CPU, memory and framerate on one line",
    ["gpu_temp", "gpu_usage", "cpu_temp", "cpu_usage", "ram", "fps", "frametime"], { layout: "line", labels: true, units: "small", columns: false, group_colors: true, group_values: false, zoom: 2, corner: "tl", margin_x: 10, margin_y: 10, graph: false, box_on: true, box_opacity: 55 }],
  ["Every core", "CPU load, then each core's usage, clock and temperature",
    ["cpu_usage", "cpu_temp", "core_table", "gpu_temp", "gpu_usage", "fps"], { layout: "grouped", labels: true, columns: true, fps_lines: false, group_colors: true, group_values: false, heat: true, zoom: 2, corner: "tr", margin_x: 8, margin_y: 8 }],
];

const OV_CORNERS = [["tl", "Top left"], ["tr", "Top right"], ["bl", "Bottom left"], ["br", "Bottom right"]];
const OV_SLOT_COLOR = { 4: "gpu_color", 5: "cpu_color", 6: "ram_color", 7: "fps_color", 8: "info_color" };

const ovStyleNow = () => ({ ...OV_DEF, ...(S.state.settings.osd_style || {}) });

async function ovRefresh() {
  S.cache.rtss = await invoke("rtss_state").catch(() => S.cache.rtss);
}

/** Changes some style fields, tells the backend and redraws the page. */
async function ovStyle(patch) {
  await setSetting("osd_style", { ...ovStyleNow(), ...patch });
  await ovRefresh();
  render();
}

async function ovItemsSet(list) {
  await setSetting("osd_items", list);
  await ovRefresh();
  render();
}

function ovSwitch(title, desc, on, onset, help) {
  return h("label", { class: "boost-opt" }, toggle(on, onset), h("div", null, h("div", { class: "t" }, title, help ? helpTip(help) : null), desc ? h("div", { class: "s" }, desc) : null));
}

function ovRow(title, control, desc, help) {
  return h("div", { class: "ov-row2" }, h("div", { class: "ov-lbl" }, h("div", { class: "t" }, title, help ? helpTip(help) : null), desc ? h("div", { class: "s" }, desc) : null), h("div", { class: "ov-ctl" }, control));
}

function ovColor(value, fallback, onset) {
  const input = h("input", { type: "color", value: "#" + String(value || fallback).replace("#", "").toLowerCase(), "aria-label": "Colour" });
  input.addEventListener("change", () => onset(input.value.slice(1).toUpperCase()));
  return h("div", { class: "ov-color" }, input, h("button", { class: "btn sm ghost", title: "Back to the default colour", "aria-label": "Default colour", onclick: () => onset(fallback) }, I("undo", "sm")));
}

function ovColorField(title, key, st) {
  return h("label", null, h("span", { class: "dim small" }, title), ovColor(st[key], OV_DEF[key], (c) => ovStyle({ [key]: c })));
}

// ------------------------------------------------------------------- the preview

/** The frametime graph in the preview: the game's last frames when one is running, else a made-up trace. */
function ovGraphSvg(st, ms) {
  const r0 = S.cache.rtss || {};
  const real = (r0.graph || []).length >= 20 ? r0.graph : (r0.ft_recent || []).slice(-120);
  const n = real.length >= 20 ? real.length : 60, base = Math.max(1, ms || 7);
  const top = st.graph_max || (real.length >= 20 ? Math.max(...real) * 1.2 : base * 2.4);
  let pts = "";
  for (let i = 0; i < n; i++) {
    const spike = i % 17 === 9 ? 1.9 : 1 + 0.12 * Math.sin(i * 1.7) + 0.08 * Math.sin(i * 0.6);
    const v = Math.min(1, (real.length >= 20 ? real[i] : base * spike) / top);
    pts += `${(i / (n - 1)) * 100},${100 - v * 96} `;
  }
  const fill = st.graph_style === 1 ? `<polygon points="0,100 ${pts}100,100" fill="currentColor" opacity=".35"/>` : "";
  return `<svg viewBox="0 0 100 100" preserveAspectRatio="none"><polyline points="${pts}" fill="none" stroke="currentColor" stroke-width="2.4" vector-effect="non-scaling-stroke"/>${fill}</svg>`;
}

/** Downloads and installs RivaTuner through winget (from Guru3D), then refreshes the page. */
async function ovInstallRtss() {
  S.cache.rtssInstalling = true;
  render();
  note("Installing RivaTuner: this takes about a minute");
  try {
    await run("rtss_install", null, "RivaTuner installed and running");
  } catch {}
  S.cache.rtssInstalling = false;
  await ovRefresh();
  render();
}

/** Large preview: the preview takes the wide column and the editor the narrow one, so both stay usable. Per viewer. */
const ovBig = () => { try { return localStorage.getItem("deck.ovBig") === "1"; } catch { return false; } };
function ovSetBig(on) {
  try { localStorage.setItem("deck.ovBig", on ? "1" : "0"); } catch {}
  render();
}

function ovPreview(st, r) {
  const body = h("div", { class: "ov-osd" + (st.group_values ? " gv" : "") + (st.shadow ? " shadow" : "") + (st.box_on ? " box" : "") + (st.bold ? " bold" : "") });
  const bar = st.color_bar ? h("div", { class: "ov-bar " + (st.corner[1] === "r" ? "r" : "l") }) : null;
  const screen = h("div", { class: "ov-screen", "data-c": st.corner },
    h("div", { class: "ov-scene" }), bar, body,
    OV_CORNERS.map(([c, name]) => h("button", { class: "ov-pad " + c + (st.corner === c ? " on" : ""), title: name, "aria-label": "Put the overlay " + name.toLowerCase(), onclick: () => ovStyle({ corner: c }) })));
  const v = (k, val) => screen.style.setProperty(k, val);
  // the colours the overlay gets: a saved colour that isn't 6 hex digits falls back like it does there
  const col = (key) => "#" + (/^#?[0-9a-f]{6}$/i.test(st[key] || "") ? st[key].replace("#", "") : OV_DEF[key]);
  v("--lc", col("label_color")); v("--vc", col("value_color")); v("--wc", col("warn_color")); v("--w1", col("warm_color"));
  Object.entries(OV_SLOT_COLOR).forEach(([slot, key]) => v("--g" + slot, col(key)));
  v("--z", st.zoom);
  v("--mx", Math.max(3, st.margin_x / 4) + "px");
  v("--my", Math.max(3, st.margin_y / 4) + "px");
  v("--box", `rgba(0,0,0,${st.box_opacity / 100})`);
  body.style.fontFamily = OV_CSS_FONT[st.font] || `"${st.font}", sans-serif`;
  const draw = () => {
    const lines = (S.cache.rtss || {}).lines || [];
    const gc = st.group_colors;
    const nodes = lines.map((l) => h("div", { class: "ov-ln" + (gc ? " g" + l.slot : "") },
      l.label ? h("span", { class: "ov-l" }, l.label + "  ") : null,
      l.parts.map((p, i) => h("span", { class: "ov-p t" + p.tone },
        p.pre ? h("span", { class: "ov-pre" }, p.pre + " ") : null,
        h("span", { class: "ov-v" }, p.t),
        p.u ? h("span", { class: "ov-u " + st.units }, " " + p.u) : null,
        i < l.parts.length - 1 ? "  " : null))));
    if (st.graph && nodes.length) {
      const g = h("div", { class: "ov-graph" + (st.graph_fit ? " fit" : "") + (gc ? " g7" : ""), style: { width: st.graph_fit ? "" : st.graph_w * 1.6 + "px", height: st.graph_h * 4 + "px" } });
      g.innerHTML = ovGraphSvg(st, (S.cache.rtss || {}).frametime);
      nodes.push(g);
    }
    body.replaceChildren(...(nodes.length ? nodes : [h("div", { class: "ov-ln ov-l" }, "Nothing selected yet")]));
    // a tall overlay shrinks to fit the little screen (the real one is drawn at full size)
    body.style.transform = "";
    requestAnimationFrame(() => {
      const room = screen.clientHeight - 8, need = body.offsetHeight + 6;
      const k = Math.min(1, room / Math.max(1, need), (screen.clientWidth - 8) / Math.max(1, body.offsetWidth + 6));
      body.style.transformOrigin = (st.corner[0] === "b" ? "bottom " : "top ") + (st.corner[1] === "r" ? "right" : "left");
      body.style.transform = k < 1 ? `scale(${k.toFixed(3)})` : "";
    });
  };
  draw();
  HW.updaters.push(draw);
  const live = r.fps != null ? h("span", { class: "chip on num" }, `${Math.round(r.fps)} FPS${r.app ? " · " + r.app : ""}`) : h("span", { class: "dim small" }, "No game running: the framerate shows a dash");
  return h("div", { class: "card col ov-prev" },
    h("div", { class: "tile-head" }, I("eye"), h("span", null, "Preview"), h("span", { class: "grow" }), live,
      h("button", { class: "icon-btn", title: ovBig() ? "Smaller preview" : "Larger preview (keep editing on the left)", "aria-label": ovBig() ? "Smaller preview" : "Larger preview", "aria-pressed": ovBig() ? "true" : "false", onclick: () => ovSetBig(!ovBig()) }, I(ovBig() ? "shrink" : "expand"))),
    screen,
    h("div", { class: "dim small" }, "Your readings, right now. Click a corner to move the overlay there. RivaTuner draws the real thing, so its font and spacing differ a little."));
}

// ------------------------------------------------------------------- what to show

function ovLooksCard(s, r) {
  const known = new Set((r.items || []).map((i) => i.id));
  return h("div", { class: "card col" },
    h("div", { class: "tile-head" }, I("sparkle"), h("span", null, "Quick looks"), h("span", { class: "grow" }), h("span", { class: "dim small ov-hint" }, "Sets the items and the style, then change anything")),
    h("div", { class: "ov-looks" }, OV_LOOKS.map(([name, desc, ids, patch]) => h("button", { class: "ov-look", onclick: async () => {
      await setSetting("osd_items", ids.filter((i) => known.has(i)));
      await ovStyle(patch);
    } }, h("b", null, name), h("span", { class: "dim small" }, desc)))));
}

/** Moves the items in `pick` (keeping their order) so the block starts where `at` was. */
function ovMoveBlock(list, pick, at) {
  const sel = list.filter((x) => pick.has(x));
  const rest = list.filter((x) => !pick.has(x));
  const anchor = list.slice(at).find((x) => !pick.has(x)); // the first unpicked item at or after the drop
  const k = anchor == null ? rest.length : rest.indexOf(anchor);
  return [...rest.slice(0, k), ...sel, ...rest.slice(k)];
}

function ovItemsCard(s, r) {
  const defs = new Map((r.items || []).map((i) => [i.id, i]));
  const catName = Object.fromEntries((r.cats || []).map(([k, l]) => [k, l]));
  const on = (s.osd_items || []).filter((id) => defs.has(id));
  const groups = { ...(ovStyleNow().groups || {}) };
  // the selection survives redraws; items that left the list leave it too
  const sel = (S.cache.ovSel = new Set([...(S.cache.ovSel || [])].filter((id) => on.includes(id))));
  const set = (list) => ovItemsSet(list);
  const multi = (id) => ["cpu_cores", "core_clock", "core_temp", "core_table", "thread_table"].includes(id);
  const pick = (id, e) => {
    if (e?.shiftKey && S.cache.ovAnchor && on.includes(S.cache.ovAnchor)) {
      const [i, j] = [on.indexOf(S.cache.ovAnchor), on.indexOf(id)].sort((x, y) => x - y);
      on.slice(i, j + 1).forEach((x) => sel.add(x));
    } else if (sel.has(id)) sel.delete(id);
    else sel.add(id);
    S.cache.ovAnchor = id;
    render();
  };
  const moveSel = (dir) => {
    const idx = on.map((x, i) => (sel.has(x) ? i : -1)).filter((i) => i >= 0);
    if (!idx.length) return;
    if (dir === "top") return set(ovMoveBlock(on, sel, 0));
    if (dir === "bottom") return set(ovMoveBlock(on, sel, on.length));
    const next = [...on];
    if (dir < 0) {
      if (idx[0] === 0) return;
      for (const i of idx) [next[i - 1], next[i]] = [next[i], next[i - 1]];
    } else {
      if (idx[idx.length - 1] === on.length - 1) return;
      for (const i of [...idx].reverse()) [next[i + 1], next[i]] = [next[i], next[i + 1]];
    }
    set(next);
  };
  const putGroups = async (g) => { await setSetting("osd_style", { ...ovStyleNow(), groups: g }); };
  const groupSel = async () => {
    const ids = on.filter((x) => sel.has(x) && !multi(x));
    if (!ids.length) return note("The per-core and per-thread rows take several lines, so they can't share one", true);
    const was = groups[ids[0]] || "";
    const label = await promptBox("Put on one line", "The label in front of the line (GPU, CPU, SYS…)", was || "SYS", "Put together");
    if (label == null) return;
    const g = { ...groups };
    for (const id of ids) { if (label) g[id] = label; else delete g[id]; }
    await putGroups(g);
    // side by side in the list too, where the first of them was
    await set(ovMoveBlock(on, new Set(ids), on.indexOf(ids[0])));
  };
  const ungroupSel = async () => {
    const g = { ...groups };
    for (const id of sel) delete g[id];
    await putGroups(g);
    await ovRefresh();
    render();
  };
  const move = (from, to) => {
    if (from === to || from < 0 || to < 0 || to >= on.length) return;
    const next = [...on];
    next.splice(to, 0, next.splice(from, 1)[0]);
    set(next);
  };
  const rows = on.map((id, k) => {
    const d = defs.get(id);
    const picked = sel.has(id);
    const box = h("input", { type: "checkbox", class: "ov-check", checked: picked, "aria-label": "Select " + d.short });
    box.addEventListener("click", (e) => { e.preventDefault(); pick(id, e); });
    const g = groups[id];
    const el = h("div", { class: "ov-item" + (d.have ? "" : " none") + (picked ? " picked" : ""), draggable: "true", "data-k": k },
      h("span", { class: "ov-grip", "aria-hidden": "true" }),
      box,
      h("div", { class: "ov-in", onclick: (e) => pick(id, e) }, h("b", null, d.short), h("span", { class: "dim small" }, `${catName[d.cat] || ""} · ${d.label}`)),
      g && !multi(id) ? h("span", { class: "chip sm-chip ov-gchip", title: "On a line of its own called " + g }, I("link", "sm"), g) : null,
      d.have ? null : h("span", { class: "chip sm-chip", title: "This PC gives no such reading right now, so the overlay skips it" }, "no reading"),
      d.stat ? h("span", { class: "chip sm-chip", title: "Counted by Deck from every frame RivaTuner draws, over the time set under Behaviour" }, "from frames") : null,
      h("button", { class: "icon-btn sm", title: "Move up", "aria-label": "Move " + d.short + " up", disabled: k === 0, onclick: () => move(k, k - 1) }, I("moveup", "sm")),
      h("button", { class: "icon-btn sm down", title: "Move down", "aria-label": "Move " + d.short + " down", disabled: k === on.length - 1, onclick: () => move(k, k + 1) }, I("moveup", "sm")),
      h("button", { class: "icon-btn sm danger", title: "Remove", "aria-label": "Remove " + d.short, onclick: () => set(on.filter((x) => x !== id)) }, I("x", "sm")));
    // dragging a selected item takes the whole selection along
    el.addEventListener("dragstart", (e) => {
      e.dataTransfer.setData("text/plain", id);
      e.dataTransfer.effectAllowed = "move";
      (picked ? [...el.parentNode.querySelectorAll(".ov-item.picked")] : [el]).forEach((x) => x.classList.add("drag"));
    });
    el.addEventListener("dragend", () => el.parentNode?.querySelectorAll(".drag").forEach((x) => x.classList.remove("drag")));
    el.addEventListener("dragover", (e) => { e.preventDefault(); el.classList.add("over"); });
    el.addEventListener("dragleave", () => el.classList.remove("over"));
    el.addEventListener("drop", (e) => {
      e.preventDefault();
      el.classList.remove("over");
      const from = e.dataTransfer.getData("text/plain");
      if (!on.includes(from) || from === id) return;
      const block = sel.has(from) ? sel : new Set([from]);
      if (block.has(id)) return;
      // dropped on an item further down: land after it
      set(ovMoveBlock(on, block, on.indexOf(from) < k ? k + 1 : k));
    });
    return el;
  });

  const anyGrouped = [...sel].some((id) => groups[id]);
  const bar = sel.size ? h("div", { class: "ov-selbar", role: "toolbar", "aria-label": "Selected items" },
    h("b", null, `${sel.size} selected`),
    h("button", { class: "btn sm ghost", title: "Move the selected items up", onclick: () => moveSel(-1) }, I("moveup", "sm"), "Up"),
    h("button", { class: "btn sm ghost ov-down", title: "Move the selected items down", onclick: () => moveSel(1) }, I("moveup", "sm"), "Down"),
    h("button", { class: "btn sm ghost", onclick: () => moveSel("top") }, "To the top"),
    h("button", { class: "btn sm ghost", onclick: () => moveSel("bottom") }, "To the bottom"),
    h("button", { class: "btn sm", title: "Show the selected items on one line with a label of your choice", onclick: groupSel }, I("link", "sm"), "Put on one line"),
    anyGrouped ? h("button", { class: "btn sm ghost", onclick: ungroupSel }, "Back to their own lines") : null,
    h("button", { class: "btn sm ghost danger", onclick: () => { const keep = on.filter((x) => !sel.has(x)); sel.clear(); set(keep); } }, I("trash", "sm"), "Remove"),
    h("span", { class: "grow" }),
    h("button", { class: "icon-btn sm", title: "Clear the selection", "aria-label": "Clear the selection", onclick: () => { sel.clear(); render(); } }, I("x", "sm"))) : null;

  const free = (r.items || []).filter((i) => !on.includes(i.id));
  const adders = (r.cats || []).map(([cat, label]) => {
    const list = free.filter((i) => i.cat === cat);
    if (!list.length) return null;
    return h("div", { class: "ov-addrow" }, h("span", { class: "ov-addcat" }, label),
      h("div", { class: "row wrap", style: { gap: "6px" } }, list.map((i) => h("button", { class: "chip ov-add" + (i.have ? "" : " none"), title: i.label + (i.have ? "" : " (no reading on this PC right now)"), onclick: () => set([...on, i.id]) }, I("plus", "sm"), i.short))));
  });

  const every = (r.items || []).filter((i) => i.have && !i.id.startsWith("fan:") && !i.id.startsWith("temp:")).map((i) => i.id);
  const presets = OV_PRESETS.map(([name, ids]) => h("button", { class: "chip", onclick: () => set((ids || every).filter((x) => defs.has(x))) }, name));
  const allPicked = on.length > 0 && on.every((x) => sel.has(x));

  return h("div", { class: "card col" },
    h("div", { class: "tile-head" }, I("list"), h("span", null, `What to show · ${on.length}`), helpTip("ov.items"), h("span", { class: "grow" }),
      on.length ? h("button", { class: "link", onclick: () => { if (allPicked) sel.clear(); else on.forEach((x) => sel.add(x)); render(); } }, allPicked ? "Select none" : "Select all") : null),
    bar,
    on.length ? h("div", { class: "ov-items" }, rows) : h("div", { class: "empty" }, "Nothing is shown. Add something below."),
    h("div", { class: "ov-presets" }, h("span", { class: "dim small" }, "Start from"), presets),
    free.length ? h("div", { class: "ov-adders" }, adders) : null);
}

// ------------------------------------------------------------------- layout

function ovLayoutCard(st, s) {
  const items = s.osd_items || [];
  const hasCores = items.some((i) => ["cpu_cores", "core_clock", "core_temp"].includes(i));
  const hasMem = items.some((i) => ["gpu_mem", "ram", "commit", "proc_ram"].includes(i));
  const demo = {
    grouped: ["GPU  64°C  98%", "CPU  58°C  31%", "FPS  143  6.9ms"],
    list: ["GPU temp   64°C", "GPU load   98%", "FPS        143"],
    line: ["GPU 64°C 98%  CPU 58°C  FPS 143"],
  };
  const pick = (v, title, sub) => h("button", { class: "ov-lay" + (st.layout === v ? " on" : ""), onclick: () => ovStyle({ layout: v }), "aria-pressed": st.layout === v },
    h("pre", null, demo[v].join("\n")), h("b", null, title), h("span", { class: "dim small" }, sub));
  return h("div", { class: "card col" },
    h("div", { class: "tile-head" }, I("layout"), h("span", null, "Layout"), helpTip("ov.layout")),
    h("div", { class: "ov-lays" }, pick("grouped", "Grouped", "One line per group"), pick("list", "List", "One line per item"), pick("line", "One line", "All in a row")),
    ovRow("Labels", toggle(st.labels, (v) => ovStyle({ labels: v })), "GPU, CPU, FPS… in front of the numbers"),
    st.labels && st.layout !== "line" ? ovRow("Line up the values", toggle(st.align, (v) => ovStyle({ align: v })), "Pads the labels so the numbers start in one column") : null,
    st.layout === "grouped" ? ovRow("Framerate figures on their own lines", toggle(st.fps_lines, (v) => ovStyle({ fps_lines: v })), "FPS, Min, Avg, Max and the lows each get a line, like Afterburner", "ov.fps_lines") : null,
    st.layout !== "line" ? ovRow("Tidy columns", toggle(st.columns, (v) => ovStyle({ columns: v })), "Numbers line up under each other (best with a monospace font)", "ov.columns") : null,
    ovRow("Units", seg([["small", "Small"], ["normal", "Normal"], ["off", "None"]], st.units, (v) => ovStyle({ units: v })), "°C, %, MHz after each number"),
    items.includes("time") ? ovRow("Clock", h("div", { class: "row", style: { gap: "12px" } }, seg([[false, "24 h"], [true, "12 h"]], st.clock_12h, (v) => ovStyle({ clock_12h: v })), seg([[false, "No seconds"], [true, "Seconds"]], st.clock_secs, (v) => ovStyle({ clock_secs: v })))) : null,
    hasMem ? ovRow("Memory in", seg([["MB", "MB"], ["GB", "GB"]], st.mem_unit, (v) => ovStyle({ mem_unit: v }))) : null,
    ovRow("Decimals in the framerate", toggle(!st.int_fps, (v) => ovStyle({ int_fps: !v })), "143.4 instead of 143", "ov.decimals"),
    hasCores ? ovRow("Cores per row", seg([4, 6, 8, 10, 12, 16].map((n) => [n, String(n)]), st.cores_per_row, (v) => ovStyle({ cores_per_row: v })), "For the per-thread and per-core rows") : null);
}

// ------------------------------------------------------------------- colours

function ovColorsCard(st) {
  const same = (t) => Object.entries(t).every(([k, v]) => st[k] === v);
  return h("div", { class: "card col" },
    h("div", { class: "tile-head" }, I("droplet"), h("span", null, "Colours")),
    h("div", { class: "ov-themes" }, OV_THEMES.map(([name, t]) => h("button", { class: "ov-theme" + (same(t) ? " on" : ""), onclick: () => ovStyle(t), "aria-pressed": same(t) },
      h("span", { class: "sw" }, [t.label_color, t.value_color, t.group_colors ? t.gpu_color : t.warn_color, t.group_colors ? t.cpu_color : t.warm_color].map((c) => h("i", { style: { background: "#" + c } }))), name))),
    h("div", { class: "ov-colors" }, ovColorField("Labels", "label_color", st), ovColorField("Numbers", "value_color", st), ovColorField("Warm", "warm_color", st), ovColorField("Hot", "warn_color", st)),
    ovSwitch("A colour for each part", "GPU, CPU, memory and framerate lines each get their own.", st.group_colors, (v) => ovStyle({ group_colors: v })),
    st.group_colors ? ovSwitch("Colour the numbers too", "Otherwise only the labels take the part's colour.", st.group_values, (v) => ovStyle({ group_values: v })) : null,
    st.group_colors ? h("div", { class: "ov-colors" }, ovColorField("GPU", "gpu_color", st), ovColorField("CPU", "cpu_color", st), ovColorField("Memory", "ram_color", st), ovColorField("Framerate", "fps_color", st), ovColorField("Other", "info_color", st)) : null,
    ovSwitch("Colour numbers by how loaded or hot they are", "Warm from 70 % load or 12 °C under the temperature limit, hot from 90 % or the limit.", st.heat, (v) => ovStyle({ heat: v }), "ov.heat"));
}

// ------------------------------------------------------------------- how RivaTuner draws it

function ovLookCard(st, r) {
  const put = (patch) => ovStyle(patch);
  // a real list: the suggested overlay fonts first, then every font installed on this PC
  if (!S.cache.fonts) invoke("fonts_list").then((f) => { S.cache.fonts = f; if (S.page === "overlay") render(); }).catch(() => (S.cache.fonts = []));
  const installed = (S.cache.fonts || []).filter((f) => !OV_FONTS.includes(f));
  const opt = (f) => h("option", { value: f, selected: f === st.font }, f);
  const font = h("select", { class: "ov-font", "aria-label": "Font", onchange: (e) => put({ font: e.target.value }) },
    OV_FONTS.includes(st.font) || installed.includes(st.font) ? null : opt(st.font),
    h("optgroup", { label: "Suggested" }, OV_FONTS.map(opt)),
    installed.length ? h("optgroup", { label: "All fonts on this PC" }, installed.map(opt)) : null);
  return h("div", { class: "card col" },
    h("div", { class: "tile-head" }, I("sliders"), h("span", null, "Look in the game"), h("span", { class: "grow" }),
      r.installed && !r.profiles ? h("span", { class: "chip warn", title: "RivaTuner makes its profiles the first time it runs" }, "Start RivaTuner once first") : null),
    ovRow("Position", seg(OV_CORNERS, st.corner, (v) => put({ corner: v })), null, "ov.position"),
    sliderRow("Distance from the side", st.margin_x, 1, 600, 1, " px", (v) => put({ margin_x: v })),
    sliderRow("Distance from the top/bottom", st.margin_y, 1, 600, 1, " px", (v) => put({ margin_y: v })),
    sliderRow("Text size", st.zoom, 1, 8, 1, "×", (v) => put({ zoom: v }), "ov.zoom"),
    ovRow("Font", h("div", { class: "row", style: { gap: "10px" } }, font,
      h("div", { class: "row", style: { gap: "6px" } }, toggle(st.bold, (v) => put({ bold: v })), h("span", { class: "dim" }, "Bold"))), "Any font installed on this PC", "ov.font"),
    ovRow("Text shadow", toggle(st.shadow, (v) => put({ shadow: v })), "A dark outline that keeps the letters readable"),
    ovRow("Dark box behind the text", toggle(st.box_on, (v) => put({ box_on: v }))),
    st.box_on ? sliderRow("Box opacity", st.box_opacity, 5, 100, 5, " %", (v) => put({ box_opacity: v })) : null,
    ovRow("Shrink to fit the screen", toggle(st.fit, (v) => put({ fit: v })), "For long overlays on small screens"),
    ovRow("Frametime graph", toggle(st.graph, (v) => put({ graph: v })), "A graph of the game's frametimes under the text", "ov.graph"),
    st.graph ? ovRow("Graph width", seg([[true, "Fit the overlay"], [false, "Set it"]], st.graph_fit, (v) => put({ graph_fit: v })), "Fit: as wide as the longest line, and it follows when the overlay changes", "ov.graph_fit") : null,
    st.graph && !st.graph_fit ? sliderRow("Width", st.graph_w, 8, 128, 2, " chars", (v) => put({ graph_w: v })) : null,
    st.graph ? sliderRow("Graph height", st.graph_h, 1, 24, 1, " lines", (v) => put({ graph_h: v })) : null,
    st.graph ? ovRow("Graph shows", seg([[5, "5 s"], [10, "10 s"], [20, "20 s"], [30, "30 s"], [60, "1 min"]], st.graph_secs, (v) => put({ graph_secs: v })), "Longer moves slower", "ov.graph_secs") : null,
    st.graph ? ovRow("Smooth the line", toggle(st.graph_smooth, (v) => put({ graph_smooth: v })), "Each point is the average of its frames. Off: the slowest frame, so every hitch shows", "ov.graph_smooth") : null,
    st.graph ? ovRow("Graph style", seg([[0, "Lines"], [1, "Filled"], [2, "Bars"]], st.graph_style === 3 ? 0 : st.graph_style, (v) => put({ graph_style: v }))) : null,
    st.graph ? sliderRow("Graph top", st.graph_max, 0, 100, 1, st.graph_max ? " ms" : " auto", (v) => put({ graph_max: v })) : null,
    ovRow("Frame time bar", toggle(st.color_bar, (v) => put({ color_bar: v })), "A coloured strip along the screen edge that changes colour on every frame", "ov.color_bar"),
    ovRow("Use this look in every game", toggle(st.all_games, (v) => put({ all_games: v })), "A game with its own RivaTuner profile ignores the global one", "ov.all_games"),
    h("div", { class: "row wrap" },
      h("span", { class: "dim small grow" }, "Written to RivaTuner's profiles. Your originals are kept in RivaTuner\\Profiles\\Deck backup."),
      h("button", { class: "btn sm ghost", onclick: async () => {
        if (!(await confirmBox("Put RivaTuner's look back", "RivaTuner's profile files go back to how they were before Deck first changed them.", "Put back"))) return;
        try { const n = await invoke("rtss_restore_look"); note(n + " profile" + (n === 1 ? "" : "s") + " restored"); } catch (e) { note(String(e), true); }
      } }, I("undo", "sm"), "Put RivaTuner's look back")));
}

// ------------------------------------------------------------------- warnings and behaviour

function ovWarnCard(st) {
  return h("div", { class: "card col" },
    h("div", { class: "tile-head" }, I("alert"), h("span", null, "Warnings on the overlay"), helpTip("ov.warn")),
    ovSwitch("Turn a number red when it gets too high", "Temperatures at or above the limit, and the framerate under its limit.", st.warn, (v) => ovStyle({ warn: v })),
    st.warn || st.heat ? sliderRow("GPU temperature limit", st.warn_gpu_temp, 60, 100, 1, " °C", (v) => ovStyle({ warn_gpu_temp: v })) : null,
    st.warn || st.heat ? sliderRow("CPU temperature limit", st.warn_cpu_temp, 60, 105, 1, " °C", (v) => ovStyle({ warn_cpu_temp: v })) : null,
    st.warn ? sliderRow("Framerate under", st.warn_fps, 0, 240, 5, st.warn_fps ? " FPS" : " off", (v) => ovStyle({ warn_fps: v })) : null);
}

/** RivaTuner's framerate limiter: its key (like Afterburner's), on/off now, and the limit. */
function ovLimiterRow(s) {
  const l = S.cache.limiter;
  if (!l) invoke("rtss_limiter", { what: "state" }).then((r) => { S.cache.limiter = r; if (S.page === "overlay") render(); }).catch(() => {});
  const act = async (what, fps) => { try { S.cache.limiter = await run("rtss_limiter", { what, fps }); } catch {} render(); };
  const fps = h("input", { type: "number", min: 0, max: 1000, step: 1, value: l?.fps ?? "", style: { width: "74px" }, "aria-label": "Limit in frames a second", title: "0 = no limit",
    onchange: (e) => act("set", Math.max(0, Math.min(1000, +e.target.value || 0))) });
  return ovRow("Framerate limit", h("div", { class: "row wrap", style: { gap: "8px", justifyContent: "flex-end" } },
    fps, h("span", { class: "dim" }, "FPS"),
    l && l.on != null ? toggle(!!l.on, () => act("toggle")) : null,
    hotkeyField(s.limiter_hotkey, "limiter", (k) => setSetting("limiter_hotkey", k).then(render))),
    "RivaTuner's limiter: holds every game to this framerate. The key turns it on and off in game, like Afterburner's limiter key", "ov.limiter");
}

function ovBehaviourCard(st, s) {
  return h("div", { class: "card col" },
    h("div", { class: "tile-head" }, I("gear"), h("span", null, "Behaviour")),
    ovRow("Show or hide with", hotkeyField(s.osd_hotkey, "overlay", (k) => setSetting("osd_hotkey", k).then(render)), "Page Up, Page Down, Home, End, Insert, Delete, Pause or an F-key work on their own; other keys need Ctrl, Alt or Win"),
    ovRow("Update every", seg([[250, "¼ s"], [500, "½ s"], [1000, "1 s"], [2000, "2 s"]], s.osd_ms, async (v) => { await setSetting("osd_ms", v); render(); }), null, "ov.update"),
    ovRow("Start the figures over with", h("div", { class: "row", style: { gap: "8px" } },
      hotkeyField(s.osd_reset_hotkey, "overlay_reset", (k) => setSetting("osd_reset_hotkey", k).then(render)),
      h("button", { class: "btn sm ghost", onclick: () => invoke("rtss_reset_stats").then(() => note("Framerate figures start over")) }, I("refresh", "sm"), "Now")),
      "Average, lows, lowest, highest and the graph. Handy at the start of a match or benchmark run", "ov.reset"),
    ovLimiterRow(s),
    ovRow("Average and lows cover", seg([[30, "30 s"], [60, "1 min"], [120, "2 min"], [300, "5 min"]], st.stats_secs, (v) => ovStyle({ stats_secs: v })), "How far back the average, 1% low, lowest and highest look", "ov.stats"),
    ovSwitch("Start RivaTuner with Deck", null, s.rtss_autostart, (v) => setSetting("rtss_autostart", v)),
    ovSwitch("Hide RivaTuner's tray icon", "RivaTuner keeps running; open its settings with the button below", s.rtss_tray_hidden, async (v) => {
      try { await invoke("rtss_tray", { hidden: v }); S.state.settings.rtss_tray_hidden = v; } catch (e) { note(String(e)); }
    }),
    ovRow("RivaTuner's own settings", h("button", { class: "btn sm", onclick: () => invoke("rtss_open").catch((e) => note(String(e))) }, I("external", "sm"), "Open RivaTuner"), "RivaTuner restarts so its window opens properly; a game's overlay blinks for a moment"),
    ovSwitch("Share readings like Afterburner", "Lets RivaTuner's overlay editor and stream widgets read the same numbers.", s.mahm_on, (v) => setSetting("mahm_on", v), "ov.mahm"));
}

// ------------------------------------------------------------------- the page

/** Its own sidebar page (Monitor → Overlay). It needs RivaTuner, not this PC's hardware, so it shows on every PC. */
Pages.overlay = {
  async load() {
    await ovRefresh();
  },
  async tick() {
    await ovRefresh();
    for (const f of HW.updaters) { try { f(); } catch (e) { console.error(e); } }
    return false;
  },
  render() {
    HW.updaters = [];
    return h("div", null, head("Overlay", "Your readings over your games: what to show, how it looks and the key that shows it. RivaTuner draws it."), overlayPage());
  },
};

function overlayPage() {
  const s = S.state.settings;
  const st = ovStyleNow();
  const r = S.cache.rtss || {};
  const on = !!s.osd_on;
  const status = !r.installed
    ? h("span", { class: "chip warn" }, "RivaTuner isn't installed")
    : r.running ? h("span", { class: "chip good" }, I("check"), "RivaTuner running") : h("span", { class: "chip warn" }, "RivaTuner isn't running");
  const master = h("div", { class: "card ov-master" + (on ? " on" : "") },
    toggle(on, async (v) => { await setSetting("osd_on", v); await ovRefresh(); render(); }),
    h("div", { class: "grow" },
      h("div", { class: "ov-title" }, on ? "The overlay is on" : "The overlay is off"),
      h("div", { class: "dim" }, !r.installed
        ? "RivaTuner Statistics Server draws the overlay in games. Deck installs it from its maker (Guru3D) in about a minute, then sets it up and runs it for you."
        : on ? "Deck sends the readings to RivaTuner, which draws them in games." : "Turn it on to see the readings over your games.")),
    status,
    r.installed && !r.running ? h("button", { class: "btn sm", onclick: async () => { await run("rtss_start", null, "Starting RivaTuner…"); setTimeout(async () => { await ovRefresh(); render(); }, 1500); } }, "Start RivaTuner") : null,
    !r.installed ? h("button", { class: "btn sm primary", disabled: S.cache.rtssInstalling ? "" : null, onclick: ovInstallRtss }, S.cache.rtssInstalling ? "Installing…" : "Install RivaTuner") : null);
  return h("div", { class: "ov" + (ovBig() ? " big" : "") },
    master,
    h("div", { class: "ov-cols" },
      h("div", { class: "ov-main" }, ovLooksCard(s, r), ovItemsCard(s, r), ovLayoutCard(st, s), ovColorsCard(st), ovLookCard(st, r), ovWarnCard(st), ovBehaviourCard(st, s)),
      h("div", { class: "ov-side" }, ovPreview(st, r))));
}
