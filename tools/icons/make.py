"""Renders Deck's profile icons: white Fluent glyph on a glossy gradient rounded tile (the
style of the existing PT CPU icons), 256 px via headless Edge, then multi-size .ico."""
import os, re, subprocess, sys
from PIL import Image

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = sys.argv[1]
EDGE = r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe"

# name: (glyph, top colour, bottom colour)
ICONS = {
    "gpu-green": ("developer_board", "#7ee08a", "#1f9d45"),
    "gpu-blue": ("developer_board", "#79c6ff", "#1f6fe0"),
    "gpu-amber": ("developer_board", "#ffd36b", "#f08a12"),
    "gpu-red": ("developer_board", "#ff9a7a", "#d8283f"),
    "gpu-flame": ("fire", "#ffb06b", "#e2341d"),
    "gpu-snow": ("weather_snowflake", "#8ee8ff", "#1b8fd8"),
    "gpu-leaf": ("leaf_one", "#9be38a", "#1f9f6a"),
    "fan": ("weather_squalls", "#9fd8ff", "#2a7fd0"),
    "bolt": ("flash", "#ffe27a", "#f0a014"),
    "gamepad": ("games", "#c4a2ff", "#6a3fe0"),
    "film": ("movies_and_tv", "#ff9ec4", "#d6337a"),
    "moon": ("weather_moon", "#a99cff", "#4a35c8"),
    "sun": ("weather_sunny", "#ffd56b", "#f58a1f"),
    "shield": ("shield", "#9fb4d8", "#3f5a8f"),
    "turtle": ("animal_turtle", "#7fe0c8", "#14907a"),
    "rabbit": ("animal_rabbit", "#ffb3d0", "#d64f8a"),
    "headphones": ("headphones", "#d0a6ff", "#8a3fd0"),
    "code": ("code", "#a9b8c8", "#46586c"),
    "battery": ("battery_charge", "#a6ec8a", "#2c9f3a"),
    "gauge": ("gauge", "#ffd27a", "#ef8a17"),
    "rocket": ("rocket", "#ff9f86", "#d8304a"),
}

TPL = """<!doctype html><html><head><style>
html,body{margin:0;background:transparent;width:256px;height:256px;overflow:hidden}
.t{position:absolute;left:4px;top:4px;width:248px;height:248px;border-radius:54px;
 background:linear-gradient(160deg,%(a)s 0%%,%(b)s 100%%);
 box-shadow:inset 0 0 0 2px rgba(255,255,255,.28), inset 0 -10px 24px rgba(0,0,0,.18);overflow:hidden}
.g{position:absolute;left:-30%%;top:-62%%;width:160%%;height:100%%;border-radius:50%%;
 background:linear-gradient(180deg,rgba(255,255,255,.30),rgba(255,255,255,.06))}
svg{position:absolute;left:52px;top:52px;width:152px;height:152px;filter:drop-shadow(0 6px 8px rgba(0,0,0,.28))}
svg path{fill:#fff}
</style></head><body><div class="t"><div class="g"></div></div>%(svg)s</body></html>"""

os.makedirs(OUT, exist_ok=True)
tmp = os.path.join(HERE, "render")
os.makedirs(tmp, exist_ok=True)
for name, (glyph, a, b) in ICONS.items():
    svg = open(os.path.join(HERE, "svg", glyph + ".svg"), encoding="utf-8").read()
    svg = re.sub(r'fill="#[0-9A-Fa-f]+"', 'fill="#fff"', svg)
    html = os.path.join(tmp, name + ".html")
    open(html, "w", encoding="utf-8").write(TPL % {"a": a, "b": b, "svg": svg})
    png = os.path.join(tmp, name + ".png")
    subprocess.run([EDGE, "--headless=new", "--disable-gpu", "--hide-scrollbars", "--force-device-scale-factor=1",
                    "--default-background-color=00000000", f"--screenshot={png}", "--window-size=256,256",
                    "file:///" + html.replace("\\", "/")], check=True, capture_output=True, timeout=60)
    im = Image.open(png).convert("RGBA").crop((0, 0, 256, 256))
    im.save(os.path.join(OUT, name + ".ico"), sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (256, 256)])
    im.save(os.path.join(tmp, name + "-final.png"))
    print(name)

# contact sheet
names = list(ICONS)
sheet = Image.new("RGBA", (8 * 136, ((len(names) + 7) // 8) * 136), (36, 36, 40, 255))
for i, n in enumerate(names):
    im = Image.open(os.path.join(tmp, n + "-final.png")).resize((128, 128), Image.LANCZOS)
    sheet.paste(im, ((i % 8) * 136 + 4, (i // 8) * 136 + 4), im)
sheet.save(os.path.join(HERE, "sheet2.png"))
