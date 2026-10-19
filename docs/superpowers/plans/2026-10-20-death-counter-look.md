# Death counter look and page Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A chosen-style in-game counter that opens into a boss card after a death (plus an optional red edge), and a Deaths page built on play sessions, tries, records, notes, all-games totals, a share picture and export.

**Architecture:** New pure statistics (`deaths/stats.rs`) worked out from the existing event history plus a new `play` event; new overlay drawing (`deaths/look.rs`, `deaths/edge.rs`) on the existing SVG rasteriser and layered windows; `deaths::state()` grows the JSON the page reads; the page (`ui/js/deaths.js`) is rebuilt tab by tab with inline-SVG charts (`ui/js/deathcharts.js`).

**Tech Stack:** Rust (Tauri 2, windows-sys, resvg via `pad::raster`), vanilla JS modules, the `tools/testing/ui-preview` harness.

**Spec:** `docs/superpowers/specs/2026-10-20-death-counter-look-design.md`

## Global Constraints

- Throttled builds only. Every cargo command: `powershell -NoProfile -Command "(Get-Process -Id $PID).PriorityClass='BelowNormal'; $env:CARGO_TARGET_DIR='<the shared build cache>'; cargo <cmd> -j 6"` from `src-tauri` (written below as `CARGO <cmd>`).
- No commits per task: the user commits only when they ask. Before that commit, `PROJECT_STATE.md` (dated entry) and `CHANGELOG.md` must be updated (the pre-commit hook checks; never `--no-verify`); no local paths or personal data in the repo.
- The counter never takes focus or input; it is hidden from captures unless `pill_in_recordings`.
- The overlay draws only when what it shows changes, plus animations: fade between counter and card ≤ 300 ms, red edge ≈ 800 ms. No timer faster than 33 ms, and only while animating.
- History files only grow new event kinds; old files load unchanged; old settings files load unchanged (new `DeathSettings` fields have defaults).
- Never show what Deck can't know (boss health, "% through a fight"): "better" = a longer try.
- Page tabs stay: Now, Bosses, History, Games, Settings.
- Default values from the spec: style `"skull"`, counts `"both"`, outside a fight `"game"`, card on for 5 s (2–10), red edge off, the user's note on the card off, session split after 15 min away, play checkpoint every 5 min, estimated sessions split at a 1 h gap, milestones at 1, 10, 50, 100 then every 100th death, last 10 tries in bars.
- UI copy: plain words, sentence case, no jargon (match the existing page).

## Review Focus

1. A history with tens of thousands of events (years of play): `state()` runs every page tick (1 s) and must stay fast. Pinned in Task 3 (`state_json_for_a_huge_history_is_quick`).
2. The PC clock jumps back (events out of time order): no negative or giant try lengths, no panic. Pinned in Task 1 (`tries_survive_events_out_of_order`).
3. Deck restarts mid-session: the play lines before and after a short restart are one session, not two. Pinned in Task 1 (`play_lines_a_few_minutes_apart_are_one_session`).
4. Undo right after a death: the card closes and the counter shows the corrected number. Pinned in Task 6 (`undo_closes_the_card`).
5. A very long or non-Latin boss name in the overlay: the counter and card stay a sane width. Pinned in Task 5 (`a_long_name_is_cut_not_widened`).

---

### Task 1: Tries, sessions and records from the history

**Files:**
- Modify: `src-tauri/src/deaths/book.rs` (Event gets `ms`, `Ev::Play`, Book keeps death times and play lines)
- Create: `src-tauri/src/deaths/stats.rs` (pure functions over a Book; tests in the same file)
- Modify: `src-tauri/src/deaths/mod.rs` (`mod stats;`)

**Interfaces:**
- Produces (book.rs):
  - `Ev::Play` (serialises `"play"`); `Event.ms: u64` (`#[serde(default, skip_serializing_if = "is_zero")]`).
  - `Book.deaths: Vec<(u64, Option<usize>)>` (time, fight index) replacing the private `counted`; `Undo` pops it. `total_deaths()`/`can_undo()` keep their meaning.
  - `Book.wins: Vec<(u64, usize)>` (time, fight index).
  - `Book.plays: Vec<(u64, u64)>` (start, ms), one per start (the longest `ms` wins), sorted by start.
- Produces (stats.rs):
  - `pub const MAX_TRY_MS: u64 = 30 * 60_000;` `pub const SESSION_GAP_MS: u64 = 15 * 60_000;` `pub const ESTIMATE_GAP_MS: u64 = 60 * 60_000;`
  - `pub struct Try { pub fight: Option<usize>, pub end: u64, pub ms: u64, pub won: bool }` — a try ends at a death or a win; `ms` = end − (fight start or the previous death/win in that fight); a loose death (no fight) has `fight: None` and `ms` from the previous loose death within `MAX_TRY_MS` else 0. `ms > MAX_TRY_MS` → treated as unknown (0).
  - `pub fn tries(b: &Book) -> Vec<Try>` (by end time).
  - `pub struct Session { pub start: u64, pub end: u64, pub estimated: bool }`
  - `pub fn sessions(b: &Book, events: &[Event], now: u64) -> Vec<Session>` (oldest first): play lines merged when the gap between one's end and the next's start < `SESSION_GAP_MS`; events before the first play line form estimated sessions split at gaps > `ESTIMATE_GAP_MS`.
  - `pub struct Records { pub longest_try_ms: u64, pub longest_try_boss: Option<String>, pub fewest_to_win: Option<(String, u32)>, pub deadliest: Option<(String, u32)>, pub most_in_session: u32 }` (Serialize)
  - `pub fn records(b: &Book, sessions: &[Session]) -> Records`
  - `pub fn death_note(b: &Book) -> Option<String>` — about the latest death: milestone (`"100th death in this game"`, `"1st death in this game"`…) wins over `"Your longest try yet"` (latest try is the longest of ≥ 3 tries on this boss); else None.

- [ ] **Step 1: Write the failing tests** in `stats.rs` (`#[cfg(test)] mod tests`), using `Book::replay` on hand-made events:

```rust
#[test] fn a_try_runs_from_the_fight_start_or_the_last_death() {
    // fight at 0, deaths at 60s and 200s, win at 260s
    let t = tries(&book(&[fs(0, "A"), d(60_000), d(200_000), win(260_000)]));
    assert_eq!(t.iter().map(|t| t.ms).collect::<Vec<_>>(), vec![60_000, 140_000, 60_000]);
    assert!(t[2].won && !t[0].won);
}
#[test] fn tries_survive_events_out_of_order() {
    let t = tries(&book(&[fs(100_000, "A"), d(50_000), d(400_000)]));   // clock went back
    assert!(t.iter().all(|t| t.ms <= MAX_TRY_MS));
}
#[test] fn a_try_longer_than_half_an_hour_is_unknown() {
    let t = tries(&book(&[fs(0, "A"), d(31 * 60_000)]));
    assert_eq!(t[0].ms, 0);
}
#[test] fn undo_removes_the_try_of_the_death_it_takes_back() {
    let t = tries(&book(&[fs(0, "A"), d(10_000), d(20_000), undo(21_000)]));
    assert_eq!(t.len(), 1);
}
#[test] fn play_lines_a_few_minutes_apart_are_one_session() {
    let b = book(&[play(0, 600_000), play(900_000, 600_000), play(10_000_000, 60_000)]);
    let s = sessions(&b, &events, 20_000_000);
    assert_eq!(s.len(), 2);
    assert_eq!((s[0].start, s[0].end, s[0].estimated), (0, 1_500_000, false));
}
#[test] fn old_history_gets_estimated_sessions_split_at_an_hour() {
    let s = sessions(&b /* deaths at 0, 10 min, 3 h */, &events, 4 * 3_600_000);
    assert_eq!(s.len(), 2); assert!(s[0].estimated);
}
#[test] fn the_hundredth_death_is_a_milestone_and_wins_over_a_record() { /* 100 deaths, last try longest */
    assert_eq!(death_note(&b).as_deref(), Some("100th death in this game"));
}
#[test] fn the_longest_try_yet_is_noted() {
    assert_eq!(death_note(&book(&[fs(0,"A"), d(10_000), d(20_000), d(80_000)])).as_deref(), Some("Your longest try yet"));
}
#[test] fn records_name_the_deadliest_boss_and_the_fewest_deaths_to_win() {
    let r = records(&b, &[]);
    assert_eq!(r.deadliest, Some(("Margit".into(), 3)));
    assert_eq!(r.fewest_to_win, Some(("Godrick".into(), 1)));
}
```

In `book.rs` tests: `a_play_line_saves_its_length` (`{"t":5,"ev":"play","ms":60000}` round-trips; a death still serialises without `ms`), and `the_longest_play_line_per_start_is_kept`.

- [ ] **Step 2: Run them to see them fail**

Run: `CARGO test --bin deck deaths::stats deaths::book`
Expected: compile errors (`stats` module, `Ev::Play`, `ms` missing).

- [ ] **Step 3: Implement** the book changes and `stats.rs` with the signatures above. Ordinal words: `1st, 2nd, 3rd, Nth` (11th–13th use "th").

- [ ] **Step 4: Run the tests**

Run: `CARGO test --bin deck deaths::`
Expected: all pass (the existing book tests too).

---

### Task 2: Recording play sessions

**Files:**
- Modify: `src-tauri/src/deaths/mod.rs` (St gets `playing`; the run loop calls `session_step`; writes play lines)
- Test: in `mod.rs` tests

**Interfaces:**
- Consumes: `Ev::Play`, `Event.ms`, `stats::SESSION_GAP_MS`.
- Produces:
  - `struct Playing { key: String, start: u64, seen: u64, saved: u64 }` in `St.playing: Option<Playing>`.
  - `fn session_step(p: Option<Playing>, front: Option<&str>, now: u64) -> (Option<Playing>, Vec<(String, u64, u64)>)` — returns the new state and the play lines to write `(game key, start, ms)`: a counted game in front starts or continues a session (a different game ends the old one); with nothing (or another game) in front for `SESSION_GAP_MS` the session ends; a checkpoint line every 5 min (`now - saved >= 300_000`).
  - `fn write_play(st: &mut St, key: &str, start: u64, ms: u64)` — replaces the game's existing play event with the same `t` (or appends), updates the Book, `store::save`.

- [ ] **Step 1: Failing tests**

```rust
#[test] fn a_session_starts_continues_and_checkpoints() {
    let (p, w) = session_step(None, Some("exe:a"), 0);
    assert!(w.is_empty()); let p = p.unwrap(); assert_eq!(p.start, 0);
    let (p, w) = session_step(Some(p), Some("exe:a"), 300_000);
    assert_eq!(w, vec![("exe:a".into(), 0, 300_000)]);
    let (p, w) = session_step(p, None, 600_000);          // alt-tabbed
    assert!(p.is_some() && w.is_empty());
    let (p, w) = session_step(p, None, 300_000 + 15 * 60_000);
    assert!(p.is_none()); assert_eq!(w, vec![("exe:a".into(), 0, 300_000)]); // ends at last seen
}
#[test] fn another_game_ends_the_session() {
    let (p, _) = session_step(None, Some("exe:a"), 0);
    let (p, w) = session_step(p, Some("exe:b"), 60_000);
    assert_eq!(w, vec![("exe:a".into(), 0, 60_000)]); assert_eq!(p.unwrap().key, "exe:b");
}
#[test] fn a_checkpoint_replaces_the_play_line_instead_of_adding_one() { /* write_play twice same start → one Play event, ms = second */ }
```

- [ ] **Step 2: Run, see them fail.** `CARGO test --bin deck deaths::tests` → errors: `session_step` missing.
- [ ] **Step 3: Implement** `session_step`, `write_play`, and call it once per loop look (`next_look`) with the counted front game's key (`front` filtered by `counted`); on Deck exit nothing special (the checkpoint bounds the loss to 5 min).
- [ ] **Step 4: Run** `CARGO test --bin deck deaths::` → pass.

---

### Task 3: The page's data (`state()` JSON), notes, records, all games

**Files:**
- Modify: `src-tauri/src/deaths/mod.rs` (`state()`), `src-tauri/src/deaths/store.rs` (notes), `src-tauri/src/deathscmd.rs` (`deaths_note`), `src-tauri/src/main.rs` (register the command)
- Modify: `tools/testing/ui-preview/mock.js` (the mock `deaths_state` returns the new shape)

**Interfaces:**
- Consumes: Task 1 (`tries`, `sessions`, `records`), Task 2 (`playing` for the open session).
- Produces:
  - `store::load_notes(key) -> BTreeMap<String, String>` / `store::save_notes(key, &map) -> Result<(), String>`; file `<key file stem>.notes.json`; keys are `book::boss_key`; text trimmed, ≤ 500 chars, empty removes.
  - Command `deaths_note(game: String, boss: String, text: String) -> R<()>`.
  - `state()` → `game` gains (and drops `history`):
    - `"tries": [ms; ≤10]` — the open fight's last tries, else the game's;
    - `"session": { start, ms, deaths, bosses, beaten, pace_ms, deadliest, estimated, live }` | null (the latest session; `live` while `playing` is this game);
    - `"bosses"`: each `BossStats` plus `best_try_ms`, `tries_ms` (≤ 200, oldest first), `won_on` (try number of the first win) | null, `won_at` | null, `last` (latest activity), `note`;
    - `"sessions": [{ start, end, estimated, deaths, fought: [boss], beaten: [boss], per15: [n], events: [{ i, t, ev, boss, src }] }]` newest first, ≤ 60 sessions, events ≤ 400 per session, newest first;
    - `"records": Records`.
  - top level `"overall": { deaths, played_ms, beaten, games: [{ key, name, deaths }] (top 5), bosses: [{ game, boss, deaths }] (top 5), weeks: [n; 12] (oldest first, Monday weeks) }`.

- [ ] **Step 1: Failing tests** in `mod.rs` tests (build an `St` with a log in memory, call the JSON builder split out as `fn game_json(st: &mut St, key: &str, now: u64) -> Value` and `fn overall_json(st: &mut St, now: u64) -> Value`):

```rust
#[test] fn the_game_json_has_sessions_tries_and_records() {
    let v = game_json(&mut st_with(events_of_two_sessions()), "exe:a", NOW);
    assert_eq!(v["sessions"].as_array().unwrap().len(), 2);
    assert_eq!(v["sessions"][0]["deaths"], 3);              // newest first
    assert_eq!(v["tries"].as_array().unwrap().len(), 3);
    assert_eq!(v["bosses"][0]["best_try_ms"], 140_000);
    assert!(v["records"]["deadliest"].is_array());
}
#[test] fn notes_are_kept_per_boss_whatever_the_spelling() { /* save "  margit " → load under boss_key("Margit") */ }
#[test] fn state_json_for_a_huge_history_is_quick() {
    let ev = (0..40_000u64).map(|i| if i % 50 == 0 { fs(i * 60_000, &format!("B{}", i / 500)) } else { d(i * 60_000) }).collect::<Vec<_>>();
    let t = std::time::Instant::now();
    let v = game_json(&mut st_with(ev), "exe:a", NOW);
    assert!(t.elapsed() < std::time::Duration::from_millis(150), "{:?}", t.elapsed());
    assert!(v["sessions"].as_array().unwrap().len() <= 60);
}
```

- [ ] **Step 2: Run, see them fail.** `CARGO test --bin deck deaths::tests` → `game_json` missing.
- [ ] **Step 3: Implement** the store functions, `game_json`, `overall_json` (overall reads each game's Book through `st.log`; cache it for 10 s keyed on the games' `last`), the command, the registration, and update `mock.js`'s `deaths_state` to the new shape with realistic numbers (two bosses, three sessions, a note).
- [ ] **Step 4: Run** `CARGO test --bin deck deaths::` and `node tools/testing/ui-preview/regress.mjs` → pass (the old page still renders from the new data: `history` gone means the History tab shows its empty state until Task 9).

---

### Task 4: Counter settings

**Files:**
- Modify: `src-tauri/src/deaths/cfg.rs`
- Modify: `tools/testing/ui-preview/mock.js` (settings defaults)

**Interfaces:**
- Produces `DeathSettings` fields (serde default as listed): `pill_style: String` (`"skull"`, one of `STYLES = ["skull","ring","huge"]`), `pill_counts: String` (`"both"`, `COUNTS = ["boss","game","both"]`), `pill_outside: String` (`"game"`, `["game","hide"]`), `card: bool` (true), `card_secs: u32` (5, clamp 2–10), `card_lines: CardLines` (struct, `#[serde(default)]`: `try_no, try_len, boss_time, session, bars, note: bool` all true, `my_note: bool` false), `red_edge: bool` (false).

- [ ] **Step 1: Failing test** in `cfg.rs`:

```rust
#[test] fn new_counter_settings_default_and_clamp() {
    let s: DeathSettings = serde_json::from_str(r#"{"pill":true}"#).unwrap();
    assert_eq!((s.pill_style.as_str(), s.pill_counts.as_str(), s.pill_outside.as_str(), s.card, s.card_secs, s.red_edge), ("skull", "both", "game", true, 5, false));
    assert!(s.card_lines.bars && !s.card_lines.my_note);
    let s: DeathSettings = serde_json::from_str(r#"{"pill_style":"neon","card_secs":60}"#).unwrap();
    let s = s.clamped();
    assert_eq!((s.pill_style.as_str(), s.card_secs), ("skull", 10));
}
```

- [ ] **Step 2: Run, see it fail.** `CARGO test --bin deck deaths::cfg` → unknown fields.
- [ ] **Step 3: Implement** the fields, defaults and `clamped()`.
- [ ] **Step 4: Run** → pass.

---

### Task 5: Drawing the counter, the card and the labels

**Files:**
- Create: `src-tauri/src/deaths/look.rs` (pure: data in, `Img` out; tests in the file)
- Modify: `src-tauri/src/deaths/mod.rs` (`mod look;`)

**Interfaces:**
- Consumes: `pad::art::{Theme, theme, text_line, rect, circle, path, n}`, `pad::raster::{svg, halo, Img}`, Task 4 settings.
- Produces:
  - `pub struct Counter { pub boss: Option<(String, u32)>, pub game: (String, u32), pub ring_mark: Option<u32> }`
  - `pub fn counter(c: &Counter, s: &DeathSettings, t: &Theme, k: f32) -> Option<Img>` — None when `pill_counts`/`pill_outside` leave nothing to show (no fight + `"boss"`, or no fight + outside `"hide"`). Styles per spec: skull (red skull icon from an SVG path, bold number, small uppercase label over the name), ring (number in a ring filled `count / mark`, or `count % 10 / 10` laps without a mark; second number small under it for "both"), huge (≈ 4× the number size, 0.88 opacity text with a soft shadow, label small under). "both": boss number main, game total smaller beside it (skull/huge) or under it (ring).
  - `pub struct Card { pub title: String, pub try_next: u32, pub deaths: u32, pub try_ms: u64, pub boss_ms: u64, pub session_deaths: u32, pub bars: Vec<u64>, pub note: Option<String>, pub my_note: Option<String> }`
  - `pub fn card(c: &Card, s: &DeathSettings, t: &Theme, k: f32) -> Img` — lines per `s.card_lines`; the deaths number always shown in the death red `#ff6a6a`; bars: the last one `#e04848`, others `#6b2a2a`, heights relative to the longest.
  - `pub fn label(text: &str, t: &Theme, k: f32) -> Img` — "Already counted", "Undone", "Defeated"… in the counter's box style.
  - Names longer than 28 characters are cut with "…" (by characters, not bytes).
  - Durations: `fn dur(ms) -> String` → `"2:41"` under an hour, `"1 h 05"` over.

- [ ] **Step 1: Failing tests**

```rust
#[test] fn every_style_draws_and_grows_with_the_number() {
    for style in ["skull", "ring", "huge"] {
        let s = DeathSettings { pill_style: style.into(), ..Default::default() };
        let a = counter(&one("Margit", 9, 40), &s, art::theme("ds4"), 1.0).unwrap();
        let b = counter(&one("Margit", 1234, 40), &s, art::theme("ds4"), 1.0).unwrap();
        assert!(b.w() >= a.w(), "{style}");
    }
}
#[test] fn nothing_to_show_draws_nothing() {
    let s = DeathSettings { pill_counts: "boss".into(), ..Default::default() };
    assert!(counter(&Counter { boss: None, game: ("G".into(), 3), ring_mark: None }, &s, art::theme("ds4"), 1.0).is_none());
}
#[test] fn the_card_gets_shorter_when_lines_are_off() {
    let all = card(&sample(), &DeathSettings::default(), art::theme("ds4"), 1.0);
    let mut s = DeathSettings::default(); s.card_lines.bars = false; s.card_lines.note = false;
    assert!(card(&sample(), &s, art::theme("ds4"), 1.0).h() < all.h());
}
#[test] fn a_long_name_is_cut_not_widened() {
    let s = DeathSettings::default();
    let short = counter(&one(&"界".repeat(28), 5, 5), &s, art::theme("ds4"), 1.0).unwrap();
    let long = counter(&one(&"界".repeat(300), 5, 5), &s, art::theme("ds4"), 1.0).unwrap();
    assert!(long.w() <= short.w() + 20);
}
#[test] fn durations_read_like_a_clock() { assert_eq!((dur(161_000), dur(3_900_000)), ("2:41".into(), "1 h 05".into())); }
```

- [ ] **Step 2: Run, see them fail.** `CARGO test --bin deck deaths::look` → module missing.
- [ ] **Step 3: Implement** `look.rs`. Colours from the theme (`panel`, `text`, `dim`) with the death red as the only strong colour; sizes scale with `k`.
- [ ] **Step 4: Run** → pass.

---

### Task 6: Showing it in game (the card after a death, fades, the red edge)

**Files:**
- Modify: `src-tauri/src/deaths/pill.rs` (`show_img`, fades)
- Create: `src-tauri/src/deaths/edge.rs` (four click-through edge windows)
- Modify: `src-tauri/src/deaths/mod.rs` (St `card`, the run loop's drawing; `act` sets/clears the card; old text `flash` kept for labels)

**Interfaces:**
- Consumes: Task 5 (`look::counter/card/label`), Task 1 (`tries`, `death_note`), Task 3's per-boss numbers (reuse the same helpers, not the JSON), `pad::window::Layered`.
- Produces:
  - `Pill::show_img(&mut self, key: &str, img: &Img, mon, s: &DeathSettings)` (redraws only when `key` changes; `position()` as now) and `Pill::fade(&mut self, to: f32)`; `show(text…)` is removed in favour of `look::label` + `show_img`.
  - `St.card: Option<(String, Instant, look::Card)>` — set by `act("death")` when `s.card` (not for "Already counted"), cleared by `act("undo")` for that game; shown for `card_secs`.
  - `pub struct Edge` with `Edge::new()`, `flash(&mut self, mon: (i32,i32,i32,i32), in_recordings: bool)`, `tick(&mut self) -> bool` (true while animating): four `Layered` strips (top, bottom, left, right), each a red inner gradient (`#c81414`, 0.8 → 0 over 9 % of the shorter side), drawn once per monitor size; alpha 1 → 0 over 800 ms in ~33 ms steps.
  - The loop: `next_look` is 33 ms while a fade or the edge animates, 200 ms while a card or label shows, else 1 s (as today).

- [ ] **Step 1: Failing tests** (pure parts, in `mod.rs` tests):

```rust
#[test] fn a_death_opens_the_card_and_undo_closes_it() { /* act-level helper on St: after_death(&mut st, key, settings) sets st.card; after_undo clears */ }
#[test] fn undo_closes_the_card() { /* the card's game key matches → None; another game's card stays */ }
#[test] fn the_card_is_not_opened_when_turned_off_or_already_counted() { }
#[test] fn the_ring_mark_is_the_last_beaten_boss_deaths() { /* fn ring_mark(b: &Book) -> Option<u32> */ }
```

In `edge.rs`: `the_edge_fades_out_in_about_800ms` (pure `fn alpha_at(ms: u64) -> f32`: `alpha_at(0) == 1.0`, `alpha_at(800) == 0.0`, monotonic).

- [ ] **Step 2: Run, see them fail.** `CARGO test --bin deck deaths::` → missing functions.
- [ ] **Step 3: Implement**: split `act`'s death/undo side effects into `after_death`/`after_undo` on `St`; the run loop picks, per tick, label (text flash) > card (within `card_secs`) > counter, draws through `look`, cross-fades on a change of kind (alpha down 150 ms, swap, up 150 ms), pops the counter when its number changes (drawn at `k × 1.15` for 120 ms, then `k`), and drives `Edge` when `s.red_edge`.
- [ ] **Step 4: Run** `CARGO test --bin deck deaths::` → pass. `CARGO clippy --all-targets -- -D warnings` → clean.

---

### Task 7: Live preview, share picture, export

**Files:**
- Modify: `src-tauri/src/deathscmd.rs` (commands), `src-tauri/src/main.rs` (register; `file_dialog` reused), `src-tauri/src/clipboard.rs` (`set_png` → `pub(crate)`), `src-tauri/src/deaths/look.rs` (`share`), `src-tauri/src/deaths/mod.rs` (`export_csv`)

**Interfaces:**
- Produces:
  - Command `deaths_preview() -> R<Value>`: `{ "counter": "data:image/png;base64,…" | null, "card": "data:…" }` drawn with the saved settings and fixed sample data (boss "Margit", 14 deaths, game 212, card with 10 bars and the note "Your longest try yet"), at `k = 1.0`.
  - `pub fn share(title: &str, rows: &[(&str, String)], bars: &[u64], t: &Theme) -> Img` in `look.rs` (a 600×315 card: game or boss name, up to 4 big stats, the try bars, "Deck" small in a corner).
  - Command `deaths_share(game: String, boss: Option<String>, save: bool) -> R<Option<String>>`: draws the picture; `save == false` copies it to the clipboard (`clipboard::set_png`) and returns None; `save == true` asks for a file (`file_dialog` + `blocking_save_file`, `.png`) and returns the path.
  - `pub fn export_csv(game: Option<&str>) -> String` in `mod.rs`: header `time,game,event,boss,source`, ISO-8601 local time, one row per event (all games when None), quoted fields with `"` doubled.
  - Command `deaths_export(game: Option<String>) -> R<Option<String>>` (save dialog, `.csv`).

- [ ] **Step 1: Failing tests**: `export_csv_quotes_and_lists_every_event` (a boss named `He said "hi", twice` round-trips as `"He said ""hi"", twice"`; header line exact); `the_share_picture_is_600_by_315` (`look::share(...).w() == 600 && h() == 315`).
- [ ] **Step 2: Run, see them fail.**
- [ ] **Step 3: Implement**; set `DIALOG` around the save dialog like `pick` does.
- [ ] **Step 4: Run** `CARGO test --bin deck deaths::` → pass.

---

### Task 8: Charts and the Now tab

**Files:**
- Create: `ui/js/deathcharts.js`
- Modify: `ui/js/deaths.js` (`nowTab`), `ui/app.css` (`.dc-*` rules)

**Interfaces:**
- Consumes: Task 3 JSON (`game.tries`, `game.session`, `game.open`, `game.total`, `game.records`).
- Produces (deathcharts.js, each returns an SVG element via the page's `h`-style helper or `document.createElementNS`):
  - `bars(values: number[], { height = 28, last = true, label })` — taller = longer; last bar red when `last`; `role="img"` with an `aria-label` from `label`.
  - `line(values: number[], { height = 70, color, label })` — a polyline scaled to the max, first/last try labels.
  - `perTime(counts: number[], { height = 26, label })` — small bars, no emphasis.

- [ ] **Step 1: Build `deathcharts.js`** with the three functions; no chart library.
- [ ] **Step 2: Rebuild `nowTab(x)`** per spec: header (name, in front / last played, the count-this-game switch as now), both counters big (boss in `var(--red)`-style red, game neutral), buttons unchanged (+1, Undo, Boss defeated / Abandon, start with name chips, rename), last 10 tries `bars`, a four-tile row (this session deaths, playing for, dying every, deadliest boss; tiles hidden when `session` is null), a "Records" row (longest try, fewest deaths to beat a boss, deadliest boss), "Share" button (`deaths_share` with `save: false`, then a toast "Copied to the clipboard" with a "Save as…" action), then teaching cards unchanged.
- [ ] **Step 3: Verify** `node tools/testing/ui-preview/regress.mjs` and `node tools/testing/ui-preview/a11y.mjs` → pass/clean; look at the Now screenshot from `tools/testing/ui-preview/shot.mjs` at 1280 and 390 wide.

---

### Task 9: Bosses cards and notes; History sessions

**Files:**
- Modify: `ui/js/deaths.js` (`bossesTab`, `historyTab`), `ui/app.css`

**Interfaces:**
- Consumes: Task 3 JSON (`bosses[*]`, `sessions[*]`), `deaths_note`, `popMenu` from `profiles.js`, `confirmBox`, Task 8 charts.

- [ ] **Step 1: `bossesTab`**: a two-column grid of cards (one column under 640 px), fighting first then by `last`; each: name + chip (fighting / beaten), deaths (red while fighting), time on boss, best try, "beaten on try N, <when>" when won, `line(tries_ms)` red while fighting / green once beaten, a one-line verdict ("Tries are getting longer" when the mean of the last 5 tries > mean of the first 5, else nothing), the note (click to edit; Enter or blur saves through `deaths_note`), "Share" (as on Now, with `boss`).
- [ ] **Step 2: `historyTab`**: one card per session, the first open, others folded (click the header); header: "Tonight"/"Yesterday"/weekday/date, time range, length (marked "about" when `estimated`), deaths, chips for bosses beaten, `perTime(per15)`; inside: a timeline of events (newest first) with words per event ("Died to <boss> · seen on screen", "Beat <boss> after N deaths, <dur>", "Started <boss>", "Undid a death"), runs of more than 5 deaths in a row folded to "+ N more"; each line has a "⋯" button opening `popMenu` with "Delete this line…" (the existing `confirmBox` text and `act("delete:i")`).
- [ ] **Step 3: Verify** regress + a11y as in Task 8; check that every "⋯" button has an `aria-label`.

---

### Task 10: Games overview, export, counter settings with preview

**Files:**
- Modify: `ui/js/deaths.js` (`gamesTab`, `settingsTab`), `ui/app.css`, `tools/testing/ui-preview/mock.js` (`deaths_preview`, `deaths_share`, `deaths_export` mocks)

**Interfaces:**
- Consumes: Task 3 `overall`, Task 4 settings, Task 7 commands.

- [ ] **Step 1: `gamesTab`**: on top, totals (deaths, hours played, bosses beaten), "Deadliest games" and "Deadliest bosses" (top 5 each), deaths per week (`perTime(weeks)`), an "Export everything" button (`deaths_export` with no game); per game row an "Export" item next to "Show"; the rest unchanged.
- [ ] **Step 2: `settingsTab`**, "In-game counter" section: existing rows kept; new rows: Style (`seg` skull / ring / huge number), Counts (boss / game / both), Outside a fight (game total / hide), Card after a death (toggle) with Seconds (`slider` 2–10) and a toggle per line (Try number, This try's length, Time on the boss, This session, Recent tries, Highlights, My boss note), Red edge on a death (toggle). A sticky preview beside the section (under it below 900 px): the two images from `deaths_preview`, refreshed after each `setDeaths`.
- [ ] **Step 3: Verify** regress + a11y; the preview images load in the harness (mocked data URLs).

---

### Task 11: Docs, checks, install

**Files:**
- Modify: `PROJECT_STATE.md`, `CHANGELOG.md` (Unreleased), `DECK.md` (deaths row: `stats.rs`, `look.rs`, `edge.rs`, play events, new commands)

- [ ] **Step 1: Full checks**: `CARGO clippy --all-targets -- -D warnings` (clean), `CARGO test --bin deck` (all pass), `node tools/testing/ui-preview/regress.mjs`, `node tools/testing/ui-preview/a11y.mjs`.
- [ ] **Step 2: Docs**: a dated `PROJECT_STATE.md` entry (what changed, the spec and plan paths, what wasn't checked in a game), `CHANGELOG.md` "Unreleased" (Added: counter styles, the card, red edge, sessions, boss cards, records, notes, all games, share, export).
- [ ] **Step 3: Install** (ask first if a game is running): list windows in its own step, then `CARGO build --release` and `install.ps1 -NoBuild -Quiet`.
- [ ] **Step 4: Tell the user** what to try in game (a death opens the card; Settings → style, red edge), and that it isn't committed or released until they say.
