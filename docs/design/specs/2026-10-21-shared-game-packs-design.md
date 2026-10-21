# Shared game packs: what one player teaches Deck, everyone gets

Status: design, 2026-10-21. Replaces the GitHub-issue death-screen library (2026-10-19).

## Why

The death counter learns things about a game: its death screen, its victory screen, where its boss
health bar is, and the names of its bosses. Today only death screens can be shared, and sharing them
takes a GitHub account, a copy-and-paste of a ~7,500-character code into an issue form, and a pull
request the maintainer merges by hand. The library in the repo is still empty: nobody has used it.

The owner's words: "there should be a better way/more automatic way to update the boss info
database", "no github account, but surely there's another way", and on the current sharing: "feels
like it's bad right now".

What's wrong today, worst first (from the review of 2026-10-21):

1. Contributors need a GitHub account, and their issue is public under it.
2. Five manual steps (copy, browser, paste, type the name, tick a box); a blob of up to ~7,500 characters.
3. Never exercised end to end; it depends on a repo setting nobody has checked.
4. Every entry waits for the maintainer to merge a pull request (at most 20 waiting).
5. Two entries at once conflict (both bump `version` in the same file).
6. No signal that a shared screen works or miscounts; the only remedy is turning the whole library
   off for a game. A bad entry stays for everyone.
7. A game with 6 shared screens can never take a better one.
8. Only death screens: bar positions, victory screens and boss names can't be shared.
9. A friend's code fills your own 3 slots and looks like your own teaching.
10. Every print carries the exact time its teacher taught it.

What already works and stays: the download side in `deaths/library.rs` (once a day, ETag, size caps,
every print checked like one you taught, one bad entry never empties the library, an embedded copy in
every build, game keys that are the program's name).

## Goals

- **No account for anyone but the maintainer.** A player shares with one click, or with nothing at all
  once they've said "always".
- **Automatic when people agree.** An item reaches everyone without the maintainer once two different
  installs, from two different networks, sent the same thing.
- **Self-correcting.** A shared item that miscounts is turned off on that PC at once, reported, and
  dropped for everyone when enough installs report it.
- **Everything the boss work learns is shareable** as one *game pack* per game: death screens,
  victory screens, the boss bar's spot, boss names, and "not a boss" names.
- **Friends** can still swap a pack directly, with a short code, without it taking your own slots.
- **The maintainer can review everything**: see every entry with its pictures, approve one sent by a
  single player, remove a wrong one, restore it, and publish at once (owner: "how can I manually review
  … the wrong entries. As an admin or something").
- **Several of each** (owner: "we can make it so that we can have multiple of each"): several death
  screens, victory screens and boss bar places per game, locally and in the library.

## Non-goals

- No accounts, logins, profiles, leaderboards or per-player pages.
- No pictures of anyone's screen ever leave the PC: only the 64x36 fingerprints Deck already makes
  (a mask and grey values), bar spots (four numbers) and names.
- No live sync of your counts, deaths or history. Only what Deck learned about the *game*.
- The hand-kept catalogue (`catalog.json`) stays as it is; it isn't edited by the service.

## Anti-goals (would make it a failure even if it works)

- Deck uploading without the player knowing. The first share is always asked; "always" is a choice
  the player makes, and the setting says exactly what goes out.
- One person (or one spammer with many fake installs) able to push data to everyone.
- The maintainer's GitHub token anywhere outside the Worker.
- Daily commits flooding `main` or colliding with the maintainer's local work.
- A broken or missing service making the death counter worse: Deck keeps working with the copy it has.

## The parts

```
Deck (each PC)                    Cloudflare Worker (maintainer's)          GitHub (public)
──────────────                    ────────────────────────────────          ───────────────
teach / read a boss name
  → pack ── POST /v1/submit ──→   check, rate-limit, store (D1)
"counted wrong"                   
  → ──────── POST /v1/report ──→  store
                                  daily cron: group what agrees,
                                  drop what's reported, build the  ──→  branch `library`:
                                  library, commit if it changed          library.json
Deck downloads once a day  ←──────────────── raw.githubusercontent.com/…/library/library.json
```

- **Intake:** a Cloudflare Worker with a D1 (SQLite) database, on the free plan. Code in
  `services/library-worker/` in this repo.
- **Published library:** one file, `library.json`, on its own branch `library` of this repo, written
  by the Worker at most once a day. It's public, readable, and its history is the audit trail. `main`
  is never touched by the Worker.
- **Deck** downloads it exactly as today (once a day, ETag), from the new branch and path.

## What a game pack holds

```jsonc
{
  "pack": 2,
  "game": "exe:bbq",                    // library::good_key
  "name": "The First Berserker: Khazan",// library::clean_name
  "death": [Print],                      // taught death screens, at most 8
  "won":   [Print],                      // taught victory screens, at most 8
  "bars":  [{ "x0": 290, "x1": 710, "y0": 52, "y1": 62, "aspect": 1777 }],  // bar::Spot + picture shape, at most 4
  "bosses": ["Blade Phantom Ozma"],     // names read on screen, see below; at most 50
  "not_bosses": ["Viper"]               // names marked "Not a boss"; at most 50
}
```

- `Print` is today's `teach::Print` (`mask`, `values`, `aspect`) with **`made` always 0** in a pack.
- **Which boss names go in:** names Deck read on a boss bar (`Fight.seen` and the fight's `boss` when
  `auto`), only for fights with at least one death or a win, never names marked "Not a boss", and never
  names typed by hand (those may be nicknames).
- Every list is de-duplicated (prints by mask+values, names by `book::boss_key`).

The same JSON, deflate-compressed and base64url-encoded, prefixed `deck-pack:`, is the **friend code**.

## Deck: sending

New setting `deaths.share_learned`: `"ask"` (default), `"always"`, `"never"`.

- **When there's something to send:** after teaching a death screen, a victory screen or a bar place;
  after marking "Not a boss"; and once a day for new boss names (only names not sent before).
- **"ask":** a small card on the Deaths page (not a popup, never in game): "Share what Deck learned
  about Khazan with other players?" with **Share**, **Always share**, **Not now**, **Never**. It lists
  what would go out in plain words ("1 death screen (a 64x36 fingerprint), the boss bar's place,
  4 boss names").
- **What's sent:** the pack, a random **install id** (128 bits, made once, stored in
  `deaths\install.id`, used for nothing else), and Deck's version. Nothing else.
- **How:** one HTTPS POST from a background thread, with Deck's normal timeouts. On failure it's kept in
  `deaths\outbox.json` (at most 20 packs) and retried with the library refresh, never more than once an
  hour. Nothing blocks the game or the page.
- Turning the death counter or `community_library` off stops all sending too.

## Deck: what it does with the library

Library format 2 (below) gives, per game: death and victory prints, a bar spot, boss names, not-a-boss
names, each item with an `id`. Precedence: **yours > a friend's > the library > the catalogue.**

- **Death and victory screens** are matched like today's library prints (`by = "library:<id>"`).
- **Bar places:** yours, then the library's, are all looked at (each only on pictures of its shape,
  `aspect` within 2%), then the middle as always. `screen::set_spot` takes a list.
- **Boss names:** a name read on a bar that is `same_boss` with a library name is taken in the
  library's spelling, and engages after **one** read instead of two.
- **Not-a-boss names:** treated like your own "Not a boss" list.

### Several of each, locally

- Your own taught death screens and victory screens: up to **8** each per game (was 3). Matching a
  print is a few hundred cell reads, so 8 + 8 + the library's per frame stays well under a millisecond.
- Your own bar places: up to **4** per game (`deaths.bar_spots` becomes game key -> list). Teaching
  another adds it; each has its own Forget. A place carries the picture shape it was taught on.
- The page lists each kind with a count ("Death screens: 4 of 8").

### Friend packs

`deaths\friends.json`: packs imported from friend codes, one per game (a newer import replaces it).
They don't count against your 3 slots and show on the page as "from a friend" with their own Forget.
`teach::unshare` keeps reading old `deck-death-screen:` codes and imports them as a friend pack.

### Turning off one item, and reporting it

- Each library or friend item on the page gets **"This one counted wrong"**: it goes on a local
  off-list (`deaths.library_off_items`, ids) and, when sharing isn't "never", a report is sent.
- **By itself:** when two deaths counted by the same library item are undone within 30 s of being
  counted (within one day), Deck turns that item off and reports it, with a label "Deck stopped using a
  shared death screen that counted wrong" on the page.
- The old per-game "Don't use them" switch stays.

## The Worker

### Endpoints

- `POST /v1/submit` — body `{ install, deck, pack }`, at most 48 KB. 202 on accept; 400 with a short
  reason when the pack fails a check; 429 when rate-limited.
- `POST /v1/report` — body `{ install, deck, game, id, why: "miscounted" }`. 202.
- `GET /v1/health` — `{ ok: true, version }`, for the maintainer.
- `/admin/…` — the review page and its actions (below), only behind Cloudflare Access.

No other routes. No CORS for `/v1` (Deck isn't a browser page).

### Checks on arrival (the same as Deck's)

Game key (`good_key`), name (`clean_name`), each print (`teach::Taught::new`: mask size, mask and values
agree, at least 24 cells, enough spread), a spot (`Spot::sane`), names (`boss::clean_name` rules: 3+
letters, at most 7 words and 48 characters), list caps. **One shared set of test files**
(`data/library/fixtures/*.json`, good and bad packs with the expected verdict) is run by the Rust tests
and the Worker's tests, so the two can't drift.

### Rate limits and identity

- Per install: 30 submits and 30 reports a day. Per network: 100 a day.
- The network is stored only as `sha256(ip + daily salt)`; the salt changes every day and old ones are
  deleted, so the database can't tell which network sent what after a day.
- Install ids are stored as `sha256(install + fixed salt)`.

### Daily build (cron, 04:00 UTC)

For each game, from every stored submission:

1. **Group what agrees.**
   - *Prints* (death and won separately): two prints agree when they were taught on the same picture
     shape (`aspect` within 2%) and each one, drawn as a thumbnail (`teach::picture` without the
     contrast stretch), is matched by the other's `Taught::matches`. Groups are built greedily.
   - *Bar places:* two agree when their picture shapes match (within 2%) and every edge is within 20
     thousandths. The published place is the median.
   - *Names:* agree when `same_boss`; the published spelling is the one sent by the most installs.
2. **Your decisions first:** removed groups are left out, approved ones are kept.
3. **Keep a group when at least 2 installs from at least 2 networks sent it**, or one of the
   maintainer's installs did (`TRUSTED_INSTALLS`, a Worker secret listing install-id hashes).
4. **Drop a group when** the installs that reported it are at least 3 and at least as many as the
   installs that sent it (unless you approved it after the reports).
5. **Cap:** the 8 death and 8 victory groups per game with the most installs; 4 bar places; 200 names;
   200 not-a-boss names. A better group (more installs) replaces a weaker one, so a game never gets
   stuck.
6. Each published item's `id` is a short hash of its content (stable across builds).
7. If the result differs from what's on the `library` branch, commit it there with `version` + 1
   (GitHub contents API, one request to read, one to write). Otherwise do nothing.

Submissions older than 365 days, and from installs that haven't sent anything in that time, are
deleted. Reports are kept as long as the item they're about.

### Reviewing: the admin page

A page served by the Worker at `/admin`, for the maintainer only.

- **Who can open it:** Cloudflare Access (free plan) protects `/admin*`: you sign in with a one-time
  code sent to your email, and no other address is allowed. The Worker also checks Access's signed
  header (`Cf-Access-Jwt-Assertion`, verified against the team's public keys) on every admin request,
  so the page is closed even if Access were set up wrong. No password or admin key exists anywhere.
- **What it shows**, per game (searchable, newest activity first):
  - every group the daily build would form, published or not: death and victory screens drawn as
    pictures (the fingerprint, like the page's "learned" thumbnails), bar places drawn as a box on a
    screen-shaped outline, names as a list;
  - for each: installs that sent it, networks, reports, first and last seen, and its state:
    **published**, **waiting** (one sender), **dropped by reports**, **removed by you**, **approved by you**.
- **What you can do:** **Approve** (publish even with one sender), **Remove** (never publish this, and
  anything that agrees with it), **Restore** (undo either), rename a game, fix a boss name's spelling,
  and **Publish now** (run the daily build at once, still one commit).
- Your decisions are stored in D1 (`decisions`: item hash, action, when) and win over the automatic
  rules. A removed item's hash is also refused at submit time, so the same wrong screen doesn't keep
  coming back.
- Plain HTML from the Worker, no framework, no third-party scripts; every action is a POST with
  Access's header checked and a same-origin check.

### Library format 2

```jsonc
{
  "format": 2,
  "version": 41,
  "built": "2026-10-22T04:00:00Z",
  "games": {
    "exe:bbq": {
      "name": "The First Berserker: Khazan",
      "death": [{ "id": "d7f3a1", "mask": "…", "values": "…", "aspect": 1777, "n": 5 }],
      "won":   [{ "id": "…", … }],
      "bars":  [{ "id": "…", "x0": 290, "x1": 710, "y0": 52, "y1": 62, "aspect": 1777, "n": 3 }],
      "bosses": ["Blade Phantom Ozma", "…"],
      "not_bosses": ["…"]
    }
  }
}
```

`n` is how many installs agreed (shown on the page: "taught by 5 players"). Deck reads format 1 too
(`prints` → `death`), so the embedded copy can be either during the change.

## Security and privacy

- Deck runs as administrator: every byte of the downloaded library still goes through `library::parse`
  with the format-2 additions checked the same strict way (spot via `Spot::sane`, names via the name
  rules, caps on every list). A failed check skips the item, never the library.
- The Worker's GitHub token is fine-grained, for this repo only, contents read/write. A repository
  ruleset protects `main` (no direct pushes) so the token can only usefully write the `library` branch.
- Logged by the Worker: counts and errors, never packs, install ids or addresses.
- `docs/trust-boundaries.md` gets a row for the new outbound request and the new download path; the
  Deaths settings say what is sent, in the words above.

## Deploying (the maintainer, once)

A `services/library-worker/README.md` with the exact commands: a free Cloudflare account,
`npx wrangler login`, `wrangler d1 create`, apply the schema, `wrangler secret put GITHUB_TOKEN` and
`TRUSTED_INSTALLS`, `wrangler deploy`, then in the Cloudflare dashboard: Zero Trust → Access → an
application for `<worker>/admin*` allowing only your email (and its team domain and audience tag set
as Worker variables for the header check). The Worker's address goes into Deck as a constant
(`library::SUBMIT_URL`); a hidden setting `deaths.library_service` overrides it for testing.

## What's removed

The issue form `.github/ISSUE_TEMPLATE/death_screen.yml`, the workflow
`.github/workflows/death-screen-intake.yml`, `tools/deaths-library/ingest.py`, and
`data/death-screens/library.json` (empty; nothing to move). "Submit to the shared library" on the
page becomes the share card above.

## Testing

- Rust: pack building (what goes in, what never does), friend codes (new and old), library format 2
  parsing with hostile input, precedence, the auto turn-off after two undone deaths, the outbox.
- Worker: `vitest` with Cloudflare's Workers test pool: validation from the shared fixtures, rate
  limits, the daily build's grouping, agreement and reporting rules, the commit only when changed, and
  the admin routes (refused without a valid Access header, decisions win over the rules, a removed
  item is refused at submit).
- Both run the same agreement fixtures (two prints taught from different frames of one death screen
  must agree; a death screen and a menu must not).
- By hand, after deploying: one pack from each of two PCs (or the maintainer's trusted install) shows
  up on the `library` branch the next day and in Deck the day after.

## Left to the builder

File layout inside `services/library-worker/`, the D1 schema, the exact wording of the share card,
how the outbox is retried within the hourly limit, and the grouping algorithm's details beyond the
agreement rule above.

## Settled while reading this back as the builder

- **Before the Worker is deployed** `library::SUBMIT_URL` is empty: Deck builds packs and keeps them in
  the outbox but sends nothing, and the share card doesn't show. Setting the address is a one-line
  change after deploying.
- **The embedded copy:** a release copies `library.json` from the `library` branch into
  `data/library/library.json` before building (a step in the release checklist, one download, by
  hand). The build never fetches anything itself.
- **An item only one install sent** (and not the maintainer's) waits until a second install sends it,
  or until you approve it on the admin page.
- **Agreement of two prints:** B's cells that fall outside A's mask are read as the background value,
  so two prints agree only when they mostly cover the same cells. The fixtures pin this down.

## Calls made without the owner (check these)

- Sending is **asked the first time** ("ask" by default), not on silently.
- The library lives on a **separate `library` branch**, not `main`.
- **Two installs from two networks** make an item public; **3+ reports, at least as many as senders,**
  remove it.
- Boss names typed by hand are **never** sent.
- The maintainer's own install counts as enough on its own (`TRUSTED_INSTALLS`).
- The admin page is guarded by **Cloudflare Access with your email** (no password), and the Worker
  checks Access's signed header itself too.
- Limits: **8** death and **8** victory screens, **4** bar places per game, both yours and the library's.
