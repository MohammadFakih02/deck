// Opens Deck's UI in headless Edge against the fake backend and returns helpers for scripted tests.
//   const d = await openDeck({ w: 1260, h: 900, port: 9370 });  await d.ev("1+1");  await d.click(x, y);  d.close();
import { spawn, spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { pathToFileURL, fileURLToPath } from "node:url";

export async function openDeck({ w = 1260, h = 900, port = 9370, page = null } = {}) {
  const edge = spawn("C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe", ["--headless=new", `--remote-debugging-port=${port}`, "--allow-file-access-from-files", "--disable-gpu", "--hide-scrollbars", `--user-data-dir=${process.env.TEMP}\\deck-test-edge-${process.pid}-${port}`, `--window-size=${w},${h}`, "about:blank"], { stdio: "ignore" });
  let list;
  for (let i = 0; i < 50; i++) { try { list = await (await fetch(`http://127.0.0.1:${port}/json`)).json(); if (list.find((t) => t.type === "page" && t.url === "about:blank")) break; } catch {} await new Promise((r) => setTimeout(r, 200)); }
  const t = list.find((x) => x.type === "page" && x.url === "about:blank");
  const ws = new WebSocket(t.webSocketDebuggerUrl);
  let id = 0; const pend = new Map(); const logs = [];
  ws.onmessage = (m) => { const d = JSON.parse(m.data); if (d.id && pend.has(d.id)) { pend.get(d.id)(d); pend.delete(d.id); } if (d.method === "Runtime.exceptionThrown") logs.push("EXC " + (d.params.exceptionDetails.exception?.description || d.params.exceptionDetails.text)); if (d.method === "Runtime.consoleAPICalled" && d.params.type === "error") logs.push("ERR " + d.params.args.map((a) => a.value || a.description).join(" ")); };
  await new Promise((r) => (ws.onopen = r));
  const send = (method, params = {}) => new Promise((res) => { const i = ++id; pend.set(i, res); ws.send(JSON.stringify({ id: i, method, params })); });
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  await send("Runtime.enable");
  await send("Emulation.setDeviceMetricsOverride", { width: w, height: h, deviceScaleFactor: 1, mobile: false });
  const here = path.dirname(fileURLToPath(import.meta.url));
  const uiDir = process.env.DECK_UI || path.resolve(here, "../../../ui");
  const ui = pathToFileURL(uiDir).href + "/";
  const html = fs.readFileSync(path.join(uiDir, "index.html"), "utf8").replace("<head>", `<head><base href="${ui}"><script src="${pathToFileURL(path.join(here, "mock.js")).href}"></script><script src="${pathToFileURL(path.join(here, "mock-data.js")).href}"></script>`);
  const harness = path.join(here, `harness-${port}.html`);
  fs.writeFileSync(harness, html);
  await send("Page.navigate", { url: pathToFileURL(harness).href });
  await sleep(1500);
  const ev = async (e) => { const r = await send("Runtime.evaluate", { expression: e, awaitPromise: true, returnByValue: true }); if (r.result?.exceptionDetails) return { error: r.result.exceptionDetails.exception?.description || r.result.exceptionDetails.text }; return r.result?.result?.value; };
  await ev(`document.body.classList.add("shown","style-window")`);
  const mouse = (type, x, y) => send("Input.dispatchMouseEvent", { type, x, y, button: "left", clickCount: 1 });
  const key = async (k, code = k, vk = 0) => { await send("Input.dispatchKeyEvent", { type: "rawKeyDown", key: k, code, windowsVirtualKeyCode: vk }); await send("Input.dispatchKeyEvent", { type: "keyUp", key: k, code, windowsVirtualKeyCode: vk }); };
  const shot = async (file) => { const r = await send("Page.captureScreenshot", { format: "png" }); fs.writeFileSync(file, Buffer.from(r.result.data, "base64")); };
  const close = () => { try { ws.close(); spawnSync("taskkill", ["/pid", String(edge.pid), "/T", "/F"], { stdio: "ignore" }); } catch {} };
  return { ev, send, mouse, key, shot, sleep, logs, close };
}
