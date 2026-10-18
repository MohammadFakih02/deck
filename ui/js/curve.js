// Canvas curve editor shared by the fan curves (°C → %) and the GPU fan curve.
// Drag a point, click empty space to add one, right-click a point to remove it.
// A live marker shows where the reading is now and what the curve gives for it.
import { S, accent, h } from "./core.js";
import { hover } from "./search.js";


/**
 * opts: { points: [[x, y]], x: [min, max], y: [min, max], xUnit, yUnit, marker: () => x | null,
 *         onchange(points), minPoints, height }
 * Returns { el, set(points), redraw() }.
 */
export function curveEditor(opts) {
  const o = { minPoints: 2, height: 230, xUnit: "°C", yUnit: "%", ...opts };
  let pts = (o.points || []).map((p) => [p[0], p[1]]).sort((a, b) => a[0] - b[0]);
  const wrap = h("div", { class: "curve" });
  const cv = h("canvas", { class: "curve-cv", style: { height: o.height + "px" } });
  const tip = h("div", { class: "curve-tip hidden" });
  wrap.append(cv, tip);
  const pad = { l: 38, r: 12, t: 12, b: 26 };
  let drag = -1, hover = -1;

  const geo = () => {
    const w = cv.clientWidth || 600, ht = cv.clientHeight || o.height;
    return { w, ht, iw: w - pad.l - pad.r, ih: ht - pad.t - pad.b };
  };
  const toPx = (p, g) => [pad.l + ((p[0] - o.x[0]) / (o.x[1] - o.x[0])) * g.iw, pad.t + (1 - (p[1] - o.y[0]) / (o.y[1] - o.y[0])) * g.ih];
  const fromPx = (x, y, g) => [o.x[0] + ((x - pad.l) / g.iw) * (o.x[1] - o.x[0]), o.y[0] + (1 - (y - pad.t) / g.ih) * (o.y[1] - o.y[0])];
  const evalAt = (x) => {
    if (!pts.length) return null;
    if (x <= pts[0][0]) return pts[0][1];
    for (let i = 1; i < pts.length; i++) {
      if (x <= pts[i][0]) {
        const [x0, y0] = pts[i - 1], [x1, y1] = pts[i];
        return y0 + ((y1 - y0) * (x - x0)) / Math.max(0.001, x1 - x0);
      }
    }
    return pts[pts.length - 1][1];
  };

  function draw() {
    const g = geo();
    const dpr = window.devicePixelRatio || 1;
    cv.width = g.w * dpr; cv.height = g.ht * dpr;
    const c = cv.getContext("2d");
    c.scale(dpr, dpr);
    const css = getComputedStyle(document.documentElement);
    const acc = accent(), line = css.getPropertyValue("--line").trim() || "#222838", faint = css.getPropertyValue("--faint").trim() || "#636b7e";
    c.font = "11px Segoe UI, sans-serif";
    c.fillStyle = faint;
    c.strokeStyle = line;
    c.lineWidth = 1;
    // grid
    const xs = niceSteps(o.x[0], o.x[1], 8), ys = niceSteps(o.y[0], o.y[1], 5);
    for (const x of xs) {
      const [px] = toPx([x, o.y[0]], g);
      c.beginPath(); c.moveTo(px + 0.5, pad.t); c.lineTo(px + 0.5, pad.t + g.ih); c.stroke();
      const last = x === xs[xs.length - 1];
      c.textAlign = last ? "right" : "center"; c.fillText(x + (last ? " " + o.xUnit : ""), last ? px + 4 : px, g.ht - 8);
    }
    for (const y of ys) {
      const [, py] = toPx([o.x[0], y], g);
      c.beginPath(); c.moveTo(pad.l, py + 0.5); c.lineTo(pad.l + g.iw, py + 0.5); c.stroke();
      c.textAlign = "right"; c.fillText(y + (o.yUnit === "%" ? "%" : ""), pad.l - 6, py + 4);
    }
    if (!pts.length) return;
    // area + line
    const P = pts.map((p) => toPx(p, g));
    c.beginPath();
    c.moveTo(pad.l, P[0][1]);
    P.forEach(([x, y]) => c.lineTo(x, y));
    c.lineTo(pad.l + g.iw, P[P.length - 1][1]);
    c.strokeStyle = acc; c.lineWidth = 2.2; c.lineJoin = "round"; c.stroke();
    c.lineTo(pad.l + g.iw, pad.t + g.ih); c.lineTo(pad.l, pad.t + g.ih); c.closePath();
    const grad = c.createLinearGradient(0, pad.t, 0, pad.t + g.ih);
    grad.addColorStop(0, acc + "40"); grad.addColorStop(1, acc + "05");
    c.fillStyle = grad; c.fill();
    // live marker
    const m = o.marker ? o.marker() : null;
    if (m != null && isFinite(m)) {
      const mx = Math.min(Math.max(m, o.x[0]), o.x[1]);
      const my = evalAt(mx);
      const [px, py] = toPx([mx, my], g);
      c.setLineDash([4, 4]); c.strokeStyle = "#ffb547"; c.lineWidth = 1.2;
      c.beginPath(); c.moveTo(px, pad.t); c.lineTo(px, pad.t + g.ih); c.stroke(); c.setLineDash([]);
      c.fillStyle = "#ffb547"; c.beginPath(); c.arc(px, py, 4.5, 0, 7); c.fill();
      c.textAlign = px > pad.l + g.iw - 90 ? "right" : "left";
      c.fillText(`${Math.round(m)}${o.xUnit} → ${Math.round(my)}${o.yUnit}`, px + (c.textAlign === "left" ? 8 : -8), pad.t + 12);
    }
    // points
    P.forEach(([x, y], i) => {
      c.beginPath(); c.arc(x, y, i === drag || i === hover ? 7 : 5.5, 0, 7);
      c.fillStyle = i === drag ? acc : "#fff"; c.fill();
      c.lineWidth = 2; c.strokeStyle = acc; c.stroke();
    });
  }

  const hit = (e) => {
    const g = geo();
    const r = cv.getBoundingClientRect();
    const x = e.clientX - r.left, y = e.clientY - r.top;
    let best = -1, bd = 12;
    pts.forEach((p, i) => {
      const [px, py] = toPx(p, g);
      const d = Math.hypot(px - x, py - y);
      if (d < bd) { bd = d; best = i; }
    });
    return { i: best, x, y, g };
  };
  const clampPt = (i, p) => {
    const lo = i > 0 ? pts[i - 1][0] + 1 : o.x[0];
    const hi = i < pts.length - 1 ? pts[i + 1][0] - 1 : o.x[1];
    return [Math.round(Math.min(Math.max(p[0], lo), hi)), Math.round(Math.min(Math.max(p[1], o.y[0]), o.y[1]))];
  };
  const showTip = (i, g) => {
    if (i < 0) { tip.classList.add("hidden"); return; }
    const [px, py] = toPx(pts[i], g);
    tip.textContent = `${pts[i][0]}${o.xUnit} · ${pts[i][1]}${o.yUnit}`;
    placeTip(tip, wrap, px, py);
  };
  const changed = () => o.onchange && o.onchange(pts.map((p) => [p[0], p[1]]));

  cv.addEventListener("pointerdown", (e) => {
    if (e.button !== 0) return;
    const { i, x, y, g } = hit(e);
    if (i >= 0) drag = i;
    else {
      const p = fromPx(x, y, g);
      if (p[0] < o.x[0] || p[0] > o.x[1]) return;
      pts.push([Math.round(p[0]), Math.round(p[1])]);
      pts.sort((a, b) => a[0] - b[0]);
      drag = pts.findIndex((q) => q[0] === Math.round(p[0]));
      changed();
    }
    S.interacting++;
    cv.setPointerCapture(e.pointerId);
    draw(); showTip(drag, g);
  });
  cv.addEventListener("pointermove", (e) => {
    const { i, x, y, g } = hit(e);
    if (drag >= 0) {
      pts[drag] = clampPt(drag, fromPx(x, y, g));
      draw(); showTip(drag, g);
    } else if (i !== hover) {
      hover = i; cv.style.cursor = i >= 0 ? "grab" : "crosshair"; draw(); showTip(i, g);
    }
  });
  const end = () => {
    if (drag < 0) return;
    drag = -1; S.interacting = Math.max(0, S.interacting - 1);
    tip.classList.add("hidden");
    changed(); draw();
  };
  cv.addEventListener("pointerup", end);
  cv.addEventListener("pointercancel", end);
  cv.addEventListener("pointerleave", () => { if (drag < 0) { hover = -1; tip.classList.add("hidden"); draw(); } });
  cv.addEventListener("contextmenu", (e) => {
    e.preventDefault();
    const { i } = hit(e);
    if (i >= 0 && pts.length > o.minPoints) { pts.splice(i, 1); changed(); draw(); }
  });
  // canvases need a size before they can draw
  new ResizeObserver(() => draw()).observe(cv);
  return { el: wrap, set(p) { pts = p.map((q) => [q[0], q[1]]); draw(); }, redraw: draw, get: () => pts.map((p) => [p[0], p[1]]) };
}

/** Round grid steps between lo and hi, about n of them. */
export function niceSteps(lo, hi, n) {
  const raw = (hi - lo) / n;
  const mag = Math.pow(10, Math.floor(Math.log10(raw)));
  const step = [1, 2, 2.5, 5, 10].map((m) => m * mag).find((s) => s >= raw) || raw;
  const out = [];
  for (let v = Math.ceil(lo / step) * step; v <= hi + 1e-9; v += step) out.push(Math.round(v * 100) / 100);
  return out;
}

/** Line graph of several series (Sensors page): series = [{ values, color, label, max }] */
/** Puts a curve tooltip above (x, y), kept inside `box` so it never makes the page scroll. */
export function placeTip(tip, box, x, y) {
  tip.classList.remove("hidden");
  const bw = box.clientWidth, tw = tip.offsetWidth, th = tip.offsetHeight;
  tip.style.left = Math.round(Math.min(Math.max(x - tw / 2, 4), Math.max(4, bw - tw - 4))) + "px";
  tip.style.top = Math.round(y - th - 10 < 4 ? y + 12 : y - th - 10) + "px";
}

export function lineGraph(canvas, series, opts) {
  const o = { min: 0, max: 100, unit: "", ...opts };
  const dpr = window.devicePixelRatio || 1;
  const w = canvas.clientWidth || 400, ht = canvas.clientHeight || 140;
  canvas.width = w * dpr; canvas.height = ht * dpr;
  const c = canvas.getContext("2d");
  c.scale(dpr, dpr);
  const css = getComputedStyle(document.documentElement);
  const line = css.getPropertyValue("--line").trim() || "#222838", faint = css.getPropertyValue("--faint").trim() || "#636b7e";
  const pad = { l: 32, r: 6, t: 6, b: 6 };
  const iw = w - pad.l - pad.r, ih = ht - pad.t - pad.b;
  c.font = "10.5px Segoe UI, sans-serif"; c.fillStyle = faint; c.strokeStyle = line; c.lineWidth = 1;
  for (const y of niceSteps(o.min, o.max, 4)) {
    const py = pad.t + (1 - (y - o.min) / (o.max - o.min)) * ih;
    c.beginPath(); c.moveTo(pad.l, py + 0.5); c.lineTo(pad.l + iw, py + 0.5); c.stroke();
    c.textAlign = "right"; c.fillText(y + o.unit, pad.l - 5, py + 3.5);
  }
  for (const s of series) {
    const v = s.values || [];
    const n = o.n || 300;
    const step = iw / (n - 1), off = n - v.length;
    c.beginPath();
    let pen = false;
    v.forEach((y, i) => {
      if (y == null || !isFinite(y)) { pen = false; return; }
      const px = pad.l + (i + off) * step, py = pad.t + (1 - (Math.min(Math.max(y, o.min), o.max) - o.min) / (o.max - o.min)) * ih;
      pen ? c.lineTo(px, py) : c.moveTo(px, py);
      pen = true;
    });
    c.strokeStyle = s.color; c.lineWidth = 1.7; c.lineJoin = "round"; c.stroke();
  }
}
