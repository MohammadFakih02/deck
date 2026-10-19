// Monitor → Deaths: the death counter. Deaths per game and per boss, in any game. Counting
// happens in the backend (tray, hotkeys, this page); the in-game pill is a native window.
import { I, Pages, S, h, invoke, note, seg, setSetting, settingRow, slider, toggle, confirmBox } from "./core.js";
import { HEADS, tabsHead } from "./pages1.js";
import { DEFAULT_SUB, render } from "./app.js";
import { hotkeyField } from "./hw.js";

Object.assign(HEADS, {
  "deaths/now": ["Deaths", "Count your deaths in any game, per boss. The tray menu has the same buttons, so you never have to leave the game for long."],
  "deaths/bosses": ["Deaths", "Every boss you've fought in this game: deaths, attempts, wins and time."],
  "deaths/history": ["Deaths", "Everything counted in this game, newest first. Delete anything that was counted by mistake."],
  "deaths/games": ["Deaths", "Every game you've counted deaths in. Any game works; Deck knows extra things about many more."],
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

function fightCard(g) {
  const f = g.open;
  if (f) {
    const rename = h("input", { type: "text", value: f.boss, "aria-label": "Boss name", list: "deaths-names" });
    return h("div", { class: "card dc-fight on" },
      h("div", { class: "dc-fight-head" },
        h("div", { class: "grow" }, h("div", { class: "small dim" }, "Fighting"), h("div", { class: "dc-boss" }, f.boss)),
        h("div", { class: "dc-count", "aria-label": `${f.deaths} deaths in this fight` }, f.deaths, h("span", null, f.deaths === 1 ? "death" : "deaths"))),
      h("div", { class: "small dim" }, `Since ${when(f.started)}. Deaths on the way back to the boss count too, until you mark it defeated or abandon it.`),
      h("div", { class: "dc-row" },
        h("button", { class: "btn primary", onclick: () => act("win") }, I("check"), "Boss defeated"),
        h("button", { class: "btn", onclick: () => act("abandon") }, "Abandon fight"),
        h("span", { class: "grow" }),
        rename,
        h("button", { class: "btn sm", onclick: () => act("rename", rename.value) }, "Rename")));
  }
  const name = h("input", { type: "text", placeholder: "Boss name", "aria-label": "Boss name", list: "deaths-names", onkeydown: (e) => { if (e.key === "Enter") act("start", name.value); } });
  return h("div", { class: "card dc-fight" },
    h("div", { class: "small dim" }, "No fight open. Start one and every death counts toward that boss until you beat it."),
    h("div", { class: "dc-row" }, name, h("button", { class: "btn primary", onclick: () => act("start", name.value) }, I("play"), "Start boss fight")),
    (g.names || []).length ? h("div", { class: "dc-chips" }, h("span", { class: "small dim" }, "Again:"),
      g.names.slice(0, 8).map((n) => h("button", { class: "chip", onclick: () => act("start", n) }, n))) : null);
}

function nowTab(x) {
  const g = x.game;
  if (!g) return noGame();
  const inFront = x.front && x.front.key === g.key;
  return h("div", null,
    h("datalist", { id: "deaths-names" }, (g.names || []).map((n) => h("option", { value: n }))),
    h("div", { class: "card ov-strip" + (g.off ? "" : " on") },
      h("label", { class: "switch" }, h("input", { type: "checkbox", checked: !g.off, "aria-label": "Count deaths in this game", onchange: (e) => act(e.target.checked ? "on" : "off") }), h("i")),
      h("div", { class: "grow" },
        h("div", { class: "ov-title" }, g.name),
        h("div", { class: "small dim" }, g.off ? "Not counted (turned off for this game)." : inFront ? "In front now." : "The last game you played."))),
    h("div", { class: "card dc-total" },
      h("div", { class: "dc-count big" }, g.total, h("span", null, g.total === 1 ? "death in this game" : "deaths in this game")),
      h("div", { class: "dc-row" },
        h("button", { class: "btn primary", disabled: g.off, onclick: () => act("death") }, "+1 death"),
        h("button", { class: "btn", disabled: g.off || !g.can_undo, onclick: () => act("undo") }, I("undo"), "Undo last death"))),
    g.off ? null : fightCard(g),
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
  const list = g.bosses || [];
  if (!list.length) return h("div", { class: "card empty" }, "No boss fights in ", h("b", null, g.name), " yet. Start one on the Now tab or from the tray.");
  return h("div", { class: "card dc-table-wrap" },
    h("table", { class: "dc-table" },
      h("thead", null, h("tr", null, ["Boss", "Deaths", "Attempts", "Wins", "Deaths to first kill", "Time"].map((c) => h("th", null, c)))),
      h("tbody", null, list.map((b) => h("tr", { class: b.fighting ? "on" : "" },
        h("td", null, b.boss, b.fighting ? h("span", { class: "chip sm-chip2" }, "fighting") : null),
        h("td", null, b.deaths), h("td", null, b.attempts), h("td", null, b.wins),
        h("td", null, b.deaths_to_first_win ?? "–"), h("td", null, dur(b.time_ms)))))));
}

// ------------------------------------------------------------------ History tab

function historyTab(x) {
  const g = x.game;
  if (!g) return noGame();
  const list = g.history || [];
  if (!list.length) return h("div", { class: "card empty" }, "Nothing counted in ", h("b", null, g.name), " yet.");
  return h("div", { class: "settings-card" }, list.map((e) => settingRow(
    `${EV_TEXT[e.ev] || e.ev}${e.boss ? " " + e.boss : ""}`,
    `${when(e.t)}${SRC_TEXT[e.src] ? " · " + SRC_TEXT[e.src] : ""}`,
    h("button", { class: "btn sm", "aria-label": "Delete this line", onclick: async () => {
      if (await confirmBox("Delete this line?", "Counts and stats are worked out again without it.", "Delete")) act(`delete:${e.i}`);
    } }, "Delete"))));
}

// ------------------------------------------------------------------ Games tab

function gamesTab(x) {
  const list = x.games || [];
  return h("div", null,
    list.length ? h("div", { class: "settings-card" }, list.map((g) => settingRow(
      h("span", null, g.name, g.fighting ? h("span", { class: "chip sm-chip2" }, "fighting") : null, g.off ? h("span", { class: "chip sm-chip2" }, "off") : null),
      `${g.deaths} death${g.deaths === 1 ? "" : "s"} · last counted ${when(g.last)}`,
      h("button", { class: "btn sm", onclick: async () => { viewing = g.key; S.sub = "now"; await Pages.deaths.load(); render(); } }, "Show")))) : noGame(),
    h("p", { class: "small dim" }, st().library && st().library.games ? `The shared library has death screens for ${st().library.games} game${st().library.games === 1 ? "" : "s"} (${st().library.prints} in all).` : "The shared library has no death screens yet. Teach Deck one and submit it, and it helps everyone."),
    importCard(),
    (x.presets || []).length ? h("div", { class: "section" }, h("h2", null, "Games with presets")) : null,
    (x.presets || []).length ? h("div", { class: "settings-card" }, x.presets.map((p) => settingRow(p.name, presetText(p), null))) : null,
    h("p", { class: "small dim pad-foot" }, `Every game counts, known by its program wherever you got it, and every game is watched for the common death screens ("YOU DIED", "GAME OVER", "WASTED"…). For ${x.catalog || "a few"} games Deck also knows their own death screen or reads their save file; not all of these have been checked in the real game yet. For any other game, teach Deck its death screen.`));
}

// ------------------------------------------------------------------ Settings tab

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
    h("div", { class: "settings-card" },
      row("Show the counter in games", "A small pill with the boss and its deaths. Click-through, it never takes focus.", toggle(!!s.pill, (v) => setDeaths({ pill: v }))),
      row("In every game", "Off: only in games you've counted deaths in.", toggle(!!s.pill_always, (v) => setDeaths({ pill_always: v }))),
      row("Corner", "On the game's monitor.", seg(CORNERS, s.pill_corner || "top", (v) => setDeaths({ pill_corner: v }))),
      row("Size", `${Math.round((s.pill_scale || 1) * 100)}%`, h("div", { style: { width: "220px" } }, slider(Math.round((s.pill_scale || 1) * 100), 50, 200, 5, null, (v) => setDeaths({ pill_scale: v / 100 })))),
      row("Opacity", `${Math.round((s.pill_opacity || 0.9) * 100)}%`, h("div", { style: { width: "220px" } }, slider(Math.round((s.pill_opacity || 0.9) * 100), 20, 100, 1, null, (v) => setDeaths({ pill_opacity: v / 100 })))),
      row("Show in screenshots and recordings", "For streaming. Off: only you see it.", toggle(!!s.pill_in_recordings, (v) => setDeaths({ pill_in_recordings: v })))),
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
