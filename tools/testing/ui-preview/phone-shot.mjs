// Screenshots the phone remote (src-tauri/phone/index.html) at phone and tablet sizes with a fake
// WebSocket feeding it a realistic state, and checks that a tap survives a state push.
//   node phone-shot.mjs <outdir> [phoneHtml]      (PORT env picks the debug port)
import { spawn, spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { pathToFileURL, fileURLToPath } from "node:url";
import { BROWSER } from "./harness.mjs";
const here = path.dirname(fileURLToPath(import.meta.url));
const [, , outDir = "phone-shots", src = path.resolve(here, "../../../src-tauri/phone/index.html")] = process.argv;
const port = +(process.env.PORT || 9380);
fs.mkdirSync(outDir, { recursive: true });
setTimeout(() => { console.log("TIMEOUT"); process.exit(2); }, 120000).unref();

const STATE = {
  t: "state", pc: "DESKTOP-GAMING",
  media: { current: { id: "spotify", title: "Nights", artist: "Frank Ocean", app: "Spotify", status: "playing", pos: 94, len: 307 } },
  master: { vol: 0.62, muted: false },
  apps: [{ key: "spotify", name: "Spotify", vol: 0.8, muted: false }, { key: "cs2", name: "Counter-Strike 2", vol: 0.55, muted: false }, { key: "discord", name: "Discord", vol: 1, muted: false }],
  outputs: [{ id: "a", name: "Speakers (Realtek High Definition Audio)", default: false }, { id: "b", name: "Headphones (HyperX Cloud II Virtual Surround Sound)", default: true }],
  power: { timers: [{ id: 1, label: "Pasta", end_ms: Date.now() + 340000, done: false }], now: Date.now(), awake_active: false, plan: null },
  boost: { on: false, busy: false },
  sys: { cpu: 23, ram: 58, gpu: { temp: 61 }, down: 920000 },
  profiles: { cpu: [{ id: "eco", name: "Eco" }, { id: "mid", name: "Mid" }, { id: "max", name: "Max" }], gpu: [{ id: "ab5", name: "AB Profile 5" }, { id: "q", name: "Quiet" }], active_cpu: "mid", active_gpu: "ab5" },
  shared: [{ id: 1, kind: "link", text: "https://youtu.be/dQw4w9WgXcQ", from: "phone" }, { id: 2, kind: "file", name: "IMG_2041.jpg", size: 2400000, from: "pc" }],
  toasts: [],
  // the phone as speaker and mic (its card), with VB-CABLE on the PC
  speaker: false, mic: false, mic_default: false, cable: true,
};
const FAKE = `<script>
window.__sent = [];
window.__urls = [];
const realFetch = window.fetch;
window.fetch = (u, o) => (window.__urls.push(String(u)), String(u) === "/session" ? Promise.resolve(new Response("FAKESESSION")) : realFetch(u, o));
class FakeWS { constructor(u) { window.__urls.push(String(u)); this.readyState = 1; FakeWS.last = this; setTimeout(() => { this.onopen && this.onopen(); this.push(); }, 50); }
  send(m) { window.__sent.push(JSON.parse(m)); } close() {}
  push() { this.onmessage && this.onmessage({ data: JSON.stringify(window.__state) }); } }
window.__state = ${JSON.stringify(STATE)};
window.WebSocket = FakeWS;
window.confirm = () => true;
</script>`;
// The page runs under the policy Deck sends with it (phoneauth::csp_for: inline scripts by hash,
// nothing from elsewhere); a meta tag carries it here, and any violation counts as a script error.
const { createHash } = await import("node:crypto");
const WATCH = `<script>document.addEventListener("securitypolicyviolation", (e) => console.error("CSP blocked " + e.violatedDirective + " " + e.blockedURI));</script>`;
const page0 = fs.readFileSync(src, "utf8").replace("<head>", "<head>" + WATCH + FAKE);
const hashes = [...page0.matchAll(/<script(?![^>]*src=)[^>]*>([\s\S]*?)<\/script>/g)].map((m) => `'sha256-${createHash("sha256").update(m[1].replace(/\r\n?/g, "\n")).digest("base64")}'`).join(" ");
const CSP = `default-src 'none'; script-src 'self' ${hashes}; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; media-src 'self' blob:; connect-src 'self' ws: wss:; worker-src 'self' blob:; manifest-src 'self'; base-uri 'none'; form-action 'none'`;
const html = page0.replace("<head>", `<head><meta http-equiv="Content-Security-Policy" content="${CSP}">`);
const file = path.join(here, `phone-harness-${port}.html`);
fs.writeFileSync(file, html);

const edge = spawn(BROWSER, ["--headless=new", `--remote-debugging-port=${port}`, "--allow-file-access-from-files", "--disable-gpu", "--hide-scrollbars", `--user-data-dir=${process.env.TEMP}\\deck-phone-edge-${process.pid}`, "about:blank"], { stdio: "ignore" });
let list;
for (let i = 0; i < 150; i++) { try { list = await (await fetch(`http://127.0.0.1:${port}/json`)).json(); if (list.find((t) => t.type === "page" && t.url === "about:blank")) break; } catch {} await new Promise((r) => setTimeout(r, 200)); }
const ws = new WebSocket(list.find((x) => x.type === "page" && x.url === "about:blank").webSocketDebuggerUrl);
let id = 0; const pend = new Map(); const logs = [];
ws.onmessage = (m) => { const d = JSON.parse(m.data); if (d.id && pend.has(d.id)) { pend.get(d.id)(d); pend.delete(d.id); } if (d.method === "Runtime.exceptionThrown") logs.push("EXC " + (d.params.exceptionDetails.exception?.description || d.params.exceptionDetails.text)); if (d.method === "Runtime.consoleAPICalled" && d.params.type === "error") logs.push("ERR " + d.params.args.map((a) => a.value || a.description).join(" ")); };
await new Promise((r) => (ws.onopen = r));
const send = (method, params = {}) => new Promise((res) => { const i = ++id; pend.set(i, res); ws.send(JSON.stringify({ id: i, method, params })); });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const ev = async (e) => { const r = await send("Runtime.evaluate", { expression: e, awaitPromise: true, returnByValue: true }); return r.result?.result?.value; };
await send("Runtime.enable");
const sizes = [["phone-320x568", 320, 568, 2], ["phone-375x812", 375, 812, 3], ["phone-430x932", 430, 932, 3], ["tablet-768x1024", 768, 1024, 2], ["tablet-landscape-1024x768", 1024, 768, 2]];
for (const [name, w, h, dpr] of sizes) {
  await send("Emulation.setDeviceMetricsOverride", { width: w, height: h, deviceScaleFactor: dpr, mobile: true });
  await send("Emulation.setTouchEmulationEnabled", { enabled: true, maxTouchPoints: 5 });
  await send("Page.navigate", { url: pathToFileURL(file).href + "#k=phonekey-SECRET" });
  await sleep(900);
  const m = await ev(`({ overflowX: document.documentElement.scrollWidth - innerWidth, height: document.documentElement.scrollHeight, unnamed: [...document.querySelectorAll("button, a[href]")].filter((b) => b.offsetParent && !(b.getAttribute("aria-label") || b.textContent.trim() || b.title)).length, small: [...document.querySelectorAll("button, a[href]")].filter((b) => { const r = b.getBoundingClientRect(); return b.offsetParent && (r.width < 24 || r.height < 24); }).length, zoomable: !/user-scalable=no/.test(document.querySelector("meta[name=viewport]").content) })`);
  console.log(name.padEnd(28), JSON.stringify(m));
  const full = await ev(`document.documentElement.scrollHeight`);
  await send("Emulation.setDeviceMetricsOverride", { width: w, height: Math.min(full, 4000), deviceScaleFactor: 1, mobile: true });
  await sleep(300);
  const r = await send("Page.captureScreenshot", { format: "png" });
  fs.writeFileSync(path.join(outDir, `remote-${name}.png`), Buffer.from(r.result.data, "base64"));
}
// a tap that spans a state push (the PC sends one every 1.5 s) must still count
await send("Emulation.setDeviceMetricsOverride", { width: 375, height: 812, deviceScaleFactor: 1, mobile: true });
await send("Page.navigate", { url: pathToFileURL(file).href + "#k=phonekey-SECRET" });
await sleep(900);
if (logs.length) { console.log("FAIL  " + logs.slice(0, 5).join(" | ")); process.exit(1); }
const at = await ev(`(() => { const b = document.querySelector("[data-a=awake]"); b.scrollIntoView({ block: "center" }); const r = b.getBoundingClientRect(); return [r.left + r.width / 2, r.top + r.height / 2]; })()`);
await ev(`window.__sent.length = 0`);
await send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: [{ x: at[0], y: at[1] }] });
await ev(`WebSocket.last.push()`);
await sleep(60);
await send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });
await sleep(300);
const sent = await ev(`window.__sent.filter((m) => m.a === "awake").length`);
console.log(`${sent === 1 ? "PASS" : "FAIL"}  tap survives a state push (awake messages: ${sent})`);
// the key stays on the phone: what goes to the PC is a session and a signature (phoneauth.rs)
const urls = await ev(`[...window.__urls, ...[...document.querySelectorAll("a[href]")].map((a) => a.getAttribute("href"))]`);
const leaks = urls.filter((u) => u.includes("phonekey-SECRET"));
const signedWs = urls.some((u) => /\/ws\?s=FAKESESSION&c=\d+&m=[0-9a-f]{64}$/.test(u));
console.log(`${!leaks.length && signedWs ? "PASS" : "FAIL"}  the phone key is in no request (${urls.length} checked${leaks.length ? ", leaked in " + leaks[0] : ""}${signedWs ? "" : ", no signed /ws"})`);
console.log(logs.length ? logs.join("\n") : "no script errors");
try { ws.close(); spawnSync("taskkill", ["/pid", String(edge.pid), "/T", "/F"], { stdio: "ignore" }); } catch {}
process.exit(0);
