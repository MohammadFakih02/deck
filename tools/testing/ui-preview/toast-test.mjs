// Checks the notification window (ui/toast.html) in headless Edge with a stub backend:
//   node toast-test.mjs        PASS/FAIL per check, exit code 1 on any failure
import { spawn, spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { pathToFileURL, fileURLToPath } from "node:url";
import { BROWSER } from "./harness.mjs";

const port = +(process.env.PORT || 9391);
const here = path.dirname(fileURLToPath(import.meta.url));
const uiDir = process.env.DECK_UI || path.resolve(here, "../../../ui");
const edge = spawn(BROWSER, ["--headless=new", `--remote-debugging-port=${port}`, "--allow-file-access-from-files", "--disable-gpu", `--user-data-dir=${process.env.TEMP}\\deck-toast-edge-${process.pid}`, "--window-size=400,600", "about:blank"], { stdio: "ignore" });
// Edge goes away even if a check below throws (a crash used to leave it running, holding the port)
process.on("exit", () => spawnSync("taskkill", ["/pid", String(edge.pid), "/T", "/F"], { stdio: "ignore" }));
// up to 30 s: a cold CI runner can take longer than 10 s to start Edge
let list;
for (let i = 0; i < 150; i++) { try { list = await (await fetch(`http://127.0.0.1:${port}/json`)).json(); if (list.find((t) => t.type === "page" && t.url === "about:blank")) break; } catch {} await new Promise((r) => setTimeout(r, 200)); }
const page = list?.find((x) => x.type === "page" && x.url === "about:blank");
if (!page) { console.error("FAIL  headless Edge didn't open a page within 30 s"); process.exit(1); }
const ws = new WebSocket(page.webSocketDebuggerUrl);
let id = 0; const pend = new Map(); const logs = [];
ws.onmessage = (m) => { const d = JSON.parse(m.data); if (d.id && pend.has(d.id)) { pend.get(d.id)(d); pend.delete(d.id); } if (d.method === "Runtime.exceptionThrown") logs.push(d.params.exceptionDetails.exception?.description || d.params.exceptionDetails.text); };
await new Promise((r) => (ws.onopen = r));
const send = (method, params = {}) => new Promise((res) => { const i = ++id; pend.set(i, res); ws.send(JSON.stringify({ id: i, method, params })); });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const ev = async (e) => (await send("Runtime.evaluate", { expression: e, awaitPromise: true, returnByValue: true })).result?.result?.value;
await send("Runtime.enable");
// a stub backend: events are delivered through window.__emit(name, payload)
const stub = `<script>window.__calls=[];window.__ls={};window.__emit=(n,p)=>(window.__ls[n]||[]).forEach(f=>f({payload:p}));
window.__TAURI__={core:{invoke:async(c,a)=>{window.__calls.push([c,a]);return c==="get_state"?{settings:{accent:"#4f8cff"}}:null;}},event:{listen:async(n,f)=>{(window.__ls[n]=window.__ls[n]||[]).push(f);return()=>{};}}};</script>`;
const html = fs.readFileSync(path.join(uiDir, "toast.html"), "utf8").replace("<head>", `<head><base href="${pathToFileURL(uiDir).href}/">${stub}`);
const file = path.join(here, `toast-harness-${port}.html`);
fs.writeFileSync(file, html);
await send("Page.navigate", { url: pathToFileURL(file).href });
await sleep(800);

let failed = 0;
const check = (name, ok, info = "") => { console.log(`${ok ? "PASS" : "FAIL"}  ${name}${info ? "  (" + info + ")" : ""}`); if (!ok) failed++; };
const toast = (id, extra = {}) => ev(`window.__emit("toast", ${JSON.stringify({ id, kind: "info", title: id, body: "", actions: [["ok", "OK"]], sticky: false, sound: false, ...extra })})`);
const center = (sel) => ev(`(() => { const r = document.querySelector(${JSON.stringify(sel)}).getBoundingClientRect(); return [r.left + r.width / 2, r.top + r.height / 2]; })()`);
const mouse = (type, x, y) => send("Input.dispatchMouseEvent", { type, x, y, button: "left", clickCount: 1 });

// 1. A toast under the mouse doesn't time out while you're reading or reaching for it.
await toast("a");
await sleep(300);
const [ax, ay] = await center('[data-id="a"] .b');
await mouse("mouseMoved", ax, ay);
await sleep(7300);
check("a toast under the mouse stays past its 6.5 s", await ev(`!!document.querySelector('[data-id="a"]')`));

// 2. While the mouse is over the stack, another toast timing out doesn't shift the one being clicked.
await ev(`document.dispatchEvent(new MouseEvent("mouseleave"))`); // the mouse goes away
await sleep(300);
await ev(`document.querySelectorAll(".t").forEach((t) => t.remove())`);
await toast("a2", { sticky: true });
await toast("b", { sticky: true }); // appended below a2
await sleep(300);
const [bx, by] = await center('[data-id="b"] .b');
await mouse("mouseMoved", bx, by);
const before = await center('[data-id="b"] .b');
await ev(`window.__emit("toast-dismiss", { id: "a2" })`); // the backend dismisses the one above
await sleep(400);
const after = await center('[data-id="b"] .b');
check("buttons don't move under the mouse when another toast goes", before[1] === after[1], `${before[1]} -> ${after[1]}`);
await mouse("mousePressed", bx, by); await mouse("mouseReleased", bx, by);
await sleep(300);
check("the click lands on the toast that was under the mouse", await ev(`JSON.stringify(window.__calls.filter(c => c[0] === "toast_action"))`) === JSON.stringify([["toast_action", { id: "b", action: "ok" }]]), await ev(`JSON.stringify(window.__calls.filter(c => c[0] === "toast_action"))`));

// 3. Once the mouse leaves, held removals happen and timers run again.
await mouse("mouseMoved", 390, 590);
await ev(`document.dispatchEvent(new MouseEvent("mouseleave"))`);
await sleep(1500);
check("held removals happen once the mouse leaves", await ev(`!document.querySelector('[data-id="a2"]')`));
await toast("c");
await sleep(7300);
check("toasts still time out when the mouse is elsewhere", await ev(`!document.querySelector('[data-id="c"]')`));

check("no script errors", logs.length === 0, logs.join(" | "));
ws.close();
spawnSync("taskkill", ["/pid", String(edge.pid), "/T", "/F"], { stdio: "ignore" });
fs.rmSync(file, { force: true });
process.exit(failed ? 1 : 0);
