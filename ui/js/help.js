// What the "?" next to a setting explains (see helpTip in core.js).
// Each entry: t = title, what = what it does, why = when you'd want it,
// change = what happens when you move it, warn = the catch, if there is one.

const HELP = {
  // ------------------------------------------------------------------ CPU page
  "cpu.vitals": {
    t: "Live readings",
    what: "The CPU's clock (the fastest core right now), package temperature, total load and package power, read once a second. The little graphs show the last minute.",
    why: "Check a profile does what you meant: a cap should keep Clock under it, a power limit should keep Power under it.",
    change: "Nothing to change here. The readings come from the hardware engine, so some of them need Deck to run as admin.",
  },
  "cpu.profiles": {
    t: "CPU profiles",
    what: "A profile is a Windows power plan plus Deck's extras (power limits, a minimum clock, a turbo overclock, Game Mode). Click a card to switch to it.",
    why: "One click (or one key) between quiet-and-cool for the desktop and everything-on for games, without digging through Windows' power options.",
    change: "Switching changes the active power plan straight away and applies the extras. Editing the active profile applies the change when you save.",
  },
  "cpu.adopt": {
    t: "Use my existing plans",
    what: "Turns power plans already on this PC into Deck profiles. The plans themselves aren't changed or copied.",
    why: "If you already made plans (or a tool like ThrottleStop or a game installer did), you keep them and their shortcuts.",
    change: "Only adds cards to Deck. Windows' own plans (Balanced, High performance…) aren't ticked for you.",
  },
  "cpu.clockcheck": {
    t: "In-game clock check",
    what: "While a game is in front and a capped profile is on, Deck records the real CPU clock: the average and the peak.",
    why: "Some games (and Windows Game Mode) push the CPU past a power-plan cap. This shows whether your cap really held, without alt-tabbing out.",
    change: "Nothing to set. \"Above the cap\" means the average went more than 5% over it: turn on \"Hold the cap in games\" in the profile.",
  },
  "cpu.ram": {
    t: "Clear RAM",
    what: "Asks every program to hand back memory it isn't using, and empties Windows' standby cache (files kept in memory in case they're needed again).",
    why: "Before a game on a PC that's short on memory, or when a game stutters after hours of browsing.",
    change: "Free memory goes up at once. Programs reload what they need, so the first moments after can be a little slower. Nothing is closed and no work is lost.",
  },

  // ------------------------------------------------------------- CPU editor
  "cpu.cap": {
    t: "Max clock",
    what: "The highest clock Windows lets the cores run at. It's the power plan's \"Maximum processor frequency\".",
    why: "A cap makes the CPU cooler, quieter and less power-hungry. Most games barely change at a sensible cap, because they're limited by the graphics card.",
    change: "Lower: less heat, fan noise and power; less speed in work that needs the CPU (compiling, rendering, CPU-heavy games). \"No cap\" lets it boost as far as it can.",
    warn: "Windows Game Mode can lift this cap inside games. Turn on \"Hold the cap in games\" below to stop that.",
  },
  "cpu.cap_p": {
    t: "P-cores max",
    what: "The highest clock for the performance cores, the big cores that run games and anything in front.",
    why: "This is the cap that decides heat and power in games. Capping a few hundred MHz below the top turbo often saves a lot of power for very little speed.",
    change: "Lower: cooler and quieter, slower when the CPU is the bottleneck. \"No cap\" lets them boost freely.",
    warn: "Windows Game Mode can lift this cap inside games. Turn on \"Hold the cap in games\" below to stop that.",
  },
  "cpu.cap_e": {
    t: "E-cores max",
    what: "The highest clock for the efficiency cores, the small cores Windows uses for background work.",
    why: "Background work rarely needs speed, so a low cap here saves power and heat almost for free.",
    change: "Lower: less power from background tasks. Too low can make heavy multi-core work (compiling, rendering, shader compiles) slower.",
  },
  "cpu.turbo": {
    t: "Turbo boost",
    what: "How the CPU boosts above its base clock (Windows' \"processor performance boost mode\").",
    why: "Off is the strongest cooling setting there is. Efficient boosts only when it pays off, a middle ground for laptops and quiet desktops.",
    change: "Off: the CPU stays at its base clock: much cooler, much slower. On: boosts when there's work. Aggressive: boosts sooner and harder (what most plans use). Efficient / Efficient aggressive: the same, but only when the boost is worth the power.",
  },
  "cpu.max_state": {
    t: "Max processor state",
    what: "The top speed Windows asks for, as a share of the CPU's full speed.",
    why: "An old cooling trick: 99% switches turbo off on most CPUs while everything else stays normal.",
    change: "100%: full speed. Below 100% lowers the ceiling; on most CPUs anything under 100% also turns turbo off.",
  },
  "cpu.min_state": {
    t: "Min processor state",
    what: "The lowest speed Windows lets the CPU drop to when there's little to do.",
    why: "Leave it low (5% is Windows' default) for a cool, quiet idle. Raising it can make the PC feel a touch snappier.",
    change: "Higher: the CPU idles faster and warmer, and uses more power doing nothing. It never limits top speed.",
  },
  "cpu.min_cores": {
    t: "Cores kept awake",
    what: "The share of cores Windows never parks (puts into deep sleep) when the PC is quiet.",
    why: "Parked cores take a moment to wake. Keeping more awake can smooth out stutters in some games; keeping fewer saves idle power.",
    change: "Higher: quicker response to sudden work, more idle power and heat. 100% means no core is ever parked.",
  },
  "cpu.epp": {
    t: "Energy preference",
    what: "A hint to the CPU's own boost logic (Intel Speed Shift, AMD CPPC) on whether to favour speed or saving power.",
    why: "It changes how eagerly the CPU climbs to high clocks, often a bigger effect on heat and battery than the caps.",
    change: "0: ramps up instantly, hottest. 100: ramps up slowly, coolest. \"Plan's own\" leaves the power plan's value.",
  },
  "cpu.power": {
    t: "Power limits",
    what: "How many watts the CPU package may draw, set in the CPU itself (Intel's PL1 and PL2).",
    why: "The most direct way to control heat: a CPU can't run hotter than its watts allow, whatever the clock says.",
    change: "Stock: the motherboard's limits. Saver: 15 W sustained, 25 W bursts, and Windows set to best efficiency. Performance: the motherboard's maximum limits and Windows set to best performance. Custom: your own numbers. Deck puts the limits back if the firmware resets them.",
    warn: "Needs Deck to run as admin.",
  },
  "cpu.pl1": {
    t: "Sustained power (PL1)",
    what: "The wattage the CPU may draw for as long as it likes.",
    why: "This decides heat and noise under long loads: games, rendering, compiling.",
    change: "Lower: cooler, quieter, slower under long loads. Higher than your cooler can handle just means the CPU runs into its temperature limit instead.",
  },
  "cpu.pl2": {
    t: "Burst power (PL2)",
    what: "The wattage the CPU may draw for short bursts (tens of seconds) before it drops to the sustained limit.",
    why: "Bursts make the PC feel quick when you open things. Lowering it stops short heat spikes and fan revs.",
    change: "Lower: fewer fan spikes, slower bursts. It's always kept at least as high as the sustained limit.",
  },
  "cpu.min_lock": {
    t: "Keep the clock at least",
    what: "Raises the power plan's minimum so the cores never drop below this clock.",
    why: "Some games stutter while the CPU ramps up from a low clock. Holding it up removes that ramp.",
    change: "Higher: smoother in those games, more heat and power at idle. Deck releases it by itself if the CPU passes 92 °C, and puts the plan's own minimum back when you switch away.",
    warn: "Needs Deck to run as admin.",
  },
  "cpu.oc": {
    t: "Overclock turbo",
    what: "Raises the CPU's turbo ratio limit, so every core may boost higher than stock.",
    why: "More speed in CPU-limited games and work, on unlocked Intel CPUs (K and KF models).",
    change: "Higher: faster, hotter, more power. It's gone after a restart and Deck applies it again when the profile is switched on, but not after an unexpected restart, in case the overclock caused it.",
    warn: "Too high can crash or freeze the PC. Go up in small steps and test; if the PC restarts by itself, turn this off first.",
  },
  "cpu.hold": {
    t: "Hold the cap in games",
    what: "Turns Windows Game Mode off while this profile is on.",
    why: "Game Mode raises the CPU's limits inside games, which lifts the caps you set here.",
    change: "On: the caps hold in games. Game Mode comes back by itself when you switch to a profile without a cap.",
  },

  // ------------------------------------------------------------------ GPU page
  "gpu.vitals": {
    t: "Live readings",
    what: "The card's core and memory clocks, board power, temperature, fan speed, core voltage and load, read once a second.",
    why: "Check a profile on the card: an undervolt should show a lower voltage at a similar clock, a power limit a lower wattage.",
    change: "Nothing to change. The chip at the top says what's holding the card back right now: the power limit, temperature or the voltage limit.",
  },
  "gpu.profiles": {
    t: "GPU profiles",
    what: "A profile is a set of card settings: power limit, clocks or a voltage curve, and the fan. Click a card to put it on the graphics card.",
    why: "Switch between a quiet desktop setup and a game setup with one click or one key, without MSI Afterburner.",
    change: "Switching applies the profile to the card at once; nothing restarts.",
  },
  "gpu.stock": {
    t: "Back to stock",
    what: "Puts the card back to NVIDIA's defaults: power limit, clocks, voltage curve and fan control.",
    why: "When something feels unstable, or to compare against the card's normal behaviour.",
    change: "Applies at once. Your profiles are kept.",
  },
  "gpu.startup": {
    t: "Apply a profile at start",
    what: "The profile Deck puts on the card when Deck starts with Windows.",
    why: "Card settings are lost at every restart. This brings your usual setup back without a click.",
    change: "\"None\" leaves the card at stock until you pick a profile. A profile with an overclock isn't applied after an unexpected restart, in case it caused it.",
  },
  "gpu.import": {
    t: "Import from Afterburner",
    what: "Copies MSI Afterburner's profiles (with curves and Ctrl+Alt keys), and if you want, its startup profile and overlay items.",
    why: "Move over from Afterburner without setting everything up again.",
    change: "Only adds profiles in Deck; Afterburner's own files aren't changed.",
  },

  // ------------------------------------------------------------- GPU editor
  "gpu.power": {
    t: "Power limit",
    what: "How much power the card may draw, as a share of its default board power.",
    why: "The easiest way to make a card cooler and quieter. Many cards lose only a few percent of speed at 80%.",
    change: "Lower: less heat, noise and power, a little less speed in heavy games. Higher (if the card allows it): room for an overclock to hold its clocks.",
  },
  "gpu.temp": {
    t: "Temperature limit",
    what: "The temperature the card slows itself down to stay under.",
    why: "Keeps a card in a small or hot case from running near its maximum.",
    change: "Lower: the card holds a cooler temperature by giving up clock when it gets warm. \"Card's own\" uses NVIDIA's default.",
  },
  "gpu.clockmode": {
    t: "Offset or curve",
    what: "Offset moves the card's whole clock curve up or down by one amount. Curve lets you set the clock at each voltage.",
    why: "Offset is simple and enough for an overclock. Curve is for undervolting: the same clock at a lower voltage, so the card runs cooler and quieter.",
    change: "Switching to Curve starts from the current offset. Switching back to Offset drops the curve.",
  },
  "gpu.core": {
    t: "Core clock offset",
    what: "Adds (or takes away) this many MHz at every point of the card's boost curve.",
    why: "A small overclock for a few more frames, or a negative offset for a cooler card.",
    change: "Higher: faster, until it isn't stable: then games crash, show glitches or the driver resets. Go up 15 to 30 MHz at a time and test a game each step.",
    warn: "Use Try it first: if it crashes, Cancel puts the old settings back.",
  },
  "gpu.mem": {
    t: "Memory clock offset",
    what: "Adds this many MHz to the video memory's clock.",
    why: "Helps games that move a lot of data (high resolutions, big textures).",
    change: "Higher: more bandwidth, until errors start. Newer memory corrects its own errors, so too high often shows as lower frame rates rather than a crash: compare a benchmark after each step.",
  },
  "gpu.volt": {
    t: "Core voltage boost",
    what: "Lets the card use the top voltage steps NVIDIA allows, which it normally holds back.",
    why: "Can help an overclock hold a higher clock. It rarely helps on its own.",
    change: "Higher: a little more clock headroom, noticeably more heat and power. NVIDIA's own safety limit still applies.",
  },
  "gpu.fan": {
    t: "Fan",
    what: "Who controls the card's fans: the driver's own curve, a fixed speed, or your curve.",
    why: "A custom curve can keep the card quieter at the desktop and cooler in games than the stock behaviour.",
    change: "Driver: NVIDIA's default. Fixed: always this speed. Curve: Deck sets the speed from the GPU temperature every second. The fan goes back to the driver when Deck quits.",
  },
  "gpu.fan_curve": {
    t: "Fan curve",
    what: "Fan speed (up) against GPU temperature (across). The marker shows the temperature now.",
    why: "Flat and low at desktop temperatures for silence, rising steeply near the temperatures games reach.",
    change: "Drag points to change it; Deck follows it every second. The card's minimum fan speed still applies.",
  },
  "gpu.curve": {
    t: "Voltage / frequency curve",
    what: "Each point is a voltage and the clock the card runs at that voltage. The card picks a point by load and temperature.",
    why: "Undervolting: hold a good clock from a lower voltage up, so the card never reaches its top voltages. Same frames, less heat, quieter fans.",
    change: "A point raised too far for its voltage is unstable (crashes, driver resets). Try it before saving: Cancel puts the old settings back.",
  },
  "gpu.flat": {
    t: "Hold a clock from a voltage",
    what: "From the voltage you give up, the curve runs at the clock you give: the flat-line undervolt Afterburner users do by hand.",
    why: "The quickest undervolt. A common start is the card's normal game clock at around 850 to 900 mV.",
    change: "Apply sets the curve. \"Flatten there\" keeps the clock the curve already has at that voltage. Test in a game: if it crashes, raise the voltage a step or lower the clock.",
  },
  "gpu.shift": {
    t: "Shift the curve",
    what: "Moves the whole curve up (+) or down (−) by this many MHz.",
    why: "Fine-tunes an undervolt or overclock without reshaping it.",
    change: "Up: faster, less stable. Down: slower, more stable.",
  },
  "gpu.lock": {
    t: "Lock point",
    what: "Keeps the card at the selected point's voltage and clock, whatever the load.",
    why: "For testing one point's stability, or holding a steady clock in a benchmark.",
    change: "The card stays at that clock even at the desktop, so it runs warmer. Unlock to let it move again.",
  },
  "gpu.try": {
    t: "Try it",
    what: "Puts these settings on the card now without saving them.",
    why: "Test a change in a game before keeping it. You can leave Deck open in the background while you play.",
    change: "Cancel (or closing the editor) puts the card back the way it was. Save keeps it.",
  },

  // --------------------------------------------------------- both editors
  "prof.hotkey": {
    t: "Keyboard shortcut",
    what: "A key combination that switches to this profile from anywhere, even inside a game.",
    why: "Change profile without leaving the game.",
    change: "Use Ctrl, Alt or Win with a key. If another program already uses it, the chip turns yellow and Deck keeps trying.",
  },
  "prof.links": {
    t: "Shortcuts",
    what: "A desktop or Start menu shortcut that switches to this profile.",
    why: "Switch from the Start menu or a stream deck without opening Deck.",
    change: "Saving adds or removes the shortcut. Deleting the profile removes its shortcuts too.",
  },
  // ------------------------------------------------------------------ Fans
  "fans.mode": {
    t: "Fan mode",
    what: "Who decides this fan's speed. BIOS: the motherboard, as if Deck weren't there. Fixed: one speed all the time. Curve: a speed that follows a temperature.",
    why: "Curves keep the PC quiet at the desktop and cool in games. Fixed is handy for testing or for a pump that should always run fast.",
    change: "Takes effect when you press Apply. Fans go back to the BIOS when Deck quits, so a wrong choice never sticks. Whatever the curve says, curve fans run at 100% once the CPU reaches 85 °C.",
    warn: "A CPU or pump fan set too low can let the CPU overheat. Keep the curve's top end at 100%.",
  },
  "fans.follows": {
    t: "Follows",
    what: "The temperature this fan's curve reads: the CPU, the graphics card, the hotter of the two, their average, or a board sensor.",
    why: "Case fans that follow the hotter of CPU and GPU help in games, where the graphics card makes most of the heat.",
    change: "The marker on the curve moves to the new temperature and the fan follows it from the next second.",
  },
  "fans.curve": {
    t: "Fan curve",
    what: "Temperature (across) against fan speed (up). Between the points the speed changes smoothly. The marker shows where the fan is right now.",
    why: "Flat and low on the left keeps it quiet at the desktop; steep on the right makes the fans react fast when it gets hot.",
    change: "Drag a point, click empty space to add one, right-click a point to remove it. Fans using this curve follow it after Apply.",
  },
  "fans.hyst": {
    t: "Slow down only after it drops",
    what: "Fans speed up straight away when it gets warmer, but only slow down once the temperature has fallen this many degrees.",
    why: "Stops the fans going up and down every few seconds when the temperature hovers around a point on the curve.",
    change: "Higher: steadier and a little louder for longer. 0: the fans follow every degree.",
  },
  "fans.delay": {
    t: "Wait between slow-downs",
    what: "After slowing down, the fans wait this long before they may slow down again. Speeding up is never delayed.",
    why: "Short bursts of load (opening an app, a loading screen) don't make the fans pulse.",
    change: "Longer: calmer sound, the fans stay faster a little longer after the load ends.",
  },
  "fans.stop": {
    t: "Stop system fans below",
    what: "Case fans on a curve stop completely while their temperature is under this. Headers named CPU, pump or AIO never stop.",
    why: "A silent PC when it's idle and cool.",
    change: "0 turns it off. A stopped fan starts again 3 °C above the limit, and at once if the CPU reaches 85 °C.",
    warn: "Some fans need a moment and a push to start again. Check yours do before relying on it.",
  },
  "fans.experimental": {
    t: "Unverified board",
    what: "Deck recognises this fan chip and reads it, but writing fan speeds to it hasn't been checked on this exact board.",
    why: "Turn it on to control the fans anyway. Most boards with this chip work the same way.",
    change: "If the fans behave oddly, turn it off or quit Deck: the fans go straight back to the BIOS.",
  },

  // -------------------------------------------------------------- Lighting
  "light.effect": {
    t: "Effect",
    what: "What every light (board, ARGB strips, RAM) shows. Some effects use your colour, some make their own (Rainbow, Temperature, Screen, Music).",
    why: "One place for all your RGB instead of a program per brand.",
    change: "Sent to the lights when you press Apply. Off darkens them but leaves the board's own settings alone.",
  },
  "light.bright": {
    t: "Brightness",
    what: "How bright the lights are, from off to full.",
    why: "Dimmer is easier on the eyes at night and some strips look washed out at full.",
    change: "All lights change together after Apply.",
  },
  "light.speed": {
    t: "Speed",
    what: "How fast the effect moves: breathing, waves, comets and colour cycles.",
    why: "Slow looks calm; fast looks busy and draws the eye.",
    change: "Takes effect after Apply. It doesn't change Static or Off.",
  },
  "light.music": {
    t: "Music sensitivity",
    what: "How strongly the lights react to the sound playing on the PC.",
    why: "Quiet music needs more; loud music with everything flashing needs less.",
    change: "Higher: brighter and jumpier at the same volume.",
  },
  "light.auto_game": {
    t: "Game effect while a game is in front",
    what: "When a game is the front window, the lights switch to the Game effect: they flash with the game's sound (shots, explosions).",
    why: "Lights that react to the game without you switching anything.",
    change: "Your normal effect comes back when the game is closed or you switch away from it.",
  },
  "light.game_only": {
    t: "Only the game's sound",
    what: "The Game effect listens to the game alone, not to music or voice chat playing at the same time.",
    why: "Otherwise Discord voices and Spotify make the lights flash too.",
    change: "With it off, the lights follow all the sound on the PC.",
  },
  "light.zones": {
    t: "Where",
    what: "Which lights Deck drives: the board's 12 V RGB header, each ARGB header and the RAM.",
    why: "Leave out lights another program controls, or ones you want dark.",
    change: "A light you turn off here keeps whatever it last showed.",
  },
  "light.leds": {
    t: "LEDs per strip",
    what: "How many LEDs are on each ARGB header, counting every fan and strip chained on it.",
    why: "Waves and comets need it to run smoothly along the whole chain.",
    change: "Too low: the end of the chain stays dark. Too high: the effect runs off the end and looks slow.",
  },

  // -------------------------------------------------------- Cooler display
  "lcd.show": {
    t: "What the display shows",
    what: "Sensors (CPU and GPU temperature and load), a clock, scrolling text, a design you paint, a music meter, or several of them in turn.",
    why: "The display is the most visible part of the PC; make it useful.",
    change: "The preview shows the result straight away; the cooler follows after Apply.",
  },
  "lcd.colors": {
    t: "Colours",
    what: "How the numbers and shapes are coloured: by temperature, by load, one colour of your choice, breathing, or following the lighting.",
    why: "By temperature turns the display into a warning light you see from across the room.",
    change: "After Apply. Following the lighting keeps the display matched to your RGB effect.",
  },
  "lcd.bright": {
    t: "Display brightness",
    what: "How bright the cooler's display is.",
    why: "Full brightness can light up a dark room.",
    change: "After Apply. Night dimming below can lower it on a schedule.",
  },
  "lcd.night": {
    t: "Dim at night",
    what: "Between the two hours the display uses the night brightness instead.",
    why: "A bright display is distracting in a bedroom at night.",
    change: "Works across midnight (for example 23:00 to 07:00).",
  },
  "lcd.flash": {
    t: "Flash when hot",
    what: "The whole display flashes red when the CPU reaches the temperature you set, whatever it was showing.",
    why: "A warning you can't miss if a fan stops or a game pushes the CPU too far.",
    change: "It stops flashing as soon as the CPU cools below the limit.",
  },

  // ---------------------------------------------------------------- Sensors
  "sens.alerts": {
    t: "Heat alerts",
    what: "A Windows notification when the CPU or graphics card goes above the temperature you set.",
    why: "Catches a stopped fan, a clogged filter or a runaway overclock before it causes crashes.",
    change: "At most one notification every five minutes per part, so a long hot game doesn't spam you.",
  },
  "sens.csv": {
    t: "Log to a file",
    what: "Every reading on the All readings tab is written once a second to monitor.csv, which Excel and graphing tools open.",
    why: "To find out what happened before a crash, or to compare two settings over a whole gaming session.",
    change: "The file starts over at 50 MB so it can't fill the disk.",
  },
  "sens.power": {
    t: "Power",
    what: "The CPU package power (from its energy counters) and the graphics card's board power, once a second for five minutes.",
    why: "Shows what a power limit or undervolt really changed, and how much the PC draws in a game.",
    change: "Nothing to change here. CPU power needs Deck's admin set-up.",
  },
  "sens.frames": {
    t: "Frametimes",
    what: "How long each frame of the game took to draw, from RivaTuner, with the average framerate and the 1% and 0.1% lows Deck counts from them.",
    why: "The average hides stutter; the lows and the spikes in this graph show it. A smooth game has a flat line.",
    change: "Needs RivaTuner running and a game it draws in. Counted while the overlay is on or this page is open.",
  },

  // ---------------------------------------------------------------- Overlay
  "ov.items": {
    t: "What to show",
    what: "The readings on the overlay, top to bottom (or left to right on one line). Tick several to move, group or remove them together.",
    why: "Grouping puts readings you pick on one line with your own label, for example GPU and CPU temperatures on a TEMP line.",
    change: "The preview and the game update straight away. Shift-click ticks everything between two items.",
  },
  "ov.layout": {
    t: "Layout",
    what: "Grouped: a line per part (GPU, CPU, FPS). List: a line per reading. One line: everything in a row.",
    why: "Grouped is like Afterburner; one line takes the least room over a game.",
    change: "Lines you made with \"Put on one line\" stay together in Grouped and One line.",
  },
  "ov.fps_lines": {
    t: "Framerate figures on their own lines",
    what: "Each of FPS, average, lowest, highest and the lows gets its own labelled line instead of sharing the FPS line.",
    why: "Easier to read at a glance when you show several framerate figures.",
    change: "Only in the Grouped layout.",
  },
  "ov.columns": {
    t: "Tidy columns",
    what: "Pads the numbers so they line up under each other, with units in their own column.",
    why: "Numbers that change width (9 to 10) stop the line jumping around.",
    change: "Best with a monospace font such as Unispace or Consolas.",
  },
  "ov.decimals": {
    t: "Decimals in the framerate",
    what: "Shows 143.4 instead of 143.",
    why: "Useful when comparing settings closely; distracting otherwise.",
    change: "Also applies to the average and the lows.",
  },
  "ov.heat": {
    t: "Colour by load or heat",
    what: "Numbers turn the warm colour from 70% load (or 12 °C under the limit) and the hot colour from 90% (or the limit).",
    why: "See trouble out of the corner of your eye without reading the numbers.",
    change: "Uses the limits under Warnings.",
  },
  "ov.position": {
    t: "Position",
    what: "The corner of the screen the overlay sits in, and how far it is from the edges.",
    why: "Keep it clear of the game's own map, health bar or chat.",
    change: "RivaTuner moves it at once in running games.",
  },
  "ov.zoom": {
    t: "Text size",
    what: "RivaTuner's zoom: how many screen pixels each pixel of the text takes.",
    why: "Bigger on 4K or TV screens, smaller on a laptop.",
    change: "Larger text covers more of the game.",
  },
  "ov.font": {
    t: "Font",
    what: "The font RivaTuner draws the overlay with. Any font installed on this PC.",
    why: "Monospace fonts (Unispace, Consolas) keep the numbers from jumping as they change.",
    change: "Written to RivaTuner's profile; the preview approximates it.",
  },
  "ov.graph": {
    t: "Frametime graph",
    what: "A line of how long the game's frames take, drawn by RivaTuner under the overlay text. Deck feeds it a steady number of points a second, so it scrolls at the same pace at 60 or 1000 FPS.",
    why: "Stutter shows up as spikes even when the framerate number looks fine.",
    change: "Width and height are in characters and lines of the overlay text. Graph top auto follows the slowest point.",
  },
  "ov.graph_fit": {
    t: "Graph width",
    what: "Fit makes the graph exactly as wide as the overlay's longest line; Set it uses the width you choose.",
    why: "A fitted graph lines up with the text and stays tidy when you add or remove readings.",
    change: "With Fit, adding a long line (the clock, a core table) widens the graph too.",
  },
  "ov.graph_secs": {
    t: "Graph shows",
    what: "How much time the graph spans from its left edge to its right edge.",
    why: "5 s shows each hitch clearly; a minute shows trends, like the game getting slower over time.",
    change: "Longer spans scroll slower. The graph always has 120 points.",
  },
  "ov.graph_smooth": {
    t: "Smooth the line",
    what: "On: each point is the average frametime of the frames in it. Off: each point is the slowest frame in it.",
    why: "Smooth is calmer to look at; off is better for hunting stutter, since a single bad frame still makes a spike.",
    change: "Takes effect from the next point; the line already drawn stays as it was.",
  },
  "ov.reset": {
    t: "Start the figures over",
    what: "Clears the average, 1% and 0.1% lows, lowest, highest and the graph, and counts again from that moment.",
    why: "Measure just one match, one benchmark run or one setting, without the loading screen or the menu in the numbers.",
    change: "The key works in games while profile shortcuts are on. The figures also start over by themselves when you switch to another game.",
  },
  "ov.color_bar": {
    t: "Frame time bar",
    what: "A thin coloured strip along the edge of the screen that changes colour on every frame.",
    why: "Used with capture cards and frame analysis tools to spot dropped or repeated frames.",
    change: "Most people leave it off.",
  },
  "ov.all_games": {
    t: "Every game",
    what: "Deck also writes the look into each game profile RivaTuner has made, not just the global one.",
    why: "A game with its own RivaTuner profile ignores the global look, so it would keep the old style.",
    change: "Your original profiles are kept in RivaTuner\\Profiles\\Deck backup, and \"Put RivaTuner's look back\" restores them.",
  },
  "ov.warn": {
    t: "Warnings",
    what: "A number turns the hot colour when a temperature reaches its limit, or the framerate falls below yours.",
    why: "Spot overheating or a framerate drop without staring at the numbers.",
    change: "The temperature limits also set where the warm colour starts when colouring by heat.",
  },
  "ov.update": {
    t: "Update every",
    what: "How often Deck sends new readings to the overlay.",
    why: "¼ s feels live; 1 s is easier to read and costs a little less.",
    change: "The framerate and frametime numbers are filled in by RivaTuner itself, at its own pace.",
  },
  "ov.stats": {
    t: "Average and lows cover",
    what: "How far back the average, lowest, highest, 1% low and 0.1% low look, counted from every frame RivaTuner draws.",
    why: "A short window shows the current scene; a long one is better for comparing settings.",
    change: "The figures start over when you switch to a different game.",
  },
  "ov.mahm": {
    t: "Share readings like Afterburner",
    what: "Deck publishes its readings the way MSI Afterburner does, in memory other programs can read.",
    why: "RivaTuner's OverlayEditor, stream widgets and other tools that expect Afterburner keep working without it.",
    change: "Turn it off if Afterburner itself is running, so the two don't fight.",
  },

  // ------------------------------------------------------------- Game boost
  "boost.power": {
    t: "Fastest power plan",
    what: "Switches Windows to its fastest power plan while Full power is on and back to yours after.",
    why: "Stops the CPU dropping its clock between frames, which can cause small stutters.",
    change: "More power use and heat while it's on. Replaced by a CPU profile if you pick one.",
  },
  "boost.priority": {
    t: "Slow down background apps",
    what: "Apps without a window get low CPU priority while Full power is on.",
    why: "When something in the background works hard (an update, an indexer), the game still gets the CPU first.",
    change: "Everything goes back to normal priority afterwards. Voice chat and music players are left alone.",
  },
  "boost.trim": {
    t: "Free memory",
    what: "Asks Windows to move memory that background apps aren't using out of the way.",
    why: "More room for the game on a PC with 8 or 16 GB.",
    change: "Nothing closes. A background app may be a little slow the first time you go back to it.",
  },
  "boost.auto_end": {
    t: "Turn off by itself",
    what: "Full power ends about a minute after your game closes.",
    why: "So you don't leave the PC in game mode with apps closed.",
    change: "Off: it stays on until you turn it off.",
  },

  // ---------------------------------------------------------------- Settings
  "set.fullscreen": {
    t: "Hot corner over fullscreen apps",
    what: "Whether the hot corner works while a game or video fills the screen.",
    why: "Off avoids opening Deck by accident when you flick the mouse into a corner mid-game.",
    change: "The keyboard shortcut still works either way.",
  },
  "set.clip_paste": {
    t: "Clicking pastes",
    what: "Clicking an item in the clipboard history hides Deck and pastes it into the app you were using.",
    why: "One click instead of copy, switch back, Ctrl+V.",
    change: "Off: a click only copies it, and you paste yourself.",
  },
  "set.hotkeys": {
    t: "Profile shortcuts",
    what: "Whether the keys you gave CPU and GPU profiles and the overlay work.",
    why: "Turn them off if they clash with a game's own keys.",
    change: "The keys you set are kept and come back when you turn this on again.",
  },
};
