# Death counter — design

Date: 2026-10-19. Status: agreed direction; stage 1 being built. Background, research and the
council's findings: `My-apps\DECK-GAMING-PLAN.md` §4 (kept outside the repo).

## Intent

Count how often you die in hard games, per boss, with as little effort as possible. It must
work in **any** game (the user rejected an Elden-Ring-only scope) and be **smart about the
games people actually die in**: a catalogue of 100+ known games that Deck recognises on its own,
with the best way of counting for each.

## Goals

- Every death counted, per game and per boss, with attempts, wins, time and deaths before the
  first kill. Every count can be undone.
- Counting needs no effort in known games (save file or screen), little effort elsewhere
  (hotkey, controller combo, tray).
- The count is visible in-game on Deck's own pill, without alt-tabbing.
- Nothing is lost: a crash, a kill of Deck or a reboot mid-fight changes nothing.

## Non-goals

- Reading or writing game memory, injecting into games, hooking input. Anti-cheat
  (Easy Anti-Cheat, Vanguard, BattlEye) must have nothing to object to.
- Writing save files. Saves are only ever read, from a copy.
- Streaming integrations, charts, shareable cards, audio detection (cut by the council).
- Boss names recognised from the screen (later, optional: OCR pre-fill you confirm).

## Anti-goals (failure even if it works)

- **Wrong counts.** A counter that miscounts gets turned off. Automatic counting must be
  precise; when unsure it doesn't count.
- **Double counts.** Two automatic sources must never both count the same death. A death from
  one way of counting within 10 s of one from another, where one of the two is automatic (your +1
  key while the screen also saw it), counts once and the pill says "Already counted" (2026-10-19
  review). Two presses of your own keys are two deaths.
- **Cost in games.** Under 1 % of the CPU in total; nothing at all while no tracked game is in front.
- **Windows popping up** in front of a game.

## Constraints

- Lives in Deck (Tauri 2 + Rust), public repo, used by friends: nothing specific to one PC.
  The game catalogue is product data, not personal data, so game names in it are fine
  (my call; the old "no game names" note in the private plan was about the user's own games).
- Deck runs elevated: files go through `util::write_atomic`.
- Main interaction is the tray + a page; hotkeys and a controller combo are **opt-in extras**
  (the user asked for both on 2026-10-19), off by default.

## How it works

### Games: recognising them

- Every second while counting is possible, the tracker looks at the program in front
  (`hw::game::foreground_game()` → pid → exe path).
- **Every game's key is its exe** (2026-10-19, user: "just use the exe, even for Steam games"): `exe:<stem>`
  without `.exe` and Unreal's `-Win64-Shipping`/`-WinGDK-Shipping`, so every store's copy is the
  same game. The store folder below only names it.
- **Steam games** are recognised by their install folder: Deck reads Steam's
  `libraryfolders.vdf` and `appmanifest_<appid>.acf` files (installdir → app id, name), so any
  Steam game gets its real name. Key: `steam-<appid>`.
- **Other games** by exe: key `exe-<stem>`, name from the catalogue or the window title.
  Emulators are matched by exe + window title (shadPS4 "Bloodborne", RPCS3 "Demon's Souls").
- **The catalogue** (`deaths/catalog.json`, embedded): 100+ games from Steam's Souls-like,
  Boss Rush, Precision Platformer, Action Roguelike, Metroidvania and Hack-and-Slash tags plus
  hand-picked ones, filtered to well-known titles (≥ 1,500 Steam reviews) and checked by hand.
  Each entry: Steam app id, name, optional exe names, `kind` (`boss` / `run` / `level`, only
  the wording changes), optional `screen` rule, optional `save` reader, `verified` flag.
- **Every game counts** (user, 2026-10-19: "don't want it to just detect these games"): any
  game in front is recognised and can be counted with no setup; the catalogue is not a
  whitelist, it only adds extras (save reader, the game's own screen rule, wording). A game can
  be turned off ("Don't count this game"), remembered.

### History (stage 1)

- One file per game: `Settings::dir()\deaths\<key>.jsonl`, one event per line
  (`{"t","ev","fight","boss","src"}`; ev = fight_start / death / undo / win / abandon / rename),
  rewritten through `write_atomic` after every event. Stats are worked out on read
  (`deaths/book.rs`, already written).
- A fight stays open across retries until **Boss defeated** or **Abandon**; deaths on the way
  back to the boss count toward it. A new fight closes the old one unwon. Deaths with no fight
  open still count for the game.
- `src` says where a death came from: `manual`, `hotkey`, `pad`, `save`, `screen`.

### Ways of counting, best first

| # | Way | When | Counts |
|---|---|---|---|
| 1 | **Save file** | the game has a save reader and its save is found | deaths (delta of the save's counter), wins where the save has boss flags |
| 2 | **Screen** | tracked game in front, no healthy save reader | the game's own rule, else the common death words, else a taught fingerprint |
| 3 | **Manual** | always | tray, page, hotkey, controller combo |

- One automatic source per game at a time: while a save reader works, screen detection is off
  (a reader not yet `verified` only takes over once it has counted a death, so one that reads the
  wrong place can't silence a screen watcher that works; 2026-10-19 review)
  for that game. Manual always works.
- Every automatic death shows the pill flash and can be undone (tray, page, hotkey, combo).

### Save readers (stage 2)

- Trait: `locate() -> Vec<PathBuf>`, `read(bytes) -> Vec<Character { slot, name, deaths, bosses }>`.
- The tracker checks the save's modified time every 2 s while the game runs, copies it (shared
  read) and reads the copy. The character whose count changed is the one being played.
- **Elden Ring first** (`ER0000.sl2`, Seamless Co-op `ER0000.co2`): boss flags are documented
  by community tools (MIT / Apache-2.0); the death counter's place is found by diffing two
  copies of a real save around one death (the user's own, read-only), then checked on a second.
- Then, as each is verified: Dark Souls Remastered / PtDE (format documented by `dsfp`),
  Dark Souls II / III and Sekiro (encrypted with public community keys), Celeste (plain XML),
  Cuphead (plain JSON), Hades (compressed Lua state). A reader ships only after it matched a
  real save.

### Screen detection (stage 3, after the capture spike passes in real games)

- Windows Graphics Capture of the game window (from `My-apps\capture-spike`), 4 frames/s for a
  game Deck knows (its own death words or taught screens), 2 for any other (2026-10-19 review),
  shrunk on the GPU; only while a tracked game is in front and not minimised.
- **Common death words** (no setup, every game): when the middle of the frame changes sharply, Windows'
  built-in OCR reads it, looking for "YOU DIED", "DIED", "DEATH", "DEFEATED", "GAME OVER",
  "WASTED", "YOU ARE DEAD", "MISSION FAILED", "RETRY". At most one OCR per second.
- **Catalogue rules**: a game's own words (FromSoftware's "YOU DIED") or a shipped
  fingerprint (Sekiro's 死). A game's words are added to the common ones, never replace them
  (builder's call, 2026-10-19: presets are unchecked in the real games, so a wrong one must not
  hide "YOU DIED"). The common list is phrases only; single words ("DEATH", "RETRY") were
  dropped for false counts in menus. A line of OCR text must be the phrase on its own. A phrase
  of several words may have one letter read wrong; a single word ("WASTED") must be exact, and a
  dropped letter never passes (2026-10-19 review: "WANTED" and "YOU DID" matched before).
  Browsers, video players and Windows' own programs are never games (`boost::not_a_game`), so a
  fullscreen video of a game counts nothing. Exe names many games share (`Game.exe`) are keyed
  with their folder (`exe:tothemoon-game`) and aren't accepted by the library. Taught prints keep
  the picture's shape (`aspect`) and are only matched on pictures of that shape (within 3 %).
- **Built without a game (2026-10-19, user: "keep it theory for now")**: the save readers are
  checked against synthetic files made from the published formats, screen detection against a
  drawn "YOU DIED" on Deck's own window (ignored live test). First basic presets: 15 games.
- **Teaching** ("That was a death"): the last 10 s of frames as a strip, you click the clearest,
  the changed area becomes the mask; up to 3 per game. Built 2026-10-19: the strip is offered
  after a death counted by hand ("Did Deck miss that death?" on the Now tab); fingerprints
  are 64x36 grey cells matched by correlation, so they work without an OCR language.
  **Shared library (2026-10-19, user: "teaching the app teaches the whole repo"):** prints are
  collected in `data/death-screens/library.json` (embedded in builds, refreshed from GitHub at most
  daily, setting `community_library`, default on: builder's call, it's a download only). Submitting
  = copy the code + open a prefilled issue; a workflow turns it into a PR the maintainer reviews
  (builder's call: no automatic merge, a bad print would count deaths that didn't happen). Only game
  keys that are the same on every PC are accepted (steam:, epic:, exe:, emu:). Taught fingerprints can be exported so
  friends can share them and good ones become catalogue rules.
- 2 matches in a row = a death, then 8 s cooldown; near-black frames ignored; a "detector blind"
  warning when capture returns black.

### In-game display: the pill

- Deck's own layered window (the controller overlay's engine: `pad::window::Layered`,
  `pad::art::pill`), click-through, topmost, on the game's monitor, a corner chosen in settings.
- Shows "Malenia · 14" in a fight, "Deaths · 52" otherwise; flashes "+1" on a death and
  "Undone" on an undo. Shown while a game that has deaths or an open fight is in front (so it
  never sits on games you don't count); a setting shows it in every game.
- Hidden from recordings by default (setting to show it, for streamers).

### Controls

- **Tray → Death counter**: Start boss fight… (opens the page with the name box), the last 5
  bosses (starts that fight at once), +1 death, Undo last death, Boss defeated, Abandon fight,
  Don't count this game / Count this game, Open.
- **Hotkeys** (off by default): +1 death, undo (`RegisterHotKey` via `profiles/hotkeys.rs`).
- **Controller combo** (off by default): hold Select/View + D-pad Down 1 s = +1 death, Select +
  D-pad Up 1 s = undo; read passively through its own SDL reader (the overlay's reader only runs while the overlay is on), opened only while the combo is on and a counted game is in front. Holding is
  required so a quick press in the game never counts.
- Which game an action goes to: the game in front, else the game with an open fight, else
  the last one used.

### The page: Monitor → Deaths

Tabs (kept as tabs, per the user's rule):
- **Now**: the game, the open fight card (boss, deaths, time) with +1 / Undo / Defeated /
  Abandon, the name box with suggestions (this game's earlier bosses), how deaths are being
  counted right now (save / screen / manual) and its health.
- **Bosses**: per boss: deaths, attempts, wins, deaths before the first kill, time; rename,
  merge.
- **History**: the events, newest first; undo, delete, move a death to another boss.
- **Games**: your counted games and the catalogue (search), each with how it can be counted
  and whether that's verified; turn a game off / on.
- **Settings**: pill corner/size/opacity, show in recordings, hotkeys, controller combo,
  automatic counting on/off per way.

### Settings (`deaths` object)

`on`, `pill`, `pill_always`, `pill_corner`, `pill_scale`, `pill_opacity`, `pill_in_recordings`,
`hotkey_death`, `hotkey_undo`, `pad_combo`, `auto_save`, `auto_screen`, `off_games` (games
turned off).

## Stages and exit tests

1. **Manual counting, catalogue, recognition, pill, page, tray, hotkeys, combo.**
   Exit: an evening in two games; kill Deck mid-fight; counts and the open fight survive.
2. **Save readers**, Elden Ring first. Exit: one death in the game = +1 within a few seconds,
   no double count, on the user's save.
3. **Screen detection.** Exit (from the plan): ≥ 95 % of deaths caught, 0 false counts in an
   hour of play and loading screens, on recorded clips from 3 games; < 1 % CPU.

## Left to the builder

File layout inside `src-tauri/src/deaths/`, the tray menu's exact wording, the page's layout
(follow the controller page), test names.

## Open (the user's, not blocking stage 1)

- Lords of the Fallen: the first death only sends you to Umbral. Count only the real death?
- Co-op: only your own deaths count (screen rule), summons/revives don't.
