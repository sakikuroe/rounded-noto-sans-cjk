use std::{env, fs, path};

use read_fonts::{TableProvider, types};
use rounded_noto_sans_cjk::{config, naming};

/// `font_path` のフォントファイルの `name` テーブルから、Windows
/// プラットフォーム・英語 (米国) の著作権表示 (nameID 0) を読み取る。
///
/// `main` が、生成後のフォントの著作権表示 (`naming::FontNaming::copyright`)
/// を組み立てる際、変換元フォントの原著作権表示をそのまま引用するために
/// 使う。ハードコードした文字列を使わずここで都度読み取ることで、
/// `fonts.toml` の `source`・`ascii_source` が指すフォントを差し替えても、
/// 常に実際のフォントに書かれている著作権表示と一致した引用になる。
///
/// # Args
/// - `font_path` - 著作権表示を読み取る対象のフォントファイルのパスである。
///
/// # Returns
/// `font_path` の nameID 0 (Windows・英語 (米国)) の文字列を返す。
///
/// # Panics
/// - `font_path` が読み込めない、または有効な OpenType フォントとして
///   解析できない場合にパニックする。
/// - `font_path` の `name` テーブルに、Windows・英語 (米国) の著作権表示
///   (nameID 0) が存在しない場合にパニックする。
fn read_copyright_notice(font_path: &path::Path) -> String {
    const WINDOWS_PLATFORM_ID: u16 = 3;
    const WINDOWS_ENCODING_ID: u16 = 1;
    const WINDOWS_LANG_ID_EN_US: u16 = 0x0409;

    let font_data = fs::read(font_path)
        .unwrap_or_else(|e| panic!("{} の読み込みに失敗した: {e}", font_path.display()));
    let font = read_fonts::FontRef::new(&font_data).unwrap_or_else(|e| {
        panic!(
            "{} は有効なOpenTypeフォントではない: {e}",
            font_path.display()
        )
    });
    let name = font.name().expect("name テーブルの解析に失敗した");
    let string_data = name.string_data();

    name.name_record()
        .iter()
        .find(|record| {
            record.platform_id() == WINDOWS_PLATFORM_ID
                && record.encoding_id() == WINDOWS_ENCODING_ID
                && record.language_id() == WINDOWS_LANG_ID_EN_US
                && record.name_id() == types::NameId::COPYRIGHT_NOTICE
        })
        .and_then(|record| record.string(string_data).ok())
        .map(|value| value.to_string())
        .unwrap_or_else(|| {
            panic!(
                "{} に著作権表示 (nameID 0) が見つからない",
                font_path.display()
            )
        })
}

/// 本ツール自身の著作権表示である。生成したフォントの著作権表示の先頭に
/// 付け、そのあとに変換元フォントの原著作権表示を引用する。
const PROJECT_COPYRIGHT: &str = "Copyright © 2026 KUROE Saki.";

/// 生成したフォントの著作権表示 (`naming::FontNaming::copyright`) を組み立
/// てる。
///
/// `PROJECT_COPYRIGHT` を先頭に置き、変換元フォント (および ASCII 差し替え
/// 元フォントがあればそれも) の原著作権表示を "Portions" として引用したう
/// えで、本ツールによる改変内容を末尾に追記する。第三者フォントの著作権
/// 表示を保持しつつ改変内容を明示することで、OFL が要求する著作権表示の
/// 保持と、実際には改変版であることの明示を両立させる。
///
/// # Args
/// - `original_copyright` - 変換元フォントの原著作権表示である。
/// - `ascii_copyright` - ASCII 差し替え元フォントの原著作権表示である。
///   `None` の場合、ASCII 差し替えを行わないフォントとして組み立てる。
///
/// # Returns
/// 組み立てた著作権表示の文字列を返す。
fn build_copyright(original_copyright: &str, ascii_copyright: Option<&str>) -> String {
    match ascii_copyright {
        Some(ascii_copyright) => format!(
            "{PROJECT_COPYRIGHT} Portions {original_copyright} Portions {ascii_copyright} \
             Modified by rounding all glyph outlines and, for the ASCII range, replacing them \
             with another typeface's outlines; see \
             https://github.com/sakikuroe/rounded-noto-sans-cjk for details."
        ),
        None => format!(
            "{PROJECT_COPYRIGHT} Portions {original_copyright} Modified by rounding all glyph \
             outlines; see https://github.com/sakikuroe/rounded-noto-sans-cjk for details."
        ),
    }
}

/// TOML 設定ファイルを読み込み、そこに列挙された各フォントを一括で生成する。
///
/// コマンドライン引数の 1 番目 (`argv[1]`) を設定ファイルのパスとして
/// 解釈する。省略した場合はカレントディレクトリの `fonts.toml` を使う。
/// 設定ファイルの書式は `rounded_noto_sans_cjk::config::Config` を参照。
///
/// 各フォントは、`convert_static`・`convert_static_with_ascii` による丸め
/// 変換のあと、`rounded_noto_sans_cjk::naming::rename` で `name` テーブルを
/// `fonts.toml` の `family_name`・`style_name` および `build_copyright` が
/// 組み立てた著作権表示へ書き換える。これにより、配布するフォントが変換元
/// (Noto Sans CJK JP など) と同じファミリー名を名乗り続けることを避ける。
///
/// # Panics
/// - 設定ファイルが読み込めない、または解析できない場合にパニックする。
/// - いずれかのフォントの変換で `rounded_noto_sans_cjk::convert_static` が
///   パニックする条件を満たした場合、同様にパニックする。
/// - 変換元フォント (または ASCII 差し替え元フォント) に著作権表示
///   (nameID 0) が存在しない場合にパニックする。
fn main() {
    let args = env::args().collect::<Vec<String>>();
    let config_path = args.get(1).map(String::as_str).unwrap_or("fonts.toml");

    let config = config::Config::load(path::Path::new(config_path));

    for entry in &config.fonts {
        let source = config.source_path(entry);
        let output = config.output_path(entry);
        let ascii_source = config.ascii_source_path(entry);

        match &ascii_source {
            Some(ascii_source) => {
                let ascii_base_radius = entry
                    .ascii_base_radius
                    .expect("ascii_source を指定した場合は ascii_base_radius も必須である");
                let ascii_inner_radius = entry
                    .ascii_inner_radius
                    .expect("ascii_source を指定した場合は ascii_inner_radius も必須である");
                let ascii_rond = entry
                    .ascii_rond
                    .expect("ascii_source を指定した場合は ascii_rond も必須である");
                println!(
                    "[{}] {} + ASCII({}) -> {} (base_radius={}, inner_radius={}, rond={} / ascii_base_radius={}, ascii_inner_radius={}, ascii_rond={})",
                    entry.name,
                    source.display(),
                    ascii_source.display(),
                    output.display(),
                    entry.base_radius,
                    entry.inner_radius,
                    entry.rond,
                    ascii_base_radius,
                    ascii_inner_radius,
                    ascii_rond
                );
                rounded_noto_sans_cjk::convert_static_with_ascii(
                    &source,
                    ascii_source,
                    &output,
                    entry.base_radius,
                    entry.inner_radius,
                    entry.rond,
                    ascii_base_radius,
                    ascii_inner_radius,
                    ascii_rond,
                );
            }
            None => {
                println!(
                    "[{}] {} -> {} (base_radius={}, inner_radius={}, rond={})",
                    entry.name,
                    source.display(),
                    output.display(),
                    entry.base_radius,
                    entry.inner_radius,
                    entry.rond
                );
                rounded_noto_sans_cjk::convert_static(
                    &source,
                    &output,
                    entry.base_radius,
                    entry.inner_radius,
                    entry.rond,
                );
            }
        }

        // 変換が完了した出力ファイルを読み直し、name テーブルを配布用の
        // 名称・著作権表示へ書き換えたうえで上書きする。convert_static・
        // convert_static_with_ascii のシグネチャ (ファイルへの直接書き込み)
        // を変えずに済むよう、あえて一度書き出したファイルを読み直す
        // 構成にしている。
        println!(
            "  -> renaming to \"{} {}\"",
            entry.family_name, entry.style_name
        );
        let original_copyright = read_copyright_notice(&source);
        let ascii_copyright = ascii_source.as_deref().map(read_copyright_notice);
        let copyright = build_copyright(&original_copyright, ascii_copyright.as_deref());
        let naming = naming::FontNaming {
            family_name: entry.family_name.clone(),
            style_name: entry.style_name.clone(),
            copyright,
            version: config.version.clone(),
        };
        let converted = fs::read(&output).expect("生成したフォントの読み込みに失敗した");
        let renamed = naming::rename(&converted, &naming);
        fs::write(&output, renamed).expect("改名したフォントの書き込みに失敗した");
    }
}
