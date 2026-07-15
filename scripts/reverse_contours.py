# TrueType (glyf) フォントの全グリフの輪郭の巻き方向を反転するスクリプトである。
#
# fontTools.varLib.instancer でインスタンス化した TrueType フォントは、
# CFF ベースのフォントと外郭の巻き方向が逆 (時計回り) になっている。
# 本ツールの丸め処理 (src/round.rs の corner_radius) は CFF の慣例
# (外郭が反時計回り) を前提とするため、変換前にこのスクリプトで
# 巻き方向を揃えておく必要がある。
#
# 使い方: python3 scripts/reverse_contours.py <入力.ttf> <出力.ttf>
import sys

from fontTools.pens.recordingPen import RecordingPen
from fontTools.pens.reverseContourPen import ReverseContourPen
from fontTools.pens.ttGlyphPen import TTGlyphPen
from fontTools.ttLib import TTFont

input_path, output_path = sys.argv[1], sys.argv[2]
font = TTFont(input_path)
glyph_set = font.getGlyphSet()
glyf_table = font["glyf"]

for glyph_name in font.getGlyphOrder():
    glyph = glyf_table[glyph_name]
    if glyph.isComposite() or glyph.numberOfContours <= 0:
        continue
    recording = RecordingPen()
    glyph_set[glyph_name].draw(ReverseContourPen(recording))
    tt_pen = TTGlyphPen(glyph_set)
    recording.replay(tt_pen)
    glyf_table[glyph_name] = tt_pen.glyph()

font.save(output_path)
