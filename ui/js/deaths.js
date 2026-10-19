// Monitor → Deaths: the death counter. Deaths per game and per boss, in any game. Counting
// happens in the backend (tray, hotkeys, this page); the in-game pill is a native window.
import { I, Pages, S, h, invoke, note, seg, setSetting, settingRow, slider, toggle, confirmBox } from "./core.js";
import { HEADS, tabsHead } from "./pages1.js";
import { DEFAULT_SUB, render } from "./app.js";
import { hotkeyField } from "./hw.js";
import { popMenu } from "./profiles.js";
import { bars, line, perTime } from "./deathcharts.js";

Object.assign(HEADS, {
  "deaths/now": ["Deaths", "Count your deaths in any game, per boss. The tray menu has the same buttons, so you never have to leave the game for long."],
  "deaths/bosses": ["Deaths", "Every boss you've fought in this game: deaths, tries, time, and whether your tries are getting longer."],
  "deaths/history": ["Deaths", "Your play sessions in this game, newest first, and what happened in each. Delete anything counted by mistake."],
  "deaths/games": ["Deaths", "Every game you've counted deaths in, and all of them together."],
  "deaths/settings": ["Deaths", "The in-game counter, keyboard shortcuts and how counting works."],
});
DEFAULT_SUB.deaths = "now";

const TABS = [["now", "Now"], ["bosses", "Bosses"], ["history", "History"], ["games", "Games"], ["settings", "Settings"]];
const CORNERS = [["top-left", "↖"], ["top", "↑"], ["top-right", "↗"], ["bottom-left", "↙"], ["bottom", "↓"], ["bottom-right", "↘"]];
const EV_TEXT = { death: "Death", undo: "Undo", fight_start: "Fight started", win: "Boss defeated", abandon: "Fight abandoned", rename: "Renamed to" };
const SRC_TEXT = { manual: "", hotkey: "shortcut", pad: "controller", save: "save file", screen: "screen" };

const sub = () => S.sub || "now";
const st = () => S.cache.deaths || {};
const cfg = () => S.state.settings.deaths || st().settings || {};
let viewing = null; // a game picked in the Games tab; null = the one in front / last used
let strip = null; // the frames to teach from, once shown: { game, at, frames: [{ t, img }] }
let shareCode = { game: "", code: "" }; // the last code shown, for the game it was made for

async function setDeaths(changes) {
  await setSetting("deaths", { ...cfg(), ...changes });
  render();
}

async function act(what, boss) {
  try {
    await invoke("deaths_act", { what, boss: boss ?? null, game: st().game?.key ?? viewing });
    await Pages.deaths.load();
    render();
  } catch (e) {
    note(String(e), true);
  }
}

function dur(ms) {
  const m = Math.round(ms / 60000);
  if (m < 60) return `${m} min`;
  return `${Math.floor(m / 60)} h ${m % 60} min`;
}

function when(t) {
  const d = new Date(t);
  const today = new Date().toDateString() === d.toDateString();
  return today ? d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" }) : d.toLocaleDateString([], { day: "numeric", month: "short" }) + " " + d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}

function noGame() {
  return h("div", { class: "card empty" },
    h("b", null, "No game yet"),
    h("p", { class: "small dim" }, "Start any game. Deck sees it come to the front and you can count from the tray menu (Death counter), a keyboard shortcut, or this page."));
}

// ------------------------------------------------------------------ Now tab

/** "2:41" under an hour, "1 h 05" over: how long a try lasted. */
function clock(ms) {
  const s = Math.floor((ms || 0) / 1000);
  if (s < 3600) return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
  return `${Math.floor(s / 3600)} h ${String(Math.floor(s / 60) % 60).padStart(2, "0")}`;
}

function fig(value, label, cls) {
  return h("div", { class: "dc-fig" + (cls ? " " + cls : "") }, h("b", null, value), h("span", null, label));
}

/** Share a picture of the game's numbers (or one boss's): copy it, or save it as a file. */
function shareMenu(anchor, boss) {
  const game = st().game?.key;
  popMenu(anchor, [
    ["Copy the picture", async () => {
      try { await invoke("deaths_share", { game, boss: boss ?? null, save: false }); note("Picture copied. Paste it anywhere."); } catch (e) { note(String(e), true); }
    }],
    ["Save the picture…", async () => {
      try { if (await invoke("deaths_share", { game, boss: boss ?? null, save: true })) note("Picture saved."); } catch (e) { note(String(e), true); }
    }],
  ]);
}

function fightControls(g) {
  const f = g.open;
  if (f) {
    const rename = h("input", { type: "text", value: f.boss, "aria-label": "Boss name", list: "deaths-names" });
    return h("div", { class: "dc-row" },
      h("button", { class: "btn", onclick: () => act("win") }, I("check"), "Boss defeated"),
      h("button", { class: "btn", onclick: () => act("abandon") }, "Abandon fight"),
      h("span", { class: "grow" }),
      rename,
      h("button", { class: "btn sm", onclick: () => act("rename", rename.value) }, "Rename"));
  }
  const name = h("input", { type: "text", placeholder: "Boss name", "aria-label": "Boss name", list: "deaths-names", onkeydown: (e) => { if (e.key === "Enter") act("start", name.value); } });
  return h("div", null,
    h("div", { class: "dc-row" }, name, h("button", { class: "btn", onclick: () => act("start", name.value) }, I("play"), "Start boss fight")),
    (g.names || []).length ? h("div", { class: "dc-chips" }, h("span", { class: "small dim" }, "Again:"),
      g.names.slice(0, 8).map((n) => h("button", { class: "chip", onclick: () => act("start", n) }, n))) : null);
}

function sessionTiles(g) {
  const s = g.session;
  if (!s) return null;
  return h("div", { class: "dc-tiles" },
    h("div", { class: "card dc-tile" }, fig(s.deaths, s.live ? "this session" : "last session", "red")),
    h("div", { class: "card dc-tile" }, fig((s.estimated ? "about " : "") + dur(s.ms), s.live ? "playing so far" : "played")),
    h("div", { class: "card dc-tile" }, fig(s.pace_ms ? clock(s.pace_ms) : "–", "a try lasts")),
    h("div", { class: "card dc-tile" }, fig(s.deadliest || "–", "deadliest boss")));
}

function recordsRow(g) {
  const r = g.records;
  if (!r || !g.total) return null;
  const items = [
    r.longest_try_ms ? ["Longest try", clock(r.longest_try_ms) + (r.longest_try_boss ? ` · ${r.longest_try_boss}` : "")] : null,
    r.fewest_to_win ? ["Quickest win", `${r.fewest_to_win[0]} · ${r.fewest_to_win[1]} death${r.fewest_to_win[1] === 1 ? "" : "s"}`] : null,
    r.deadliest ? ["Deadliest boss", `${r.deadliest[0]} · ${r.deadliest[1]}`] : null,
    r.most_in_session ? ["Most in a session", String(r.most_in_session)] : null,
  ].filter(Boolean);
  return h("div", { class: "card dc-records" },
    h("div", { class: "dc-records-head" }, h("b", { class: "grow" }, "Records"),
      h("button", { class: "btn sm", "aria-label": "Share this game's numbers", onclick: (e) => shareMenu(e.currentTarget, null) }, I("send"), "Share")),
    h("div", { class: "dc-record-list" }, items.map(([k, v]) => h("div", { class: "dc-record" }, h("span", { class: "small dim" }, k), h("span", null, v)))));
}

function nowTab(x) {
  const g = x.game;
  if (!g) return noGame();
  const inFront = x.front && x.front.key === g.key;
  const f = g.open;
  const bossDeaths = f ? ((g.bosses || []).find((b) => b.boss === f.boss)?.deaths ?? f.deaths) : 0;
  return h("div", null,
    h("datalist", { id: "deaths-names" }, (g.names || []).map((n) => h("option", { value: n }))),
    h("div", { class: "card ov-strip" + (g.off ? "" : " on") },
      h("label", { class: "switch" }, h("input", { type: "checkbox", checked: !g.off, "aria-label": "Count deaths in this game", onchange: (e) => act(e.target.checked ? "on" : "off") }), h("i")),
      h("div", { class: "grow" },
        h("div", { class: "ov-title" }, g.name),
        h("div", { class: "small dim" }, g.off ? "Not counted (turned off for this game)." : inFront ? "In front now." : "The last game you played."))),
    g.off ? null : h("div", { class: "card dc-hero" + (f ? " on" : "") },
      h("div", { class: "dc-hero-top" },
        h("div", { class: "grow dc-hero-name" }, h("div", { class: "small dim" }, f ? "Fighting" : "No fight open"), h("div", { class: "dc-boss" }, f ? f.boss : "Deaths between fights count toward the game")),
        f ? h("div", { class: "dc-big red", "aria-label": `${bossDeaths} deaths to this boss` }, bossDeaths, h("span", null, "this boss")) : null,
        h("div", { class: "dc-big", "aria-label": `${g.total} deaths in this game` }, g.total, h("span", null, "this game"))),
      h("div", { class: "dc-row" },
        h("button", { class: "btn primary", onclick: () => act("death") }, "+1 death"),
        h("button", { class: "btn", disabled: !g.can_undo, onclick: () => act("undo") }, I("undo"), "Undo"),
        h("span", { class: "grow" }),
        (g.tries || []).length > 1 ? h("div", { class: "dc-tries" }, h("span", { class: "small dim" }, "Last tries"),
          bars(g.tries, { label: `How long your last ${g.tries.length} tries lasted; the newest is on the right` })) : null),
      fightControls(g)),
    g.off ? null : sessionTiles(g),
    g.off ? null : recordsRow(g),
    g.off ? null : teachCard(g),
    g.off ? null : taughtCard(g),
    g.off ? null : h("p", { class: "small dim pad-foot" }, autoText(g, x.settings || {})));
}

// ------------------------------------------------------------------ teaching a death screen

async function teach(what, extra) {
  const g = st().game;
  try {
    const out = await invoke("deaths_teach", { what, game: g?.key ?? null, pick: extra?.pick ?? null, code: extra?.code ?? null });
    await Pages.deaths.load();
    return out;
  } catch (e) {
    note(String(e), true);
    return null;
  }
}

async function showStrip(g) {
  try {
    const s = await invoke("deaths_strip", { game: g.key });
    strip = s && s.frames && s.frames.length ? { game: g.key, ...s } : null;
    if (!strip) note("Those pictures are gone; count the next death and try again.", true);
  } catch (e) {
    note(String(e), true);
  }
  render();
}

function teachCard(g) {
  const open = strip && strip.game === g.key;
  if (!g.strip && !open) return null;
  if (!open) {
    return h("div", { class: "card dc-teach" },
      h("div", { class: "grow" },
        h("b", null, "Did Deck miss that death?"),
        h("p", { class: "small dim" }, "Show it the death screen once and it counts it by itself from then on. Useful when the death screen has no words, like Sekiro's.")),
      h("button", { class: "btn", onclick: () => showStrip(g) }, "Show the last 10 seconds"));
  }
  const pick = async (i) => {
    const ok = await teach("teach", { pick: i });
    if (ok !== null) {
      strip = null;
      note("Learned. Deck will count this death screen by itself from now on.");
    }
    render();
  };
  return h("div", { class: "card dc-teach open" },
    h("div", { class: "dc-teach-head" },
      h("b", null, "Click the picture where the death screen is clearest"),
      h("button", { class: "btn sm", onclick: () => { strip = null; render(); } }, "Cancel")),
    h("div", { class: "dc-strip" }, strip.frames.map((f, i) => h("button", {
      class: "dc-shot",
      "aria-label": `Picture ${i + 1}, ${Math.max(0, Math.round((strip.at - f.t) / 1000))} seconds before you counted the death`,
      onclick: () => pick(i),
    }, h("img", { src: f.img, alt: "" }), h("span", null, `${Math.max(0, Math.round((strip.at - f.t) / 1000))} s`)))),
    h("p", { class: "small dim" }, "These pictures stay in memory only. Deck learns the part that changed from the game around it."));
}

async function submit(g) {
  const code = await teach("submit");
  if (!code) return;
  try {
    await navigator.clipboard.writeText(code);
    note("Copied. Paste it into the form that just opened and press Submit.");
  } catch {
    shareCode = { game: g.key, code };
    note("Couldn't copy it; copy the code shown below, then paste it into the form.", true);
  }
  invoke("open_url", { url: st().library?.issue_url, keep: true });
  render();
}

function taughtCard(g) {
  const taught = g.taught || [];
  if (!taught.length) {
    return g.library ? h("div", { class: "card dc-teach" }, h("div", { class: "grow" },
      h("b", null, `${g.library} death screen${g.library === 1 ? "" : "s"} for ${g.name} from the shared library`),
      h("p", { class: "small dim" }, "People who play this game taught Deck its death screen, so it counts it by itself. Wrong counts? Turn the library off in Settings."))) : null;
  }
  const share = async () => {
    const code = await teach("share");
    if (!code) return;
    shareCode = { game: st().game?.key ?? "", code };
    try {
      await navigator.clipboard.writeText(code);
      note("Copied. Send it to a friend; they add it on the Games tab.");
    } catch {
      note("Couldn't copy it; select the code below and copy it.", true);
    }
    render();
  };
  return h("div", { class: "card" },
    h("div", { class: "dc-teach-head" },
      h("b", null, `Death screens Deck learned for ${g.name}: ${taught.length} of 3`),
      h("div", { class: "dc-row" },
        g.can_submit && cfg().community_library !== false ? h("button", { class: "btn sm primary", title: "Adds it to the shared library for everyone, after the maintainer has looked at it", onclick: () => submit(g) }, "Submit to the shared library") : null,
        h("button", { class: "btn sm", onclick: share }, "Share with a friend"))),
    taught.map((made, i) => h("div", { class: "dc-taught" },
      h("span", null, `Learned ${when(made)}`),
      h("button", { class: "btn sm", "aria-label": `Forget the death screen learned ${when(made)}`, onclick: async () => { if (await teach("forget", { pick: i }) !== null) render(); } }, "Forget"))),
    g.library ? h("p", { class: "small dim" }, `The shared library also has ${g.library} for this game.`) : null,
    shareCode.code && shareCode.game === g.key ? h("textarea", { class: "dc-code", readonly: true, rows: 2, "aria-label": "Share code", onfocus: (e) => e.target.select() }, shareCode.code) : null);
}

function importCard() {
  let input;
  const add = async () => {
    const code = (input.value || "").trim();
    if (!code) return;
    const name = await teach("import", { code });
    if (name !== null) {
      input.value = "";
      note(`Added. Deck now knows ${name}'s death screen.`);
      render();
    }
  };
  input = h("input", { type: "text", placeholder: "deck-death-screen:…", "aria-label": "Share code from a friend", onkeydown: (e) => { if (e.key === "Enter") add(); } });
  return h("div", { class: "card dc-teach" },
    h("div", { class: "grow" },
      h("b", null, "Add a death screen a friend taught"),
      h("div", { class: "dc-row" }, input, h("button", { class: "btn", onclick: add }, "Add"))));
}

function presetText(p) {
  const parts = [];
  if (p.save) parts.push("deaths from its save file");
  if (p.words && p.words.length) parts.push("death screen: " + p.words.map((w) => `"${w}"`).join(", "));
  return parts.join(" · ") + (p.verified ? "" : " · not checked in the game yet");
}

const AUTO_TEXT = {
  save: "Counting automatically from the game's save file.",
  save_missing: "This game's save file wasn't found yet, so deaths aren't counted from it. Count with the buttons, the tray or your shortcuts.",
  screen: "Watching the screen for the game's death screen.",
  screen_blind: "Deck can't see this game's screen (it shows black), so deaths aren't counted automatically. Borderless windowed usually fixes it.",
  screen_failed: "Watching the screen didn't start here (Windows' screen capture or text recognition is missing). Count with the buttons, the tray or your shortcuts.",
  screen_lang: "Watching the screen, but Windows can't read English text here, so English death screens like \"YOU DIED\" may be missed. Add English in Windows Settings → Time & language → Language & region.",
};

function autoText(g, s) {
  if (g.auto && AUTO_TEXT[g.auto]) return AUTO_TEXT[g.auto];
  if (g.save && s.auto_save) return "Deaths will be read from the save file while the game runs.";
  if (s.auto_screen) return "While the game is in front, Deck watches for death screens (\"YOU DIED\", \"GAME OVER\" and similar). Anything it misses: the buttons, the tray or your shortcuts.";
  return "Automatic counting is off. Count with the buttons here, the tray menu or the shortcuts in Settings.";
}

// ------------------------------------------------------------------ Bosses tab

function bossesTab(x) {
  const g = x.game;
  if (!g) return noGame();
  const list = [...(g.bosses || [])].sort((a, b) => (b.fighting - a.fighting) || (b.last - a.last));
  if (!list.length) return h("div", { class: "card empty" }, "No boss fights in ", h("b", null, g.name), " yet. Start one on the Now tab or from the tray.");
  return h("div", { class: "dc-boss-grid" }, list.map(bossCard));
}

const avg = (a) => a.reduce((x, y) => x + y, 0) / Math.max(1, a.length);

function noteField(b) {
  const ta = h("textarea", { class: "dc-note", rows: 2, maxlength: 500, placeholder: "A note for this boss: a strategy, a reminder", "aria-label": `Note for ${b.boss}` });
  ta.value = b.note || "";
  const save = async () => {
    if (ta.value.trim() === (b.note || "")) return;
    try { await invoke("deaths_note", { game: st().game.key, boss: b.boss, text: ta.value }); b.note = ta.value.trim(); note("Note saved."); } catch (e) { note(String(e), true); }
  };
  ta.addEventListener("blur", save);
  ta.addEventListener("keydown", (e) => { if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); ta.blur(); } });
  return ta;
}

function bossCard(b) {
  const beaten = b.wins > 0;
  const t = (b.tries_ms || []).filter((x) => x > 0);
  const learning = t.length >= 10 && avg(t.slice(-5)) > avg(t.slice(0, 5)) * 1.1;
  return h("div", { class: "card dc-bcard" + (b.fighting ? " on" : "") },
    h("div", { class: "dc-bhead" },
      h("b", { class: "grow dc-bname" }, b.boss),
      b.fighting ? h("span", { class: "chip sm-chip2 live" }, "fighting") : beaten ? h("span", { class: "chip sm-chip2 win" }, "beaten") : null,
      h("button", { class: "icon-btn sm", title: "Share", "aria-label": `Share ${b.boss}`, onclick: (e) => shareMenu(e.currentTarget, b.boss) }, I("send"))),
    h("div", { class: "dc-figs" },
      fig(b.deaths, "deaths", b.fighting ? "red" : ""),
      fig(dur(b.time_ms), "on this boss"),
      fig(b.best_try_ms ? clock(b.best_try_ms) : "–", "longest try"),
      beaten && b.won_on ? fig(`try ${b.won_on}`, "beaten on") : fig(b.attempts, "tries")),
    t.length > 1 ? line(t, { color: beaten && !b.fighting ? "var(--good)" : "var(--bad)", label: `How long each of your ${t.length} tries on ${b.boss} lasted, oldest first` }) : null,
    h("div", { class: "small dim" }, beaten && b.won_at ? `Beaten ${when(b.won_at)}.` : learning ? "Your tries are getting longer: you're learning it." : t.length > 1 ? "Each point is a try; higher lasted longer." : ""),
    noteField(b));
}

// ------------------------------------------------------------------ History tab

const opened = new Set(); // sessions opened by hand (their start)
const closed = new Set(); // the newest session folded by hand
const unfolded = new Set(); // runs of deaths shown in full (their first line)

function dayName(t) {
  const d = new Date(t), now = new Date();
  const days = Math.round((new Date(now.toDateString()) - new Date(d.toDateString())) / 86400000);
  if (days === 0) return "Today";
  if (days === 1) return "Yesterday";
  if (days < 7) return d.toLocaleDateString([], { weekday: "long" });
  return d.toLocaleDateString([], { day: "numeric", month: "short", year: d.getFullYear() === now.getFullYear() ? undefined : "numeric" });
}
const hm = (t) => new Date(t).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
const SRC_WORD = { manual: "counted here", hotkey: "your shortcut", pad: "controller", save: "save file", screen: "seen on screen" };

function eventText(e) {
  const boss = e.boss || "";
  switch (e.ev) {
    case "death": return boss ? `Died to ${boss}` : "Died";
    case "win": return `Beat ${boss || "the boss"} after ${e.deaths} death${e.deaths === 1 ? "" : "s"}${e.ms ? `, ${dur(e.ms)}` : ""}`;
    case "abandon": return `Gave up on ${boss || "the fight"}`;
    case "fight_start": return `Started ${boss}`;
    case "undo": return "Took back a death";
    case "rename": return `Renamed the fight to ${boss}`;
    default: return e.ev;
  }
}

function eventLine(e) {
  return h("div", { class: "dc-ev " + e.ev },
    h("span", { class: "dc-ev-t" }, hm(e.t)),
    h("span", { class: "grow" }, eventText(e), SRC_WORD[e.src] && e.ev === "death" ? h("span", { class: "dim" }, " · " + SRC_WORD[e.src]) : null),
    h("button", { class: "icon-btn sm", title: "More", "aria-label": `Options for: ${eventText(e)} at ${hm(e.t)}`, onclick: (ev) => popMenu(ev.currentTarget, [
      ["Delete this line…", async () => { if (await confirmBox("Delete this line?", "Counts and stats are worked out again without it.", "Delete")) act(`delete:${e.i}`); }, true],
    ]) }, I("more")));
}

/** The timeline, with long runs of deaths folded ("+ 19 more"). */
function timeline(events) {
  const out = [];
  for (let i = 0; i < events.length;) {
    let j = i;
    while (j < events.length && events[j].ev === "death" && events[j].boss === events[i].boss) j++;
    const run = j - i;
    if (run > 5 && !unfolded.has(events[i].i)) {
      out.push(...events.slice(i, i + 3).map(eventLine));
      const first = events[i].i;
      out.push(h("button", { class: "dc-more", onclick: () => { unfolded.add(first); render(); } }, `+ ${run - 3} more deaths`));
      i = j;
    } else {
      out.push(eventLine(events[i]));
      i++;
    }
  }
  return out;
}

function sessionCard(s, newest) {
  const open = opened.has(s.start) || (newest && !closed.has(s.start));
  const toggle = () => { if (open) { opened.delete(s.start); if (newest) closed.add(s.start); } else { opened.add(s.start); closed.delete(s.start); } render(); };
  return h("div", { class: "card dc-session" + (open ? " open" : "") },
    h("button", { class: "dc-session-head", "aria-expanded": open ? "true" : "false", onclick: toggle },
      h("div", { class: "grow" },
        h("b", null, dayName(s.start)),
        h("span", { class: "dim" }, ` · ${hm(s.start)} – ${hm(s.end)} · ${s.estimated ? "about " : ""}${dur(s.end - s.start)}`)),
      h("span", { class: "dc-session-deaths" }, `${s.deaths} death${s.deaths === 1 ? "" : "s"}`),
      (s.beaten || []).slice(0, 2).map((b) => h("span", { class: "chip sm-chip2 win" }, "beat " + b)),
      (s.per15 || []).length > 1 ? perTime(s.per15, { label: "Deaths over this session" }) : null,
      I("chevron-down", "dc-chev")),
    open ? h("div", { class: "dc-session-body" },
      (s.fought || []).length ? h("div", { class: "small dim" }, "Fought: " + s.fought.join(", ")) : null,
      (s.events || []).length ? timeline(s.events) : h("div", { class: "small dim" }, "Nothing counted in this session.")) : null);
}

function historyTab(x) {
  const g = x.game;
  if (!g) return noGame();
  const list = g.sessions || [];
  if (!list.length) return h("div", { class: "card empty" }, "Nothing counted in ", h("b", null, g.name), " yet.");
  return h("div", { class: "dc-sessions" }, list.map((s, i) => sessionCard(s, i === 0)),
    h("p", { class: "small dim pad-foot" }, "Sessions marked \"about\" are from before Deck recorded when you play; they're worked out from when your deaths were counted."));
}

// ------------------------------------------------------------------ Games tab

async function exportCsv(game) {
  try { if (await invoke("deaths_export", { game: game ?? null })) note("Exported."); } catch (e) { note(String(e), true); }
}

function overallCard(o) {
  if (!o || !o.deaths) return null;
  return h("div", { class: "card dc-overall" },
    h("div", { class: "dc-records-head" }, h("b", { class: "grow" }, "All games"),
      h("button", { class: "btn sm", onclick: () => exportCsv(null) }, I("download"), "Export everything")),
    h("div", { class: "dc-figs" }, fig(o.deaths, "deaths", "red"), fig(dur(o.played_ms), "played"), fig(o.beaten, "bosses beaten")),
    h("div", { class: "dc-weeks" }, perTime(o.weeks, { height: 34, label: "Deaths per week over the last 12 weeks" }), h("span", { class: "small dim" }, "Deaths per week, last 12 weeks")),
    h("div", { class: "dc-two" },
      h("div", null, h("div", { class: "small dim" }, "Deadliest games"), (o.games || []).map((g) => h("div", { class: "dc-record" }, h("span", null, g.name), h("span", { class: "dim" }, g.deaths)))),
      h("div", null, h("div", { class: "small dim" }, "Deadliest bosses"), (o.bosses || []).map((b) => h("div", { class: "dc-record" }, h("span", null, b.boss, h("span", { class: "dim" }, " · " + b.game)), h("span", { class: "dim" }, b.deaths))))));
}

function gamesTab(x) {
  const list = x.games || [];
  return h("div", null,
    overallCard(x.overall),
    list.length ? h("div", { class: "settings-card" }, list.map((g) => settingRow(
      h("span", null, g.name, g.fighting ? h("span", { class: "chip sm-chip2" }, "fighting") : null, g.off ? h("span", { class: "chip sm-chip2" }, "off") : null),
      `${g.deaths} death${g.deaths === 1 ? "" : "s"} · last counted ${when(g.last)}`,
      h("div", { class: "dc-row" },
        h("button", { class: "btn sm", "aria-label": `Export ${g.name}`, onclick: () => exportCsv(g.key) }, "Export"),
        h("button", { class: "btn sm", onclick: async () => { viewing = g.key; S.sub = "now"; await Pages.deaths.load(); render(); } }, "Show"))))) : noGame(),
    h("p", { class: "small dim" }, st().library && st().library.games ? `The shared library has death screens for ${st().library.games} game${st().library.games === 1 ? "" : "s"} (${st().library.prints} in all).` : "The shared library has no death screens yet. Teach Deck one and submit it, and it helps everyone."),
    importCard(),
    (x.presets || []).length ? h("div", { class: "section" }, h("h2", null, "Games with presets")) : null,
    (x.presets || []).length ? h("div", { class: "settings-card" }, x.presets.map((p) => settingRow(p.name, presetText(p), null))) : null,
    h("p", { class: "small dim pad-foot" }, `Every game counts, known by its program wherever you got it, and every game is watched for the common death screens ("YOU DIED", "GAME OVER", "WASTED"…). For ${x.catalog || "a few"} games Deck also knows their own death screen or reads their save file; not all of these have been checked in the real game yet. For any other game, teach Deck its death screen.`));
}

// ------------------------------------------------------------------ Settings tab

const CARD_LINES = [
  ["try_no", "Try number", "\"Try 16 next\"."],
  ["try_len", "This try's length", "How long the try you just lost lasted."],
  ["boss_time", "Time on the boss", "All your tries on this boss together."],
  ["session", "This session", "Deaths since you started playing."],
  ["bars", "Recent tries", "A bar per try, taller = lasted longer."],
  ["note", "Highlights", "\"Your longest try yet\", your 100th death."],
  ["my_note", "My boss note", "The note you wrote on the boss's card."],
];

let preview = { key: "", counter: null, card: null };

/** The counter and the card as they'd look, drawn by the same code as in game. */
function previewCard() {
  const key = JSON.stringify(cfg()) + "|" + (S.state.settings.pad?.theme || "");
  if (preview.key !== key) {
    preview.key = key;
    invoke("deaths_preview").then((p) => { preview = { key, counter: p.counter, card: p.card }; render(); }).catch(() => {});
  }
  return h("div", { class: "card dc-preview", "aria-label": "Preview of the in-game counter" },
    h("div", { class: "small dim" }, "Preview"),
    h("div", { class: "dc-preview-scene" },
      preview.counter ? h("img", { src: preview.counter, alt: "The counter while you play" }) : h("span", { class: "small dim" }, "Nothing shows with these settings."),
      preview.card && cfg().card !== false ? h("img", { src: preview.card, alt: "The card after a death" }) : null));
}

function settingsTab() {
  const s = cfg();
  const row = settingRow;
  return h("div", null,
    h("div", { class: "section" }, h("h2", null, "Counting")),
    h("div", { class: "settings-card" },
      row("Death counter", "Off: no tray menu, no in-game counter, no shortcuts.", toggle(!!s.on, (v) => setDeaths({ on: v }))),
      row("+1 death shortcut", "Works in games too. Ctrl, Alt or Win with a key, or an F-key on its own.", hotkeyField(s.hotkey_death || "", "death", (k) => setDeaths({ hotkey_death: k }))),
      row("Undo shortcut", "Takes back the last death.", hotkeyField(s.hotkey_undo || "", "death_undo", (k) => setDeaths({ hotkey_undo: k }))),
      row("Controller combo", "Hold Select (View) + D-pad Down for a second: +1 death. Select + D-pad Up: undo. A quick press never counts. The game sees these buttons too (Select may open its map), so leave this off if that gets in the way.", toggle(!!s.pad_combo, (v) => setDeaths({ pad_combo: v })))),
    h("div", { class: "section" }, h("h2", null, "Automatic counting")),
    h("div", { class: "settings-card" },
      row("Read save files", "For games Deck has a save reader for (Celeste, Dark Souls). Read only: the save is never changed.", toggle(!!s.auto_save, (v) => setDeaths({ auto_save: v }))),
      row("Shared death-screen library", "Death screens other people taught Deck, kept in Deck's public GitHub page. Deck looks for new ones at most once a day (it only downloads; nothing about you is sent). Off: only your own.", toggle(s.community_library !== false, (v) => setDeaths({ community_library: v }))),
      row("Watch for death screens", "While a game is in front, Deck looks at its window a few times a second and reads the middle when the picture settles. Nothing is recorded or saved.", toggle(!!s.auto_screen, (v) => setDeaths({ auto_screen: v })))),
    h("div", { class: "section" }, h("h2", null, "In-game counter")),
    h("div", { class: "dc-counter-settings" },
      h("div", { class: "settings-card" },
        row("Show the counter in games", "Click-through, it never takes focus.", toggle(!!s.pill, (v) => setDeaths({ pill: v }))),
        row("Style", "How it looks while you play.", seg([["skull", "Skull and number"], ["ring", "Ring"], ["huge", "Huge number"]], s.pill_style || "skull", (v) => setDeaths({ pill_style: v }))),
        row("Counts", "In a fight: the boss's deaths, the game's, or both side by side.", seg([["boss", "This boss"], ["game", "The game"], ["both", "Both"]], s.pill_counts || "both", (v) => setDeaths({ pill_counts: v }))),
        row("Outside a fight", "Between bosses.", seg([["game", "The game's total"], ["hide", "Nothing"]], s.pill_outside || "game", (v) => setDeaths({ pill_outside: v }))),
        row("In every game", "Off: only in games you've counted deaths in.", toggle(!!s.pill_always, (v) => setDeaths({ pill_always: v }))),
        row("Corner", "On the game's monitor.", seg(CORNERS, s.pill_corner || "top", (v) => setDeaths({ pill_corner: v }))),
        row("Size", `${Math.round((s.pill_scale || 1) * 100)}%`, h("div", { style: { width: "220px" } }, slider(Math.round((s.pill_scale || 1) * 100), 50, 200, 5, null, (v) => setDeaths({ pill_scale: v / 100 })))),
        row("Opacity", `${Math.round((s.pill_opacity || 0.9) * 100)}%`, h("div", { style: { width: "220px" } }, slider(Math.round((s.pill_opacity || 0.9) * 100), 20, 100, 1, null, (v) => setDeaths({ pill_opacity: v / 100 })))),
        row("Red edges on a death", "The screen's edges glow red for a moment when you die.", toggle(!!s.red_edge, (v) => setDeaths({ red_edge: v }))),
        row("Show in screenshots and recordings", "For streaming. Off: only you see it.", toggle(!!s.pill_in_recordings, (v) => setDeaths({ pill_in_recordings: v })))),
      previewCard()),
    h("div", { class: "section" }, h("h2", null, "The card after a death")),
    h("div", { class: "settings-card" },
      row("Open the card after a death", "Over the death screen for a few seconds, then back to the counter.", toggle(s.card !== false, (v) => setDeaths({ card: v }))),
      s.card !== false ? row("Seconds", `${s.card_secs || 5} s`, h("div", { style: { width: "220px" } }, slider(s.card_secs || 5, 2, 10, 1, null, (v) => setDeaths({ card_secs: v })))) : null,
      s.card !== false ? CARD_LINES.map(([k, title, desc]) => row(title, desc, toggle(!!(s.card_lines || {})[k], (v) => setDeaths({ card_lines: { ...(s.card_lines || {}), [k]: v } })))) : null),
    h("p", { class: "small dim pad-foot" }, "Deck never reads or changes a game's memory and never injects anything, so anti-cheat has nothing to object to. Games in exclusive fullscreen may hide the counter; borderless shows it."));
}

// ------------------------------------------------------------------ the page

Pages.deaths = {
  async load() {
    S.cache.deaths = await invoke("deaths_state", { game: viewing });
  },
  async tick() {
    const before = JSON.stringify(S.cache.deaths);
    await Pages.deaths.load();
    return JSON.stringify(S.cache.deaths) !== before;
  },
  render() {
    const x = st();
    const t = sub();
    const el = h("div", { class: "page-narrow dc" }, tabsHead("deaths", "Deaths", TABS, t));
    if (viewing && x.game && x.front?.key !== viewing) {
      el.append(h("div", { class: "card dc-viewing" }, h("span", { class: "grow small" }, "Showing ", h("b", null, x.game.name), "."),
        h("button", { class: "btn sm", onclick: async () => { viewing = null; await Pages.deaths.load(); render(); } }, "Back to the current game")));
    }
    el.append(t === "bosses" ? bossesTab(x) : t === "history" ? historyTab(x) : t === "games" ? gamesTab(x) : t === "settings" ? settingsTab() : nowTab(x));
    return el;
  },
};
