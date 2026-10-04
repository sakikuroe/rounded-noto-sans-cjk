//! CLI の事前検証と、生成後段の失敗時に既存の出力を守ることを検証する。

use std::{fs, process};

use write_fonts::tables::{cmap, head, hhea, hmtx, maxp, name, os2, post};
use write_fonts::types;

/// 圧縮までは成功し、著作権表示の読み取りで失敗する小さな入力を用意する。
fn input_font(copyright: bool) -> Vec<u8> {
    let mut builder = write_fonts::FontBuilder::new();
    builder
        .add_table(&head::Head {
            units_per_em: 1000,
            ..Default::default()
        })
        .unwrap();
    builder
        .add_table(&hhea::Hhea {
            number_of_h_metrics: 2,
            ..Default::default()
        })
        .unwrap();
    builder.add_table(&maxp::Maxp::new(2)).unwrap();
    builder
        .add_table(&hmtx::Hmtx {
            h_metrics: vec![
                hmtx::LongMetric {
                    advance: 1000,
                    side_bearing: 0
                };
                2
            ],
            left_side_bearings: Vec::new(),
        })
        .unwrap();
    builder
        .add_table(&cmap::Cmap::from_mappings([('A', types::GlyphId::new(1))]).unwrap())
        .unwrap();
    let mut records = [
        (1, "Generation Test"),
        (2, "Regular"),
        (3, "Generation Test Regular"),
        (4, "Generation Test Regular"),
        (6, "GenerationTest-Regular"),
    ]
    .into_iter()
    .map(|(id, value)| {
        name::NameRecord::new(
            3,
            1,
            0x0409,
            types::NameId::new(id),
            value.to_string().into(),
        )
    })
    .collect::<Vec<_>>();
    if copyright {
        records.push(name::NameRecord::new(
            3,
            1,
            0x0409,
            types::NameId::new(0),
            "Copyright Test".to_string().into(),
        ));
        records.sort_by_key(|record| record.name_id);
    }
    builder.add_table(&name::Name::new(records)).unwrap();
    builder.add_table(&os2::Os2::default()).unwrap();
    builder.add_table(&post::Post::default()).unwrap();
    let paths = vec![
        kurbo::BezPath::new(),
        kurbo::BezPath::from_svg("M100 0 L600 0 L600 700 L100 700 Z").unwrap(),
    ];
    rounded_noto_sans_cjk::variable_font::build_static_font(&builder.build(), &paths)
}

/// 不正な丸みは入力を読む前に検出し、既存出力を上書きしない。
#[test]
fn cli_rejects_invalid_roundness_before_reading_input() {
    let directory = tempfile::tempdir().unwrap();
    let output_path = directory.path().join("output.otf");
    fs::write(&output_path, b"existing font").unwrap();
    for value in ["NaN", "inf", "-inf", "-0.1", "1.1"] {
        let output = process::Command::new(env!("CARGO_BIN_EXE_rounded-noto-sans-cjk"))
            .arg(directory.path().join("missing.otf"))
            .arg(&output_path)
            .args(["40", "0", value])
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("roundness must be within 0..1"));
        assert_eq!(b"existing font", fs::read(&output_path).unwrap().as_slice());
    }
}

/// 輪郭変換後の失敗でも、最終出力には以前の完成済みフォントが残る。
#[test]
fn generator_preserves_existing_output_when_naming_fails() {
    let directory = tempfile::tempdir().unwrap();
    fs::write(directory.path().join("input.otf"), input_font(false)).unwrap();
    let output_path = directory.path().join("output.otf");
    fs::write(&output_path, b"existing font").unwrap();
    let config = r#"
fonts_dir = "."
[[font]]
name = "test"
source = "input.otf"
output = "output.otf"
base_radius = 20.0
inner_radius = 0.0
rond = 0.85
family_name = "Generation Test"
style_name = "Regular"
"#;
    fs::write(directory.path().join("fonts.toml"), config).unwrap();
    let output = process::Command::new(env!("CARGO_BIN_EXE_generate"))
        .current_dir(directory.path())
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("nameID 0"),
        "Unexpected failure: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(b"existing font", fs::read(output_path).unwrap().as_slice());
}

/// 完成したフォントへの置換時にも、既存ファイルのアクセス権を維持する。
#[cfg(unix)]
#[test]
fn generator_publishes_completed_font_with_existing_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let directory = tempfile::tempdir().unwrap();
    fs::write(directory.path().join("input.otf"), input_font(true)).unwrap();
    let output_path = directory.path().join("output.otf");
    fs::write(&output_path, b"old").unwrap();
    fs::set_permissions(&output_path, fs::Permissions::from_mode(0o640)).unwrap();
    let config = r#"
fonts_dir = "."
[[font]]
name = "test"
source = "input.otf"
output = "output.otf"
base_radius = 20.0
inner_radius = 0.0
rond = 0.85
family_name = "Generation Test"
style_name = "Regular"
"#;
    fs::write(directory.path().join("fonts.toml"), config).unwrap();
    let output = process::Command::new(env!("CARGO_BIN_EXE_generate"))
        .current_dir(directory.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(b"OTTO", &fs::read(&output_path).unwrap()[..4]);
    assert_eq!(
        0o640,
        fs::metadata(output_path).unwrap().permissions().mode() & 0o777
    );
}
