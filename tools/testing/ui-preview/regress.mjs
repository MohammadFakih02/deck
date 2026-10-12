// Regression checks for Deck's UI shell, run against the fake backend:
//   node regress.mjs           prints PASS/FAIL per check, exit code 1 on any failure
import { openDeck } from "./harness.mjs";
const d = await openDeck({ port: +(process.env.PORT || 9371) });
let failed = 0;
const check = (name, ok, info = "") => { console.log(`${ok ? "PASS" : "FAIL"}  ${name}${info ? "  (" + info + ")" : ""}`); if (!ok) failed++; };

// 1. A click survives a live redraw that lands between mouse-down and mouse-up.
await d.ev(`go("audio")`);
await d.sleep(600);
const r = await d.ev(`(() => { const b = document.querySelector(".mix-mute"); const x = b.getBoundingClientRect(); return [x.left + x.width / 2, x.top + x.height / 2]; })()`);
await d.ev(`window.__mockCalls.length = 0`);
await d.mouse("mouseMoved", r[0], r[1]);
await d.mouse("mousePressed", r[0], r[1]);
await d.ev(`tick()`); // the 1-second refresh arrives while the button is held
await d.sleep(50);
await d.mouse("mouseReleased", r[0], r[1]);
await d.sleep(200);
const calls = await d.ev(`window.__mockCalls.filter((c) => c === "set_master").length`);
check("click on a live page is not lost to a refresh", calls === 1, `set_master calls: ${calls}`);
await d.sleep(1200); // let the deferred redraw happen

// 1b. Same on Home, whose tiles refresh every second.
await d.ev(`go("home")`);
await d.sleep(600);
const pp = await d.ev(`(() => { const b = document.querySelector(".tile .play, button.play"); const x = b.getBoundingClientRect(); return [x.left + x.width / 2, x.top + x.height / 2]; })()`);
await d.ev(`window.__mockCalls.length = 0`);
await d.mouse("mouseMoved", pp[0], pp[1]);
await d.mouse("mousePressed", pp[0], pp[1]);
await d.ev(`tick()`);
await d.sleep(50);
await d.mouse("mouseReleased", pp[0], pp[1]);
await d.sleep(200);
const mc = await d.ev(`window.__mockCalls.filter((c) => c === "media_control").length`);
check("click on Home is not lost to a tile refresh", mc === 1, `media_control calls: ${mc}`);

// 2. Keyboard focus stays on the same control across a live redraw.
await d.ev(`go("system", "processes")`);
await d.sleep(600);
await d.ev(`(() => { const b = [...document.querySelectorAll(".content button")].find((x) => /End/.test(x.textContent)); b.focus(); window.__f = b.textContent + "|" + [...document.querySelectorAll(".content button")].indexOf(b); })()`);
await d.ev(`tick()`);
await d.sleep(300);
const same = await d.ev(`(() => { const a = document.activeElement; return a && a.tagName === "BUTTON" && document.querySelector(".content").contains(a) ? a.textContent + "|" + [...document.querySelectorAll(".content button")].indexOf(a) : "lost:" + (a && a.tagName); })()`);
const was = await d.ev(`window.__f`);
check("keyboard focus survives a live redraw", same === was, `${was} -> ${same}`);

// 3. Every switch and slider has an accessible name; switches say they are switches.
for (const [p, s] of [["settings", null], ["boost", null], ["audio", null], ["fans", null], ["windows", "open"]]) {
  await d.ev(`go(${JSON.stringify(p)}, ${JSON.stringify(s)})`);
  await d.sleep(500);
  const bad = await d.ev(`[...document.querySelectorAll(".panel .switch input, .panel input[type=range]")].filter((i) => !(i.getAttribute("aria-label") || (i.labels && [...i.labels].some((l) => l.textContent.trim())))).length`);
  const role = await d.ev(`[...document.querySelectorAll(".panel .switch input")].every((i) => i.getAttribute("role") === "switch")`);
  check(`${p}${s ? "/" + s : ""}: switches and sliders are named`, bad === 0 && role, `unnamed ${bad}, role ok ${role}`);
}

// 4. Icon-only buttons have a name.
await d.ev(`go("home")`);
await d.sleep(600);
const unnamed = await d.ev(`[...document.querySelectorAll(".panel button")].filter((b) => b.offsetParent && !(b.getAttribute("aria-label") || b.textContent.trim() || b.title)).length`);
check("home: icon buttons are named", unnamed === 0, `unnamed ${unnamed}`);

// 5. The active nav item is marked for screen readers; toasts are announced.
const cur = await d.ev(`document.querySelectorAll(".side [aria-current=page]").length`);
check("sidebar marks the current page", cur === 1, `aria-current count ${cur}`);
const live = await d.ev(`document.getElementById("toast").getAttribute("aria-live")`);
check("toasts are announced", live === "polite", `aria-live ${live}`);

// 6. Dialogs: role, focus inside, Tab stays inside, focus returns to the opener, Escape closes.
await d.ev(`go("tools", "notes")`);
await d.sleep(500);
await d.ev(`(() => { const b = document.querySelector(".content button"); b.focus(); window.__opener = b; confirmBox("Delete?", "Sure?", "Delete", true); })()`);
await d.sleep(150);
const dlg = await d.ev(`(() => { const m = document.querySelector(".modal"); return { role: m.getAttribute("role"), modal: m.getAttribute("aria-modal"), inside: m.contains(document.activeElement) }; })()`);
check("dialog has role and takes focus", dlg.role === "dialog" && dlg.modal === "true" && dlg.inside, JSON.stringify(dlg));
for (let i = 0; i < 4; i++) await d.key("Tab", "Tab", 9);
const stillIn = await d.ev(`document.querySelector(".modal").contains(document.activeElement)`);
check("Tab stays inside the dialog", stillIn === true);
await d.key("Escape", "Escape", 27);
await d.sleep(100);
const back = await d.ev(`({ gone: !document.querySelector(".modal"), back: document.activeElement === window.__opener })`);
check("Escape closes the dialog and focus returns", back.gone && back.back, JSON.stringify(back));

// 7. Space on a focused button presses it (it must not jump to the search box).
await d.ev(`go("power", "timers")`);
await d.sleep(500);
await d.ev(`(() => { window.__mockCalls.length = 0; const b = [...document.querySelectorAll(".content button")].find((x) => x.textContent.trim() === "5m"); b.focus(); })()`);
await d.send("Input.dispatchKeyEvent", { type: "keyDown", key: " ", code: "Space", windowsVirtualKeyCode: 32, text: " " });
await d.send("Input.dispatchKeyEvent", { type: "keyUp", key: " ", code: "Space", windowsVirtualKeyCode: 32 });
await d.sleep(200);
const sp = await d.ev(`({ calls: [...window.__mockCalls], q: document.getElementById("q").value, active: document.activeElement.id })`);
check("Space presses the focused button", sp.calls.includes("timer_add") && sp.q === "", JSON.stringify(sp));
await d.ev(`document.getElementById("q").value = ""`);

// 8. Typing a letter while a button has focus still goes to search.
await d.ev(`document.querySelector(".content button").focus()`);
await d.send("Input.dispatchKeyEvent", { type: "keyDown", key: "c", code: "KeyC", windowsVirtualKeyCode: 67, text: "c" });
await d.send("Input.dispatchKeyEvent", { type: "keyUp", key: "c", code: "KeyC", windowsVirtualKeyCode: 67 });
await d.sleep(150);
const ty = await d.ev(`document.activeElement.id`);
check("typing a letter jumps to search", ty === "q", ty);

// 8b. A sidebar click survives the 1-second tick (the sidebar used to be rebuilt every tick).
await d.ev(`go("home")`);
await d.sleep(500);
const nv = await d.ev(`(() => { const b = [...document.querySelectorAll(".side .nav")].find((x) => /^Sound/.test(x.textContent.trim())); const x = b.getBoundingClientRect(); return [x.left + x.width / 2, x.top + x.height / 2]; })()`);
await d.mouse("mouseMoved", nv[0], nv[1]);
await d.mouse("mousePressed", nv[0], nv[1]);
await d.ev(`tick()`);
await d.sleep(50);
await d.mouse("mouseReleased", nv[0], nv[1]);
await d.sleep(300);
const pg = await d.ev(`S.page + "/" + S.sub`);
check("sidebar click is not lost to a tick", pg === "audio/null", pg);

// 8c. A click survives a redraw that a backend event triggers (network ping, timers, clipboard...).
await d.ev(`go("power", "timers")`);
await d.sleep(500);
const tb = await d.ev(`(() => { const b = [...document.querySelectorAll(".content button")].find((x) => x.textContent.trim() === "5m"); const x = b.getBoundingClientRect(); return [x.left + x.width / 2, x.top + x.height / 2]; })()`);
await d.ev(`window.__mockCalls.length = 0`);
await d.mouse("mouseMoved", tb[0], tb[1]);
await d.mouse("mousePressed", tb[0], tb[1]);
await d.ev(`render()`);
await d.sleep(50);
await d.mouse("mouseReleased", tb[0], tb[1]);
await d.sleep(300);
const tcalls = await d.ev(`window.__mockCalls.filter((c) => c === "timer_add").length`);
check("click is not lost to an event-driven redraw", tcalls === 1, `timer_add calls: ${tcalls}`);

// 9. Enter right after typing acts on the new query, not on the previous query's results
//    (the backend answers each keystroke a moment later).
await d.ev(`(() => {
  const real = T.core.invoke;
  window.__copied = [];
  T.core.invoke = async (cmd, args) => {
    if (cmd === "search") { await new Promise((r) => setTimeout(r, 400)); const v = Function("return (" + args.q + ")")(); return { q: args.q, apps: [], calc: { value: String(v), raw: v }, convert: null, files: [], clips: [], games: [] }; }
    if (cmd === "set_clipboard") { window.__copied.push(args.text); return null; }
    return real(cmd, args);
  };
  window.__realInvoke = real;
})()`);
await d.ev(`(() => { const q = document.getElementById("q"); q.focus(); q.value = "2+2"; q.dispatchEvent(new Event("input")); })()`);
await d.sleep(700);
await d.ev(`(() => { const q = document.getElementById("q"); q.value = "2+23"; q.dispatchEvent(new Event("input")); })()`);
await d.key("Enter", "Enter", 13);
await d.sleep(800);
const copied = await d.ev(`window.__copied.join(",")`);
check("Enter after typing uses the new query's answer", copied === "25", `copied: ${copied || "nothing"}`);
await d.ev(`(() => { T.core.invoke = window.__realInvoke; const q = document.getElementById("q"); q.value = ""; q.dispatchEvent(new Event("input")); })()`);

// 10. Keep awake: one click on a length starts it; Turn off stops it.
const clickText = async (sel, re) => {
  const r = await d.ev(`(() => { const b = [...document.querySelectorAll(${JSON.stringify(sel)})].find((x) => ${re}.test(x.textContent.trim())); if (!b) return null; b.scrollIntoView({ block: "center" }); const x = b.getBoundingClientRect(); return [x.left + x.width / 2, x.top + x.height / 2]; })()`);
  if (!r) return false;
  await d.mouse("mouseMoved", r[0], r[1]); await d.mouse("mousePressed", r[0], r[1]); await d.mouse("mouseReleased", r[0], r[1]);
  await d.sleep(150);
  await d.ev(`(async () => { S.live = await invoke("live"); render(); })()`);
  await d.sleep(150);
  return true;
};
await d.ev(`go("power", "awake")`);
await d.sleep(500);
await clickText(".pw-opt", "/^1 hour/");
const aw = await d.ev(`({ on: S.live.power.awake_active, mode: S.live.power.awake.mode, left: Math.round((S.live.power.awake.until - Date.now() / 1000) / 60), pressed: document.querySelector(".pw-opt.on")?.textContent })`);
check("keep awake: one click on 1 hour starts it", aw.on && aw.mode === "until" && aw.left === 60 && /1 hour/.test(aw.pressed || ""), JSON.stringify(aw));
await clickText(".pw-hero button", "/Turn off/");
check("keep awake: Turn off stops it", (await d.ev(`S.live.power.awake_active`)) === false);

// 11. Shut down when: build a plan, see it waiting, cancel it.
await d.ev(`go("power", "plan")`);
await d.sleep(500);
await clickText(".pw-act", "/^Sleep$/");
await clickText(".pw-cond", "/^After a while/");
const sum = await d.ev(`document.querySelector(".pw-sum").textContent`);
await clickText(".pw-go button", "/Start/");
const pl = await d.ev(`(() => { const p = S.live.power.plan; return p && { action: p.action, cond: p.cond, mins: Math.round((p.at - Date.now() / 1000) / 60), hero: document.querySelector(".pw-hero .boost-say")?.textContent }; })()`);
check("shut down when: builder sets the plan it shows", sum === "Sleep in 30 min" && pl && pl.action === "sleep" && pl.cond === "time" && pl.mins === 30 && /^Sleep at/.test(pl.hero), `${sum} ${JSON.stringify(pl)}`);
await clickText(".pw-hero button", "/Cancel/");
check("shut down when: Cancel clears it", (await d.ev(`S.live.power.plan`)) === null);

// 11b. Typed commands: "awake 2h" keeps the PC awake for two hours.
await d.ev(`(() => { window.__awakeArgs = null; const real = T.core.invoke; window.__realInvoke2 = real; T.core.invoke = async (c, a) => { if (c === "set_awake") window.__awakeArgs = a; if (c === "search") return { q: a.q, apps: [], calc: null, convert: null, files: [], clips: [], games: [] }; return real(c, a); }; const q = document.getElementById("q"); q.focus(); q.value = "awake 2h"; q.dispatchEvent(new Event("input")); })()`);
await d.sleep(300);
await d.key("Enter", "Enter", 13);
await d.sleep(300);
const awk = await d.ev(`JSON.stringify(window.__awakeArgs)`);
check("search: \"awake 2h\" keeps the PC awake 2 hours", awk === JSON.stringify({ mode: "until", minutes: 120 }), awk);
await d.ev(`(() => { T.core.invoke = window.__realInvoke2; const q = document.getElementById("q"); q.value = ""; q.dispatchEvent(new Event("input")); })()`);

// 12. Clean up never empties the Recycle Bin unless it is ticked by hand.
await d.ev(`(() => { delete S.cache.cleanSel; go("files", "cleanup"); })()`);
await d.sleep(600);
const rb = await d.ev(`(() => { const l = [...document.querySelectorAll("label.clean")].find((x) => /Recycle Bin/.test(x.textContent)); const sec = l && l.closest("div").parentElement.querySelector(".section h2")?.textContent; return { ticked: l ? l.querySelector("input").checked : null, inSafe: /Safe to clean/.test(sec || "") }; })()`);
check("clean up: Recycle Bin is not ticked or called safe by default", rb.ticked === false && !rb.inSafe, JSON.stringify(rb));

// 13. Disk space offers no Recycle Bin button on Windows' own folders and files.
await d.ev(`go("files", "space")`);
await d.sleep(800);
const dk = await d.ev(`(() => { const row = (n) => [...document.querySelectorAll(".item")].find((x) => x.querySelector(".t")?.textContent === n); const has = (n) => { const r = row(n); return r ? (r.querySelector("button[title='Move to Recycle Bin']") ? "trash" : r.querySelector("[aria-label=Protected]") ? "lock" : "none") : "missing"; }; return [has("Windows"), has("Users"), has("Intel"), has("render_final.mp4"), has("pagefile.sys")].join(","); })()`);
check("disk space: system folders get a lock, not a trash button", dk === "lock,lock,trash,trash,lock", dk);

// 14. Alt-tab away from a profile editor (Deck hides): the editor and its "Try it" stay; a plain
//     question closes. Coming back puts the focus in the editor, not the search box.
await d.ev(`go("home")`);
await d.sleep(300);
await d.ev(`(() => { window.__dis = 0; modal("Edit GPU profile", h("div", null, h("input", { id: "kept-in", "aria-label": "Name" })), () => [h("button", { class: "btn" }, "Save")], { wide: true, keep: true, onDismiss: () => window.__dis++ }); confirmBox("Delete it?", "", "Delete", true); })()`);
await d.sleep(200);
await d.ev(`window.__emit("hiding")`);
await d.sleep(200);
const kept = await d.ev(`(() => { const t = [...document.querySelectorAll(".modal-back")].map((m) => m.textContent); return { editor: t.some((x) => /Edit GPU profile/.test(x)), question: t.some((x) => /Delete it/.test(x)), undone: window.__dis }; })()`);
await d.ev(`window.__emit("shown", {})`);
await d.sleep(600);
const back2 = await d.ev(`(() => { const a = document.activeElement; return a && a.closest(".modal") ? "editor" : (a && a.id) || "none"; })()`);
check("alt-tab keeps an open profile editor and its trial; focus returns to it", kept.editor && !kept.question && kept.undone === 0 && back2 === "editor", JSON.stringify({ ...kept, focus: back2 }));
await d.ev(`document.querySelectorAll(".modal-back").forEach((m) => m.remove())`);
const both = await d.ev(`fetch("js/profiles.js").then((r) => r.text()).then((s) => (s.match(/wide: true, keep: true/g) || []).length)`);
check("both profile editors are kept while hidden", both === 2, `found ${both}`);

// 15. The sidebar: Notes is gone; Snap zones and Workspaces are tabs of Windows, Locked files and
//     Windows panels tabs of Tools.
await d.ev(`go("windows")`);
await d.sleep(400);
const nav = await d.ev(`[...document.querySelectorAll("#side .nav span")].map((x) => x.textContent)`);
const wtabs = await d.ev(`[...document.querySelectorAll(".page-head .seg button")].map((x) => x.textContent).join("|")`);
await d.ev(`go("tools")`);
await d.sleep(400);
const ttabs = await d.ev(`[...document.querySelectorAll(".page-head .seg button")].map((x) => x.textContent).join("|")`);
check("sidebar merges Windows and Tools pages and drops Notes",
  nav.includes("Windows") && nav.includes("Tools") && !["Notes", "Snap zones", "Workspaces", "Screen tools", "Locked files", "Windows panels"].some((x) => nav.includes(x))
    && wtabs === "Open windows|Snap zones|Workspaces" && ttabs === "Screen tools|Locked files|Windows panels", `${wtabs} / ${ttabs}`);
await d.ev(`(async () => { window.__mockCalls.length = 0; await go("settings"); })()`);
await d.sleep(300);
const tileNames = await d.ev(`document.querySelector(".content").textContent.includes("Quick note")`);
check("Notes tile is gone from the Home tiles list", !tileNames);

// 16. Network: pings once on opening, then only while live pinging is on.
await d.ev(`go("system", "network")`);
await d.sleep(800);
await d.ev(`window.__mockCalls.length = 0`);
for (let i = 0; i < 3; i++) { await d.ev(`tick()`); await d.sleep(150); }
const idle = await d.ev(`window.__mockCalls.filter((c) => c === "net_ping").length`);
await d.ev(`document.querySelector(".net-ping .btn").click()`);
await d.sleep(200);
for (let i = 0; i < 3; i++) { await d.ev(`tick()`); await d.sleep(150); }
const running = await d.ev(`window.__mockCalls.filter((c) => c === "net_ping").length`);
const stopLabel = await d.ev(`document.querySelector(".net-ping .btn").textContent`);
await d.ev(`document.querySelector(".net-ping .btn").click()`);
await d.sleep(300);
await d.ev(`window.__mockCalls.length = 0`);
for (let i = 0; i < 3; i++) { await d.ev(`tick()`); await d.sleep(150); }
const after = await d.ev(`window.__mockCalls.filter((c) => c === "net_ping").length`);
check("ping runs only between Start and Stop", idle === 0 && running >= 2 && stopLabel === "Stop" && after === 0, `idle ${idle}, live ${running}, after stop ${after}`);

// 17. CPU and GPU editors: every setting has a "?" that explains it on hover.
await d.ev(`go("cpu")`);
await d.sleep(500);
await d.ev(`editCpu(S.cache.cpu.profiles[1])`);
await d.sleep(300);
const cpuTips = await d.ev(`document.querySelectorAll(".modal .help-q").length`);
await d.ev(`document.querySelector(".modal .help-q").dispatchEvent(new MouseEvent("mouseenter"))`);
const card = await d.ev(`(() => { const c = document.querySelector(".help-card"); return c ? c.textContent.slice(0, 40) : null; })()`);
await d.ev(`document.querySelector(".modal .help-q").dispatchEvent(new MouseEvent("mouseleave"))`);
const gone = await d.ev(`!document.querySelector(".help-card")`);
await d.ev(`document.querySelectorAll(".modal-back").forEach((m) => m.remove())`);
await d.ev(`(async () => { await go("gpu"); await editGpu(S.cache.gpu.profiles[0]); })()`);
await d.sleep(400);
const gpuTips = await d.ev(`document.querySelectorAll(".modal .help-q").length`);
await d.ev(`document.querySelectorAll(".modal-back").forEach((m) => m.remove())`);
check("CPU and GPU editors explain each setting", cpuTips >= 14 && gpuTips >= 10 && !!card && gone, `cpu ${cpuTips}, gpu ${gpuTips}, card "${card}"`);

// 18. The overlay font is a real list (a text box with suggestions only offered the font already typed).
await d.ev(`(async () => { await go("sensors"); S.cache.sensTab = "overlay"; render(); })()`);
await d.sleep(700);
const fonts = await d.ev(`(() => { const s = document.querySelector("select.ov-font"); return s ? s.options.length : 0; })()`);
check("overlay font offers every font, not just the one chosen", fonts > 10, `${fonts} options`);

// Customize: hide a Home tile, see it in the tray, bring it back; resize by the buttons
const home = await d.ev(`(async () => {
  await go("home"); S.cache.layoutEdit = "home"; render(); await new Promise((r) => setTimeout(r, 200));
  const before = document.querySelectorAll(".tiles > .pg-panel").length;
  document.querySelector('[aria-label="Hide Clock"]').click(); await new Promise((r) => setTimeout(r, 300));
  const hidden = document.querySelectorAll(".tiles > .pg-panel").length;
  const tray = [...document.querySelectorAll(".pg-tray .chip")].map((c) => c.textContent).join(",");
  const tilesSaved = !S.state.settings.tiles.includes("clock");
  document.querySelector('[aria-label="Make Now playing wider"]').click(); await new Promise((r) => setTimeout(r, 300));
  const span = document.querySelector('[data-panel="media"]').style.getPropertyValue("--span");
  [...document.querySelectorAll(".pg-tray .chip")].find((c) => c.textContent.includes("Clock")).click(); await new Promise((r) => setTimeout(r, 300));
  const back = S.state.settings.tiles.includes("clock");
  S.cache.layoutEdit = null; render();
  return { before, hidden, tray, tilesSaved, span, back };
})()`);
check("Home panels hide, come back and resize", home.hidden === home.before - 1 && home.tray.includes("Clock") && home.tilesSaved && home.span === "6" && home.back, JSON.stringify(home));

const graphs = await d.ev(`(async () => { await go("sensors"); S.cache.sensTab = "graphs"; render(); await new Promise((r) => setTimeout(r, 500));
  return { panels: [...document.querySelectorAll(".pgrid > .pg-panel")].map((p) => p.dataset.panel).join(","), low: document.querySelector(".ft-fig.lo .num")?.textContent }; })()`);
check("Sensors graphs include power and frametimes with the lows", graphs.panels.includes("power") && graphs.panels.includes("frames") && graphs.low === "88", JSON.stringify(graphs));

const grp = await d.ev(`(async () => { await go("sensors"); S.cache.sensTab = "overlay"; S.cache.ovSel = new Set(); render(); await new Promise((r) => setTimeout(r, 300));
  for (const n of ["GPU temp", "CPU temp"]) { document.querySelector('[aria-label="Select ' + n + '"]').click(); await new Promise((r) => setTimeout(r, 150)); }
  const bar = document.querySelector(".ov-selbar b")?.textContent;
  await setSetting("osd_style", { ...ovStyleNow(), groups: { gpu_temp: "TEMP", cpu_temp: "TEMP" } }); await ovRefresh(); render(); await new Promise((r) => setTimeout(r, 300));
  const lines = (S.cache.rtss.lines || []).map((l) => l.label.trim());
  await setSetting("osd_style", { ...ovStyleNow(), groups: {} }); S.cache.ovSel = new Set();
  return { bar, lines: lines.join("|") }; })()`);
check("overlay items can be picked together and put on one line", grp.bar === "2 selected" && grp.lines.split("|").includes("TEMP"), JSON.stringify(grp));

check("no script errors", d.logs.length === 0, d.logs.slice(0, 3).join(" | "));
d.close();
process.exit(failed ? 1 : 0);
