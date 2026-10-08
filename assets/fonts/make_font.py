"""Backt Inters tabellarische Ziffern (tnum) in die cmap und erzeugt eine Latin-Teilmenge.
Eingabe: Inter-Medium.otf (Inter 3.19, SIL OFL 1.1). Ausgabe: assets/fonts/Inter-Medium-Tabular.otf"""
import sys
from fontTools.ttLib import TTFont
from fontTools import subset

src, dst = sys.argv[1], sys.argv[2]
font = TTFont(src)
gsub = font["GSUB"].table
lookups = set()
for rec in gsub.FeatureList.FeatureRecord:
    if rec.FeatureTag == "tnum":
        lookups.update(rec.Feature.LookupListIndex)
mapping = {}
for i in lookups:
    for st in gsub.LookupList.Lookup[i].SubTable:
        st = getattr(st, "ExtSubTable", st)
        if hasattr(st, "mapping"):
            mapping.update(st.mapping)
changed = 0
for table in font["cmap"].tables:
    for cp in range(0x30, 0x3A):
        g = table.cmap.get(cp)
        if g in mapping:
            table.cmap[cp] = mapping[g]; changed += 1
print("tnum-Ersetzungen in cmap:", changed, "von", len(mapping), "tnum-Glyphen")
font.save("tmp_tab.otf")

opts = subset.Options()
opts.layout_features = ["kern", "liga"]
opts.name_IDs = ["*"]
opts.notdef_outline = True
opts.glyph_names = False
unicodes = (list(range(0x20, 0x7F)) + list(range(0xA0, 0x100)) +
            [0x2013, 0x2014, 0x2018, 0x2019, 0x201A, 0x201C, 0x201D, 0x201E, 0x2022, 0x2026,
             0x2190, 0x2191, 0x2192, 0x2193, 0x2212, 0x00D7, 0x20AC])
f = subset.load_font("tmp_tab.otf", opts)
s = subset.Subsetter(opts); s.populate(unicodes=unicodes); s.subset(f)
subset.save_font(f, dst, opts)
import os; print("geschrieben:", dst, os.path.getsize(dst), "Bytes")
