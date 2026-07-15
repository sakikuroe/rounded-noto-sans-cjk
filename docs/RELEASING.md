# リリース手順

生成したフォントを GitHub Release として公開するまでの手順である。
コマンドはすべてリポジトリのルートで実行する。

## 前提

- `fonts/` にソースフォントを配置済みであること (README の「フォントのビルド」を参照)
- `cffsubr` が `PATH` にあること
- [GitHub CLI (`gh`)](https://cli.github.com/) をインストールし、`gh auth login` 済みであること

## 1. バージョンを更新する

`fonts.toml` のトップレベルの `version` を更新する。この値はフォントの
`name` テーブル (nameID 5) に書き込まれる。Git のタグとは次のように対応させる。

| fonts.toml の `version` | Git タグ |
| ----------------------- | -------- |
| `Version 0.100`         | `v0.1.0` |
| `Version 0.200`         | `v0.2.0` |
| `Version 1.000`         | `v1.0.0` |
| `Version 1.001`         | `v1.0.1` |

安定するまでは 0.x を使う。破壊的でない字形調整・パラメータ変更はマイナー
(0.100 → 0.200) を上げる。

## 2. フォントをビルドする

```sh
cargo run --release --bin generate
```

`fonts/` に 4 つの OTF が生成される。1 フォントあたり数分かかる。

生成物の名前・バージョンが意図どおりか確認しておくとよい (要 `fontTools`):

```sh
python3 -c "
from fontTools.ttLib import TTFont
for p in ['RoundedNotoSansCJKJP-Regular', 'RoundedNotoSansCJKJP-Bold',
          'RoundedNotoCodeCJKJP-Regular', 'RoundedNotoCodeCJKJP-Bold']:
    n = TTFont(f'fonts/{p}.otf')['name']
    print(n.getDebugName(16), '/', n.getDebugName(17), '/', n.getDebugName(5))
"
```

## 3. リリース資産をまとめる

```sh
scripts/package-release.sh 0.1.0
```

`dist/v0.1.0/` に次が揃う。

- `RoundedNotoSansCJKJP-Regular.otf` など単体ダウンロード用の OTF 4 本
- `rounded-noto-sans-cjk-v0.1.0.zip` — OTF 4 本 + `OFL.txt` (フォントのライセンス全文) 入りの一括ダウンロード用 zip

## 4. コミット・タグ・リリースを作成する

```sh
git add fonts.toml
git commit -m "Release: v0.1.0"
git push origin main

gh release create v0.1.0 dist/v0.1.0/* \
    --title "v0.1.0" \
    --notes "変更点をここに書く"
```

`gh release create` はタグ `v0.1.0` を現在のコミットに作成して push し、
`dist/v0.1.0/` のファイルをすべて資産として添付する。公開後、
`https://github.com/sakikuroe/rounded-noto-sans-cjk/releases` で
zip と各 OTF がダウンロードできることを確認する。

## 補足: なぜ zip と OTF の両方を置くのか

フォントファミリー一式が欲しい利用者には zip (ライセンス全文同梱) が、
1 書体だけ欲しい利用者には単体 OTF が便利なためである。OTF (CFF) は
内部圧縮されており zip による削減効果は小さいので、zip は圧縮のためでは
なく「まとめて 1 ファイルで配る」ために置いている。
