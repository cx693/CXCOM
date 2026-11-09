"""Regenerate the embedded font subsets.

The UI font must cover the full GB2312 character set because received Chinese
text is arbitrary. Microsoft YaHei is used locally; for redistribution replace
it with a font whose license permits embedding.

    python tools/make_fonts.py
"""
import io
import json
import os
import sys
import urllib.request
import zipfile

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
LIB = os.path.join(ROOT, ".fonttools")
os.makedirs(LIB, exist_ok=True)

if not os.path.isdir(os.path.join(LIB, "fontTools")):
    meta = json.load(urllib.request.urlopen("https://pypi.org/pypi/fonttools/json", timeout=30))
    wheel = next(f["url"] for f in meta["urls"] if f["filename"].endswith("py3-none-any.whl"))
    zipfile.ZipFile(io.BytesIO(urllib.request.urlopen(wheel, timeout=60).read())).extractall(LIB)

sys.path.insert(0, LIB)
from fontTools import subset  # noqa: E402

ASSETS = os.path.join(ROOT, "assets")
os.makedirs(ASSETS, exist_ok=True)

chars = set(chr(c) for c in range(0x20, 0x7F))
chars.update("，。！？：；（）【】、…—·“”‘’《》→←↑↓●○■□▲△★☆")
for name in os.listdir(os.path.join(ROOT, "src")):
    if name.endswith(".rs"):
        chars.update(open(os.path.join(ROOT, "src", name), encoding="utf-8").read())
chars.discard("\x00")

text_file = os.path.join(ASSETS, "chars.txt")
open(text_file, "w", encoding="utf-8").write("".join(sorted(chars)))

subset.main([
    r"C:\Windows\Fonts\simsun.ttc", "--font-number=0",
    "--text-file=" + text_file,
    "--output-file=" + os.path.join(ASSETS, "ui_cjk.ttf"),
    "--no-hinting", "--drop-tables+=DSIG", "--layout-features=",
    "--name-IDs=1,2", "--notdef-outline",
])
subset.main([
    r"C:\Windows\Fonts\consola.ttf",
    "--text-file=" + text_file,
    "--output-file=" + os.path.join(ASSETS, "ui_mono.ttf"),
    "--no-hinting", "--drop-tables+=DSIG", "--layout-features=",
    "--name-IDs=1,2", "--notdef-outline",
])
for f in ("ui_cjk.ttf", "ui_mono.ttf"):
    print(f, os.path.getsize(os.path.join(ASSETS, f)), "bytes")
