"""Builds src-tauri/src/deaths/catalog.json (the death counter's game catalogue).

Run steamspy_tags.py here first (one SteamSpy request a minute, writes spy.json), then
`python build_catalog.py 10000`. Without spy.json only the hand-picked games are written.

Sources: ids.json (hand-picked games, Steam store search), spy.json (SteamSpy tag lists with
review counts, if present). Games from tags need >= MIN_REVIEWS reviews; hand-picked ones are
always in. Extras (death words, emulators, kind) are below.
"""
import html, json, os, re, sys

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(HERE, "..", "..", "src-tauri", "src", "deaths", "catalog.json")
MIN_REVIEWS = int(sys.argv[1]) if len(sys.argv) > 1 else 3000

JUNK = re.compile(r"soundtrack|\bost\b|demo|prologue|artbook|\bdlc\b|season pass|upgrade|bundle|playtest|editor|wallpaper|dedicated server|sdk", re.I)
# tags (or picks) that aren't about dying, and single games that slip through
NOT_DEATHY = {
    # puzzle / cosy / visual novels / sims that "Difficult" or "Hack and Slash" pull in
    "Patrick's Parabox", "Baba Is You", "Opus Magnum", "TIS-100", "EXAPUNKS", "Papers, Please", "Return of the Obra Dinn",
    "Trombone Champ", "Pro Office Calculator", "Recettear: An Item Shop's Tale", "Chicory: A Colorful Tale", "Antichamber",
    "Hooked on You: A Dead by Daylight Dating Sim™", "Bloons TD 6", "Balatro", "Keep Talking and Nobody Explodes", "Oxygen Not Included",
    "Hexcells", "Hexcells Infinite", "Tametsi", "Stephen's Sausage Roll", "5D Chess With Multiverse Time Travel", "Kingdom Rush  - Tower Defense",
    "Kingdom Rush Frontiers - Tower Defense", "Peggle Deluxe", "Insaniquarium Deluxe", "Spin Rhythm XD", "Beatblock", "Sudoku Mansion",
    "4D Minesweeper", "14 Minesweeper Variants", "Mount & Blade: Warband", "Counter-Strike: Source", "Half-Life", "Shadow Tactics: Blades of the Shogun",
    "Desperados III", "X-COM: UFO Defense", "Steam Engine Simulator", "Reentry - A Space Flight Simulator", "Supraland", "Supraland Six Inches Under",
    "Slay the Spire", "Slay the Spire 2", "Aseprite", "Blender", "Soundpad", "SteamVR", "Lossless Scaling", "Source Filmmaker",
    "EA SPORTS FIFA 21", "Football Manager 2022", "NBA 2K20", "NBA 2K21", "Pro Soccer Online", "UNO", "MONOPOLY PLUS", "Cooking Simulator",
    "PowerWash Simulator", "TCG Card Shop Simulator", "Papa's Freezeria Deluxe", "Game Dev Tycoon", "Banished", "Plague Inc: Evolved",
    "NEKOPARA Vol. 1", "NEKOPARA Vol. 2", "Senren＊Banka", "I Love You, Colonel Sanders! A Finger Lickin’ Good Dating Simulator",
    "Living With Sister: Monochrome Fantasy", "Leaf Blower Revolution - Idle Game", "Cat Goes Fishing", "Hand Simulator", "Golf With Your Friends",
    "Overcooked! 2", "PlateUp!", "Kerbal Space Program", "Microsoft Flight Simulator (2020) 40th Anniversary Edition", "Assetto Corsa Competizione",
    "DiRT Rally", "DiRT Rally 2.0", "F1 2020", "F1 22", "RIDE 4", "Forza Horizon 4", "FlatOut 2", "Hatsune Miku: Project DIVA Mega Mix+",
    "Muse Dash", "Beat Saber", "A Dance of Fire and Ice", "Mountain", "Frostpunk", "They Are Billions", "This War of Mine", "Men of War: Assault Squad 2",
    "BATTLETECH", "XCOM 2", "XCOM: Enemy Unknown", "Hue", "FEZ", "GRIS", "The Talos Principle", "Slay the Princess — The Pristine Cut",
    "Helltaker", "Lobotomy Corporation | Monster Management Simulation", "Library Of Ruina", "Divinity: Original Sin - Enhanced Edition",
    "Pathfinder: Wrath of the Righteous - Enhanced Edition", "Tyranny", "My Summer Car", "Besiege", "Elite Dangerous", "EVE Online",
    "Hardspace: Shipbreaker", "Craftopia", "Palworld", "Necesse", "The Escapists 2", "Who's Your Daddy?!", "Thronefall", "Dome Keeper",
    "Bad North: Jotunn Edition", "Totally Accurate Battle Simulator", "For The King", "Darkest Dungeon", "FTL: Faster Than Light",
    "Minion Masters", "Monster Sanctuary", "Metaphor: ReFantazio", "Persona 5 Strikers", "Inscryption", "Loop Hero", "Vampire Survivors", "Brotato", "Halls of Torment", "HoloCure - Save the Fans!",
}
# Steam ids that aren't games of their own (DLC listed as an app)
NOT_APPS = {2778580}

FROMSOFT_WORDS = ["YOU DIED"]
EXTRA = {
    # FromSoftware: the red "YOU DIED" banner
    1245620: {"words": FROMSOFT_WORDS, "exe": ["eldenring.exe", "start_protected_game.exe"]},
    570940: {"words": FROMSOFT_WORDS, "exe": ["darksoulsremastered.exe"]},
    211420: {"words": FROMSOFT_WORDS, "exe": ["darksouls.exe"], "save": "ds1"},
    236430: {"words": FROMSOFT_WORDS, "exe": ["darksoulsii.exe"]},
    335300: {"words": FROMSOFT_WORDS, "exe": ["darksoulsii.exe"]},
    374320: {"words": FROMSOFT_WORDS, "exe": ["darksoulsiii.exe"]},
    814380: {"exe": ["sekiro.exe"]},
    # save readers (deaths/save.rs)
    504230: {"save": "celeste", "exe": ["celeste.exe"]},
    2622380: {"words": FROMSOFT_WORDS, "exe": ["nightreign.exe"]},
    # the basic presets beyond FromSoftware (death words from videos and guides, unchecked in game)
    268910: {"words": ["YOU DIED"], "exe": ["cuphead.exe"]},
    # the publisher's Steam Awards post: "the words 'Khazan Has Fallen' burned forever into your screen"
    2680010: {"words": ["KHAZAN HAS FALLEN"]},
    485510: {"words": ["FREED FROM THIS MORTAL COIL"], "exe": ["nioh.exe"]},
    1325200: {"words": ["FREED FROM THIS MORTAL COIL"], "exe": ["nioh2.exe"]},
    1448440: {"words": ["COMPLETE FAILURE"], "exe": ["wolong.exe"]},
    1627720: {"exe": ["lop-win64-shipping.exe"]},
    2358720: {"exe": ["b1-win64-shipping.exe"]},
}
# games that only run in an emulator on PC
EMULATED = [
    {"steam": 0, "name": "Bloodborne", "exe": ["shadps4.exe"], "title": "Bloodborne", "kind": "boss", "words": FROMSOFT_WORDS},
    {"steam": 0, "name": "Demon's Souls", "exe": ["rpcs3.exe"], "title": "Demon's Souls", "kind": "boss", "words": FROMSOFT_WORDS},
]
KIND_BY_TAG = [("Precision Platformer", "level"), ("Action Roguelike", "run"), ("Roguelite", "run"), ("Roguelike", "run")]
LEVEL_PICKS = {504230, 40800, 581660, 240720, 1061090, 688130, 2567040, 1533420}
RUN_PICKS = {1145360, 1145350, 588650, 250900, 311690, 632360, 1649240, 418530, 881100, 247080, 2088570}


def clean(name):
    name = html.unescape(name).replace("\ufffd", "").replace("™", "").replace("®", "").strip()
    return re.sub(r"\s+", " ", name)


games = {}
ids = json.load(open(os.path.join(HERE, "ids.json"), encoding="utf-8"))
SKIP_PICKS = {"Crime Simulator", "Ashen Empires", "Lost Ruins of Arnak", "Lethal Company", "Sea of Stars", "Dark and Darker", "Resident Evil 4", "DOOM Eternal"}
for q, hits in ids.items():
    if not hits or hits[0][0] == "ERR":
        continue
    aid, name = hits[0]
    name = clean(name)
    if JUNK.search(name) or any(s in name for s in SKIP_PICKS) or aid in NOT_APPS:
        continue
    games[aid] = {"name": name, "tags": set(), "picked": True}
for aid, name in [(1325200, "Nioh 2 – The Complete Edition"), (774361, "Blasphemous"), (1123050, "GRIME"), (1110910, "Mortal Shell"),
                  (423230, "Furi"), (1145360, "Hades"), (236430, "DARK SOULS II"), (378540, "The Surge"), (211420, "DARK SOULS: Prepare To Die Edition"),
                  (2584270, "Mortal Shell II")]:
    games.setdefault(aid, {"name": name, "tags": set(), "picked": True})

spy_path = os.path.join(HERE, "spy.json")
if os.path.exists(spy_path):
    spy = json.load(open(spy_path, encoding="utf-8"))
    for tag, apps in spy.items():
        for aid, v in apps.items():
            aid = int(aid)
            n = (v.get("pos") or 0) + (v.get("neg") or 0)
            name = clean(v.get("name") or "")
            if not name or JUNK.search(name) or name in NOT_DEATHY or aid in NOT_APPS:
                continue
            if aid in games:
                games[aid]["tags"].add(tag)
                continue
            if n >= MIN_REVIEWS and tag not in ("Difficult", "Roguelite"):
                games[aid] = {"name": name, "tags": {tag}, "picked": False, "reviews": n}

out = []
seen_names = set()
for aid, g in sorted(games.items(), key=lambda kv: (not kv[1]['picked'], kv[0])):
    if g['name'].lower() in seen_names:
        continue
    seen_names.add(g['name'].lower())
    if g["name"] in NOT_DEATHY:
        continue
    kind = "boss"
    for tag, k in KIND_BY_TAG:
        if tag in g["tags"] and "Souls-like" not in g["tags"] and "Boss Rush" not in g["tags"]:
            kind = k
            break
    if aid in LEVEL_PICKS:
        kind = "level"
    if aid in RUN_PICKS:
        kind = "run"
    e = {"steam": aid, "name": g["name"], "kind": kind}
    e.update(EXTRA.get(aid, {}))
    out.append(e)
out.sort(key=lambda e: e["name"].lower())
out += EMULATED
json.dump(out, open(OUT, "w", encoding="utf-8"), indent=1, ensure_ascii=False)
print(len(out), "games;", sum(1 for e in out if e.get("words")), "with death words")
