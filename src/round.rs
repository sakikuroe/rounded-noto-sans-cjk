//! グリフの輪郭を丸めるアルゴリズムを提供するモジュールである。
//!
//! 全体の構成 (CFF2 の `blend` オペレータで丸める前・丸めた後の 2 マスターを
//! 補間する構成) は本プロジェクト独自のものだが、丸め半径の決定式および
//! 隣接する丸め弧が重ならないようにする縮小ロジックの一部は、Resource Han
//! Rounded (<https://github.com/CyanoHao/Resource-Han-Rounded>、
//! `module/round-font.js`、Copyright © 2018–2022 Cyano Hao、MIT License) の
//! 該当ロジックを翻訳・改変して取り込んだものである。詳細な対応関係は
//! `corner_radius`・`compute_cut_plan` それぞれのドキュメントコメントを
//! 参照。

use kurbo::{ParamCurve, ParamCurveArclen, ParamCurveDeriv};

/// `round_path` に渡した引数が不正であることを表すエラーである。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoundError {
    /// `base_radius` が負の値または非有限値 (NaN・無限大) である。
    InvalidBaseRadius,
    /// `inner_radius` が負の値または非有限値 (NaN・無限大) である。
    InvalidInnerRadius,
}

/// 角の前後にある 2 つの接線ベクトルから、その角で接線がどれだけ回転するかを
/// 表す符号付き角度を求める。
///
/// 戻り値の符号は、輪郭が CFF の慣例 (外郭は反時計回り、穴となる内側の輪郭は
/// 時計回り) で表現されていることを前提に定義する。すなわち、正の値は外側に
/// 開く凸角を、負の値は内側にへこむ凹角を表す。`v1`・`v2` の大きさ (ノルム)
/// は結果に影響せず、それぞれの向きのみが結果を決める。
///
/// # Args
/// - `v1` - 角の直前のセグメントが終端で向いていた接線ベクトルである。
/// - `v2` - 角の直後のセグメントが始端で向かう接線ベクトルである。
///
/// # Returns
/// `v1` の向きから `v2` の向きへ回転する符号付き角度を、区間 `(-π, π]` の
/// ラジアンで返す。`v1` または `v2` の少なくとも一方がゼロベクトルである
/// 場合は、回転方向を定義できないため `0.0` を返す。
///
/// # Examples
/// ```
/// use kurbo::Vec2;
/// use rounded_noto_sans_cjk::round::tangent_angle;
///
/// // 反時計回りに 90 度で直角に曲がる、外側に開く凸角の例。
/// let convex = tangent_angle(Vec2::new(1.0, 0.0), Vec2::new(0.0, 1.0));
/// assert!((convex - std::f64::consts::FRAC_PI_2).abs() < 1e-9);
///
/// // 時計回りに 90 度で直角に曲がる、内側にへこむ凹角の例。
/// let concave = tangent_angle(Vec2::new(1.0, 0.0), Vec2::new(0.0, -1.0));
/// assert!((concave + std::f64::consts::FRAC_PI_2).abs() < 1e-9);
///
/// // 直進する (曲がらない) 場合は 0 になる。
/// assert_eq!(0.0, tangent_angle(Vec2::new(1.0, 0.0), Vec2::new(2.0, 0.0)));
/// ```
pub fn tangent_angle(v1: kurbo::Vec2, v2: kurbo::Vec2) -> f64 {
    // ゼロベクトルは向きを持たず回転方向を定義できないため、直進とみなす。
    if v1.hypot2() == 0.0 || v2.hypot2() == 0.0 {
        return 0.0;
    }

    // v1・v2 を複素数とみなしたときの v2 / v1 の偏角が、求める符号付き角度に
    // 一致する。cross は積の虚部 (正弦に比例)、dot は実部 (余弦に比例) に
    // あたるため、atan2(cross, dot) でこの偏角が (-π, π] の範囲で得られる。
    v1.cross(v2).atan2(v1.dot(v2))
}

/// 角度がほぼ 0 とみなし、直進 (丸めなし) として扱う範囲を定義する閾値
/// (ラジアン)。輪郭抽出時の浮動小数点誤差で、本来直線であるべき角が
/// わずかに凹角側へ振れてしまうことがあるため、この程度の余裕を持たせる。
const STRAIGHT_ANGLE_EPSILON: f64 = 1e-6;

/// 角の曲がり具合 (`tangent_angle` が返す角度) と丸めの基準半径から、その角に
/// 実際に適用する丸め半径を決める。
///
/// 角度の絶対値がごく小さく直線とみなせる範囲にあるときは、符号にかかわらず
/// 丸めを行わない (0 を返す)。これは、本来直線であるべき角が、わずかな
/// 浮動小数点誤差によって凹角側に振れてしまい、`inner_radius` が不自然に
/// 適用されることを防ぐためである。この範囲を外れた場合、角度が正 (凸角)
/// であれば `base_radius * (1 - cos(angle))` を、角度が負 (凹角) であれば
/// 自己交差を避けるため常に `inner_radius` を返す。
///
/// 凸角に対するこの `base_radius * (1 - cos(angle))` という式、および凹角に
/// 固定半径 `inner_radius` を用いるという判断は、Resource Han Rounded
/// (<https://github.com/CyanoHao/Resource-Han-Rounded>、
/// `module/round-font.js` の `calculateRadius` 関数、Copyright © 2018–2022
/// Cyano Hao、MIT License) の該当ロジックを翻訳し、改変して取り込んだもので
/// ある。元実装は `wght` 軸の min・max それぞれについて `radius.min`・
/// `radius.max` という異なる基準値でこの式を評価し、2 つの結果 (いずれも
/// 丸め済みの形状) を CFF2 の `blend` で補間することで「細いウェイトでは
/// 丸みが弱く、太いウェイトでは丸みが強い」という挙動を実現している。本
/// プロジェクトは `wght` 軸を持たず丸み専用の `ROND` 軸のみを扱うため、
/// この式は 1 種類の `base_radius` に対してのみ評価し、丸めていない元の
/// 形状 (`round_path_matched` の `original` 側) と、この式で得た半径だけ
/// 丸めた形状 (`rounded` 側) との間で `blend` する構成に置き換えている。
///
/// # Args
/// - `angle` - `tangent_angle` が返す、角の直前と直後の接線がなす符号付き
///   角度 (ラジアン) であり、区間 `(-π, π]` の値を取る。
/// - `base_radius` - 凸角に適用する半径の基準値である。角度が 0 に近いほど
///   0 に近づき、角度が `π` に近づく (鋭く尖った凸角である) ほど、最大で
///   `2 * base_radius` に近づく。
/// - `inner_radius` - 凹角に適用する固定の半径であり、輪郭の自己交差を
///   避けるため、丸める辺の長さより十分小さい値を指定する。
///
/// # Returns
/// その角に適用すべき丸め半径を、`0.0` 以上の値として返す。
///
/// # Examples
/// ```
/// use rounded_noto_sans_cjk::round::corner_radius;
///
/// // 角度がちょうど 0 (完全な直線) の角は丸めない。
/// assert_eq!(0.0, corner_radius(0.0, 20.0, 5.0));
///
/// // 凹角には、角度の大きさによらず常に inner_radius を用いる。
/// assert_eq!(5.0, corner_radius(-1.0, 20.0, 5.0));
/// assert_eq!(5.0, corner_radius(-std::f64::consts::FRAC_PI_2, 20.0, 5.0));
///
/// // 凸角は base_radius * (1 - cos(angle)) になる。
/// let angle = std::f64::consts::FRAC_PI_2;
/// let expected = 20.0 * (1.0 - angle.cos());
/// assert!((expected - corner_radius(angle, 20.0, 5.0)).abs() < 1e-9);
/// ```
pub fn corner_radius(angle: f64, base_radius: f64, inner_radius: f64) -> f64 {
    if angle.abs() < STRAIGHT_ANGLE_EPSILON {
        0.0
    } else if angle > 0.0 {
        base_radius * (1.0 - angle.cos())
    } else {
        inner_radius
    }
}

/// `base_radius`・`inner_radius` が `round_path` の引数として有効であるかを
/// 検証する。
///
/// # Args
/// - `base_radius` - 検証対象の凸角用基準半径である。
/// - `inner_radius` - 検証対象の凹角用固定半径である。
///
/// # Returns
/// 両方とも `0.0` 以上の有限値であれば `Ok(())` を返す。
///
/// # Errors
/// - `base_radius` が負の値または非有限値である場合、
///   `RoundError::InvalidBaseRadius` を返す。
/// - `inner_radius` が負の値または非有限値である場合、
///   `RoundError::InvalidInnerRadius` を返す。
fn validate_radii(base_radius: f64, inner_radius: f64) -> Result<(), RoundError> {
    if !base_radius.is_finite() || base_radius < 0.0 {
        return Err(RoundError::InvalidBaseRadius);
    }
    if !inner_radius.is_finite() || inner_radius < 0.0 {
        return Err(RoundError::InvalidInnerRadius);
    }
    Ok(())
}

/// セグメントの `t` におけるその瞬間の向き (接線ベクトル) を求める。
///
/// 直線の接線は区間全体で一定であるため `t` によらず同じ値になる。曲線の
/// 場合は、`ParamCurveDeriv` で得られる導関数曲線を `t` で評価することで、
/// その点における接線ベクトルが得られる。
///
/// # Args
/// - `seg` - 接線を求める対象のセグメントである。
/// - `t` - 接線を評価するパラメータであり、区間 `[0, 1]` の値を取る。
///
/// # Returns
/// `seg` の `t` における接線ベクトルを返す。大きさ (ノルム) は導関数の
/// 定義どおりの値になり、正規化はされていない。
fn tangent_at(seg: &kurbo::PathSeg, t: f64) -> kurbo::Vec2 {
    match *seg {
        kurbo::PathSeg::Line(line) => line.p1 - line.p0,
        kurbo::PathSeg::Quad(quad) => quad.deriv().eval(t).to_vec2(),
        kurbo::PathSeg::Cubic(cubic) => cubic.deriv().eval(t).to_vec2(),
    }
}

/// `PathEl` が描画したあとの現在点 (`ClosePath` を除く) を求める。
///
/// # Args
/// - `el` - 対象の要素であり、`ClosePath` であってはならない。
///
/// # Returns
/// `el` を描画したあとの現在点を返す。
///
/// # Panics
/// - `el` が `ClosePath` である場合にパニックする。`ClosePath` は現在点を
///   サブパスの開始点に戻すだけであり、この関数の呼び出し元ではその位置を
///   すでに把握しているため、単独では扱わない。
fn path_el_end(el: &kurbo::PathEl) -> kurbo::Point {
    match *el {
        kurbo::PathEl::MoveTo(p)
        | kurbo::PathEl::LineTo(p)
        | kurbo::PathEl::QuadTo(_, p)
        | kurbo::PathEl::CurveTo(_, _, p) => p,
        kurbo::PathEl::ClosePath => unreachable!("ClosePath has no end point of its own"),
    }
}

/// セグメントを、その始点を暗黙の現在点とする `kurbo::PathEl` に変換する。
///
/// # Args
/// - `seg` - 変換対象のセグメントである。
///
/// # Returns
/// `seg` の終点まで描画する `PathEl` を返す。始点の情報は暗黙の現在点として
/// 表現されるため、戻り値には含まれない。
fn seg_to_el(seg: kurbo::PathSeg) -> kurbo::PathEl {
    match seg {
        kurbo::PathSeg::Line(line) => kurbo::PathEl::LineTo(line.p1),
        kurbo::PathSeg::Quad(quad) => kurbo::PathEl::QuadTo(quad.p1, quad.p2),
        kurbo::PathSeg::Cubic(cubic) => kurbo::PathEl::CurveTo(cubic.p1, cubic.p2, cubic.p3),
    }
}

/// セグメントを、形状を変えないまま 3 次ベジエ (`kurbo::CubicBez`) として
/// 表現し直す (次数上げ)。
///
/// `round_path_matched` が、角の置き換え区間において丸める前・丸めた後の
/// 両方を必ず同じ種類 (3 次ベジエ) の操作で表現するために使う。直線・2 次
/// ベジエは、始点・終点を変えないまま数学的に等価な 3 次ベジエへ変換する。
///
/// # Args
/// - `seg` - 変換対象のセグメントである。
///
/// # Returns
/// `seg` と同じ形状を表す `kurbo::CubicBez` を返す。
fn elevate_to_cubic(seg: kurbo::PathSeg) -> kurbo::CubicBez {
    match seg {
        kurbo::PathSeg::Line(line) => kurbo::CubicBez::new(
            line.p0,
            line.p0.lerp(line.p1, 1.0 / 3.0),
            line.p0.lerp(line.p1, 2.0 / 3.0),
            line.p1,
        ),
        kurbo::PathSeg::Quad(quad) => {
            let c1 = quad.p0 + (quad.p1 - quad.p0) * (2.0 / 3.0);
            let c2 = quad.p2 + (quad.p1 - quad.p2) * (2.0 / 3.0);
            kurbo::CubicBez::new(quad.p0, c1, c2, quad.p2)
        }
        kurbo::PathSeg::Cubic(cubic) => cubic,
    }
}

/// 角の置き換えとして挿入する、円弧を近似する 3 次ベジエを組み立てる。
///
/// 円弧を 3 次ベジエで近似する一般的な手法にならい、それぞれの端点から
/// 接線方向に、弦の長さに比例した短い制御点を置く。1/3 という係数は、目視で
/// 滑らかに見える程度の近似精度が得られるよう経験的に選んだものであり、
/// 真円との一致を保証するものではない。
///
/// Resource Han Rounded の `transformContour` (`module/round-font.js`) も、
/// 頂点近傍を接線方向オフセット付きの制御点を持つ 3 次ベジエに置き換える
/// という同種の着想を採る。ただしオフセット量の式は異なり (元実装は
/// `0.5 * 半径`、本実装は `1/3 * 弦長`)、円弧のベジエ近似自体が一般的な
/// 技法であることから、直接の翻訳ではなく独立した実装と判断している。
///
/// # Args
/// - `p_from` - 弧の始点である。
/// - `p_to` - 弧の終点である。
/// - `tangent_from` - `p_from` における接線ベクトルである。
/// - `tangent_to` - `p_to` における接線ベクトルである。
///
/// # Returns
/// 弧を近似する `kurbo::CubicBez` を返す。`p_from`・`tangent_from`・
/// `tangent_to` のいずれかが退化している (弦・接線がゼロベクトルになる)
/// 場合でも、弦の向きや制御点の代替によって破綻なく弧を返す。ただし
/// `p_from == p_to` の場合は、挿入すべき弧に幅がないため `None` を返す。
fn build_round_arc(
    p_from: kurbo::Point,
    p_to: kurbo::Point,
    tangent_from: kurbo::Vec2,
    tangent_to: kurbo::Vec2,
) -> Option<kurbo::CubicBez> {
    const HANDLE_RATIO: f64 = 1.0 / 3.0;
    let chord = p_to - p_from;

    // 弦の向きすら定義できない (p_from と p_to が一致する) 場合は、挿入
    // すべき弧に幅がない。
    if chord.hypot2() == 0.0 {
        return None;
    }

    // 接線がゼロベクトルになる退化したケース (端点で制御点が一致する
    // カスプなど) では、弦の向きで代用して破綻を防ぐ。
    let dir_from = if tangent_from.hypot2() > 0.0 {
        tangent_from
    } else {
        chord
    };
    let dir_to = if tangent_to.hypot2() > 0.0 {
        tangent_to
    } else {
        chord
    };

    let handle_len = chord.hypot() * HANDLE_RATIO;
    let c1 = p_from + dir_from.normalize() * handle_len;
    let c2 = p_to - dir_to.normalize() * handle_len;
    Some(kurbo::CubicBez::new(p_from, c1, c2, p_to))
}

/// 1 つのサブパスに対する丸めの「切り取り計画」である。
///
/// `round_subpath`・`round_subpath_matched` が、丸める前と丸めた後で必ず
/// 同じ丸め判断を共有できるよう、`compute_cut_plan` の計算結果をひとまとめ
/// にして受け渡すための構造体である。3 つの `Vec` はいずれも同じ長さ
/// (サブパスのセグメント数) を持ち、同じ添字が同じセグメントに対応する。
struct CutPlan {
    /// サブパスを構成するセグメント列である。`ClosePath` が暗黙に補う
    /// 閉じの線分も 1 つのセグメントとして含む。
    segs: Vec<kurbo::PathSeg>,

    /// 各セグメントの (始端, 終端) で、丸めのために実際に切り取る弧長で
    /// ある。隣り合う丸め弧が重ならないよう縮小調整を済ませた値を持つ。
    cuts: Vec<(f64, f64)>,

    /// `cuts` の各切り取り位置に対応する、セグメント上のパラメータ
    /// (`t_start`, `t_end`) である。常に `t_start <= t_end` を満たす。
    params: Vec<(f64, f64)>,
}

/// 1 つのサブパスについて、各頂点の丸め半径から、各セグメントの両端に
/// おける実際の切り取り量 (重なり調整後) と、切り取り位置に対応する
/// セグメント上のパラメータ `t` を計算する。
///
/// `round_subpath`・`round_subpath_matched` の両方が、丸める前と丸めた後で
/// 必ず同じ切り取り計画を使うよう、この計算を共通化している。
///
/// # Args
/// - `elements` - 計算対象のサブパスを構成する要素列である。
/// - `base_radius` - `corner_radius` にそのまま渡す凸角用の基準半径である。
/// - `inner_radius` - `corner_radius` にそのまま渡す凹角用の固定半径である。
///
/// # Returns
/// サブパスがセグメントを持たない場合は `None` を返す。持つ場合は、
/// セグメント列・切り取り量・切り取り位置をまとめた `CutPlan` を `Some` で
/// 返す。
fn compute_cut_plan(
    elements: &[kurbo::PathEl],
    base_radius: f64,
    inner_radius: f64,
) -> Option<CutPlan> {
    let segs = kurbo::segments(elements.iter().copied()).collect::<Vec<kurbo::PathSeg>>();
    let n = segs.len();
    if n == 0 {
        return None;
    }

    // 各頂点 (頂点 i は segs[i] の始点であり、直前のセグメント
    // segs[(i + n - 1) % n] の終点でもある) について、前後の接線から
    // 丸め半径を決定する。
    let radii = (0..n)
        .map(|i| {
            let prev = &segs[(i + n - 1) % n];
            let next = &segs[i];
            let angle = tangent_angle(tangent_at(prev, 1.0), tangent_at(next, 0.0));
            corner_radius(angle, base_radius, inner_radius)
        })
        .collect::<Vec<f64>>();

    // 各セグメントの弧長。丸め半径がこの長さに対して大きすぎないかの
    // 判定と、切り取り位置のパラメータ変換に使う。
    let lengths = segs
        .iter()
        .map(|seg| seg.arclen(kurbo::DEFAULT_ACCURACY))
        .collect::<Vec<f64>>();

    // セグメント j の始端 (頂点 j 側) ・終端 (頂点 j+1 側) それぞれで、
    // 丸めのために実際に切り取る弧長を求める。両端の半径の合計がセグメント
    // 長を超える場合、隣り合う丸め弧が重なり合ってしまう。この判定と、
    // 比率を保ったままセグメントの中点でちょうど接するように両方の半径を
    // 比例縮小する処理 (`scale = lengths[j] / sum`) は、Resource Han Rounded
    // (<https://github.com/CyanoHao/Resource-Han-Rounded>、
    // `module/round-font.js` の `calculateRadius` 関数末尾、
    // `if (m0T1 <= m0T2) ... else { m0T1 = m0T1 / (m0T1 + (1 - m0T2)); ... }`
    // の箇所、Copyright © 2018–2022 Cyano Hao、MIT License) にあるロジックを
    // 翻訳し、改変して取り込んだものである。元実装は弧長ではなくセグメント
    // 上のパラメータ `t` の比 (`t1 / (t1 + (1 - t2))`) で縮小率を近似的に
    // 求めているのに対し、本実装は `arclen`/`inv_arclen` を用いて弧長を直接
    // 扱うことで、この近似を行わずに縮小率と切り取り位置を計算している。
    let cuts = (0..n)
        .map(|j| {
            let r_start = radii[j];
            let r_end = radii[(j + 1) % n];
            let sum = r_start + r_end;
            // 縮小が必要なのは、2 つの半径の合計がセグメント長を超え、丸め弧
            // 同士が重なってしまう場合のみである。
            let scale = if sum > lengths[j] {
                lengths[j] / sum
            } else {
                1.0
            };
            (r_start * scale, r_end * scale)
        })
        .collect::<Vec<(f64, f64)>>();

    // 各セグメント上で、切り取り位置に対応するパラメータ t を求める。
    // 直線なら `inv_arclen` が比例計算そのものになり、曲線なら数値的に
    // 解かれる。浮動小数点誤差で t_end が t_start をわずかに下回ることを
    // 避けるため、t_end は t_start 以上に丸める。
    let params = (0..n)
        .map(|j| {
            let (cut_start, cut_end) = cuts[j];
            let t_start = segs[j].inv_arclen(cut_start, kurbo::DEFAULT_ACCURACY);
            let remaining = (lengths[j] - cut_end).max(0.0);
            let t_end = segs[j]
                .inv_arclen(remaining, kurbo::DEFAULT_ACCURACY)
                .max(t_start);
            (t_start, t_end)
        })
        .collect::<Vec<(f64, f64)>>();

    Some(CutPlan { segs, cuts, params })
}

/// `elements` を、`MoveTo` を境界として複数のサブパスに分割する。
///
/// # Args
/// - `elements` - 分割対象の要素列であり、1 個以上の `MoveTo` で始まる
///   サブパスから構成されている必要がある。
///
/// # Returns
/// 各サブパスに対応する `elements` の部分スライスを、出現順に並べた `Vec`
/// を返す。
fn split_into_subpaths(elements: &[kurbo::PathEl]) -> Vec<&[kurbo::PathEl]> {
    let mut subpaths = Vec::new();

    // 現在処理中のサブパスの開始インデックス。`MoveTo` に出会うたびに
    // 直前までのサブパスを確定させ、この値を更新する。
    let mut start = None;

    for (i, el) in elements.iter().enumerate() {
        if matches!(el, kurbo::PathEl::MoveTo(_)) {
            if let Some(s) = start {
                subpaths.push(&elements[s..i]);
            }
            start = Some(i);
        }
    }
    if let Some(s) = start {
        subpaths.push(&elements[s..]);
    }

    subpaths
}

/// 1 つの閉じたサブパスに含まれるすべての頂点を丸めた、新しい要素列を返す。
///
/// アルゴリズムの詳細は `round_path` のドキュメントを参照。
///
/// # Args
/// - `elements` - 丸める前のサブパスを構成する要素列であり、`MoveTo` から
///   始まり、閉じた 1 つながりの輪郭を表す必要がある。
/// - `base_radius` - `corner_radius` にそのまま渡す凸角用の基準半径である。
/// - `inner_radius` - `corner_radius` にそのまま渡す凹角用の固定半径である。
///
/// # Returns
/// 丸めたあとのサブパスを構成する要素列を返す。
fn round_subpath(
    elements: &[kurbo::PathEl],
    base_radius: f64,
    inner_radius: f64,
) -> Vec<kurbo::PathEl> {
    // セグメントを持たないサブパス (輪郭を持たない `MoveTo` のみの場合) は
    // 丸める対象がないため、そのまま返す。
    let Some(CutPlan { segs, cuts, params }) =
        compute_cut_plan(elements, base_radius, inner_radius)
    else {
        return elements.to_vec();
    };
    let n = segs.len();

    // 入力が、最後の頂点から開始点へ戻る線分を明示的に持たず、`ClosePath`
    // による暗黙の閉じに頼っているかどうかを判定する。`kurbo::segments` は
    // この暗黙の線分も 1 つのセグメントとして返すため、丸めがまったく
    // 適用されずセグメントの形状が変化しない場合は、再度明示的な線分として
    // 書き出すのではなく、元と同じく `ClosePath` に閉じを委ねる必要がある
    // (そうしなければ、丸めていないのに入力と異なる要素列になってしまう)。
    let start_point_orig = match elements[0] {
        kurbo::PathEl::MoveTo(p) => p,
        _ => unreachable!("a subpath must begin with MoveTo"),
    };
    let last_explicit_end = elements
        .iter()
        .rev()
        .find(|el| !matches!(el, kurbo::PathEl::ClosePath))
        .map(path_el_end);
    let is_synthetic_last_segment = last_explicit_end != Some(start_point_orig);

    // 新しいサブパスの開始点。頂点 0 が丸められる場合、元の頂点ではなく
    // 丸めた弧の上の点になる。
    let start_point = segs[0].eval(params[0].0);
    let mut new_elements = vec![kurbo::PathEl::MoveTo(start_point)];

    for j in 0..n {
        let (t_start, t_end) = params[j];

        // 丸めのために両端を切り取ったあとに残る中央部分。半径が大きすぎて
        // セグメント全体が丸め弧に飲み込まれた場合は、この部分は存在しない。
        // また、最後のセグメントが入力側では明示されておらず `ClosePath` が
        // 暗黙に補っていたものであり、かつ丸めによって形状が一切変化して
        // いない場合は、あえて明示的な要素を書き出さず `ClosePath` に閉じを
        // 委ねる。これにより、丸め半径が 0 のときに入力と完全に同じ要素列を
        // 返せる。
        let is_untouched_synthetic_close =
            j == n - 1 && is_synthetic_last_segment && t_start == 0.0 && t_end == 1.0;
        if t_end > t_start && !is_untouched_synthetic_close {
            let middle = segs[j].subsegment(t_start..t_end);
            new_elements.push(seg_to_el(middle));
        }

        // segs[j] の終端であり、次のセグメント segs[k] の始端でもある頂点を
        // 丸める弧を挿入する。
        let k = (j + 1) % n;
        let (_, cut_end) = cuts[j];
        let (cut_start_next, _) = cuts[k];

        // 両側の切り取り量がともに 0 であれば、その頂点の丸め半径は 0 と
        // 判定されている (`corner_radius` が 0 を返した) ことを意味し、
        // 頂点をそのまま経由すればよい。弧の挿入は不要である。
        if cut_end > 0.0 || cut_start_next > 0.0 {
            let p_from = segs[j].eval(t_end);
            let p_to = segs[k].eval(params[k].0);
            let tangent_from = tangent_at(&segs[j], t_end);
            let tangent_to = tangent_at(&segs[k], params[k].0);

            // 弦の向きすら定義できない (p_from と p_to が一致する) 場合は、
            // 挿入すべき弧に幅がないため、何もしなくても連続性は保たれる。
            if let Some(arc) = build_round_arc(p_from, p_to, tangent_from, tangent_to) {
                new_elements.push(kurbo::PathEl::CurveTo(arc.p1, arc.p2, arc.p3));
            }
        }
    }

    new_elements.push(kurbo::PathEl::ClosePath);
    new_elements
}

/// 1 つの閉じたサブパスについて、`round_subpath` と全く同じ丸め判断
/// (角ごとの半径・切り取り計画) を使いながら、丸める前の輪郭と丸めた後の
/// 輪郭を、要素の種類と個数が完全に一致する 2 つの要素列として同時に返す。
///
/// `round_subpath` が丸めた結果だけを返すのに対し、本関数は CFF2 の
/// `blend` オペレータで 2 つのマスターを線形補間できるよう、両方の要素列が
/// 同じ位置に同じ種類の操作 (`MoveTo`・`LineTo`・`QuadTo`・`CurveTo`) を
/// 持つことを保証する。角を丸めた区間では、丸める前の側も頂点を経由する
/// 2 つの区間として表現し直し、丸めた側も挿入する弧を 2 つの区間に分割する
/// ことで、常に同じ個数の操作になるようにしている。
///
/// # Args
/// - `elements` - 丸める前のサブパスを構成する要素列であり、`MoveTo` から
///   始まり、閉じた 1 つながりの輪郭を表す必要がある。
/// - `base_radius` - `corner_radius` にそのまま渡す凸角用の基準半径である。
/// - `inner_radius` - `corner_radius` にそのまま渡す凹角用の固定半径である。
///
/// # Returns
/// `(丸める前の要素列, 丸めた後の要素列)` を返す。2 つの要素列は、要素数と
/// 各位置の要素の種類が常に一致する。
fn round_subpath_matched(
    elements: &[kurbo::PathEl],
    base_radius: f64,
    inner_radius: f64,
) -> (Vec<kurbo::PathEl>, Vec<kurbo::PathEl>) {
    let Some(CutPlan { segs, cuts, params }) =
        compute_cut_plan(elements, base_radius, inner_radius)
    else {
        return (elements.to_vec(), elements.to_vec());
    };
    let n = segs.len();

    // 開始点そのものは丸め処理の対象外 (角の置き換え区間の外側) であり、
    // 丸める前・丸めた後で同じ座標になる。
    let start_point = segs[0].eval(params[0].0);
    let mut original_elements = vec![kurbo::PathEl::MoveTo(start_point)];
    let mut rounded_elements = vec![kurbo::PathEl::MoveTo(start_point)];

    for j in 0..n {
        let (t_start, t_end) = params[j];

        // 中央部分は丸めの影響を一切受けないため、丸める前・丸めた後の
        // 両方に、全く同じセグメントをそのまま追加する。
        if t_end > t_start {
            let middle = seg_to_el(segs[j].subsegment(t_start..t_end));
            original_elements.push(middle);
            rounded_elements.push(middle);
        }

        let k = (j + 1) % n;
        let (_, cut_end) = cuts[j];
        let (cut_start_next, _) = cuts[k];

        if cut_end > 0.0 || cut_start_next > 0.0 {
            let p_from = segs[j].eval(t_end);
            let p_to = segs[k].eval(params[k].0);
            let tangent_from = tangent_at(&segs[j], t_end);
            let tangent_to = tangent_at(&segs[k], params[k].0);

            // 丸める前の側: 頂点を経由する、元のセグメントの残り (末尾・
            // 先頭) をそのまま 2 区間として表現する。丸めた側の弧と同じ
            // 個数 (2 個) の 3 次ベジエにするため、直線・2 次ベジエは
            // 次数上げする。
            let tail = elevate_to_cubic(segs[j].subsegment(t_end..1.0));
            let head = elevate_to_cubic(segs[k].subsegment(0.0..params[k].0));
            original_elements.push(kurbo::PathEl::CurveTo(tail.p1, tail.p2, tail.p3));
            original_elements.push(kurbo::PathEl::CurveTo(head.p1, head.p2, head.p3));

            // 丸めた側: 挿入する弧を、対応する 2 区間になるよう中間点で
            // 分割する。弦の向きが定義できない (p_from == p_to) 場合は、
            // 丸める前の側と個数を合わせるため、幅を持たない 2 つの弧
            // (実質的には点) として扱う。
            let (arc1, arc2) = match build_round_arc(p_from, p_to, tangent_from, tangent_to) {
                Some(arc) => arc.subdivide(),
                None => {
                    let degenerate = kurbo::CubicBez::new(p_from, p_from, p_from, p_from);
                    (degenerate, degenerate)
                }
            };
            rounded_elements.push(kurbo::PathEl::CurveTo(arc1.p1, arc1.p2, arc1.p3));
            rounded_elements.push(kurbo::PathEl::CurveTo(arc2.p1, arc2.p2, arc2.p3));
        }
    }

    original_elements.push(kurbo::PathEl::ClosePath);
    rounded_elements.push(kurbo::PathEl::ClosePath);
    (original_elements, rounded_elements)
}

/// `path` に含まれるすべてのサブパス (`MoveTo` で始まり `ClosePath` で
/// 終わる、閉じた 1 つながりの輪郭) それぞれについて、各頂点 (隣り合う
/// 2 つのセグメントが接する角) を丸めた新しい輪郭を返す。
///
/// 各頂点では、その頂点の直前後のセグメントの接線から `tangent_angle` で
/// 角度を求め、`corner_radius` でその角に適用する丸め半径を決定したうえで、
/// 頂点からその半径ぶんだけ手前・奥にある点までの区間を、元のセグメントの
/// 形状を保ったまま短い曲線に置き換える。`corner_radius` が 0 と判定した
/// 頂点は変更しない。輪郭の巻き方向 (時計回り・反時計回りの別) およびサブ
/// パスの本数と順序は、丸めたあとも変化しない。ただし、サブパスの開始点が
/// 丸められる頂点上にある場合、その頂点も他の頂点と同様に丸められるため、
/// 返す輪郭の開始点は元の開始点と同じ座標ではなく、丸めた後の弧の上の点に
/// 変わることがある。
///
/// いずれかの頂点で決定された丸め半径が、その頂点に隣接するセグメントの
/// 長さの半分を超える場合、隣り合う頂点の丸めた弧同士が重なり合わないよう、
/// 隣接する 2 つの半径の比率を保ったまま、セグメントの中点で接するように
/// 両方の半径を比例縮小する。この縮小はセグメントごとに独立して行うため、
/// 1 つの頂点に隣接する 2 つのセグメントで、それぞれ異なる縮小率が適用
/// されることがある (このロジックの由来は `compute_cut_plan` のドキュメント
/// コメントを参照)。
///
/// # Args
/// - `path` - 丸める前の、1 個以上の閉じたサブパスからなるグリフの輪郭で
///   ある。各サブパスを構成するセグメントは、隣接するセグメントと頂点を
///   共有している必要がある (セグメント間に隙間があってはならない)。
/// - `base_radius` - `corner_radius` にそのまま渡す、凸角用の基準半径で
///   あり、`0.0` 以上の有限値でなければならない。
/// - `inner_radius` - `corner_radius` にそのまま渡す、凹角用の固定半径で
///   あり、`0.0` 以上の有限値でなければならない。
///
/// # Returns
/// `base_radius`・`inner_radius` がいずれも有効な値であれば、すべての頂点を
/// 丸めた新しい `kurbo::BezPath` を `Ok` で返す。
///
/// # Errors
/// - `base_radius` が負の値または非有限値 (NaN・無限大) である場合、
///   `RoundError::InvalidBaseRadius` を返す。
/// - `inner_radius` が負の値または非有限値 (NaN・無限大) である場合、
///   `RoundError::InvalidInnerRadius` を返す。
///
/// # Examples
/// ```
/// use kurbo::Shape;
/// use rounded_noto_sans_cjk::round::round_path;
///
/// let square = kurbo::BezPath::from_svg("M0 0 L100 0 L100 100 L0 100 Z").unwrap();
///
/// // 丸め半径を 0 にすると、どの頂点も丸められず、入力と等しい輪郭を返す。
/// assert_eq!(square, round_path(&square, 0.0, 0.0).unwrap());
///
/// // 半径 20 で丸めても、各辺の中央付近は角から十分離れているため、
/// // 外接矩形 (バウンディングボックス) は変化しない。
/// let rounded = round_path(&square, 20.0, 5.0).unwrap();
/// assert_eq!(square.bounding_box(), rounded.bounding_box());
///
/// // 負の半径はエラーになる。
/// assert!(round_path(&square, -1.0, 5.0).is_err());
/// ```
pub fn round_path(
    path: &kurbo::BezPath,
    base_radius: f64,
    inner_radius: f64,
) -> Result<kurbo::BezPath, RoundError> {
    validate_radii(base_radius, inner_radius)?;

    // 各サブパスを独立に丸め、結果を結合して 1 つの輪郭に戻す。サブパスの
    // 本数や順序は、この結合順を元の並びのまま保つことで維持される。
    let rounded_elements = split_into_subpaths(path.elements())
        .into_iter()
        .flat_map(|subpath| round_subpath(subpath, base_radius, inner_radius))
        .collect::<Vec<kurbo::PathEl>>();

    Ok(kurbo::BezPath::from_vec(rounded_elements))
}

/// `path` を丸めた結果を、丸める前の輪郭と組にして返す。
///
/// `round_path`と同じ丸め判断を用いるが、`variable_font::build_variable_font`
/// が CFF2 の `blend` オペレータで 2 つのマスター (丸める前・丸めた後) を
/// 線形補間できるよう、両方の輪郭が要素数・各位置の要素の種類について
/// 完全に一致することを保証する点が異なる。角を丸めた区間では、丸める前の
/// 輪郭側も頂点を経由する 2 区間として表現し直し、丸めた側も挿入する弧を
/// 2 区間に分割することで、この一致を実現している。
///
/// # Args
/// - `path` - 丸める前の、1 個以上の閉じたサブパスからなるグリフの輪郭で
///   ある。制約は `round_path` と同じである。
/// - `base_radius` - `corner_radius` にそのまま渡す、凸角用の基準半径で
///   あり、`0.0` 以上の有限値でなければならない。
/// - `inner_radius` - `corner_radius` にそのまま渡す、凹角用の固定半径で
///   あり、`0.0` 以上の有限値でなければならない。
///
/// # Returns
/// `base_radius`・`inner_radius` がいずれも有効な値であれば、
/// `(丸める前の輪郭, 丸めた後の輪郭)` を `Ok` で返す。2 つの輪郭は、
/// 要素数と各位置の要素の種類が常に一致する。
///
/// # Errors
/// - `base_radius` が負の値または非有限値 (NaN・無限大) である場合、
///   `RoundError::InvalidBaseRadius` を返す。
/// - `inner_radius` が負の値または非有限値 (NaN・無限大) である場合、
///   `RoundError::InvalidInnerRadius` を返す。
pub fn round_path_matched(
    path: &kurbo::BezPath,
    base_radius: f64,
    inner_radius: f64,
) -> Result<(kurbo::BezPath, kurbo::BezPath), RoundError> {
    validate_radii(base_radius, inner_radius)?;

    let mut original_elements = Vec::new();
    let mut rounded_elements = Vec::new();
    for subpath in split_into_subpaths(path.elements()) {
        let (o, r) = round_subpath_matched(subpath, base_radius, inner_radius);
        original_elements.extend(o);
        rounded_elements.extend(r);
    }

    Ok((
        kurbo::BezPath::from_vec(original_elements),
        kurbo::BezPath::from_vec(rounded_elements),
    ))
}

/// `round_path_matched` が返す組 (丸める前の輪郭・丸めた後の輪郭) を、
/// 可変フォントの `blend` オペレータを使わずに、単一の割合 `t` で線形補間
/// した 1 つの輪郭へまとめる。
///
/// `round_path_matched` は、要素数と各位置の要素の種類 (`MoveTo`・
/// `LineTo`・`QuadTo`・`CurveTo`・`ClosePath`) が常に一致する 2 つの輪郭を
/// 返すことを契約としている (`blend` による 2 マスター補間のために設計
/// されたものだが、この「構造が完全に対応している」という性質自体は
/// 可変フォントと無関係に利用できる)。本関数はその対応関係を前提に、
/// 同じ位置にある要素同士を順に読み進めながら、各要素が持つ座標点を
/// `original + (rounded - original) * t` で個別に線形補間する。
/// `ClosePath` は座標を持たないため、補間せずそのまま引き継ぐ。
///
/// `t = 0.0` を渡すと丸める前の輪郭と (誤差なく) 等しくなり、`t = 1.0` を
/// 渡すと丸めた後の輪郭と等しくなる。`t` にその他の値を渡した場合、
/// `variable_font::build_variable_font` が組み立てる可変フォントの
/// `ROND` 軸を同じ値に固定して `fonttools varLib.instancer` で静的化した
/// 場合と、数学的に同一の座標が得られる。両者とも同じ 2 つの端点の間を
/// 同じ比率で線形補間しているだけであり、経路が異なるだけで結果は一致
/// するはずである。
///
/// # Args
/// - `original` - 丸める前の輪郭である。`round_path_matched` が返す組の
///   1 つ目の要素を渡すことを想定する。
/// - `rounded` - `original` を丸めた後の輪郭である。`round_path_matched`
///   が返す組の 2 つ目の要素を渡すことを想定する。`original` と要素数・
///   各位置の要素の種類が一致している必要がある。
/// - `t` - 補間の割合である。`0.0` で `original` に、`1.0` で `rounded` に
///   一致する。範囲外の値 (負の値や 1.0 を超える値) を渡した場合は外挿
///   となり、丸めを誇張した (あるいは反転させた) 輪郭が得られる。
///
/// # Returns
/// 補間後の輪郭を表す `kurbo::BezPath` を返す。
///
/// # Panics
/// - `original` と `rounded` の要素数、または対応する位置の要素の種類が
///   一致しない場合にパニックする (`round_path_matched` の結果をそのまま
///   渡している限り起こらない)。
///
/// # Examples
/// ```
/// use kurbo::Shape;
/// use rounded_noto_sans_cjk::round::{lerp_matched_paths, round_path_matched};
///
/// let square = kurbo::BezPath::from_svg("M0 0 L100 0 L100 100 L0 100 Z").unwrap();
/// let (original, rounded) = round_path_matched(&square, 20.0, 5.0).unwrap();
///
/// // t = 0.0 では丸める前の輪郭と一致する。
/// assert_eq!(original, lerp_matched_paths(&original, &rounded, 0.0));
///
/// // t = 1.0 では丸めた後の輪郭と一致する。
/// assert_eq!(rounded, lerp_matched_paths(&original, &rounded, 1.0));
/// ```
pub fn lerp_matched_paths(
    original: &kurbo::BezPath,
    rounded: &kurbo::BezPath,
    t: f64,
) -> kurbo::BezPath {
    let original_elements = original.elements();
    let rounded_elements = rounded.elements();
    assert_eq!(
        original_elements.len(),
        rounded_elements.len(),
        "元の輪郭と丸めた輪郭の要素数が一致しない"
    );

    let interpolated_elements = original_elements
        .iter()
        .zip(rounded_elements)
        .map(|(o, r)| lerp_path_el(*o, *r, t))
        .collect::<Vec<kurbo::PathEl>>();

    kurbo::BezPath::from_vec(interpolated_elements)
}

/// 対応する位置にある 2 つの `PathEl` を、同じ種類であることを前提に、
/// 保持する座標点ごとに `t` で線形補間する。
///
/// # Args
/// - `original` - 丸める前の要素である。
/// - `rounded` - `original` と同じ種類でなければならない、丸めた後の要素
///   である。
/// - `t` - 補間の割合である。`lerp_matched_paths` と同じ意味を持つ。
///
/// # Returns
/// 座標点を補間した後の `PathEl` を返す。`ClosePath` は座標を持たないため
/// そのまま返す。
///
/// # Panics
/// - `original` と `rounded` の種類が一致しない場合にパニックする。
fn lerp_path_el(original: kurbo::PathEl, rounded: kurbo::PathEl, t: f64) -> kurbo::PathEl {
    match (original, rounded) {
        (kurbo::PathEl::MoveTo(o), kurbo::PathEl::MoveTo(r)) => kurbo::PathEl::MoveTo(o.lerp(r, t)),
        (kurbo::PathEl::LineTo(o), kurbo::PathEl::LineTo(r)) => kurbo::PathEl::LineTo(o.lerp(r, t)),
        (kurbo::PathEl::QuadTo(o1, o2), kurbo::PathEl::QuadTo(r1, r2)) => {
            kurbo::PathEl::QuadTo(o1.lerp(r1, t), o2.lerp(r2, t))
        }
        (kurbo::PathEl::CurveTo(o1, o2, o3), kurbo::PathEl::CurveTo(r1, r2, r3)) => {
            kurbo::PathEl::CurveTo(o1.lerp(r1, t), o2.lerp(r2, t), o3.lerp(r3, t))
        }
        (kurbo::PathEl::ClosePath, kurbo::PathEl::ClosePath) => kurbo::PathEl::ClosePath,
        _ => panic!("元の輪郭と丸めた輪郭で要素の種類が一致しない"),
    }
}

#[cfg(test)]
mod tests {
    use kurbo::{ParamCurveArclen, ParamCurveNearest, Shape};
    use std::panic;

    /// `PathEl` の「種類」だけを比較するための判別子を返す。
    ///
    /// 座標値は無視し、`MoveTo`・`LineTo`・`QuadTo`・`CurveTo`・`ClosePath`
    /// のいずれであるかだけを区別する。`round_path_matched` が返す 2 つの
    /// 輪郭が、要素の種類・個数について一致することを検証するために使う。
    fn el_kind(el: &kurbo::PathEl) -> u8 {
        match el {
            kurbo::PathEl::MoveTo(_) => 0,
            kurbo::PathEl::LineTo(_) => 1,
            kurbo::PathEl::QuadTo(..) => 2,
            kurbo::PathEl::CurveTo(..) => 3,
            kurbo::PathEl::ClosePath => 4,
        }
    }

    // シナリオ: 直進 (0)・凸角 (正)・凹角 (負) のいずれについても、v1・v2 の
    // 向きのみから符号付き角度が計算され、ノルムには依存しない。
    #[test]
    fn tangent_angle_signs_and_scale_invariance() {
        // Arrange
        let sut = super::tangent_angle;

        // Act
        let straight = sut(kurbo::Vec2::new(1.0, 0.0), kurbo::Vec2::new(3.0, 0.0));
        let convex = sut(kurbo::Vec2::new(2.0, 0.0), kurbo::Vec2::new(0.0, 5.0));
        let concave = sut(kurbo::Vec2::new(2.0, 0.0), kurbo::Vec2::new(0.0, -5.0));
        let sharp = sut(kurbo::Vec2::new(1.0, 0.0), kurbo::Vec2::new(-1.0, 0.01));
        let zero_vector = sut(kurbo::Vec2::ZERO, kurbo::Vec2::new(1.0, 0.0));

        // Assert
        // 直進する場合は、向きが同じであればベクトルの長さによらず 0 になる。
        assert_eq!(0.0, straight);
        // 反時計回りに 90 度曲がる凸角は正の角度になる。
        assert!((std::f64::consts::FRAC_PI_2 - convex).abs() < 1e-9);
        // 時計回りに 90 度曲がる凹角は負の角度になる。
        assert!((-std::f64::consts::FRAC_PI_2 - concave).abs() < 1e-9);
        // ほぼ真後ろを向く、鋭く尖った凸角に近い角度も扱える。
        assert!(sharp > std::f64::consts::FRAC_PI_2);
        // 少なくとも一方がゼロベクトルの場合は、回転方向を定義できないため
        // 0 を返す。
        assert_eq!(0.0, zero_vector);
    }

    // シナリオ: 直線とみなせる角度では 0、凹角では常に `inner_radius`、
    // 凸角では `base_radius * (1 - cos(angle))` を返す。
    #[test]
    fn corner_radius_matches_formula_by_angle_sign() {
        // Arrange
        let sut = super::corner_radius;
        let angle = std::f64::consts::FRAC_PI_2;
        let near_pi = std::f64::consts::PI - 1e-3;

        // Act
        let straight = sut(0.0, 20.0, 5.0);
        let straight_with_float_error = sut(-1e-9, 20.0, 5.0);
        let concave = sut(-0.1, 20.0, 5.0);
        let concave_near_pi = sut(-std::f64::consts::PI + 0.01, 20.0, 5.0);
        let convex = sut(angle, 20.0, 5.0);
        let convex_near_pi = sut(near_pi, 20.0, 5.0);
        let non_negative = sut(0.5, 20.0, 5.0);

        // Assert
        // ほぼ 0 とみなせる角度 (浮動小数点誤差を想定した微小な凹角を含む)
        // では、符号によらず丸めない。
        assert_eq!(0.0, straight);
        assert_eq!(0.0, straight_with_float_error);
        // 凹角には、角度の大きさによらず常に inner_radius が使われる。
        assert_eq!(5.0, concave);
        assert_eq!(5.0, concave_near_pi);
        // 凸角は base_radius * (1 - cos(angle)) になり、角度が π に近づく
        // ほど 2 * base_radius に近づく。
        assert!((20.0 * (1.0 - angle.cos()) - convex).abs() < 1e-9);
        assert!(convex_near_pi > 39.9);
        // 半径は常に 0 以上になる。
        assert!(non_negative >= 0.0);
    }

    // シナリオ: 半径 0 で丸めた場合、入力と全く同じ輪郭が返る。
    #[test]
    fn round_path_with_zero_radius_is_identity() {
        // Arrange
        let square = kurbo::BezPath::from_svg("M0 0 L100 0 L100 100 L0 100 Z").unwrap();
        let sut = super::round_path;

        // Act
        let rounded = sut(&square, 0.0, 0.0).unwrap();

        // Assert
        assert_eq!(square, rounded);
    }

    // シナリオ: 負の半径はどちらの引数であってもエラーになり、対応する
    // エラー種別が返る。
    #[test]
    fn round_path_rejects_invalid_radii() {
        // Arrange
        let square = kurbo::BezPath::from_svg("M0 0 L100 0 L100 100 L0 100 Z").unwrap();
        let sut = super::round_path;

        // Act
        let negative_base = sut(&square, -1.0, 5.0);
        let negative_inner = sut(&square, 5.0, -1.0);
        let nan_base = sut(&square, f64::NAN, 5.0);
        let infinite_inner = sut(&square, 5.0, f64::INFINITY);

        // Assert
        assert_eq!(Err(super::RoundError::InvalidBaseRadius), negative_base);
        assert_eq!(Err(super::RoundError::InvalidInnerRadius), negative_inner);
        assert_eq!(Err(super::RoundError::InvalidBaseRadius), nan_base);
        assert_eq!(Err(super::RoundError::InvalidInnerRadius), infinite_inner);
    }

    // シナリオ: 正方形を適度な半径で丸めると、外接矩形は変わらず (角が
    // 内側に削られるだけで外にはみ出さない)、各辺の中点は元の座標のまま
    // 残る。
    #[test]
    fn round_path_square_keeps_edge_midpoints_and_bounding_box() {
        // Arrange
        let square = kurbo::BezPath::from_svg("M0 0 L100 0 L100 100 L0 100 Z").unwrap();
        let sut = super::round_path;

        // Act
        let rounded = sut(&square, 20.0, 5.0).unwrap();

        // Assert
        assert_eq!(square.bounding_box(), rounded.bounding_box());

        // 各辺の中点は角から十分離れているため、丸めた輪郭上にも同じ点が
        // 残っているはずである。
        for midpoint in [
            kurbo::Point::new(50.0, 0.0),
            kurbo::Point::new(100.0, 50.0),
            kurbo::Point::new(50.0, 100.0),
            kurbo::Point::new(0.0, 50.0),
        ] {
            let on_path = rounded
                .segments()
                .any(|seg| seg.nearest(midpoint, 1e-6).distance_sq < 1e-6);
            assert!(on_path, "{midpoint:?} should remain on the rounded path");
        }

        // 元の頂点 (0, 0) の近傍は、丸めによって内側へ削られているはずで
        // あり、輪郭上の最も近い点との距離は 0 より大きくなる。
        let corner = kurbo::Point::new(0.0, 0.0);
        let nearest_dist_sq = rounded
            .segments()
            .map(|seg| seg.nearest(corner, 1e-6).distance_sq)
            .fold(f64::INFINITY, f64::min);
        assert!(nearest_dist_sq > 1.0);
    }

    // シナリオ: 意図的に鋭い凹角を含む L 字型では、凹角に `inner_radius` が
    // 適用され、辺の長さの半分を超えない限り凸角には `base_radius` に
    // 基づく半径が適用される。
    #[test]
    fn round_path_l_shape_handles_concave_corner() {
        // Arrange
        // 反時計回り (外郭) の L 字型で、(50, 50) が内側にへこむ凹角になる。
        let l_shape =
            kurbo::BezPath::from_svg("M0 0 L100 0 L100 50 L50 50 L50 100 L0 100 Z").unwrap();
        let sut = super::round_path;

        // Act
        let rounded = sut(&l_shape, 20.0, 5.0).unwrap();

        // Assert
        // 丸めても輪郭は閉じたままであり、外接矩形は変化しない
        // (すべての角が内側に削られるだけであるため)。
        assert_eq!(l_shape.bounding_box(), rounded.bounding_box());

        // 凹角 (50, 50) の近傍は、半径 5 の内側の弧に置き換わっているため、
        // 元の頂点そのものは輪郭上に残らない。
        let concave_corner = kurbo::Point::new(50.0, 50.0);
        let nearest_dist_sq = rounded
            .segments()
            .map(|seg| seg.nearest(concave_corner, 1e-6).distance_sq)
            .fold(f64::INFINITY, f64::min);
        assert!(nearest_dist_sq > 1e-9);
    }

    // シナリオ: 半径が隣接する辺の長さに対して大きすぎる場合、自動調整に
    // よって隣り合う頂点の丸め弧が重ならないよう半径が縮小される。
    #[test]
    fn round_path_shrinks_overlapping_radii() {
        // Arrange
        // 一辺 20 の正方形に対し、半径 100 は明らかに辺の長さを超えており、
        // 自動調整なしでは隣り合う丸め弧が重なってしまう。
        let small_square = kurbo::BezPath::from_svg("M0 0 L20 0 L20 20 L0 20 Z").unwrap();
        let sut = super::round_path;

        // Act
        let rounded = sut(&small_square, 100.0, 5.0).unwrap();

        // Assert
        // 自動調整により、どの辺についても丸め弧が辺の中点でちょうど
        // 接するように縮小されるため、丸めたあとの輪郭は、辺の中点を
        // 頂点とする (元の正方形よりひとまわり小さい) 菱形に収まる。
        let bbox = rounded.bounding_box();
        assert!(bbox.width() <= 20.0 + 1e-6);
        assert!(bbox.height() <= 20.0 + 1e-6);

        // 縮小が正しく機能していれば、丸めたあとの輪郭が自己交差すること
        // なく、辺の中点 (10, 0) 付近を通るはずである。
        let midpoint = kurbo::Point::new(10.0, 0.0);
        let nearest_dist_sq = rounded
            .segments()
            .map(|seg| seg.nearest(midpoint, 1e-6).distance_sq)
            .fold(f64::INFINITY, f64::min);
        assert!(nearest_dist_sq < 1.0);
    }

    // シナリオ: 複数のサブパスからなる輪郭では、サブパスの本数と順序が
    // 保たれる。
    #[test]
    fn round_path_preserves_subpath_count_and_order() {
        // Arrange
        let two_squares =
            kurbo::BezPath::from_svg("M0 0 L10 0 L10 10 L0 10 Z M20 0 L30 0 L30 10 L20 10 Z")
                .unwrap();
        let sut = super::round_path;
        let count_subpaths = |path: &kurbo::BezPath| {
            path.elements()
                .iter()
                .filter(|el| matches!(el, kurbo::PathEl::MoveTo(_)))
                .count()
        };
        let perimeter =
            |path: &kurbo::BezPath| path.segments().map(|seg| seg.arclen(1e-6)).sum::<f64>();

        // Act
        let rounded = sut(&two_squares, 2.0, 1.0).unwrap();

        // Assert
        assert_eq!(count_subpaths(&two_squares), count_subpaths(&rounded));
        // 各サブパスの丸めたあとの弧長は、元の周長よりも短くなる
        // (角が切り取られて短絡されるため)。
        assert!(perimeter(&rounded) < perimeter(&two_squares));
    }

    // シナリオ: `round_path_matched` が返す 2 つの輪郭は、要素数と各位置の
    // 要素の種類が常に一致する。これは、CFF2 の blend オペレータで 2 つの
    // マスターを補間するために必須の性質である。
    #[test]
    fn round_path_matched_returns_structurally_matching_pair() {
        // Arrange
        let square = kurbo::BezPath::from_svg("M0 0 L100 0 L100 100 L0 100 Z").unwrap();
        let sut = super::round_path_matched;

        // Act
        let (original, rounded) = sut(&square, 20.0, 5.0).unwrap();

        // Assert
        let original_kinds = original.elements().iter().map(el_kind).collect::<Vec<_>>();
        let rounded_kinds = rounded.elements().iter().map(el_kind).collect::<Vec<_>>();
        assert_eq!(original_kinds, rounded_kinds);
        // 角を丸めているため、単なる線分だけでは表現できず、少なくとも
        // 1 つは CurveTo (種類 3) を含むはずである。
        assert!(rounded_kinds.contains(&3));
    }

    // シナリオ: `round_path_matched` の丸めた側の輪郭は、`round_path` を同じ
    // 引数で呼んだ結果と、幾何学的に (外接矩形の一致という形で) 同じ形状を
    // 表す。丸めた弧を 2 区間に分割している点だけが異なり、曲線分割は形状を
    // 変えない操作であるため、両者は同じ弧を描く。
    #[test]
    fn round_path_matched_rounded_component_matches_round_path() {
        // Arrange
        let square = kurbo::BezPath::from_svg("M0 0 L100 0 L100 100 L0 100 Z").unwrap();
        let sut = super::round_path_matched;

        // Act
        let (_, matched_rounded) = sut(&square, 20.0, 5.0).unwrap();
        let plain_rounded = super::round_path(&square, 20.0, 5.0).unwrap();

        // Assert
        assert_eq!(plain_rounded.bounding_box(), matched_rounded.bounding_box());
    }

    // シナリオ: `round_path_matched` の丸める前の輪郭は、要素の構造こそ
    // 入力と異なりうるが (角ごとに区間が分割されるため)、幾何学的には入力と
    // 同じ形状 (同じ外接矩形・同じ周長) を表す。
    #[test]
    fn round_path_matched_original_component_preserves_input_geometry() {
        // Arrange
        let square = kurbo::BezPath::from_svg("M0 0 L100 0 L100 100 L0 100 Z").unwrap();
        let sut = super::round_path_matched;
        let perimeter =
            |path: &kurbo::BezPath| path.segments().map(|seg| seg.arclen(1e-9)).sum::<f64>();

        // Act
        let (matched_original, _) = sut(&square, 20.0, 5.0).unwrap();

        // Assert
        assert_eq!(square.bounding_box(), matched_original.bounding_box());
        assert!((perimeter(&square) - perimeter(&matched_original)).abs() < 1e-6);
    }

    // シナリオ: 負の半径はどちらの引数であってもエラーになり、対応する
    // エラー種別が返る。
    #[test]
    fn round_path_matched_rejects_invalid_radii() {
        // Arrange
        let square = kurbo::BezPath::from_svg("M0 0 L100 0 L100 100 L0 100 Z").unwrap();
        let sut = super::round_path_matched;

        // Act
        let negative_base = sut(&square, -1.0, 5.0);
        let negative_inner = sut(&square, 5.0, -1.0);

        // Assert
        assert_eq!(Err(super::RoundError::InvalidBaseRadius), negative_base);
        assert_eq!(Err(super::RoundError::InvalidInnerRadius), negative_inner);
    }

    // シナリオ: t = 0.0 では丸める前の輪郭 (original) と厳密に一致し、
    // t = 1.0 では丸めた後の輪郭 (rounded) と厳密に一致する。
    #[test]
    fn lerp_matched_paths_at_endpoints_matches_original_and_rounded() {
        // Arrange
        let square = kurbo::BezPath::from_svg("M0 0 L100 0 L100 100 L0 100 Z").unwrap();
        let (original, rounded) = super::round_path_matched(&square, 20.0, 5.0).unwrap();
        let sut = super::lerp_matched_paths;

        // Act
        let at_zero = sut(&original, &rounded, 0.0);
        let at_one = sut(&original, &rounded, 1.0);

        // Assert
        assert_eq!(original, at_zero);
        assert_eq!(rounded, at_one);
    }

    // シナリオ: 中間の割合 (t = 0.5) では、各座標が原点・丸め後の対応する
    // 座標をちょうど 2 等分する位置になる。
    #[test]
    fn lerp_matched_paths_halfway_averages_coordinates() {
        // Arrange
        let square = kurbo::BezPath::from_svg("M0 0 L100 0 L100 100 L0 100 Z").unwrap();
        let (original, rounded) = super::round_path_matched(&square, 20.0, 5.0).unwrap();
        let sut = super::lerp_matched_paths;

        // Act
        let halfway = sut(&original, &rounded, 0.5);

        // Assert: 対応する要素同士の座標が、常に元の座標と丸め後の座標の
        // ちょうど中点になっている。
        for ((o_el, r_el), h_el) in original
            .elements()
            .iter()
            .zip(rounded.elements())
            .zip(halfway.elements())
        {
            match (*o_el, *r_el, *h_el) {
                (
                    kurbo::PathEl::MoveTo(o) | kurbo::PathEl::LineTo(o),
                    kurbo::PathEl::MoveTo(r) | kurbo::PathEl::LineTo(r),
                    kurbo::PathEl::MoveTo(h) | kurbo::PathEl::LineTo(h),
                ) => {
                    assert_eq!(o.midpoint(r), h);
                }
                (kurbo::PathEl::ClosePath, kurbo::PathEl::ClosePath, kurbo::PathEl::ClosePath) => {}
                (
                    kurbo::PathEl::CurveTo(o1, o2, o3),
                    kurbo::PathEl::CurveTo(r1, r2, r3),
                    kurbo::PathEl::CurveTo(h1, h2, h3),
                ) => {
                    assert_eq!(o1.midpoint(r1), h1);
                    assert_eq!(o2.midpoint(r2), h2);
                    assert_eq!(o3.midpoint(r3), h3);
                }
                _ => panic!("要素の種類が一致しない組み合わせが現れた"),
            }
        }
    }

    // シナリオ: 要素数が一致しない 2 つの輪郭を渡すとパニックする。
    #[test]
    fn lerp_matched_paths_panics_on_length_mismatch() {
        // Arrange
        let original = kurbo::BezPath::from_svg("M0 0 L100 0 Z").unwrap();
        let rounded = kurbo::BezPath::from_svg("M0 0 L100 0 L100 100 Z").unwrap();
        let sut = super::lerp_matched_paths;

        // Act
        let result = panic::catch_unwind(|| sut(&original, &rounded, 0.5));

        // Assert
        assert!(result.is_err());
    }
}
