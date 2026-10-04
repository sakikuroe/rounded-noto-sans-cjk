//! 生成したフォントの `name` テーブルを、配布用の名称・著作権表示・商標に
//! 関する断り書きへ書き換える機能を提供するモジュールである。
//!
//! `variable_font::build_static_font` 等は、輪郭以外のメタデータを変換元の
//! フォントからそのまま引き継ぐ。そのままでは、丸めただけの改変版が
//! "Noto Sans CJK JP" のような変換元と同一のファミリー名を名乗り続けて
//! しまい、フォント名に対する Google の商標主張 (変換元フォント自身の
//! nameID 7 に "Noto is a trademark of Google Inc." と明記されている) と
//! 衝突しかねない。本モジュールは、この問題に対応するため、ファミリー名・
//! 著作権表示など一部の nameID だけを新しい値へ差し替え、デザイナー名や
//! ライセンス説明など残すべき他の nameID はそのまま引き継ぐ。

use read_fonts::TableProvider;
use write_fonts::from_obj::FromTableRef;
use write_fonts::tables::{head, name, os2};
use write_fonts::types;

/// name テーブルへ書き込む際に用いる、Windows プラットフォームの
/// プラットフォーム ID・エンコーディング ID・言語 ID (英語 (米国)) である。
/// 変換元フォントの英語名レコードも同じ組み合わせで格納されているため、
/// これに合わせることで、既存レコードとの整合性を保つ。
const WINDOWS_PLATFORM_ID: u16 = 3;
const WINDOWS_ENCODING_ID: u16 = 1;
const WINDOWS_LANG_ID_EN_US: u16 = 0x0409;

/// 生成するフォントの `name` テーブルに設定する、名称・著作権表示である。
pub struct FontNaming {
    /// フォントファミリー名 (nameID 1・16) である。変換元フォントの
    /// ファミリー名 (例: "Noto Sans CJK JP") とは異なる、本ツールの改変版
    /// であることが分かる名称を指定する必要がある。
    pub family_name: String,

    /// フォントサブファミリー名 (nameID 2・17) であり、"Regular"・"Bold"
    /// のようなスタイルを表す。
    pub style_name: String,

    /// 著作権表示 (nameID 0) である。変換元フォント (および ASCII 差し替え
    /// 元フォント) の原著作権表示を保持しつつ、本ツールによる改変である旨を
    /// 追記した文字列を渡すことを想定する。
    pub copyright: String,

    /// バージョン文字列 (nameID 5) であり、"Version 1.000" のような書式を
    /// 想定する。
    pub version: String,
    /// 入力のウェイトと独立した出力ウェイト。未指定の Regular/Bold は400/700。
    pub weight_class: Option<u16>,
}

/// 変換元フォントの商標名 (Noto・Source) について、本ツールが公式版や
/// 後継版ではないことを明示する断り書きである。nameID 7 (Trademark) に
/// 設定する。
///
/// Noto Sans CJK JP・Noto Sans Mono CJK JP の nameID 7 には "Noto is a
/// trademark of Google Inc." と明記されているが、OFL 上の Reserved Font Name
/// 宣言はないため、改変版のファミリー名に "Noto" を含めることは許容される
/// (Nerd Fonts などの他の改変フォントも同様の扱いをしている)。一方、
/// Source Code Pro の nameID 0 には "Reserved Font Name 'Source'" という
/// 予約名宣言があるため、"Source" はファミリー名に含めない。いずれに
/// ついても、公式版と誤認されないよう無関係であることを明示しておく。
const TRADEMARK_DISCLAIMER: &str = "This is an unofficial, independently modified \
    derivative and is not produced, endorsed, or affiliated with Google or Adobe. \
    \"Noto\" is a trademark of Google Inc. \"Source\" is a trademark of Adobe.";

/// `font_data` の `name` テーブルのうち、`naming` が指定する項目だけを
/// 新しい値へ差し替えたフォントのバイト列を返す。
///
/// 差し替えるのは、ファミリー名・サブファミリー名・ユニーク識別子・
/// フルネーム・バージョン・PostScript 名・商標表示 (nameID 0・1・2・3・4・
/// 5・6・7、および存在すれば typographic 版の 16・17) の Windows
/// プラットフォーム (`WINDOWS_PLATFORM_ID`・`WINDOWS_ENCODING_ID`・
/// `WINDOWS_LANG_ID_EN_US`) のレコードのみである。デザイナー名・
/// ライセンス説明・各種 URL (nameID 8〜14 など) を含む、それ以外の
/// nameID のレコードはすべて元のフォントからそのまま引き継ぐ。これにより、
/// 元のフォントの設計者へのクレジットや、OFL のライセンス説明といった
/// 保持すべき情報を失わずに、名称と著作権表示だけを更新できる。
///
/// # Args
/// - `font_data` - 名称を書き換える対象のフォントのバイト列であり、有効な
///   OpenType フォントである必要がある。
/// - `naming` - 新しく設定する名称・著作権表示である。
///
/// # Returns
/// `name` テーブルを差し替えたフォントのバイト列を返す。`name` 以外の
/// テーブルは原則引き継ぐが、head・OS/2 の版とスタイルも更新する。
/// 静的フォントでは入力由来の STAT を除去し、旧署名 DSIG も除去する。
///
/// # Panics
/// - `font_data` が有効な OpenType フォントとして解析できない場合、または
///   `name` テーブルの解析に失敗した場合にパニックする。
pub fn rename(font_data: &[u8], naming: &FontNaming) -> Vec<u8> {
    let font =
        read_fonts::FontRef::new(font_data).expect("font_data は有効な OpenType フォントではない");
    let original_name = font.name().expect("name テーブルの解析に失敗した");
    let string_data = original_name.string_data();

    // 新しく設定する値を並べる。同じファミリー・サブファミリーの組は
    // typographic 版 (16・17) にも重複させておくことで、4 スタイル以上の
    // ファミリーであっても OS 側が正しくグループ化できるようにする。
    let full_name = format!("{} {}", naming.family_name, naming.style_name);
    let postscript_name = format!(
        "{}-{}",
        naming.family_name.replace(' ', ""),
        naming.style_name.replace(' ', "")
    );
    let unique_id = format!("{};rounded-noto-sans-cjk;{postscript_name}", naming.version);
    let overrides = [
        (types::NameId::COPYRIGHT_NOTICE, naming.copyright.clone()),
        (types::NameId::FAMILY_NAME, naming.family_name.clone()),
        (types::NameId::SUBFAMILY_NAME, naming.style_name.clone()),
        (types::NameId::UNIQUE_ID, unique_id),
        (types::NameId::FULL_NAME, full_name),
        (types::NameId::VERSION_STRING, naming.version.clone()),
        (types::NameId::POSTSCRIPT_NAME, postscript_name),
        (types::NameId::TRADEMARK, TRADEMARK_DISCLAIMER.to_string()),
        (
            types::NameId::TYPOGRAPHIC_FAMILY_NAME,
            naming.family_name.clone(),
        ),
        (
            types::NameId::TYPOGRAPHIC_SUBFAMILY_NAME,
            naming.style_name.clone(),
        ),
    ];

    // 上書き対象の nameID のみを集めておき、元のレコードのうち上書き対象で
    // ない (デザイナー名・URL・ライセンス説明などの) レコードだけを、
    // あとで引き継げるようにする。
    let overridden_ids = overrides
        .iter()
        .map(|(id, _)| *id)
        .collect::<Vec<types::NameId>>();

    let mut records = Vec::new();
    for record in original_name.name_record() {
        let is_windows_en_us = record.platform_id() == WINDOWS_PLATFORM_ID
            && record.encoding_id() == WINDOWS_ENCODING_ID
            && record.language_id() == WINDOWS_LANG_ID_EN_US;
        // 上書き対象の nameID、または英語 (米国) 以外のプラットフォームの
        // レコードは、ここでは引き継がない。前者はこのあと新しい値で
        // 追加し、後者は本モジュールが対応しない言語のレコードであり、
        // 内容の整合性を保証できないため除外する。
        if !is_windows_en_us || overridden_ids.contains(&record.name_id()) {
            continue;
        }
        let Ok(value) = record.string(string_data) else {
            continue;
        };
        records.push(name::NameRecord::new(
            WINDOWS_PLATFORM_ID,
            WINDOWS_ENCODING_ID,
            WINDOWS_LANG_ID_EN_US,
            record.name_id(),
            value.to_string().into(),
        ));
    }
    for (name_id, value) in overrides {
        records.push(name::NameRecord::new(
            WINDOWS_PLATFORM_ID,
            WINDOWS_ENCODING_ID,
            WINDOWS_LANG_ID_EN_US,
            name_id,
            value.into(),
        ));
    }
    // name テーブルの仕様上、name_record 配列は
    // (platformID, encodingID, languageID, nameID) の昇順でソートされて
    // いる必要がある。ここではプラットフォーム・エンコーディング・言語が
    // すべて共通なので、実質的には nameID だけで比較すればよい。
    records.sort_by_key(|r| r.name_id);

    let mut builder = write_fonts::FontBuilder::new();
    builder
        .add_table(&name::Name::new(records))
        .expect("name テーブルの組み立てに失敗した");

    let mut output_head = head::Head::from_table_ref(&font.head().expect("Missing head table"));
    output_head.font_revision = parse_version(&naming.version)
        .expect("Version must be 'Version <finite nonnegative number below 32768>'");
    let mut output_os2 = os2::Os2::from_table_ref(&font.os2().expect("Missing OS/2 table"));
    let bold = naming.style_name.contains("Bold");
    let italic = naming.style_name.contains("Italic") || naming.style_name.contains("Oblique");
    let default_weight = match naming.style_name.as_str() {
        "Regular" | "Italic" => 400,
        "Bold" | "Bold Italic" => 700,
        _ => output_os2.us_weight_class,
    };
    output_os2.us_weight_class = naming.weight_class.unwrap_or(default_weight);
    assert!(
        (1..=1000).contains(&output_os2.us_weight_class),
        "Weight class must be between 1 and 1000"
    );
    // name のスタイルと OS/2・head の選択情報を同時に更新する。
    output_os2.fs_selection.remove(
        os2::SelectionFlags::BOLD | os2::SelectionFlags::ITALIC | os2::SelectionFlags::REGULAR,
    );
    output_head
        .mac_style
        .remove(head::MacStyle::BOLD | head::MacStyle::ITALIC);
    if bold {
        output_os2.fs_selection.insert(os2::SelectionFlags::BOLD);
        output_head.mac_style.insert(head::MacStyle::BOLD);
    }
    if italic {
        output_os2.fs_selection.insert(os2::SelectionFlags::ITALIC);
        output_head.mac_style.insert(head::MacStyle::ITALIC);
    }
    if naming.style_name == "Regular" {
        output_os2.fs_selection.insert(os2::SelectionFlags::REGULAR);
    }
    builder
        .add_table(&output_head)
        .expect("Failed to write head");
    builder
        .add_table(&output_os2)
        .expect("Failed to write OS/2");

    // 更新済みの head・OS/2 と、入力のスタイル参照・旧署名を除いて引き継ぐ。
    for table_record in font.table_directory().table_records() {
        let tag = table_record.tag();
        if [b"name", b"head", b"OS/2", b"DSIG"]
            .iter()
            .any(|bytes| tag == types::Tag::new(bytes))
            || (font.fvar().is_err() && tag == types::Tag::new(b"STAT"))
        {
            continue;
        }
        if let Some(data) = font.table_data(tag) {
            builder.add_raw(tag, data.as_bytes().to_vec());
        }
    }

    builder.build()
}

/// nameID 5 の版の数値部分を head.fontRevision の16.16値へ変換する。
///
/// # Args
/// - `version` - `Version 1.001` のような版文字列。数値後の説明は許可する。
///
/// # Returns
/// 有限で0以上32768未満なら値を返し、不正な書式なら `None` を返す。
pub fn parse_version(version: &str) -> Option<types::Fixed> {
    let value = version
        .strip_prefix("Version ")?
        .split_whitespace()
        .next()?
        .parse::<f64>()
        .ok()?;
    (value.is_finite() && (0.0..32768.0).contains(&value)).then(|| types::Fixed::from_f64(value))
}

#[cfg(test)]
mod tests {
    use write_fonts::types;

    /// テストで書き換える対象の、最小限の name テーブルを持つフォントの
    /// バイト列を組み立てる。
    ///
    /// `src/variable_font/tests/test_font.rs` と同様に、元のファミリー名・
    /// デザイナー名・著作権表示を持つ最小限のフォントを用意し、`rename`
    /// による書き換えの前後を比較できるようにする。
    fn build_source_font() -> Vec<u8> {
        use write_fonts::tables::{head, hhea, hmtx, maxp, name, os2, post};
        use write_fonts::types;

        let mut builder = write_fonts::FontBuilder::new();

        let head = head::Head {
            units_per_em: 1000,
            ..Default::default()
        };
        builder
            .add_table(&head)
            .expect("head テーブルの組み立てに失敗した");
        builder
            .add_table(&hhea::Hhea {
                number_of_h_metrics: 1,
                ..Default::default()
            })
            .expect("hhea テーブルの組み立てに失敗した");
        builder
            .add_table(&maxp::Maxp::new(1))
            .expect("maxp テーブルの組み立てに失敗した");
        builder
            .add_table(&hmtx::Hmtx {
                h_metrics: vec![hmtx::LongMetric {
                    advance: 1000,
                    side_bearing: 0,
                }],
                left_side_bearings: Vec::new(),
            })
            .expect("hmtx テーブルの組み立てに失敗した");

        // 上書き対象 (ファミリー名・著作権表示) と、引き継がれるべき対象
        // (デザイナー名) の両方を含む、元の name テーブルを用意する。
        let name_records = [
            (
                types::NameId::COPYRIGHT_NOTICE,
                "© Original Copyright Holder.",
            ),
            (types::NameId::FAMILY_NAME, "Original Font Name"),
            (types::NameId::SUBFAMILY_NAME, "Regular"),
            (types::NameId::FULL_NAME, "Original Font Name"),
            (types::NameId::POSTSCRIPT_NAME, "OriginalFontName-Regular"),
            (types::NameId::DESIGNER, "Original Designer"),
            (
                types::NameId::LICENSE_DESCRIPTION,
                "Licensed under the SIL OFL 1.1",
            ),
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

        builder
            .add_table(&os2::Os2::default())
            .expect("OS/2 テーブルの組み立てに失敗した");
        builder
            .add_table(&post::Post::new_v2([".notdef"]))
            .expect("post テーブルの組み立てに失敗した");

        builder.build()
    }

    /// テスト用の `FontNaming` を返す。
    fn test_naming() -> super::FontNaming {
        super::FontNaming {
            family_name: "Rounded Test Sans".to_string(),
            style_name: "Regular".to_string(),
            copyright: "Copyright © 2026 Test Author. Portions © Original Copyright Holder."
                .to_string(),
            version: "Version 1.000".to_string(),
            weight_class: None,
        }
    }

    /// フォントの英語 (米国) 名レコードから、指定した nameID の文字列を
    /// 取り出す。
    fn read_name(font_data: &[u8], name_id: types::NameId) -> Option<String> {
        use read_fonts::TableProvider;

        let font = read_fonts::FontRef::new(font_data).unwrap();
        let name = font.name().unwrap();
        let string_data = name.string_data();
        name.name_record()
            .iter()
            .find(|record| {
                record.platform_id() == 3
                    && record.encoding_id() == 1
                    && record.language_id() == 0x0409
                    && record.name_id() == name_id
            })
            .and_then(|record| record.string(string_data).ok())
            .map(|s| s.to_string())
    }

    // シナリオ: ファミリー名・著作権表示は、`FontNaming` で指定した新しい
    // 値に置き換わる。
    #[test]
    fn rename_overrides_family_name_and_copyright() {
        // Arrange
        let font_data = build_source_font();
        let naming = test_naming();
        let sut = super::rename;

        // Act
        let renamed = sut(&font_data, &naming);

        // Assert
        assert_eq!(
            Some("Rounded Test Sans".to_string()),
            read_name(&renamed, types::NameId::FAMILY_NAME)
        );
        assert_eq!(
            Some("Copyright © 2026 Test Author. Portions © Original Copyright Holder.".to_string()),
            read_name(&renamed, types::NameId::COPYRIGHT_NOTICE)
        );
    }

    // シナリオ: デザイナー名・ライセンス説明のような、上書き対象に含まれない
    // nameID のレコードは、元のフォントから変更されずに引き継がれる。
    #[test]
    fn rename_preserves_unrelated_name_records() {
        // Arrange
        let font_data = build_source_font();
        let naming = test_naming();
        let sut = super::rename;

        // Act
        let renamed = sut(&font_data, &naming);

        // Assert
        assert_eq!(
            Some("Original Designer".to_string()),
            read_name(&renamed, types::NameId::DESIGNER)
        );
        assert_eq!(
            Some("Licensed under the SIL OFL 1.1".to_string()),
            read_name(&renamed, types::NameId::LICENSE_DESCRIPTION)
        );
    }

    // シナリオ: 商標に関する断り書きが nameID 7 (Trademark) に設定される。
    #[test]
    fn rename_sets_trademark_disclaimer() {
        // Arrange
        let font_data = build_source_font();
        let naming = test_naming();
        let sut = super::rename;

        // Act
        let renamed = sut(&font_data, &naming);

        // Assert
        let trademark =
            read_name(&renamed, types::NameId::TRADEMARK).expect("TRADEMARK レコードが存在しない");
        assert!(trademark.contains("not produced, endorsed, or affiliated"));
    }

    // シナリオ: PostScript 名は、ファミリー名・サブファミリー名から空白を
    // 除いてハイフンで連結した値になる。
    #[test]
    fn rename_derives_postscript_name_without_spaces() {
        // Arrange
        let font_data = build_source_font();
        let naming = test_naming();
        let sut = super::rename;

        // Act
        let renamed = sut(&font_data, &naming);

        // Assert
        assert_eq!(
            Some("RoundedTestSans-Regular".to_string()),
            read_name(&renamed, types::NameId::POSTSCRIPT_NAME)
        );
    }
    /// 元の Regular/490 を Bold と命名した場合、OS の選択情報も更新する。
    #[test]
    fn rename_updates_style_and_font_revision() {
        use read_fonts::TableProvider;
        let source = build_source_font();
        let mut naming = test_naming();
        naming.style_name = "Bold".to_string();
        naming.version = "Version 0.101".to_string();
        let bytes = super::rename(&source, &naming);
        let font = read_fonts::FontRef::new(&bytes).unwrap();
        let os2 = font.os2().unwrap();
        assert_eq!(700, os2.us_weight_class());
        assert!(
            os2.fs_selection()
                .contains(write_fonts::tables::os2::SelectionFlags::BOLD)
        );
        assert!(
            !os2.fs_selection()
                .contains(write_fonts::tables::os2::SelectionFlags::REGULAR)
        );
        assert!(
            font.head()
                .unwrap()
                .mac_style()
                .contains(write_fonts::tables::head::MacStyle::BOLD)
        );
        assert!((font.head().unwrap().font_revision().to_f64() - 0.101).abs() < 1e-5);
        naming.style_name = "Regular".to_string();
        naming.weight_class = Some(450);
        let bytes = super::rename(&bytes, &naming);
        let font = read_fonts::FontRef::new(&bytes).unwrap();
        assert_eq!(450, font.os2().unwrap().us_weight_class());
        assert!(
            !font
                .head()
                .unwrap()
                .mac_style()
                .contains(write_fonts::tables::head::MacStyle::BOLD)
        );
        assert!(
            font.os2()
                .unwrap()
                .fs_selection()
                .contains(write_fonts::tables::os2::SelectionFlags::REGULAR)
        );
    }

    /// head に表現できない版を、名前の変更前に拒否する。
    #[test]
    fn invalid_versions_are_rejected() {
        for version in [
            "invalid",
            "Version NaN",
            "Version inf",
            "Version -1",
            "Version 32768",
        ] {
            assert!(super::parse_version(version).is_none());
        }
    }
}
