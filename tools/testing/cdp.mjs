// Talks to Deck's WebView2 over the DevTools protocol (Deck started with --remote-debugging-port=9222).
//   node cdp.mjs eval "<js expression, may await>"      prints the JSON result
//   node cdp.mjs shot out.png [w h]                      screenshot of Deck's page only (not the screen)
//   node cdp.mjs inv cmd '{"json":"args"}'               invoke a Tauri command
//   node cdp.mjs targets
//   CDP_PAGE=toast node cdp.mjs ...                      same, on the toast window
const [, , cmd, a1, a2, a3] = process.argv;
const list = await (await fetch("http://127.0.0.1:9222/json")).json();
if (cmd === "targets") { console.log(list.map((t) => `${t.type} ${t.title} ${t.url}`).join("\n")); process.exit(0); }
// CDP_PAGE=toast (or zones, osd) drives that window instead of the main one
const t = process.env.CDP_PAGE ? list.find((x) => x.type === "page" && x.url.includes(process.env.CDP_PAGE)) : list.find((x) => x.type === "page" && /index\.html|tauri\.localhost\/?$/.test(x.url) && !/toast|zones|osd/i.test(x.url)) || list.find((x) => x.type === "page");
const ws = new WebSocket(t.webSocketDebuggerUrl);
let id = 0;
const pending = new Map();
ws.onmessage = (m) => { const d = JSON.parse(m.data); if (d.id && pending.has(d.id)) { pending.get(d.id)(d); pending.delete(d.id); } };
await new Promise((r) => (ws.onopen = r));
const send = (method, params = {}) => new Promise((res) => { const i = ++id; pending.set(i, res); ws.send(JSON.stringify({ id: i, method, params })); });
async function ev(expr) {
  const r = await send("Runtime.evaluate", { expression: `(async () => { return (${expr}); })()`, awaitPromise: true, returnByValue: true, timeout: 120000 });
  if (r.result?.exceptionDetails) return { error: r.result.exceptionDetails.exception?.description || r.result.exceptionDetails.text };
  return r.result?.result?.value;
}
let out;
if (cmd === "eval") out = await ev(a1);
else if (cmd === "inv") out = await ev(`window.__TAURI__.core.invoke(${JSON.stringify(a1)}, ${a2 || "{}"}).catch(e => ({ error: String(e) }))`);
else if (cmd === "shot") {
  if (a2) await send("Emulation.setDeviceMetricsOverride", { width: +a2, height: +a3, deviceScaleFactor: 1, mobile: false });
  const r = await send("Page.captureScreenshot", { format: "png" });
  if (a2) await send("Emulation.clearDeviceMetricsOverride");
  const fs = await import("node:fs");
  fs.writeFileSync(a1, Buffer.from(r.result.data, "base64"));
  out = "saved " + a1;
}
console.log(typeof out === "string" ? out : JSON.stringify(out, null, 1));
ws.close();
process.exit(0);
