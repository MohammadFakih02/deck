// usage: node phone.mjs <phone key> <cpu profile id to switch to> <cpu profile id to go back to>
// Requests are signed like the phone page does (see src-tauri/src/phoneauth.rs): the key never travels.
import crypto from "node:crypto";
const [tok, idA, idB] = process.argv.slice(2), base = "http://127.0.0.1:47830";
const sid = await (await fetch(base + "/session", { method: "POST" })).text();
let ctr = 0;
const signed = (path, key = tok) => { ctr++; return `s=${sid}&c=${ctr}&m=${crypto.createHmac("sha256", key).update(`${sid}|${ctr}|${path}`).digest("hex")}`; };
const st = async (u) => (await fetch(base + u)).status;
const replay = signed("/check");
console.log("check bad", await st("/check?" + signed("/check", "nope")), "good", await st("/check?" + replay), "replayed", await st("/check?" + replay), "page", await st("/"));
console.log("old ?k= refused", await st("/check?k=" + tok), "foreign host refused", (await fetch(base + "/", { headers: { host: "evil.example.com" } })).status);
const ws = new WebSocket(`ws://127.0.0.1:47830/ws?${signed("/ws")}`);
let n = 0, last;
ws.onmessage = (m) => { n++; last = JSON.parse(m.data); };
await new Promise((r) => (ws.onopen = r));
await new Promise((r) => setTimeout(r, 2500));
console.log("msgs", n, "keys", Object.keys(last || {}).slice(0, 25).join(","));
const cpu = last?.cpu || last?.profiles?.cpu;
console.log("cpu block", JSON.stringify(cpu)?.slice(0, 200));
ws.send(JSON.stringify({ a: "profile", kind: "cpu", id: idA }));
await new Promise((r) => setTimeout(r, 3000));
console.log("after switch", JSON.stringify(last?.cpu || last?.profiles?.cpu)?.slice(0, 120));
ws.send(JSON.stringify({ a: "profile", kind: "cpu", id: idB }));
await new Promise((r) => setTimeout(r, 2500));
console.log("back", JSON.stringify(last?.cpu || last?.profiles?.cpu)?.slice(0, 120));
ws.close();
