// Game boost: Full power while you play (everything comes back after) and a one-off Clean up.
import { I, Pages, S, ago, appIcon, fmtBytes, h, helpTip, invoke, note, setSetting, toggle } from "./core.js";
import { head } from "./pages1.js";
import { row } from "./pcs.js";
import { render } from "./app.js";


Pages.boost = {
  render() {
    const st = S.cache.boost || {};
    const el = h("div", null,
      head("Game boost", "Free up the PC for a game. Games, launchers, anti-cheat, voice chat and your mouse or headset software are never touched."));
    el.append(boostHero(st));
    el.append(boostApps(st));
    return el;
  },
  async load() {
    const [st, apps] = await Promise.all([invoke("boost_state"), invoke("boost_apps")]);
    S.cache.boost = st;
    S.cache.boostApps = apps;
    S.cache.boostTick = 0;
  },
  live: true,
  async tick() {
    const before = JSON.stringify([S.cache.boost, S.cache.boostApps]);
    S.cache.boost = await invoke("boost_state");
    // the app list is slower to gather: every 5 s
    if (++S.cache.boostTick % 5 === 0 && !S.cache.boost.busy) S.cache.boostApps = await invoke("boost_apps");
    return JSON.stringify([S.cache.boost, S.cache.boostApps]) !== before;
  },
};

export async function boostRun(what) {
  S.cache.boost = { ...(S.cache.boost || {}), busy: true };
  render();
  try {
    const r = await invoke("boost_run", { what });
    if (what === "off") note(r.reopened?.length ? `Back to normal. Reopened ${r.reopened.join(", ")}` : "Back to normal");
    else if (what === "on") note("Full power is on");
    else note(cleanSummary(r));
  } catch (e) {
    note(String(e), true);
  }
  await Pages.boost.load();
  if (S.page === "boost") render();
}

export function cleanSummary(r) {
  const parts = [];
  parts.push(r.closed?.length ? `Closed ${r.closed.join(", ")}` : "Nothing to close");
  if (r.freed > 50 * 1048576) parts.push(`freed ${fmtBytes(r.freed)}`);
  return parts.join(" · ");
}

/** One card: Full power on the left (with the one-off Clean up under it), what it does on the right. */
export function boostHero(st) {
  const a = st.active;
  const busy = st.busy;
  const ram = st.ram || {};
  const last = st.last_clean;
  const left = h("div", { class: "col boost-main" });
  left.append(h("div", { class: "tile-head" }, I("zap"), h("span", null, "Full power"), h("span", { class: "grow" }), a ? h("span", { class: "chip on" }, "On") : null));
  if (!a) {
    left.append(
      h("div", { class: "boost-say" }, "Everything for the game, then everything back."),
      h("div", { class: "dim" }, "Closes the apps ticked below, switches to the fastest power plan, slows down background apps and frees memory. When your game closes, Deck puts it all back."),
      h("button", { class: "btn primary lg", disabled: busy, onclick: () => boostRun("on") }, I("zap"), busy ? "Working…" : "Turn on Full power"));
  } else {
    const since = new Date(a.since * 1000).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
    left.append(
      h("div", { class: "boost-say" }, a.game ? `Playing ${a.game}` : a.seen_game ? "Game closed, turning off soon…" : "Waiting for a game…"),
      h("div", { class: "dim" }, `On since ${since}. ${S.state.settings.boost_auto_end ? "Turns itself off about a minute after your game closes." : "Stays on until you turn it off."}`),
      h("button", { class: "btn lg", disabled: busy, onclick: () => boostRun("off") }, I("x"), busy ? "Working…" : "Turn off and put everything back"));
  }
  left.append(
    h("div", { class: "boost-once" },
      h("div", { class: "grow" }, h("div", { class: "t" }, "Just clean up, once"),
        h("div", { class: "s" }, last ? `${ago(last.at * 1000)}: ${cleanSummary(last)}${last.kept?.length ? ` · ${last.kept.join(", ")} stayed open` : ""}` : "Closes the ticked apps and frees memory. Nothing reopens later.")),
      h("button", { class: "btn", disabled: busy, onclick: () => boostRun("clean") }, I("broom"), busy ? "Working…" : "Clean up now")),
    ram.total ? h("div", { class: "boost-ram" },
      h("div", { class: "row" }, h("span", { class: "dim" }, "Memory in use"), h("span", { class: "grow" }), h("b", { class: "num" }, `${fmtBytes(ram.total - ram.free)} of ${fmtBytes(ram.total)}`)),
      h("div", { class: "meter" }, h("i", { style: { width: `${Math.round((1 - ram.free / ram.total) * 100)}%` } }))) : null);
  return h("div", { class: "card boost-hero2" + (a ? " on" : "") }, left, a ? boostDone(a) : boostOptions(st));
}

/** What Full power did, one line each. */
export function boostDone(a) {
  const line = (ic, text, tip) => h("div", { class: "boost-line", title: tip || "" }, I(ic, "sm"), h("span", null, text));
  const rows = [];
  if (a.closed.length) rows.push(line("check", `Closed ${a.closed.length} app${a.closed.length === 1 ? "" : "s"}`, a.closed.join(", ")));
  if (a.plan_to) rows.push(line("check", `Power plan: ${a.plan_to}`, `Back to ${a.plan_from} after`));
  if (a.lowered) rows.push(line("check", `${a.lowered} background apps slowed`));
  if (a.freed > 50 * 1048576) rows.push(line("check", `${fmtBytes(a.freed)} of memory freed`));
  if (!rows.length) rows.push(line("info", "Nothing needed changing"));
  return h("div", { class: "col boost-side" }, h("div", { class: "boost-sub" }, "What it did"), ...rows,
    a.kept.length ? h("div", { class: "dim small" }, `${a.kept.join(", ")} stayed open (it may be asking to save something).`) : null);
}

export function boostOptions(st) {
  const s = S.state.settings;
  const p = st.plan || {};
  const planText = p.kind === "scheme" ? `${p.from || "Your plan"} → ${p.name} while you play, then back.`
    : p.kind === "overlay" ? "Power mode → Best performance while you play, then back."
    : p.kind === "already" ? `Already on ${p.name}, nothing to change.`
    : "This PC doesn't offer a faster plan.";
  const row = (key, title, sub, disabled) => h("label", { class: "boost-opt" + (disabled ? " off" : "") },
    toggle(!!s[key] && !disabled, (v) => { setSetting(key, v); render(); }),
    h("div", null, h("div", { class: "t" }, title, helpTip(key.replace("_", "."))), h("div", { class: "s" }, sub)));
  return h("div", { class: "col boost-side" },
    h("div", { class: "boost-sub" }, "While it's on"),
    row("boost_power", "Fastest power plan", s.boost_cpu ? "Not used: the CPU profile you picked below brings its own power plan." : planText, p.kind === "none" || p.kind === "already" || !!s.boost_cpu),
    boostProfiles(s),
    row("boost_priority", "Slow down background apps", "Apps with no window get low priority, so the game gets the CPU first. Voice chat and music are left alone."),
    row("boost_trim", "Free memory when it's low", "Only when 85% or more of the memory is in use: moves idle memory of background apps aside. With room to spare it does nothing, since Windows already gives the game what it needs."),
    row("boost_auto_end", "Turn off by itself", "About a minute after your game closes, or after 3 hours if no game shows up. Deck spots games from Steam, Epic, Riot, GOG, Ubisoft and Xbox, anything fullscreen, and the program in front when it works the graphics card hard."));
}

export function boostApps(st) {
  const list = S.cache.boostApps;
  const el = h("div", null);
  if (!list) return el.append(h("div", { class: "section" }, h("h2", null, "Apps to close")), h("div", { class: "empty" }, "Looking at what's running…")), el;
  const picked = list.filter((a) => a.checked);
  const mem = picked.reduce((n, a) => n + (a.mem || 0), 0);
  const setAll = async (want) => {
    await Promise.all(list.filter((a) => !!a.checked !== want(a)).map((a) => invoke("boost_pick", { key: a.key, on: want(a) })));
    S.cache.boostApps = await invoke("boost_apps");
    render();
  };
  el.append(h("div", { class: "section" }, h("h2", null, "Apps to close"), h("span", { class: "dim small" }, list.length ? `${picked.length} of ${list.length} ticked${mem > 50 * 1048576 ? ` · about ${fmtBytes(mem)}` : ""}` : ""), h("span", { class: "grow" }),
    list.some((a) => a.suggested) ? h("button", { class: "btn sm ghost", onclick: () => setAll((a) => !!a.suggested) }, "Only suggested") : null,
    list.length ? h("button", { class: "btn sm ghost", onclick: () => setAll(() => false) }, "None") : null));
  if (!list.length) return el.append(h("div", { class: "empty" }, "Nothing to close. Only Windows, games and protected apps are running.")), el;
  el.append(h("div", { class: "boost-grid" }, list.map((a) => {
    const sub = [fmtBytes(a.mem), a.window ? "window open" : "in the background"];
    if (a.voice) sub.push("voice, calls or music");
    return h("div", { class: "win boost-app" + (a.checked ? " picked" : "") },
      appIcon("file", a.exe, a.name),
      h("div", { style: { minWidth: 0 } },
        h("div", { class: "t ell" }, a.name, a.suggested ? h("span", { class: "chip sm-chip" }, "Suggested") : null),
        h("div", { class: "s ell", title: a.exe }, sub.join(" · "))),
      toggle(a.checked, async (v) => {
        a.checked = v;
        await invoke("boost_pick", { key: a.key, on: v });
        render();
      }));
  })));
  el.append(h("div", { class: "faint small", style: { marginTop: "10px" } }, "Both buttons close the ticked apps. Full power opens them again afterwards."));
  return el;
}

/** "While Full power is on, use CPU profile [..] and GPU profile [..]" (put back afterwards). */
export function boostProfiles(s) {
  if (!s.cpu_profiles.length && !s.gpu_profiles.length) return null;
  const pick = (key, list, kind) => h("label", { class: "field" }, h("span", null, kind + " profile"),
    h("select", { onchange: (e) => { setSetting(key, e.target.value); render(); } },
      h("option", { value: "", selected: !s[key] }, "Don't change"),
      list.map((p) => h("option", { value: p.id, selected: s[key] === p.id }, p.name))));
  return h("div", { class: "boost-opt", style: { cursor: "default" } }, h("span", { style: { width: "38px" } }),
    h("div", null, h("div", { class: "t" }, "Switch profiles while it's on"), h("div", { class: "s" }, "Your previous profiles come back when Full power turns off. Picking a CPU profile replaces Fastest power plan."),
      h("div", { class: "row wrap", style: { marginTop: "8px" } },
        s.cpu_profiles.length ? pick("boost_cpu", s.cpu_profiles, "CPU") : null,
        s.gpu_profiles.length ? pick("boost_gpu", s.gpu_profiles, "GPU") : null)));
}
