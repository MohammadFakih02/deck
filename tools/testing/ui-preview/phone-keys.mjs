// Tests the phone's on-screen keyboard (controller page → Keyboard) in headless Edge/Chrome with a fake
// WebSocket standing in for the PC: pressing, combos, toggles, the mouse pad, editing (move, resize,
// remove, undo), layouts tied to games, and what is sent to the PC. Screenshots go to <outdir>.
//   node phone-keys.mjs <outdir>      (PORT env picks the debug port)
import { spawn, spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { pathToFileURL, fileURLToPath } from "node:url";
import { createHash } from "node:crypto";
import { BROWSER } from "./harness.mjs";
const here = path.dirname(fileURLToPath(import.meta.url));
const [, , outDir = "phone-keys-shots"] = process.argv;
const src = path.resolve(here, "../../../src-tauri/phone/index.html");
const port = +(process.env.PORT || 9381);
fs.mkdirSync(outDir, { recursive: true });
setTimeout(() => { console.log("TIMEOUT"); process.exit(2); }, 120000).unref();

const FAKE = `<script>
window.__sent = [];
const realFetch = window.fetch;
window.fetch = (u, o) => String(u) === "/session" ? Promise.resolve(new Response("FAKESESSION")) : realFetch(u, o);
class FakeWS { constructor(u) { this.url = String(u); this.readyState = 1; FakeWS.all.push(this); FakeWS.last = this; setTimeout(() => { this.onopen && this.onopen(); }, 20); }
  send(m) { window.__sent.push({ url: this.url.split("?")[0].split("/").pop(), m: JSON.parse(m) }); } close() {}
  push(o) { this.onmessage && this.onmessage({ data: JSON.stringify(o) }); } }
FakeWS.all = [];
window.WebSocket = FakeWS;
window.confirm = () => true;
window.prompt = (q, d) => window.__prompt || d || "";
</script>`;
const WATCH = `<script>document.addEventListener("securitypolicyviolation", (e) => console.error("CSP blocked " + e.violatedDirective + " " + e.blockedURI));</script>`;
const page0 = fs.readFileSync(src, "utf8").replace("<head>", "<head>" + WATCH + FAKE);
const hashes = [...page0.matchAll(/<script(?![^>]*src=)[^>]*>([\s\S]*?)<\/script>/g)].map((m) => `'sha256-${createHash("sha256").update(m[1].replace(/\r\n?/g, "\n")).digest("base64")}'`).join(" ");
const CSP = `default-src 'none'; script-src 'self' ${hashes}; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; media-src 'self' blob:; connect-src 'self' ws: wss:; worker-src 'self' blob:; manifest-src 'self'; base-uri 'none'; form-action 'none'`;
const file = path.join(here, `phone-harness-${port}.html`);
fs.writeFileSync(file, page0.replace("<head>", `<head><meta http-equiv="Content-Security-Policy" content="${CSP}">`));

const edge = spawn(BROWSER, ["--headless=new", `--remote-debugging-port=${port}`, "--allow-file-access-from-files", "--disable-gpu", "--hide-scrollbars", `--user-data-dir=${process.env.TEMP}\\deck-phonekeys-${process.pid}`, "about:blank"], { stdio: "ignore" });
const cleanup = () => { try { spawnSync("taskkill", ["/pid", String(edge.pid), "/T", "/F"], { stdio: "ignore" }); } catch {} try { fs.unlinkSync(file); } catch {} };
process.on("exit", cleanup);
let list;
for (let i = 0; i < 150; i++) { try { list = await (await fetch(`http://127.0.0.1:${port}/json`)).json(); if (list.find((t) => t.type === "page" && t.url === "about:blank")) break; } catch {} await new Promise((r) => setTimeout(r, 200)); }
const ws = new WebSocket(list.find((x) => x.type === "page" && x.url === "about:blank").webSocketDebuggerUrl);
let id = 0; const pend = new Map(); const logs = [];
ws.onmessage = (m) => {
  const d = JSON.parse(m.data);
  if (d.id && pend.has(d.id)) { pend.get(d.id)(d); pend.delete(d.id); }
  if (d.method === "Runtime.exceptionThrown") logs.push("EXC " + (d.params.exceptionDetails.exception?.description || d.params.exceptionDetails.text));
  if (d.method === "Runtime.consoleAPICalled" && d.params.type === "error") logs.push("console.error " + d.params.args.map((a) => a.value ?? a.description).join(" "));
};
await new Promise((r) => (ws.onopen = r));
const send = (method, params = {}) => new Promise((res) => { const i = ++id; pend.set(i, res); ws.send(JSON.stringify({ id: i, method, params })); });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const ev = async (e) => { const r = await send("Runtime.evaluate", { expression: e, awaitPromise: true, returnByValue: true }); if (r.result?.exceptionDetails) throw new Error(r.result.exceptionDetails.exception?.description || "evaluate failed: " + e.slice(0, 80)); return r.result?.result?.value; };
await send("Runtime.enable");
let fails = 0;
const check = (ok, what, extra = "") => { console.log(`${ok ? "PASS" : "FAIL"}  ${what}${extra ? "  " + extra : ""}`); if (!ok) fails++; };
const shot = async (name) => { const r = await send("Page.captureScreenshot", { format: "png" }); fs.writeFileSync(path.join(outDir, name + ".png"), Buffer.from(r.result.data, "base64")); };

// helpers inside the page
const PAGE_HELPERS = `
window.pe = (type, el, dx = 0, dy = 0, pid = 1) => { const r = el.getBoundingClientRect(); el.dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, pointerId: pid, clientX: r.left + r.width / 2 + dx, clientY: r.top + r.height / 2 + dy, isPrimary: true })); };
window.btn = (label) => [...document.querySelectorAll("#kbd .kbtn")].find((e) => e.textContent === label);
window.lastKh = () => { const k = window.__sent.filter((s) => s.m.t === "kh"); return k.length ? k[k.length - 1].m.k : null; };
window.sentOf = (t) => window.__sent.filter((s) => s.m.t === t).map((s) => s.m);
`;

await send("Emulation.setDeviceMetricsOverride", { width: 844, height: 390, deviceScaleFactor: 2, mobile: true });
await send("Emulation.setTouchEmulationEnabled", { enabled: true, maxTouchPoints: 5 });
await send("Page.navigate", { url: pathToFileURL(file).href + "#k=phonekey-SECRET" });
await sleep(700);
await ev(PAGE_HELPERS);
await ev(`padOnly(true)`);
await sleep(150);
const pad = await ev(`WebSocket.all.filter((w) => w.url.includes("/pad")).length`);
check(pad === 1, "the controller page opens its own connection to /pad");
check(await ev(`window.__sent.some((s) => s.url === "pad" && s.m.t === "games")`), "it tells the PC whether keys may be used in games");

// the PC has no layouts yet: the phone makes the two ready-made ones and saves them
await ev(`WebSocket.all.find((w) => w.url.includes("/pad")).push({ t: "kb", data: { v: 1, rev: 0, auto: true, active: "", layouts: [] } })`);
await sleep(1000);
const saved = await ev(`window.sentOf("kbsave")`);
check(saved.length === 1 && saved[0].data.layouts.length === 2 && saved[0].data.layouts[0].items.length > 60, "first time: Keyboard + Gaming are made and saved on the PC", saved.length ? `(${saved[0].data.layouts.map((l) => l.name + " " + l.items.length).join(", ")})` : "");
// the PC answers with what it stored
await ev(`(() => { const d = JSON.parse(JSON.stringify(window.sentOf("kbsave")[0].data)); d.rev = 1; WebSocket.all.find((w) => w.url.includes("/pad")).push({ t: "kb", data: d }); })()`);

await ev(`document.getElementById("padKb").click()`);
await sleep(200);
check(await ev(`document.getElementById("padonly").classList.contains("kbm") && getComputedStyle(document.getElementById("gpad")).display === "none"`), "keyboard mode shows the keyboard and hides the controller");
await shot("1-full-keyboard");

// pressing: hold, combo, toggle, tap
await ev(`window.__sent.length = 0`);
await ev(`pe("pointerdown", btn("Q"))`);
check(JSON.stringify(await ev(`lastKh()`)) === `["q"]`, "touching a key holds it on the PC");
await ev(`pe("pointerup", btn("Q"))`);
check(JSON.stringify(await ev(`lastKh()`)) === `[]`, "letting go releases it");
await ev(`pe("pointerdown", btn("Shift"), 0, 0, 1); pe("pointerdown", btn("W"), 0, 0, 2)`);
check(JSON.stringify(await ev(`lastKh()`)) === `["shift","w"]`, "two fingers: Shift and W held together, in the order pressed");
await ev(`pe("pointerup", btn("Shift"), 0, 0, 1)`);
check(JSON.stringify(await ev(`lastKh()`)) === `["w"]`, "letting go of one leaves the other held");
await ev(`pe("pointerup", btn("W"), 0, 0, 2)`);
await ev(`window.__sent.length = 0`);
await sleep(1200);
check((await ev(`window.sentOf("kh").length`)) === 0, "nothing is sent while nothing is held");
await ev(`pe("pointerdown", btn("A"))`);
await ev(`window.__sent.length = 0`);
await sleep(1150);
check((await ev(`window.sentOf("kh").length`)) >= 2, "a held key is repeated twice a second (a lost message can't stick it)");
await ev(`pe("pointercancel", btn("A"))`);
check(JSON.stringify(await ev(`lastKh()`)) === `[]`, "a cancelled touch releases the key");

// switch to Gaming, check mouse buttons, scroll, mouse pad
await ev(`(() => { const s = document.getElementById("kbSel"); s.value = kbCur().id === s.options[0].value ? s.options[1].value : s.options[0].value; s.dispatchEvent(new Event("change")); })()`);
await sleep(100);
check((await ev(`kbCur().name`)) === "Gaming", "the layout is picked in the settings");
await shot("2-gaming");
await ev(`window.__sent.length = 0`);
await ev(`pe("pointerdown", btn("LMB"))`);
check(JSON.stringify(await ev(`lastKh()`)) === `["mouse1"]`, "a mouse button is held as mouse1");
await ev(`pe("pointerup", btn("LMB"))`);
await ev(`window.__sent.length = 0`);
await ev(`pe("pointerdown", btn("▲"))`);
check((await ev(`window.sentOf("wh")`)).some((m) => m.dy === 120), "the scroll button scrolls up one notch");
await ev(`pe("pointerup", btn("▲"))`);
await ev(`window.__sent.length = 0`);
await ev(`(() => { const p = btn("Mouse"); pe("pointerdown", p, 0, 0, 3); pe("pointermove", p, 30, 10, 3); pe("pointermove", p, 60, 20, 3); pe("pointerup", p, 60, 20, 3); })()`);
await sleep(100);
const rm = await ev(`window.sentOf("rm")`);
check(rm.length > 0 && rm.reduce((a, m) => a + m.dx, 0) > 80 && rm.reduce((a, m) => a + m.dy, 0) > 25, "dragging the mouse pad moves the mouse", JSON.stringify(rm.map((m) => [m.dx, m.dy])));
await ev(`window.__sent.length = 0`);
await ev(`(() => { const p = btn("Mouse"); pe("pointerdown", p, 0, 0, 4); pe("pointerup", p, 0, 0, 4); })()`);
await sleep(120);
check(JSON.stringify(await ev(`window.__sent.filter((s) => s.m.t === "kh").map((s) => s.m.k)`)) === `[["mouse1"],[]]`, "a quick touch on the mouse pad is a click");

// the mouse pad's speed is the layout's own (settings), and quick swipes can be boosted
const drag = async () => { await ev(`window.__sent.length = 0`); await ev(`(() => { const p = btn("Mouse"); pe("pointerdown", p, 0, 0, 5); pe("pointermove", p, 40, 0, 5); pe("pointerup", p, 40, 0, 5); })()`); await sleep(100); return (await ev(`window.sentOf("rm")`)).reduce((a, m) => a + m.dx, 0); };
await ev(`kbCur().acc = false; kbCur().pad = 1`);
const slow = await drag();
await ev(`kbCur().pad = 4`);
const fast = await drag();
check(slow > 0 && Math.abs(fast / slow - 4) < 0.3, "the layout's mouse pad speed multiplies the movement", `(${slow} → ${fast})`);
await ev(`kbCur().acc = true; kbCur().pad = 1`);
check((await drag()) > slow * 1.5, "quick swipes go further than slow drags of the same distance");
await ev(`kbCur().acc = false; kbCur().pad = 1`);

// editing
await ev(`document.getElementById("padEdit").click()`);
await sleep(100);
check(await ev(`document.getElementById("kbIns").classList.contains("on")`), "edit mode opens the editor panel");
await ev(`window.__sent.length = 0`);
await ev(`pe("pointerdown", btn("W"))`);
check((await ev(`window.sentOf("kh").length`)) === 0, "in edit mode a touch selects, it does not press");
const before = await ev(`(() => { const it = kbItemOf(KB.sel); return [it.x, it.y, it.w]; })()`);
await ev(`(() => { const e = btn("W"); pe("pointermove", e, 80, 40); pe("pointerup", e, 80, 40); })()`);
const after = await ev(`(() => { const it = kbItemOf(KB.sel); return [it.x, it.y, it.w]; })()`);
check(after[0] > before[0] && after[1] > before[1], "dragging moves the button", `${before.map((n) => n.toFixed(3))} → ${after.map((n) => n.toFixed(3))}`);
await shot("3-edit");
// resize by the corner
const w0 = after[2];
await ev(`(() => { const h = document.querySelector("#kbd .kbtn.sel .kh"); pe("pointerdown", h); pe("pointermove", h, 40, 30); pe("pointerup", h, 40, 30); })()`);
check((await ev(`kbItemOf(KB.sel).w`)) > w0, "dragging the corner makes it bigger");
// change the key and the way it acts
await ev(`(() => { const i = document.getElementById("kiKeys"); i.value = "ctrl+shift+esc"; i.dispatchEvent(new Event("input")); const m = document.getElementById("kiMode"); m.value = "tap"; m.dispatchEvent(new Event("change")); })()`);
check(JSON.stringify(await ev(`kbItemOf(KB.sel).keys`)) === `["ctrl","shift","esc"]` && (await ev(`kbItemOf(KB.sel).mode`)) === "tap", "the editor sets a combo and 'one tap'");
await ev(`(() => { const i = document.getElementById("kiKeys"); i.value = "ctrl+hyper"; i.dispatchEvent(new Event("input")); })()`);
check((await ev(`document.getElementById("kiBad").textContent`)).includes("hyper"), "a name that is not a key is called out");
await ev(`(() => { const i = document.getElementById("kiKeys"); i.value = "ctrl+num+"; i.dispatchEvent(new Event("input")); })()`);
check(JSON.stringify(await ev(`kbItemOf(KB.sel).keys`)) === `["ctrl","num+"]`, "'+' inside a name (num+) is not taken for a separator");
// removing keys
const n0 = await ev(`kbCur().items.length`);
await ev(`document.getElementById("kiRemove").click()`);
check((await ev(`kbCur().items.length`)) === n0 - 1, "a button is removed from the editor");
await ev(`document.getElementById("kbUndo").click()`);
check((await ev(`kbCur().items.length`)) === n0, "…and Undo brings it back");
await ev(`document.getElementById("kbDel").click()`);
await ev(`pe("pointerdown", btn("Z")); pe("pointerdown", btn("X")); pe("pointerdown", btn("C"))`);
check((await ev(`kbCur().items.length`)) === n0 - 3, "Remove mode: tapping three buttons removes them", `(${n0} → ${await ev(`kbCur().items.length`)})`);
await ev(`document.getElementById("kbDel").click()`);
await shot("4-after-removing");
// adding
await ev(`document.getElementById("kbAddKey").click()`);
check((await ev(`kbCur().items.length`)) === n0 - 2 && (await ev(`!!KB.sel`)), "+ Key adds a button and selects it");
await ev(`(() => { const p = document.getElementById("kbPreset"); p.value = "numpad"; p.dispatchEvent(new Event("change")); })()`);
check((await ev(`kbCur().items.length`)) === n0 - 2 + 16, "'Add a set' adds the numpad");
// saved on the PC after the changes
await sleep(1000);
const last = await ev(`window.sentOf("kbsave").pop()`);
check(last && last.data.layouts.find((l) => l.name === "Gaming").items.length === n0 - 2 + 16, "the changes are saved on the PC", last ? "" : "(nothing sent)");
await ev(`document.getElementById("kbEditDone").click()`);

// layouts for games
await ev(`document.getElementById("padSet").click()`);
await ev(`WebSocket.all.find((w) => w.url.includes("/pad")).push({ t: "game", name: "Dark Souls II" })`);
await sleep(100);
check((await ev(`document.getElementById("kbGameNow").hidden`)) === false, "the settings offer the game that is in front");
await ev(`window.prompt_ = 1; window.__prompt = "Dark Souls II"; document.getElementById("kbNewBlank").click()`);
await ev(`document.getElementById("kbGameNow").click()`);
await sleep(50);
check((await ev(`kbCur().name`)) === "Dark Souls II" && JSON.stringify(await ev(`kbCur().games`)) === `["dark souls ii"]`, "a new layout is tied to the game in front");
await ev(`document.getElementById("kbSel").value = KB.doc.layouts.find((l) => l.name === "Keyboard").id; document.getElementById("kbSel").dispatchEvent(new Event("change"))`);
check((await ev(`kbCur().name`)) === "Keyboard", "picking another layout by hand wins for now");
await ev(`WebSocket.all.find((w) => w.url.includes("/pad")).push({ t: "game", name: null })`);
await ev(`WebSocket.all.find((w) => w.url.includes("/pad")).push({ t: "game", name: "dark souls ii" })`);
check((await ev(`kbCur().name`)) === "Dark Souls II", "when the game comes to the front its layout comes up by itself");
await ev(`WebSocket.all.find((w) => w.url.includes("/pad")).push({ t: "game", name: null })`);
check((await ev(`kbCur().name`)) !== "Dark Souls II", "…and goes away with the game");
await ev(`document.getElementById("padSetDone").click()`);

// size and opacity are the layout's own
await ev(`document.getElementById("padSet").click()`);
await ev(`(() => { const s = document.getElementById("poS"); s.value = "1.3"; s.dispatchEvent(new Event("change")); const o = document.getElementById("poO"); o.value = "0.4"; o.dispatchEvent(new Event("input")); })()`);
check((await ev(`kbCur().size`)) === 1.3 && (await ev(`kbCur().op`)) === 0.4 && (await ev(`getComputedStyle(document.getElementById("kbd")).getPropertyValue("--kbo").trim()`)) === "0.4", "size and opacity are set per layout");
await shot("5-settings");
await ev(`document.getElementById("padSetDone").click()`);

// leaving releases everything
await ev(`document.getElementById("padKb").click()`);
await ev(`document.getElementById("padKb").click()`);
await ev(`pe("pointerdown", btn("Esc") || btn("A"))`);
await ev(`window.__sent.length = 0`);
await ev(`document.getElementById("padKb").click()`);
check(JSON.stringify(await ev(`lastKh()`)) === `[]` || (await ev(`lastKh()`)) === null, "switching back to the controller lets go of every key");

// the keyboard over the PC's screen (Screen view): buttons take touches, the rest goes through to the screen
await ev(`padOnly(false)`);
await sleep(150);
await ev(`kbOver(true)`);
await sleep(150);
check(await ev(`(() => { const v = document.getElementById("padonly"); return v.classList.contains("ov") && v.classList.contains("kbm") && getComputedStyle(v).pointerEvents === "none" && getComputedStyle(v).backgroundColor === "rgba(0, 0, 0, 0)"; })()`), "over the screen: transparent, and touches between buttons pass through");
check(await ev(`getComputedStyle(document.querySelector("#kbd .kbtn")).pointerEvents === "auto" && getComputedStyle(document.getElementById("kbd")).pointerEvents === "none"`), "…while the buttons still take touches");
await ev(`window.__sent.length = 0; const b = [...document.querySelectorAll("#kbd .kbtn")].find((e) => e.textContent === "Space" || e.textContent === "Q"); pe("pointerdown", b)`);
check((await ev(`lastKh()`)) !== null && (await ev(`lastKh()`)).length === 1, "a button pressed over the screen reaches the PC");
await ev(`document.querySelectorAll("#kbd .kbtn.on").forEach((e) => pe("pointerup", e))`);
await ev(`document.getElementById("padClose").click()`);
check(await ev(`!document.getElementById("padonly").classList.contains("on") && !PAD.on && !PAD.over`), "its close button closes only the keyboard");

// the page: no unnamed buttons, no script errors, no CSP violations
const unnamed = await ev(`[...document.querySelectorAll("#padonly button")].filter((b) => b.offsetParent && !(b.getAttribute("aria-label") || b.textContent.trim() || b.title)).length`);
check(unnamed === 0, "every button on the page has a name");
check(!logs.length, "no script errors or blocked scripts", logs.slice(0, 3).join(" | "));
console.log(fails ? `${fails} FAILED` : "all passed");
process.exit(fails ? 1 : 0);
