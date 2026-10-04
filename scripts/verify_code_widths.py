#!/usr/bin/env python3
"""生成した Code Regular / Bold のメトリクスと実際の組版を検証します。"""

import argparse
import json
from collections import Counter
from pathlib import Path

import uharfbuzz as hb
from fontTools.ttLib import TTFont
from fontTools.pens.boundsPen import BoundsPen

from normalize_code_widths import character_width


def shape(font, text, features=None, language=None):
    buffer = hb.Buffer()
    buffer.add_str(text)
    buffer.guess_segment_properties()
    if language:
        buffer.language = language
    hb.shape(font, buffer, features or {})
    return [(info.codepoint, pos.x_advance, pos.x_offset, pos.y_offset)
            for info, pos in zip(buffer.glyph_infos, buffer.glyph_positions)]


def verify(path, reference=None):
    data = Path(path).read_bytes()
    font = TTFont(path)
    em = font["head"].unitsPerEm
    shaping_font = hb.Font(hb.Face(data))
    shaping_font.scale = (em, em)
    metrics = font["hmtx"].metrics
    counts = Counter(advance for advance, _ in metrics.values())
    assert set(counts) == {0, em // 2, em}, counts
    assert sum(counts.values()) == font["maxp"].numGlyphs == len(font.getGlyphOrder())
    cmap = font.getBestCmap()
    for cp, glyph in cmap.items():
        assert character_width(cp, em) == metrics[glyph][0], (hex(cp), glyph, metrics[glyph])
    uvs = { (cp, vs): glyph for table in font["cmap"].tables if table.format == 14
            for vs, rows in table.uvsDict.items() for cp, glyph in rows }
    for (cp, vs), glyph in uvs.items():
        expected = character_width(cp, em)
        assert metrics[glyph or cmap[cp]][0] == expected, (cp, vs)
        actual = shape(shaping_font, chr(cp) + chr(vs))
        assert sum(row[1] for row in actual) == expected, (cp, vs, actual)
        if glyph:
            assert actual[0][0] == font.getGlyphID(glyph), (cp, vs, actual)
    if reference:
        original = TTFont(reference)
        assert set(cmap) == set(original.getBestCmap()), "Unicode mappings lost"
        previous_uvs = {(cp, vs) for t in original["cmap"].tables if t.format == 14
                        for vs, rows in t.uvsDict.items() for cp, _ in rows}
        assert set(uvs) == previous_uvs, "Variation sequences lost"

    samples = {
        "AéΩæ─█ﾡ": 7 * em // 2,
        "┌────────┐": 10 * em // 2,
        "│abc 日本│": 10 * em // 2,
        "│éΩæ 한  │": 10 * em // 2,
        "が": em, "か\u3099": em,
        "é": em // 2, "e\u0301": em // 2, "x\u0301": em // 2,
        "한": em, "\u1112\u1161\u11ab": em,
        "\u1113\u1161\u11a8": em,
        "한\u302e": em, "\u1113\u1161\u11a8\u302f": em,
        "fi": em, "---": 3 * em // 2,
    }
    results = {}
    for text, expected in samples.items():
        for features in ({}, {"fwid": 1, "hwid": 1, "pwid": 1, "halt": 1,
                              "liga": 1, "dlig": 1, "aalt": 1}):
            rows = shape(shaping_font, text, features)
            assert sum(row[1] for row in rows) == expected, (text, features, rows, expected)
            assert all(row[0] != 0 for row in rows), (text, "missing glyph")
        results[text] = rows
    # 日本語・中国語の文書でも、古い Jamo 列を三文字幅へ分解しません。
    for language in ("ja", "ko", "en", "zh"):
        for text in ("ᄓᅡᆨ", "ᄓᅡᆨ〯"):
            rows = shape(shaping_font, text, language=language)
            assert sum(row[1] for row in rows) == em, (language, text, rows)
    # 罫線・ブロック要素は左右の境界位置を維持します。
    glyph_set = font.getGlyphSet()
    # サンプル組版だけでは、特定の字形の圧縮による位置ずれを検出できません。
    for glyph, (_, lsb) in metrics.items():
        pen = BoundsPen(glyph_set)
        glyph_set[glyph].draw(pen)
        if pen.bounds:
            assert abs(lsb - pen.bounds[0]) <= 1, (glyph, lsb, pen.bounds)
        else:
            assert lsb == 0, (glyph, lsb)
    for cp in (0x2500, 0x253C, 0x2580, 0x2584, 0x2588):
        glyph = cmap[cp]
        pen = BoundsPen(glyph_set)
        glyph_set[glyph].draw(pen)
        x0, _, x1, _ = pen.bounds
        assert abs(x0) < 1 and abs(x1 - em // 2) < 1, (cp, pen.bounds)
    # 幅がゼロでも、アクセントが次のセルに出ていれば不正です。
    for text in ("x\u0301", "Q\u0307", "Ω\u0307", "Ж\u0301", "x\u0301\u0307"):
        rows = shape(shaping_font, text)
        assert len(rows) >= 2, (text, rows)
        origin = 0
        previous_top = None
        for index, (gid, advance, dx, dy) in enumerate(rows):
            pen = BoundsPen(glyph_set)
            glyph_set[font.getGlyphName(gid)].draw(pen)
            x0, y0, x1, y1 = pen.bounds
            left, right = origin + dx + x0, origin + dx + x1
            bottom, top = dy + y0, dy + y1
            if index:
                assert -1 <= left < right <= em // 2 + 1, (text, left, right)
                assert bottom >= previous_top, (text, bottom, previous_top)
            previous_top = top
            origin += advance
    return {"font": str(path), "glyphs": len(metrics), "advances": dict(counts),
            "unicode_mappings": len(cmap), "variation_sequences": len(uvs),
            "samples": results}, {cp: metrics[glyph][0] for cp, glyph in cmap.items()}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("fonts", nargs="+")
    parser.add_argument("--references", nargs="+")
    args = parser.parse_args()
    references = args.references or [None] * len(args.fonts)
    if len(references) != len(args.fonts):
        parser.error("Provide one reference for each font")
    classifications = []
    for path, reference in zip(args.fonts, references):
        report, classes = verify(path, reference)
        print(json.dumps(report, ensure_ascii=False, indent=2))
        classifications.append(classes)
    assert all(c == classifications[0] for c in classifications), "Weight classifications differ"


if __name__ == "__main__":
    main()
