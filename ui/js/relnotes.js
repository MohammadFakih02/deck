// Release notes from GitHub (a CHANGELOG section in Markdown), laid out for the Updates card:
// sections (New, Changed, Fixed, Security) with their items, each item's bold lead standing out.
// Built from text nodes only: the notes are never put in as HTML.
import { h } from "./core.js";

const SECTIONS = {
  added: ["New", "good", true],
  changed: ["Changed", "acc", true],
  fixed: ["Fixed", "warn", false],
  security: ["Security", "bad", false],
  removed: ["Removed", "dim", false],
};

/** `**bold**`, `*italic*`, `` `code` `` and `[text](url)` (the text only) in one line, as nodes. */
export function inline(text) {
  const out = [];
  const re = /\*\*(.+?)\*\*|`([^`]+)`|\*([^*\s][^*]*?)\*|\[([^\]]+)\]\([^)]*\)/g;
  let at = 0;
  for (const m of text.matchAll(re)) {
    if (m.index > at) out.push(text.slice(at, m.index));
    if (m[1] !== undefined) out.push(h("b", null, inline(m[1])));
    else if (m[2] !== undefined) out.push(h("code", null, m[2]));
    else if (m[3] !== undefined) out.push(h("i", null, m[3]));
    else out.push(m[4]);
    at = m.index + m[0].length;
  }
  if (at < text.length) out.push(text.slice(at));
  return out;
}

/** The notes as sections: [{ title, kind, open, items: [text] }]. An item's indented lines join it;
 *  the setup's checksum line is left out (Deck checks it by itself). */
export function parse(md) {
  const sections = [];
  let cur = null;
  for (const raw of String(md || "").split(/\r?\n/)) {
    const line = raw.trimEnd();
    if (!line.trim() || /^SHA-256 of /i.test(line.trim())) continue;
    const head = line.match(/^#{1,6}\s+(.*)$/);
    if (head) {
      const key = head[1].trim().toLowerCase();
      const [title, kind, open] = SECTIONS[key] || [head[1].trim(), "dim", true];
      cur = { title, kind, open, items: [] };
      sections.push(cur);
      continue;
    }
    if (!cur) {
      cur = { title: "", kind: "dim", open: true, items: [] };
      sections.push(cur);
    }
    const item = line.match(/^[-*]\s+(.*)$/);
    if (item) cur.items.push(item[1]);
    else if (cur.items.length && /^\s/.test(line)) cur.items[cur.items.length - 1] += " " + line.trim().replace(/^[-*]\s+/, "");
    else cur.items.push(line.trim());
  }
  return sections.filter((s) => s.items.length);
}

/** A "(where to find it)" at the start of `text`, and the rest: ["(Deck → Phone)", "rest"]. */
export function aside(text) {
  if (!text.startsWith("(")) return ["", text];
  let depth = 0;
  for (let i = 0; i < text.length; i++) {
    if (text[i] === "(") depth++;
    else if (text[i] === ")" && --depth === 0) return [text.slice(0, i + 1), text.slice(i + 1).replace(/^[\s:.,;]+/, "")];
  }
  return ["", text];
}

/** One item: its bold lead on top (with a leading "(where)" beside it), the rest under it, long
 *  ones folded to a few lines. */
function item(text) {
  const lead = text.match(/^\*\*(.+?)\*\*[:.]?\s*(.*)$/);
  const [where, rest] = lead ? aside(lead[2]) : ["", text];
  // "**Built-in apps:** remove Clipchamp…" reads as a sentence once the lead has its own line
  const body = lead ? rest.charAt(0).toUpperCase() + rest.slice(1) : rest;
  const long = body.length > 220;
  const el = h("li", { class: "rn-item" + (long ? " folded" : "") },
    lead ? h("div", { class: "rn-lead" }, inline(lead[1].replace(/[:.]$/, "")), where ? h("span", { class: "rn-where" }, " ", inline(where)) : null) : null,
    body ? h("div", { class: "rn-body" }, inline(body)) : null);
  if (long) {
    const more = h("button", { class: "rn-more", "aria-expanded": "false", onclick: () => {
      const open = el.classList.toggle("folded") === false;
      more.setAttribute("aria-expanded", String(open));
      more.textContent = open ? "Less" : "More";
    } }, "More");
    el.append(more);
  }
  return el;
}

/** The notes for `version`: New and Changed open, Fixed and Security folded under their counts. */
export function releaseNotes(md, version, openUrl) {
  const sections = parse(md);
  if (!sections.length) return null;
  return h("div", { class: "rn" },
    h("div", { class: "rn-head" },
      h("b", null, `What's new in ${version}`),
      openUrl ? h("button", { class: "btn ghost sm", onclick: openUrl }, "Full notes on GitHub") : null),
    sections.map((s) => {
      const list = h("ul", { class: "rn-list" }, s.items.map(item));
      const label = h("span", { class: `rn-tag ${s.kind}` }, s.title || "Notes");
      const count = h("span", { class: "rn-count" }, String(s.items.length));
      return h("details", { class: "rn-sec", open: s.open ? "" : null }, h("summary", null, label, count), list);
    }));
}
