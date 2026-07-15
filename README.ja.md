# rounded-noto-sans-cjk

[![CI](https://github.com/sakikuroe/rounded-noto-sans-cjk/actions/workflows/ci.yml/badge.svg)](https://github.com/sakikuroe/rounded-noto-sans-cjk/actions/workflows/ci.yml)

[English README](./README.md)

Noto Sans CJK JP および Noto Sans Mono CJK JP のすべてのグリフの角を丸め、やわらかい印象の角丸日本語フォントファミリーを生成する Rust 製のツールです。CFF、CFF2、TrueType (`glyf`) のアウトラインを持つ静的な OpenType フォントであれば、他のフォントも変換できます。

![生成されるフォントの見本](docs/images/specimen.png)

このリポジトリをビルドすると、以下の 4 つのフォントが生成されます。

| ファミリー               | スタイル | 出力ファイル                               |
| ------------------------ | -------- | ------------------------------------------ |
| Rounded Noto Sans CJK JP | Regular  | `fonts/RoundedNotoSansCJKJP-Regular.otf`   |
| Rounded Noto Sans CJK JP | Bold     | `fonts/RoundedNotoSansCJKJP-Bold.otf`      |
| Rounded Noto Code CJK JP | Regular  | `fonts/RoundedNotoCodeCJKJP-Regular.otf`   |
| Rounded Noto Code CJK JP | Bold     | `fonts/RoundedNotoCodeCJKJP-Bold.otf`      |

## 仕組み

![Noto Sans CJK JP と Rounded Noto Sans CJK JP の変換前後の比較](docs/images/comparison.png)

凸角は鋭いほど大きく丸め、凹角は輪郭の自己交差を防ぐために小さな固定半径で丸めます。そのうえで、丸めた輪郭を元の輪郭と補間することで、丸みの度合いを調整します。丸め半径の計算式は [Resource Han Rounded](https://github.com/CyanoHao/Resource-Han-Rounded) をベースにしており、アルゴリズムの詳細は `src/round.rs` に記載されています。Mono (等幅) フォントでは、ASCII 範囲のグリフを [Source Code Pro](https://github.com/adobe-fonts/source-code-pro) の輪郭に差し替えたうえで、個別のパラメータを用いて丸め処理を行っています。

## 動作環境

- Rust 1.85 以降
- Python 3 および [fontTools](https://github.com/fonttools/fonttools)・[cffsubr](https://github.com/adobe-type-tools/cffsubr) (`cffsubr` は `PATH` から実行できる必要があります)
- 数 GB の空きメモリ (変換は 1 フォントあたり数分かかり、ピーク時に数 GB のメモリを使用します)

## フォントのビルド方法

コマンドはすべてリポジトリのルートディレクトリで実行してください。ライセンス上の理由からソースフォントは同梱していないため、以下の手順でダウンロードして `fonts/` ディレクトリへ配置してください (`fonts/` は `.gitignore` に登録済みのため、コミットには含まれません)。

### 1. Python ツールのインストール

```sh
pip install fonttools cffsubr
```

### 2. ソースフォントのダウンロードと前処理

Noto Sans CJK JP DemiLight (Sans Regular の変換元):

```sh
mkdir -p fonts/weights
curl -LO https://github.com/notofonts/noto-cjk/releases/download/Sans2.004/06_NotoSansCJKjp.zip
unzip -j 06_NotoSansCJKjp.zip "*DemiLight.otf" -d fonts/weights
```

Noto Sans Mono CJK JP Regular / Bold (Mono の変換元):

```sh
curl -LO https://github.com/notofonts/noto-cjk/releases/download/Sans2.004/11_NotoSansMonoCJKjp.zip
unzip -j 11_NotoSansMonoCJKjp.zip "*.otf" -d fonts
```

Noto Sans JP ウェイト 490 (Sans Bold の変換元)。このウェイトの静的フォントファイルは提供されていないため、可変フォントからインスタンスを切り出します。インスタンス化した結果は TrueType (`glyf`) 形式で、輪郭の巻き方向が CFF の慣例と逆になっているため、同梱のスクリプトで全グリフの輪郭を反転しておきます。

```sh
curl -L -o "NotoSansJP[wght].ttf" "https://raw.githubusercontent.com/notofonts/noto-cjk/main/google-fonts/NotoSansJP%5Bwght%5D.ttf"
python3 -m fontTools.varLib.instancer -o NotoSansJP-w490.ttf "NotoSansJP[wght].ttf" wght=490
python3 scripts/reverse_contours.py NotoSansJP-w490.ttf fonts/NotoSansJP-w490-reversed.ttf
```

Source Code Pro (Mono フォントの ASCII 部分の差し替え用):

```sh
curl -LO https://github.com/adobe-fonts/source-code-pro/releases/download/2.042R-u/1.062R-i/1.026R-vf/VF-source-code-VF-1.026R.zip
unzip VF-source-code-VF-1.026R.zip VF/SourceCodeVF-Upright.otf
python3 -m fontTools.varLib.instancer -o fonts/SourceCodePro-w480.otf VF/SourceCodeVF-Upright.otf wght=480

curl -LO https://github.com/adobe-fonts/source-code-pro/releases/download/2.042R-u/1.062R-i/1.026R-vf/OTF-source-code-pro-2.042R-u_1.062R-i.zip
unzip -j OTF-source-code-pro-2.042R-u_1.062R-i.zip OTF/SourceCodePro-Bold.otf -d fonts
```

ここまでの手順を完了すると、`fonts/` ディレクトリは次の構成になります (ダウンロードした zip ファイルや中間ファイルは削除しても問題ありません)。

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

### 3. ビルドの実行

```sh
cargo run --release --bin generate
```

`fonts.toml` の設定を読み込み、冒頭の表に示されている 4 つのフォントを `fonts/` ディレクトリに出力します。

## 単一フォントの変換

`fonts.toml` を使用せず、フォントを 1 本だけ変換する場合は、以下のコマンドを実行します。

```sh
cargo run --release --bin rounded-noto-sans-cjk -- <入力フォント> <出力フォント> [base_radius inner_radius t]
```

- `base_radius` — 凸角の基準半径 (デフォルト値: `40.0`)
- `inner_radius` — 凹角に適用する固定半径 (デフォルト値: `5.0`)
- `t` — 元の輪郭と丸めた輪郭の補間比率 `0.0`〜`1.0` (デフォルト値: `0.85`)

## 設定

どのソースフォントをどのパラメータで変換するか、および出力されるフォントのファミリー名やスタイル名は `fonts.toml` で定義されています。別のウェイトを変換したい場合や、丸みの度合いを微調整したい場合は、`[[font]]` エントリーを編集してください。各設定項目の詳細については、同ファイル内のコメントを参照してください。

## ライセンス

- ソースコード — [MIT License](./LICENSE) のもとで公開されています。丸め半径の計算式は Resource Han Rounded (Copyright © 2018–2022 Cyano Hao, MIT License) に基づいています。詳細は [`THIRD-PARTY-NOTICES.md`](./THIRD-PARTY-NOTICES.md) を参照してください。
- 生成されるフォント — 変換元フォントのライセンスが適用されます。Noto Sans CJK JP、Noto Sans Mono CJK JP、および Source Code Pro はすべて [SIL Open Font License 1.1](https://openfontlicense.org/) に基づいてライセンスされており、これらから生成されたフォントにも同ライセンスが適用されます。

Noto CJK フォントは OFL 上の Reserved Font Name (予約フォント名) を宣言していないため、派生フォントの名前に "Noto" を含めることができます。本ツールで生成するフォントは、改変版であることが明確に伝わるよう "Rounded Noto" を冠した名前にしています。一方で "Source" は Adobe が OFL 上で宣言している Reserved Font Name であり、派生フォント名に使用できないため、Noto Sans Mono CJK JP と Source Code Pro を組み合わせた等幅フォントファミリーは "Rounded Noto Code CJK JP" と命名しています。元の著作権表示および商標表示 ("Noto" は Google Inc. の商標です) は、フォントの `name` テーブル内に保持されます。なお、本プロジェクトは Google および Adobe とは無関係であり、両社の承認を受けたものではありません。