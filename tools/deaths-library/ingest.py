"""Adds the death screens in a "Death screen for the shared library" issue to
data/death-screens/library.json (run by .github/workflows/death-screen-intake.yml, which opens a
pull request for the maintainer to review).

  ISSUE_BODY=... python tools/deaths-library/ingest.py [library.json]   -> writes the file, prints a summary
  python tools/deaths-library/ingest.py --selftest

The checks mirror Deck's own (src-tauri/src/deaths/library.rs and screen/teach.rs): a code is
base64 JSON {deck_death_screen: 1, game, name, prints: [{mask, values, made}]}; a print's mask is
64*36 bits (hex), `values` holds one byte (hex) per set bit, at least 24 cells and some variation;
a game key is exe:/emu:<name> (no store numbers). Nothing here runs the issue's text: it is
only parsed as data.
"""
import base64, json, os, re, sys

TW, TH = 64, 36
MIN_CELLS = 24
MIN_SPREAD = 8.0
# Deck uses at most this many library prints per game (MAX_PER_GAME * 2 in library.rs)
MAX_PER_GAME = 6
KEY = re.compile(r"^(exe|emu):[A-Za-z0-9._-]{1,64}$")
CODE = re.compile(r"deck-death-screen:([A-Za-z0-9+/=\s]+)")
DEFAULT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "data", "death-screens", "library.json")


def valid_print(p):
    """None when fine, else the reason."""
    if not isinstance(p, dict):
        return "a print isn't an object"
    mask, values = p.get("mask"), p.get("values")
    if not isinstance(mask, str) or not isinstance(values, str):
        return "mask and values must be text"
    try:
        m, v = bytes.fromhex(mask), bytes.fromhex(values)
    except ValueError:
        return "mask or values aren't hex"
    if len(m) != TW * TH // 8:
        return "the mask is the wrong size"
    cells = sum(bin(b).count("1") for b in m)
    if cells != len(v) or cells < MIN_CELLS:
        return "the mask and values don't match, or too few cells"
    mean = sum(v) / len(v)
    if sum(abs(x - mean) for x in v) / len(v) < MIN_SPREAD:
        return "a flat picture can't be a death screen"
    return None


def aspect(v):
    """The picture's shape (width / height x 1000) the print was taught on; 0 when unknown."""
    if isinstance(v, bool) or not isinstance(v, int) or not 0 <= v <= 10000:
        return 0
    return v


def preview(p):
    """The print drawn in text, for the pull request: the reviewer sees what will be matched
    (cells left out are '.', the rest from dark ' ' to bright '@')."""
    m, v = bytes.fromhex(p["mask"]), bytes.fromhex(p["values"])
    shade = " :-=+*#%@"
    rows, k = [], 0
    for y in range(TH):
        row = ""
        for x in range(TW):
            i = y * TW + x
            if m[i // 8] & (1 << (i % 8)):
                row += shade[min(len(shade) - 1, v[k] * len(shade) // 256)]
                k += 1
            else:
                row += "."
        rows.append(row)
    return "\n".join(rows)


def made(v):
    """The teaching time, as a number Deck can read back (a u64; anything else becomes 0)."""
    if isinstance(v, bool) or not isinstance(v, int) or not 0 <= v < 2**53:
        return 0
    return v


def read_code(body):
    m = CODE.search(body or "")
    if not m:
        raise ValueError("no deck-death-screen: code found")
    try:
        data = json.loads(base64.b64decode("".join(m.group(1).split()), validate=True))
    except Exception:
        raise ValueError("the code can't be read")
    if not isinstance(data, dict) or data.get("deck_death_screen") != 1:
        raise ValueError("not a Deck death-screen code")
    game, name, prints = data.get("game"), data.get("name"), data.get("prints")
    if not isinstance(game, str) or not KEY.match(game):
        raise ValueError("the game key isn't one the library accepts")
    if not isinstance(prints, list) or not prints:
        raise ValueError("no prints in the code")
    for p in prints:
        why = valid_print(p)
        if why:
            raise ValueError(why)
    # the name is shown in a pull request and a comment: letters, digits and a few marks only (no @mentions, links, markup)
    name = re.sub(r"[^\w .:'&()+-]", "", str(name or game))[:100].strip() or game
    return game, name, [{"mask": p["mask"], "values": p["values"], "made": made(p.get("made")), "aspect": aspect(p.get("aspect"))} for p in prints]


def add(lib, game, name, prints):
    """Adds the prints to the library dict; returns how many were new."""
    g = lib.setdefault("games", {}).setdefault(game, {"name": name, "prints": []})
    n = 0
    for p in prints:
        if len(g["prints"]) >= MAX_PER_GAME:
            break
        if not any(q["mask"] == p["mask"] and q["values"] == p["values"] for q in g["prints"]):
            g["prints"].append(p)
            n += 1
    if n:
        lib["version"] = int(lib.get("version", 1)) + 1
    elif not g["prints"]:
        del lib["games"][game]
    return n


def main(path, body):
    game, name, prints = read_code(body)
    lib = json.load(open(path, encoding="utf-8")) if os.path.exists(path) else {"version": 1, "games": {}}
    n = add(lib, game, name, prints)
    if not n:
        print(f"nothing new for {name} ({game}): already in the library, or it has {MAX_PER_GAME} already")
        return 1
    json.dump(lib, open(path, "w", encoding="utf-8", newline="\n"), indent=1, ensure_ascii=False, sort_keys=True)
    open(path, "a", encoding="utf-8", newline="\n").write("\n")
    print(f"added {n} death screen{'s' if n != 1 else ''} for {name} ({game}); library version {lib['version']}")
    for i, p in enumerate(prints):
        shape = f", taught on a {p['aspect'] / 1000:.2f}:1 picture" if p["aspect"] else ""
        print(f"\nDeath screen {i + 1}{shape} (what Deck will match; '.' is left out):\n```\n{preview(p)}\n```")
    return 0


def selftest():
    cells = 40
    bits = bytearray(TW * TH // 8)
    for i in range(cells):
        bits[i // 8] |= 1 << (i % 8)
    good = {"mask": bits.hex(), "values": bytes(100 + i * 3 for i in range(cells)).hex(), "made": 5}
    code = lambda **kw: "Text before\n### The code\n\ndeck-death-screen:" + base64.b64encode(json.dumps({"deck_death_screen": 1, "game": "exe:sekiro", "name": "Sekiro", "prints": [good], **kw}).encode()).decode() + "\n\n### Game\n\nSekiro"
    assert (made(5), made(-1), made(10**30), made(True), made("7"), made(1.5), made(None)) == (5, 0, 0, 0, 0, 0, 0)
    assert (aspect(1777), aspect(-1), aspect(99999), aspect("x"), aspect(None)) == (1777, 0, 0, 0, 0)
    pv = preview(good).split("\n")
    assert len(pv) == TH and all(len(r) == TW for r in pv) and pv[0][:cells].count(".") == 0 and pv[1].strip(".") == "", "the drawing has the print's cells"
    assert read_code(code(prints=[{**good, "aspect": 1777}]))[2][0]["aspect"] == 1777
    assert read_code(code(prints=[{**good, "made": 10**30}]))[2][0]["made"] == 0, "a huge number would break every client's library"
    assert valid_print(good) is None
    assert valid_print({**good, "values": bytes(cells).hex()}) is not None, "flat"
    assert valid_print({**good, "mask": "00"}) is not None, "short mask"
    assert valid_print({**good, "values": "zz"}) is not None, "not hex"
    nm = read_code(code(name="@everyone [click](http://x) `rm`"))[1]
    assert not any(c in nm for c in "@[]`<>*_#"), nm
    g, n, p = read_code(code())
    assert (g, n, len(p)) == ("exe:sekiro", "Sekiro", 1)
    for bad in ("", "no code here", "deck-death-screen:!!!", code(game="../../x"), code(prints=[])):
        try:
            read_code(bad)
        except ValueError:
            continue
        raise AssertionError(f"accepted: {bad[:40]!r}")
    lib = {"version": 1, "games": {}}
    assert add(lib, g, n, p) == 1 and lib["version"] == 2
    assert add(lib, g, n, p) == 0 and lib["version"] == 2, "a duplicate changes nothing"
    import tempfile
    d = tempfile.mkdtemp()
    f = os.path.join(d, "lib.json")
    json.dump({"version": 1, "games": {}}, open(f, "w"))
    assert main(f, code()) == 0 and json.load(open(f))["version"] == 2
    assert main(f, code()) == 1
    print("selftest ok")


if __name__ == "__main__":
    if sys.argv[1:] == ["--selftest"]:
        selftest()
    else:
        try:
            sys.exit(main(sys.argv[1] if len(sys.argv) > 1 else DEFAULT, os.environ.get("ISSUE_BODY", "")))
        except ValueError as e:
            print(f"can't add it: {e}")
            sys.exit(2)
