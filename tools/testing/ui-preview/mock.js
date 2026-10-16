// Fake Tauri backend so Deck's UI renders in a plain browser for screenshots.
(() => {
  const N = 300;
  const wave = (base, amp, noise, f = 1) => Array.from({ length: N }, (_, i) => +(base + amp * Math.sin(i / (18 * f)) + (Math.random() - 0.5) * noise).toFixed(1));
  const temp_names = ["System", "MOS", "Chipset", "CPU socket", "PCIe x1", "VRM"];
  const fan_names = ["CPU fan", "Pump", "Front 1", "Front 2", "Rear", "Top 1", "Top 2"];
  const rpm = [1180, 2650, 840, 860, 910, 0, 0];
  const rows = [];
  const add = (group, name, unit, dec, v, spread = 0.12) => {
    const f = (x) => (x == null ? "-" : `${x.toFixed(dec)} ${unit}`.trim());
    rows.push({ group, name, unit, cur: f(v), min: f(v == null ? null : v * (1 - spread)), max: f(v == null ? null : v * (1 + spread * 1.6)), avg: f(v == null ? null : v * (1 + spread * 0.2)) });
  };
  const cpu = "CPU: Intel Core i7-13700K";
  add(cpu, "Package temperature", "°C", 1, 58.2);
  for (let i = 0; i < 6; i++) add(cpu, `P-core ${i} temperature`, "°C", 0, 52 + i * 2);
  for (let i = 0; i < 8; i++) add(cpu, `E-core ${i} temperature`, "°C", 0, 47 + (i % 4));
  add(cpu, "Core max temperature", "°C", 0, 63);
  add(cpu, "Core voltage (VID)", "V", 3, 1.182, 0.05);
  add(cpu, "Package power", "W", 1, 64.3, 0.5);
  add(cpu, "Cores power", "W", 1, 51.7, 0.5);
  for (let i = 0; i < 6; i++) add(cpu, `P-core ${i} clock`, "MHz", 0, 5300 - i * 10, 0.3);
  for (let i = 0; i < 8; i++) add(cpu, `E-core ${i} clock`, "MHz", 0, 4000, 0.3);
  add(cpu, "Total load", "%", 1, 23.4, 0.8);
  for (let i = 0; i < 6; i++) add(cpu, `P-core ${i} load`, "%", 1, 30 - i * 3, 0.8);
  for (let i = 0; i < 8; i++) add(cpu, `E-core ${i} load`, "%", 1, 8 + i, 0.8);
  const gpu = "GPU: NVIDIA GeForce RTX 4070 Ti SUPER";
  add(gpu, "Temperature", "°C", 0, 61);
  add(gpu, "Hot spot temperature", "°C", 0, 72);
  add(gpu, "Core load", "%", 0, 87, 0.6);
  add(gpu, "Memory controller load", "%", 0, 44, 0.6);
  add(gpu, "Memory used", "MB", 0, 7340, 0.4);
  add(gpu, "Memory total", "MB", 0, 16376, 0);
  add(gpu, "Core clock", "MHz", 0, 2715, 0.5);
  add(gpu, "Memory clock", "MHz", 0, 10501, 0.5);
  add(gpu, "Power", "W", 1, 238.4, 0.7);
  add(gpu, "Fan", "%", 0, 48, 0.5);
  const mb = "Motherboard: MSI PRO Z790-A WIFI (NCT6687D)";
  temp_names.forEach((n, i) => add(mb, `${n} temperature`, "°C", 1, 34 + i * 4));
  [["Vcore", 1.21], ["+5V", 5.04], ["+12V", 12.1], ["+3.3V", 3.32], ["DRAM", 1.35], ["CPU SA", 1.05]].forEach(([n, v]) => add(mb, `${n} voltage`, "V", 3, v, 0.01));
  fan_names.forEach((n, i) => add(mb, `${n} speed`, "RPM", 0, rpm[i], 0.2));
  fan_names.forEach((n, i) => add(mb, `${n} duty`, "%", 0, rpm[i] ? 40 + i * 5 : 0, 0.2));
  const mem = "Memory";
  [["Physical memory used", "GB", 1, 18.4], ["Physical memory available", "GB", 1, 13.5], ["Physical memory load", "%", 0, 58], ["Committed (incl. page file)", "GB", 1, 24.1], ["Commit limit", "GB", 1, 36.8], ["Standby cache", "GB", 1, 9.2], ["Free (zeroed)", "GB", 1, 3.1]].forEach((r) => add(mem, ...r));
  for (const d of ["Samsung SSD 990 PRO 2TB (Disk 0)", "WD_BLACK SN850X 1TB (Disk 1)", "ST2000DM008 (Disk 2)"]) {
    const g = `Drive: ${d}`;
    add(g, "Temperature", "°C", 0, 41);
    add(g, "Read rate", "MB/s", 1, 12.4, 0.9);
    add(g, "Write rate", "MB/s", 1, 3.2, 0.9);
    add(g, "Total activity", "%", 0, 6, 0.9);
  }
  for (const n of ["Ethernet", "Wi-Fi"]) {
    const g = `Network: ${n}`;
    add(g, "Download rate", "KB/s", 1, n === "Ethernet" ? 845.2 : 0, 0.9);
    add(g, "Upload rate", "KB/s", 1, n === "Ethernet" ? 64.1 : 0, 0.9);
    add(g, "Downloaded (since start)", "MB", 1, n === "Ethernet" ? 1843.5 : 0.4, 0);
    add(g, "Uploaded (since start)", "MB", 1, n === "Ethernet" ? 211.8 : 0.1, 0);
  }

  // Cooler display: shape copied from a real hw_state (84 LEDs)
  const LCD = {"geometry": [[0, [4, 18, 30, 10]], [1, [4, 32, 30, 10]], [2, [44, 11, 4, 18]], [3, [47, 8, 18, 4]], [4, [64, 11, 4, 18]], [5, [47, 28, 18, 4]], [6, [44, 31, 4, 18]], [7, [47, 48, 18, 4]], [8, [64, 31, 4, 18]], [9, [76, 11, 4, 18]], [10, [79, 8, 18, 4]], [11, [96, 11, 4, 18]], [12, [79, 28, 18, 4]], [13, [76, 31, 4, 18]], [14, [79, 48, 18, 4]], [15, [96, 31, 4, 18]], [16, [108, 11, 4, 18]], [17, [111, 8, 18, 4]], [18, [128, 11, 4, 18]], [19, [111, 28, 18, 4]], [20, [108, 31, 4, 18]], [21, [111, 48, 18, 4]], [22, [128, 31, 4, 18]], [23, [140, 10, 18, 14]], [24, [140, 34, 18, 14]], [25, [176, 11, 4, 18]], [26, [176, 31, 4, 18]], [27, [188, 11, 4, 18]], [28, [191, 8, 18, 4]], [29, [208, 11, 4, 18]], [30, [191, 28, 18, 4]], [31, [188, 31, 4, 18]], [32, [191, 48, 18, 4]], [33, [208, 31, 4, 18]], [34, [220, 11, 4, 18]], [35, [223, 8, 18, 4]], [36, [240, 11, 4, 18]], [37, [223, 28, 18, 4]], [38, [220, 31, 4, 18]], [39, [223, 48, 18, 4]], [40, [240, 31, 4, 18]], [41, [254, 22, 16, 16]], [82, [4, 76, 30, 10]], [83, [4, 90, 30, 10]], [81, [44, 69, 4, 18]], [80, [47, 66, 18, 4]], [79, [64, 69, 4, 18]], [78, [47, 86, 18, 4]], [77, [44, 89, 4, 18]], [76, [47, 106, 18, 4]], [75, [64, 89, 4, 18]], [74, [76, 69, 4, 18]], [73, [79, 66, 18, 4]], [72, [96, 69, 4, 18]], [71, [79, 86, 18, 4]], [70, [76, 89, 4, 18]], [69, [79, 106, 18, 4]], [68, [96, 89, 4, 18]], [67, [108, 69, 4, 18]], [66, [111, 66, 18, 4]], [65, [128, 69, 4, 18]], [64, [111, 86, 18, 4]], [63, [108, 89, 4, 18]], [62, [111, 106, 18, 4]], [61, [128, 89, 4, 18]], [59, [140, 68, 18, 14]], [60, [140, 92, 18, 14]], [58, [176, 69, 4, 18]], [57, [176, 89, 4, 18]], [56, [188, 69, 4, 18]], [55, [191, 66, 18, 4]], [54, [208, 69, 4, 18]], [53, [191, 86, 18, 4]], [52, [188, 89, 4, 18]], [51, [191, 106, 18, 4]], [50, [208, 89, 4, 18]], [49, [220, 69, 4, 18]], [48, [223, 66, 18, 4]], [47, [240, 69, 4, 18]], [46, [223, 86, 18, 4]], [45, [220, 89, 4, 18]], [44, [223, 106, 18, 4]], [43, [240, 89, 4, 18]], [42, [254, 80, 16, 16]]], "display": {"anim": "Spinner", "bright": 100, "clock24": true, "clock_bottom": "Date", "color": "#009BDE", "color_mode": "By temperature", "content": "Sensors", "fahrenheit": false, "flash": false, "flash_at": 90, "music": "Bars", "night": false, "night_bright": 20, "night_from": 23, "night_to": 7, "off": false, "rotate": ["Sensors", "Clock"], "rotate_secs": 8, "scroll": false, "show": [true, true, true, true], "speed": 40, "text_bottom": "", "text_top": "HELLO", "design": ["#000000", "#000000", "#000000", "#000000", "#000000", "#000000", "#000000", "#000000", "#000000", "#000000", "#000000", "#000000", "#000000", "#000000", "#000000", "#000000", "#000000", "#000000", "#000000", "#000000", "#000000", "#000000", "#000000", "#000000", "#000000", "#000000", "#000000", "#000000", "#000000", "#000000", "#000000", "#000000", "#000000", "#000000", "#000000", "#000000", "#000000", "#000000", "#000000", "#000000", "#000000", "#000000", "#000000", "#000000", "#000000", "#000000", "#000000", "#000000", "#000000", "#000000", "#000000", "#000000", "#000000", "#000000", "#000000", "#000000", "#000000", "#000000", "#000000", "#000000", "#000000", "#000000", "#000000", "#000000", "#000000", "#000000", "#000000", "#000000", "#000000", "#000000", "#000000", "#000000", "#000000", "#000000", "#000000", "#000000", "#000000", "#000000", "#000000", "#000000", "#000000", "#000000", "#000000", "#000000"]}, "lists": {"anims": ["Spinner", "Chase", "Sparkle", "Wave", "Snake"], "clock_bottom": ["Date", "Seconds", "CPU temp", "GPU temp", "Off"], "contents": ["Sensors", "Clock", "Text", "Design", "Animation", "Music meter", "Rotate"], "dmodes": ["Custom", "Rainbow", "Color cycle", "Breathing", "By temperature", "By load", "Same as lighting"], "music_disp": ["Bars", "Spectrum", "Pulse", "Beat flash", "Levels"]}};
  const DEFAULT_CURVES = [["Silent", [[20, 20], [50, 25], [70, 40], [85, 70], [100, 100]]], ["Balanced", [[20, 25], [45, 30], [65, 50], [80, 80], [100, 100]]], ["Performance", [[20, 35], [40, 45], [60, 70], [75, 100], [100, 100]]]];
  const settings = {
    mode: "app", accent: "#4f8cff", theme_picked: true, look_picked: true, setup_picked: true, open_anim: "zoom", low_spec: false, pc_cards: ["cpu", "ram", "gpu"], pc_drives: true, remote_screen: "control", remote_screen_quality: "balanced", remote_screen_fps: 60, remote_screen_kbps: 0, remote_screen_priority: "smooth", remote_screen_codec: "auto", nav_folded: [], nav_width: 0, panels: {}, ping_hosts: ["1.1.1.1", "8.8.8.8", "google.com"],
    osd_on: true, osd_items: ["gpu_temp", "gpu_usage", "gpu_mem", "gpu_clock", "gpu_power", "cpu_temp", "cpu_usage", "cpu_clock", "ram", "fps", "frametime", "fps_1low"], osd_hotkey: "Ctrl+Alt+O", osd_reset_hotkey: "", osd_ms: 500, rtss_autostart: true, mahm_on: false,
    breaks_on: false, theme_auto: false, theme_mode: "times", light_at: "07:00", dark_at: "19:00",
    remote_on: true, // remote_info says the phone server is running
  };
  const speed = { phase: "done", down_mbps: 487.3, up_mbps: 42.8, ping_ms: 11.2, jitter_ms: 1.4, progress: 1, error: null, at: Date.now() - 3600 * 5 * 1000 };
  const valve = [["Frankfurt", "Germany", 18, 0], ["Amsterdam", "Netherlands", 24, 0], ["Vienna", "Austria", 27, 0], ["Stockholm", "Sweden", 39, 0], ["Warsaw", "Poland", 41, 0], ["Madrid", "Spain", 46, 0.02], ["London", "United Kingdom", 52, 0], ["Helsinki", "Finland", 58, 0], ["Istanbul", "Turkey", 71, 0], ["Dubai", "UAE", 112, 0], ["Virginia", "United States", 128, 0], ["Singapore", "Singapore", 184, 0.05], ["Tokyo", "Japan", 241, 0], ["Sydney", "Australia", null, 1]];
  const R = {
    // drive health: a healthy NVMe, a worn SATA hard drive with bad sectors, and a USB drive that hides its data
    drive_health: () => [
      { disk: 0, model: "Example NVMe 1TB", serial: "A", bus: "NVMe", ssd: true, size: 1000204886016, status: "good", reasons: [], history: ["The drive got too hot in the past (up to about 68 °C, its limit is about 60 °C)"], health: 97, temp: 45, hours: 3171, cycles: 1153, written: 14134055936000, read: 9e12, unsafe_shutdowns: 61, media_errors: 0, bad_sectors: null, attrs: [] },
      { disk: 1, model: "Example HDD 1TB", serial: "B", bus: "SATA", ssd: false, size: 1000204886016, status: "caution", reasons: ["8 reallocated sectors (worn-out spots the drive replaced)", "1 sector waiting to be reallocated"], health: null, temp: 38, hours: 33385, cycles: 2264, written: null, read: null, unsafe_shutdowns: null, media_errors: null, bad_sectors: 9,
        attrs: [{ id: 1, name: "Read error rate", current: 100, worst: 100, threshold: 16, raw: 0 }, { id: 5, name: "Reallocated sectors", current: 100, worst: 100, threshold: 5, raw: 8 }, { id: 9, name: "Power-on hours", current: 21, worst: 21, threshold: 0, raw: 33385 }, { id: 194, name: "Temperature", current: 157, worst: 120, threshold: 0, raw: 38 }, { id: 197, name: "Pending sectors", current: 100, worst: 100, threshold: 0, raw: 1 }] },
      { disk: 2, model: "USB Flash", serial: "", bus: "USB", ssd: false, size: 64023257088, status: "unknown", reasons: [], health: null, temp: null, hours: null, cycles: null, written: null, read: null, unsafe_shutdowns: null, media_errors: null, bad_sectors: null, attrs: [] },
    ],
    get_state: () => ({ settings, version: "0.1.1", pc: "PC", user: "you", remote: { clients: 0 }, index: {}, pinned: false, home: "C:/Users/you", admin: true, visible: true, hw: { pages: ["cpu", "gpu", "fans", "lighting", "lcd", "sensors", "thispc"], ready: true } }),
    live: () => ({ sys: { down: 845200 + Math.random() * 90000, up: 64100 + Math.random() * 8000, cpu: 23, mem: 58 }, media: null, master: null, apps: [], power: window.__power, toasts: 0, boost: { on: false }, profiles: { cpu: "Mid", gpu: "AB Profile 5" } }),
    net_info: () => ({ lan: "192.168.1.42", public: "84.112.37.201", host: "DESKTOP-GAMING", speed }),
    net_public_ip: () => "84.112.37.201",
    net_ping: ({ hosts }) => hosts.map((h, i) => { const b = [9.8, 14.2, 21.7, 44][i] ?? 30; const lost = Math.random() < 0.04; return { host: h, ms: lost ? null : b + Math.random() * b * 0.6 + (Math.random() < 0.08 ? 40 : 0), loss: lost ? 1 : 0 }; }),
    net_speed: () => null, net_speed_cancel: () => null,
    game_ping_games: () => [
      { id: "cs2", name: "Counter-Strike 2", installed: true, kind: "valve" }, { id: "deadlock", name: "Deadlock", installed: true, kind: "valve" }, { id: "dota2", name: "Dota 2", installed: false, kind: "valve" },
      { id: "fortnite", name: "Fortnite", installed: false, kind: "aws" }, { id: "rocketleague", name: "Rocket League", installed: false, kind: "aws" }, { id: "dbd", name: "Dead by Daylight", installed: false, kind: "aws" }, { id: "tf2", name: "Team Fortress 2", installed: false, kind: "valve" }],
    game_ping: () => valve.map(([name, sub, ms, loss]) => ({ name, sub, ms, loss })),
    hw_state: () => { const st = hwState0(); st.geometry = LCD.geometry; st.cfg.display = { ...LCD.display }; Object.assign(st.lists, LCD.lists); return st; },
    hw_lcd_preview: () => LCD.geometry.map((_, i) => (i % 7 ? "#00a0e0" : "#000000")),
    hw_state0: () => ({ ready: true, admin: true,
      info: { temp_names, fan_names, control: "verified", chip: "NCT6687D", board_vendor: "Micro-Star International Co., Ltd.", board_name: "PRO Z790-A WIFI", bios: "7E07v1C", cpu: "Intel Core i7-13700K", cpu_note: "", gpu: "NVIDIA GeForce RTX 4070 Ti SUPER", dimms: [["Kingston FURY", "DDR5-6000 16 GB"], ["Kingston FURY", "DDR5-6000 16 GB"]] },
      sources: ["CPU", "GPU", "Highest of CPU and GPU", "System"],
      lists: { modes: ["Static", "Breathing", "Rainbow", "Color Cycle", "Wave", "Comet", "Temperature", "Music", "Screen", "Game", "Off"], music_styles: ["Pulse", "Spectrum", "Bars"], game_sounds: ["Shots and explosions", "Everything"], fixed: ["Auto (BIOS)", "Fixed 30%", "Fixed 40%", "Fixed 50%", "Fixed 60%", "Fixed 80%", "Fixed 100%"], default_curves: DEFAULT_CURVES },
      status: { rgb: "Motherboard RGB connected", ram: "RAM RGB: 2 modules", ram_ok: true, rgb_ok: true, disp: "No cooler display found", disp_ok: null }, lights_off: false,
      cfg: { lighting: { mode: "Wave", color: "#BF5AF2", bright: 80, speed: 40, zones: [true, true, false, true], n1: 60, n2: 60, music_style: "Pulse", music_rainbow: true, music_gain: 150, auto_game: true, game_sound: "Shots and explosions", game_only: true }, alerts: { on: true, cpu: 90, gpu: 85, log_csv: false },
        fans: { allow_experimental: false, hyst: 3, delay: 8, stop: 0, curves: DEFAULT_CURVES, channels: fan_names.map((n, i) => ({ name: n, mode: i === 1 ? "Fixed 100%" : i < 5 ? "Curve: Balanced" : "Auto (BIOS)", src: i === 1 ? 0 : 2 })) } } }),
    hw_live: () => ({ cpu_t: 58 + Math.random() * 2, cpu_l: 23, cpu_mhz: 5300, gpu_t: 61, gpu_l: 87, fan_pct: [40, 100, 45, 45, 50, 0, 0], fan_rpm: rpm, board_t: temp_names.map((_, i) => 34 + i * 4), volts: [], source_t: [58, 61, 61, 38], in_game: false, lights_off: false, status: { rgb: "Motherboard RGB connected", ram: "RAM RGB: 2 modules", ram_ok: true } }),
    hw_hist: () => ({ cpu_t: wave(56, 6, 3), gpu_t: wave(60, 4, 2, 1.4), cpu_l: wave(25, 15, 10), gpu_l: wave(80, 12, 8, 1.3), cpu_w: wave(95, 30, 12), gpu_w: wave(210, 25, 15, 1.2), board: temp_names.map((_, i) => wave(34 + i * 4, 1, 0.6, 2)), rpm: rpm.map((r) => wave(r, r * 0.05, r * 0.02, 1.7)) }),
    hw_table: () => ({ since: Math.floor(Date.now() / 1000) - 4 * 3600 - 17 * 60, rows }),
    hw_table_reset: () => null,
    fonts_list: () => ["Bahnschrift", "Consolas", "JetBrains Mono", "Segoe UI", "Arial"],
    rtss_limiter: ({ what, fps }) => ({ on: what === "toggle" ? true : false, fps: fps ?? 141 }),
    rtss_state: () => {
      const D = [["gpu_temp", "Temperature", "GPU temp", "gpu", "gpu", "°C", 64], ["gpu_usage", "Usage", "GPU load", "gpu", "gpu", "%", 98], ["gpu_mem", "Memory used", "VRAM", "gpu", "vram", "MB", 7000], ["gpu_mem_pct", "Memory used (%)", "VRAM %", "gpu", "vram", "%", 85], ["gpu_clock", "Core clock", "GPU clock", "gpu", "gpu", "MHz", 1920], ["gpu_memclock", "Memory clock", "VRAM clock", "gpu", "vram", "MHz", 7000], ["gpu_power", "Power", "GPU power", "gpu", "gpu", "W", 212], ["gpu_power_pct", "Power (% of the default limit)", "GPU power %", "gpu", "gpu", "%", 96], ["gpu_fan", "Fan", "GPU fan", "gpu", "gpu", "%", 58], ["gpu_volt", "Voltage", "GPU volt", "gpu", "gpu", "mV", 1050], ["gpu_limit", "What is holding the clock back", "GPU limit", "gpu", "gpu", "", "power"],
        ["cpu_temp", "Temperature", "CPU temp", "cpu", "cpu", "°C", 71], ["cpu_usage", "Usage", "CPU load", "cpu", "cpu", "%", 34], ["cpu_clock", "Clock", "CPU clock", "cpu", "cpu", "MHz", 5300], ["cpu_power", "Power", "CPU power", "cpu", "cpu", "W", 96], ["cpu_volt", "Voltage (VID)", "CPU volt", "cpu", "cpu", "V", 1.213],
        ["cpu_cores", "Usage of each thread (grid)", "Thread %", "cpu", "cores", "", 0], ["core_clock", "Clock of each core", "Core MHz", "cpu", "coreclk", "", 0], ["core_temp", "Temperature of each core", "Core temp", "cpu", "coretmp", "", 0], ["core_table", "Each core: usage, clock and temperature", "Cores", "cpu", "coretab", "", 0], ["thread_table", "Each thread: usage and clock (like Afterburner)", "Threads", "cpu", "threadtab", "", 0],
        ["ram", "RAM used", "RAM", "mem", "ram", "MB", 12288], ["commit", "Commit charge", "Commit", "mem", "ram", "MB", 18000], ["proc_ram", "RAM used by the game", "Game RAM", "mem", "ram", "MB", 5400],
        ["fps", "Framerate", "FPS", "game", "fps", "FPS", 143], ["frametime", "Frametime", "Frametime", "game", "fps", "ms", 6.9], ["fps_avg", "Average framerate", "FPS Avg", "game", "fps", "FPS", 139], ["fps_1low", "1% low", "FPS 1% Low", "game", "fps", "FPS", 88], ["fps_01low", "0.1% low", "FPS 0.1% Low", "game", "fps", "FPS", 61], ["fps_min", "Lowest framerate", "FPS Min", "game", "fps", "FPS", 52], ["fps_max", "Highest framerate", "FPS Max", "game", "fps", "FPS", 201],
        ["net_down", "Download speed", "Down", "pc", "net", "MB/s", 4.2], ["net_up", "Upload speed", "Up", "pc", "net", "MB/s", 0.3], ["fan:0", "Fan speed: CPU fan", "CPU fan", "pc", "fan", "RPM", 1180], ["fan:1", "Fan speed: Rear fan", "Rear fan", "pc", "fan", "RPM", 820], ["temp:0", "Temperature: VRM", "VRM", "pc", "btemp", "°C", 48],
        ["app", "Game name", "Game", "info", "title", "", "cs2"], ["res", "Resolution", "Res", "info", "info", "", "2560x1440"], ["api", "Graphics API", "API", "info", "info", "", "D3D12"], ["time", "Clock", "Time", "info", "info", "", "02:42:00 PM"]];
      const st = { layout: "grouped", labels: true, units: "small", align: true, warn: true, heat: false, group_colors: false, fps_lines: false, columns: false, warn_gpu_temp: 85, warn_cpu_temp: 90, warn_fps: 0, cores_per_row: 8, mem_unit: "MB", int_fps: true, ...(settings.osd_style || {}) };
      const GL = { gpu: "GPU", cpu: "CPU", cores: "CPU%", coreclk: "MHz", coretmp: "°C", coretab: "", threadtab: "", vram: "MEM", ram: "RAM", app: "APP", fps: "FPS", net: "NET", fan: "FAN", btemp: "TEMP", title: "", info: "" };
      const SLOT = { gpu: 4, vram: 4, cpu: 5, cores: 5, coreclk: 5, coretmp: 5, coretab: 5, threadtab: 5, ram: 6, app: 6, fps: 7 };
      const part = (d) => {
        let v = d[6], u = d[5];
        if (u === "MB" && st.mem_unit === "GB") { v = (v / 1024).toFixed(1); u = "GB"; } else if (typeof v === "number") v = ["frametime", "net_down", "net_up", "cpu_volt"].includes(d[0]) ? String(v) : String(Math.round(v));
        const num = parseFloat(v);
        let tone = 0;
        if (d[0] === "gpu_temp" || d[0] === "cpu_temp") { const lim = d[0] === "gpu_temp" ? st.warn_gpu_temp : st.warn_cpu_temp; tone = st.warn && num >= lim ? 2 : st.heat && num >= lim - 12 ? 1 : 0; }
        if (st.heat && (d[0] === "gpu_usage" || d[0] === "cpu_usage")) tone = num >= 90 ? 2 : num >= 70 ? 1 : 0;
        if (st.heat && d[0] === "gpu_mem_pct") tone = num >= 92 ? 2 : num >= 78 ? 1 : 0;
        if (d[0] === "gpu_limit") tone = 1;
        return { t: String(v), u: st.units === "off" ? "" : u, tone, pre: "" };
      };
      const picked = (settings.osd_items || []).map((id) => D.find((d) => d[0] === id)).filter(Boolean);
      const per = st.cores_per_row;
      const P = (txt, u, tone = 0) => ({ t: txt, u: st.units === "off" ? "" : u, tone, pre: "" });
      const grid = (label, group, ps) => { const out = []; for (let i = 0; i < ps.length; i += per) out.push({ label: i ? "" : label, group, slot: SLOT[group] || 8, parts: ps.slice(i, i + per) }); return out; };
      const cores = [["P-core 1", 41, 5300, 63], ["P-core 2", 33, 5250, 61], ["P-core 3", 78, 5300, 68], ["P-core 4", 12, 4100, 55], ["P-core 5", 9, 3900, 54], ["P-core 6", 22, 4800, 58], ["E-core 1", 55, 4200, null], ["E-core 2", 18, 4200, null]];
      const multi = (id, label) => {
        if (id === "cpu_cores") return grid(label, "cores", Array.from({ length: 20 }, (_, i) => P(String(Math.round(12 + ((i * 37) % 70))).padStart(3), "")));
        if (id === "core_clock") return grid(label, "coreclk", cores.map((c) => P(String(c[2]).padStart(4), "")));
        if (id === "core_temp") return grid(label, "coretmp", cores.filter((c) => c[3] != null).map((c) => P(String(c[3]).padStart(3), "", st.heat && c[3] >= st.warn_cpu_temp - 12 ? 1 : 0)));
        if (id === "thread_table") return Array.from({ length: 20 }, (_, i) => ({ label: "CPU" + (i + 1), group: "threadtab", slot: 5, parts: [P(String(Math.round(2 + ((i * 37) % 73))).padStart(3), "%"), P(String(3395 + (i > 11 ? 99 : 0)).padStart(4), "MHz")] }));
        return cores.map((c) => ({ label: c[0], group: "coretab", slot: 5, parts: [P(String(c[1]).padStart(3), "%", st.heat && c[1] >= 70 ? 1 : 0), P(String(c[2]).padStart(4), "MHz"), ...(c[3] != null ? [P(String(c[3]).padStart(3), "°C")] : [])] }));
      };
      const isMulti = (id) => ["cpu_cores", "core_clock", "core_temp", "core_table", "thread_table"].includes(id);
      let lines = [];
      if (st.layout === "list") {
        for (const d of picked) { if (isMulti(d[0])) lines.push(...multi(d[0], d[2])); else lines.push({ label: d[4] === "title" ? "" : d[2], group: d[4], slot: SLOT[d[4]] || 8, parts: [part(d)] }); }
      } else if (st.layout === "line") {
        const parts = []; let last = "";
        for (const d of picked) { if (isMulti(d[0])) continue; const p = part(d); if (d[4] !== last) { p.pre = GL[d[4]]; last = d[4]; } parts.push(p); }
        if (parts.length) lines.push({ label: "", group: "info", slot: 8, parts });
      } else {
        const mine = (d) => (isMulti(d[0]) ? "" : ((st.groups || {})[d[0]] || "").trim());
        const key = (d) => (mine(d) ? "u:" + mine(d) : d[4]);
        const order = []; for (const d of picked) if (!order.includes(key(d))) order.push(key(d));
        for (const g of order) {
          if (g.startsWith("u:")) { const ds = picked.filter((d) => key(d) === g); lines.push({ label: g.slice(2), group: ds[0][4], slot: SLOT[ds[0][4]] || 8, parts: ds.map(part) }); continue; }
          if (["cores", "coreclk", "coretmp", "coretab", "threadtab"].includes(g)) { picked.filter((d) => d[4] === g).forEach((d) => lines.push(...multi(d[0], GL[g]))); continue; }
          if (g === "fps" && st.fps_lines) { picked.filter((d) => d[4] === g).forEach((d) => lines.push({ label: d[2], group: g, slot: 7, parts: [part(d)] })); continue; }
          lines.push({ label: GL[g], group: g, slot: SLOT[g] || 8, parts: picked.filter((d) => d[4] === g && !mine(d)).map(part) });
        }
      }
      if (st.columns && st.layout !== "line") {
        const cols = Math.max(0, ...lines.map((l) => l.parts.length));
        for (let c = 0; c < cols; c++) {
          const w = Math.max(0, ...lines.map((l) => (l.parts[c] ? l.parts[c].t.length : 0))), uw = Math.max(0, ...lines.map((l) => (l.parts[c] ? l.parts[c].u.length : 0)));
          lines.forEach((l) => { if (l.parts[c]) { l.parts[c].t = l.parts[c].t.padStart(w); l.parts[c].u = l.parts[c].u.padEnd(uw); } });
        }
      }
      if (!st.labels) lines.forEach((l) => { l.label = ""; l.parts.forEach((p) => (p.pre = "")); });
      if (st.align && st.layout !== "line") { const w = Math.max(0, ...lines.map((l) => l.label.length)); lines.forEach((l) => { if (l.label) l.label = l.label.padEnd(w); }); }
      const ft = Array.from({ length: 600 }, (_, i) => 6.9 + Math.sin(i / 7) * 0.5 + (i % 97 === 40 ? 9 : 0) + (i % 211 === 100 ? 16 : 0));
      return { installed: true, running: true, profiles: true, fps: 143, frametime: 6.9, app: "cs2.exe", frames: 4200, lines,
        ft_recent: ft, graph: ft.slice(-120).map((x, i) => 6.9 + Math.sin(i / 9) * 0.6), stats: { avg: 139.4, min: 52.1, max: 201.7, low1: 88.3, low01: 61.2, secs: 60 },
        cats: [["gpu", "Graphics card"], ["cpu", "Processor"], ["mem", "Memory"], ["game", "Game and frames"], ["pc", "Fans, sensors and network"], ["info", "Info"]],
        items: D.map((d) => ({ id: d[0], label: d[1], short: d[2], cat: d[3], have: d[0] !== "gpu_volt", stat: ["fps_avg", "fps_1low", "fps_01low", "fps_min", "fps_max"].includes(d[0]) })) };
    },
    rtss_restore_look: () => 2,
    rtss_reset_stats: () => null,
    hotkeys_state: () => ({}),
    // keep awake and "shut down when" keep their state, so the Power pages can be tried
    set_awake: (a) => { const w = window.__power.awake; if (a.mode !== undefined) w.mode = a.mode; if (a.minutes) w.until = Math.floor(Date.now() / 1000) + a.minutes * 60; if (a.target) w.app = a.target; if (a.display !== undefined) w.display = a.display; window.__power.awake_active = w.mode !== "off"; return null; },
    set_plan: (a) => { window.__power.plan = { ...a.plan, state: "waiting", created: Math.floor(Date.now() / 1000), progress: a.plan.cond === "downloads" ? "Waiting for a download to start" : "" }; return null; },
    cancel_plan: () => { window.__power.plan = null; return null; },
  };
  function hwState0() { return R.hw_state0(); }
  window.__power = { awake: { mode: "off", until: 0, app: "", display: true }, awake_active: false, plan: null, timers: [], stopwatch: { running: false, acc_ms: 0, laps: [] }, now: Date.now(), idle: 3, win_timeouts: { sleep: 1800, screen: 600 } };
  window.__R = R; window.__settings = settings;
  window.__mockCalls = [];
  window.__TAURI__ = {
    core: { invoke: async (cmd, args) => { window.__mockCalls.push(cmd); if (R[cmd]) return structuredClone(R[cmd](args || {})); return null; }, convertFileSrc: (p) => p },
    // listeners are kept so a test can fire Deck's events: window.__emit("hiding")
    event: { listen: async (name, cb) => { (window.__listeners[name] ||= []).push(cb); return () => {}; } },
  };
  window.__listeners = {};
  window.__emit = (name, payload) => Promise.all((window.__listeners[name] || []).map((cb) => cb({ payload })));
})();
