// Monitor → Deaths: the death counter. Deaths per game and per boss, in any game. Counting
// happens in the backend (tray, hotkeys, this page); the in-game pill is a native window.
import { I, Pages, S, h, invoke, modal, note, seg, setSetting, settingRow, slider, toggle, confirmBox } from "./core.js";
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
let barPick = null; // the picture to show the boss's bar and name on: { game, img, w, h, bars, t, now, sel, name }
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

async function notABoss(f) {
  const text = `Deck forgets this fight (its deaths still count for the game) and won't open a fight for "${f.boss}" in this game again. Undo it in Settings.`;
  if (await confirmBox("Not a boss?", text, "Not a boss", true)) act("not_boss");
}

function fightControls(g) {
  const f = g.open;
  if (f) {
    const rename = h("input", { type: "text", value: f.boss, "aria-label": "Boss name", list: "deaths-names" });
    return h("div", { class: "dc-row" },
      h("button", { class: "btn", onclick: () => act("win") }, I("check"), "Boss defeated"),
      h("button", { class: "btn", onclick: () => act("abandon") }, "Abandon fight"),
      f.seen ? h("button", { class: "btn", onclick: () => notABoss(f) }, "Not a boss") : null,
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

/** The session's numbers, in one card laid out like Records. */
function sessionNow(g) {
  const s = g.session;
  if (!s) return null;
  const items = [
    ["Deaths", s.deaths, "red"],
    [s.live ? "Playing for" : "Played", (s.estimated ? "about " : "") + dur(s.ms)],
    ["A try lasts", s.pace_ms ? clock(s.pace_ms) : "–"],
    ["Deadliest boss", s.deadliest || "–"],
  ];
  return h("div", { class: "card dc-records" },
    h("div", { class: "dc-records-head" }, h("b", { class: "grow" }, s.live ? "This session" : "Last session")),
    h("div", { class: "dc-record-list big" }, items.map(([k, v, cls]) => h("div", { class: "dc-record" + (cls ? " " + cls : "") }, h("span", { class: "small dim" }, k), h("b", null, v)))));
}

function recordsRow(g) {
  const r = g.records;
  if (!r || !g.total) return null;
  const items = [
    r.longest_try_ms ? ["Longest try", clock(r.longest_try_ms) + (r.longest_try_boss ? ` · ${r.longest_try_boss}` : "")] : null,
    r.fewest_to_win ? ["Quickest win", `${r.fewest_to_win[0]} · ${r.fewest_to_win[1]} death${r.fewest_to_win[1] === 1 ? "" : "s"}`] : null,
    r.deadliest ? ["Deadliest boss", `${r.deadliest[0]} · ${r.deadliest[1]} deaths`] : null,
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
  const onOff = h("label", { class: "switch" }, h("input", { type: "checkbox", checked: !g.off, "aria-label": "Count deaths in this game", onchange: (e) => act(e.target.checked ? "on" : "off") }), h("i"));
  const gameLine = h("div", { class: "grow" },
    h("div", { class: "ov-title" }, g.name),
    h("div", { class: "small dim" }, g.off ? "Not counted (turned off for this game)." : inFront ? "In front now" : "The last game you played"));
  if (g.off) return h("div", { class: "dc-now" }, h("div", { class: "card ov-strip" }, onOff, gameLine));
  const num = (n, label, cls) => h("div", { class: "dc-num" + (cls ? " " + cls : ""), "aria-label": `${n} deaths ${label}` }, h("b", null, n), h("span", null, label));
  return h("div", { class: "dc-now" },
    h("datalist", { id: "deaths-names" }, (g.names || []).map((n) => h("option", { value: n }))),
    h("div", { class: "card dc-hero" + (f ? " on" : "") },
      h("div", { class: "dc-hero-game" }, onOff, gameLine,
        (g.tries || []).length > 1 ? h("div", { class: "dc-tries" }, h("span", { class: "small dim" }, "Last tries"),
          bars(g.tries, { label: `How long your last ${g.tries.length} tries lasted; the newest is on the right` })) : null),
      h("div", { class: "dc-hero-main" },
        h("div", { class: "dc-hero-name" },
          h("div", { class: "dc-eyebrow" }, !f ? "No boss fight" : f.in_arena ? "Fighting · its health bar is up" : f.auto ? "Fighting · found on screen" : "Fighting"),
          h("div", { class: "dc-boss" }, f ? f.boss : "Between bosses"),
          f ? null : h("div", { class: "small dim" }, "Deaths count toward the game. Start a fight to count them for a boss too.")),
        h("div", { class: "dc-nums" },
          f ? num(bossDeaths, "this boss", "red") : null,
          num(g.total, "this game"))),
      h("div", { class: "dc-row" },
        h("button", { class: "btn primary", onclick: () => act("death") }, "+1 death"),
        h("button", { class: "btn", disabled: !g.can_undo, onclick: () => act("undo") }, I("undo"), "Undo")),
      h("div", { class: "dc-hero-fight" }, fightControls(g))),
    sessionNow(g),
    recordsRow(g),
    guessCard(g),
    teachCard(g),
    taughtCard(g),
    wonCard(g),
    h("p", { class: "small dim pad-foot" }, autoText(g, x.settings || {})));
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

/** "Is this the death screen?": a screen that showed up before each of the last deaths (or wins)
 *  you counted yourself, found by Deck. Words in any language, a symbol or a flash: it's the picture. */
function guessCard(g) {
  const q = g.guess;
  if (!q) return null;
  const what = q.win ? "victory" : "death";
  const answer = async (yes) => {
    if (await teach(yes ? "guess_yes" : "guess_no") === null) return;
    note(yes ? (q.win ? "Learned. Deck marks the boss beaten when this screen shows." : "Learned. Deck counts this death screen by itself from now on.") : "Okay, Deck won't suggest that one again.");
    render();
  };
  return h("div", { class: "card dc-guess" },
    printPic(q, what),
    h("div", { class: "grow" },
      h("b", null, `Is this the ${what} screen?`),
      h("p", { class: "small dim" }, `It showed up before each of the last ${q.seen} ${q.win ? "wins" : "deaths"} you counted yourself. If it is, Deck counts ${q.win ? "the win" : "it"} by itself from now on.`)),
    h("div", { class: "dc-row" },
      h("button", { class: "btn primary", onclick: () => answer(true) }, "Yes, that's it"),
      h("button", { class: "btn", onclick: () => answer(false) }, "No")));
}

function teachCard(g) {
  const open = strip && strip.game === g.key;
  if ((!g.strip || g.guess) && !open) return null;
  const win = !!g.strip_win;
  if (!open) {
    return h("div", { class: "card dc-teach" },
      h("div", { class: "grow" },
        h("b", null, win ? "Did Deck miss that win?" : "Did Deck miss that death?"),
        h("p", { class: "small dim" }, win
          ? "Show it the victory screen once (\"ENEMY FELLED\", or whatever the game shows) and it marks the boss beaten by itself from then on."
          : "Show it the death screen once and it counts it by itself from then on. Useful when the death screen has no words, like Sekiro's.")),
      h("button", { class: "btn", onclick: () => showStrip(g) }, "Show the last 10 seconds"));
  }
  const pick = async (i) => {
    const ok = await teach("teach", { pick: i });
    if (ok !== null) {
      strip = null;
      note(win ? "Learned. Deck will mark the boss beaten when this screen shows." : "Learned. Deck will count this death screen by itself from now on.");
    }
    render();
  };
  return h("div", { class: "card dc-teach open" },
    h("div", { class: "dc-teach-head" },
      h("b", null, win ? "Click the picture where the victory screen is clearest" : "Click the picture where the death screen is clearest"),
      h("button", { class: "btn sm", onclick: () => { strip = null; render(); } }, "Cancel")),
    h("div", { class: "dc-strip" }, strip.frames.map((f, i) => h("button", {
      class: "dc-shot",
      "aria-label": `Picture ${i + 1}, ${Math.max(0, Math.round((strip.at - f.t) / 1000))} seconds before you counted the ${win ? "win" : "death"}`,
      onclick: () => pick(i),
    }, h("img", { src: f.img, alt: "" }), h("span", null, `${Math.max(0, Math.round((strip.at - f.t) / 1000))} s`)))),
    h("p", { class: "small dim" }, "Deck learns the part that changed from the game around it. It keeps a small copy of the picture you click, to show you which screen it learned; the others stay in memory only."));
}

/** A learned screen: the picture it was taught on with the part Deck looks for outlined; older
 *  ones kept no picture, so the part itself is shown, zoomed in. */
function printPic(t, what) {
  const a = t.area;
  if (t.shot) {
    return h("div", { class: "dc-print" },
      h("img", { src: t.shot, alt: `The ${what} screen Deck learned` }),
      // a little room around it: the outline shouldn't cover the words
      a ? h("span", { class: "dc-print-area", style: boxStyle({ x0: Math.max(0, a.x0 - 20), x1: Math.min(1000, a.x1 + 20), y0: Math.max(0, a.y0 - 35), y1: Math.min(1000, a.y1 + 35) }) }) : null);
  }
  if (!t.img) return null;
  let style = { width: "100%", left: "0", top: "0" };
  if (a) {
    // a cell of room around it, and no closer than 3 times
    const w = Math.min(1, (a.x1 - a.x0) / 1000 + 0.04), hh = Math.min(1, (a.y1 - a.y0) / 1000 + 0.06);
    const k = Math.min(3, 1 / Math.max(w, hh));
    const cx = (a.x0 + a.x1) / 2000, cy = (a.y0 + a.y1) / 2000;
    style = { width: `${k * 100}%`, left: `${(0.5 - cx * k) * 100}%`, top: `${(0.5 - cy * k) * 100}%` };
  }
  return h("div", { class: "dc-print old", title: "Learned before Deck kept a picture of it" },
    h("img", { src: t.img, alt: `The part of the ${what} screen Deck looks for`, style }));
}

function wonCard(g) {
  const won = g.won_taught || [];
  if (!won.length) return null;
  const forget = async (i, t) => {
    if (await confirmBox("Forget this victory screen?", `Deck stops marking bosses beaten in ${g.name} from the screen learned ${when(t.made)}.`, "Forget", true) && await teach("forget_won", { pick: i }) !== null) render();
  };
  return h("div", { class: "card" },
    h("b", null, `Victory screens Deck learned for ${g.name}: ${won.length} of 3`),
    h("p", { class: "small dim" }, "When one shows, the open fight is marked beaten."),
    won.map((t, i) => h("div", { class: "dc-taught" },
      printPic(t, "victory"),
      h("div", { class: "grow" }, `Learned ${when(t.made)}`),
      h("button", { class: "btn sm danger", "aria-label": `Forget the victory screen learned ${when(t.made)}`, onclick: () => forget(i, t) }, "Forget"))));
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

function counted(n, last) {
  return n ? `Counted ${n} death${n === 1 ? "" : "s"}, the last ${when(last)}` : "Hasn't counted a death yet";
}

async function forgetScreen(i, t) {
  const text = `Deck stops counting deaths when it sees this screen.${t.deaths ? ` The ${t.deaths} death${t.deaths === 1 ? "" : "s"} it counted stay; delete wrong ones in History.` : ""}`;
  if (await confirmBox("Forget this death screen?", text, "Forget", true) && await teach("forget", { pick: i }) !== null) render();
}

/** The shared library's death screens for this game: what they counted, and a way to stop using them. */
function libraryLine(g) {
  if (!g.library || cfg().community_library === false) return null;
  const n = `${g.library} death screen${g.library === 1 ? "" : "s"} from the shared library`;
  const flip = async () => {
    if (!g.library_off && !(await confirmBox("Stop using the library's death screens?", `Deck stops counting deaths in ${g.name} from the shared library's death screens (yours still count). Turn them back on here any time.`, "Stop using them", true))) return;
    if (await teach(g.library_off ? "library_on" : "library_off") !== null) render();
  };
  return h("div", { class: "dc-taught" },
    h("div", { class: "grow" },
      h("div", null, g.library_off ? `${n}: not used for this game` : n),
      h("div", { class: "small dim" }, g.library_off ? "You turned them off." : g.library_deaths ? `Counted ${g.library_deaths} death${g.library_deaths === 1 ? "" : "s"}` : "People who play this game taught Deck its death screen.")),
    h("button", { class: "btn sm" + (g.library_off ? "" : " danger"), onclick: flip }, g.library_off ? "Use them again" : "Don't use them"));
}

function taughtCard(g) {
  const taught = g.taught || [];
  if (!taught.length) {
    const lib = libraryLine(g);
    return lib ? h("div", { class: "card" }, h("b", null, `Death screens for ${g.name}`), lib) : null;
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
    h("p", { class: "small dim" }, "Counted a death that wasn't one? See which screen counted last, forget it, and teach the real death screen again."),
    taught.map((t, i) => h("div", { class: "dc-taught" },
      printPic(t, "death"),
      h("div", { class: "grow" },
        h("div", null, `Learned ${when(t.made)}`),
        h("div", { class: "small dim" }, counted(t.deaths, t.last))),
      h("button", { class: "btn sm danger", "aria-label": `Forget the death screen learned ${when(t.made)}`, onclick: () => forgetScreen(i, t) }, "Forget"))),
    libraryLine(g),
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
  if (g.auto === "screen" && s.auto_boss !== false) return "Watching the screen for the game's death screen and for boss health bars.";
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
  return h("div", null,
    list.length ? h("div", { class: "dc-boss-grid" }, list.map(bossCard))
      : h("div", { class: "card empty" }, "No boss fights in ", h("b", null, g.name), " yet. Start one on the Now tab or from the tray."),
    barCard(g, cfg()));
}

async function showBars(g) {
  try {
    const p = await invoke("deaths_bar", { what: "picture", game: g.key });
    barPick = { game: g.key, missing: !p, ...(p || {}), sel: null, name: null };
  } catch (e) {
    note(String(e), true);
  }
  render();
}

async function forgetSpot(g, what) {
  try {
    await invoke("deaths_bar", { what, game: g.key });
    await Pages.deaths.load();
  } catch (e) {
    note(String(e), true);
  }
  render();
}

async function saveSpots(g) {
  const p = barPick;
  try {
    if (p.sel !== null) await invoke("deaths_bar", { what: "pick", game: g.key, bar: p.sel });
    if (p.name) await invoke("deaths_bar", { what: "name", game: g.key, area: p.name });
    barPick = null;
    note(p.name ? "Learned. Deck reads the boss's name only inside your box in this game." : "Learned. Deck looks for the boss's health bar there in this game.");
    await Pages.deaths.load();
  } catch (e) {
    note(String(e), true);
  }
  render();
}

/** A box (thousandths of the picture) above or below the picked bar, where most games put the name. */
function nameNextToBar(b, above) {
  const tall = Math.max((b.y1 - b.y0) * 4, 60);
  const x0 = Math.max(0, b.x0 - 20);
  return above ? { x0, x1: b.x1, y0: Math.max(0, b.y0 - tall), y1: b.y0 } : { x0, x1: b.x1, y0: b.y1, y1: Math.min(1000, b.y1 + tall) };
}

const boxStyle = (r) => ({ left: `${r.x0 / 10}%`, top: `${r.y0 / 10}%`, width: `${(r.x1 - r.x0) / 10}%`, height: `${(r.y1 - r.y0) / 10}%` });

/** The picture: the bar-like strips to click, and a box drawn by dragging around the boss's name. */
function barPicture(g) {
  const p = barPick;
  const box = h("div", { class: "dc-namebox", hidden: !p.name, style: p.name ? boxStyle(p.name) : {} });
  const at = (wrap, e) => {
    const r = wrap.getBoundingClientRect();
    const c = (v, lo, len) => Math.round(Math.min(1000, Math.max(0, ((v - lo) / len) * 1000)));
    return [c(e.clientX, r.left, r.width), c(e.clientY, r.top, r.height)];
  };
  let from = null;
  const wrap = h("div", {
    class: "dc-big",
    onpointerdown: (e) => {
      if (e.target.closest(".dc-barpick")) return;
      e.preventDefault();
      wrap.setPointerCapture(e.pointerId);
      from = at(wrap, e);
    },
    onpointermove: (e) => {
      if (!from) return;
      const [x, y] = at(wrap, e);
      const r = { x0: Math.min(from[0], x), x1: Math.max(from[0], x), y0: Math.min(from[1], y), y1: Math.max(from[1], y) };
      Object.assign(box.style, boxStyle(r));
      box.hidden = false;
      p.name = r;
    },
    onpointerup: () => {
      from = null;
      if (p.name && (p.name.x1 - p.name.x0 < 20 || p.name.y1 - p.name.y0 < 8)) p.name = null;
      render();
    },
  },
  h("img", { src: p.img, alt: "The game a moment before you switched to Deck", draggable: "false" }),
  p.bars.map((b, j) => h("button", {
    class: "dc-barpick" + (p.sel === j ? " sel" : ""),
    style: { left: `${b.x0 / 10}%`, width: `${(b.x1 - b.x0) / 10}%`, top: `calc(${(b.y0 + b.y1) / 20}% - 12px)` },
    "aria-label": `Bar ${j + 1}${p.sel === j ? ", picked" : ""}`,
    "aria-pressed": p.sel === j ? "true" : "false",
    onclick: () => { p.sel = p.sel === j ? null : j; render(); },
  })),
  box);
  return wrap;
}

/** Where the game's boss bar and the boss's name are: found by themselves, from the catalogue, or shown once. */
function barCard(g, s) {
  if (!s.auto_screen || s.auto_boss === false || g.kind !== "boss") return null;
  if (barPick && barPick.game === g.key) {
    const p = barPick;
    const cancel = h("button", { class: "btn sm", onclick: () => { barPick = null; render(); } }, "Cancel");
    if (p.missing) {
      return h("div", { class: "card dc-teach open" },
        h("div", { class: "dc-teach-head" }, h("b", null, "No picture with a bar yet"), cancel),
        h("p", { class: "small dim" }, "Go back to a boss fight while its health bar is on screen, then switch here and try again."));
    }
    const picked = p.sel !== null ? p.bars[p.sel] : null;
    return h("div", { class: "card dc-teach open" },
      h("div", { class: "dc-teach-head" },
        h("b", null, "Click the boss's health bar, then drag a box around its name"),
        cancel),
      barPicture(g),
      h("div", { class: "dc-row" },
        h("button", { class: "btn sm", disabled: !picked, onclick: () => { p.name = nameNextToBar(picked, true); render(); } }, "Name is above the bar"),
        h("button", { class: "btn sm", disabled: !picked, onclick: () => { p.name = nameNextToBar(picked, false); render(); } }, "Name is below it"),
        p.name ? h("button", { class: "btn sm", onclick: () => { p.name = null; render(); } }, "Clear the box") : null,
        h("span", { class: "grow" }),
        h("button", { class: "btn primary", disabled: p.sel === null && !p.name, onclick: () => saveSpots(g) }, "Save")),
      h("p", { class: "small dim" }, "Outlined: everything shaped like a bar. Your own health bar is one too; pick the boss's. The box only needs to cover the name, not the bar. This picture stays in memory only."));
  }
  const parts = [g.bar_spot ? "You showed Deck where this game's boss bar is." : g.bar_preset ? "Deck knows where this game's boss bar is." : "Deck finds a boss's health bar by itself when it's centred at the top or the bottom of the screen.",
    g.name_spot ? "It reads the boss's name inside the box you drew." : "It reads the boss's name next to the bar."];
  return h("div", { class: "card dc-teach" },
    h("div", { class: "grow" },
      h("b", null, "The boss's health bar and name"),
      h("p", { class: "small dim" }, parts.join(" ") + " Wrong bar or misread names? During a boss fight, switch here and show Deck where they are.")),
    h("div", { class: "dc-row" },
      h("button", { class: "btn", onclick: () => showBars(g) }, "Show where they are"),
      g.bar_spot ? h("button", { class: "btn", onclick: () => forgetSpot(g, "forget") }, "Forget the bar") : null,
      g.name_spot ? h("button", { class: "btn", onclick: () => forgetSpot(g, "forget_name") }, "Forget the name box") : null));
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

/** How alike two names are (0..1): the letters they share in order. */
function likeness(a, b) {
  a = a.toLowerCase(); b = b.toLowerCase();
  const row = new Array(b.length + 1).fill(0);
  for (const x of a) {
    let prev = 0;
    for (let j = 0; j < b.length; j++) {
      const cur = row[j + 1];
      row[j + 1] = x === b[j] ? prev + 1 : Math.max(row[j + 1], row[j]);
      prev = cur;
    }
  }
  return (2 * row[b.length]) / Math.max(1, a.length + b.length);
}

/** "Same boss as…": a misread name ("aldii") folded into the boss it was, the most alike first. */
function mergeMenu(anchor, b) {
  const others = (st().game?.bosses || []).filter((o) => o.boss !== b.boss).sort((x, y) => likeness(b.boss, y.boss) - likeness(b.boss, x.boss)).slice(0, 6);
  if (!others.length) { note("There's no other boss in this game to merge it into."); return; }
  popMenu(anchor, others.map((o) => [`Same boss as ${o.boss}`, async () => {
    if (await confirmBox(`Merge into ${o.boss}?`, `Every death, try and win counted for "${b.boss}" moves to ${o.boss}, as if Deck had read the name right.`, "Merge")) act(`merge:${b.boss}`, o.boss);
  }]));
}

function bossCard(b) {
  const beaten = b.wins > 0;
  const t = (b.tries_ms || []).filter((x) => x > 0);
  const learning = t.length >= 10 && avg(t.slice(-5)) > avg(t.slice(0, 5)) * 1.1;
  return h("div", { class: "card dc-bcard" + (b.fighting ? " on" : "") },
    h("div", { class: "dc-bhead" },
      h("b", { class: "grow dc-bname" }, b.boss),
      b.fighting ? h("span", { class: "chip sm-chip2 live" }, "fighting") : beaten ? h("span", { class: "chip sm-chip2 good" }, "beaten") : null,
      h("button", { class: "icon-btn sm", title: "Share", "aria-label": `Share ${b.boss}`, onclick: (e) => shareMenu(e.currentTarget, b.boss) }, I("send")),
      h("button", { class: "icon-btn sm", title: "Same boss as…", "aria-label": `Options for ${b.boss}`, onclick: (e) => mergeMenu(e.currentTarget, b) }, I("more"))),
    h("div", { class: "dc-figs" },
      fig(b.deaths, "deaths", b.fighting ? "red" : ""),
      fig(dur(b.time_ms), "on this boss"),
      fig(b.best_try_ms ? clock(b.best_try_ms) : "–", "longest try"),
      beaten && b.won_on ? fig(`try ${b.won_on}`, "beaten on") : fig(b.attempts, "tries")),
    t.length > 1 ? line(t, { color: beaten && !b.fighting ? "var(--good)" : "var(--bad)", label: `How long each of your ${t.length} tries on ${b.boss} lasted, oldest first` }) : null,
    b.closest && !beaten ? h("div", { class: "small" }, `Closest: ${b.closest[0] > 1 ? `phase ${b.closest[0]}, ` : ""}${b.closest[1]}% of its health left.`) : null,
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
    case "death": return boss ? `Died to ${boss}${e.hp != null ? ` (${e.hp}% left)` : ""}` : "Died";
    case "win": return `Beat ${boss || "the boss"} ${e.deaths ? `after ${e.deaths} death${e.deaths === 1 ? "" : "s"}` : "without dying"}${e.ms ? `, ${dur(e.ms)}` : ""}`;
    case "abandon": return `Gave up on ${boss || "the fight"}`;
    case "fight_start": return `Started ${boss}`;
    case "undo": return "Took back a death";
    case "rename": return `Renamed the fight to ${boss}`;
    default: return e.ev;
  }
}

function eventLine(e) {
  return h("div", { class: "dc-ev ev-" + e.ev },
    h("span", { class: "dc-ev-t" }, hm(e.t)),
    h("span", { class: "grow" }, eventText(e), SRC_WORD[e.src] && (e.ev === "death" || e.src === "screen") ? h("span", { class: "dim" }, " · " + SRC_WORD[e.src]) : null),
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
      (s.beaten || []).slice(0, 2).map((b) => h("span", { class: "chip sm-chip2 good" }, "beat " + b)),
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
  ["boss_hp", "Boss health left", "How much health the boss had when you died (where Deck saw its bar)."],
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

/** "Reset everything": a red warning, and the button only works once you've typed "reset". */
function resetAll() {
  const games = st().games || [];
  const deaths = games.reduce((n, g) => n + (g.deaths || 0), 0);
  const input = h("input", { type: "text", autocomplete: "off", spellcheck: "false" });
  const body = h("div", { class: "dc-reset" },
    h("div", { class: "dc-alert", role: "alert" },
      I("alert"),
      h("div", null,
        h("b", null, "This wipes every death Deck has counted."),
        h("div", null, `${deaths} death${deaths === 1 ? "" : "s"} in ${games.length} game${games.length === 1 ? "" : "s"}.`))),
    h("p", { class: "small" }, h("b", null, "Gone: "), "counts, fights and bosses, tries, play sessions, records and boss notes, for every game."),
    h("p", { class: "small" }, h("b", null, "Staying: "), "your settings and the death screens Deck learned."),
    h("p", { class: "small dim" }, "Deck can't undo this. A copy of the old history goes into a reset folder in Deck's data folder, if you ever need it."),
    h("label", { class: "field" }, h("span", null, 'Type "reset" to confirm'), input));
  modal("Reset all death counts?", body, (close) => {
    const ok = h("button", { class: "btn danger", disabled: true, onclick: async () => {
      close();
      try {
        await invoke("deaths_reset");
        viewing = null;
        strip = null;
        await Pages.deaths.load();
        note("Death counts reset. Starting from scratch.");
      } catch (e) {
        note(String(e), true);
      }
      render();
    } }, I("trash"), "Reset everything");
    input.addEventListener("input", () => { ok.disabled = input.value.trim().toLowerCase() !== "reset"; });
    input.addEventListener("keydown", (e) => { if (e.key === "Enter" && !ok.disabled) ok.click(); });
    return [h("button", { class: "btn ghost", onclick: close }, "Cancel"), ok];
  });
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
      row("Watch for death screens", "While a game is in front, Deck looks at its window a few times a second and reads the middle when the picture settles. Nothing is recorded or saved.", toggle(!!s.auto_screen, (v) => setDeaths({ auto_screen: v }))),
      s.auto_screen ? row("Watch in", "Games Deck knows: games on its list, games with death screens taught or shared, and games you've counted a death in yourself. Any other game isn't looked at; count its deaths with your shortcut and Deck starts watching it.", seg([["known", "Games Deck knows"], ["all", "Every game"]], s.watch_in || "known", (v) => setDeaths({ watch_in: v }))) : null,
      s.auto_screen ? row("Find boss fights by themselves", "When a boss's health bar comes up (centred, at the top or the bottom), Deck reads the name on it and opens that boss's fight. A try is timed from when the bar shows up, deaths on the way back count for the game, and the bar emptying (or a banner like \"ENEMY FELLED\") is a win. Phases, cutscenes and second bars stay in the same try.", toggle(s.auto_boss !== false, (v) => setDeaths({ auto_boss: v }))) : null,
      s.auto_screen && s.auto_boss !== false && (s.not_bosses || []).length ? row("Not bosses", "Names you marked \"Not a boss\". Remove one to let Deck open fights for it again.",
        h("div", { class: "dc-chips" }, s.not_bosses.map((e) => {
          const name = e.split("\t")[1] || e;
          return h("button", { class: "chip", "aria-label": `Remove ${name} from Not bosses`, onclick: () => setDeaths({ not_bosses: s.not_bosses.filter((x) => x !== e) }) }, name, " ", I("x"));
        }))) : null),
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
        row("Show in screenshots and recordings", "Off: only you see it, but NVIDIA Instant Replay (ShadowPlay) turns itself off while the counter is up, because it takes a hidden window for protected video.", toggle(!!s.pill_in_recordings, (v) => setDeaths({ pill_in_recordings: v })))),
      previewCard()),
    h("div", { class: "section" }, h("h2", null, "The card after a death")),
    h("div", { class: "settings-card" },
      row("Open the card after a death", "Over the death screen for a few seconds, then back to the counter.", toggle(s.card !== false, (v) => setDeaths({ card: v }))),
      s.card !== false ? row("Seconds", `${s.card_secs || 5} s`, h("div", { style: { width: "220px" } }, slider(s.card_secs || 5, 2, 10, 1, null, (v) => setDeaths({ card_secs: v })))) : null,
      s.card !== false ? CARD_LINES.map(([k, title, desc]) => row(title, desc, toggle(!!(s.card_lines || {})[k], (v) => setDeaths({ card_lines: { ...(s.card_lines || {}), [k]: v } })))) : null),
    h("div", { class: "section" }, h("h2", null, "Start over")),
    h("div", { class: "settings-card dc-danger" },
      row("Reset all death counts", "Clears every game's deaths, fights, sessions, records and boss notes. Settings and learned death screens stay.",
        h("button", { class: "btn danger", onclick: resetAll }, I("trash"), "Reset everything…"))),
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
