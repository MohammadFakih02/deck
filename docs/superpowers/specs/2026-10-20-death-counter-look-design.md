# Death counter: the in-game counter and the Deaths page, redone

Date: 2026-10-20. Builds on `2026-10-19-death-counter-design.md` (counting, detection and teaching stay as they are).

## Why

Counting works now (Khazan counts its deaths by itself and teaching learns its death screen), but the user finds the
whole thing "very underwhelming": the in-game counter is one line of grey text, the History tab "looks bad" (a flat
list of lines, each with a Delete button), and there is little to see or do beyond the count. This pass makes the
counter something you enjoy glancing at mid-fight, and makes the page tell you how a session and a boss went.

## Goals

- An in-game counter that reads at a glance and reacts to a death, in a style the user picks, with every part
  switchable.
- A Deaths page that shows sessions, bosses and progress (are my tries getting longer?) instead of raw lines.
- A few features that make the history worth keeping: records, notes per boss, all games together, a share
  picture, export.

## Non-goals

- No change to how deaths are detected or taught, or to game identification (program only, no store ids).
- No boss detection from the screen (reading boss names off health bars), no challenge goals: possible later.
- No knowledge of how far into a fight you got (boss health): Deck only knows *when* things happened. "Better" always
  means "the try lasted longer".

## Anti-goals (would make it a failure even if it works)

- Anything that costs frame rate or stutters a game: the overlay draws only when something changes, plus a short
  animation after a death.
- Anything that steals focus, takes input or shows up in the user's screen captures unless they ask
  (`pill_in_recordings` keeps meaning what it means).
- Merging the page's tabs into one (the user wants tabs kept; see `CLAUDE.md`/memory: never merge tabbed pages).
- Numbers that lie: a stat that Deck can't really know (boss health, "% through the fight") isn't shown.

## Part 1: the in-game counter

Chosen with the user from mockups (`.superpowers/brainstorm/`, not committed): "B while you play, C after you die",
plus two more styles to choose from and an optional red edge.

### While you play: three styles (user picks; default "skull")

- **Skull and number** (default): a dark rounded box, a red skull icon, the count big and bold, the label small
  beside it ("DEATHS" over the boss name, or over the game name outside a fight).
- **Ring**: the count inside a ring that fills toward a mark, the name beside it. The mark is the number of deaths
  the last beaten boss in this game took; with no beaten boss yet, the ring fills once per 10 deaths and starts
  again (builder's call on the exact look of a "lap").
- **One huge number**: a large, bold, slightly see-through number with the label small under it, no box.

### What it counts (user: "game-wise deaths, not just bosses… maybe even 2 counters")

A setting: **this boss**, **the whole game**, or **both**. In a fight, "both" shows the boss count as the main number
and the game total as a smaller second number (in the ring style the ring follows the boss and the game total sits
under it). Outside a fight only the game counter shows, or nothing if "outside a fight" is set to hide.

### After a death: the card (on by default, switchable)

When a death is counted (any source: screen, save, key, pad, page), the counter opens into a card for a few seconds
(setting, 2–10 s, default 5), then closes back to the style with the new number. The card shows, each line
switchable:

- the boss (or game) name and "Try N next";
- the deaths to this boss (the big red number);
- how long the try you just lost lasted;
- time spent on this boss;
- deaths this session (Part 2; "this session" rather than the calendar day, so a session past midnight stays one);
- bars for the last 10 tries, taller = lasted longer, the newest in brighter red;
- a short note when something stands out: "Your longest try yet", "100th death in this game", "Same as your record
  for <boss>"… (from records, Part 3), and the user's own note for the boss if they turned that on (Part 3).

Outside a fight the card shows the game's numbers instead of a boss's. "Already counted" (one death counted twice
within 10 s) keeps flashing as now and does not open a card.

### Red edge (off by default, switchable)

On a death the edges of the game's monitor glow red for about 0.8 s and fade. A separate click-through, topmost,
never-activated layered window over the game's monitor, shown only for the animation; hidden from captures like the
counter unless `pill_in_recordings`.

### Settings (all in the existing "In-game counter" section of the Settings tab)

Existing: on/off, corner, size, opacity, in recordings, show in every game. New: style, what it counts, outside a
fight (game total / hide), the card (on/off, seconds, each line), red edge. A **live preview** next to the settings
shows the counter and the card as they'd look, drawn by the same code that draws them in game (a command returns the
picture), so the preview can't drift from the real thing.

### Look

Follows Deck's theme like the controller overlay does (the theme's panel colour, text colour, accent where it fits),
with the death red as the one strong colour. Drawn with the existing SVG rasteriser (`pad/art.rs`, `raster::svg`).
Size and opacity settings apply to every style and the card. Animation: a short pop on the number when it changes
and a fade between the style and the card; nothing moves while you're just playing.

## Part 2: the Deaths page

Tabs stay: **Now, Bosses, History, Games, Settings**.

### Play sessions (new data, used by Now, History and the card)

Deck records when you play: a session for a game starts when it comes to the front (and is counted) and ends after
it hasn't been in front for 15 minutes (alt-tabbing doesn't split a session). Stored in the game's history file as a
new event, e.g. `{"t": <start>, "ev": "play", "ms": <length so far>}`, written when the session ends and as a
checkpoint every 5 minutes (a Deck crash loses at most 5 minutes; replay keeps the longest line per start). Older
history files without play lines still work: their sessions are worked out from the deaths' times (a gap of more than
an hour starts a new one), marked as estimated.

### Tries

A try is the time from the fight's start or the previous death to a death (or a win), within one fight. Try lengths
feed the bars, "longest try", the Bosses curve and the records. A try that spans a Deck restart or a long gap (more
than 30 minutes without the game in front) isn't counted as a long try.

### Now

- The game's name and whether it's in front; fighting / not fighting.
- Both counters big: this boss (red) and this game.
- The buttons where they are today: +1 death, Undo, Boss defeated / Abandon, start a fight with the name chips.
- The last 10 tries as bars.
- This session: deaths, playing time, how often you're dying (average try length), deadliest boss.
- Teaching and taught death screens stay below, unchanged.

### Bosses: one card per boss instead of the table

Deaths, tries, time on the boss, best (longest) try, beaten or fighting, beaten on which try and when. A small line
chart of try length over the tries ("getting longer = learning it"), red while fighting, green once beaten. Sorted
fighting first, then most recent. The user's note for the boss (Part 3) is on its card.

### History: sessions instead of lines

One card per play session, newest first: when, how long, deaths, bosses fought and beaten, and a small chart of
deaths over the session. The newest session opens; the others fold and open on click. Inside, the events as a
compact timeline ("Beat Blade Phantom after 9 deaths, 34 min", "Died to maluca · seen on screen"), long runs of
deaths folded ("+ 19 more"). Deleting a mistaken line is in a small menu on each line (with the existing
confirmation), not a button on every row.

### Games: all games together

On top: totals across every game (deaths, hours played, bosses beaten), the deadliest games and bosses, and deaths
per week for the last few months. Below: the game list as now (Show, the shared library, import, presets).

## Part 3: features

- **Records and milestones** (per game, from the history): longest try, fewest deaths to beat a boss, deadliest boss,
  most deaths in a session; milestones at the 1st, 10th, 50th, 100th, then every 100th death of a game. They feed
  the card's note and a "Records" row on Now.
- **Notes per boss**: a short text per boss (Bosses card, editable), optionally shown on the card after a death.
  Stored per game next to its history (builder's call on the file).
- **Share picture**: on a boss card and on Now, "Share" draws a picture of the stats (boss or whole game) with the
  same rasteriser and copies it to the clipboard, with "Save as…" next to it. No upload.
- **Export**: on Games, "Export" saves a game's history (or all) as a CSV: time, game, event, boss, source.

## Constraints

- Everything in `CLAUDE.md` for this repo: docs before every commit, no personal data, no local paths in the repo,
  throttled builds, no windows popping up, the counter never takes focus or input.
- The history file format only grows (new event kinds); old files keep working. Old Deck versions already skip
  lines they can't read (`store.rs` reads with `filter_map(.. .ok())`); an older version that deletes a line rewrites
  the file without the play lines, which is accepted (only on a downgrade).
- Settings are added to `DeathSettings` with defaults so existing settings files load unchanged.
- The UI preview harness (`tools/testing/ui-preview`) and its mock data get the new page so `regress.mjs` and
  `a11y.mjs` cover it.

## Decided by Claude (the user said "do whatever you think is best")

- Which page ideas and features: all three page redesigns; records, notes, all-games, share picture, export.
  Left out: named areas for deaths outside bosses (the game counter covers it), challenge goals, boss names from the
  screen.
- Red edge off by default; the card on by default for 5 s; the default style is "skull and number"; "both" counters
  is the default for what it counts.
- Sessions split after 15 minutes away; estimated sessions for old history.
- The ring's mark (last beaten boss's deaths, laps of 10 otherwise).

## Left to the builder

Exact sizes, spacing and icon shapes; the chart drawing on the page (inline SVG, no chart library); file names for
notes; how the preview command caches; animation frame timing within the stated limits.
