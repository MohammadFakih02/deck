// usage: node phone.mjs <phone key> <cpu profile id to switch to> <cpu profile id to go back to>
const [tok, idA, idB] = process.argv.slice(2), base = "http://127.0.0.1:47830";
const st = async (u) => (await fetch(base + u)).status;
console.log("check bad", await st("/check?k=nope"), "good", await st("/check?k=" + tok), "page", await st("/"));
const ws = new WebSocket(`ws://127.0.0.1:47830/ws?k=${tok}`);
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
