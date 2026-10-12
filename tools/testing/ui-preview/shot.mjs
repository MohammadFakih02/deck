// Screenshots one Deck page in headless Edge with a fake backend (mock.js): no Deck restart, no hardware.
//   node shot.mjs <page> <sub|-> <out.png> [w h] [js-after]      FULL=1 for the whole scroll height
//   e.g. node shot.mjs sensors - s.png 1260 900 'S.cache.sensTab="readings";render()'
// Add fake commands to mock.js when a page needs them.
import { spawn } from "node:child_process";
const [, , page, sub, out, w = "1260", hgt = "900", extra = ""] = process.argv;
const port = +(process.env.PORT || 9333);
setTimeout(() => { console.log("TIMEOUT"); process.exit(2); }, 40000).unref();
const edge = spawn("C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe", ["--headless=new", `--remote-debugging-port=${port}`, "--allow-file-access-from-files", "--disable-gpu", "--hide-scrollbars", `--user-data-dir=${process.env.TEMP}\deck-prev-edge-${process.pid}`, `--window-size=${w},${hgt}`, "about:blank"], { stdio: "ignore" });
let list;
for (let i = 0; i < 50; i++) { try { list = await (await fetch(`http://127.0.0.1:${port}/json`)).json(); if (list.find((t) => t.type === "page")) break; } catch {} await new Promise((r) => setTimeout(r, 200)); }
const t = list.find((x) => x.type === "page");
const ws = new WebSocket(t.webSocketDebuggerUrl);
let id = 0; const pend = new Map(); const logs = [];
ws.onmessage = (m) => { const d = JSON.parse(m.data); if (d.id && pend.has(d.id)) { pend.get(d.id)(d); pend.delete(d.id); } if (d.method === "Runtime.exceptionThrown") logs.push("EXC " + JSON.stringify(d.params.exceptionDetails.exception?.description || d.params.exceptionDetails.text)); if (d.method === "Runtime.consoleAPICalled" && d.params.type === "error") logs.push("ERR " + d.params.args.map((a) => a.value || a.description).join(" ")); };
await new Promise((r) => (ws.onopen = r));
const send = (method, params = {}) => new Promise((res) => { const i = ++id; pend.set(i, res); ws.send(JSON.stringify({ id: i, method, params })); });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
await send("Runtime.enable");
await send("Emulation.setDeviceMetricsOverride", { width: +w, height: +hgt, deviceScaleFactor: 1, mobile: false });
const fs = await import("node:fs"), path = await import("node:path"), { pathToFileURL, fileURLToPath } = await import("node:url");
const here = path.dirname(fileURLToPath(import.meta.url));
const ui = pathToFileURL(path.resolve(here, "../../../ui")).href + "/";
const html = fs.readFileSync(path.resolve(here, "../../../ui/index.html"), "utf8").replace("<head>", `<head><base href="${ui}"><script src="${pathToFileURL(path.join(here, "mock.js")).href}"></script><script src="${pathToFileURL(path.join(here, "mock-data.js")).href}"></script>`);
const harness = path.join(here, `harness-${port}.html`); fs.writeFileSync(harness, html);
const url = pathToFileURL(harness).href;
await send("Page.navigate", { url });
await sleep(1500);
const ev = async (e) => { const r = await send("Runtime.evaluate", { expression: e, awaitPromise: true, returnByValue: true }); return r.result?.result?.value ?? r.result?.exceptionDetails?.exception?.description; };
console.log(await ev(`(async()=>{document.body.classList.add("shown");document.body.classList.add("style-window");await go(${JSON.stringify(page)}, ${sub === "-" ? "null" : JSON.stringify(sub)});await new Promise(r=>setTimeout(r,800));${extra};await new Promise(r=>setTimeout(r,600));return window.__out || "ok"})()`));
const full = process.env.FULL === "1";
if (full) {
  const hgt2 = await ev(`document.querySelector(".content").scrollHeight + 80`);
  await send("Emulation.setDeviceMetricsOverride", { width: +w, height: hgt2, deviceScaleFactor: 1, mobile: false });
  await ev(`document.querySelector(".panel").style.height="100vh"`); await sleep(500);
}
const shot = await send("Page.captureScreenshot", { format: "png" });
fs.writeFileSync(out, Buffer.from(shot.result.data, "base64"));
console.log(logs.join("\n"));
try { (await import("node:child_process")).spawnSync("taskkill", ["/pid", String(edge.pid), "/T", "/F"], { stdio: "ignore" }); } catch {}
process.exit(0);
