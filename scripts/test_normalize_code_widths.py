"""配布元フォントなしでも、セル幅と組版参照の整合性を検証します。"""

import io
import unittest
import uharfbuzz as hb

from fontTools.feaLib.builder import addOpenTypeFeaturesFromString
from fontTools.fontBuilder import FontBuilder
from fontTools.pens.t2CharStringPen import T2CharStringPen
from fontTools.pens.boundsPen import BoundsPen
from fontTools.ttLib import TTFont

from normalize_code_widths import character_width, normalize


def fixture():
    order = [".notdef", "unreachable", "A", "Omega", "e", "acute", "eacute", "hangul",
             "line", "block", "emspace", "tone", "variant"]
    widths = [1000, 1000, 500, 758, 554, 0, 554, 920, 1000, 1000, 1000, 250, 1000]
    builder = FontBuilder(1000, isTTF=False)
    builder.setupGlyphOrder(order)
    builder.setupCharacterMap({0x41: "A", 0x03A9: "Omega", 0x65: "e",
                               0x301: "acute", 0xE9: "eacute", 0x3131: "hangul",
                               0xFFA1: "hangul", 0x2500: "line", 0x2588: "block",
                               0x2003: "emspace", 0x302E: "tone", 0x4E00: "variant"},
                              uvs=[(0x4E00, 0xE0100, "eacute")])
    builder.setupHorizontalMetrics({g: (w, 0) for g, w in zip(order, widths)})
    builder.setupHorizontalHeader(ascent=1000, descent=-200)
    builder.setupNameTable({"familyName": "Width Test", "styleName": "Regular"})
    builder.setupOS2(sTypoAscender=1000, sTypoDescender=-200)
    builder.setupPost()
    programs = {}
    for index, (glyph, width) in enumerate(zip(order, widths)):
        pen = T2CharStringPen(None, None, CFF2=True)
        right = width if width else 120
        pen.moveTo((0, 0))
        pen.lineTo((right, 0))
        pen.lineTo((right, 700 + index))
        pen.lineTo((0, 700 + index))
        pen.closePath()
        programs[glyph] = pen.getCharString()
    builder.setupCFF2(programs)
    addOpenTypeFeaturesFromString(builder.font, """
        markClass acute <anchor 60 0> @TOP;
        feature mark { pos base e <anchor 270 700> mark @TOP; } mark;
        feature ccmp { sub e acute by eacute; } ccmp;
        feature fwid { sub A by variant; } fwid;
    """)
    data = io.BytesIO()
    builder.font.save(data)
    return TTFont(io.BytesIO(data.getvalue()))


class WidthTests(unittest.TestCase):
    def test_point_only_contour_does_not_shift_glyph_during_compression(self):
        import cffsubr

        font = fixture()
        top = font["CFF2"].cff.topDictIndex[0]
        source = top.CharStrings["A"]
        pen = T2CharStringPen(None, None, CFF2=True)
        pen.moveTo((150, 350))
        pen.curveTo((150, 350), (150, 350), (150, 350))
        pen.closePath()
        pen.moveTo((20, 0))
        pen.lineTo((400, 0))
        pen.lineTo((400, 700))
        pen.lineTo((20, 700))
        pen.closePath()
        top.CharStrings["A"] = pen.getCharString(
            private=source.private, globalSubrs=source.globalSubrs, optimize=False)
        normalize(font)
        glyph = font.getBestCmap()[0x41]
        before = font.getGlyphSet()
        expected = BoundsPen(before)
        before[glyph].draw(expected)
        cffsubr.subroutinize(font)
        after = font.getGlyphSet()
        actual = BoundsPen(after)
        after[glyph].draw(actual)
        self.assertEqual(expected.bounds, actual.bounds)
        self.assertEqual(font["hmtx"][glyph][1], round(actual.bounds[0]))

    def test_character_properties_and_exceptions(self):
        for text, expected in [("AéΩæ─█ﾡ", 500), ("が한ㄱＡ\u2003", 1000),
                               ("\u0301\u3099\u302e\u200b", 0), ("\u00ad", 500)]:
            for char in text:
                with self.subTest(char=char):
                    self.assertEqual(expected, character_width(ord(char), 1000))

    def test_all_metrics_and_shared_glyphs_survive_roundtrip(self):
        font = fixture()
        before = font.getGlyphSet()
        expected_bounds = BoundsPen(before)
        before[font.getBestCmap()[0x41]].draw(expected_bounds)
        report = normalize(font)
        data = io.BytesIO()
        font.save(data)
        restored = TTFont(io.BytesIO(data.getvalue()))
        cmap = restored.getBestCmap()
        self.assertEqual({0, 500, 1000}, {a for a, _ in restored["hmtx"].metrics.values()})
        self.assertEqual(len(restored.getGlyphOrder()), report["glyphs"])
        self.assertNotEqual(cmap[0x3131], cmap[0xFFA1])
        self.assertEqual(1000, restored["hmtx"][cmap[0x3131]][0])
        self.assertEqual(500, restored["hmtx"][cmap[0xFFA1]][0])
        self.assertNotIn("unreachable", restored.getGlyphOrder())
        uvs = next(t for t in restored["cmap"].tables if t.format == 14)
        glyph = uvs.uvsDict[0xE0100][0][1]
        self.assertEqual(1000, restored["hmtx"][glyph][0])
        self.assertEqual(500, restored["hmtx"][cmap[0xE9]][0])
        after = restored.getGlyphSet()
        actual_bounds = BoundsPen(after)
        after[cmap[0x41]].draw(actual_bounds)
        self.assertEqual(expected_bounds.bounds, actual_bounds.bounds)

    def test_mark_anchor_follows_base_transform(self):
        font = fixture()
        normalize(font)
        table = font["GPOS"].table.LookupList.Lookup[0].SubTable[0]
        anchor = table.BaseArray.BaseRecord[0].BaseAnchor[0]
        self.assertEqual(round(270 * 500 / 554), anchor.XCoordinate)
        self.assertEqual(700, anchor.YCoordinate)

    def test_width_features_cannot_change_cells(self):
        font = fixture()
        normalize(font)
        for record in font["GSUB"].table.FeatureList.FeatureRecord:
            if record.FeatureTag == "fwid":
                self.assertEqual([], record.Feature.LookupListIndex)

    def test_alphabetic_marks_attach_and_stack_inside_base_cell(self):
        font = fixture()
        normalize(font)
        data = io.BytesIO()
        font.save(data)
        shaping_font = hb.Font(hb.Face(data.getvalue()))
        shaping_font.scale = (1000, 1000)
        buffer = hb.Buffer()
        buffer.add_str("A\u0301\u0301")
        buffer.guess_segment_properties()
        hb.shape(shaping_font, buffer)
        self.assertEqual(3, len(buffer.glyph_infos))
        self.assertEqual(500, sum(p.x_advance for p in buffer.glyph_positions))
        glyphs = font.getGlyphSet()
        order = font.getGlyphOrder()
        cursor = 0
        previous_top = None
        for info, position in zip(buffer.glyph_infos, buffer.glyph_positions):
            pen = BoundsPen(glyphs)
            glyphs[order[info.codepoint]].draw(pen)
            left, bottom, right, top = pen.bounds
            left += cursor + position.x_offset
            right += cursor + position.x_offset
            bottom += position.y_offset
            top += position.y_offset
            self.assertGreaterEqual(left, 0)
            self.assertLessEqual(right, 500)
            if previous_top is not None:
                self.assertGreater(bottom, previous_top)
            previous_top = top
            cursor += position.x_advance

    def test_harfbuzz_preserves_mixed_cells_with_width_features_enabled(self):
        font = fixture()
        normalize(font)
        data = io.BytesIO()
        font.save(data)
        shaping_font = hb.Font(hb.Face(data.getvalue()))
        shaping_font.scale = (1000, 1000)
        buffer = hb.Buffer()
        buffer.add_str("AΩㄱﾡe\u0301")
        buffer.guess_segment_properties()
        hb.shape(shaping_font, buffer, {"fwid": 1})
        self.assertEqual(3000, sum(p.x_advance for p in buffer.glyph_positions))
        self.assertTrue(all(g.codepoint for g in buffer.glyph_infos))


if __name__ == "__main__":
    unittest.main()
