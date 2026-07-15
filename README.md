# rounded-noto-sans-cjk

[![CI](https://github.com/sakikuroe/rounded-noto-sans-cjk/actions/workflows/ci.yml/badge.svg)](https://github.com/sakikuroe/rounded-noto-sans-cjk/actions/workflows/ci.yml)

[日本語版 README](./README.ja.md)

A Rust tool that rounds the corners of every glyph in Noto Sans CJK JP and Noto Sans Mono CJK JP, producing a soft, rounded Japanese font family. It can also convert any other static OpenType font with CFF, CFF2, or TrueType (`glyf`) outlines.

![Specimen of the generated fonts](docs/images/specimen.png)

Building this repository produces four fonts:

| Family                   | Style   | Output file                                |
| ------------------------ | ------- | ------------------------------------------ |
| Rounded Noto Sans CJK JP | Regular | `fonts/RoundedNotoSansCJKJP-Regular.otf`   |
| Rounded Noto Sans CJK JP | Bold    | `fonts/RoundedNotoSansCJKJP-Bold.otf`      |
| Rounded Noto Code CJK JP | Regular | `fonts/RoundedNotoCodeCJKJP-Regular.otf`   |
| Rounded Noto Code CJK JP | Bold    | `fonts/RoundedNotoCodeCJKJP-Bold.otf`      |

## How it works

![Before/after comparison of Noto Sans CJK JP and Rounded Noto Sans CJK JP](docs/images/comparison.png)

Convex corners are rounded more strongly the sharper they are, while concave corners get a small fixed radius to avoid self-intersection; the result is blended with the original outline to control the overall roundness. The corner-radius formula is adapted from [Resource Han Rounded](https://github.com/CyanoHao/Resource-Han-Rounded), and the algorithm is documented in `src/round.rs`. In the Mono fonts, the ASCII range is replaced with [Source Code Pro](https://github.com/adobe-fonts/source-code-pro) outlines rounded with their own parameters.

## Requirements

- Rust 1.85+
- Python 3 with [fontTools](https://github.com/fonttools/fonttools) and [cffsubr](https://github.com/adobe-type-tools/cffsubr) (`cffsubr` must be on `PATH`)
- A few GB of free memory; each font takes several minutes to convert

## Building the fonts

Run all commands at the repository root. For license reasons the source fonts are not bundled; download them into the gitignored `fonts/` directory as follows.

### 1. Install the Python tools

```sh
pip install fonttools cffsubr
```

### 2. Download and prepare the source fonts

Noto Sans CJK JP DemiLight (source of Sans Regular):

```sh
mkdir -p fonts/weights
curl -LO https://github.com/notofonts/noto-cjk/releases/download/Sans2.004/06_NotoSansCJKjp.zip
unzip -j 06_NotoSansCJKjp.zip "*DemiLight.otf" -d fonts/weights
```

Noto Sans Mono CJK JP Regular / Bold (sources of Mono):

```sh
curl -LO https://github.com/notofonts/noto-cjk/releases/download/Sans2.004/11_NotoSansMonoCJKjp.zip
unzip -j 11_NotoSansMonoCJKjp.zip "*.otf" -d fonts
```

Noto Sans JP at weight 490 (source of Sans Bold). No static build exists at this weight, so instantiate it from the variable font. The result is TrueType, whose contours wind in the opposite direction to CFF, so also reverse them with the bundled script:

```sh
curl -L -o "NotoSansJP[wght].ttf" "https://raw.githubusercontent.com/notofonts/noto-cjk/main/google-fonts/NotoSansJP%5Bwght%5D.ttf"
python3 -m fontTools.varLib.instancer -o NotoSansJP-w490.ttf "NotoSansJP[wght].ttf" wght=490
python3 scripts/reverse_contours.py NotoSansJP-w490.ttf fonts/NotoSansJP-w490-reversed.ttf
```

Source Code Pro (ASCII replacement for the Mono fonts):

```sh
curl -LO https://github.com/adobe-fonts/source-code-pro/releases/download/2.042R-u/1.062R-i/1.026R-vf/VF-source-code-VF-1.026R.zip
unzip VF-source-code-VF-1.026R.zip VF/SourceCodeVF-Upright.otf
python3 -m fontTools.varLib.instancer -o fonts/SourceCodePro-w480.otf VF/SourceCodeVF-Upright.otf wght=480

curl -LO https://github.com/adobe-fonts/source-code-pro/releases/download/2.042R-u/1.062R-i/1.026R-vf/OTF-source-code-pro-2.042R-u_1.062R-i.zip
unzip -j OTF-source-code-pro-2.042R-u_1.062R-i.zip OTF/SourceCodePro-Bold.otf -d fonts
```

`fonts/` should now contain (zips and intermediate files may be deleted):

```
fonts/
├── weights/
│   └── NotoSansCJKjp-DemiLight.otf
├── NotoSansJP-w490-reversed.ttf
├── NotoSansMonoCJKjp-Regular.otf
├── NotoSansMonoCJKjp-Bold.otf
├── SourceCodePro-w480.otf
└── SourceCodePro-Bold.otf
```

### 3. Build

```sh
cargo run --release --bin generate
```

This reads `fonts.toml` and writes the four fonts listed at the top of this README into `fonts/`.

## Converting a single font

To round one font directly, without `fonts.toml`:

```sh
cargo run --release --bin rounded-noto-sans-cjk -- <input font> <output font> [base_radius inner_radius t]
```

- `base_radius` — base radius for convex corners (default `40.0`)
- `inner_radius` — fixed radius for concave corners (default `5.0`)
- `t` — blend ratio between the original and rounded outlines, `0.0`–`1.0` (default `0.85`)

## Configuration

`fonts.toml` defines which source font is converted with which parameters, and the family/style names written into the results. To convert other weights or adjust the roundness, edit its `[[font]]` entries; the fields are described in the comments in that file.

## License

- **Source code** — [MIT License](./LICENSE). The corner-radius formula is derived from Resource Han Rounded (Copyright © 2018–2022 Cyano Hao, MIT License); see [`THIRD-PARTY-NOTICES.md`](./THIRD-PARTY-NOTICES.md).
- **Generated fonts** — governed by the licenses of the source fonts. Noto Sans CJK JP, Noto Sans Mono CJK JP, and Source Code Pro are all licensed under the [SIL Open Font License 1.1](https://openfontlicense.org/), which also applies to fonts generated from them.

The Noto CJK fonts declare no Reserved Font Name under the OFL, so derived fonts may keep "Noto" in their names; the generated fonts are named "Rounded Noto …" to make clear that they are modified versions. "Source" is a Reserved Font Name of Adobe and must not appear in derived font names, which is why the monospaced family — a blend of Noto Sans Mono CJK JP and Source **Code** Pro — is named "Rounded Noto Code CJK JP". The original copyright and trademark notices ("Noto" is a trademark of Google Inc.) are preserved in the `name` table. This project is not affiliated with or endorsed by Google or Adobe.
