// Fake data for the pages other than Network / Sensors. Loaded after mock.js; patches window.__R and window.__settings.
(() => {
  const R = window.__R, st = window.__settings;
  const now = Date.now();
  const hist = (b, a, n = 60) => Array.from({ length: n }, (_, i) => Math.max(0, Math.round(b + a * Math.sin(i / 7) + (Math.random() - 0.5) * a * 0.6)));
  // coloured letter icons instead of the real icon server
  window.__iconSrc = (kind, key) => {
    const hue = [...String(key)].reduce((a, c) => a + c.charCodeAt(0), 0) % 360;
    const nm = window.__names?.[key] || String(key);
    const l = (nm.split(/[\\/]/).pop().replace(/[^A-Za-z0-9]/g, "")[0] || "?").toUpperCase();
    return "data:image/svg+xml;utf8," + encodeURIComponent(`<svg xmlns="http://www.w3.org/2000/svg" width="64" height="64"><rect width="64" height="64" rx="14" fill="hsl(${hue},55%,42%)"/><text x="32" y="43" font-family="Segoe UI" font-size="30" font-weight="700" fill="white" text-anchor="middle">${l}</text></svg>`);
  };
  const APPS = ["Discord", "Steam", "Spotify", "Visual Studio Code", "Google Chrome", "Microsoft Edge", "File Explorer", "Terminal", "Settings", "Epic Games Launcher", "OBS Studio", "Notepad", "Calculator", "Paint", "Task Manager", "7-Zip", "VLC media player", "Blender", "Figma", "Slack", "Telegram", "NVIDIA App", "HWiNFO64", "RivaTuner Statistics Server", "Battle.net", "Notion", "Obsidian", "Postman", "Docker Desktop", "Git Bash"];
  window.__names = Object.fromEntries(APPS.map((n, i) => ["app" + i, n]));
  const applist = APPS.map((n, i) => ({ id: "app" + i, name: n, path: `C:\\Program Files\\${n}\\${n.replace(/ /g, "")}.exe` }));
  Object.assign(st, {
    tiles: ["clock", "media", "system", "audio", "pinned", "quick", "clipboard", "power", "notes", "network"],
    pinned: [], frecency: { "app:app0": [14, now], "app:app1": [12, now], "app:app2": [9, now], "app:app3": [7, now], "app:app9": [4, now] },
    notes: [{ id: "n1", text: "Check RAM timings tonight\nBuy thermal pads for the VRM", updated: now - 3600e3, pinned: false }, { id: "n2", text: "Valorant crosshair code: 0;P;c;5;h;0", updated: now - 86400e3, pinned: true }],
    colors: ["#4F8CFF", "#FF453A", "#30D158", "#BF5AF2", "#FFD60A", "#1C1C1E"],
    mode: "panel", open_hotkey: "", hot_corner: "top-left", hot_corner_ms: 250, hot_corner_fullscreen: false, hide_on_blur: true, autostart: true, accent: "#4f8cff",
    clip_images: true, clip_max: 400, web_search: "https://www.google.com/search?q=", index_on: true, index_roots: ["C:\Users\you\Documents", "C:\Users\you\Downloads", "D:\Projects"], hotkeys_on: true, profile_toast: true,
    clip_on: true, clip_paste: true, zones_on: true, zones_trigger: "rbutton", layout: "thirds",
    layouts: [
      { id: "halves", name: "Halves", zones: [{ x: 0, y: 0, w: 0.5, h: 1 }, { x: 0.5, y: 0, w: 0.5, h: 1 }] },
      { id: "thirds", name: "Thirds", zones: [{ x: 0, y: 0, w: 0.333, h: 1 }, { x: 0.333, y: 0, w: 0.334, h: 1 }, { x: 0.667, y: 0, w: 0.333, h: 1 }] },
      { id: "grid", name: "Grid", zones: [{ x: 0, y: 0, w: 0.5, h: 0.5 }, { x: 0.5, y: 0, w: 0.5, h: 0.5 }, { x: 0, y: 0.5, w: 0.5, h: 0.5 }, { x: 0.5, y: 0.5, w: 0.5, h: 0.5 }] },
      { id: "main", name: "Main + side", zones: [{ x: 0, y: 0, w: 0.66, h: 1 }, { x: 0.66, y: 0, w: 0.34, h: 0.5 }, { x: 0.66, y: 0.5, w: 0.34, h: 0.5 }] },
    ],
    workspaces: [
      { id: "w1", name: "Coding", icon: "layout", windows: [{ exe: "C:\\Code\\Code.exe" }, { exe: "C:\\Chrome\\chrome.exe" }, { exe: "C:\\Terminal\\wt.exe" }], close: ["Steam.exe", "EpicGamesLauncher.exe"], audio_out: "", awake: false },
      { id: "w2", name: "Gaming", icon: "layout", windows: [{ exe: "C:\\Steam\\steam.exe" }, { exe: "C:\\Discord\\Discord.exe" }], close: [], audio_out: "Headphones (HyperX Cloud II)", awake: true },
    ],
  });
  R.set_setting = ({ key, value }) => { st[key] = value; return null; };
  R.gpu_live = () => ({ core_mhz: 2715, mem_mhz: 10501, power_w: 238, temp_c: 61, util: 87, mem_used_mb: 7340, mem_total_mb: 16376, fan_pct: [48], fan_manual: false, voltage_mv: 1050, limit: "power" });
  R.hw_mem = () => ({ used: 18.4, total: 32, cached: 9.2, free: 3.1 });
  const GB = 1073741824;
  R.disk_status = () => ({ running: false, root: "C:\\", target: "C:\\", took: 2.4, files: 812334, bytes: 740 * GB, elapsed: 2, method: "MFT", free: 212e9 });
  R.disk_types = () => [[".pak", 120, 900], [".vpk", 64, 210], [".dll", 48, 61000], [".exe", 31, 9000], [".mp4", 28, 140], [".sys", 30, 400], ["", 22, 80000], [".cache", 18, 3000]]
    .map(([ext, gb, count], i) => ({ ext, bytes: gb * GB, count, color: ["#e07a1f", "#d6c21a", "#2fb34a", "#9b4fd6", "#e0457b", "#2f8fe0", "#8a8f9c", "#3fc6d8"][i] }));
  // a made-up nested drive, squarified like the backend does (folders: label strip and padding)
  R.disk_treemap = ({ w, h }) => {
    let seed = 7;
    const rnd = () => ((seed = (seed * 16807) % 2147483647) / 2147483647);
    const tree = (depth) => Array.from({ length: depth > 2 ? 0 : 5 + Math.floor(rnd() * 6) }, (_, i) => {
      const kids = depth < 2 && rnd() < 0.5 ? tree(depth + 1) : [];
      return { name: kids.length ? `Folder ${depth}-${i}` : `file${i}.${["pak", "vpk", "dll", "exe", "mp4"][i % 5]}`, size: kids.length ? kids.reduce((a, k) => a + k.size, 0) : (1 + rnd() * 40) * GB, kids, c: 1 + (i % 5) };
    }).sort((a, b) => b.size - a.size);
    const sq = (areas, x, y, w, h) => {
      const out = []; let i = 0;
      while (i < areas.length) {
        const sh = Math.min(w, h); let j = i, sum = 0, best = Infinity;
        while (j < areas.length) { const ns = sum + areas[j]; const wr = Math.max((sh * sh * areas[i]) / (ns * ns), (ns * ns) / (sh * sh * areas[j])); if (wr > best) break; best = wr; sum = ns; j++; }
        const t = sum / sh;
        if (w >= h) { let yy = y; for (let k = i; k < j; k++) { out.push([x, yy, t, areas[k] / t]); yy += areas[k] / t; } x += t; w -= t; }
        else { let xx = x; for (let k = i; k < j; k++) { out.push([xx, y, areas[k] / t, t]); xx += areas[k] / t; } y += t; h -= t; }
        i = j;
      }
      return out;
    };
    const o = { r: [], k: [], c: [], d: [], ref: [], n: [], s: [], palette: ["#8a8f9c", "#e07a1f", "#d6c21a", "#2fb34a", "#9b4fd6", "#e0457b", "#2f8fe0", "#3fc6d8", "#c94fc4", "#d83a2f", "#7fae2e", "#1fb39a", "#a8702e", "#5a6ee0", "#e0a64f", "#6fd08a"] };
    const push = (r, k, c, d, ref, n, s) => { o.r.push(...r); o.k.push(k); o.c.push(c); o.d.push(d); o.ref.push(ref); o.n.push(n); o.s.push(s); };
    let ids = 0;
    const lay = (node, [x, y, w, h], depth) => {
      push([x, y, w, h], 0, 0, depth, ids++, node.name, node.size);
      const head = h >= 30 && w >= 40 ? 13 : 0, pad = w >= 8 && h >= 8 ? 1.5 : 0;
      const [ix, iy, iw, ih] = [x + pad, y + head + pad, w - 2 * pad, h - head - 2 * pad];
      if (iw < 1 || ih < 1) return;
      const sc = (iw * ih) / node.size;
      sq(node.kids.map((k) => k.size * sc), ix, iy, iw, ih).forEach((r, i) => {
        const k = node.kids[i];
        if (k.kids.length) lay(k, r, depth + 1); else push(r, 1, k.c, depth + 1, i, k.name, k.size);
      });
    };
    const kids = tree(0), used = kids.reduce((a, k) => a + k.size, 0), free = used * 0.25;
    const [ur, fr] = sq([used, free].map((v) => (v * w * h) / (used + free)), 0, 0, w, h);
    lay({ name: "C:\\", size: used, kids }, ur, 0);
    push(fr, 2, 0, 0, -1, "Free space", free);
    return o;
  };
  R.disk_children = () => ({ id: 0, crumbs: [{ id: 0, name: "C:\\" }], size: 740 * GB, files: 812334, folders: 96000, items: [
    ["Users", 310, 421000, 1], ["Windows", 38, 190000, 2], ["Program Files", 96, 61000, 3], ["Program Files (x86)", 54, 48000, 4], ["ProgramData", 21, 15000, 5], ["pagefile.sys", 18, 1, 0], ["hiberfil.sys", 12, 1, 0], ["$Recycle.Bin", 7, 120, 6], ["Intel", 1, 40, 7], ["Temp", 2, 900, 8]]
    .map(([name, gb, files, id]) => ({ id, name, size: gb * GB, files, folders: Math.round(files / 9), dir: name.includes(".sys") ? false : true, path: "C:\\" + name, keep: /^(Users|Windows|Program|pagefile|hiberfil|\$Recycle)/.test(name) })) });
  R.disk_big = () => [["D:/Fortnite/pakchunk0-WindowsClient.pak", 41], ["D:/CS2/game/csgo/pak01_dir.vpk", 24], ["C:/pagefile.sys", 18], ["D:/Premiere/render_final.mp4", 12], ["C:/hiberfil.sys", 12]].map(([p, gb]) => ({ name: p.split("/").pop(), path: p.replaceAll("/", "\\"), size: gb * GB, keep: p.startsWith("C:/") }));
  R.cleanup_state = () => ({ busy: false, last: [2.3 * GB], recycle: { items: 14, size: 1.8 * GB }, targets: [
    { id: "temp", name: "Windows temp files", desc: "Leftovers apps and installers put in the temp folders.", level: "safe", size: 3.1 * GB },
    { id: "shader", name: "Shader caches", desc: "NVIDIA and DirectX shader caches. Games rebuild them.", level: "safe", size: 1.2 * GB },
    { id: "crash", name: "Crash dumps and logs", desc: "Reports of old crashes.", level: "safe", size: 640 * 1048576 },
    { id: "chrome", name: "Chrome cache", desc: "Pages and images Chrome saved to load faster.", level: "safe", size: 2.4 * GB, running: true, close_first: "chrome.exe" },
    { id: "discord", name: "Discord cache", desc: "Images and media Discord stored.", level: "safe", size: 420 * 1048576 },
    { id: "thumbs", name: "Thumbnail cache", desc: "Explorer's picture previews.", level: "safe", size: 180 * 1048576 },
    { id: "wu", name: "Windows Update downloads", desc: "Old update files. Windows downloads them again if needed.", level: "careful", size: 4.6 * GB },
    { id: "installers", name: "Old installers", desc: "Setup files in Downloads you have already run.", level: "careful", size: 7.9 * GB }] });
  R.cleanup_scan = () => null;
  R.index_info = () => ({ building: false, count: 184233, took: 2.4 });
  R.rdesk_status = () => ({ installed: true, on: true, id: "123456789", direct: true, port: 21118, lan: "192.168.1.42", tray_hidden: true, tailscale: { installed: true, on: true, ip: "100.64.12.34" } });
  R.rdesk_do = () => R.rdesk_status();
  // Your PCs: Transfer and Claude Sync
  const tnow = Date.now();
  R.transfer_state = () => ({
    on: true, name: "DESKTOP", code: "K7QM-2XPA-9WTR-HB4N-6ZCE", manual_peers: [], inbox_dir: "C:\\Users\\you\\Downloads\\From my PCs",
    ask_before_receive: false, trust_install: false, shared_folders: ["C:\\Users\\you\\Projects"], skip_build: true,
    excludes: "target, node_modules, .git, __pycache__, *.pdb, *.obj, dist, build", dest_exists: "merge", port: 47834,
    policies: [{ id: "a1", name: "LAPTOP", mode: "ask" }],
    peers: [{ id: "a1", name: "LAPTOP", ip: "192.168.1.23" }, { id: "b2", name: "OFFICE-PC", ip: "192.168.1.40" }],
    xfers: [
      { id: 3, send: true, peer: "LAPTOP", label: "Projects", total: 812000000, done: 351000000, files: 2310, state: "active", error: "", speed: 61200000, secs: 6, note: "" },
      { id: 2, send: false, peer: "LAPTOP", label: "photos.zip", total: 48200000, done: 48200000, files: 1, state: "done", error: "", speed: 0, secs: 1.4, note: "" },
      { id: 1, send: true, peer: "OFFICE-PC", label: "Setup.exe", total: 120000000, done: 0, files: 1, state: "declined", error: "", speed: 0, secs: 0, note: "" }],
    pending: [{ id: 9, peer: "LAPTOP", label: "Firefox Setup.exe", files: 1, total: 63400000, install: true, deletes: 0 }],
    reqs: [],
    inbox: [
      { name: "VLC installer", dir: "C:\\x\\vlc", entry: "vlc.exe", kind: "nsis", kind_label: "Installer", from: "LAPTOP", size: 44000000, files: 1, time: tnow - 3600e3, note: "", gone: false, silent: true, extra: null },
      { name: "LAPTOP setup", dir: "C:\\x\\setup", entry: null, kind: "setup", kind_label: "Setup snapshot", from: "LAPTOP", size: 5200000, files: 18, time: tnow - 7200e3, note: "", gone: false, silent: false, extra: { look: 14, wallpaper: true, windhawk: "1.6", mods: 4, backup: false } },
      { name: "6 apps", dir: "C:\\x\\apps", entry: null, kind: "apps", kind_label: "App list", from: "LAPTOP", size: 900, files: 1, time: tnow - 86400e3, note: "Installed 5 of 6; 1 failed (no internet)", gone: false, silent: false, extra: { apps: ["7-Zip", "VLC", "Discord", "Steam", "Notepad++", "Firefox"] } }],
    scans: [], browse: null, setup_msg: "", logs: ["10:01:02  Transfer started on DESKTOP", "10:01:05  Found LAPTOP on the network (192.168.1.23:47834)"], data: "C:\\Users\\you\\AppData\\Roaming\\Deck\\transfer",
  });
  R.transfer_do = () => R.transfer_state();
  R.transfer_apps = () => ({ apps: [{ id: "x.0", name: "Battle.net", version: "1.0.0", runtime: false }, { id: "x.1", name: "BCUninstaller", version: "1.1.0", runtime: false }, { id: "x.2", name: "CapCut", version: "1.2.0", runtime: false }, { id: "x.3", name: "Claude", version: "1.3.0", runtime: false }, { id: "x.4", name: "Display Driver Uninstaller", version: "1.4.0", runtime: false }, { id: "x.5", name: "FanControl", version: "1.5.0", runtime: false }, { id: "x.6", name: "FastCopy", version: "1.6.0", runtime: false }, { id: "x.7", name: "HandBrake", version: "1.7.0", runtime: false }, { id: "x.8", name: "Inno Setup", version: "1.8.0", runtime: false }, { id: "x.9", name: "Internet Download Manager", version: "1.9.0", runtime: false }, { id: "x.10", name: "JDownloader 2", version: "1.10.0", runtime: false }, { id: "x.11", name: "Microsoft OneDrive", version: "1.11.0", runtime: false }, { id: "x.12", name: "Node.js", version: "1.12.0", runtime: false }, { id: "x.13", name: "OBS Studio", version: "1.13.0", runtime: false }, { id: "x.14", name: "Ollama", version: "1.14.0", runtime: false }, { id: "x.15", name: "Python Install Manager", version: "1.15.0", runtime: false }, { id: "x.16", name: "Steam", version: "1.16.0", runtime: false }, { id: "x.17", name: "Tailscale", version: "1.17.0", runtime: false }, { id: "x.18", name: "VLC media player", version: "1.18.0", runtime: false }, { id: "x.19", name: "7-Zip", version: "1.19.0", runtime: false }, { id: "7zip.7zip", name: "7-Zip", version: "24.08", runtime: false }, { id: "VideoLAN.VLC", name: "VLC media player", version: "3.0.21", runtime: false }, { id: "Microsoft.VCRedist.2015+.x64", name: "Microsoft Visual C++ 2015-2022", version: "14.40", runtime: true }, { id: "Valve.Steam", name: "Steam", version: "2.10", runtime: false }] });
  R.transfer_setup_preview = () => ({ look: 14, wallpaper: true, windhawk: "1.6", windhawk_installed: true, mods: ["taskbar-labels", "start-menu-styler", "explorer-details", "volume-scroll"] });
  R.claude_sync_state = () => ({
    on: true, standalone: false, phase: "Idle", last_cycle: 12000, last_result: "Up to date", last_error: "", lan_port: 47836, local_files: [214, 18, 63], local_bytes: 381000000,
    received: 42, conflicts: 0, hook_note: "", pending: 0, version: "2.5.0",
    held: R.__csHeld === false ? [] : [{ path: "settings.json", from: "LAPTOP" }],
    machines: [{ id: "m1", name: "LAPTOP", via: "network", ago: 30000, files: 290, online: true, version: "2.5.0", cmp: 0 }],
    projects: [{ alias: "~", home: true, path: "C:\\Users\\you", auto: false, origin: "local", sync: true, exists: true, sessions: 31, elsewhere: [{ machine: "LAPTOP", path: "C:\\Users\\me" }] },
      { alias: "website", home: false, path: "D:\\code\\website", auto: true, origin: "LAPTOP", sync: true, exists: false, sessions: 4, elsewhere: [{ machine: "LAPTOP", path: "C:\\dev\\website" }] }],
    open_here: [{ sid: "0b1c2d3e-0000-4000-8000-000000000001", name: "fix login", alias: "website", started: tnow - 600e3, pending: null }],
    open_elsewhere: [{ sid: "5f6e7d8c-0000-4000-8000-000000000002", name: "", alias: "~", machine: "LAPTOP", started: tnow - 1200e3, taken: false }],
    sessions: [
      { sid: "0b1c2d3e-0000-4000-8000-000000000001", title: "Fix the login form", first: "the login button does nothing on mobile", last: "now add a test", prompts: 14, size: 2400000, ago: 60000, alias: "website", here: true, elsewhere: ["LAPTOP"], online: [true], open_here: true, open_there: null, waiting: null },
      { sid: "5f6e7d8c-0000-4000-8000-000000000002", title: "", first: "plan the trip budget", last: "make it a table", prompts: 6, size: 310000, ago: 900000, alias: "~", here: true, elsewhere: ["LAPTOP"], online: [true], open_here: false, open_there: "LAPTOP", waiting: null }],
    cfg: { name: "DESKTOP", key: "Q3VX-8MNA-2KPL-7WRT-4HZE-9CBD", lan: true, manual_peers: "", hub: false, hub_dir: "", sessions: true, memory: true, config: true, interval: 20, paused: false, session_lock: true, machine_note: true },
    onedrive: "C:\\Users\\you\\OneDrive", claude_dir: "C:\\Users\\you\\.claude", data: "C:\\Users\\you\\AppData\\Roaming\\Deck\\claude-sync", logs: ["10:00:01 Claude Sync started"],
  });
  R.claude_sync_do = ({ what, args } = {}) => { if (what === "held") { R.__csHeld = false; R.__csHeldApply = args.apply; } return R.claude_sync_state(); };
  R.pick_many = () => [];
  R.share_state = () => ({ tailscale: true, funnel: "https://desktop.tailnet.ts.net", funnel_error: "", shares: [
    { token: "a1", name: "holiday-photos.zip", size: 812000000, expires: Date.now() + 20 * 3600e3, max: 1, done: 0, internet: true, preparing: false, error: "", home: "http://192.168.1.42:47837/s/k3Jd9aQ2xLm8", web: "https://desktop.tailnet.ts.net/deck-share/k3Jd9aQ2xLm8" },
    { token: "b2", name: "notes.pdf", size: 2400000, expires: Date.now() + 50 * 60e3, max: 0, done: 2, internet: false, preparing: false, error: "", home: "http://192.168.1.42:47837/s/Zp4Wq8Tn1Rv6", web: null }] });
  R.share_create = () => R.share_state();
  R.share_stop = () => R.share_state();
  R.remote_info = () => ({ enabled: true, away: "http://100.64.12.34:47830/#k=preview", away_qr: "", away_get_qr: "", away_peers: [], away_account: "someone@example.com", qr: '<svg xmlns="http://www.w3.org/2000/svg" width="180" height="180" viewBox="0 0 9 9"><rect width="9" height="9" fill="#fff"/><path d="M1 1h3v3H1zM5 1h3v3H5zM1 5h3v3H1zM5 5h1v1H5zM7 6h1v2H6V7h1z" fill="#000"/></svg>', clients: 1, origin: "http://192.168.1.42:47830", shared: [{ id: 1, kind: "text", text: "https://youtu.be/dQw4w9WgXcQ", from: "phone", ts: Date.now() - 600e3 }, { id: 2, kind: "file", name: "IMG_2041.jpg", from: "phone", ts: Date.now() - 3600e3, size: 2.4e6, path: "x" }, { id: 3, kind: "text", text: "Meet at 9", from: "pc", ts: Date.now() - 86400e3 }] });
  R.apps_list = () => applist;
  Object.assign(st, { boost_power: true, boost_cpu: "", boost_gpu: "", cpu_battery_profile: "", cpu_plugged_profile: "", cpu_battery_same: true, boost_priority: true, boost_trim: true, boost_auto_end: true,
    cpu_profiles: [{ id: "c1", name: "Eco" }, { id: "c2", name: "Mid" }, { id: "c3", name: "Max" }], gpu_profiles: [{ id: "g1", name: "AB Profile 5" }, { id: "g2", name: "Quiet" }] });
  window.__boost = { on: false, busy: false, active: null, last_clean: { at: Math.floor(now / 1000) - 7200, closed: ["Discord", "Spotify"], freed: 1.4e9, kept: [] }, ram: { total: 32e9, free: 13.5e9 }, plan: { kind: "scheme", from: "Balanced", name: "Ultimate Performance" } };
  window.__boostApps = [
    { key: "chrome", name: "Google Chrome", exe: "C:\\Chrome\\chrome.exe", mem: 2.1e9, window: true, voice: false, suggested: true, checked: true },
    { key: "code", name: "Visual Studio Code", exe: "C:\\Code\\Code.exe", mem: 1.1e9, window: true, voice: false, suggested: true, checked: true },
    { key: "discord", name: "Discord", exe: "C:\\Discord\\Discord.exe", mem: 640e6, window: true, voice: true, suggested: false, checked: false },
    { key: "spotify", name: "Spotify", exe: "C:\\Spotify\\Spotify.exe", mem: 410e6, window: true, voice: true, suggested: false, checked: false },
    { key: "onedrive", name: "OneDrive", exe: "C:\\OD\\OneDrive.exe", mem: 180e6, window: false, voice: false, suggested: true, checked: true },
    { key: "acc", name: "Adobe Creative Cloud", exe: "C:\\Adobe\\Creative Cloud.exe", mem: 220e6, window: false, voice: false, suggested: true, checked: true },
    { key: "docker", name: "Docker Desktop", exe: "C:\\Docker\\Docker.exe", mem: 1.8e9, window: false, voice: false, suggested: true, checked: false },
  ];
  R.boost_state = () => window.__boost;
  R.boost_apps = () => window.__boostApps;
  R.boost_pick = ({ key, on }) => { const a = window.__boostApps.find((x) => x.key === key); if (a) a.checked = on; return null; };
  const prof = (id, name, summary, hotkey, extra) => ({ p: { id, name, hotkey: hotkey || "", desktop: false, start_menu: false, icon: "", ...extra }, summary, icon_url: null, hotkey_state: hotkey ? "ok" : null, plan_name: "Deck · " + name });
  const cpuP = { cap_p: 4600, cap_e: 3600, turbo: 1, max_state: 100, min_state: 5, min_cores: 100, epp: 40, power: { mode: "custom", pl1: 125, pl2: 181 }, min_lock: null, oc: null, hold_in_games: true, plan: "x" };
  R.cpu_live = () => ({ mhz: 4600, temp: 58, load: 23, power: 64.3, power_short: "125 / 181 W", active: "c2" });
  R.cpu_state = () => ({ admin: true, rapl: true, active: "c2", profiles: [prof("c1", "Eco", "Cap 3.0 GHz · PL1 65 W", "Ctrl+Alt+1"), prof("c2", "Mid", "Cap 4.6 GHz · PL1 125 W", "Ctrl+Alt+2", cpuP), prof("c3", "Max", "Turbo · PL1 181 W", "Ctrl+Alt+3")], clock: null, limits: { min: 800, turbo: 5300, oc: true, oc_max: 5800 }, hybrid: true });
  const gpuP = { power_pct: 85, temp_limit: null, core_mhz: 120, mem_mhz: 500, volt_pct: null, curve: null, lock_mv: null, fan: { mode: "curve", pct: 50, points: [[30, 30], [50, 40], [65, 60], [80, 85], [90, 100]] } };
  R.gpu_state = () => ({ found: true, admin: true, active: "g1", startup: "g1", profiles: [prof("g1", "AB Profile 5", "PL 85% · +120 core · curve", "Ctrl+Alt+O", gpuP), prof("g2", "Quiet", "PL 70% · fan 45%", "Ctrl+Alt+P"), prof("g3", "Stock", "Default", null)], caps: { name: "NVIDIA GeForce RTX 4070 Ti SUPER", driver: "572.16", power: [100, 285, 285], power_pct: [50, 112], core: [-1000, 1000], mem: [-1000, 1500], temp: [65, 90], volt: true, fan_min: 30, curve: true, max_core: 2850 } });
  // the card's stock voltage/frequency curve: 700 to 1100 mV
  R.fonts_list = () => ["Arial", "Bahnschrift", "Calibri", "Cambria", "Cascadia Code", "Cascadia Mono", "Consolas", "Courier New", "Georgia", "JetBrains Mono", "Lucida Console", "Segoe UI", "Segoe UI Variable", "Tahoma", "Unispace", "Verdana"];
  R.gpu_curve = () => ({ points: Array.from({ length: 33 }, (_, i) => ({ mv: 700 + i * 12.5, base: Math.round(1000 + 1850 * Math.pow(i / 32, 0.8)), delta: 0 })) });
  R.games_list = () => [
    { id: "s730", name: "Counter-Strike 2", store: "steam", size: 38e9, last: Math.floor(now / 1000) - 600, dir: "C:\\Steam\\730", launch: "steam://run/730" },
    { id: "s1422450", name: "Deadlock", store: "steam", size: 26e9, last: Math.floor(now / 1000) - 86400 * 2, dir: "C:\\Steam\\1422450", launch: "steam://run/1422450" },
    { id: "e1", name: "Fortnite", store: "epic", size: 94e9, last: Math.floor(now / 1000) - 86400 * 19, dir: "C:\\Epic\\Fortnite", launch: "x" },
    { id: "s570", name: "Dota 2", store: "steam", size: 41e9, last: null, dir: "C:\\Steam\\570", launch: "steam://run/570" },
    { id: "s1245620", name: "Elden Ring", store: "steam", size: 49e9, last: Math.floor(now / 1000) - 86400 * 60, dir: "C:\\Steam\\1245620", launch: "x" },
  ];
  R.audio_state = () => ({ outputs: [{ id: "o1", name: "Speakers (Realtek High Definition Audio)", default: false }, { id: "o2", name: "Headphones (HyperX Cloud II Virtual Surround Sound)", default: true }, { id: "o3", name: "Digital Output (NVIDIA High Definition Audio)", default: false }] });
  const live = R.live;
  R.live = () => ({
    ...live(),
    sys: {
      cpu: 23, cores: 20, ram_pct: 58, ram_used: 18.4e9, ram_total: 32e9, gpu: { temp: 61, util: 87, power_w: 238 }, down: 845200 + Math.random() * 90000, up: 64100 + Math.random() * 8000,
      cpu_hist: hist(25, 18), ram_hist: hist(58, 4), gpu_hist: hist(80, 12), down_hist: hist(5e5, 4e5), up_hist: hist(6e4, 3e4),
      disks: [{ mount: "C:", free: 212e9, total: 953e9, removable: false }, { mount: "D:", free: 1.1e12, total: 1.86e12, removable: false }, { mount: "E:", free: 61e9, total: 931e9, removable: false }],
    },
    media: { current: { id: "m1", title: "Nights", artist: "Frank Ocean", album: "Blonde", app: "Spotify", status: "playing", pos: 94, len: 307, can_prev: true, can_next: true, art: null }, others: [{ id: "m2", title: "How to lose weight - YouTube", artist: "", app: "Chrome", status: "paused", can_prev: false, can_next: false }] },
    master: { vol: 0.62, muted: false, peak: 0.34, device: "Headphones (HyperX Cloud II Virtual Surround Sound)" },
    apps: [{ key: "spotify", name: "Spotify", exe: "C:\\Spotify\\Spotify.exe", vol: 0.8, muted: false, peak: 0.4 }, { key: "cs2", name: "Counter-Strike 2", exe: "C:\\Steam\\cs2.exe", vol: 0.55, muted: false, peak: 0.7 }, { key: "discord", name: "Microsoft Edge WebView2 Runtime", exe: "C:\\Discord\\Discord.exe", vol: 1, muted: false, peak: 0.1 }, { key: "chrome", name: "Google Chrome", exe: "C:\\Chrome\\chrome.exe", vol: 0.3, muted: true, peak: 0 }],
    power: { ...window.__power, now: Date.now(), timers: [{ id: "t1", label: "Pasta", secs: 600, end_ms: Date.now() + 340e3, paused_left: null, done: false }] },

  });
  const clips = [
    { id: "c1", kind: "text", text: "https://github.com/anthropics/claude-code/issues/4821?utm_source=twitter&utm_medium=social", files: [], pinned: false, ts: now - 90e3, app: "Chrome" },
    { id: "c2", kind: "text", text: "const total = items.reduce((a, b) => a + b.price * b.qty, 0);\nconsole.log(total.toFixed(2));", files: [], pinned: true, ts: now - 12 * 60e3, app: "Code" },
    { id: "c3", kind: "image", text: "", files: [], image: "shot1", w: 1920, h: 1080, pinned: false, ts: now - 40 * 60e3, app: "Snipping Tool" },
    { id: "c4", kind: "files", text: "", files: ["D:\\Downloads\\Resume_final_v3.pdf", "D:\\Downloads\\cover-letter.docx"], pinned: false, ts: now - 3 * 3600e3, app: "Explorer" },
    { id: "c5", kind: "text", text: "192.168.1.1", files: [], pinned: false, ts: now - 5 * 3600e3, app: "Terminal" },
    { id: "c6", kind: "text", text: "Hey, are we still on for tonight? I can hop on at 9 if you're free.", files: [], pinned: false, ts: now - 26 * 3600e3, app: "Discord" },
  ];
  R.clip_list = ({ limit }) => clips.slice(0, limit || 8);
  R.windows_list = () => [
    { hwnd: 1, title: "Deck", app: "Deck", exe: "C:\\Program Files\\Deck\\Deck.exe", topmost: false, opacity: 255, hung: false },
    { hwnd: 2, title: "index.html — deck — Visual Studio Code", app: "Visual Studio Code", exe: "C:\\Code\\Code.exe", topmost: false, opacity: 255, hung: false },
    { hwnd: 3, title: "YouTube - Google Chrome", app: "Google Chrome", exe: "C:\\Chrome\\chrome.exe", topmost: true, opacity: 200, hung: false },
    { hwnd: 4, title: "Discord | #general", app: "Discord", exe: "C:\\Discord\\Discord.exe", topmost: false, opacity: 255, hung: false },
    { hwnd: 5, title: "Spotify Premium", app: "Spotify", exe: "C:\\Spotify\\Spotify.exe", topmost: false, opacity: 255, hung: true },
  ];
  R.processes = () => [
    ["Google Chrome", "chrome.exe", 4.2, 2.1e9, 18, "YouTube - Google Chrome", true], ["Visual Studio Code", "Code.exe", 1.8, 1.1e9, 9, "deck — VS Code", true], ["Counter-Strike 2", "cs2.exe", 38.5, 4.7e9, 1, "Counter-Strike 2", true],
    ["Discord", "Discord.exe", 0.9, 640e6, 6, "Discord", true], ["Spotify", "Spotify.exe", 0.6, 410e6, 5, "Spotify Premium", true], ["Windows Explorer", "explorer.exe", 0.4, 210e6, 1, "", false],
    ["NVIDIA Overlay", "nvcontainer.exe", 0.2, 180e6, 3, "", false], ["Steam", "steam.exe", 0.5, 330e6, 4, "Steam", true], ["svchost", "svchost.exe", 0.3, 95e6, 42, "", false], ["Deck", "Deck.exe", 0.4, 110e6, 3, "", false],
  ].map(([name, exe, cpu, mem, count, title, app]) => ({ name, exe: "C:\\" + exe, cpu, mem, count, title, app, hung: false, protected: /explorer|svchost/.test(exe) }));
  R.startup_list = () => [
    { id: "s1", name: "Discord", exe: "C:\\Discord\\Discord.exe", command: "C:\\Users\\you\\AppData\\Local\\Discord\\Update.exe --processStart Discord.exe", source: "Registry (you)", enabled: true, can_change: true },
    { id: "s2", name: "Steam", exe: "C:\\Steam\\steam.exe", command: "\"C:\\Steam\\steam.exe\" -silent", source: "Registry (you)", enabled: true, can_change: true },
    { id: "s3", name: "Spotify", exe: "C:\\Spotify\\Spotify.exe", command: "C:\\Spotify\\Spotify.exe --autostart --minimized", source: "Registry (you)", enabled: false, can_change: true },
    { id: "s4", name: "RTSS", exe: "C:\\RTSS\\RTSS.exe", command: "\"C:\\RTSS\\RTSS.exe\"", source: "Registry (all users)", enabled: true, can_change: false },
    { id: "s5", name: "NVIDIA Broadcast", exe: "C:\\NV\\NvBroadcast.exe", command: "C:\\NV\\NvBroadcast.exe", source: "Startup folder", enabled: false, can_change: true },
  ];
  R.display_state = () => ({
    dark: true, schedule: null, monitors: [
      { device: "\\\\.\\DISPLAY1", name: "LG ULTRAGEAR 27GP850", primary: true, width: 2560, height: 1440, hz: 165, rates: [60, 75, 120, 144, 165], brightness: 70, hdr_supported: true, hdr_on: false },
      { device: "\\\\.\\DISPLAY2", name: "Dell P2419H", primary: false, width: 1920, height: 1080, hz: 60, rates: [60], brightness: null, hdr_supported: false, hdr_on: false }],
  });
})();
