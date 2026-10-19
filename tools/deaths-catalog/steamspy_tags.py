import json, os, time, urllib.parse, urllib.request
tags = ["Souls-like", "Boss Rush", "Precision Platformer", "Action Roguelike", "Metroidvania", "Hack and Slash", "Difficult", "Roguelite"]
out = json.load(open("spy.json", encoding="utf-8")) if os.path.exists("spy.json") else {}
for i, t in enumerate(tags):
    if t in out:
        continue
    if i:
        time.sleep(60)
    url = "https://steamspy.com/api.php?request=tag&tag=" + urllib.parse.quote(t)
    req = urllib.request.Request(url, headers={"User-Agent": "Mozilla/5.0"})
    try:
        d = json.load(urllib.request.urlopen(req, timeout=60))
    except Exception as e:
        print("ERR", t, e, flush=True)
        continue
    out[t] = {k: {"name": v.get("name"), "pos": v.get("positive", 0), "neg": v.get("negative", 0), "owners": v.get("owners")} for k, v in d.items()}
    json.dump(out, open("spy.json", "w", encoding="utf-8"), ensure_ascii=False)
    print(t, len(d), flush=True)
print("done")
