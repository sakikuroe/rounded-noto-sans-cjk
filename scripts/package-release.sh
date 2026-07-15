#!/usr/bin/env bash
# 生成済みフォントを GitHub Release へ添付する zip にまとめるスクリプトである。
#
# `cargo run --release --bin generate` で fonts/ に 4 フォントを生成したあとに
# 実行する。dist/v<version>/ に、単体ダウンロード用の各 OTF と、全フォント +
# licenses/OFL.txt (フォントのライセンス全文) をまとめた zip の両方を揃える。
# GitHub Release へは dist/v<version>/* をそのまま添付すればよい。
#
# 使い方: scripts/package-release.sh <バージョン (例: 1.0.0)>
set -euo pipefail
cd "$(dirname "$0")/.."

version=${1:?usage: scripts/package-release.sh <version e.g. 1.0.0>}
[[ $version =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || {
    echo "error: バージョンは X.Y.Z 形式で指定すること (例: 1.0.0)" >&2
    exit 1
}

# 梱包対象のフォント一覧は fonts.toml の fonts_dir と各エントリーの output
# から読み取る。ビルド出力の定義を fonts.toml と二重管理すると、設定変更に
# 本スクリプトが追従できなくなるためである (tomllib は Python 3.11 以上)。
mapfile -t fonts < <(python3 - <<'PY'
import tomllib

with open("fonts.toml", "rb") as f:
    config = tomllib.load(f)
for font in config["font"]:
    print(f"{config['fonts_dir']}/{font['output']}")
PY
)
[[ ${#fonts[@]} -gt 0 ]] || {
    echo "error: fonts.toml から出力フォントの一覧を読み取れない" >&2
    exit 1
}
for f in "${fonts[@]}"; do
    [[ -f $f ]] || { echo "error: $f がない。先に generate を実行すること" >&2; exit 1; }
done
[[ -f licenses/OFL.txt ]] || { echo "error: licenses/OFL.txt がない" >&2; exit 1; }

outdir="dist/v${version}"
rm -rf "$outdir"
mkdir -p "$outdir"
cp "${fonts[@]}" "$outdir/"
# zip コマンドには依存せず、Python 標準ライブラリで zip -j 相当を行う。
python3 - "$outdir/rounded-noto-sans-cjk-v${version}.zip" "${fonts[@]}" licenses/OFL.txt <<'PY'
import os
import sys
import zipfile

out, *files = sys.argv[1:]
with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED) as z:
    for f in files:
        z.write(f, os.path.basename(f))
PY
echo "created:"
ls -1 "$outdir"
