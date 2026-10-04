# Code フォントのセル幅

`fonts.toml` の `normalize_code_widths = true` は、静的 CFF2 の水平字送り幅を 0・0.5・1 em に揃えます。既定の Code Regular / Bold に適用し、Sans には適用しません。分類は `unicodedata2==16.0.0` に収録された Unicode 16.0.0 のデータを使用します。生成には `scripts/requirements.txt` の Python 依存関係が必要です。

通常文字は East_Asian_Width が W / F なら 1 em、Na / H / N / A なら 0.5 em です。このため、罫線・ブロック要素・拡張ラテン文字・ギリシャ文字・キリル文字は半角、漢字・かな・ハングル完成音節は全角になります。Mn / Me の結合文字と Cf の制御文字はゼロ幅にします。SOFT HYPHEN (U+00AD) は表示時の役割に合わせて半角、ハングル声調記号 (U+302E・U+302F) は結合記号としてゼロ幅にします。

EM QUAD / EM SPACE (U+2001・U+2003) は全角、EN QUAD / EN SPACE (U+2000・U+2002) は半角です。それ以外の空白は通常文字の分類に従います。Jamo (U+1100–U+11FF、U+A960–U+A97F、U+D7B0–U+D7FF) は単独では全角とし、GSUB が選ぶ合成用の中声・終声字形は元のゼロ幅の役割を維持します。

異体字は元の文字の分類を継承し、GSUB の単一置換・選択置換・結合文字との合成も分類を継承します。同じ輪郭を参照していても分類が異なる場合は、字形を分離します。文字コードのない字形を一律に除去せず、全 Unicode cmap・異体字シーケンス・保持する全 GSUB 機能からの到達可能性を fontTools で計算し、対応する GPOS・GDEF・垂直メトリクスも更新します。未知のテーブル・未対応の組版形式・分類不能な字形があれば生成を停止します。

セル幅を変える aalt / fwid / hwid / pwid / halt / vhal と欧文の liga / dlig を無効化します。ccmp のうち複数の通常文字を一字形へ縮める置換も除き、結合文字の合成と Jamo 合成を維持します。したがって、これらの幅変更機能をエディター側で有効にしてもセル数は変わりません。locl、歴史字形、かな・漢字の異体字、縦書き用字形は保持します。

非ゼロ幅の輪郭は水平に縮尺調整し、必要に応じてセル内へ配置します。罫線・ブロック要素は元の左右の境界を 0・0.5 em の位置へ移します。結合アンカーにも同じ座標変換を適用します。合成 Jamo の輪郭には音節と共通の 0.92 em から 1 em への変換を使います。ゼロ幅のアクセント・濁点は基底文字に重なり、ゼロ幅のハングル声調記号は HarfBuzz が音節の後ろに残すため音節の左へ配置します。声調記号が左へ張り出すのは結合記号としての意図した動作です。

ラテン・ギリシャ・キリル文字には、輪郭の境界からアクセントの結合アンカーを補います。上側・下側のアクセントを文字の中央に合わせ、同じ側に複数ある場合は積み重ねます。元の注音符号用アンカーは保持します。Jamo 合成機能 (ljmo / vjmo / tjmo) は日本語・中国語の言語指定でも有効にし、文書の言語によって古いハングル音節の幅が変わるのを防ぎます。

生成後の検証には `python3 scripts/verify_code_widths.py fonts/RoundedNotoCodeCJKJP-Regular.otf fonts/RoundedNotoCodeCJKJP-Bold.otf` を使用します。全字形の集計、全 cmap・異体字シーケンス、Regular / Bold の分類一致、HarfBuzz の代表的な組版、アクセントの結合位置、罫線・ブロックの境界位置を確認します。`--references` に変更前のフォントを同じ順番で渡すと文字コードと異体字シーケンスの欠落も確認します。

ブラウザーでの確認用サンプルは [code-width-specimen.html](../code-width-specimen.html) にあります。VS Code では生成した両ウェイトをインストールし、`editor.fontFamily` を `Rounded Noto Code CJK JP` に設定して [code-width-specimen.txt](../code-width-specimen.txt) を開いてください。行の枠と結合記号を Regular / Bold で比較できます。既に同名のフォントを使用している場合は、差し替え後にアプリケーションを再起動してください。

文字分類の根拠は [Unicode 16.0.0 の East Asian Width](https://www.unicode.org/reports/tr11/tr11-43.html)、アンカー調整は [OpenType GPOS](https://learn.microsoft.com/en-us/typography/opentype/spec/gpos)、声調記号の配置は [HarfBuzz のハングル処理](https://github.com/harfbuzz/harfbuzz/blob/main/src/hb-ot-shaper-hangul.cc) を参照しています。
