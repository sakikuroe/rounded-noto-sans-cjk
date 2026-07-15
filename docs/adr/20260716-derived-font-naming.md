# ADR-20260716: 生成フォントのファミリー名に "Noto" を含め、等幅は "Rounded Noto Code CJK JP" とする

## 本決定がいまどの段階にあるか (Status)

Proposed (2026-07-16)

## 本決定が必要になった背景と状況

本ツールが生成する派生フォントに、配布用のファミリー名を付ける必要がある。変換元フォントのライセンスと商標の状況は次のとおりである。

- Noto Sans CJK JP・Noto Sans Mono CJK JP は SIL Open Font License 1.1 で配布されており、name テーブルの著作権表示 (nameID 0) に OFL の Reserved Font Name (RFN) を宣言していない。一方、商標表示 (nameID 7) には "Noto is a trademark of Google Inc." と記載されている。
- Source Code Pro は著作権表示に RFN として "Source" を宣言している。OFL 第 3 条により、RFN は明示的な許諾なく派生フォント名に使用できない。
- 等幅の生成フォントは Noto Sans Mono CJK JP と Source Code Pro の混植であり、両方の制約を受ける。

また、変換元と同一の名前を名乗ると、利用者が公式版と取り違えるうえ、同名フォントの共存もできない。

## 検討した選択肢とそれぞれの長所・短所

### "Rounded Noto Sans CJK JP" / "Rounded Noto Code CJK JP" とする案

- 長所: 変換元が Noto であることと改変版であることが名前だけで伝わる。RFN 宣言がないため OFL 上の制約はなく、Nerd Fonts が RFN のない Noto を "Noto Nerd Font" として配布している前例もある。等幅の "Code" は、RFN の "Source" を避けつつ Source Code Pro 由来とコーディング用途を示せる (Nerd Fonts が Source Code Pro 派生で "Source" のみ回避し "Code Pro" を残した前例に同じ)。
- 短所: 商標は OFL とは別の法体系であり、"Noto" を含む名前には理論上の商標リスクが残る。

### "Noto" を含まない名前 (例: "Rounded Sans CJK JP") とする案

- 長所: 商標リスクを最小化できる。
- 短所: 変換元との関係が名前から伝わらず、"Rounded Sans" のような一般的すぎる名前は他のフォントと衝突しやすい。

### 完全な新造語 (Sarasa Gothic・Cica 方式) とする案

- 長所: 商標リスクがなく、独自ブランドとして育てられる。
- 短所: 由来がまったく伝わらず、検索性も低い。命名の考案と周知のコストがかかる。

### 何もしない (変換元と同一の名前を継承する)

- 短所: RFN がないため OFL 違反ではないが、公式版と識別できず共存もできない。丸め改変版が "Noto Sans CJK JP" を名乗ることは利用者の混乱を招くため採用できない。

## 最終的に採用する方針

ファミリー名を "Rounded Noto Sans CJK JP" (Sans) および "Rounded Noto Code CJK JP" (等幅) とする。

## その方針を採用し他を退けた理由

Noto CJK フォントに RFN 宣言がないことを name テーブルと OFL FAQ で確認しており、OFL 上は "Noto" を含む派生名が許容される。由来が名前から伝わる利点は、新造語や無関係な名前の商標リスク低減よりも利用者にとって価値が大きいと判断した。商標への配慮としては、生成フォントの name テーブルに原著作権表示と商標表示を保持し、非公式の改変版であり Google・Adobe と無関係である旨の断り書きを追加する (`src/naming.rs`) ことで足りると考える。"Source" は RFN のため名前に使用できず、"Code" による示唆で代替する。

## 本決定によって生じる影響と後続の作業

- ファミリー名は利用者の設定ファイルやスタイルシートから参照されるため、以後の改名は破壊的変更となる。名前は安定させ、変更する場合は新しい ADR で本決定を supersede する。
- `fonts.toml` の `family_name` と出力ファイル名 (`RoundedNotoSansCJKJP-*.otf` 等) を本命名に揃える (実施済み)。
- トレードオフ: Google から商標上の申し立てがあった場合は改名が必要になる。その際は本 ADR の前提 (RFN 宣言なし・断り書きによる誤認防止) を起点に再検討する。

## 関連する Issue・PR・ドキュメント

- README ライセンス節 (利用者向けの命名とライセンスの説明)
- `src/naming.rs` (name テーブルの書き換えと断り書きの実装)
- https://openfontlicense.org/ofl-faq/ (RFN と商標の関係に関する OFL FAQ)
