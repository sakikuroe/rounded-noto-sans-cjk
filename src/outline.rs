//! フォントのバイト列から、グリフの輪郭を `kurbo::BezPath` として取り出す
//! 機能を提供するモジュールである。

use skrifa::{
    MetadataProvider, font, instance,
    outline::{self, OutlinePen},
    raw::{TableProvider, types},
};

/// `skrifa::outline::OutlinePen` を実装し、輪郭描画コールバックをそのまま
/// `kurbo::BezPath` の対応するメソッド呼び出しへ転送するペンである。
///
/// skrifa はグリフの輪郭を、単純グリフか複合グリフかを問わず、この
/// トレイトを実装したペンへのコールバック列として描画する。複合グリフの
/// 場合も、コンポーネントの参照先輪郭に変換行列を適用したうえで展開された
/// 座標がそのままコールバックされるため、このペン自身は座標変換を意識する
/// 必要がない。
#[derive(Default)]
struct BezPathPen {
    /// 描画結果を蓄積していく輪郭。
    path: kurbo::BezPath,
}

impl OutlinePen for BezPathPen {
    fn move_to(&mut self, x: f32, y: f32) {
        self.path.move_to((x as f64, y as f64));
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.path.line_to((x as f64, y as f64));
    }

    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        self.path
            .quad_to((cx0 as f64, cy0 as f64), (x as f64, y as f64));
    }

    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        self.path.curve_to(
            (cx0 as f64, cy0 as f64),
            (cx1 as f64, cy1 as f64),
            (x as f64, y as f64),
        );
    }

    fn close(&mut self) {
        self.path.close_path();
    }
}

/// 輪郭の各サブパスで、`ClosePath` 直前の最後の点を `MoveTo` の始点に
/// 溶接する (座標を完全に一致させる) 距離のしきい値 (フォント設計単位)。
///
/// 手動でヒンティング・調整されたフォントでは、本来同一であるべき輪郭の
/// 開始点と終了点が、整数座標への丸め誤差でわずかにずれて記録されている
/// ことがある。このズレを放置すると、`round` モジュールがそれを極端に
/// 短い辺として扱ってしまう。その両端の頂点に通常の丸め半径を適用しようと
/// した結果、丸め弧が極端に押し潰され、輪郭が自己交差気味に尖ってしまう
/// 不具合につながるため、抽出の時点で溶接して取り除く。
///
/// 値の根拠: ASCII 差し替え元の Source Code Pro (Regular/Bold) 全 ASCII
/// グリフについて、この継ぎ目のズレを実測したところ、意図せず生じたと
/// 考えられるズレ (整数座標への丸め誤差) は最大でも 3.162 unit
/// (`sqrt(3^2 + 1^2)`) だった。一方、この継ぎ目を意図的に長い辺として
/// 使っている箇所 (`ClosePath` の暗黙の線分をグリフの実際の輪郭の一部と
/// して使う描画) では、最小でも 24 unit 離れていた。両者の間に位置する
/// この値であれば、意図しないズレのみを確実に吸収しつつ、意図した長い
/// 辺を誤って溶接することはない。
const SEAM_WELD_EPSILON: f64 = 4.0;

/// `PathEl` が描画したあとの現在点 (`ClosePath` を除く) を求める。
///
/// # Args
/// - `el` - 対象の要素であり、`ClosePath` であってはならない。
///
/// # Returns
/// `el` を描画したあとの現在点を返す。
///
/// # Panics
/// - `el` が `ClosePath` である場合にパニックする。
fn path_el_end(el: &kurbo::PathEl) -> kurbo::Point {
    match *el {
        kurbo::PathEl::MoveTo(p)
        | kurbo::PathEl::LineTo(p)
        | kurbo::PathEl::QuadTo(_, p)
        | kurbo::PathEl::CurveTo(_, _, p) => p,
        kurbo::PathEl::ClosePath => unreachable!("ClosePath has no end point of its own"),
    }
}

/// `el` の終点の座標を `new_end` へ書き換える。
///
/// # Args
/// - `el` - 書き換え対象の要素であり、`ClosePath` であってはならない。
/// - `new_end` - 新しく設定する終点の座標である。
///
/// # Panics
/// - `el` が `ClosePath` である場合にパニックする。
fn set_path_el_end(el: &mut kurbo::PathEl, new_end: kurbo::Point) {
    match el {
        kurbo::PathEl::MoveTo(p)
        | kurbo::PathEl::LineTo(p)
        | kurbo::PathEl::QuadTo(_, p)
        | kurbo::PathEl::CurveTo(_, _, p) => *p = new_end,
        kurbo::PathEl::ClosePath => unreachable!("ClosePath has no end point of its own"),
    }
}

/// 輪郭の各サブパスについて、`ClosePath` 直前の最後の点が `MoveTo` の
/// 始点と `SEAM_WELD_EPSILON` 以内の近さにある場合に、座標を始点へ完全に
/// 一致させた (溶接した) 輪郭を返す。
///
/// ズレがしきい値を超える場合は、意図した形状である可能性があるため
/// 変更しない。溶接によって、本来同一の点であるはずの継ぎ目に生じていた
/// 極小セグメントが消え、以降の丸め処理がその頂点を通常どおりの角として
/// 扱えるようになる。
///
/// # Args
/// - `path` - 溶接対象の輪郭である。1 個以上の `MoveTo` から始まる
///   サブパスで構成されている必要がある。
///
/// # Returns
/// 溶接後の輪郭を返す。
fn weld_seams(path: &kurbo::BezPath) -> kurbo::BezPath {
    let mut elements = path.elements().to_vec();

    // 直前に見た MoveTo の添字。ClosePath に出会うたびに、この MoveTo の
    // 始点と直前の要素の終点を比較するために使う。
    let mut current_move_to_index: Option<usize> = None;

    for i in 0..elements.len() {
        match elements[i] {
            kurbo::PathEl::MoveTo(_) => {
                current_move_to_index = Some(i);
            }
            kurbo::PathEl::ClosePath => {
                // ClosePath 単独のサブパス (直前に描画要素がない) は溶接の
                // 対象がないため読み飛ばす。
                let (Some(move_to_index), true) = (current_move_to_index, i > 0) else {
                    continue;
                };
                let kurbo::PathEl::MoveTo(start) = elements[move_to_index] else {
                    unreachable!("current_move_to_index always points at a MoveTo");
                };

                let last_index = i - 1;
                let last_end = path_el_end(&elements[last_index]);
                if last_end != start && last_end.distance(start) <= SEAM_WELD_EPSILON {
                    set_path_el_end(&mut elements[last_index], start);
                }
            }
            _ => {}
        }
    }

    kurbo::BezPath::from_vec(elements)
}

/// フォントのバイト列から、フォントに収録されたすべてのグリフの輪郭を
/// `kurbo::BezPath` として抽出する。
///
/// 各グリフの輪郭は skrifa の輪郭描画 (グリフの各セグメントを訪れるペン) を
/// 使って構築するため、単純グリフだけでなく複合グリフ (コンポーネント参照を
/// 持つグリフ) についても、参照先の輪郭を変換行列で変換したうえで展開し、
/// 単一の `kurbo::BezPath` として返す。半角スペースなど、輪郭を持たないグリフは、
/// 結果から省略されることなく、要素数 0 の空の `kurbo::BezPath` として含まれる。
/// また、各サブパスの開始点と終了点が `SEAM_WELD_EPSILON` 以内でわずかに
/// ずれているだけの場合は、座標を完全に一致させる (溶接する) ため、返される
/// 輪郭は常に厳密に閉じている。
///
/// # Args
/// - `font_data` - 読み込み対象のフォントファイルの内容をそのまま格納した
///   バイト列であり、CFF または CFF2 のアウトラインを含む静的な OpenType
///   フォントである必要がある。
///
/// # Returns
/// フォントの `maxp` テーブルが示すグリフ数と同じ長さを持ち、各要素が
/// グリフ ID の昇順 (0, 1, 2, ...) に対応する `Vec<kurbo::BezPath>` を
/// 返す。同じ `font_data` を渡した場合、呼び出すたびに常に同じ結果を返す
/// (決定的である)。
///
/// # Panics
/// - `font_data` が有効な OpenType フォントとして解析できない場合、または
///   収録されたいずれかのグリフの輪郭抽出に失敗した場合にパニックする。
///
/// # Examples
/// ```no_run
/// let font_data = std::fs::read("NotoSansCJKjp-Regular.otf").unwrap();
/// let glyphs = rounded_noto_sans_cjk::outline::extract_glyphs(&font_data);
///
/// // 同じ入力からは、順序も内容も完全に一致する結果が得られる。
/// assert_eq!(glyphs, rounded_noto_sans_cjk::outline::extract_glyphs(&font_data));
/// ```
pub fn extract_glyphs(font_data: &[u8]) -> Vec<kurbo::BezPath> {
    // フォントのバイト列を解析する。テーブル構造が壊れている場合はここで
    // 失敗するため、仕様どおりパニックさせる。
    let font =
        font::FontRef::new(font_data).unwrap_or_else(|e| panic!("failed to parse font data: {e}"));

    // maxp テーブルが示すグリフ数を、返す Vec の長さの基準として用いる。
    // 輪郭を持たないグリフも省略せずに含めるため、輪郭コレクション側の
    // 走査ではなく、こちらのグリフ数を正として 0 からの連番で処理する。
    let glyph_count = font
        .maxp()
        .unwrap_or_else(|e| panic!("failed to read maxp table: {e}"))
        .num_glyphs();

    // グリフ ID から輪郭を引くためのコレクション。CFF/CFF2 いずれの形式で
    // あっても、このコレクションが差異を吸収してくれるため、以降は形式を
    // 意識せずに扱える。
    let outline_glyphs = font.outline_glyphs();

    // グリフ ID の昇順 (0, 1, 2, ...) に、各グリフの輪郭を kurbo::BezPath へ変換
    // していく。
    (0..glyph_count)
        .map(|gid| {
            let glyph_id = types::GlyphId::from(gid);

            // 指定したグリフ ID の輪郭を取得する。輪郭を持たないグリフ
            // (スペースなど) であっても、コレクション自体には空の輪郭として
            // 登録されているため、ここで取得に失敗するのは異常な状態である。
            let glyph = outline_glyphs
                .get(glyph_id)
                .unwrap_or_else(|| panic!("no outline available for glyph {glyph_id}"));

            // 拡大縮小やヒンティングを行わず、フォント設計単位のままの輪郭を
            // 得るための描画設定。グリフごとに同一の内容 (単位系・原点) で
            // 生成するため、同じ `font_data` から常に同じ結果が得られる。
            let settings = outline::DrawSettings::unhinted(
                instance::Size::unscaled(),
                instance::LocationRef::default(),
            );

            // 単純グリフ・複合グリフを問わず、輪郭の各セグメントがペンへ
            // コールバックされる。複合グリフの場合、参照先の輪郭へ変換行列を
            // 適用したうえで展開された座標がコールバックされるため、ここでは
            // 追加の座標変換を行う必要がない。
            let mut pen = BezPathPen::default();
            glyph
                .draw(settings, &mut pen)
                .unwrap_or_else(|e| panic!("failed to draw glyph {glyph_id}: {e}"));

            // 継ぎ目のわずかなズレに起因する極小セグメントを取り除く。
            weld_seams(&pen.path)
        })
        .collect::<Vec<kurbo::BezPath>>()
}

#[cfg(test)]
mod tests {
    use kurbo::Shape;
    use write_fonts::tables::{glyf, head, hhea, hmtx, loca, maxp};
    use write_fonts::types;

    /// テストで使う三角形の輪郭。
    ///
    /// 直線分のみで構成し、on-curve 点だけを使うため、TrueType のグリフ
    /// フォーマットへ変換しても座標がそのまま保たれ、`extract_glyphs` の
    /// 結果と入力を直接比較できる。
    fn triangle() -> kurbo::BezPath {
        let mut path = kurbo::BezPath::new();
        path.move_to((0.0, 0.0));
        path.line_to((0.0, 700.0));
        path.line_to((600.0, 700.0));
        path.close_path();
        path
    }

    /// テストで使う、2 次ベジェ曲線 (`quad_to`) を含む輪郭。
    ///
    /// 制御点を off-curve 点として明示的に配置しているため曖昧さがなく、
    /// TrueType へ変換しても同じ `quad_to` セグメントとして復元される。
    fn lens() -> kurbo::BezPath {
        let mut path = kurbo::BezPath::new();
        path.move_to((0.0, 0.0));
        path.quad_to((300.0, 700.0), (600.0, 0.0));
        path.quad_to((300.0, -700.0), (0.0, 0.0));
        path.close_path();
        path
    }

    /// テストで使う、開始点と終了点が 1 unit だけずれた (継ぎ目に隙間の
    /// ある) 三角形の輪郭。
    ///
    /// 手動調整されたフォントで実際に見られる、本来同一であるはずの
    /// 継ぎ目にわずかなズレが生じているパターンを模している。ズレが
    /// `SEAM_WELD_EPSILON` 以内であるため、`extract_glyphs` によって
    /// 溶接され、最後の点は `MoveTo` の始点と厳密に一致するはずである。
    fn triangle_with_seam_gap() -> kurbo::BezPath {
        let mut path = kurbo::BezPath::new();
        path.move_to((0.0, 0.0));
        path.line_to((0.0, 700.0));
        path.line_to((600.0, 700.0));
        path.line_to((1.0, 0.0));
        path.close_path();
        path
    }

    /// 単純グリフ 2 個 (`triangle`・`lens`) と、それらをコンポーネントとして
    /// 参照する複合グリフ、輪郭を持たない空グリフ、および継ぎ目に隙間の
    /// ある単純グリフの、合計 5 グリフを収録した最小限の TrueType (`glyf`)
    /// OpenType フォントをその場で組み立てる。
    ///
    /// `extract_glyphs` の仕様は CFF/CFF2 のアウトラインを持つフォントを
    /// 前提としているが、その実体は skrifa の `outline_glyphs()` /
    /// `OutlinePen` という、輪郭のソース形式に依存しない抽象を経由して
    /// 実装されている。そのため、単純グリフ・複合グリフの展開や、空グリフの
    /// 扱いといった `extract_glyphs` 自身のロジックは、`glyf` ベースの
    /// フォントでも同一のコードパスで検証できる。
    ///
    /// 実際に CFF/CFF2 のバイト列をテストで用意しなかった理由は、次のとおり
    /// である。
    /// - `write-fonts` (0.50.0) には、`CFF`/`CFF2` テーブルそのものを組み立
    ///   てる高水準な builder が存在しない (`glyf`/`loca` 用の
    ///   `glyf::GlyfLocaBuilder` のような対応物がない)。低水準な生成コード
    ///   (`generated_cff.rs` 等) はヘッダーや charset、FDSelect といった
    ///   断片を提供するのみで、Top DICT のオフセット計算や Type2
    ///   charstring のエンコードなどを自前で行わない限り、有効な CFF
    ///   テーブルを構築できない。
    /// - skrifa/read-fonts 自身のテストも、CFF/CFF2 の検証には手組みの
    ///   バイト列ではなく `font-test-data` クレートが同梱する実フォントの
    ///   フィクスチャを使っており、本クレートの `Cargo.toml` にはこの
    ///   クレートへの依存が (意図的に) 追加されていない。
    fn build_test_font() -> Vec<u8> {
        // gid 0: 輪郭を持たない空グリフ。半角スペースなどを模している。
        let empty_glyph = glyf::Glyph::Empty;

        // gid 1, gid 2: 単純グリフ。
        let triangle_path = triangle();
        let lens_path = lens();
        let triangle_glyph =
            glyf::Glyph::Simple(glyf::SimpleGlyph::from_bezpath(&triangle_path).unwrap());
        let lens_glyph = glyf::Glyph::Simple(glyf::SimpleGlyph::from_bezpath(&lens_path).unwrap());

        // gid 4: 継ぎ目に 1 unit の隙間がある単純グリフ。
        let seam_gap_path = triangle_with_seam_gap();
        let seam_gap_glyph =
            glyf::Glyph::Simple(glyf::SimpleGlyph::from_bezpath(&seam_gap_path).unwrap());

        // gid 3: gid 1 と gid 2 を、それぞれ異なる平行移動量で参照する
        // 複合グリフ。参照先の輪郭に変換 (ここでは平行移動) を適用して
        // 展開したうえで、1 つの輪郭として返されることを検証するために
        // 用意する。
        let component1 = glyf::Component::new(
            types::GlyphId16::new(1),
            glyf::Anchor::Offset { x: 1000, y: 0 },
            Default::default(),
            glyf::ComponentFlags::default(),
        );
        let component2 = glyf::Component::new(
            types::GlyphId16::new(2),
            glyf::Anchor::Offset { x: -500, y: 200 },
            Default::default(),
            glyf::ComponentFlags::default(),
        );
        let mut composite_glyph =
            glyf::CompositeGlyph::new(component1, triangle_path.bounding_box());
        composite_glyph.add_component(component2, lens_path.bounding_box());
        let composite_glyph = glyf::Glyph::Composite(composite_glyph);

        // glyf/loca テーブルを組み立てる。グリフの並び順がそのままグリフ
        // ID (0, 1, 2, 3, 4) に対応する。
        let mut builder = glyf::GlyfLocaBuilder::new();
        builder
            .add_glyph(&empty_glyph)
            .unwrap()
            .add_glyph(&triangle_glyph)
            .unwrap()
            .add_glyph(&lens_glyph)
            .unwrap()
            .add_glyph(&composite_glyph)
            .unwrap()
            .add_glyph(&seam_gap_glyph)
            .unwrap();
        let (glyf, loca, loca_format) = builder.build();

        const GLYPH_COUNT: u16 = 5;

        // グリフの輪郭抽出のみに関心があるため、head/hhea/hmtx/maxp は
        // `glyf`/`loca` の読み込みに必要な最小限の値のみを設定する。
        let head = head::Head {
            units_per_em: 1000,
            index_to_loc_format: match loca_format {
                loca::LocaFormat::Short => 0,
                loca::LocaFormat::Long => 1,
            },
            ..Default::default()
        };
        let hhea = hhea::Hhea {
            number_of_h_metrics: GLYPH_COUNT,
            ..Default::default()
        };
        let maxp = maxp::Maxp {
            num_glyphs: GLYPH_COUNT,
            ..Default::default()
        };
        let hmtx = hmtx::Hmtx {
            h_metrics: (0..GLYPH_COUNT)
                .map(|_| hmtx::LongMetric {
                    advance: 1000,
                    side_bearing: 0,
                })
                .collect(),
            left_side_bearings: Vec::new(),
        };

        let mut font_builder = write_fonts::FontBuilder::new();
        font_builder
            .add_table(&head)
            .unwrap()
            .add_table(&hhea)
            .unwrap()
            .add_table(&maxp)
            .unwrap()
            .add_table(&hmtx)
            .unwrap()
            .add_table(&glyf)
            .unwrap()
            .add_table(&loca)
            .unwrap();
        font_builder.build()
    }

    // シナリオ: maxp が示すグリフ数と同じ数の輪郭を返す。
    #[test]
    fn returns_a_path_per_glyph_in_maxp_order() {
        // Arrange
        let font_data = build_test_font();
        let sut = super::extract_glyphs;

        // Act
        let glyphs = sut(&font_data);

        // Assert
        assert_eq!(5, glyphs.len());
    }

    // シナリオ: 輪郭を持たないグリフ (gid 0) は、省略されず要素数 0 の
    // 空の kurbo::BezPath として結果に含まれる。
    #[test]
    fn glyph_without_outline_is_kept_as_empty_bezpath() {
        // Arrange
        let font_data = build_test_font();
        let sut = super::extract_glyphs;

        // Act
        let glyphs = sut(&font_data);

        // Assert
        assert_eq!(0, glyphs[0].elements().len());
    }

    // シナリオ: 単純グリフの輪郭は、加工されず入力どおりに抽出される。
    #[test]
    fn simple_glyph_outline_is_extracted_as_is() {
        // Arrange
        let font_data = build_test_font();
        let sut = super::extract_glyphs;

        // Act
        let glyphs = sut(&font_data);

        // Assert
        // 直線のみで構成される単純グリフ (gid 1) は、on-curve 点のみで
        // 曖昧さがないため、入力した輪郭と完全に一致する。
        assert_eq!(triangle(), glyphs[1]);
        // 2 次ベジェ曲線を含む単純グリフ (gid 2) も同様に、入力した輪郭と
        // 完全に一致する。
        assert_eq!(lens(), glyphs[2]);
    }

    // シナリオ: 複合グリフは、参照先の輪郭それぞれにコンポーネントの
    // 変換 (平行移動) を適用したうえで展開され、単一の輪郭として返る。
    #[test]
    fn composite_glyph_outline_is_expanded_with_component_transforms() {
        // Arrange
        let font_data = build_test_font();
        let sut = super::extract_glyphs;

        // Act
        let glyphs = sut(&font_data);

        // Assert
        let mut expected = kurbo::BezPath::new();
        for element in triangle().path_elements(0.1) {
            expected.push(kurbo::Affine::translate((1000.0, 0.0)) * element);
        }
        for element in lens().path_elements(0.1) {
            expected.push(kurbo::Affine::translate((-500.0, 200.0)) * element);
        }
        assert_eq!(expected, glyphs[3]);
    }

    // シナリオ: 開始点と終了点がわずか (SEAM_WELD_EPSILON 以内) にずれた
    // 輪郭は、終了点が開始点へ溶接され、厳密に閉じた輪郭として返る。
    #[test]
    fn seam_gap_within_epsilon_is_welded_to_start_point() {
        // Arrange
        let font_data = build_test_font();
        let sut = super::extract_glyphs;

        // Act
        let glyphs = sut(&font_data);

        // Assert
        // 最後の LineTo の終点 (1.0, 0.0) が、MoveTo の始点 (0.0, 0.0) と
        // 厳密に一致する点まで溶接されているはずである。
        let mut expected = kurbo::BezPath::new();
        expected.move_to((0.0, 0.0));
        expected.line_to((0.0, 700.0));
        expected.line_to((600.0, 700.0));
        expected.line_to((0.0, 0.0));
        expected.close_path();
        assert_eq!(expected, glyphs[4]);
    }

    // シナリオ: `weld_seams` は、しきい値を超えて離れた終了点には手を
    // 加えない。意図した形状である可能性があるためである。
    #[test]
    fn weld_seams_leaves_gap_beyond_epsilon_untouched() {
        // Arrange
        let mut path = kurbo::BezPath::new();
        path.move_to((0.0, 0.0));
        path.line_to((0.0, 700.0));
        path.line_to((600.0, 700.0));
        path.line_to((super::SEAM_WELD_EPSILON + 1.0, 0.0));
        path.close_path();
        let sut = super::weld_seams;

        // Act
        let welded = sut(&path);

        // Assert
        assert_eq!(path, welded);
    }

    // シナリオ: 同じ入力バイト列を渡した場合、呼び出すたびに常に同じ結果を
    // 返す (決定的である)。
    #[test]
    fn extract_glyphs_is_deterministic() {
        // Arrange
        let font_data = build_test_font();
        let sut = super::extract_glyphs;

        // Act
        let first = sut(&font_data);
        let second = sut(&font_data);

        // Assert
        assert_eq!(first, second);
    }

    // シナリオ: 有効な OpenType フォントとして解析できないバイト列を渡すと
    // パニックする。
    #[test]
    #[should_panic]
    fn panics_on_malformed_font_data() {
        // Arrange
        let font_data = [0_u8, 1, 2, 3];
        let sut = super::extract_glyphs;

        // Act
        sut(&font_data);

        // Assert: #[should_panic] により、Act がパニックすることをもって検証する。
    }

    // シナリオ: 空のバイト列を渡すと、有効なフォントとして解析できず
    // パニックする。
    #[test]
    #[should_panic]
    fn panics_on_empty_input() {
        // Arrange
        let font_data: [u8; 0] = [];
        let sut = super::extract_glyphs;

        // Act
        sut(&font_data);

        // Assert: #[should_panic] により、Act がパニックすることをもって検証する。
    }
}
