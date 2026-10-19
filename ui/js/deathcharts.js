// Small inline charts for the Deaths page (no chart library): bars for tries and deaths over
// time, and a line for how long each try lasted. Colours come from the page's CSS (.dc-chart).
const NS = "http://www.w3.org/2000/svg";

function el(tag, attrs) {
  const e = document.createElementNS(NS, tag);
  for (const [k, v] of Object.entries(attrs || {})) e.setAttribute(k, v);
  return e;
}

function frame(w, h, height, label, cls) {
  const s = el("svg", { viewBox: `0 0 ${w} ${h}`, preserveAspectRatio: "none", role: "img", "aria-label": label || "", class: "dc-chart " + (cls || "") });
  s.style.height = height + "px";
  return s;
}

/** A bar per value, taller = more; the last one stands out when `last`. 0 shows as a stub. */
export function bars(values, { height = 28, last = true, label = "", cls = "" } = {}) {
  const v = values || [];
  const s = frame(Math.max(v.length, 1) * 10, height, height, label, "bars " + cls);
  const max = Math.max(1, ...v);
  v.forEach((x, i) => {
    const bh = x ? Math.max(3, (x / max) * height) : 2;
    s.append(el("rect", { x: i * 10 + 1, y: height - bh, width: 8, height: bh, rx: cls.includes("per") ? 0 : 1.5, class: last && i === v.length - 1 ? "dc-b now" : x ? "dc-b" : "dc-b none" }));
  });
  return s;
}

/** Deaths per stretch of time: plain bars, nothing stands out. */
export function perTime(counts, { height = 26, label = "" } = {}) {
  return bars(counts, { height, last: false, label, cls: "per" });
}

/** A line through `values` (oldest first), scaled to the largest. */
export function line(values, { height = 70, color = "var(--bad)", label = "" } = {}) {
  const w = 200;
  const s = frame(w, height, height, label, "line");
  const v = values || [];
  if (v.length < 2) return s;
  const max = Math.max(1, ...v);
  const pts = v.map((x, i) => `${((i / (v.length - 1)) * w).toFixed(1)},${(height - 4 - (x / max) * (height - 10)).toFixed(1)}`).join(" ");
  s.append(el("polyline", { points: pts, fill: "none", stroke: color, "stroke-width": 2, "vector-effect": "non-scaling-stroke", "stroke-linejoin": "round", "stroke-linecap": "round" }));
  return s;
}
