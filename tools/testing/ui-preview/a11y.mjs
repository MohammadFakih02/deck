// Visits every Deck page against the fake backend (mock.js) and counts accessibility and layout
// problems: unnamed buttons/links, unlabelled form fields, small click targets, horizontal overflow,
// script errors. Also times page switches.
//   PORT=9360 node a11y.mjs [w h] [out.json]
import { spawn } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { pathToFileURL, fileURLToPath } from "node:url";
const [, , w = "1260", hgt = "900", outFile = ""] = process.argv;
const port = +(process.env.PORT || 9360);
setTimeout(() => { console.log("TIMEOUT"); process.exit(2); }, 240000).unref();
const edge = spawn("C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe", ["--headless=new", `--remote-debugging-port=${port}`, "--allow-file-access-from-files", "--disable-gpu", "--hide-scrollbars", `--user-data-dir=${process.env.TEMP}\\deck-a11y-edge-${process.pid}`, `--window-size=${w},${hgt}`, "about:blank"], { stdio: "ignore" });
let list;
for (let i = 0; i < 50; i++) { try { list = await (await fetch(`http://127.0.0.1:${port}/json`)).json(); if (list.find((t) => t.type === "page")) break; } catch {} await new Promise((r) => setTimeout(r, 200)); }
const t = list.find((x) => x.type === "page");
const ws = new WebSocket(t.webSocketDebuggerUrl);
let id = 0; const pend = new Map(); const logs = [];
ws.onmessage = (m) => { const d = JSON.parse(m.data); if (d.id && pend.has(d.id)) { pend.get(d.id)(d); pend.delete(d.id); } if (d.method === "Runtime.exceptionThrown") logs.push("EXC " + (d.params.exceptionDetails.exception?.description || d.params.exceptionDetails.text)); if (d.method === "Runtime.consoleAPICalled" && d.params.type === "error") logs.push("ERR " + d.params.args.map((a) => a.value || a.description).join(" ")); };
await new Promise((r) => (ws.onopen = r));
const send = (method, params = {}) => new Promise((res) => { const i = ++id; pend.set(i, res); ws.send(JSON.stringify({ id: i, method, params })); });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
await send("Runtime.enable");
await send("Emulation.setDeviceMetricsOverride", { width: +w, height: +hgt, deviceScaleFactor: 1, mobile: false });
const here = path.dirname(fileURLToPath(import.meta.url));
const ui = pathToFileURL(path.resolve(here, "../../../ui")).href + "/";
const html = fs.readFileSync(path.resolve(here, "../../../ui/index.html"), "utf8").replace("<head>", `<head><base href="${ui}"><script src="${pathToFileURL(path.join(here, "mock.js")).href}"></script><script src="${pathToFileURL(path.join(here, "mock-data.js")).href}"></script>`);
const harness = path.join(here, `harness-${port}.html`);
fs.writeFileSync(harness, html);
await send("Page.navigate", { url: pathToFileURL(harness).href });
await sleep(1500);
const ev = async (e) => { const r = await send("Runtime.evaluate", { expression: e, awaitPromise: true, returnByValue: true }); return r.result?.result?.value ?? r.result?.exceptionDetails?.exception?.description; };
await ev(`document.body.classList.add("shown","style-window")`);

// The checks, run inside the page.
const CHECK = `(() => {
  const vis = (el) => { const r = el.getBoundingClientRect(); const cs = getComputedStyle(el); return r.width > 0 && r.height > 0 && cs.visibility !== "hidden" && cs.display !== "none"; };
  const name = (el) => (el.getAttribute("aria-label") || el.getAttribute("aria-labelledby") && document.getElementById(el.getAttribute("aria-labelledby"))?.textContent || el.textContent || el.getAttribute("title") || el.querySelector("img[alt]")?.alt || "").trim();
  const desc = (el) => (el.className && typeof el.className === "string" ? "." + el.className.split(" ").filter(Boolean).join(".") : el.tagName.toLowerCase()) + (el.closest("[class]") ? " in " + (el.parentElement?.className || "") : "");
  const out = { unnamedButtons: [], unlabelledFields: [], smallTargets: [], overflowX: 0, focusableNoRing: 0 };
  const root = document.querySelector(".panel");
  for (const b of root.querySelectorAll("button, a[href], [role=button]")) {
    if (!vis(b)) continue;
    if (!name(b)) out.unnamedButtons.push(desc(b));
    const r = b.getBoundingClientRect();
    if (r.width < 24 || r.height < 24) out.smallTargets.push(desc(b) + " " + Math.round(r.width) + "x" + Math.round(r.height));
  }
  for (const f of root.querySelectorAll("input, select, textarea")) {
    if (f.type === "hidden" || f.type === "file") continue;
    const sw = f.closest(".switch");
    if (!vis(sw || f)) continue;
    const lab = f.getAttribute("aria-label") || f.getAttribute("aria-labelledby") || f.labels?.length && [...f.labels].some((l) => l.textContent.trim()) || f.getAttribute("title") || (f.placeholder && f.type !== "range" ? f.placeholder : "");
    if (!lab) out.unlabelledFields.push((f.type || f.tagName.toLowerCase()) + " " + desc(sw || f));
  }
  const c = document.querySelector(".content");
  out.overflowX = Math.max(0, c.scrollWidth - c.clientWidth);
  const wide = [...c.querySelectorAll("*")].filter((el) => { const r = el.getBoundingClientRect(); return r.right > c.getBoundingClientRect().right + 2 && vis(el) && getComputedStyle(el).position !== "fixed"; }).slice(0, 5).map(desc);
  out.wide = wide;
  return out;
})()`;

const pages = fs.readFileSync(path.join(here, "all.sh"), "utf8").split("<<'LIST'")[1].split("LIST")[0].trim().split("\n").map((l) => l.trim().split(/\s+/)).filter((x) => x[0]);
const pagesPlus = [...pages, ["sensors", "readings", 'S.cache.sensTab="readings";render()'], ["sensors", "overlay", 'S.cache.sensTab="overlay";render()'], ["sensors", "alerts", 'S.cache.sensTab="alerts";render()'], ["settings", "home", 'S.cache.setTab="home";render()']];
const report = {};
for (const [page, sub, extra] of pagesPlus) {
  logs.length = 0;
  const t0 = await ev(`performance.now()`);
  await ev(`(async()=>{ await go(${JSON.stringify(page)}, ${!sub || sub === "-" || extra ? "null" : JSON.stringify(sub)}); ${extra || ""} })()`);
  const t1 = await ev(`performance.now()`);
  await sleep(700);
  const r = await ev(CHECK);
  const key = page + (sub && sub !== "-" ? "/" + sub : "");
  report[key] = { ms: Math.round(t1 - t0), ...r, errors: [...logs] };
  const n = (a) => (Array.isArray(a) ? a.length : a);
  console.log(`${key.padEnd(20)} switch ${String(Math.round(t1 - t0)).padStart(4)} ms  unnamed ${n(r.unnamedButtons)}  unlabelled ${n(r.unlabelledFields)}  small ${n(r.smallTargets)}  overflowX ${r.overflowX}  errors ${logs.length}`);
}
if (outFile) fs.writeFileSync(outFile, JSON.stringify(report, null, 1));
try { (await import("node:child_process")).spawnSync("taskkill", ["/pid", String(edge.pid), "/T", "/F"], { stdio: "ignore" }); } catch {}
process.exit(0);
