#!/usr/bin/env python3
"""Code フォントのセル幅と、輪郭・組版参照を整合させます。"""

import argparse
import copy
import itertools
import math
from collections import Counter, defaultdict
from types import SimpleNamespace

import unicodedata2 as unicode
from fontTools import subset
from fontTools.fontBuilder import FontBuilder
from fontTools.misc.roundTools import otRound
from fontTools.otlLib import builder as layout_builder
from fontTools.pens.boundsPen import BoundsPen
from fontTools.pens.recordingPen import RecordingPen
from fontTools.pens.t2CharStringPen import T2CharStringPen
from fontTools.pens.transformPen import TransformPen
from fontTools.ttLib import TTFont, newTable
from fontTools.ttLib.tables import otTables

UNICODE_VERSION = "16.0.0"
# 明示的に有効化してもセル幅を変えないよう、これらの機能を取り除きます。
WIDTH_FEATURES = {"aalt", "fwid", "hwid", "pwid", "halt", "vhal", "liga", "dlig"}
SUPPORTED_TABLES = {
    "GlyphOrder", "BASE", "CFF2", "DSIG", "GDEF", "GPOS", "GSUB", "OS/2",
    "VORG", "cmap", "head", "hhea", "hmtx", "maxp", "name", "post",
    "vhea", "vmtx",
}


def character_width(cp, em):
    """元の字送り幅ではなく、固定版の Unicode 特性から分類します。"""
    if unicode.unidata_version != UNICODE_VERSION:
        raise ValueError(f"Unicode {UNICODE_VERSION} is required")
    char = chr(cp)
    # SOFT HYPHEN は可視化された際のハイフンを半角セルに置きます。
    if cp == 0x00AD:
        return em // 2
    if cp in {0x302E, 0x302F} or unicode.category(char) in {"Mn", "Me", "Cf"}:
        return 0
    # em/en の名前を持つ空白は EAW より優先します。
    if cp in {0x2001, 0x2003}:
        return em
    if cp in {0x2000, 0x2002}:
        return em // 2
    # 初・中・終声は単独表示では全角、合成用の置換字形は別にゼロ幅にします。
    if (0x1100 <= cp <= 0x11FF or 0xA960 <= cp <= 0xA97F
            or 0xD7B0 <= cp <= 0xD7FF):
        return em
    return em if unicode.east_asian_width(char) in {"W", "F"} else em // 2


def subtables(font, tag):
    """Extension lookup の中身も同じ処理で扱います。"""
    if tag not in font:
        return
    for lookup in font[tag].table.LookupList.Lookup:
        for table in lookup.SubTable:
            yield getattr(table, "ExtSubTable", table)


def remove_width_features(font):
    """lookup の番号を維持し、参照されなくなった lookup は subset に委ねます。"""
    for tag in ("GSUB", "GPOS"):
        if tag in font:
            for record in font[tag].table.FeatureList.FeatureRecord:
                if record.FeatureTag in WIDTH_FEATURES:
                    record.Feature.LookupListIndex = []
                    record.Feature.LookupCount = 0
            # 到達不能な lookup を落とし、無効化した機能が分類へ伝播するのを防ぎます。
            font[tag].prune_lookups()


def compact(font):
    """Unicode・UVS・全 GSUB 機能の closure を保ち、全テーブルを再構築します。"""
    unknown = set(font.keys()) - SUPPORTED_TABLES
    if unknown:
        raise ValueError(f"Unaudited tables: {sorted(unknown)}")
    options = subset.Options()
    options.layout_features = ["*"]
    options.name_IDs = ["*"]
    options.name_languages = ["*"]
    options.name_legacy = True
    options.notdef_outline = True
    options.recalc_timestamp = False
    options.glyph_names = True
    options.hinting = True
    worker = subset.Subsetter(options=options)
    # format 14 の selector を含め、全 Unicode マッピングを残します。
    codepoints = set()
    for table in font["cmap"].tables:
        if table.format == 14:
            codepoints.update(table.uvsDict)
            codepoints.update(cp for rows in table.uvsDict.values() for cp, _ in rows)
        elif table.isUnicode():
            codepoints.update(table.cmap)
    worker.populate(unicodes=codepoints)
    # 全輪郭をこのあと再構築するため、古いサブルーチンの解析・再番号付けは
    # 不要です。CFF2 だけを一時的に外し、他のテーブルには通常の subset を適用します。
    cff = font["CFF2"]
    # CFF2 の charset は TTFont の字形順を遅延参照するため、順序を変える前に
    # CharStrings と元の charset を確定させます。
    original_top = cff.cff.topDictIndex[0]
    _ = original_top.CharStrings
    _ = original_top.charset
    del font["CFF2"]
    try:
        worker.subset(font)
    finally:
        font["CFF2"] = cff
    cff.subset_glyphs(SimpleNamespace(glyphs=worker.glyphs_retained,
                                    glyphs_emptied=worker.glyphs_emptied,
                                    options=options))


def substitution_pairs(tables):
    """幅を継承する単一置換・選択置換・合成の辺を列挙します。"""
    for table in tables:
        yield from getattr(table, "mapping", {}).items()
        for source, targets in getattr(table, "alternates", {}).items():
            for target in targets:
                yield source, target
        for source, ligatures in getattr(table, "ligatures", {}).items():
            for ligature in ligatures:
                yield source, ligature.LigGlyph


def classify(font, em, dormant_pairs):
    """直接・UVS の文字特性を GSUB の出力へ伝播します。"""
    widths = defaultdict(set)
    hangul = set()
    for table in font["cmap"].tables:
        if table.format == 14:
            for rows in table.uvsDict.values():
                for cp, glyph in rows:
                    if glyph is not None:
                        widths[glyph].add(character_width(cp, em))
        elif table.isUnicode():
            for cp, glyph in table.cmap.items():
                widths[glyph].add(character_width(cp, em))
                if (0x1100 <= cp <= 0x11FF or 0xA960 <= cp <= 0xA97F
                        or 0xD7B0 <= cp <= 0xD7FF):
                    hangul.add(glyph)

    # ccmp の連続ダッシュや反復記号はセル数を縮めるため除きます。
    # Jamo 合成と、基底文字 + ゼロ幅記号の合成は維持します。
    for table in subtables(font, "GSUB"):
        if hasattr(table, "ligatures"):
            for first, ligatures in list(table.ligatures.items()):
                table.ligatures[first] = [
                    lig for lig in ligatures
                    if first in hangul or all(widths[g] == {0} for g in lig.Component)
                ]
                if not table.ligatures[first]:
                    del table.ligatures[first]

    tables = list(subtables(font, "GSUB"))
    while True:
        previous = {g: set(w) for g, w in widths.items()}
        for table in tables:
            for source, target in substitution_pairs([table]):
                if not widths[source]:
                    continue
                # Noto の合成用 Jamo は元から字送り幅が 0 です。
                widths[target].update({0} if font["hmtx"][target][0] == 0
                                      else widths[source])
                if source in hangul:
                    hangul.add(target)
        if previous == dict(widths):
            break

    # 除外した通常文字の合成置換だけから到達する字形も残しているため、
    # 未分類の出力に限ってその元文字の分類を継承します。
    while True:
        changed = False
        for source, target in dormant_pairs:
            if widths[source] and not widths[target]:
                widths[target] = ({0} if font["hmtx"][target][0] == 0
                                  else {max(widths[source])})
                changed = True
        if not changed:
            break

    for glyph in font.getGlyphOrder():
        if not widths[glyph]:
            # .notdef と、GSUB の文脈専用字形の最後の分類です。
            # 識別不能な任意幅を近似せず、既存のセル役割のみを認めます。
            advance = font["hmtx"][glyph][0]
            if advance == 0:
                widths[glyph] = {0}
            elif advance in {em, 920 * em // 1000}:
                widths[glyph] = {em}
            elif advance == em // 2:
                widths[glyph] = {em // 2}
            else:
                raise ValueError(f"Unclassified glyph: {glyph}, advance={advance}")
    return widths, hangul


def split_glyphs(font, widths):
    """幅が衝突した字形を複製し、元字形・幅から出力字形への対応を返します。"""
    order = font.getGlyphOrder()
    variants = {}
    origins = {}
    top = font["CFF2"].cff.topDictIndex[0]
    cs = top.CharStrings
    for glyph in list(order):
        # 通常の全角参照を元字形に残し、半角参照を別字形にします。
        choices = sorted(widths[glyph], reverse=True)
        for index, width in enumerate(choices):
            name = glyph if index == 0 else f"{glyph}.cell{width}"
            variants[glyph, width] = name
            origins[name] = (glyph, width)
            if index == 0:
                continue
            if name in order:
                raise ValueError(f"Glyph name collision: {name}")
            # サブルーチンの共有は保持し、字形オブジェクトだけを複製します。
            charstring = copy.copy(cs[glyph])
            if cs.charStringsAreIndexed:
                cs.charStrings[name] = len(cs.charStringsIndex)
                cs.charStringsIndex.append(charstring)
                if hasattr(top, "FDSelect"):
                    top.FDSelect.gidArray.append(top.FDSelect.gidArray[order.index(glyph)])
            else:
                cs.charStrings[name] = charstring
            order.append(name)
            for tag in ("hmtx", "vmtx"):
                if tag in font:
                    font[tag][name] = font[tag][glyph]
            if "VORG" in font and glyph in font["VORG"].VOriginRecords:
                font["VORG"].VOriginRecords[name] = font["VORG"].VOriginRecords[glyph]
    if len(order) > 65535:
        raise ValueError(f"Glyph limit exceeded: {len(order)}")
    font.setGlyphOrder(order)
    top.charset = order
    top.numGlyphs = len(order)
    font["maxp"].numGlyphs = len(order)
    return variants, origins


def update_references(font, widths, variants, em):
    """複製後の cmap・GSUB・GDEF 参照を更新します。"""
    for table in font["cmap"].tables:
        if table.format == 14:
            table.uvsDict = {vs: [(cp, variants[g, character_width(cp, em)]
                                  if g else None) for cp, g in rows]
                             for vs, rows in table.uvsDict.items()}
        elif table.isUnicode():
            table.cmap = {cp: variants[g, character_width(cp, em)]
                          for cp, g in table.cmap.items()}

    def output(glyph, width):
        return variants[glyph, 0 if font["hmtx"][glyph][0] == 0 else width]

    for table in subtables(font, "GSUB"):
        if hasattr(table, "mapping"):
            table.mapping = {variants[g, w]: output(out, w)
                             for g, out in table.mapping.items() for w in widths[g]}
        if hasattr(table, "alternates"):
            table.alternates = {variants[g, w]: [output(out, w) for out in outs]
                                for g, outs in table.alternates.items() for w in widths[g]}
        if hasattr(table, "ligatures"):
            result = defaultdict(list)
            for first, ligatures in table.ligatures.items():
                for lig in ligatures:
                    for values in itertools.product(*(sorted(widths[g])
                                                      for g in [first] + lig.Component)):
                        new = copy.deepcopy(lig)
                        new.Component = [variants[g, w] for g, w in
                                         zip(lig.Component, values[1:])]
                        new.LigGlyph = output(lig.LigGlyph, values[0])
                        result[variants[first, values[0]]].append(new)
            table.ligatures = dict(result)
        # Noto の文脈置換は format 3 の Coverage です。未知の形式は黙認しません。
        if hasattr(table, "Format") and table.__class__.__name__ in {
                "ChainContextSubst", "ContextSubst"}:
            if table.Format != 3:
                raise ValueError("Only coverage-based contextual substitutions are supported")
            for key, value in vars(table).items():
                if key.endswith("Coverage"):
                    coverages = value if isinstance(value, list) else [value]
                    for coverage in coverages:
                        coverage.glyphs = sorted(
                            {variants[g, w] for g in coverage.glyphs for w in widths[g]},
                            key=font.getGlyphID)
    if "GDEF" in font:
        for key in ("GlyphClassDef", "MarkAttachClassDef"):
            table = getattr(font["GDEF"].table, key, None)
            if table:
                table.classDefs = {variants[g, w]: cls for g, cls in
                                   table.classDefs.items() for w in widths[g]}


def adjust_outlines(font, origins, hangul, em):
    """輪郭をセルに収め、同じ変換を GPOS のアンカーにも適用します。"""
    glyph_set = font.getGlyphSet()
    top = font["CFF2"].cff.topDictIndex[0]
    cs = top.CharStrings
    transforms = {}
    programs = {}
    tone_glyphs = {font.getBestCmap().get(cp) for cp in (0x302E, 0x302F)}
    original_metrics = dict(font["hmtx"].metrics)
    font_bounds = []
    extents = []
    for index, (name, (original, width)) in enumerate(origins.items()):
        if index and index % 10000 == 0:
            print(f"     {index} / {len(origins)} glyphs", flush=True)
        advance = original_metrics[original][0]
        recording = RecordingPen()
        glyph_set[original].draw(recording)
        bounds = BoundsPen(glyph_set)
        recording.replay(bounds)
        # 合成 Jamo のゼロ幅字形にも、初声と同じ 920 → 1000 の変換を使います。
        scale = em / (920 * em / 1000) if original in hangul else 1.0
        offset = 0.0
        if width and advance:
            scale = width / advance
            if bounds.bounds:
                x0, _, x1, _ = bounds.bounds
                if x1 > x0:
                    scale = min(scale, width / (x1 - x0))
                offset = (width - advance * scale) / 2
                offset = max(-x0 * scale, min(offset, width - x1 * scale))
        elif name in tone_glyphs:
            # ゼロ幅声調記号は HarfBuzz が音節の後ろに残すため、音節の左へ戻します。
            offset = -em - advance
        transforms[name] = (scale, offset)
        pen = T2CharStringPen(None, glyph_set, CFF2=True)
        recording.replay(TransformPen(pen, (scale, 0, 0, 1, offset, 0)))
        source_cs = cs[original]
        program = pen.getCharString(private=source_cs.private,
                                   globalSubrs=source_cs.globalSubrs)
        # 65,000 字形分の Python の座標オブジェクトを保持せず、順次バイト列化します。
        program.compile(isCFF2=True)
        programs[name] = program
        lsb = otRound(bounds.bounds[0] * scale + offset) if bounds.bounds else 0
        font["hmtx"][name] = width, lsb
        if bounds.bounds:
            x0, y0, x1, y1 = bounds.bounds
            transformed = (x0 * scale + offset, y0, x1 * scale + offset, y1)
            font_bounds.append(transformed)
            right = otRound(transformed[2])
            extents.append((lsb, width - right, right))
        else:
            extents.append((0, width, 0))
    # サブルーチンを持たない新しい CFF2 を作り、旧 FDArray とヒントも除きます。
    FontBuilder(font=font).setupCFF2(programs)

    def anchor(value, name):
        if value is not None:
            scale, offset = transforms[name]
            value.XCoordinate = otRound(value.XCoordinate * scale + offset)

    # 実フォントにある mark-to-base は Coverage と配列を一緒に拡張します。
    by_original = defaultdict(list)
    for name, (original, _) in origins.items():
        by_original[original].append(name)
    for table in subtables(font, "GPOS"):
        kind = table.__class__.__name__
        if kind == "MarkBasePos":
            for coverage_key, array_key, records_key, count_key, anchor_key in (
                    ("MarkCoverage", "MarkArray", "MarkRecord", "MarkCount", "MarkAnchor"),
                    ("BaseCoverage", "BaseArray", "BaseRecord", "BaseCount", "BaseAnchor")):
                coverage = getattr(table, coverage_key)
                array = getattr(table, array_key)
                records = []
                glyphs = []
                for original, record in zip(coverage.glyphs, getattr(array, records_key)):
                    for name in by_original[original]:
                        new = copy.deepcopy(record)
                        anchors = getattr(new, anchor_key)
                        for item in anchors if isinstance(anchors, list) else [anchors]:
                            anchor(item, name)
                        glyphs.append(name)
                        records.append(new)
                rows = sorted(zip(glyphs, records), key=lambda row: font.getGlyphID(row[0]))
                coverage.glyphs = [name for name, _ in rows]
                setattr(array, records_key, [record for _, record in rows])
                setattr(array, count_key, len(records))
        elif kind == "SinglePos":
            # 垂直方向の調整は保持し、水平セル幅・配置は輪郭側で確定します。
            values = table.Value if table.Format == 2 else [table.Value]
            for value in values:
                for attr in ("XAdvance", "XPlacement"):
                    if hasattr(value, attr):
                        setattr(value, attr, 0)
            if table.Format == 1:
                table.Coverage.glyphs = sorted(
                    {name for g in table.Coverage.glyphs for name in by_original[g]},
                    key=font.getGlyphID)
            else:
                raise ValueError("Unsupported per-glyph single positioning")
        else:
            raise ValueError(f"Unsupported positioning: {kind}")
    font["hhea"].advanceWidthMax = em
    font["hhea"].minLeftSideBearing = min(row[0] for row in extents)
    font["hhea"].minRightSideBearing = min(row[1] for row in extents)
    font["hhea"].xMaxExtent = max(row[2] for row in extents)
    if font_bounds:
        font["head"].xMin = math.floor(min(row[0] for row in font_bounds))
        font["head"].yMin = math.floor(min(row[1] for row in font_bounds))
        font["head"].xMax = math.ceil(max(row[2] for row in font_bounds))
        font["head"].yMax = math.ceil(max(row[3] for row in font_bounds))
    # 計算済みの境界を使い、保存時に全 65,000 字形を再描画するのを避けます。
    font.recalcBBoxes = False
    font["OS/2"].xAvgCharWidth = em // 2
    # 二種類の非ゼロ幅を持つため、単一幅を表す isFixedPitch は立てません。
    font["post"].isFixedPitch = 0


def normalize(font):
    """静的 CFF2 の全字形を分類し、字形数上限内で幅を統一します。"""
    if "CFF2" not in font or "fvar" in font:
        raise ValueError("A static CFF2 font is required")
    em = font["head"].unitsPerEm
    if em % 2:
        raise ValueError("unitsPerEm must be even")
    original_count = len(font.getGlyphOrder())
    # 固定幅と両立しない機能を除き、残した全 GSUB 機能の closure を保持します。
    remove_width_features(font)
    print("  -> retaining Unicode, UVS and GSUB closure", flush=True)
    compact(font)
    retained_count = len(font.getGlyphOrder())
    dormant_pairs = list(substitution_pairs(subtables(font, "GSUB")))
    print("  -> classifying glyph roles", flush=True)
    widths, hangul = classify(font, em, dormant_pairs)
    variants, origins = split_glyphs(font, widths)
    print(f"  -> adjusting {len(origins)} outlines and layout references", flush=True)
    update_references(font, widths, variants, em)
    adjust_outlines(font, origins, hangul, em)
    ensure_hangul_features(font)
    ensure_alphabetic_marks(font)
    if "DSIG" in font:
        del font["DSIG"]
    counts = Counter(advance for advance, _ in font["hmtx"].metrics.values())
    if set(counts) - {0, em // 2, em} or sum(counts.values()) != len(font.getGlyphOrder()):
        raise ValueError("Inconsistent normalized metrics")
    return {"unicode_version": UNICODE_VERSION, "removed": original_count - retained_count,
            "added": len(origins) - retained_count, "glyphs": len(origins),
            "advances": dict(sorted(counts.items()))}


def ensure_hangul_features(font):
    """文書の言語が日本語でも、古い Jamo 列を一つの音節に組みます。"""
    if "GSUB" not in font:
        return
    table = font["GSUB"].table
    for record in table.ScriptList.ScriptRecord:
        if record.ScriptTag != "hang" or record.Script.DefaultLangSys is None:
            continue
        script = record.Script
        jamo = {index for index in script.DefaultLangSys.FeatureIndex
                if table.FeatureList.FeatureRecord[index].FeatureTag
                in {"ljmo", "vjmo", "tjmo"}}
        for language in script.LangSysRecord:
            indices = set(language.LangSys.FeatureIndex) | jamo
            language.LangSys.FeatureIndex = sorted(indices)
            language.LangSys.FeatureCount = len(indices)


def ensure_alphabetic_marks(font):
    """元フォントで欠けている欧文の結合位置を、輪郭の境界から補います。"""
    em = font["head"].unitsPerEm
    glyph_set = font.getGlyphSet()
    cmap = font.getBestCmap()
    letter_glyphs = {g for cp, g in cmap.items()
                     if unicode.category(chr(cp)).startswith("L")
                     and any(script in unicode.name(chr(cp), "")
                             for script in ("LATIN", "GREEK", "CYRILLIC"))}
    # 言語・歴史字形の置換で選ばれた文字にも、同じ結合位置を用意します。
    while True:
        previous = len(letter_glyphs)
        for source, target in substitution_pairs(subtables(font, "GSUB")):
            if source in letter_glyphs and font["hmtx"][target][0]:
                letter_glyphs.add(target)
        if previous == len(letter_glyphs):
            break

    marks = {}
    mark_bounds = {}
    for cp, glyph in cmap.items():
        if not (0x300 <= cp <= 0x36F or 0x1AB0 <= cp <= 0x1AFF
                or 0x1DC0 <= cp <= 0x1DFF):
            continue
        if unicode.category(chr(cp)) not in {"Mn", "Me"}:
            continue
        pen = BoundsPen(glyph_set)
        glyph_set[glyph].draw(pen)
        if not pen.bounds:
            continue
        x0, y0, x1, y1 = pen.bounds
        mark_class = 1 if unicode.combining(chr(cp)) in {202, 220, 240} else 0
        marks[glyph] = (mark_class, layout_builder.buildAnchor(
            otRound((x0 + x1) / 2), otRound(y1 if mark_class else y0)))
        mark_bounds[glyph] = pen.bounds
    if not marks or not letter_glyphs:
        return
    class_count = max(cls for cls, _ in marks.values()) + 1
    bases = {}
    gap = otRound(em * 0.04)
    for glyph in letter_glyphs:
        pen = BoundsPen(glyph_set)
        glyph_set[glyph].draw(pen)
        if not pen.bounds:
            continue
        x0, y0, x1, y1 = pen.bounds
        bases[glyph] = {cls: layout_builder.buildAnchor(
            otRound((x0 + x1) / 2), otRound(y0 - gap if cls else y1 + gap))
            for cls in range(class_count)}

    if "GPOS" not in font:
        font["GPOS"] = newTable("GPOS")
        table = font["GPOS"].table = otTables.GPOS()
        table.Version = 0x00010000
        table.ScriptList = otTables.ScriptList()
        table.ScriptList.ScriptRecord = []
        table.FeatureList = otTables.FeatureList()
        table.FeatureList.FeatureRecord = []
        table.LookupList = otTables.LookupList()
        table.LookupList.Lookup = []
    table = font["GPOS"].table
    base_lookup = layout_builder.buildLookup([
        layout_builder.buildMarkBasePosSubtable(marks, bases, font.getReverseGlyphMap())])
    # 複数のアクセントは、同じ種類の記号の上 (または下) へ重ねます。
    stack = otTables.MarkMarkPos()
    stack.Format = 1
    stack.ClassCount = class_count
    stack.Mark1Coverage = layout_builder.buildCoverage(marks, font.getReverseGlyphMap())
    stack.Mark2Coverage = layout_builder.buildCoverage(marks, font.getReverseGlyphMap())
    stack.Mark1Array = layout_builder.buildMarkArray(marks, font.getReverseGlyphMap())
    stack.Mark2Array = otTables.Mark2Array()
    stack.Mark2Array.Mark2Count = len(marks)
    stack.Mark2Array.Mark2Record = []
    for glyph in stack.Mark2Coverage.glyphs:
        x0, y0, x1, y1 = mark_bounds[glyph]
        cls = marks[glyph][0]
        anchors = [None] * class_count
        anchors[cls] = layout_builder.buildAnchor(
            otRound((x0 + x1) / 2), otRound(y0 - gap if cls else y1 + gap))
        stack.Mark2Array.Mark2Record.append(layout_builder.buildMark2Record(anchors))
    new_indices = [len(table.LookupList.Lookup), len(table.LookupList.Lookup) + 1]
    table.LookupList.Lookup.extend([base_lookup, layout_builder.buildLookup([stack])])
    table.LookupList.LookupCount = len(table.LookupList.Lookup)
    mark_features = [i for i, r in enumerate(table.FeatureList.FeatureRecord)
                     if r.FeatureTag == "mark"]
    if not mark_features:
        record = otTables.FeatureRecord()
        record.FeatureTag = "mark"
        record.Feature = otTables.Feature()
        record.Feature.FeatureParams = None
        record.Feature.LookupListIndex = []
        mark_features = [len(table.FeatureList.FeatureRecord)]
        table.FeatureList.FeatureRecord.append(record)
    for index in mark_features:
        feature = table.FeatureList.FeatureRecord[index].Feature
        feature.LookupListIndex.extend(new_indices)
        feature.LookupCount = len(feature.LookupListIndex)
    table.FeatureList.FeatureCount = len(table.FeatureList.FeatureRecord)
    scripts = {r.ScriptTag: r for r in table.ScriptList.ScriptRecord}
    for tag in ("DFLT", "latn", "grek", "cyrl"):
        if tag not in scripts:
            record = otTables.ScriptRecord()
            record.ScriptTag = tag
            record.Script = otTables.Script()
            record.Script.DefaultLangSys = None
            record.Script.LangSysRecord = []
            record.Script.LangSysCount = 0
            scripts[tag] = record
        script = scripts[tag].Script
        if script.DefaultLangSys is None:
            language = script.DefaultLangSys = otTables.LangSys()
            language.LookupOrder = None
            language.ReqFeatureIndex = 0xFFFF
            language.FeatureIndex = []
        for language in [script.DefaultLangSys] + [r.LangSys for r in script.LangSysRecord]:
            if not any(index in language.FeatureIndex for index in mark_features):
                language.FeatureIndex.append(mark_features[0])
            language.FeatureCount = len(language.FeatureIndex)
    table.ScriptList.ScriptRecord = [scripts[tag] for tag in sorted(scripts)]
    table.ScriptList.ScriptCount = len(scripts)
    if "GDEF" not in font:
        font["GDEF"] = newTable("GDEF")
        font["GDEF"].table = otTables.GDEF()
        font["GDEF"].table.Version = 0x00010000
        for attr in ("AttachList", "LigCaretList", "MarkAttachClassDef"):
            setattr(font["GDEF"].table, attr, None)
        font["GDEF"].table.GlyphClassDef = None
    definition = font["GDEF"].table.GlyphClassDef
    if definition is None:
        definition = font["GDEF"].table.GlyphClassDef = otTables.ClassDef()
        definition.classDefs = {}
    definition.classDefs.update({glyph: 1 for glyph in bases})
    definition.classDefs.update({glyph: 3 for glyph in marks})


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("input")
    parser.add_argument("output")
    args = parser.parse_args()
    font = TTFont(args.input, recalcTimestamp=False)
    report = normalize(font)
    print(report, flush=True)
    # 輪郭を書き直したあとにサブルーチン化し、サイズを元と同程度に保ちます。
    import cffsubr
    print("  -> subroutinizing normalized outlines", flush=True)
    cffsubr.subroutinize(font)
    font.save(args.output)


if __name__ == "__main__":
    main()
