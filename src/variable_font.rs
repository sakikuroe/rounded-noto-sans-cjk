//! 元の静的フォントと丸めたグリフから、丸みを可変軸として持つ CFF2 可変
//! フォントを組み立てる機能を提供するモジュールである。
//!
//! `write-fonts` クレートは CFF2 の item variation store やトップ DICT を
//! 高レベルに組み立てる API を持たないため、本モジュールでは CFF2 の
//! CharString (`blend` オペレータを含む) やトップ DICT / Font DICT /
//! Private DICT を直接バイト列として組み立てる。オフセット値は常に固定幅
//! (5 バイト整数) でエンコードすることで、DICT のバイト長がオフセットの
//! 実際の値に依存しないようにし、レイアウト計算を単純な 1 パスの算術で
//! 完結させている。

use std::io::Write;

use read_fonts::TableProvider;
use write_fonts::FontBuilder;
use write_fonts::ps::cff::v2;
use write_fonts::tables::{avar, fvar, variations};
use write_fonts::types::{F2Dot14, Fixed, NameId, Tag};

/// 新設する可変軸のタグである。既存のどの登録済み軸タグとも重複しない
/// 4 文字のタグを独自に定義する。
const ROUNDNESS_AXIS_TAG: Tag = Tag::new(b"ROND");

/// `ROND` 軸の表示名として `name` テーブルに登録する nameID である。
/// 標準の nameID (0〜25) と衝突しない、フォント固有の名前用に予約された
/// 範囲 (256〜32767) から選んでいる。
const ROUNDNESS_AXIS_NAME_ID: NameId = NameId::new(256);

/// `ROUNDNESS_AXIS_NAME_ID` に対応する、ROND 軸の表示名の文字列である。
const ROUNDNESS_AXIS_NAME: &str = "Roundness";

/// 可変フォントの最終出力から除外するタグの一覧である。
///
/// `CFF ` および (万一存在した場合の) `glyf`・`loca` は輪郭データそのもの
/// であり、`CFF2` に置き換えるため元のバイト列を引き継がない。`fvar`・
/// `avar` は本モジュールが新たに組み立てるため、元のフォントに同名の
/// テーブルが存在してもそちらを使う。
const EXCLUDED_TABLE_TAGS: [Tag; 6] = [
    Tag::new(b"CFF "),
    Tag::new(b"CFF2"),
    Tag::new(b"glyf"),
    Tag::new(b"loca"),
    Tag::new(b"fvar"),
    Tag::new(b"avar"),
];

/// `build_variable_font` の最終出力から除外するタグの一覧である。
///
/// `EXCLUDED_TABLE_TAGS` に加えて `name` も除外する。`build_variable_font`
/// は ROND 軸の表示名レコードを追加した `name` テーブルを
/// `build_name_table_with_axis_name` で組み立て直すため、元のフォントの
/// `name` テーブルをそのまま引き継いではならない。
const VARIABLE_FONT_EXCLUDED_TABLE_TAGS: [Tag; 7] = [
    Tag::new(b"CFF "),
    Tag::new(b"CFF2"),
    Tag::new(b"glyf"),
    Tag::new(b"loca"),
    Tag::new(b"fvar"),
    Tag::new(b"avar"),
    Tag::new(b"name"),
];

/// 元の静的フォントのバイト列と、丸め前後で対応が取れたグリフの輪郭から、
/// 丸みを表す可変軸を持つ CFF2 可変フォントのバイト列を組み立てる。
///
/// name・hmtx・cmap など、輪郭以外のメタデータはすべて `original_font_data`
/// から引き継ぐ。新たに、既存のどの登録済み軸とも重複しない独自軸タグ
/// `ROND` を持つ `fvar`/`avar` テーブルを追加する。この軸は最小値 (既定値)
/// が丸める前の字形に、最大値が丸めた字形に対応し、その間はグリフごとの
/// CFF2 item variation store 上の線形補間によって連続的に丸まっていく。
/// 既定値は軸の最小値と等しいため、可変軸の値を指定せずにこのフォントを
/// 描画すると、元の (丸めていない) 字形が得られる。
///
/// CFF2 の `blend` オペレータは、2 つのマスター間でオペレータの並びが完全に
/// 一致していることを要求する。`matched_glyphs` の各要素は
/// `round::round_path_matched` が返す組であり、丸める前・丸めた後の輪郭が
/// 要素数・各位置の要素の種類について常に一致することが保証されているため、
/// 本関数はその一致を前提に、サブパスとセグメントを先頭から順に対応付けて
/// 読み進めるだけでよい。
///
/// # Args
/// - `original_font_data` - 変換元の静的フォントのバイト列であり、CFF の
///   アウトラインを含む静的な OpenType フォントである必要がある。
/// - `matched_glyphs` - グリフ ID の順序で並んだ、`(丸める前の輪郭, 丸めた
///   後の輪郭)` の組の一覧である。各組は、`outline::extract_glyphs` で
///   取り出した輪郭を `round::round_path_matched` に渡して得られたもので
///   なければならない。要素数は `original_font_data` のグリフ数と一致して
///   いなければならない。
///
/// # Returns
/// `ROND` 軸を 1 つだけ持つ OpenType/CFF2 可変フォントのバイト列を返す。
/// 先頭 4 バイトは、CFF2 ベースの OpenType フォントであることを示す sfnt
/// バージョンタグ `OTTO` である。
///
/// # Panics
/// - `matched_glyphs` の要素数が `original_font_data` に含まれるグリフ数と
///   一致しない場合にパニックする。
/// - いずれかのグリフについて、組の 2 つの輪郭のサブパスの本数、または
///   対応するサブパスのセグメント数が一致しない場合にパニックする
///   (`round::round_path_matched` の結果をそのまま渡している限り起こらない)。
/// - `original_font_data` が有効な OpenType フォントとして解析できない
///   場合にパニックする。
///
/// # Examples
/// ```no_run
/// use rounded_noto_sans_cjk::{outline, round, variable_font};
///
/// let font_data = std::fs::read("NotoSansCJKjp-Regular.otf").unwrap();
/// let glyphs = outline::extract_glyphs(&font_data);
/// let matched = glyphs
///     .iter()
///     .map(|g| round::round_path_matched(g, 20.0, 5.0).unwrap())
///     .collect::<Vec<_>>();
/// let variable_font_data = variable_font::build_variable_font(&font_data, &matched);
///
/// // CFF2 可変フォントも sfnt バージョンタグは OTTO である。
/// assert_eq!(b"OTTO", &variable_font_data[0..4]);
/// ```
pub fn build_variable_font(
    original_font_data: &[u8],
    matched_glyphs: &[(kurbo::BezPath, kurbo::BezPath)],
) -> Vec<u8> {
    let font = read_fonts::FontRef::new(original_font_data)
        .expect("original_font_data は有効な OpenType フォントとして解析できなかった");

    // グリフ数の整合性を確認する。以降の処理はグリフ ID がそのまま
    // 配列添字になることを前提にしている。
    let num_glyphs = font
        .maxp()
        .expect("maxp テーブルの解析に失敗した")
        .num_glyphs() as usize;
    assert_eq!(
        num_glyphs,
        matched_glyphs.len(),
        "matched_glyphs の要素数は original_font_data のグリフ数と一致する必要がある"
    );

    // フォント行列 (FontMatrix) は unitsPerEm の逆数であり、これによって
    // CFF2 の CharString 座標系を head テーブルの unitsPerEm と
    // 一致させる。0 は不正な値なので、その場合のみ CFF の慣例である
    // 1000 にフォールバックする。
    let units_per_em = font
        .head()
        .expect("head テーブルの解析に失敗した")
        .units_per_em();
    let units_per_em = if units_per_em == 0 {
        1000
    } else {
        units_per_em
    };
    let matrix_scale = 1.0 / f64::from(units_per_em);

    // 各グリフについて、blend オペレータで 2 つのマスター (丸める前・
    // 丸めた後) を線形補間する CharString を組み立てる。
    let charstrings = matched_glyphs
        .iter()
        .map(|(original, rounded)| build_glyph_charstring(original, rounded))
        .collect::<Vec<_>>();

    let cff2_bytes = build_cff2_table(&charstrings, matrix_scale);

    let mut builder = FontBuilder::new();
    builder.add_raw(Tag::new(b"CFF2"), cff2_bytes);
    builder
        .add_table(&build_fvar())
        .expect("fvar テーブルの組み立てに失敗した");
    builder
        .add_table(&build_avar())
        .expect("avar テーブルの組み立てに失敗した");
    // fvar が参照する ROND 軸の表示名レコードを持つ name テーブルを、元の
    // フォントの全レコードを引き継いだうえで組み立てる。
    builder
        .add_table(&build_name_table_with_axis_name(&font))
        .expect("name テーブルの組み立てに失敗した");

    // hmtx・cmap など、輪郭・名称以外のメタデータはすべて元のフォントから
    // そのまま引き継ぐ。輪郭関連のテーブル、新たに組み立て直す fvar・avar・
    // name は除外する (copy_missing_tables は追加済みのタグを上書きしない
    // ため、除外しなければ元の 'CFF ' テーブル等が余分に残ってしまう)。
    for record in font.table_directory().table_records() {
        let tag = record.tag();
        if VARIABLE_FONT_EXCLUDED_TABLE_TAGS.contains(&tag) {
            continue;
        }
        if let Some(data) = font.table_data(tag) {
            builder.add_raw(tag, data.as_bytes().to_vec());
        }
    }

    builder.build()
}

/// 元の静的フォントのバイト列と、丸めた後の輪郭だけから、可変軸を一切
/// 持たない静的な CFF2 フォントのバイト列を組み立てる。
///
/// `build_variable_font` が `round::round_path_matched` の 2 マスターを
/// `blend` オペレータで補間するのに対し、本関数は `round::round_path` が
/// 返す「丸めた後の輪郭」だけを受け取り、各座標を CharString へ
/// 直接書き込む。そのため `blend`・`fvar`・`avar`・item variation store は
/// 一切生成せず、CFF2 のトップ DICT にも VariationStore への参照を持たない。
///
/// この違いは最終的な曲線セグメント数に影響する。`round_path_matched` は
/// 2 マスターの要素数を一致させるために各丸め弧を 2 本の 3 次ベジエへ
/// 分割するが、`round_path` は 1 本の 3 次ベジエのまま保持する。可変
/// フォントを `fonttools varLib.instancer` で 1 点に固定しても、この
/// 2 分割された弧は 1 本に戻らないため、最初から `round_path` の結果を
/// 焼き付ける本関数の方が、丸め弧に由来するセグメント数を約半分に抑え
/// られる。
///
/// name・hmtx・cmap など、輪郭以外のメタデータはすべて `original_font_data`
/// からそのまま引き継ぐ。
///
/// # Args
/// - `original_font_data` - 変換元の静的フォントのバイト列であり、CFF の
///   アウトラインを含む静的な OpenType フォントである必要がある。
/// - `rounded_glyphs` - グリフ ID の順序で並んだ、丸めた後の輪郭の一覧で
///   ある。各要素は、`outline::extract_glyphs` で取り出した輪郭を
///   `round::round_path` に渡して得られたものでなければならない。要素数は
///   `original_font_data` のグリフ数と一致していなければならない。
///
/// # Returns
/// 可変軸を持たない OpenType/CFF2 フォントのバイト列を返す。先頭 4 バイトは
/// CFF2 ベースの OpenType フォントであることを示す sfnt バージョンタグ
/// `OTTO` である。
///
/// # Panics
/// - `rounded_glyphs` の要素数が `original_font_data` に含まれるグリフ数と
///   一致しない場合にパニックする。
/// - `original_font_data` が有効な OpenType フォントとして解析できない
///   場合にパニックする。
///
/// # Examples
/// ```no_run
/// use rounded_noto_sans_cjk::{outline, round, variable_font};
///
/// let font_data = std::fs::read("NotoSansCJKjp-Regular.otf").unwrap();
/// let glyphs = outline::extract_glyphs(&font_data);
/// let rounded = glyphs
///     .iter()
///     .map(|g| round::round_path(g, 40.0, 5.0).unwrap())
///     .collect::<Vec<_>>();
/// let static_font_data = variable_font::build_static_font(&font_data, &rounded);
///
/// assert_eq!(b"OTTO", &static_font_data[0..4]);
/// ```
pub fn build_static_font(original_font_data: &[u8], rounded_glyphs: &[kurbo::BezPath]) -> Vec<u8> {
    let font = read_fonts::FontRef::new(original_font_data)
        .expect("original_font_data は有効な OpenType フォントとして解析できなかった");

    // グリフ数の整合性を確認する。以降の処理はグリフ ID がそのまま配列
    // 添字になることを前提にしている。
    let num_glyphs = font
        .maxp()
        .expect("maxp テーブルの解析に失敗した")
        .num_glyphs() as usize;
    assert_eq!(
        num_glyphs,
        rounded_glyphs.len(),
        "rounded_glyphs の要素数は original_font_data のグリフ数と一致する必要がある"
    );

    // FontMatrix は unitsPerEm の逆数であり、CFF2 の CharString 座標系を
    // head の unitsPerEm と一致させる。0 は不正なので、その場合のみ CFF の
    // 慣例である 1000 にフォールバックする。
    let units_per_em = font
        .head()
        .expect("head テーブルの解析に失敗した")
        .units_per_em();
    let units_per_em = if units_per_em == 0 {
        1000
    } else {
        units_per_em
    };
    let matrix_scale = 1.0 / f64::from(units_per_em);

    // 各グリフについて、blend を使わず座標を直接書き込む CharString を
    // 組み立てる。
    let charstrings = rounded_glyphs
        .iter()
        .map(build_static_glyph_charstring)
        .collect::<Vec<_>>();

    let cff2_bytes = build_static_cff2_table(&charstrings, matrix_scale);

    let mut builder = FontBuilder::new();
    builder.add_raw(Tag::new(b"CFF2"), cff2_bytes);

    // 輪郭関連のテーブル・可変軸関連のテーブルを除いたメタデータは、すべて
    // 元のフォントからそのまま引き継ぐ。fvar・avar を組み立てないため、
    // 除外一覧に含まれるこれらのタグは結果に現れない (元の静的フォントは
    // そもそも持たない)。
    for record in font.table_directory().table_records() {
        let tag = record.tag();
        if EXCLUDED_TABLE_TAGS.contains(&tag) {
            continue;
        }
        if let Some(data) = font.table_data(tag) {
            builder.add_raw(tag, data.as_bytes().to_vec());
        }
    }

    builder.build()
}

/// `build_variable_font` が組み立てた CFF2 可変フォントのバイト列を、外部
/// コマンド `cffsubr` でサブルーチン化し、圧縮したバイト列を返す。
///
/// `build_variable_font`は、各グリフの blend オペレータを展開したまま
/// CharString を組み立てるため、CJK のように似た部首・画を多数の
/// グリフで共有するフォントでは、出力がもとの静的フォントよりも大幅に
/// 大きくなる。`cffsubr` (Adobe の AFDKO に含まれる `tx` を利用する外部
/// コマンド) は、CharString 間の繰り返しパターンを検出して
/// サブルーチンへ括り出すことで、この肥大化を圧縮する。この処理は
/// CharString の表現を最適化するだけであり、字形やすべての可変軸の
/// 値における描画結果は変化しない。
///
/// # Args
/// - `font_data` - `build_variable_font` が返した、CFF2 テーブルを含む
///   OpenType フォントのバイト列である。
///
/// # Returns
/// サブルーチン化されたフォントのバイト列を返す。
///
/// # Panics
/// - `cffsubr` コマンドが `PATH` 上に見つからない場合にパニックする
///   (`pip install cffsubr` などで別途インストールしておく必要がある)。
/// - `cffsubr` がエラー終了した場合にパニックする。
pub fn subroutinize(font_data: &[u8]) -> Vec<u8> {
    let mut input_file = tempfile::NamedTempFile::new().expect("一時ファイルの作成に失敗した");
    input_file
        .write_all(font_data)
        .expect("一時ファイルへの書き込みに失敗した");

    // -o を指定しない場合、cffsubr は結果をそのまま標準出力へ書き出す。
    // 出力用の一時ファイルを別途用意する必要がないため、これを利用する。
    let output = std::process::Command::new("cffsubr")
        .arg(input_file.path())
        .output()
        .expect(
            "cffsubr の実行に失敗した (`pip install cffsubr` 等でインストールされている必要がある)",
        );

    assert!(
        output.status.success(),
        "cffsubr がエラー終了した: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    output.stdout
}

/// `BezPath` を、サブパスごとに区切った 3 次ベジエ曲線の列に変換する。
///
/// 直線・2 次ベジエは、形状を変えないまま数学的に等価な 3 次ベジエへ次数を
/// 上げる (degree elevation) ことで統一的に扱う。サブパスの終端が始点と
/// 一致していない場合は、`ClosePath` を暗黙の直線として補う。
///
/// すべての点 (オンカーブ・オフカーブを問わない) は、整数のフォントデザイン
/// 単位へ丸めたうえで扱う。丸め処理の幾何計算 (三角関数や 1/3 分割による
/// 次数上げなど) は、本来整数であるべき座標にごくわずかな小数部を
/// 生じさせるが、この誤差はフォントの見た目には影響しない一方、CFF2 の
/// CharString 上では固定小数点 (5 バイト) と整数 (多くの場合 1〜3
/// バイト) の差としてバイト数に大きく響く。オンカーブ点を丸めておくことで、
/// 経路の長さによらず誤差が蓄積しない。
///
/// # Args
/// - `path` - 変換元の `BezPath` である。
///
/// # Returns
/// 各要素がサブパス 1 本分の 3 次ベジエ曲線列である `Vec<Vec<kurbo::CubicBez>>`
/// を返す。
fn to_cubic_subpaths(path: &kurbo::BezPath) -> Vec<Vec<kurbo::CubicBez>> {
    let mut subpaths = Vec::new();
    let mut current = Vec::new();
    // 現在のペン位置と、現在のサブパスの始点。ClosePath で始点に戻る際の
    // 判定に始点を用いる。いずれも整数に丸めた値を保持する。
    let mut cur = kurbo::Point::ZERO;
    let mut start = kurbo::Point::ZERO;
    for element in path.elements() {
        match *element {
            kurbo::PathEl::MoveTo(p) => {
                // 直前のサブパスが存在すれば確定させ、新しいサブパスを開始する。
                if !current.is_empty() {
                    subpaths.push(std::mem::take(&mut current));
                }
                cur = round_point_to_integer(p);
                start = cur;
            }
            kurbo::PathEl::LineTo(p) => {
                let p = round_point_to_integer(p);
                current.push(elevate_line(cur, p));
                cur = p;
            }
            kurbo::PathEl::QuadTo(c, p) => {
                let c = round_point_to_integer(c);
                let p = round_point_to_integer(p);
                current.push(elevate_quad(cur, c, p));
                cur = p;
            }
            kurbo::PathEl::CurveTo(c1, c2, p) => {
                let c1 = round_point_to_integer(c1);
                let c2 = round_point_to_integer(c2);
                let p = round_point_to_integer(p);
                current.push(kurbo::CubicBez::new(cur, c1, c2, p));
                cur = p;
            }
            kurbo::PathEl::ClosePath => {
                // 始点まで戻っていなければ、暗黙の直線で閉じる。
                if cur != start {
                    current.push(elevate_line(cur, start));
                    cur = start;
                }
            }
        }
    }
    if !current.is_empty() {
        subpaths.push(current);
    }
    subpaths
}

/// 点の座標を、最も近い整数のフォントデザイン単位へ丸める。
///
/// # Args
/// - `p` - 丸める対象の点である。
///
/// # Returns
/// `x`・`y` をそれぞれ最も近い整数に丸めた点を返す。
fn round_point_to_integer(p: kurbo::Point) -> kurbo::Point {
    kurbo::Point::new(p.x.round(), p.y.round())
}

/// 直線分 `p0` -> `p1` を、形状を変えないまま 3 次ベジエとして表現する。
///
/// 制御点を始点・終点を 3 等分する位置に置くことで、直線と完全に等価な
/// 3 次ベジエになる。
fn elevate_line(p0: kurbo::Point, p1: kurbo::Point) -> kurbo::CubicBez {
    kurbo::CubicBez::new(p0, p0.lerp(p1, 1.0 / 3.0), p0.lerp(p1, 2.0 / 3.0), p1)
}

/// 2 次ベジエ (始点 `p0`、制御点 `c`、終点 `p1`) を、形状を変えないまま
/// 3 次ベジエとして表現する (次数上げ)。
fn elevate_quad(p0: kurbo::Point, c: kurbo::Point, p1: kurbo::Point) -> kurbo::CubicBez {
    let c1 = p0 + (c - p0) * (2.0 / 3.0);
    let c2 = p1 + (c - p1) * (2.0 / 3.0);
    kurbo::CubicBez::new(p0, c1, c2, p1)
}

/// 元の輪郭と丸めた輪郭の 1 グリフ分から、両者を `blend` オペレータで線形
/// 補間する CFF2 CharString のバイト列を組み立てる。
///
/// `original`・`rounded` は、`round::round_path_matched` が返す組であり、
/// サブパスの本数と各サブパスのセグメント数が常に一致していることを前提に
/// する。そのため、両者を先頭から順に対応付けて読み進めるだけでよく、
/// セグメント数を独自に揃え直す処理は行わない。
///
/// # Args
/// - `original` - 丸める前の輪郭である。
/// - `rounded` - `original` を `round::round_path_matched` で丸めた輪郭で
///   ある。
///
/// # Returns
/// 組み立てた CharString のバイト列を返す。
///
/// # Panics
/// - `original` と `rounded` のサブパスの本数、または対応するサブパスの
///   セグメント数が一致しない場合にパニックする。
fn build_glyph_charstring(original: &kurbo::BezPath, rounded: &kurbo::BezPath) -> Vec<u8> {
    let original_subpaths = to_cubic_subpaths(original);
    let rounded_subpaths = to_cubic_subpaths(rounded);
    assert_eq!(
        original_subpaths.len(),
        rounded_subpaths.len(),
        "元の輪郭と丸めた輪郭のサブパス本数が一致しない"
    );

    let mut buf = Vec::new();
    // 2 つのマスターそれぞれについて、独立にペン位置を追跡する。
    let mut cur_original = kurbo::Point::ZERO;
    let mut cur_rounded = kurbo::Point::ZERO;

    for (original_cubics, rounded_cubics) in original_subpaths.iter().zip(&rounded_subpaths) {
        assert_eq!(
            original_cubics.len(),
            rounded_cubics.len(),
            "元の輪郭と丸めた輪郭でサブパスのセグメント数が一致しない"
        );

        // このサブパスの始点への rmoveto を、2 マスター分の差分として書く。
        let original_start = original_cubics[0].p0;
        let rounded_start = rounded_cubics[0].p0;
        push_number_or_blend_operands(
            &mut buf,
            &[
                original_start.x - cur_original.x,
                original_start.y - cur_original.y,
            ],
            &[
                (rounded_start.x - cur_rounded.x) - (original_start.x - cur_original.x),
                (rounded_start.y - cur_rounded.y) - (original_start.y - cur_original.y),
            ],
        );
        buf.push(21); // rmoveto
        cur_original = original_start;
        cur_rounded = rounded_start;

        for (original_seg, rounded_seg) in original_cubics.iter().zip(rounded_cubics) {
            // rrcurveto の 3 組の相対座標 (制御点 1、制御点 2、終点) を、
            // それぞれ直前の点からの差分として求める。
            let original_deltas = [
                original_seg.p1 - cur_original,
                original_seg.p2 - original_seg.p1,
                original_seg.p3 - original_seg.p2,
            ];
            let rounded_deltas = [
                rounded_seg.p1 - cur_rounded,
                rounded_seg.p2 - rounded_seg.p1,
                rounded_seg.p3 - rounded_seg.p2,
            ];

            let mut defaults = Vec::with_capacity(6);
            let mut deltas = Vec::with_capacity(6);
            for (original_delta, rounded_delta) in original_deltas.iter().zip(&rounded_deltas) {
                defaults.push(original_delta.x);
                defaults.push(original_delta.y);
                deltas.push(rounded_delta.x - original_delta.x);
                deltas.push(rounded_delta.y - original_delta.y);
            }
            push_number_or_blend_operands(&mut buf, &defaults, &deltas);
            buf.push(8); // rrcurveto

            cur_original = original_seg.p3;
            cur_rounded = rounded_seg.p3;
        }
    }

    buf
}

/// `to_static_subpaths` が扱う、静的フォント専用の 1 セグメント分の表現で
/// ある。
///
/// `to_cubic_subpaths` と異なり、直線分を 3 次ベジエへ次数上げしない。
/// 直線は `rlineto`/`hlineto`/`vlineto` へ、曲線は `rrcurveto` へ、それぞれ
/// 対応する CharString のオペレータに直接対応する。
enum StaticSeg {
    /// 直線分。CFF2 は 2 次以上の曲線オペレータしか持たないため、直線は
    /// 次数上げせずそのまま保持する。
    Line(kurbo::Line),
    /// 3 次ベジエ曲線。2 次ベジエ (`QuadTo`) は、形状を変えないまま
    /// `elevate_quad` で次数上げした上でここに格納する。
    Cubic(kurbo::CubicBez),
}

/// `BezPath` を、サブパスごとに区切った `StaticSeg` の列に変換する。
///
/// `to_cubic_subpaths` との違いは、直線分 (`LineTo` および `ClosePath` が
/// 補う暗黙の直線) を 3 次ベジエへ次数上げせず `StaticSeg::Line` のまま
/// 保持する点である。これにより、後続の CharString 組み立てで
/// 直線を `rlineto`/`hlineto`/`vlineto` (6 バイト前後で済む 2 引数以下の
/// オペレータ) として書き出せ、次数上げした場合の `rrcurveto` (6 引数) より
/// 大幅にコンパクトになる。2 次ベジエは CFF2 に対応するオペレータがない
/// ため、`to_cubic_subpaths` と同様に次数上げを行う。
///
/// 座標を整数のフォントデザイン単位へ丸める理由は `to_cubic_subpaths` と
/// 同じであり、経路の長さによらず誤差が蓄積しないようにするためである。
///
/// # Args
/// - `path` - 変換元の `BezPath` である。
///
/// # Returns
/// 各要素がサブパス 1 本分の `StaticSeg` 列である `Vec<Vec<StaticSeg>>` を
/// 返す。
fn to_static_subpaths(path: &kurbo::BezPath) -> Vec<Vec<StaticSeg>> {
    let mut subpaths = Vec::new();
    let mut current = Vec::new();
    // 現在のペン位置と、現在のサブパスの始点。ClosePath で始点に戻る際の
    // 判定に始点を用いる。いずれも整数に丸めた値を保持する。
    let mut cur = kurbo::Point::ZERO;
    let mut start = kurbo::Point::ZERO;
    for element in path.elements() {
        match *element {
            kurbo::PathEl::MoveTo(p) => {
                // 直前のサブパスが存在すれば確定させ、新しいサブパスを開始する。
                if !current.is_empty() {
                    subpaths.push(std::mem::take(&mut current));
                }
                cur = round_point_to_integer(p);
                start = cur;
            }
            kurbo::PathEl::LineTo(p) => {
                let p = round_point_to_integer(p);
                current.push(StaticSeg::Line(kurbo::Line::new(cur, p)));
                cur = p;
            }
            kurbo::PathEl::QuadTo(c, p) => {
                let c = round_point_to_integer(c);
                let p = round_point_to_integer(p);
                current.push(StaticSeg::Cubic(elevate_quad(cur, c, p)));
                cur = p;
            }
            kurbo::PathEl::CurveTo(c1, c2, p) => {
                let c1 = round_point_to_integer(c1);
                let c2 = round_point_to_integer(c2);
                let p = round_point_to_integer(p);
                current.push(StaticSeg::Cubic(kurbo::CubicBez::new(cur, c1, c2, p)));
                cur = p;
            }
            kurbo::PathEl::ClosePath => {
                // 始点まで戻っていなければ、暗黙の直線で閉じる。
                if cur != start {
                    current.push(StaticSeg::Line(kurbo::Line::new(cur, start)));
                    cur = start;
                }
            }
        }
    }
    if !current.is_empty() {
        subpaths.push(current);
    }
    subpaths
}

/// 丸めた後の 1 グリフ分の輪郭から、可変軸を持たない静的な CFF2 チャート
/// ストリングのバイト列を組み立てる。
///
/// `build_glyph_charstring` と異なり 2 マスターの補間を行わないため、
/// `blend` オペレータもデルタ値も書かない。さらに `to_cubic_subpaths` を
/// 使う `build_glyph_charstring` とも異なり、直線分は 3 次ベジエへ次数上げ
/// せず `to_static_subpaths` からそのまま受け取り、`push_static_line` で
/// `rlineto`/`hlineto`/`vlineto` として書き出す。曲線のみ座標を直前の点
/// からの相対値として `push_charstring_number` で直接書き込み、
/// `rrcurveto` で表現する。CJK の字形は直線区間が多くを占めるため、この
/// 使い分けにより、直線 1 本あたり最大 6 引数の `rrcurveto` が最大 2 引数の
/// 演算子へ縮む。丸め弧自体も `round::round_path` の返す 1 本の 3 次ベジエ
/// のまま焼き付くため、丸め弧あたりのセグメント数は `round_path_matched`
/// 経由の半分で済む。
///
/// # Args
/// - `rounded` - 丸めた後の輪郭である。`round::round_path` の結果を渡す
///   ことを想定する。
///
/// # Returns
/// 組み立てた CharString のバイト列を返す。輪郭を持たないグリフに
/// 対しては空のバイト列を返す。
fn build_static_glyph_charstring(rounded: &kurbo::BezPath) -> Vec<u8> {
    let subpaths = to_static_subpaths(rounded);

    let mut buf = Vec::new();
    // 直前に書き込んだ点を追跡し、各座標をそこからの相対値として書き出す。
    let mut cur = kurbo::Point::ZERO;

    for segs in &subpaths {
        // 空のサブパス (輪郭を持たないグリフなど) はスキップする。
        let Some(first) = segs.first() else {
            continue;
        };

        // このサブパスの始点への rmoveto を、直前の点からの差分として書く。
        let start = match first {
            StaticSeg::Line(line) => line.p0,
            StaticSeg::Cubic(cubic) => cubic.p0,
        };
        push_charstring_number(&mut buf, start.x - cur.x);
        push_charstring_number(&mut buf, start.y - cur.y);
        buf.push(21); // rmoveto
        cur = start;

        for seg in segs {
            match seg {
                StaticSeg::Line(line) => {
                    push_static_line(&mut buf, line.p1 - cur);
                    cur = line.p1;
                }
                StaticSeg::Cubic(cubic) => {
                    // rrcurveto の 3 組の相対座標 (制御点 1、制御点 2、
                    // 終点) を、それぞれ直前の点からの差分として求める。
                    let d1 = cubic.p1 - cur;
                    let d2 = cubic.p2 - cubic.p1;
                    let d3 = cubic.p3 - cubic.p2;
                    push_charstring_number(&mut buf, d1.x);
                    push_charstring_number(&mut buf, d1.y);
                    push_charstring_number(&mut buf, d2.x);
                    push_charstring_number(&mut buf, d2.y);
                    push_charstring_number(&mut buf, d3.x);
                    push_charstring_number(&mut buf, d3.y);
                    buf.push(8); // rrcurveto
                    cur = cubic.p3;
                }
            }
        }
    }

    buf
}

/// 直前の点から `delta` だけ移動する直線分を、その向きに応じて最も
/// コンパクトなオペレータで書き出す。
///
/// `delta` の `y` 成分が 0 であれば水平線分なので `hlineto` (引数 1 個)、
/// `x` 成分が 0 であれば垂直線分なので `vlineto` (引数 1 個) を使う。
/// いずれの成分も 0 でない斜めの線分は `rlineto` (引数 2 個) で書く。両方の
/// 成分が 0 である退化した (実質的に移動しない) 線分は、描画結果に影響
/// しないためオペレータ自体を書かずに読み飛ばす。
///
/// CJK の字形は水平・垂直な画が大部分を占めるため、この使い分けにより
/// 直線 1 本あたりの引数の個数を実質的に半分程度へ削減できる。
///
/// # Args
/// - `buf` - 書き込み先のバイト列である。
/// - `delta` - 直前の点から見た、直線の終点までの相対座標である。
fn push_static_line(buf: &mut Vec<u8>, delta: kurbo::Vec2) {
    if delta.x == 0.0 && delta.y == 0.0 {
        return;
    }
    if delta.y == 0.0 {
        push_charstring_number(buf, delta.x);
        buf.push(6); // hlineto
    } else if delta.x == 0.0 {
        push_charstring_number(buf, delta.y);
        buf.push(7); // vlineto
    } else {
        push_charstring_number(buf, delta.x);
        push_charstring_number(buf, delta.y);
        buf.push(5); // rlineto
    }
}

/// 丸みによる差分が存在しない (すべての `deltas` がほぼ 0 である) 区間の
/// 割合を検出するための許容誤差である。
///
/// 輪郭のうち丸めの影響を受けない区間 (長い直線の途中など) は、
/// `round::round_path_matched` の実装上、2 つのマスターへ全く同じ座標を
/// 書き込んでいるため、本来は厳密に 0 になる。ごくわずかな余裕を持たせて
/// いるのは、浮動小数点演算の順序によって生じうる無視できる誤差を吸収する
/// ためである。
const ZERO_DELTA_EPSILON: f64 = 1e-6;

/// オペランドを、`blend` が本当に必要な場合にのみ使うことで、チャート
/// ストリングのバイト数を削減する。
///
/// `deltas` がすべて (誤差の範囲内で) 0 である場合、丸みによってその区間の
/// 形状が変化していないことを意味する。CJK の字形は直線区間が多くを占め、
/// かつ角の丸めは一部の頂点近傍にしか影響しないため、この最適化により
/// 大部分の区間で `blend` のオーバーヘッド (デフォルト値とデルタ値を両方
/// 積み、個数と `blend` オペレータを書く) を避けられる。`deltas` に 1 つでも
/// 0 でない値があれば、通常どおり `push_blend_operator` で書き出す。
///
/// # Args
/// - `buf` - 書き込み先のバイト列である。
/// - `defaults` - 各オペランドの既定値 (ROND = 最小値のときの値) である。
/// - `deltas` - `defaults` と対になる、最大値までの差分である。
fn push_number_or_blend_operands(buf: &mut Vec<u8>, defaults: &[f64], deltas: &[f64]) {
    if deltas.iter().all(|d| d.abs() < ZERO_DELTA_EPSILON) {
        for &value in defaults {
            push_charstring_number(buf, value);
        }
    } else {
        push_blend_operator(buf, defaults, deltas);
    }
}

/// CFF2 CharString の `blend` オペレータを、必要なオペランドと共に
/// 書き出す。
///
/// `defaults` は ROND 軸の最小値 (既定値) における値、`deltas` はそこから
/// 最大値までの差分である。ROND 軸は単一のリージョンだけを持つため、
/// `n` 個のデフォルト値・`n` 個のデルタ値・個数 `n` の順にスタックへ積み、
/// 最後に `blend` オペレータ (16) を書く。
///
/// # Args
/// - `buf` - 書き込み先のバイト列である。
/// - `defaults` - 各オペランドの既定値 (ROND = 最小値のときの値) である。
/// - `deltas` - `defaults` と対になる、最大値までの差分である。
fn push_blend_operator(buf: &mut Vec<u8>, defaults: &[f64], deltas: &[f64]) {
    debug_assert_eq!(defaults.len(), deltas.len());
    for &value in defaults {
        push_charstring_number(buf, value);
    }
    for &value in deltas {
        push_charstring_number(buf, value);
    }
    push_charstring_int(buf, defaults.len() as i32);
    buf.push(16); // blend
}

/// 整数と非有限値以外の値との差が、これ以下であれば整数とみなす許容誤差
/// である。輪郭の座標はもともと整数のフォントデザイン単位だが、丸め処理の
/// 幾何計算 (三角関数や曲線の分割) を経ることで、本来整数であるべき値に
/// ごくわずかな浮動小数点誤差が乗ることがある。この誤差はフォントの
/// 1000 分の 1 未満であり目視では区別できないため、整数として丸めて
/// コンパクトに書き出してよい。
const INTEGER_SNAP_EPSILON: f64 = 1e-3;

/// CFF2 CharString の数値オペランドを、最も短い表現で書き出す。
///
/// `value` が (誤差の範囲内で) 整数とみなせる場合は、`push_charstring_int`
/// による整数エンコード (多くの場合 1〜3 バイト) を使う。整数とみなせない
/// 場合のみ、16.16 固定小数点数 (オペコード 255、5 バイト) にフォールバック
/// する。座標のほとんどは整数であるため、この使い分けによってチャート
/// ストリング全体のバイト数を大きく削減できる。
///
/// # Args
/// - `buf` - 書き込み先のバイト列である。
/// - `value` - 書き込む数値である。
fn push_charstring_number(buf: &mut Vec<u8>, value: f64) {
    let rounded = value.round();
    let is_integer = (value - rounded).abs() < INTEGER_SNAP_EPSILON
        && rounded >= f64::from(i32::MIN)
        && rounded <= f64::from(i32::MAX);
    if is_integer {
        push_charstring_int(buf, rounded as i32);
    } else {
        push_charstring_fixed(buf, value);
    }
}

/// CFF2 CharString の数値オペランドを、16.16 固定小数点数
/// (オペコード 255) として書き出す。
///
/// `push_charstring_number` が、整数とみなせない値のフォールバック先として
/// 使う。
fn push_charstring_fixed(buf: &mut Vec<u8>, value: f64) {
    buf.push(255);
    buf.extend_from_slice(&Fixed::from_f64(value).to_bits().to_be_bytes());
}

/// CFF2 CharString の整数オペランドを、最短の表現で書き出す。
///
/// `blend` オペレータの個数引数のような小さな整数にのみ用いる。
/// エンコード方式は <https://learn.microsoft.com/en-us/typography/opentype/spec/cff2#table-3-operand-encoding>
/// の Table 3 に従う。
fn push_charstring_int(buf: &mut Vec<u8>, value: i32) {
    match value {
        -107..=107 => buf.push((value + 139) as u8),
        108..=1131 => {
            let v = value - 108;
            buf.push(247 + (v >> 8) as u8);
            buf.push((v & 0xFF) as u8);
        }
        -1131..=-108 => {
            let v = -value - 108;
            buf.push(251 + (v >> 8) as u8);
            buf.push((v & 0xFF) as u8);
        }
        -32768..=32767 => {
            buf.push(28);
            buf.extend_from_slice(&(value as i16).to_be_bytes());
        }
        _ => {
            buf.push(29);
            buf.extend_from_slice(&value.to_be_bytes());
        }
    }
}

/// DICT の整数オペランドを、値によらず常に 5 バイト (オペコード 29 +
/// 32 ビット整数) で書き出す。
///
/// トップ DICT・Font DICT に埋め込むオフセット値は、他のテーブルの配置を
/// 決めた後でなければ確定しない。そこで、値に関わらずバイト長が一定となる
/// この形式を用いることで、「DICT のバイト長を決めてからオフセット値を
/// 計算し、それを DICT に書き戻す」という単純な 1 パスの手順を可能にして
/// いる。
fn push_dict_offset(buf: &mut Vec<u8>, value: i32) {
    buf.push(29);
    buf.extend_from_slice(&value.to_be_bytes());
}

/// DICT の実数オペランド (オペコード 30) を、10 進表記のニブル列として
/// 書き出す。
///
/// <https://learn.microsoft.com/en-us/typography/opentype/spec/cff2#table-5-nibble-definitions>
/// の対応表に従い、`value` の10進表記の各文字をニブルに変換したうえで
/// 2 個ずつバイトへ詰める。本モジュールが実際にこの関数へ渡す値
/// (FontMatrix の要素) は指数表記を必要としない範囲に限られる。
fn push_dict_real(buf: &mut Vec<u8>, value: f64) {
    let text = format!("{value}");
    let mut nibbles = Vec::with_capacity(text.len() + 1);
    for ch in text.chars() {
        let nibble = match ch {
            '0'..='9' => ch as u8 - b'0',
            '.' => 0xA,
            '-' => 0xE,
            _ => panic!("unsupported character in DICT real number: {ch}"),
        };
        nibbles.push(nibble);
    }
    nibbles.push(0xF); // 終端ニブル
    if nibbles.len() % 2 != 0 {
        nibbles.push(0xF); // バイト境界に合わせるパディング
    }

    buf.push(30);
    for pair in nibbles.chunks_exact(2) {
        buf.push((pair[0] << 4) | pair[1]);
    }
}

/// CFF2 トップ DICT のバイト列を組み立てる。
///
/// # Args
/// - `charstrings_offset` - CharStrings INDEX への、CFF2 テーブル先頭からの
///   絶対オフセットである。
/// - `fd_array_offset` - FDArray INDEX への絶対オフセットである。
/// - `variation_store_offset` - VariationStore データ (2 バイトの長さ
///   フィールドを含む) への絶対オフセットである。
/// - `matrix_scale` - FontMatrix の対角成分 (`1 / unitsPerEm`) である。
///
/// # Returns
/// 組み立てたトップ DICT のバイト列を返す。
fn build_top_dict(
    charstrings_offset: i32,
    fd_array_offset: i32,
    variation_store_offset: i32,
    matrix_scale: f64,
) -> Vec<u8> {
    let mut buf = Vec::new();

    // FontMatrix (12 7): head の unitsPerEm を CFF2 座標系に反映する。
    push_dict_real(&mut buf, matrix_scale);
    push_dict_real(&mut buf, 0.0);
    push_dict_real(&mut buf, 0.0);
    push_dict_real(&mut buf, matrix_scale);
    push_dict_real(&mut buf, 0.0);
    push_dict_real(&mut buf, 0.0);
    buf.push(12);
    buf.push(7);

    // CharstringsOffset (17)
    push_dict_offset(&mut buf, charstrings_offset);
    buf.push(17);

    // FDArrayOffset (12 36)
    push_dict_offset(&mut buf, fd_array_offset);
    buf.push(12);
    buf.push(36);

    // VariationStoreOffset (24)
    push_dict_offset(&mut buf, variation_store_offset);
    buf.push(24);

    buf
}

/// 可変軸を持たない静的な CFF2 のトップ DICT のバイト列を組み立てる。
///
/// `build_top_dict` から VariationStoreOffset (24) を除いたものである。
/// `blend` オペレータを一切使わない静的な CharString では item
/// variation store が不要であり、CFF2 の仕様上 VariationStore は省略できる
/// ため、この参照自体を書かない。
///
/// # Args
/// - `charstrings_offset` - CharStrings INDEX への、CFF2 テーブル先頭からの
///   絶対オフセットである。
/// - `fd_array_offset` - FDArray INDEX への絶対オフセットである。
/// - `matrix_scale` - FontMatrix の対角成分 (`1 / unitsPerEm`) である。
///
/// # Returns
/// 組み立てたトップ DICT のバイト列を返す。
fn build_static_top_dict(
    charstrings_offset: i32,
    fd_array_offset: i32,
    matrix_scale: f64,
) -> Vec<u8> {
    let mut buf = Vec::new();

    // FontMatrix (12 7): head の unitsPerEm を CFF2 座標系に反映する。
    push_dict_real(&mut buf, matrix_scale);
    push_dict_real(&mut buf, 0.0);
    push_dict_real(&mut buf, 0.0);
    push_dict_real(&mut buf, matrix_scale);
    push_dict_real(&mut buf, 0.0);
    push_dict_real(&mut buf, 0.0);
    buf.push(12);
    buf.push(7);

    // CharstringsOffset (17)
    push_dict_offset(&mut buf, charstrings_offset);
    buf.push(17);

    // FDArrayOffset (12 36)
    push_dict_offset(&mut buf, fd_array_offset);
    buf.push(12);
    buf.push(36);

    buf
}

/// FDArray に格納する Font DICT のバイト列を組み立てる。
///
/// Private DICT への `size`・`offset` (12 18 ではなく単一オペコード 18)
/// のみを持つ最小限の Font DICT である。
///
/// # Args
/// - `private_dict_offset` - Private DICT への、CFF2 テーブル先頭からの
///   絶対オフセットである。
/// - `private_dict_size` - Private DICT のバイト長である。
///
/// # Returns
/// 組み立てた Font DICT のバイト列を返す。
fn build_font_dict(private_dict_offset: i32, private_dict_size: i32) -> Vec<u8> {
    let mut buf = Vec::new();
    push_dict_offset(&mut buf, private_dict_size);
    push_dict_offset(&mut buf, private_dict_offset);
    buf.push(18); // Private
    buf
}

/// CFF2 テーブル全体のバイト列を組み立てる。
///
/// トップ DICT・Font DICT はいずれもオフセットを固定幅でエンコードする
/// ため、1 回目はダミー値 (0) で組み立ててバイト長だけを確定させ、その
/// 長さから実際のオフセット値を計算したうえで、同じ長さのまま実際の値で
/// 組み立て直す、という 2 段階の手順で構築する。
///
/// # Args
/// - `charstrings` - グリフ ID 順の CharString のバイト列である。
/// - `matrix_scale` - FontMatrix の対角成分 (`1 / unitsPerEm`) である。
///
/// # Returns
/// 組み立てた CFF2 テーブルのバイト列を返す。
fn build_cff2_table(charstrings: &[Vec<u8>], matrix_scale: f64) -> Vec<u8> {
    const HEADER_SIZE: usize = 5;

    // Global Subr INDEX は空でよい (すべてのサブルーチンはインライン展開
    // 済みであり、グローバルサブルーチンを参照しない)。
    let global_subr_index_bytes = write_fonts::dump_table(&v2::Index::from_items(Vec::new()))
        .expect("Index の組み立てに失敗した");
    let charstrings_index_bytes =
        write_fonts::dump_table(&v2::Index::from_items(charstrings.to_vec()))
            .expect("Index の組み立てに失敗した");

    // Private DICT は空でよい (デフォルト幅などの値はすべて hmtx 側で
    // 管理しており、CFF2 の CharString は width を持たない)。
    let private_dict_bytes = Vec::<u8>::new();

    // 1 回目: Font DICT・トップ DICT のバイト長だけを確定させるための
    // ダミー値による組み立てである。オフセットは常に固定幅でエンコード
    // されるため、実際の値に置き換えてもバイト長は変化しない。
    let font_dict_bytes_len = build_font_dict(0, 0).len();
    let fd_array_index_bytes_len =
        write_fonts::dump_table(&v2::Index::from_items(vec![vec![0; font_dict_bytes_len]]))
            .expect("Index の組み立てに失敗した")
            .len();
    let top_dict_bytes_len = build_top_dict(0, 0, 0, matrix_scale).len();

    // トップ DICT の直後に Global Subr INDEX・CharStrings INDEX・FDArray
    // INDEX・Private DICT・VariationStore データがこの順に続く。すべて
    // 絶対オフセットは CFF2 テーブル先頭からの位置である。
    let trailing_data_start = HEADER_SIZE + top_dict_bytes_len;
    let charstrings_offset = trailing_data_start + global_subr_index_bytes.len();
    let fd_array_offset = charstrings_offset + charstrings_index_bytes.len();
    let private_dict_offset = fd_array_offset + fd_array_index_bytes_len;
    let variation_store_offset = private_dict_offset + private_dict_bytes.len();

    // 2 回目: 実際のオフセット値で組み立て直す。
    let font_dict_bytes =
        build_font_dict(private_dict_offset as i32, private_dict_bytes.len() as i32);
    debug_assert_eq!(font_dict_bytes.len(), font_dict_bytes_len);
    let fd_array_index_bytes =
        write_fonts::dump_table(&v2::Index::from_items(vec![font_dict_bytes]))
            .expect("Index の組み立てに失敗した");
    debug_assert_eq!(fd_array_index_bytes.len(), fd_array_index_bytes_len);
    let top_dict_bytes = build_top_dict(
        charstrings_offset as i32,
        fd_array_offset as i32,
        variation_store_offset as i32,
        matrix_scale,
    );
    debug_assert_eq!(top_dict_bytes.len(), top_dict_bytes_len);

    let item_variation_store_bytes =
        write_fonts::dump_table(&build_item_variation_store()).expect("IVS の組み立てに失敗した");

    let mut trailing_data = Vec::new();
    trailing_data.extend_from_slice(&global_subr_index_bytes);
    trailing_data.extend_from_slice(&charstrings_index_bytes);
    trailing_data.extend_from_slice(&fd_array_index_bytes);
    trailing_data.extend_from_slice(&private_dict_bytes);
    // VariationStore データは、実データの前に 2 バイトの長さフィールドを
    // 持つ (CFF2 のトップ DICT の VariationStoreOffset は、この長さ
    // フィールドの位置を指す)。
    trailing_data.extend_from_slice(&(item_variation_store_bytes.len() as u16).to_be_bytes());
    trailing_data.extend_from_slice(&item_variation_store_bytes);

    let header = v2::Cff2Header::new(
        HEADER_SIZE as u8,
        top_dict_bytes.len() as u16,
        Vec::new(),
        top_dict_bytes,
        trailing_data,
    );
    write_fonts::dump_table(&header).expect("CFF2 ヘッダーの組み立てに失敗した")
}

/// 可変軸を持たない静的な CFF2 テーブル全体のバイト列を組み立てる。
///
/// `build_cff2_table` から item variation store とトップ DICT の
/// VariationStoreOffset を取り除いたものである。トップ DICT・Font DICT の
/// オフセットを固定幅でエンコードし、1 回目のダミー値による組み立てで
/// バイト長を確定させてから 2 回目に実際のオフセットで組み立て直す、という
/// 2 段階の手順は `build_cff2_table` と同じである。
///
/// # Args
/// - `charstrings` - グリフ ID 順の CharString のバイト列である。
/// - `matrix_scale` - FontMatrix の対角成分 (`1 / unitsPerEm`) である。
///
/// # Returns
/// 組み立てた CFF2 テーブルのバイト列を返す。
fn build_static_cff2_table(charstrings: &[Vec<u8>], matrix_scale: f64) -> Vec<u8> {
    const HEADER_SIZE: usize = 5;

    // Global Subr INDEX は空でよい (すべてのサブルーチンはインライン展開
    // 済みであり、グローバルサブルーチンを参照しない)。
    let global_subr_index_bytes = write_fonts::dump_table(&v2::Index::from_items(Vec::new()))
        .expect("Index の組み立てに失敗した");
    let charstrings_index_bytes =
        write_fonts::dump_table(&v2::Index::from_items(charstrings.to_vec()))
            .expect("Index の組み立てに失敗した");

    // Private DICT は空でよい (幅などの値は hmtx 側で管理する)。
    let private_dict_bytes = Vec::<u8>::new();

    // 1 回目: Font DICT・トップ DICT のバイト長だけを確定させるための
    // ダミー値による組み立てである。オフセットは常に固定幅でエンコード
    // されるため、実際の値に置き換えてもバイト長は変化しない。
    let font_dict_bytes_len = build_font_dict(0, 0).len();
    let fd_array_index_bytes_len =
        write_fonts::dump_table(&v2::Index::from_items(vec![vec![0; font_dict_bytes_len]]))
            .expect("Index の組み立てに失敗した")
            .len();
    let top_dict_bytes_len = build_static_top_dict(0, 0, matrix_scale).len();

    // トップ DICT の直後に Global Subr INDEX・CharStrings INDEX・FDArray
    // INDEX・Private DICT がこの順に続く。VariationStore は持たない。
    let trailing_data_start = HEADER_SIZE + top_dict_bytes_len;
    let charstrings_offset = trailing_data_start + global_subr_index_bytes.len();
    let fd_array_offset = charstrings_offset + charstrings_index_bytes.len();
    let private_dict_offset = fd_array_offset + fd_array_index_bytes_len;

    // 2 回目: 実際のオフセット値で組み立て直す。
    let font_dict_bytes =
        build_font_dict(private_dict_offset as i32, private_dict_bytes.len() as i32);
    debug_assert_eq!(font_dict_bytes.len(), font_dict_bytes_len);
    let fd_array_index_bytes =
        write_fonts::dump_table(&v2::Index::from_items(vec![font_dict_bytes]))
            .expect("Index の組み立てに失敗した");
    debug_assert_eq!(fd_array_index_bytes.len(), fd_array_index_bytes_len);
    let top_dict_bytes = build_static_top_dict(
        charstrings_offset as i32,
        fd_array_offset as i32,
        matrix_scale,
    );
    debug_assert_eq!(top_dict_bytes.len(), top_dict_bytes_len);

    let mut trailing_data = Vec::new();
    trailing_data.extend_from_slice(&global_subr_index_bytes);
    trailing_data.extend_from_slice(&charstrings_index_bytes);
    trailing_data.extend_from_slice(&fd_array_index_bytes);
    trailing_data.extend_from_slice(&private_dict_bytes);

    let header = v2::Cff2Header::new(
        HEADER_SIZE as u8,
        top_dict_bytes.len() as u16,
        Vec::new(),
        top_dict_bytes,
        trailing_data,
    );
    write_fonts::dump_table(&header).expect("CFF2 ヘッダーの組み立てに失敗した")
}

/// ROND 軸 1 つだけを持つ item variation store を組み立てる。
///
/// ROND 軸の最小値 (0.0) では丸める前の字形に、最大値 (1.0) では丸めた後の
/// 字形になるよう、単一のリージョン (start=0, peak=1, end=1) を定義する。
/// 各グリフの実際のデルタ値は CharString の `blend` オペランドとして
/// 直接埋め込まれるため、ここでの `ItemVariationData` はリージョンの構成
/// (このリージョン 1 つだけが有効であること) を示すためだけに存在し、
/// 実データの行は持たない。
///
/// # Returns
/// 組み立てた `ItemVariationStore` を返す。
fn build_item_variation_store() -> variations::ItemVariationStore {
    let region = variations::VariationRegion::new(vec![variations::RegionAxisCoordinates::new(
        F2Dot14::from_f64(0.0),
        F2Dot14::from_f64(1.0),
        F2Dot14::from_f64(1.0),
    )]);
    let region_list = variations::VariationRegionList::new(1, vec![region]);
    let item_variation_data = variations::ItemVariationData::new(0, 0, vec![0], Vec::new());
    variations::ItemVariationStore::new(region_list, vec![Some(item_variation_data)])
}

/// ROND 軸 1 つだけを持つ `fvar` テーブルを組み立てる。
///
/// 最小値・既定値をともに 0.0 (丸める前)、最大値を 1.0 (`rounded_glyphs`
/// の字形) とする。軸の表示名として参照する `ROUNDNESS_AXIS_NAME_ID` は、
/// `build_name_table_with_axis_name` が `name` テーブルへ対応するレコードを
/// 追加することを前提にしている。
///
/// # Returns
/// 組み立てた `Fvar` テーブルを返す。
fn build_fvar() -> fvar::Fvar {
    let axis = fvar::VariationAxisRecord::new(
        ROUNDNESS_AXIS_TAG,
        Fixed::from_f64(0.0),
        Fixed::from_f64(0.0),
        Fixed::from_f64(1.0),
        0,
        ROUNDNESS_AXIS_NAME_ID,
    );
    fvar::Fvar::new(fvar::AxisInstanceArrays::new(vec![axis], Vec::new()))
}

/// `font` の既存の `name` テーブルに含まれるすべてのレコードを引き継ぎつつ、
/// `ROUNDNESS_AXIS_NAME_ID` に対応する ROND 軸の表示名レコード (Windows・
/// 英語 (米国)) を追加した、新しい `name` テーブルを組み立てる。
///
/// `build_fvar` が組み立てる `fvar` の軸レコードは、表示名として
/// `ROUNDNESS_AXIS_NAME_ID` を参照する。OpenType の仕様上、`fvar` が参照する
/// nameID は `name` テーブルに実在するレコードを指す必要があるため、本関数を
/// 使わずに `name` テーブルを元のフォントからそのまま引き継ぐと、参照先の
/// 存在しない不正な `fvar` になってしまう。
///
/// # Args
/// - `font` - 既存の `name` テーブルを引き継ぐ元になる、変換元の静的
///   フォントである。
///
/// # Returns
/// 元の `name` テーブルの全レコードに、ROND 軸の表示名レコードを 1 つ追加
/// した `Name` テーブルを返す。
///
/// # Panics
/// - `font` の `name` テーブルの解析に失敗した場合にパニックする。
fn build_name_table_with_axis_name(font: &read_fonts::FontRef) -> write_fonts::tables::name::Name {
    use write_fonts::tables::name::NameRecord;

    const WINDOWS_PLATFORM_ID: u16 = 3;
    const WINDOWS_ENCODING_ID: u16 = 1;
    const WINDOWS_LANG_ID_EN_US: u16 = 0x0409;

    let original_name = font.name().expect("name テーブルの解析に失敗した");
    let string_data = original_name.string_data();

    // 既存のレコードは、プラットフォームを問わずすべてそのまま引き継ぐ。
    let mut records = original_name
        .name_record()
        .iter()
        .filter_map(|record| {
            let value = record.string(string_data).ok()?;
            Some(NameRecord::new(
                record.platform_id(),
                record.encoding_id(),
                record.language_id(),
                record.name_id(),
                value.to_string().into(),
            ))
        })
        .collect::<Vec<NameRecord>>();

    // ROND 軸の表示名レコードを追加する。
    records.push(NameRecord::new(
        WINDOWS_PLATFORM_ID,
        WINDOWS_ENCODING_ID,
        WINDOWS_LANG_ID_EN_US,
        ROUNDNESS_AXIS_NAME_ID,
        ROUNDNESS_AXIS_NAME.to_string().into(),
    ));

    // name テーブルの仕様上、name_record 配列は
    // (platformID, encodingID, languageID, nameID) の昇順でソートされて
    // いる必要がある。
    records.sort_by_key(|r| (r.platform_id, r.encoding_id, r.language_id, r.name_id));

    write_fonts::tables::name::Name::new(records)
}

/// ROND 軸 1 つだけを持つ、恒等写像の `avar` テーブルを組み立てる。
///
/// 本モジュールが定義する ROND 軸はもともと線形なので、`avar` による
/// 再マッピングは行わず、恒等に対応付けるのみとする。avar の仕様上、
/// 各軸の segment map は正規化座標 -1・0・1 の 3 点を必ず含まなければ
/// ならない (欠けていると Firefox などが `avar` テーブルごと破棄する)。
/// 本プロジェクトの ROND 軸は負の値を取らないため -1 は実際には使われないが、
/// 仕様を満たすために恒等写像として含めておく。
///
/// # Returns
/// 組み立てた `Avar` テーブルを返す。
fn build_avar() -> avar::Avar {
    let segment_map = avar::SegmentMaps::new(vec![
        avar::AxisValueMap::new(F2Dot14::from_f64(-1.0), F2Dot14::from_f64(-1.0)),
        avar::AxisValueMap::new(F2Dot14::from_f64(0.0), F2Dot14::from_f64(0.0)),
        avar::AxisValueMap::new(F2Dot14::from_f64(1.0), F2Dot14::from_f64(1.0)),
    ]);
    avar::Avar::new(vec![segment_map])
}

#[cfg(test)]
mod tests {
    use super::build_variable_font;

    mod test_font;

    // シナリオ: 組み立てた可変フォントの先頭 4 バイトは、CFF2 ベースの
    // OpenType フォントであることを示す sfnt バージョンタグ `OTTO` である。
    #[test]
    fn starts_with_otto_sfnt_tag() {
        // Arrange
        let (font_data, matched_glyphs) = test_font::build_test_font();
        let sut = build_variable_font;

        // Act
        let variable_font_data = sut(&font_data, &matched_glyphs);

        // Assert
        assert_eq!(b"OTTO", &variable_font_data[0..4]);
    }

    // シナリオ: `matched_glyphs` の要素数が `original_font_data` のグリフ数と
    // 一致しない場合はパニックする。
    #[test]
    fn panics_when_glyph_count_mismatches() {
        // Arrange
        let (font_data, mut matched_glyphs) = test_font::build_test_font();
        matched_glyphs.pop();
        let sut = build_variable_font;

        // Act
        let result = std::panic::catch_unwind(|| sut(&font_data, &matched_glyphs));

        // Assert
        assert!(result.is_err());
    }

    // シナリオ: 組み立てた可変フォントは `ROND` という軸を 1 つだけ持ち、
    // その最小値と既定値が一致する。
    #[test]
    fn fvar_has_single_rond_axis_with_matching_min_and_default() {
        // Arrange
        let (font_data, matched_glyphs) = test_font::build_test_font();
        let sut = build_variable_font;

        // Act
        let variable_font_data = sut(&font_data, &matched_glyphs);

        // Assert
        let font = read_fonts::FontRef::new(&variable_font_data).unwrap();
        let fvar = skrifa::MetadataProvider::axes(&font);
        assert_eq!(1, fvar.len());
        let axis = fvar.get(0).unwrap();
        assert_eq!(read_fonts::types::Tag::new(b"ROND"), axis.tag());
        assert_eq!(axis.min_value(), axis.default_value());
        assert_eq!(0.0, axis.min_value());
        assert_eq!(1.0, axis.max_value());
    }

    // シナリオ: `ROND` 軸が参照する nameID は、name テーブルに実在する
    // レコードを指しており、その文字列を実際に引ける。参照先が存在しない
    // nameID だと、OpenType の仕様上不正な fvar になってしまう。
    #[test]
    fn fvar_axis_name_id_resolves_to_an_actual_name_record() {
        // Arrange
        let (font_data, matched_glyphs) = test_font::build_test_font();
        let sut = build_variable_font;

        // Act
        let variable_font_data = sut(&font_data, &matched_glyphs);

        // Assert
        let font = read_fonts::FontRef::new(&variable_font_data).unwrap();
        let fvar = skrifa::MetadataProvider::axes(&font);
        let axis = fvar.get(0).unwrap();
        let axis_name = skrifa::MetadataProvider::localized_strings(&font, axis.name_id())
            .english_or_first()
            .expect("ROND 軸の nameID に対応する name レコードが存在しない");
        assert_eq!("Roundness", axis_name.to_string());
    }

    // シナリオ: `ROND` 軸を最小値に固定して描画すると元の輪郭に、最大値に
    // 固定して描画すると丸めた輪郭に、それぞれ一致する。
    #[test]
    fn rendering_at_axis_extremes_matches_original_and_rounded_outlines() {
        use kurbo::Shape;

        // Arrange
        let (font_data, matched_glyphs) = test_font::build_test_font();
        let sut = build_variable_font;
        let original_glyphs = test_font::original_glyph_outlines();

        // Act
        let variable_font_data = sut(&font_data, &matched_glyphs);

        // Assert
        let font = read_fonts::FontRef::new(&variable_font_data).unwrap();
        let outline_glyphs = skrifa::MetadataProvider::outline_glyphs(&font);
        let rounded_glyphs = matched_glyphs.iter().map(|(_, rounded)| rounded);
        for (gid, (original, rounded)) in original_glyphs.iter().zip(rounded_glyphs).enumerate() {
            let glyph = outline_glyphs
                .get(read_fonts::types::GlyphId::new(gid as u32))
                .unwrap();

            // 軸の最小値 (既定値) では、元の輪郭のバウンディングボックスと
            // 一致するはずである。
            let min_path = draw_at_coord(&glyph, 0.0);
            assert_bounding_box_close(original.bounding_box(), min_path.bounding_box());

            // 軸の最大値では、丸めた輪郭のバウンディングボックスと一致する
            // はずである。
            let max_path = draw_at_coord(&glyph, 1.0);
            assert_bounding_box_close(rounded.bounding_box(), max_path.bounding_box());
        }
    }

    // シナリオ: `subroutinize` でサブルーチン化したフォントは、`ROND` 軸の
    // 最小値・最大値のいずれで描画しても、サブルーチン化する前と同じ字形
    // (バウンディングボックス) になる。すなわち、サブルーチン化はチャート
    // ストリングの表現を最適化するだけで、描画結果を変えない。
    //
    // このテストの実行には、外部コマンド `cffsubr` が `PATH` 上に
    // インストールされている必要がある (`pip install cffsubr`)。
    #[test]
    fn subroutinize_preserves_outlines_at_axis_extremes() {
        use kurbo::Shape;

        // Arrange
        let (font_data, matched_glyphs) = test_font::build_test_font();
        let variable_font_data = build_variable_font(&font_data, &matched_glyphs);
        let sut = super::subroutinize;

        // Act
        let subroutinized_data = sut(&variable_font_data);

        // Assert
        assert_eq!(b"OTTO", &subroutinized_data[0..4]);

        let before = read_fonts::FontRef::new(&variable_font_data).unwrap();
        let after = read_fonts::FontRef::new(&subroutinized_data).unwrap();
        let before_glyphs = skrifa::MetadataProvider::outline_glyphs(&before);
        let after_glyphs = skrifa::MetadataProvider::outline_glyphs(&after);

        for gid in 0..matched_glyphs.len() as u32 {
            let glyph_id = read_fonts::types::GlyphId::new(gid);
            let before_glyph = before_glyphs.get(glyph_id).unwrap();
            let after_glyph = after_glyphs.get(glyph_id).unwrap();
            for coord in [0.0, 1.0] {
                let before_path = draw_at_coord(&before_glyph, coord);
                let after_path = draw_at_coord(&after_glyph, coord);
                assert_bounding_box_close(before_path.bounding_box(), after_path.bounding_box());
            }
        }
    }

    // シナリオ: `build_static_font` が組み立てた静的 CFF2 フォントは可変軸を
    // 持たず、既定 (座標指定なし) で描画すると、渡した丸めた輪郭のバウンディング
    // ボックスに一致する。これは fvar・VariationStore を持たない CFF2 が
    // skrifa で正しく読めることの確認も兼ねる。
    #[test]
    fn static_font_has_no_axes_and_renders_rounded_outline() {
        use kurbo::Shape;

        // Arrange
        let (font_data, matched_glyphs) = test_font::build_test_font();
        let rounded_glyphs = matched_glyphs
            .iter()
            .map(|(_, rounded)| rounded.clone())
            .collect::<Vec<_>>();
        let sut = super::build_static_font;

        // Act
        let static_font_data = sut(&font_data, &rounded_glyphs);

        // Assert
        assert_eq!(b"OTTO", &static_font_data[0..4]);

        let font = read_fonts::FontRef::new(&static_font_data).unwrap();
        // 可変軸を一切持たないことを確認する。
        let axes = skrifa::MetadataProvider::axes(&font);
        assert_eq!(0, axes.len());

        let outline_glyphs = skrifa::MetadataProvider::outline_glyphs(&font);
        for (gid, rounded) in rounded_glyphs.iter().enumerate() {
            let glyph = outline_glyphs
                .get(read_fonts::types::GlyphId::new(gid as u32))
                .unwrap();
            let path = draw_at_coord(&glyph, 0.0);
            assert_bounding_box_close(rounded.bounding_box(), path.bounding_box());
        }
    }

    /// 指定した正規化座標で `glyph` を描画し、`kurbo::BezPath` として返す。
    fn draw_at_coord(glyph: &skrifa::outline::OutlineGlyph, coord: f32) -> kurbo::BezPath {
        let coords = [skrifa::instance::NormalizedCoord::from_f32(coord)];
        let location = skrifa::instance::LocationRef::new(&coords);
        let mut elements = Vec::<skrifa::outline::pen::PathElement>::new();
        glyph
            .draw(
                (skrifa::instance::Size::unscaled(), location),
                &mut elements,
            )
            .unwrap();
        path_elements_to_bez_path(&elements)
    }

    /// skrifa が出力する `PathElement` の列を `kurbo::BezPath` に変換する。
    ///
    /// テスト内で描画結果を検証しやすくするためだけの変換であり、本体側
    /// (`outline::extract_glyphs`) が内部で行っている変換とは独立している。
    fn path_elements_to_bez_path(elements: &[skrifa::outline::pen::PathElement]) -> kurbo::BezPath {
        use skrifa::outline::pen::PathElement;

        let mut path = kurbo::BezPath::new();
        for element in elements {
            match *element {
                PathElement::MoveTo { x, y } => {
                    path.move_to(kurbo::Point::new(f64::from(x), f64::from(y)));
                }
                PathElement::LineTo { x, y } => {
                    path.line_to(kurbo::Point::new(f64::from(x), f64::from(y)));
                }
                PathElement::QuadTo { cx0, cy0, x, y } => {
                    path.quad_to(
                        kurbo::Point::new(f64::from(cx0), f64::from(cy0)),
                        kurbo::Point::new(f64::from(x), f64::from(y)),
                    );
                }
                PathElement::CurveTo {
                    cx0,
                    cy0,
                    cx1,
                    cy1,
                    x,
                    y,
                } => {
                    path.curve_to(
                        kurbo::Point::new(f64::from(cx0), f64::from(cy0)),
                        kurbo::Point::new(f64::from(cx1), f64::from(cy1)),
                        kurbo::Point::new(f64::from(x), f64::from(y)),
                    );
                }
                PathElement::Close => path.close_path(),
            }
        }
        path
    }

    /// 2 つのバウンディングボックスが、Fixed 16.16 相当の誤差の範囲内で
    /// 一致することを確認する。
    fn assert_bounding_box_close(expected: kurbo::Rect, actual: kurbo::Rect) {
        const TOLERANCE: f64 = 1e-3;
        assert!(
            (expected.x0 - actual.x0).abs() < TOLERANCE
                && (expected.y0 - actual.y0).abs() < TOLERANCE
                && (expected.x1 - actual.x1).abs() < TOLERANCE
                && (expected.y1 - actual.y1).abs() < TOLERANCE,
            "expected {expected:?}, actual {actual:?}"
        );
    }
}
