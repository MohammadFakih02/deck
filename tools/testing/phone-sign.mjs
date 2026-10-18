// Checks the phone page's request signing (SHA-256 and HMAC written out in the page, since a page
// on plain HTTP has no crypto.subtle) against the standard test values and against the PC's side
// (phoneauth.rs, test signatures_match_the_phone_page). No Deck needed.
//   node tools/testing/phone-sign.mjs
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
const here = path.dirname(fileURLToPath(import.meta.url));
const html = fs.readFileSync(path.resolve(here, "../../src-tauri/phone/index.html"), "utf8");
const code = html.split("// -- signing --")[1].split("// -- end signing --")[0];
const api = new Function("key", `${code}; return { sha256, hmac, hexOf, signed, setSid: (s, c) => { sid = s; ctr = c; } };`);
let failed = 0;
const check = (name, got, want) => { const ok = got === want; console.log(`${ok ? "PASS" : "FAIL"}  ${name}${ok ? "" : `  (got ${got}, want ${want})`}`); if (!ok) failed++; };
const te = new TextEncoder();
const a = api("phone-key");
check("sha256 of nothing", a.hexOf(a.sha256(new Uint8Array(0))), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
check("sha256 of abc", a.hexOf(a.sha256(te.encode("abc"))), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
check("sha256 across two blocks", a.hexOf(a.sha256(te.encode("abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"))), "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1");
check("hmac RFC 4231 case 2", a.hexOf(a.hmac(te.encode("Jefe"), te.encode("what do ya want for nothing?"))), "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843");
check("hmac RFC 4231 case 6 (long key)", a.hexOf(a.hmac(new Uint8Array(131).fill(0xaa), te.encode("Test Using Larger Than Block-Size Key - Hash Key First"))), "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54");
a.setSid("SESSIONID", 6);
check("signed() matches the PC", a.signed("/ws"), "s=SESSIONID&c=7&m=816f170724a06f745eac07cdded9c07d9913abb49be6987187e3d0216b3276f6");
const b = api("k".repeat(100));
b.setSid("x", 0);
check("signed() with a long key matches the PC", b.signed("/file/3"), "s=x&c=1&m=76bd60e4cbc61b20cf0db821cfe995debe0e51707c70c3c92ab9a4e7a3bdcdc3");
check("the key itself is in no request", a.signed("/ws").includes("phone-key"), false);
process.exit(failed ? 1 : 0);
