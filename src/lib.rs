//! 静的な OpenType/CFF フォントを、丸みを OpenType の可変軸として持つ
//! OpenType/CFF2 可変フォントへ変換する機能を提供するクレートである。
//!
//! すべてのグリフの角を丸めた字形を計算し、丸める前を可変軸の最小値、丸めた
//! あとを最大値とする 1 つの新しい可変軸 (軸タグ `ROND`) を追加した CFF2
//! 可変フォントとして書き出す。

pub mod config;
pub mod naming;
pub mod outline;
pub mod round;
pub mod variable_font;

use std::{fs, path};

/// 1 つの静的 CFF フォントファイルを読み込み、すべてのグリフの角を丸めた
/// 字形を計算したうえで、丸みを OpenType の可変軸として持つ CFF2 可変
/// フォントファイルへ変換し、`output_path` に書き出す。
///
/// 本関数は、丸みを `ROND` 可変軸として連続的に調整できる可変フォントを
/// 出力する経路であり、本クレートの冒頭 (`//!`) に掲げた最終目標の中核と
/// なる公開 API として意図的に保持している。一方、配布フォントの生成
/// (`generate`) と CLI (`main`) は、現時点では静的フォントを出力する
/// `convert_static`・`convert_static_with_ascii` の経路のみを使用しており、
/// 本関数を呼び出す手段はライブラリ API 以外にまだ公開していない。CLI へ
/// の公開は、可変フォント出力の品質検証が済んだ段階で別途行う想定である。
///
/// 内部では `input_path` の読み込み、`outline::extract_glyphs` による全グリフの
/// 輪郭抽出、各輪郭への `round::round_path_matched` の適用、
/// `variable_font::build_variable_font` による可変フォントの組み立て、
/// `variable_font::subroutinize` によるサブルーチン化、`output_path` への
/// 書き出しを順に行う。`output_path` に既にファイルが存在する場合は
/// 上書きする。丸め半径には、すべてのグリフ・すべての頂点に対して同じ
/// `base_radius`・`inner_radius` を用いる。
///
/// # Args
/// - `input_path` - 変換元となる、静的な OpenType/CFF フォントファイルの
///   パスである。
/// - `output_path` - 変換結果の OpenType/CFF2 可変フォントを書き出すパス
///   である。
/// - `base_radius` - `round::round_path_matched` にそのまま渡す、凸角用の
///   基準半径である。
/// - `inner_radius` - `round::round_path_matched` にそのまま渡す、凹角用の
///   固定半径である。
///
/// # Panics
/// - `input_path` が読み込めない、または `outline::extract_glyphs` が有効な
///   OpenType フォントとして解析できない場合にパニックする。
/// - `base_radius` または `inner_radius` が不正な値であり、
///   `round::round_path_matched` がエラーを返した場合にパニックする。
/// - `variable_font::subroutinize` が要求する外部コマンド `cffsubr` が
///   見つからない、またはエラー終了した場合にパニックする。
/// - `output_path` へ書き込めない (親ディレクトリが存在しない、書き込み
///   権限がないなど) 場合にパニックする。
pub fn convert(
    input_path: &path::Path,
    output_path: &path::Path,
    base_radius: f64,
    inner_radius: f64,
) {
    let font_data = fs::read(input_path).expect("入力フォントの読み込みに失敗した");
    let glyphs = outline::extract_glyphs(&font_data);
    let matched_glyphs = glyphs
        .iter()
        .map(|glyph| {
            round::round_path_matched(glyph, base_radius, inner_radius)
                .expect("丸め半径のパラメータが不正である")
        })
        .collect::<Vec<_>>();
    let variable_font_data = variable_font::build_variable_font(&font_data, &matched_glyphs);
    let subroutinized_data = variable_font::subroutinize(&variable_font_data);
    fs::write(output_path, subroutinized_data).expect("出力フォントの書き込みに失敗した");
}

/// 1 つの静的 CFF フォントファイルを読み込み、すべてのグリフの角を丸めた
/// 字形を、丸みの度合い `t` で固定した単一の字形として計算したうえで、
/// 可変軸を一切持たない静的な OpenType/CFF2 フォントファイルへ変換し、
/// `output_path` に書き出す。
///
/// `convert` (可変フォントを組み立てたうえで `fonttools varLib.instancer`
/// などで特定の丸み値に固定する 2 段階の経路) と異なり、本関数は
/// `round::round_path_matched` と `round::lerp_matched_paths` を使って
/// 目的の丸み値における座標を直接計算し、`variable_font::build_static_font`
/// で最初から可変軸なしのフォントとして組み立てる。そのため `fvar`・
/// `avar`・item variation store・`blend` オペレータに由来する余分なデータを
/// 一切含まず、同じ見た目のフォントを `convert` の経路より小さく書き出せる。
///
/// 内部では `input_path` の読み込み、`outline::extract_glyphs` による
/// 全グリフの輪郭抽出、各輪郭への `round::round_path_matched` の適用、
/// `round::lerp_matched_paths` による丸み `t` での固定、
/// `variable_font::build_static_font` による静的フォントの組み立て、
/// `variable_font::subroutinize` によるサブルーチン化、`output_path` への
/// 書き出しを順に行う。`output_path` に既にファイルが存在する場合は
/// 上書きする。
///
/// # Args
/// - `input_path` - 変換元となる、静的な OpenType/CFF フォントファイルの
///   パスである。
/// - `output_path` - 変換結果の静的な OpenType/CFF2 フォントを書き出す
///   パスである。
/// - `base_radius` - `round::round_path_matched` にそのまま渡す、凸角用の
///   基準半径である。
/// - `inner_radius` - `round::round_path_matched` にそのまま渡す、凹角用の
///   固定半径である。
/// - `t` - `round::lerp_matched_paths` にそのまま渡す、丸みの度合いである。
///   `0.0` で丸める前の字形に、`1.0` で `base_radius`・`inner_radius` で
///   完全に丸めた字形に一致する。
///
/// # Panics
/// - `input_path` が読み込めない、または `outline::extract_glyphs` が有効な
///   OpenType フォントとして解析できない場合にパニックする。
/// - `base_radius` または `inner_radius` が不正な値であり、
///   `round::round_path_matched` がエラーを返した場合にパニックする。
/// - `variable_font::subroutinize` が要求する外部コマンド `cffsubr` が
///   見つからない、またはエラー終了した場合にパニックする。
/// - `output_path` へ書き込めない (親ディレクトリが存在しない、書き込み
///   権限がないなど) 場合にパニックする。
pub fn convert_static(
    input_path: &path::Path,
    output_path: &path::Path,
    base_radius: f64,
    inner_radius: f64,
    t: f64,
) {
    let font_data = fs::read(input_path).expect("入力フォントの読み込みに失敗した");
    let glyphs = outline::extract_glyphs(&font_data);
    let rounded_glyphs = glyphs
        .iter()
        .map(|glyph| {
            let (original, rounded) = round::round_path_matched(glyph, base_radius, inner_radius)
                .expect("丸め半径のパラメータが不正である");
            round::lerp_matched_paths(&original, &rounded, t)
        })
        .collect::<Vec<_>>();
    let static_font_data = variable_font::build_static_font(&font_data, &rounded_glyphs);
    let subroutinized_data = variable_font::subroutinize(&static_font_data);
    fs::write(output_path, subroutinized_data).expect("出力フォントの書き込みに失敗した");
}

/// `convert_static` と同様に日本語 (非 ASCII) グリフを丸めたうえで、ASCII
/// 文字 (`U+0020`〜`U+007E`) だけは `ascii_input_path` の輪郭に差し替えて
/// 別の丸みパラメータで丸める、静的な OpenType/CFF2 フォントファイルへ
/// 変換し、`output_path` に書き出す。
///
/// ASCII 差し替え時のスケールは、`input_path`・`ascii_input_path` それぞれ
/// の `'A'` の送り幅の比 (x 方向)、`'H'` の cap-height の比 (y 方向) から
/// 自動的に計算する。
///
/// # Args
/// - `input_path` - 日本語部分の変換元となる、静的な OpenType/CFF フォント
///   ファイルのパスである。
/// - `ascii_input_path` - ASCII 部分の差し替え元となる、静的な
///   OpenType/CFF/TrueType フォントファイルのパスである。
/// - `output_path` - 変換結果の静的な OpenType/CFF2 フォントを書き出す
///   パスである。
/// - `base_radius`・`inner_radius`・`t` - 日本語部分 (ASCII 以外の全グリフ)
///   に適用する丸みパラメータである。
/// - `ascii_base_radius`・`ascii_inner_radius`・`ascii_t` - ASCII 部分に
///   適用する丸みパラメータである。
///
/// # Panics
/// - いずれかの入力フォントが読み込めない、または有効な OpenType フォント
///   として解析できない場合にパニックする。
/// - 丸み半径のパラメータが不正な場合にパニックする。
/// - `input_path`・`ascii_input_path` の cmap に `'A'`・`'H'` が存在しない
///   場合にパニックする。
/// - `variable_font::subroutinize` が要求する外部コマンド `cffsubr` が
///   見つからない、またはエラー終了した場合にパニックする。
/// - `output_path` へ書き込めない場合にパニックする。
#[allow(clippy::too_many_arguments)]
pub fn convert_static_with_ascii(
    input_path: &path::Path,
    ascii_input_path: &path::Path,
    output_path: &path::Path,
    base_radius: f64,
    inner_radius: f64,
    t: f64,
    ascii_base_radius: f64,
    ascii_inner_radius: f64,
    ascii_t: f64,
) {
    use kurbo::Shape;
    use skrifa::raw::TableProvider;

    let font_data = fs::read(input_path).expect("入力フォントの読み込みに失敗した");
    let ascii_font_data =
        fs::read(ascii_input_path).expect("ASCII差し替え元フォントの読み込みに失敗した");

    let glyphs = outline::extract_glyphs(&font_data);
    let ascii_glyphs = outline::extract_glyphs(&ascii_font_data);

    // 日本語 (非 ASCII) 部分は base_radius・inner_radius・t で固定する。
    let mut final_glyphs = glyphs
        .iter()
        .map(|glyph| {
            let (original, rounded) = round::round_path_matched(glyph, base_radius, inner_radius)
                .expect("丸め半径のパラメータが不正である");
            round::lerp_matched_paths(&original, &rounded, t)
        })
        .collect::<Vec<kurbo::BezPath>>();

    let font = read_fonts::FontRef::new(&font_data).expect("有効な OpenType フォントではない");
    let ascii_font =
        read_fonts::FontRef::new(&ascii_font_data).expect("有効な OpenType フォントではない");
    let cmap = font.cmap().expect("cmap テーブルの解析に失敗した");
    let ascii_cmap = ascii_font.cmap().expect("cmap テーブルの解析に失敗した");
    let hmtx = font.hmtx().expect("hmtx テーブルの解析に失敗した");
    let ascii_hmtx = ascii_font.hmtx().expect("hmtx テーブルの解析に失敗した");

    // x 方向は 'A' の送り幅の比、y 方向は 'H' の cap-height の比で、別々に
    // スケールする。
    let h_gid: u32 = cmap
        .map_codepoint('H' as u32)
        .expect("'H' が cmap に存在しない")
        .into();
    let ascii_h_gid: u32 = ascii_cmap
        .map_codepoint('H' as u32)
        .expect("'H' が cmap に存在しない")
        .into();
    // cap-height は、それぞれのフォントの 'H' の外接矩形の上端から求める。
    // 本体側は丸め済みの字形 (実際に出力される字形) を、差し替え側は丸める
    // 前の字形を使うが、丸めは角を内側へ削るだけで外接矩形を変えないため、
    // どちらを使っても比は同じになる。
    let cap_height = final_glyphs[h_gid as usize].bounding_box().y1;
    let ascii_cap_height = ascii_glyphs[ascii_h_gid as usize].bounding_box().y1;
    let y_scale = cap_height / ascii_cap_height;

    let a_gid: u32 = cmap
        .map_codepoint('A' as u32)
        .expect("'A' が cmap に存在しない")
        .into();
    let ascii_a_gid: u32 = ascii_cmap
        .map_codepoint('A' as u32)
        .expect("'A' が cmap に存在しない")
        .into();
    let x_scale = hmtx.advance(a_gid.into()).unwrap_or(500) as f64
        / ascii_hmtx.advance(ascii_a_gid.into()).unwrap_or(600) as f64;

    let affine = kurbo::Affine::new([x_scale, 0.0, 0.0, y_scale, 0.0, 0.0]);

    // ASCII 文字だけ、差し替えフォントの輪郭をスケールしたうえで、
    // ASCII 専用の丸みパラメータで丸めて上書きする。
    for cp in 0x20u32..=0x7E {
        let (Some(gid), Some(ascii_gid)) = (cmap.map_codepoint(cp), ascii_cmap.map_codepoint(cp))
        else {
            continue;
        };
        let gid: u32 = gid.into();
        let ascii_gid: u32 = ascii_gid.into();
        let scaled = affine * ascii_glyphs[ascii_gid as usize].clone();
        let (original, rounded) =
            round::round_path_matched(&scaled, ascii_base_radius, ascii_inner_radius)
                .expect("丸め半径のパラメータが不正である");
        final_glyphs[gid as usize] = round::lerp_matched_paths(&original, &rounded, ascii_t);
    }

    let static_font_data = variable_font::build_static_font(&font_data, &final_glyphs);
    let subroutinized_data = variable_font::subroutinize(&static_font_data);
    fs::write(output_path, subroutinized_data).expect("出力フォントの書き込みに失敗した");
}
