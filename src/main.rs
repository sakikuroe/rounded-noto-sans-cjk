/// コマンドライン引数から入力・出力ファイルパスと丸みのパラメータを受け取り、
/// 可変軸を持たない静的な角丸めフォントへの変換を実行する。
///
/// コマンドライン引数の 1 番目 (`argv[1]`) を入力パス、2 番目 (`argv[2]`)
/// を出力パスとして解釈し、`rounded_noto_sans_cjk::convert_static` を呼び
/// 出す。丸みのパラメータ (`base_radius`・`inner_radius`・`t`) は、3〜5
/// 番目の引数として任意で指定でき、省略した場合は既定値を用いる。
///
/// # Panics
/// - コマンドライン引数が入力パス・出力パスの 2 つに満たない場合に
///   パニックする。
/// - 3〜5 番目の引数が指定されているにもかかわらず、有効な数値として
///   解析できない場合にパニックする。
/// - `rounded_noto_sans_cjk::convert_static` がパニックする条件を満たした
///   場合、同様にパニックする。
fn main() {
    // 凸角用の基準半径・凹角用の固定半径・丸みの度合いの既定値。
    // ユーザーが実際に見た目を比較検討したうえで選んだ値である。
    const DEFAULT_BASE_RADIUS: f64 = 40.0;
    const DEFAULT_INNER_RADIUS: f64 = 5.0;
    const DEFAULT_T: f64 = 0.85;

    let args = std::env::args().collect::<Vec<_>>();
    let (input_path, output_path, base_radius, inner_radius, t) = match args.as_slice() {
        [_, input_path, output_path] => (
            input_path,
            output_path,
            DEFAULT_BASE_RADIUS,
            DEFAULT_INNER_RADIUS,
            DEFAULT_T,
        ),
        [_, input_path, output_path, base_radius, inner_radius, t] => (
            input_path,
            output_path,
            base_radius
                .parse::<f64>()
                .expect("base_radius は数値として解析できる必要がある"),
            inner_radius
                .parse::<f64>()
                .expect("inner_radius は数値として解析できる必要がある"),
            t.parse::<f64>()
                .expect("t は数値として解析できる必要がある"),
        ),
        _ => {
            panic!(
                "使い方: rounded-noto-sans-cjk <入力パス> <出力パス> [base_radius inner_radius t]"
            )
        }
    };

    rounded_noto_sans_cjk::convert_static(
        std::path::Path::new(input_path),
        std::path::Path::new(output_path),
        base_radius,
        inner_radius,
        t,
    );
}
