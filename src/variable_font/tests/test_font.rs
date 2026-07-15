//! `build_variable_font` の単体テストのために、最小限の静的 CFF フォントを
//! バイト列として組み立てるヘルパーである。
//!
//! `write-fonts` は CFF (v1) テーブルの書き込みをサポートしていないため
//! (サポートしているのは CFF2 のみ)、テスト用の入力データはここで自前に
//! バイト列として組み立てる。

use write_fonts::tables::{cmap, head, hhea, hmtx, maxp, name, os2, post};
use write_fonts::types;

use super::super::super::round;

/// テスト対象の元フォントに含めるグリフ数 (`.notdef` と正方形の 2 個)。
const NUM_GLYPHS: u16 = 2;

/// テストで使う丸めパラメータ (凸角用の基準半径・凹角用の固定半径)。
const BASE_RADIUS: f64 = 60.0;
const INNER_RADIUS: f64 = 20.0;

/// GID=1 (正方形) の、丸める前の輪郭である。4 本の直線からなる。
fn square_outline() -> kurbo::BezPath {
    let mut path = kurbo::BezPath::new();
    path.move_to((100.0, 100.0));
    path.line_to((600.0, 100.0));
    path.line_to((600.0, 600.0));
    path.line_to((100.0, 600.0));
    path.close_path();
    path
}

/// `build_variable_font` の比較用に、GID 順の元の輪郭一覧を返す
/// (`.notdef` は空の輪郭)。
pub(super) fn original_glyph_outlines() -> Vec<kurbo::BezPath> {
    vec![kurbo::BezPath::new(), square_outline()]
}

/// `build_variable_font` の第 2 引数として渡す、GID 順の
/// `(丸める前の輪郭, 丸めた後の輪郭)` の一覧である。`round::round_path_matched`
/// を実際に呼び出して組み立てるため、本体側の丸め処理をそのまま経由した
/// テストになる。
fn matched_glyph_outlines() -> Vec<(kurbo::BezPath, kurbo::BezPath)> {
    vec![
        (kurbo::BezPath::new(), kurbo::BezPath::new()),
        round::round_path_matched(&square_outline(), BASE_RADIUS, INNER_RADIUS)
            .expect("テスト用の丸めパラメータは有効である"),
    ]
}

/// CFF (v1) の INDEX 構造 (CFF2 と異なり count は 2 バイト) をバイト列として
/// 組み立てる。
fn cff1_index(items: &[Vec<u8>]) -> Vec<u8> {
    if items.is_empty() {
        // count = 0 のときは、offSize 以降のフィールドを省略できる。
        return vec![0, 0];
    }

    let count = items.len() as u16;
    // オフセットは 1 始まりで、各 item の末尾位置 (次の item の開始位置) を
    // 順に並べたものである。
    let mut offset_values = Vec::with_capacity(items.len() + 1);
    let mut current = 1u32;
    offset_values.push(current);
    for item in items {
        current += item.len() as u32;
        offset_values.push(current);
    }

    // 最大オフセットを表現できる最小のバイト数を選ぶ。
    let max_offset = *offset_values.last().unwrap();
    let off_size = if max_offset <= 0xFF {
        1u8
    } else if max_offset <= 0xFFFF {
        2u8
    } else if max_offset <= 0xFF_FFFF {
        3u8
    } else {
        4u8
    };

    let mut buf = Vec::new();
    buf.extend_from_slice(&count.to_be_bytes());
    buf.push(off_size);
    for offset in &offset_values {
        let bytes = offset.to_be_bytes();
        buf.extend_from_slice(&bytes[4 - off_size as usize..]);
    }
    for item in items {
        buf.extend_from_slice(item);
    }
    buf
}

/// CFF (v1) の DICT 整数オペランドを、値によらず常に 5 バイト (オペコード
/// 29 + 32 ビット整数) で書き出す。
///
/// トップ DICT に埋め込む CharstringsOffset は、他の構造の配置を決めた
/// あとでなければ確定しないため、バイト長が値に依存しないこの形式を使う。
fn push_dict_offset(buf: &mut Vec<u8>, value: i32) {
    buf.push(29);
    buf.extend_from_slice(&value.to_be_bytes());
}

/// テスト用の最小限の CFF (v1) テーブルを組み立てる。
///
/// トップ DICT には CharstringsOffset のみを持たせ、charset・encoding・
/// Private DICT はすべて既定値 (未指定) に委ねる。
fn build_cff1_table() -> Vec<u8> {
    const HEADER_SIZE: usize = 4;
    // CharstringsOffset のみを持つトップ DICT は、常に 6 バイト
    // (オフセット 5 バイト + オペレータ 1 バイト) になる。
    const TOP_DICT_LEN: usize = 6;

    // .notdef は輪郭を持たないため、endchar だけの空の CharString
    // である。CFF (v1) の Type2 CharString は CFF2 と異なり、
    // endchar オペレータ (14) で終端する。
    let notdef_charstring = vec![14u8];
    let square_charstring = {
        let mut buf = Vec::new();
        // 100 100 rmoveto : 始点 (100, 100) へ移動する。
        super::super::push_charstring_int(&mut buf, 100);
        super::super::push_charstring_int(&mut buf, 100);
        buf.push(21);
        // 4 辺分の rlineto (5) を、正方形が閉じるように積み上げる。
        for (dx, dy) in [(500, 0), (0, 500), (-500, 0), (0, -500)] {
            super::super::push_charstring_int(&mut buf, dx);
            super::super::push_charstring_int(&mut buf, dy);
            buf.push(5);
        }
        buf.push(14); // endchar
        buf
    };

    let name_index = cff1_index(&[b"RoundedNotoSansTest".to_vec()]);
    let string_index = cff1_index(&[]);
    let global_subr_index = cff1_index(&[]);
    let charstrings_index = cff1_index(&[notdef_charstring, square_charstring]);

    // トップ DICT INDEX 自体のバイト長は、中身の値によらず一定である。
    // 先にこの長さだけを確定させ、CharstringsOffset の実際の値を計算する。
    let top_dict_index_len = cff1_index(&[vec![0u8; TOP_DICT_LEN]]).len();
    let charstrings_offset = HEADER_SIZE
        + name_index.len()
        + top_dict_index_len
        + string_index.len()
        + global_subr_index.len();

    let mut top_dict_bytes = Vec::new();
    push_dict_offset(&mut top_dict_bytes, charstrings_offset as i32);
    top_dict_bytes.push(17); // CharstringsOffset
    debug_assert_eq!(TOP_DICT_LEN, top_dict_bytes.len());

    let top_dict_index = cff1_index(&[top_dict_bytes]);
    debug_assert_eq!(top_dict_index_len, top_dict_index.len());

    let mut cff = Vec::new();
    // ヘッダー: majorVersion=1, minorVersion=0, headerSize=4, offSize=4
    cff.extend_from_slice(&[1, 0, 4, 4]);
    cff.extend_from_slice(&name_index);
    cff.extend_from_slice(&top_dict_index);
    cff.extend_from_slice(&string_index);
    cff.extend_from_slice(&global_subr_index);
    cff.extend_from_slice(&charstrings_index);
    cff
}

/// `build_variable_font` のテストに使う、最小限の静的 CFF フォントと、
/// それに対応する `(丸める前の輪郭, 丸めた後の輪郭)` の一覧を組み立てる。
///
/// # Returns
/// `(元フォントのバイト列, matched_glyphs)` のタプルを返す。
pub(super) fn build_test_font() -> (Vec<u8>, Vec<(kurbo::BezPath, kurbo::BezPath)>) {
    let mut builder = write_fonts::FontBuilder::new();
    builder.add_raw(types::Tag::new(b"CFF "), build_cff1_table());

    let head = head::Head {
        units_per_em: 1000,
        x_min: 0,
        y_min: 0,
        x_max: 600,
        y_max: 600,
        ..Default::default()
    };
    builder
        .add_table(&head)
        .expect("head テーブルの組み立てに失敗した");

    let hhea = hhea::Hhea {
        number_of_h_metrics: NUM_GLYPHS,
        ..Default::default()
    };
    builder
        .add_table(&hhea)
        .expect("hhea テーブルの組み立てに失敗した");

    builder
        .add_table(&maxp::Maxp::new(NUM_GLYPHS))
        .expect("maxp テーブルの組み立てに失敗した");

    let hmtx = hmtx::Hmtx {
        h_metrics: vec![
            hmtx::LongMetric {
                advance: 0,
                side_bearing: 0,
            },
            hmtx::LongMetric {
                advance: 700,
                side_bearing: 100,
            },
        ],
        left_side_bearings: Vec::new(),
    };
    builder
        .add_table(&hmtx)
        .expect("hmtx テーブルの組み立てに失敗した");

    // `cffsubr` (外部の `tx` コマンド) は、name・cmap を欠いたフォントを
    // 不完全とみなして処理を拒否するため、テスト用のフォントであっても
    // 最小限のこれらのテーブルを用意しておく必要がある。
    let name_records = [
        (types::NameId::new(1), "Rounded Noto Sans CJK Test"),
        (types::NameId::new(2), "Regular"),
        (types::NameId::new(3), "Rounded Noto Sans CJK Test: Regular"),
        (types::NameId::new(4), "Rounded Noto Sans CJK Test Regular"),
        (types::NameId::new(6), "RoundedNotoSansCJKTest-Regular"),
    ]
    .into_iter()
    .map(|(name_id, value)| {
        name::NameRecord::new(
            3,
            1,
            0x0409,
            name_id,
            write_fonts::OffsetMarker::new(value.to_string()),
        )
    })
    .collect();
    builder
        .add_table(&name::Name::new(name_records))
        .expect("name テーブルの組み立てに失敗した");

    let cmap = cmap::Cmap::from_mappings([('A', types::GlyphId::new(1))])
        .expect("cmap テーブルの組み立てに失敗した");
    builder
        .add_table(&cmap)
        .expect("cmap テーブルの組み立てに失敗した");

    builder
        .add_table(&os2::Os2::default())
        .expect("OS/2 テーブルの組み立てに失敗した");
    builder
        .add_table(&post::Post::new_v2([".notdef", "square"]))
        .expect("post テーブルの組み立てに失敗した");

    (builder.build(), matched_glyph_outlines())
}
