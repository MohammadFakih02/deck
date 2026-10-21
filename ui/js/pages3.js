// Pages: Tools, Phone, Settings.
import { icon } from "./icons.js";
import { I, Pages, S, ago, appIcon, confirmBox, fmtBytes, h, helpTip, invoke, modal, note, promptBox, run, seg, setSetting, settingRow, slider, toggle } from "./core.js";
import { TILE_INFO, tool } from "./home.js";
import { copyAnswer } from "./search.js";
import { head, partHead, showPart } from "./pages1.js";
import { indexView } from "./pages2.js";
import { hotkeyField } from "./hw.js";
import { row } from "./pcs.js";
import { render, resetSide } from "./app.js";
import { releaseNotes } from "./relnotes.js";


// ------------------------------------------------------------------ tools

Pages.tools = {
  async load() {},
  render() {
    const s = S.state.settings;
    // one page: screen tools, then locked files, then Windows panels ("tools/locks" scrolls to its part)
    const el = h("div", { class: "tl-page" }, head("Tools", "Screen tools, the program holding a file open, and Windows' own panels."));
    const part = (id, ic, title, sub) => partHead("tools", id, ic, title, sub);
    showPart("tools", ["locks", "panels"]);

    el.append(part("screen", "target", "Screen tools", "Pick one, then use it anywhere on your screen."));
    const big = (ic, title, text, fn) => h("button", { class: "tl-tool", onclick: fn },
      h("span", { class: "tl-tool-ic" }, I(ic, "lg")),
      h("span", { class: "tl-tool-t" }, title), h("span", { class: "tl-tool-d" }, text),
      h("span", { class: "tl-tool-go" }, "Start", I("chevron")));
    el.append(h("div", { class: "tl-tools" },
      big("droplet", "Pick a colour", "Click anywhere on screen; the colour code is copied.", () => tool("color")),
      big("type", "Copy text from the screen", "Drag over text in a picture, video or game and it's copied as text.", () => tool("text")),
      big("ruler", "Measure", "Drag a box to measure it in pixels.", () => tool("measure"))));

    // colours
    if ((s.colors || []).length) {
      el.append(h("div", { class: "card tl-colors" },
        h("div", { class: "row" }, h("b", null, "Colours you picked"), h("span", { class: "tl-sub grow" }, "Click to copy the hex code, right-click for rgb()."),
          h("button", { class: "btn sm ghost", onclick: () => setSetting("colors", []).then(render) }, I("trash"), "Clear")),
        h("div", { class: "swatches" }, s.colors.map((c) => {
          const r = parseInt(c.slice(1, 3), 16), g = parseInt(c.slice(3, 5), 16), b = parseInt(c.slice(5, 7), 16);
          return h("button", { class: "swatch", title: `${c}  ·  rgb(${r}, ${g}, ${b})\nClick: copy hex · Right-click: copy rgb`,
            onclick: () => copyAnswer(c), oncontextmenu: (e) => { e.preventDefault(); copyAnswer(`rgb(${r}, ${g}, ${b})`); } },
            h("i", { style: { background: c } }), h("span", null, c));
        }))));
    }
    el.append(part("locks", "lock", "Locked files", "\"The file is open in another program\"? See which one."), locksView());
    el.append(part("panels", "windows", "Windows panels", "Windows' own settings and tools, one click away."), panelsView());
    return el;
  },
};

export function locksView() {
  const lk = S.cache.lock;
  const pickers = h("div", { class: "row" },
    h("button", { class: "btn primary", onclick: () => lockCheck(false) }, I("file"), "Pick a file"),
    h("button", { class: "btn", onclick: () => lockCheck(true) }, I("folder"), "Pick a folder"));
  const state = (ic, cls, title, text) => h("div", { class: "tl-lock-state" }, h("span", { class: "tl-lock-ic " + cls }, I(ic, "lg")),
    h("div", { class: "grow" }, h("div", { class: "tl-lock-t" }, title), h("div", { class: "tl-sub" }, text)), pickers);
  if (!lk) return h("div", { class: "card tl-lock" }, state("lock", "", "Can't move or delete something?", "Pick the file or folder and Deck shows which program is holding it open."));
  const name = lk.path.split(/[\\/]/).filter(Boolean).pop() || lk.path;
  if (lk.holders.length === 0) return h("div", { class: "card tl-lock" }, state("check", "ok", `${name} is free`, `Nothing is using ${lk.path} right now. You can move or delete it.`));
  return h("div", { class: "card tl-lock" },
    state("alert", "busy", `${name} is in use by ${lk.holders.length} program${lk.holders.length > 1 ? "s" : ""}`, lk.path),
    h("div", { class: "list tl-lock-list" }, lk.holders.map((x) => h("div", { class: "item" }, appIcon("file", x.exe, x.name), h("div", { class: "grow" }, h("div", { class: "t" }, x.name), h("div", { class: "s" }, `${x.exe || ""} · PID ${x.pid}`)),
      h("button", { class: "btn sm danger", onclick: async () => { if (await confirmBox(`End ${x.name}?`, "Unsaved work in it will be lost.", "End it", true)) { await invoke("kill_pid", { pid: x.pid }); lockCheck(null, lk.path); } } }, "End it")))));
}

export function panelsView() {
  const sys = (ic, label, what, url) => h("button", { class: "tl-panel", onclick: () => url ? invoke("open_url", { url }) : invoke("open_system", { what }) },
    h("span", { class: "tl-panel-ic" }, I(ic)), h("span", { class: "tl-panel-t" }, label), h("span", { class: "tl-panel-go" }, I("external")));
  return h("div", { class: "tl-panels" },
    sys("activity", "Task Manager", "taskmgr"), sys("volume", "Sound devices", "sound"), sys("wifi", "Network adapters", "network"),
    sys("power", "Power options", "power"), sys("trash", "Uninstall programs", "programs"), sys("list", "Environment variables", "envvars"),
    sys("terminal", "Terminal", "terminal"), sys("download", "Downloads", "downloads"), sys("image", "Snipping Tool", null, "ms-screenclip:"), sys("gear", "Windows Settings", null, "ms-settings:"));
}

export async function lockCheck(folder, path) {
  const p = path || await invoke("pick", { folder, title: folder ? "Which folder?" : "Which file?" });
  if (!p) return;
  try {
    const holders = await invoke("who_uses", { path: p });
    S.cache.lock = { path: p, holders };
  } catch (e) { note(String(e), true); }
  render();
}

// ------------------------------------------------------------------ phone

/** The "Away" tab. The phone reaches the PC only once it's on the user's Tailscale network (else
 *  the code loads forever), so until a phone is there it first shows how to put it there: a code
 *  that opens the Tailscale app in the phone's store, and the account to sign in with. */
export function awayView(r) {
  const phones = (r.away_peers || []).filter((p) => /android|ios/i.test(p.os || ""));
  const account = r.away_account ? h("b", { style: { userSelect: "text", wordBreak: "break-all" } }, r.away_account) : "the same account as this PC";
  if (!phones.length && !S.cache.awaySkipSetup) return [
    h("div", { class: "chip warn" }, I("alert", "sm"), "Your phone isn't on Tailscale yet"),
    r.away_get_qr ? h("div", { class: "qr", html: r.away_get_qr }) : null,
    h("ol", { class: "dim", style: { margin: 0, paddingLeft: "18px", textAlign: "left", lineHeight: 1.6 } },
      h("li", null, "Scan this to get Tailscale on the phone"),
      h("li", null, "Sign in with ", account),
      h("li", null, "Turn it on (allow the VPN), then come back here for the connect code")),
    h("button", { class: "btn sm ghost", onclick: () => { S.cache.awaySkipSetup = true; render(); } }, "My phone is set up: show the code")];
  return [
    h("div", { class: "qr", html: r.away_qr }),
    h("div", { class: "dim" }, "Works anywhere while the phone is connected in the Tailscale app, signed in with ", account, "."),
    phones.length ? h("div", { class: "row wrap", style: { justifyContent: "center" } }, phones.map((p) => h("span", { class: "chip " + (p.online ? "good" : ""), title: p.online ? "Connected to Tailscale" : "Not connected to Tailscale right now" }, I("smartphone", "sm"), `${p.name}${p.online ? "" : " · offline"}`))) : null];
}

/** The Screen view's bitrate: Auto (measures the connection), a fixed limit, or one typed in. */
export function bitrateSelect(k) {
  const opts = [[0, "Auto"], [30000, "30 Mb/s"], [15000, "15 Mb/s"], [8000, "8 Mb/s"], [4000, "4 Mb/s"], [2000, "2 Mb/s"], [1000, "1 Mb/s"], [500, "0.5 Mb/s"]];
  if (!opts.some(([v]) => v === k)) opts.push([k, `${+(k / 1000).toFixed(2)} Mb/s`]);
  return h("select", { "aria-label": "Bitrate", onchange: async (e) => {
    let v = e.target.value;
    if (v === "custom") {
      const typed = await promptBox("Bitrate", "Mb/s, for example 1.5 (empty for Auto)", "", "Set");
      const t = typed == null ? null : String(typed).trim().replace(",", ".");
      const x = t ? parseFloat(t) : 0;
      // Cancel, or something that isn't a number: keep the bitrate (the redraw puts the box back)
      if (t == null || Number.isNaN(x)) return render();
      v = x > 0 ? Math.round(Math.min(200, Math.max(0.3, x)) * 1000) : 0;
    }
    setSetting("remote_screen_kbps", +v).then(render);
  } }, ...opts.map(([v, l]) => h("option", { value: String(v), selected: v === k }, l)), h("option", { value: "custom" }, "Type…"));
}

/** Phone page: phones connected now, at home and away (a phone counts once, whatever it has open). */
export function phonesConnected(r) {
  const home = r.local || 0, away = r.online || 0;
  const n = (k) => `${k} phone${k > 1 ? "s" : ""}`;
  if (!home && !away) return h("div", { class: "row" }, h("span", { class: "chip" }, I("smartphone"), "No phone connected"));
  return h("div", { class: "row wrap", style: { justifyContent: "center" } },
    home ? h("span", { class: "chip good", title: "On this PC's network (Wi-Fi or cable)" }, I("smartphone"), `${n(home)} at home`) : null,
    away ? h("span", { class: "chip good", title: "Away from home, through Tailscale" }, I("globe"), `${n(away)} away`) : null);
}

/** Phone page: how many phones may connect at home and away, and whether one side shuts the other out. */
export function connLimitsCard(s) {
  const pick = (key) => {
    const cur = s[key] || 0, opts = [[0, "No limit"], [1, "1"], [2, "2"], [3, "3"], [5, "5"]];
    if (!opts.some(([v]) => v === cur)) opts.push([cur, String(cur)]); // a value set elsewhere (settings file)
    return h("select", { onchange: (e) => setSetting(key, +e.target.value).then(render) },
      opts.map(([v, l]) => h("option", { value: String(v), selected: cur === v }, l)));
  };
  const row = (label, el) => h("div", { class: "rd-row" }, h("span", { class: "dim" }, label), el);
  const sw = (label, key) => h("label", { class: "row", style: { justifyContent: "space-between", gap: "10px" } }, h("span", null, label), toggle(!!s[key], (v) => setSetting(key, v).then(render)));
  return h("details", { class: "card col fold-card", style: { gap: "10px" } },
    h("summary", { style: { fontWeight: 700, cursor: "pointer" } }, "Who can connect"),
    row("Phones at home at a time", pick("remote_max_local")),
    row("Phones away at a time", pick("remote_max_online")),
    sw("A phone at home shuts out phones away", "remote_local_blocks_online"),
    sw("A phone away shuts out phones at home", "remote_online_blocks_local"),
    h("div", { class: "faint", style: { fontSize: "12px" } }, "A phone counts once, whatever it has open (the remote, the Screen view, its sound). Away means through Tailscale or any address outside this network. Applies to new connections; a phone that is turned away sees why and tries again every 10 seconds."));
}

/** Phone page: the PC's screen in the phone remote (view, or view and control). */
export function screenCard(r) {
  const s = S.state.settings;
  const mode = s.remote_screen || "off", q = s.remote_screen_quality || "sharp";
  const seg = (val, opts, key) => h("div", { class: "seg" }, opts.map(([v, l]) => h("button", { class: val === v ? "on" : "", onclick: () => setSetting(key, v).then(render) }, l)));
  const n = r.screen_viewers || 0;
  return h("div", { class: "card col" },
    h("div", { class: "tile-head" }, I("smartphone"), h("span", null, "Screen on your phone"), h("span", { class: "grow" }),
      n ? h("span", { class: "chip good" }, I("eye", "sm"), `${n} watching`) : null),
    h("div", { class: "dim" }, "See this PC's screen in the phone remote, at home or away (Tailscale). With control, taps click and the phone's keyboard types on the PC."),
    h("div", { class: "rd-row" }, h("span", { class: "dim" }, "Allow"), seg(mode, [["off", "Off"], ["view", "View only"], ["control", "View and control"]], "remote_screen")),
    mode !== "off" ? h("div", { class: "rd-row" }, h("span", { class: "dim" }, "Picture size"), seg(q, [["sharp", "Full"], ["balanced", "Medium"], ["fast", "Small"]], "remote_screen_quality")) : null,
    mode !== "off" ? h("div", { class: "rd-row" }, h("span", { class: "dim" }, "Smoothness"), seg([15, 24, 60].includes(s.remote_screen_fps) ? s.remote_screen_fps : 30, [[15, "15"], [24, "24"], [30, "30"], [60, "60 fps"]], "remote_screen_fps")) : null,
    mode !== "off" ? h("div", { class: "rd-row" }, h("span", { class: "dim" }, "Bitrate"), bitrateSelect(s.remote_screen_kbps || 0)) : null,
    mode !== "off" ? h("div", { class: "rd-row" }, h("span", { class: "dim" }, "When it's tight"), seg(s.remote_screen_priority === "sharp" ? "sharp" : "smooth", [["smooth", "Smooth (blurrier)"], ["sharp", "Sharp (fewer frames)"]], "remote_screen_priority")) : null,
    mode !== "off" ? h("div", { class: "rd-row" }, h("span", { class: "dim" }, "Video"), seg(s.remote_screen_codec === "jpeg" ? "jpeg" : "auto", [["auto", "H.264 (light on data)"], ["jpeg", "JPEG"]], "remote_screen_codec")) : null,
    mode !== "off" ? h("div", { class: "faint", style: { fontSize: "12px" } }, "Video: H.264 needs far less data (a 1 Mb/s line can show 30 fps), so it is the default; JPEG is the plain fallback. Picture size: Full keeps small text readable. Auto bitrate measures the connection and follows it (down to slow DSL); a fixed one never goes over. Nothing is sent while the screen doesn't change.") : null);
}

/** Phone page: the phone as this PC's speaker and mic (the mic through VB-CABLE). */
export function spkMicCard(r) {
  const s = S.state.settings;
  const mic = S.cache.phoneMic;
  const busy = S.cache.phoneMicBusy;
  const act = async (what, ok) => {
    S.cache.phoneMicBusy = what; render();
    try { S.cache.phoneMic = await run("phone_mic_do", { what }, ok); } catch { /* run() showed it */ }
    S.cache.phoneMicBusy = null; render();
  };
  const sw = (label, sub, key, then) => h("label", { class: "rd-row" }, h("span", { class: "grow" }, h("div", null, label), sub ? h("div", { class: "faint", style: { fontSize: "12px" } }, sub) : null),
    toggle(!!s[key], (v) => setSetting(key, v).then(() => (then ? then(v) : null)).then(render)));
  const n = r.mics || 0;
  let cable = null;
  if (s.remote_mic) {
    const st = mic?.state;
    cable = !mic ? h("div", { class: "faint" }, "Checking VB-CABLE…")
      : st === "ready" && mic.cable_is_speakers ? h("div", { class: "col", style: { gap: "6px" } },
        h("div", { class: "dim" }, I("alert", "sm"), " VB-CABLE became this PC's speakers (its setup does that), so the PC's sound goes into the cable and you hear nothing. Pick your speakers again:"),
        h("div", { class: "row wrap" }, (mic.outputs || []).map((o) => h("button", { class: "btn sm", onclick: async () => { await run("set_output", { id: o.id }, `Sound plays on ${o.name}`); S.cache.phoneMic = await invoke("phone_mic_state"); render(); } }, I("volume", "sm"), o.name))))
      : st === "ready" ? h("div", { class: "row wrap" }, h("span", { class: "chip good" }, I("check", "sm"), "VB-CABLE ready"), h("span", { class: "faint", style: { fontSize: "12px" } }, "Apps hear the phone on “CABLE Output”."))
      : st === "off" ? h("div", { class: "col", style: { gap: "6px" } }, h("div", { class: "dim" }, "VB-CABLE is installed but turned off in Device Manager."),
        h("div", { class: "row wrap" },
          h("button", { class: "btn" + (mic.enable_failed ? "" : " primary"), disabled: !!busy || !mic.admin, title: mic.admin ? "" : "Needs Deck's admin rights", onclick: () => act("enable", "VB-CABLE is on") }, I("power"), busy === "enable" ? "Turning on…" : "Turn VB-CABLE on"),
          h("button", { class: "btn" + (mic.enable_failed ? " primary" : ""), disabled: !!busy, onclick: () => act("get", "VB-CABLE's setup is open: click Install Driver, then restart the PC") }, I("download"), busy === "get" ? "Downloading…" : "Get the current VB-CABLE")))
      : st === "restart" ? h("div", { class: "dim" }, "Restart the PC to finish installing VB-CABLE.")
      : h("div", { class: "col", style: { gap: "6px" } },
        h("div", { class: "dim" }, "The phone mic needs VB-CABLE, VB-Audio's free virtual cable: Windows lists a microphone only when a driver makes one. Deck downloads it from vb-audio.com and opens its setup: click ", h("b", null, "Install Driver"), ", then restart the PC."),
        h("div", { class: "row" }, h("button", { class: "btn primary", disabled: !!busy, onclick: () => act("get", "VB-CABLE's setup is open") }, I("download"), busy === "get" ? "Downloading…" : "Get VB-CABLE"),
          h("button", { class: "btn ghost", onclick: () => invoke("open_url", { url: "https://vb-audio.com/Cable/", keep: true }) }, I("external"), "vb-audio.com")));
  }
  return h("div", { class: "card col" },
    h("div", { class: "tile-head" }, I("smartphone"), h("span", null, "Speaker and mic on your phone"), h("span", { class: "grow" }),
      n ? h("span", { class: "chip good" }, I("mic", "sm"), `${n} talking`) : null),
    h("div", { class: "dim" }, "Turn your phone into this PC's speaker or microphone, in the phone remote. Over Wi-Fi: about a tenth of a second late, fine for music, videos and calls."),
    sw("Phone as speaker", "The phone plays this PC's sound. It can turn the PC's own speakers off while it plays; they come back when it stops.", "remote_speaker"),
    sw("Phone as mic", "The phone's mic becomes a mic on this PC. Phones allow the mic only on a secure page: the phone remote opens Deck's https page for it, and the phone warns once about Deck's own certificate.", "remote_mic",
      (v) => v ? invoke("phone_mic_state").then((m) => { S.cache.phoneMic = m; }) : null),
    cable,
    s.remote_mic ? sw("Make it the default mic while the phone talks", "Discord, games and calls use the phone without changing their settings; your mic comes back when the phone stops.", "remote_mic_default") : null,
    spkMicHelp());
}

/** Phone page: how to use the phone as speaker and mic, and what to do when something doesn't work. */
function spkMicHelp() {
  const ol = (...items) => h("ol", { class: "dim", style: { margin: "4px 0 0", paddingLeft: "18px", lineHeight: 1.7 } }, items.map((x) => h("li", null, x)));
  const ul = (...items) => h("ul", { class: "dim", style: { margin: "4px 0 0", paddingLeft: "18px", lineHeight: 1.7 } }, items.map((x) => h("li", null, x)));
  const b = (t) => h("b", null, t);
  return h("details", { class: "fold-card" },
    h("summary", { style: { cursor: "pointer", fontWeight: 600 } }, "How to use it"),
    h("div", { style: { fontWeight: 600, marginTop: "8px" } }, "Speaker"),
    ol(["Turn on ", b("Phone as speaker"), " here."],
      ["On the phone, open the phone remote (scan the code on the left) and tap ", b("Phone as speaker"), " in the ", b("Speaker and mic"), " card."],
      [b("PC speakers off while playing"), " (on the phone) mutes this PC while the phone plays; the PC's sound comes back a few seconds after the phone stops."]),
    h("div", { style: { fontWeight: 600, marginTop: "8px" } }, "Microphone"),
    ol(["Turn on ", b("Phone as mic"), " here and set up VB-CABLE when Deck asks (install, then restart the PC once)."],
      ["On the phone, tap ", b("Mic: open the secure page"), ". The phone warns that the connection isn't private: tap ", b("Advanced"), ", then ", b("Proceed"), " (it's Deck's own certificate; it asks once)."],
      ["Tap ", b("Phone as mic"), " and allow the microphone. The bar moves when you talk."],
      ["In Discord, the game or the call app pick ", b("CABLE Output"), " as the microphone, or turn on ", b("Make it the default mic"), " above."]),
    h("div", { style: { fontWeight: 600, marginTop: "8px" } }, "If something doesn't work"),
    ul([b("No sound on the phone:"), " tap anywhere on the phone page (phones hold sound until you touch the page), and check the phone's own volume."],
      [b("The PC is silent:"), " Windows may have made VB-CABLE the speakers; Deck shows it above with buttons to pick yours again."],
      [b("The bar moves but nobody hears you:"), " the app isn't using CABLE Output; pick it in the app's voice settings, or turn on the default-mic switch and rejoin the call."],
      [b("Echo:"), " use headphones on the phone when it is the speaker and the mic at the same time, or keep its volume low."],
      [b("It stops when the screen turns off:"), " keep the phone remote open in front; Deck keeps the screen on while the phone plays or talks (on the secure page)."],
      [b("Choppy or late:"), " stay close to the router, or plug the phone in and use USB tethering; about 0.1 s of delay is normal."],
      [b("\"Not secure\" every time:"), " Deck made a new certificate (a new address); accept it once more."]));
}

/** Phone page: remote desktop through RustDesk, with Tailscale for away from home. */
export function rdeskCard() {
  const d = S.cache.rdesk;
  const busy = S.cache.rdeskBusy;
  const act = async (what, args = {}, ok) => {
    S.cache.rdeskBusy = what; render();
    try { S.cache.rdesk = await run("rdesk_do", { what, ...args }, ok); } catch { /* run() showed it */ }
    S.cache.rdeskBusy = null; render();
  };
  const el = h("div", { class: "card col" }, h("div", { class: "tile-head" }, I("monitor"), h("span", null, "Remote desktop app (RustDesk)")));
  if (!d) return (el.append(h("div", { class: "faint" }, "Checking…")), el);
  if (!d.installed) {
    el.append(
      h("div", { class: "dim" }, "See this PC's screen and use its mouse and keyboard from your phone or another computer, at home or away. Uses RustDesk (free, open source); Deck installs and sets it up."),
      h("div", { class: "row" }, h("button", { class: "btn primary", disabled: !!busy, onclick: () => act("install", {}, "RustDesk is installed. Set a password next.") }, I("download"), busy === "install" ? "Installing…" : "Install RustDesk")));
    return el;
  }
  const ts = d.tailscale || {};
  const row = (label, ...rest) => h("div", { class: "rd-row" }, h("span", { class: "dim" }, label), ...rest);
  const value = (v) => [h("b", { class: "num grow", style: { userSelect: "text" } }, v), h("button", { class: "icon-btn sm", title: "Copy", "aria-label": "Copy", onclick: () => copyAnswer(v) }, I("copy", "sm"))];
  const sw = (label, on, what, tip) => h("label", { class: "rd-row", title: tip || "" }, h("span", { class: "grow" }, label), toggle(on, (v) => act(what, { on: v })));
  el.append(sw("Allow remote desktop", d.on, "on", "Off stops RustDesk completely, also after a restart"));
  if (d.on) {
    el.append(
      d.direct ? row("At home", ...value(d.lan || "–")) : null,
      d.direct ? row("Away from home", ...(ts.on && ts.ip ? value(ts.ip)
        : [h("span", { class: "grow faint" }, ts.installed ? "Tailscale isn't signed in" : "Needs Tailscale (free)"),
          h("button", { class: "btn sm", onclick: () => act("tailscale") }, I("external", "sm"), ts.installed ? "Sign in" : "Get it")])) : null,
      d.id ? row("RustDesk ID", ...value(d.id)) : null,
      sw("Connect by address", d.direct, "direct", "Phones connect straight to this PC (home network or Tailscale), not through RustDesk's servers"),
      sw("Hide tray icons", d.tray_hidden, "tray", "RustDesk and Tailscale keep working; their state shows here instead"),
      h("div", { class: "row wrap" },
        h("button", { class: "btn", disabled: !!busy, onclick: async () => { const pw = await promptBox("Remote desktop password", "At least 8 characters. Asked when you connect.", "", "Set"); if (pw) act("password", { text: pw }, "Password set"); } }, I("lock"), "Set password"),
        h("button", { class: "btn ghost", onclick: () => act("open") }, I("external"), "Open RustDesk")),
      h("details", { class: "fold-card" },
        h("summary", { style: { cursor: "pointer", fontWeight: 600 } }, "Connect from your phone"),
        h("ol", { class: "dim", style: { margin: "6px 0 0", paddingLeft: "18px", lineHeight: 1.7 } },
          h("li", null, "Install ", h("b", null, "RustDesk"), " from the Play Store or App Store"),
          h("li", null, "Type the ", h("b", null, "At home"), " address (or ", h("b", null, "Away from home"), " when out) in the box at the top, then connect"),
          h("li", null, "Enter the password you set here"),
          h("li", null, "Away from home the phone needs ", h("b", null, "Tailscale"), " too, signed in with the same account"))));
  }
  return el;
}


Pages.phone = {
  async load() {
    S.cache.remote = await invoke("remote_info");
    // RustDesk's state takes a moment (it asks RustDesk and Tailscale): fill it in when it comes
    invoke("rdesk_status").then((d) => { S.cache.rdesk = d; if (S.page === "phone") render(); });
    if (S.state.settings.remote_mic) invoke("phone_mic_state").then((m) => { S.cache.phoneMic = m; if (S.page === "phone") render(); });
  },
  render() {
    const r = S.cache.remote || {};
    const s = S.state.settings;
    const el = h("div", null, head("Phone", "Control your PC from your phone (media, volume, power, timers) and pass text, links and files between them."));
    const left = h("div", { class: "card col", style: { alignItems: "center", textAlign: "center", gap: "12px" } },
      h("label", { class: "row", style: { alignSelf: "stretch", justifyContent: "space-between" } },
        h("b", null, "Allow phone connection"),
        toggle(s.remote_on, async (v) => { await setSetting("remote_on", v); setTimeout(async () => { S.cache.remote = await invoke("remote_info"); render(); }, 400); })),
      r.enabled && r.away ? h("div", { class: "seg" }, ["home", "away"].map((k) => h("button", { class: (S.cache.phoneAway ? "away" : "home") === k ? "on" : "", onclick: () => { S.cache.phoneAway = k === "away"; render(); } }, k === "home" ? "At home" : "Away (Tailscale)"))) : null,
      ...(r.enabled && S.cache.phoneAway && r.away ? awayView(r) : [
        r.enabled && r.qr ? h("div", { class: "qr", html: r.qr }) : h("div", { class: "empty" }, r.error || "The phone connection is off."),
        r.enabled ? h("div", { class: "dim" }, "Scan with your phone's camera. Phone and PC must be on the same Wi-Fi.") : null]),
      // while it's off there's nothing to connect to or re-code
      r.enabled ? phonesConnected(r) : null,
      !r.enabled ? null : h("button", { class: "btn sm ghost", title: "Old QR codes stop working", onclick: async () => { if (await confirmBox("Make a new code?", "Phones using the old code are disconnected and need to scan again.", "New code")) { S.cache.remote = await invoke("remote_reset_token"); render(); } } }, I("refresh", "sm"), "Make a new code"));
    const install = h("details", { class: "card col fold-card", style: { gap: "8px" } },
      h("summary", { style: { fontWeight: 700, cursor: "pointer" } }, "Put it in your app drawer (Android)"),
      h("ol", { class: "dim", style: { margin: 0, paddingLeft: "18px", lineHeight: 1.7 } },
        h("li", null, "In Chrome on the phone open ", h("b", null, "chrome://flags")),
        h("li", null, "Find ", h("b", null, "Insecure origins treated as secure"), ", add ", h("b", { style: { userSelect: "text" } }, r.origin || "the address"), ", Enabled, Relaunch"),
        h("li", null, "Scan the code, then ", h("b", null, "⋮ → Install app"))),
      h("div", { class: "faint", style: { fontSize: "12px" } }, "Chrome only offers Install for secure pages, and this page is plain HTTP on your home network; that is what the flag is for."));
    // send box
    const ta = h("textarea", { rows: 3, "aria-label": "Text or a link to send to your phone", placeholder: "Text or a link to send to your phone…", style: { width: "100%" } });
    const send = h("div", { class: "card col" },
      h("div", { class: "tile-head" }, I("send"), h("span", null, "Send to your phone")),
      ta,
      h("div", { class: "row wrap" },
        h("button", { class: "btn primary", onclick: async () => { if (!ta.value.trim()) return; await run("share_to_phone", { kind: "text", text: ta.value }, "Sent"); ta.value = ""; S.cache.remote = await invoke("remote_info"); render(); } }, I("send"), "Send text"),
        h("button", { class: "btn", onclick: async () => { await run("share_to_phone", { kind: "file" }); S.cache.remote = await invoke("remote_info"); render(); } }, I("upload"), "Send a file"),
        h("span", { class: "grow" }),
        h("button", { class: "btn ghost", onclick: () => invoke("open_system", { what: "inbox" }) }, I("folder"), "Files from phone")));
    const shared = [...(r.shared || [])].reverse();
    const inbox = h("div", { class: "card col" },
      h("div", { class: "tile-head" }, I("swap"), h("span", null, "Passed between phone and PC")),
      shared.length ? h("div", { class: "list" }, shared.map((x) => h("div", { class: "item" },
        h("div", { class: "ic" }, I(x.kind === "file" ? "file" : x.kind === "link" ? "link" : "note")),
        h("div", { class: "grow", style: { minWidth: 0 } },
          h("div", { class: "t ell" }, x.kind === "file" ? x.name : x.text),
          h("div", { class: "s" }, `${x.from === "phone" ? "From phone" : "To phone"} · ${ago(x.ts)}${x.kind === "file" ? " · " + fmtBytes(x.size) : ""}`)),
        h("div", { class: "row", style: { gap: "2px" } },
          x.kind === "file" && x.from === "phone" ? h("button", { class: "icon-btn sm", title: "Show in folder", onclick: () => invoke("reveal", { path: x.path }) }, I("folder", "sm")) : null,
          x.kind === "link" ? h("button", { class: "icon-btn sm", title: "Open", onclick: () => invoke("open_url", { url: x.text, keep: true }) }, I("external", "sm")) : null,
          x.kind !== "file" ? h("button", { class: "icon-btn sm", title: "Copy", onclick: () => copyAnswer(x.text) }, I("copy", "sm")) : null,
          h("button", { class: "icon-btn sm danger", title: "Remove from list", onclick: async () => { await invoke("shared_remove", { id: x.id }); S.cache.remote = await invoke("remote_info"); render(); } }, I("x", "sm")))))) : h("div", { class: "faint" }, "Nothing yet. Things you send from the phone are copied to the PC clipboard; files land in Downloads\\From phone."));
    el.append(h("div", { style: { display: "grid", gridTemplateColumns: "300px minmax(0, 1fr)", gap: "14px", alignItems: "start" } },
      h("div", { class: "col", style: { gap: "14px" } }, left, r.enabled ? connLimitsCard(s) : null, install),
      h("div", { class: "col", style: { gap: "14px" } }, send, screenCard(r), spkMicCard(r), rdeskCard(), inbox)));
    return el;
  },
  live: true,
  async tick() {
    const r = await invoke("remote_info");
    const changed = JSON.stringify(r) !== JSON.stringify(S.cache.remote);
    S.cache.remote = r;
    return changed && !S.interacting && document.activeElement?.tagName !== "TEXTAREA";
  },
};

// --------------------------------------------------------------- settings

export const ACCENTS = ["#4f8cff", "#7b61ff", "#b35cff", "#ff4f9a", "#ff6b4f", "#ffb020", "#2fbf71", "#1fb5c9"];
export const ENGINES = [["https://www.google.com/search?q=", "Google"], ["https://duckduckgo.com/?q=", "DuckDuckGo"], ["https://www.bing.com/search?q=", "Bing"], ["https://search.brave.com/search?q=", "Brave"], ["https://www.youtube.com/results?search_query=", "YouTube"]];

Pages.settings = {
  async load() {
    S.cache.indexInfo = await invoke("index_info");
    S.cache.update = await invoke("update_state").catch(() => null);
  },
  render() {
    const s = S.state.settings;
    const row = settingRow;
    const set = (k) => (v) => setSetting(k, v).then(() => { if (k === "accent" || k === "theme") applyAccent(); render(); });
    const tab = S.cache.setTab || "general";
    const tabs = seg([["general", "General"], ["home", "Home"], ["clip", "Clipboard & search"], ["profiles", "Profiles"], ["about", "About"]], tab, (v) => { S.cache.setTab = v; render(); });
    const el = h("div", { class: "page-narrow" }, head("Settings", null, tabs));
    let cur = "general";
    const add = (...nodes) => { if (cur === tab) el.append(...nodes); };

    cur = "general";
    add(h("div", { class: "section" }, h("h2", null, "Opening Deck")));
    const panel = s.mode !== "app";
    const modeCard = (id, icon, title, desc) => h("button", { class: "mode-card" + (s.mode === id ? " on" : ""), onclick: () => s.mode !== id && set("mode")(id) },
      I(icon), h("div", null, h("b", null, title), h("div", { class: "dim small" }, desc)));
    add(h("div", { class: "mode-cards" },
      modeCard("panel", "zap", "Quick panel", "Pops up when you rest the mouse in a corner or press your shortcut, and gets out of the way when you click somewhere else."),
      modeCard("app", "windows", "App window", "A normal window with a taskbar button that stays open until you close it. Closing it keeps Deck running in the tray.")));
    add(h("div", { class: "settings-card" },
      row("Keyboard shortcut", panel ? "Opens and closes the panel." : "Brings the window to the front (or minimizes it when it already is).",
        hotkeyField(s.open_hotkey, "open", (k) => set("open_hotkey")(k))),
      panel ? row("Hot corner", "Rest the mouse in this corner of the screen to open or close Deck.",
        seg([["off", "Off"], ["top-left", "↖"], ["top-right", "↗"], ["bottom-left", "↙"], ["bottom-right", "↘"]], s.hot_corner, set("hot_corner"))) : null,
      panel && s.hot_corner !== "off" ? row("How long to rest there", `${s.hot_corner_ms} ms. Longer means fewer accidental opens.`,
        h("div", { style: { width: "200px" } }, slider(s.hot_corner_ms, 0, 1000, 50, null, set("hot_corner_ms")))) : null,
      panel && s.hot_corner !== "off" ? row(h("span", null, "Also over fullscreen games and videos", helpTip("set.fullscreen")), "Off: the corner does nothing while a game or video is fullscreen, so you can't open Deck by accident mid-match.", toggle(s.hot_corner_fullscreen, set("hot_corner_fullscreen"))) : null,
      panel ? row("Hide when I click somewhere else", null, toggle(s.hide_on_blur, set("hide_on_blur"))) : null,
      row("Other ways in", "Click the tray icon, the Start menu entry, or the phone. Right-click the tray icon for quick tools.", null),
      row("Start with Windows", "Starts hidden in the tray.", toggle(s.autostart, set("autostart"))),
      row("Start again after a crash", "If something ends Deck (a crash, a graphics driver update), it comes back by itself, hidden. After three in ten minutes it stops and starts with the fans on the BIOS and no overclock.", toggle(s.restart_after_crash, set("restart_after_crash")))));

    cur = "home";
    add(h("div", { class: "section" }, h("h2", null, "Home tiles")));
    const tiles = (s.tiles || []).filter((t) => TILE_INFO[t]); // a removed tile (Notes) may still be saved
    const allTiles = Object.keys(TILE_INFO);
    const ordered = [...tiles, ...allTiles.filter((t) => !tiles.includes(t))];
    add(h("div", { class: "settings-card" }, ordered.map((t, i) => {
      const on = tiles.includes(t);
      return h("div", { class: "setting", style: { padding: "9px 16px" } },
        h("div", { class: "grow t", style: { opacity: on ? 1 : 0.55 } }, TILE_INFO[t].name),
        on ? h("button", { class: "icon-btn sm", title: "Move up", disabled: tiles.indexOf(t) === 0, onclick: () => { const a = [...tiles]; const k = a.indexOf(t); [a[k - 1], a[k]] = [a[k], a[k - 1]]; set("tiles")(a); } }, I("moveup", "sm")) : null,
        toggle(on, (v) => set("tiles")(v ? [...tiles, t] : tiles.filter((x) => x !== t))));
    })));

    cur = "general";
    add(h("div", { class: "section" }, h("h2", null, "Sidebar")));
    add(h("div", { class: "settings-card" },
      row("Order", "Drag a group's name, or a page inside its group, up or down in the sidebar. Folded groups stay folded after a restart.",
        h("button", { class: "btn sm", disabled: s.nav_order ? null : "", onclick: async () => { await set("nav_order")(null); resetSide(); } }, I("undo", "sm"), "Reset the order")),
      row("Show Claude Code sync", "For people who use Claude Code on more than one PC. It sits under Desktop, and stays there while it's switched on.",
        toggle(!!s.claude_sync_nav || !!s.claude_sync_on, async (v) => { await set("claude_sync_nav")(v); resetSide(); }))));

    cur = "general";
    add(h("div", { class: "section" }, h("h2", null, "Colour")));
    const theme = s.theme || "midnight";
    add(h("div", { class: "settings-card" },
      h("div", { class: "setting", style: { paddingBottom: "0" } }, h("div", { class: "grow" }, h("div", { class: "t" }, "Theme"), h("div", { class: "d" }, "The app's colours. Your accent colour stays the same."))),
      themeCards(theme, (id) => theme !== id && set("theme")(id)),
      ...[["opacity_pinned", "Opacity when pinned", "With the pin on (top right)."], ["opacity_unpinned", "Opacity when not pinned", "With the pin off."]].map(([k, t, d]) => {
        const val = h("b", { class: "num", style: { width: "44px", textAlign: "right" } }, (s[k] || 100) + "%");
        return row(t, d, h("div", { class: "row", style: { gap: "10px", width: "240px" } },
          slider(s[k] || 100, 30, 100, 5, (v) => { val.textContent = v + "%"; S.state.settings[k] = v; applyAccent(); }, (v) => set(k)(v)), val));
      }),
      row("Opening animation", s.low_spec ? "Off while low-spec mode is on." : "How Deck appears when it opens.", h("div", { class: "row", style: { gap: "8px" } },
        h("select", { disabled: !!s.low_spec, onchange: async (e) => { await setSetting("open_anim", e.target.value); applyAccent(); replayOpen(); } },
          OPEN_ANIMS.map(([id, name]) => h("option", { value: id, selected: (s.open_anim || "zoom") === id }, name))),
        h("button", { class: "btn ghost", disabled: !!s.low_spec, onclick: replayOpen }, "Preview")))));
    add(h("div", { class: "settings-card" }, row("Accent", null,
      h("div", { class: "row", style: { gap: "8px" } }, ACCENTS.map((c) => h("button", { title: c, onclick: () => set("accent")(c), style: { width: "26px", height: "26px", borderRadius: "50%", background: c, border: s.accent === c ? "2px solid #fff" : "2px solid transparent", boxShadow: s.accent === c ? `0 0 0 3px ${c}55` : "none" } }))))));

    add(h("div", { class: "section" }, h("h2", null, "Performance")));
    add(h("div", { class: "settings-card" },
      row("Low-spec mode", "For older or slower PCs: no animations or soft shadows, and live readings and graphs update every 3 seconds instead of every second. Everything still works.",
        toggle(!!s.low_spec, async (v) => { await setSetting("low_spec", v); applyAccent(); render(); }))));

    cur = "general";
    add(h("div", { class: "section" }, h("h2", null, "Units")));
    add(h("div", { class: "settings-card" },
      row("Clock speeds", "CPU and GPU clocks on every page, the phone and the in-game overlay. Offsets you type stay in MHz.",
        seg([["MHz", "MHz"], ["GHz", "GHz"]], s.unit_clock, set("unit_clock"))),
      row("Network speed", "Mb/s counts bits (how internet plans are sold); MB/s counts bytes (what downloads show). 8 Mb/s = 1 MB/s.",
        seg([["Mb/s", "Mb/s"], ["MB/s", "MB/s"]], s.unit_net, set("unit_net"))),
      row("Temperature", "Readings convert; limits and fan curves you set stay in °C.",
        seg([["C", "°C"], ["F", "°F"]], s.unit_temp, set("unit_temp")))));

    cur = "clip";
    add(h("div", { class: "section" }, h("h2", null, "Clipboard history")));
    add(h("div", { class: "settings-card" },
      row("Keep a clipboard history", "Stored only on this PC. Passwords copied from password managers are skipped.", toggle(s.clip_on, set("clip_on"))),
      row("Keep images too", null, toggle(s.clip_images, set("clip_images"))),
      row(h("span", null, "Clicking an item pastes it", helpTip("set.clip_paste")), "Deck closes and pastes into the app you were using. Off: it only copies.", toggle(s.clip_paste, set("clip_paste"))),
      row("How many to keep", "Pinned items don't count.", h("select", { onchange: (e) => set("clip_max")(+e.target.value) }, [100, 200, 400, 1000, 3000].map((n) => h("option", { value: n, selected: s.clip_max === n }, `${n} items`))))));

    cur = "clip";
    add(h("div", { class: "section" }, h("h2", null, "Search")));
    add(h("div", { class: "settings-card" },
      row("Web search with", null, h("select", { onchange: (e) => set("web_search")(e.target.value) }, ENGINES.map(([u, n]) => h("option", { value: u, selected: s.web_search === u }, n))))));
    add(h("div", { style: { marginTop: "10px" } }, indexView()));

    cur = "profiles";
    add(h("div", { class: "section" }, h("h2", null, "CPU and GPU profiles")));
    add(h("div", { class: "settings-card" },
      row(h("span", null, "Profile keyboard shortcuts", helpTip("set.hotkeys")), "The keys you give CPU and GPU profiles (and the overlay key). They work in games too.", toggle(s.hotkeys_on, set("hotkeys_on"))),
      row("Say when a profile switches", "A small notification, e.g. \"CPU → Eco\".", toggle(s.profile_toast, set("profile_toast"))),
      row("Repair shortcuts", "Makes every profile's desktop and Start menu shortcut again (after moving Deck or deleting one by mistake).",
        h("button", { class: "btn", onclick: async () => { const errs = await run("shortcuts_repair"); note(errs.length ? errs.join(" · ") : "Shortcuts repaired", errs.length > 0); } }, I("refresh"), "Repair"))));

    cur = "about";
    add(h("div", { class: "section" }, h("h2", null, "About")));
    add(h("div", { class: "settings-card" },
      row(`Deck ${S.state.version}`, "Dashboard, launcher, hardware control and Windows power tools in one tray app." + (S.state.admin ? " Running with admin rights." : ""), null),
      row("Quit Deck", "Stops the hot corner, clipboard history, timers and the phone connection until you start it again. Fans go back to the BIOS.", h("button", { class: "btn danger", onclick: async () => { if (await confirmBox("Quit Deck?", "Timers and shutdown plans stop too.", "Quit", true)) invoke("quit"); } }, I("power"), "Quit"))));
    add(h("div", { class: "section" }, h("h2", null, "Updates")));
    add(updatesCard(s, set, el));
    return el;
  },
};

/** What the Updates card says for each state of the updater (`update_state`). */
export function updateText(u) {
  if (!u) return "Updates are not available right now.";
  switch (u.state) {
    case "checking": return "Looking for a newer version…";
    case "downloading": return `Downloading Deck ${u.latest || ""}…`;
    case "installing": return "Installing. Deck restarts by itself in a moment.";
    case "error": return u.error || "Something went wrong.";
    case "available":
      return `Deck ${u.latest} is available.` + (u.waiting ? " It installs when the game or fullscreen program closes." : u.silent ? "" : " Installing asks for admin rights once.");
    case "uptodate": return `Deck ${u.current} is the newest version.`;
    default: return `Deck ${u.current}. Not checked yet in this run.`;
  }
}

function updatesCard(s, set, page) {
  const u = S.cache.update;
  const busy = u && ["checking", "downloading", "installing"].includes(u.state);
  const refresh = async () => { S.cache.update = await invoke("update_state").catch(() => null); if (page.isConnected) render(); };
  if (busy) setTimeout(() => { if (page.isConnected) refresh(); }, 1500);
  const check = async () => {
    try { S.cache.update = await invoke("update_check"); } catch (e) { note(String(e), true); S.cache.update = await invoke("update_state").catch(() => null); }
    if (page.isConnected) render();
  };
  const install = async () => { try { await invoke("update_install"); } catch (e) { note(String(e), true); } setTimeout(refresh, 600); };
  return h("div", { class: "settings-card" },
    settingRow("Updates", updateText(u), h("div", { class: "row", style: { gap: "8px" } },
      u && u.state === "available" ? h("button", { class: "btn", disabled: busy ? "" : null, onclick: install }, I("download"), "Install " + u.latest) : null,
      h("button", { class: "btn", disabled: busy ? "" : null, onclick: check }, I("refresh"), "Check now"))),
    settingRow("Update by itself", "Looks once a day on GitHub and installs a newer Deck when no game is in front. Settings and profiles stay. Off: Deck only looks when you press Check now.", toggle(s.auto_update, set("auto_update"))),
    u && u.state === "available" && u.notes ? h("div", { class: "setting" }, releaseNotes(u.notes, u.latest, () => invoke("open_url", { url: `https://github.com/MohammadFakih02/deck/releases/tag/v${u.latest}`, keep: true }))) : null);
}

/** The theme cards; `pick` gets the theme id. */
export function themeCards(cur, pick) {
  return h("div", { class: "themes" }, THEMES.map(([id, name, bg, panel, card]) => h("button", { class: "theme" + (cur === id ? " on" : ""), "aria-pressed": cur === id ? "true" : "false", onclick: () => pick(id) },
    h("div", { class: "pv", style: { background: bg, boxShadow: "inset 0 0 0 1px #ffffff14" } }, h("i", { style: { background: panel } }), h("b", null, h("i", { style: { background: card, boxShadow: "inset 0 0 0 1px #ffffff14" } }), h("i", { style: { background: card, boxShadow: "inset 0 0 0 1px #ffffff14" } }))),
    name + (id === "midnight" ? " (default)" : ""))));
}

export function accentDots(cur, pick) {
  return h("div", { class: "row accents", style: { gap: "8px" } }, ACCENTS.map((c) => h("button", { title: c, "aria-label": "Accent " + c, onclick: () => pick(c), style: { width: "26px", height: "26px", borderRadius: "50%", background: c, border: cur === c ? "2px solid #fff" : "2px solid transparent", boxShadow: cur === c ? `0 0 0 3px ${c}55` : "none" } })));
}

/** First start (or the first start after this update): pick a look and how see-through Deck is. Clicks preview at once; any way out counts as done. */
export async function themeWelcome() {
  const st = S.state?.settings;
  if (!st || st.setup_picked || document.querySelector(".theme-welcome")) return;
  const body = h("div", { class: "theme-welcome" });
  const line = (t, d, ctl) => h("div", { class: "row", style: { gap: "12px", marginTop: "12px", flexWrap: "wrap" } },
    h("div", { style: { flex: "1 1 180px", minWidth: 0 } }, h("div", { class: "t", style: { fontWeight: 600 } }, t), h("div", { class: "dim" }, d)), ctl);
  const low = () => !!S.state.settings.low_spec;
  const draw = () => body.replaceChildren(
    h("div", { class: "dim", style: { marginBottom: "10px" } }, "Pick how Deck looks and feels. You can change all of it any time in Settings → General."),
    themeCards(S.state.settings.theme || "midnight", async (id) => { await setSetting("theme", id); applyAccent(); draw(); }),
    h("div", { class: "t", style: { fontWeight: 600 } }, "Accent"),
    accentDots(S.state.settings.accent, async (c) => { await setSetting("accent", c); applyAccent(); draw(); }),
    ...[["opacity_unpinned", "Opacity", "How see-through Deck is."], ["opacity_pinned", "Opacity when pinned", "With the pin on (top right), Deck floats over your other apps."]].map(([k, t, d]) => {
      const v0 = S.state.settings[k] || 100;
      const val = h("b", { class: "num", style: { width: "44px", textAlign: "right" } }, v0 + "%");
      return h("div", { class: "row", style: { gap: "12px", marginTop: "12px", flexWrap: "wrap" } },
        h("div", { style: { flex: "1 1 180px", minWidth: 0 } }, h("div", { class: "t", style: { fontWeight: 600 } }, t), h("div", { class: "dim" }, d)),
        h("div", { class: "row", style: { gap: "10px", width: "240px" } },
          slider(v0, 30, 100, 5, (v) => { val.textContent = v + "%"; S.state.settings[k] = v; applyAccent(); }, (v) => setSetting(k, v)), val));
    }),
    line("Opening animation", low() ? "Off while low-spec mode is on." : "How Deck appears when it opens.", h("div", { class: "row", style: { gap: "8px" } },
      h("select", { disabled: low(), onchange: async (e) => { await setSetting("open_anim", e.target.value); applyAccent(); replayOpen(); } },
        OPEN_ANIMS.map(([id, name]) => h("option", { value: id, selected: (S.state.settings.open_anim || "zoom") === id }, name))),
      h("button", { class: "btn ghost", disabled: low(), onclick: replayOpen }, "Preview"))),
    line("Low-spec mode", "For older or slower PCs: no animations or soft shadows, live readings every 3 seconds.",
      toggle(low(), async (v) => { await setSetting("low_spec", v); applyAccent(); draw(); })));
  draw();
  const done = () => setSetting("setup_picked", true);
  modal("Make Deck yours", body, (close) => [h("button", { class: "btn primary", onclick: () => { close(); done(); } }, "Done")], { wide: true, onDismiss: done });
}

/** Opening animations (app.css, :root[data-anim]). */
export const OPEN_ANIMS = [["zoom", "Zoom (default)"], ["fade", "Fade"], ["pop", "Pop"], ["rise", "Rise"], ["drop", "Drop"], ["flip", "Flip"], ["swirl", "Swirl"], ["blur", "Blur in"], ["none", "None"]];

/** Plays the opening animation again, to see the one picked. */
export function replayOpen() {
  const b = document.body;
  b.classList.remove("shown");
  void b.offsetWidth;
  requestAnimationFrame(() => b.classList.add("shown"));
}

/** The app's themes: [id, name, background, panel, card] (the card colours are app.css's). */
export const THEMES = [
  ["midnight", "Midnight", "#0c0e14", "#10131b", "#141823"],
  ["graphite", "Graphite", "#111214", "#16171a", "#1b1c20"],
  ["oled", "OLED black", "#000000", "#060607", "#0d0d10"],
  ["black", "Pure black", "#000000", "#000000", "#0a0a0a"],
  ["nord", "Nord", "#1f232b", "#232831", "#2a303b"],
  ["forest", "Forest", "#0b110e", "#0f1612", "#131c17"],
  ["plum", "Plum", "#110c16", "#15101c", "#1b1524"],
  ["ember", "Ember", "#120e0c", "#171210", "#1d1714"],
];

export function applyAccent() {
  const c = S.state?.settings?.accent || "#4f8cff";
  document.documentElement.style.setProperty("--acc", c);
  // opacity: its own setting with the pin on and off (30-100 %)
  const op = S.state?.pinned ? S.state?.settings?.opacity_pinned : S.state?.settings?.opacity_unpinned;
  document.documentElement.style.setProperty("--deck-opacity", Math.max(30, Math.min(100, op || 100)) / 100);
  const low = !!S.state?.settings?.low_spec;
  document.documentElement.classList.toggle("low-spec", low);
  document.documentElement.setAttribute("data-anim", low ? "none" : S.state?.settings?.open_anim || "zoom");
  const t = S.state?.settings?.theme || "midnight";
  if (t === "midnight") document.documentElement.removeAttribute("data-theme");
  else document.documentElement.setAttribute("data-theme", t);
}
