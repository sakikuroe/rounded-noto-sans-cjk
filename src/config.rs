//! 生成するフォントの一覧と、それぞれの丸みパラメータを TOML ファイルから
//! 読み込む機能を提供するモジュールである。
//!
//! 本リポジトリが既定で配布する「通常」「太字」× Sans・Mono の 4 種類の
//! フォントは、どの元ウェイトを使い、どの丸みパラメータで変換するかを
//! ソースコードに埋め込まず、この TOML 設定ファイル (`fonts.toml`) に
//! 一元化して管理する。

use std::{fs, path};

/// 生成する 1 つのフォントの設定である。
#[derive(Debug, serde::Deserialize)]
pub struct FontEntry {
    /// この設定を人間が識別するための名前である。生成処理には使わない。
    pub name: String,
    /// 変換元となる静的フォントのファイル名であり、`Config::fonts_dir` から
    /// の相対パスとして解釈する。
    pub source: String,
    /// 変換結果を書き出すファイル名であり、`Config::fonts_dir` からの
    /// 相対パスとして解釈する。
    pub output: String,
    /// `round::round_path_matched` にそのまま渡す、凸角用の基準半径で
    /// ある。
    pub base_radius: f64,
    /// `round::round_path_matched` にそのまま渡す、凹角用の固定半径で
    /// ある。
    pub inner_radius: f64,
    /// `round::lerp_matched_paths` にそのまま渡す、丸みの度合いである。
    pub rond: f64,
    /// ASCII 文字 (`U+0020`〜`U+007E`) を別フォントの輪郭に差し替える場合
    /// の、差し替え元フォントのファイル名である。`Config::fonts_dir` から
    /// の相対パスとして解釈する。`None` の場合は ASCII 差し替えを行わず、
    /// `source` の字形をそのまま丸める (`rounded_noto_sans_cjk::convert_static`
    /// を使う)。
    #[serde(default)]
    pub ascii_source: Option<String>,
    /// ASCII 部分に適用する、凸角用の基準半径である。`ascii_source` が
    /// `Some` のときのみ使う。
    #[serde(default)]
    pub ascii_base_radius: Option<f64>,
    /// ASCII 部分に適用する、凹角用の固定半径である。`ascii_source` が
    /// `Some` のときのみ使う。
    #[serde(default)]
    pub ascii_inner_radius: Option<f64>,
    /// ASCII 部分に適用する、丸みの度合いである。`ascii_source` が `Some`
    /// のときのみ使う。
    #[serde(default)]
    pub ascii_rond: Option<f64>,
    /// 生成したフォントの `name` テーブルに設定する、フォントファミリー名
    /// である (`naming::FontNaming::family_name` にそのまま渡す)。変換元
    /// フォントのファミリー名 (例: "Noto Sans CJK JP") とは異なる名称を
    /// 指定する必要がある。詳細は `naming` モジュールのドキュメントを
    /// 参照。
    pub family_name: String,
    /// 生成したフォントの `name` テーブルに設定する、フォントサブファミリー
    /// 名 (`naming::FontNaming::style_name` にそのまま渡す) である。
    /// "Regular"・"Bold" のようなスタイル名を指定する。
    pub style_name: String,
}

/// フォント生成設定ファイル全体である。
#[derive(Debug, serde::Deserialize)]
pub struct Config {
    /// `FontEntry::source`・`FontEntry::output` の基準となるディレクトリ
    /// である。設定ファイル自身の位置ではなく、実行時のカレント
    /// ディレクトリからの相対パスとして解釈する。
    pub fonts_dir: String,
    /// 生成したフォントの `name` テーブルに設定する、バージョン文字列
    /// (`naming::FontNaming::version` にそのまま渡す) である。省略した
    /// 場合は `"Version 1.000"` を既定値として使う。
    #[serde(default = "default_version")]
    pub version: String,
    /// 生成するフォントの一覧である。TOML では `[[font]]` として複数
    /// 定義する。
    #[serde(rename = "font")]
    pub fonts: Vec<FontEntry>,
}

/// `Config::version` の既定値を返す。
fn default_version() -> String {
    "Version 1.000".to_string()
}

impl Config {
    /// `path` にある TOML ファイルを読み込み、`Config` として解析する。
    ///
    /// # Panics
    /// - `path` が読み込めない場合にパニックする。
    /// - ファイルの内容が `Config` として解析できない (必須フィールドの
    ///   欠落や型の不一致など) 場合にパニックする。
    pub fn load(path: &path::Path) -> Self {
        let text = fs::read_to_string(path).expect("設定ファイルの読み込みに失敗した");
        toml::from_str(&text).expect("設定ファイルの解析に失敗した")
    }

    /// `entry` の変換元ファイルへの、実行時のカレントディレクトリからの
    /// 相対パスを返す。
    pub fn source_path(&self, entry: &FontEntry) -> path::PathBuf {
        path::Path::new(&self.fonts_dir).join(&entry.source)
    }

    /// `entry` の変換結果を書き出すファイルへの、実行時のカレント
    /// ディレクトリからの相対パスを返す。
    pub fn output_path(&self, entry: &FontEntry) -> path::PathBuf {
        path::Path::new(&self.fonts_dir).join(&entry.output)
    }

    /// `entry.ascii_source` が `Some` の場合、その ASCII 差し替え元
    /// ファイルへの、実行時のカレントディレクトリからの相対パスを返す。
    pub fn ascii_source_path(&self, entry: &FontEntry) -> Option<path::PathBuf> {
        entry
            .ascii_source
            .as_ref()
            .map(|ascii_source| path::Path::new(&self.fonts_dir).join(ascii_source))
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::path;

    /// ASCII 差し替えの有無それぞれ 1 エントリーずつを含む、テスト用の
    /// 設定ファイルの内容である。
    const CONFIG_TEXT: &str = r#"
fonts_dir = "fonts"

[[font]]
name = "sans-regular"
source = "NotoSansCJKjp-Regular.otf"
output = "sans-regular-out.otf"
base_radius = 40.0
inner_radius = 5.0
rond = 0.85
family_name = "Rounded Test Sans"
style_name = "Regular"

[[font]]
name = "mono-regular"
source = "NotoSansMonoCJKjp-Regular.otf"
output = "mono-regular-out.otf"
base_radius = 45.0
inner_radius = 0.0
rond = 0.85
ascii_source = "SourceCodePro.otf"
ascii_base_radius = 50.0
ascii_inner_radius = 0.0
ascii_rond = 0.75
family_name = "Rounded Test Sans Mono"
style_name = "Regular"
"#;

    /// `CONFIG_TEXT` を一時ファイルへ書き出し、そのファイルを指す
    /// `NamedTempFile` を返す。`Config::load` がファイルパス経由で読み込む
    /// 仕様であるため、文字列のままではなくファイルを経由させる。
    fn write_config_file() -> tempfile::NamedTempFile {
        let mut file = tempfile::NamedTempFile::new().expect("一時ファイルの作成に失敗した");
        file.write_all(CONFIG_TEXT.as_bytes())
            .expect("一時ファイルへの書き込みに失敗した");
        file
    }

    // シナリオ: 設定ファイルを読み込むと、fonts_dir と各エントリーの
    // フィールド (ASCII 差し替えの有無を含む) がそのまま復元される。
    #[test]
    fn load_parses_fonts_dir_and_entries() {
        // Arrange
        let file = write_config_file();
        let sut = super::Config::load;

        // Act
        let config = sut(file.path());

        // Assert
        assert_eq!("fonts", config.fonts_dir);
        assert_eq!(2, config.fonts.len());
        // 1 つ目のエントリーは ASCII 差し替えを持たない。
        let sans = &config.fonts[0];
        assert_eq!("sans-regular", sans.name);
        assert_eq!(40.0, sans.base_radius);
        assert_eq!(5.0, sans.inner_radius);
        assert_eq!(0.85, sans.rond);
        assert_eq!(None, sans.ascii_source);
        assert_eq!("Rounded Test Sans", sans.family_name);
        assert_eq!("Regular", sans.style_name);
        // 2 つ目のエントリーは ASCII 差し替えの 4 フィールドをすべて持つ。
        let mono = &config.fonts[1];
        assert_eq!(Some("SourceCodePro.otf".to_string()), mono.ascii_source);
        assert_eq!(Some(50.0), mono.ascii_base_radius);
        assert_eq!(Some(0.0), mono.ascii_inner_radius);
        assert_eq!(Some(0.75), mono.ascii_rond);
        // version を省略しているため、既定値が使われる。
        assert_eq!("Version 1.000", config.version);
    }

    // シナリオ: `version` を明示的に指定した場合は、その値がそのまま
    // 使われる。
    #[test]
    fn load_uses_explicit_version_when_present() {
        // Arrange
        let mut file = tempfile::NamedTempFile::new().expect("一時ファイルの作成に失敗した");
        let text = format!("version = \"Version 2.000\"\n{CONFIG_TEXT}");
        file.write_all(text.as_bytes())
            .expect("一時ファイルへの書き込みに失敗した");
        let sut = super::Config::load;

        // Act
        let config = sut(file.path());

        // Assert
        assert_eq!("Version 2.000", config.version);
    }

    // シナリオ: 各パス解決メソッドは、fonts_dir とファイル名を結合した
    // 相対パスを返す。ASCII 差し替えを持たないエントリーでは
    // `ascii_source_path` が None になる。
    #[test]
    fn path_helpers_join_fonts_dir_with_file_names() {
        // Arrange
        let file = write_config_file();
        let sut = super::Config::load(file.path());

        // Act
        let source = sut.source_path(&sut.fonts[0]);
        let output = sut.output_path(&sut.fonts[0]);
        let no_ascii = sut.ascii_source_path(&sut.fonts[0]);
        let ascii = sut.ascii_source_path(&sut.fonts[1]);

        // Assert
        assert_eq!(path::Path::new("fonts/NotoSansCJKjp-Regular.otf"), source);
        assert_eq!(path::Path::new("fonts/sans-regular-out.otf"), output);
        assert_eq!(None, no_ascii);
        assert_eq!(
            Some(path::Path::new("fonts/SourceCodePro.otf").to_path_buf()),
            ascii
        );
    }

    // シナリオ: 存在しないパスを渡すと、読み込みに失敗してパニックする。
    #[test]
    #[should_panic]
    fn load_panics_on_missing_file() {
        // Arrange
        let sut = super::Config::load;

        // Act
        sut(path::Path::new("this-file-does-not-exist.toml"));

        // Assert: #[should_panic] により、Act がパニックすることをもって検証する。
    }

    // シナリオ: 必須フィールド (source) が欠けた内容は解析に失敗して
    // パニックする。
    #[test]
    #[should_panic]
    fn load_panics_on_missing_required_field() {
        // Arrange
        let mut file = tempfile::NamedTempFile::new().expect("一時ファイルの作成に失敗した");
        let broken = r#"
fonts_dir = "fonts"

[[font]]
name = "broken"
output = "out.otf"
base_radius = 40.0
inner_radius = 5.0
rond = 0.85
"#;
        file.write_all(broken.as_bytes())
            .expect("一時ファイルへの書き込みに失敗した");
        let sut = super::Config::load;

        // Act
        sut(file.path());

        // Assert: #[should_panic] により、Act がパニックすることをもって検証する。
    }
}
