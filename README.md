# つらつら (tsuratsura)

**1日の仕事を、つらつら書く。**

朝から終業まで開きっぱなしにする「今日の1枚」のプレーンテキストメモ帳（macOS / Windows）。

仕様: [docs/REQUIREMENTS.md](docs/REQUIREMENTS.md) / 性能測定: [docs/PERFORMANCE.md](docs/PERFORMANCE.md)

## 使い方

起動すると今日の日次ファイルが開き、そのまま入力できます。保存は自動です。

| 操作 | キー（`Primary` = macOS: Cmd / Windows: Ctrl） |
|---|---|
| 作業種別を挿入 | `Primary+K` → キー（`W` `R` `S` `M` `G`）、または `↑↓` + `Enter` / クリック。`Esc` で閉じる |
| 行頭にプレフィックスを付ける | `Primary+L` → キー（`A` `R` `I`）。選び方は作業種別と同じ |
| 過去のメモを開く | `Primary+E` → 日付を `↑↓` + `Enter` / クリック / `1`〜`9` で選ぶ。今日へ戻る時も同じ一覧から |
| 今日の全文をコピー | `Primary+Shift+C` |
| 最前面に固定 / 解除 | `Primary+Shift+T`、または右上の「最前面に固定」 |
| 折り畳む / 開く | `Primary+Shift+M`、または右上の「折り畳む」。開く時はバーをクリック / `Enter` でもよい |
| 本文を拡大 / 縮小 | `Primary` を押しながらホイール、または `Primary` と `+` / `-`。`Primary+0` で元の大きさに戻す |
| Undo / Redo | `Primary+Z` / macOS: `Cmd+Shift+Z`、Windows: `Ctrl+Y` または `Ctrl+Shift+Z` |
| 即時保存（通常は不要） | `Primary+S` |

最前面に固定している間は、ほかのアプリへ移っても開いたまま前に残ります。邪魔な時は折り畳むと小さなバーになり、バーをクリックすると元の大きさに戻ります。起動時から固定するには config に `always_on_top = true` を指定します。

日付をまたいで開いたままにした場合は、次にウィンドウへフォーカスした時に今日のファイルへ切り替わります。

## ファイルの場所

| | macOS | Windows |
|---|---|---|
| config | `~/Library/Application Support/tsuratsura/config.toml` | `%APPDATA%\tsuratsura\config.toml` |
| 日次メモ | `~/Library/Application Support/tsuratsura/daily/YYYY-MM-DD.txt` | `%APPDATA%\tsuratsura\daily\YYYY-MM-DD.txt` |

日次メモは UTF-8（BOMなし）・LF の `.txt` です。**30日より前の日次メモは、起動時と日付切り替え時に自動で削除されます**（ゴミ箱には入りません）。日数は config の `retention_days` で変更でき、`0` で削除しなくなります。config は初回起動時に [config.example.toml](config.example.toml) と同じ内容で生成され、変更は再起動で反映されます。本番用の `config.toml` はコミットしません（`.gitignore` 済み）。
`data_dir` には絶対パスのみ指定できます（`~` は展開しません）。

## フォント

本文とセレクタは、同梱の [HackGen](https://github.com/yuru7/HackGen) Regular（v2.10.0）で表示します。別のフォントを使う場合は、config の `font_family` に OS へインストール済みのフォント名を指定します（例: `font_family = "BIZ UDGothic"`。見つからない名前を指定した場合は OS の代替フォントで表示されます）。

v0.1.0（旧名 Daily Work Memo）のメモ・config は `daily-work-memo` フォルダにあります。引き継ぐ場合は、新しい版を起動する前にフォルダ名を `tsuratsura` に変えてください（macOS: `~/Library/Application Support/daily-work-memo`、Windows: `%APPDATA%\daily-work-memo`）。

## 開発

```sh
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
cargo build --release
./scripts/bundle-macos.sh        # target/release/Tsuratsura.app（universal・署名なし）
cargo run --release --example spike -- 10000   # Phase 0 spike
```

Windows の `cargo build --release` では、`assets/app-icon.ico` を実行ファイルに埋め込みます（Windows SDK のリソースコンパイラーが必要）。起動中のウィンドウにも同じ絵柄のアイコンを設定します。更新後もピン留めしたアイコンが古い場合は、ピン留めを外し、新しい実行ファイルを起動して再度ピン留めしてください。

## Phase 0 Go / No-Go

**Go（条件付き）** — macOS で起動 0.3 s（warm）、アイドル CPU 0.1〜0.2 %、通常サイズで RSS 約 89 MB（Apple M3 ネイティブ。Rosetta 実行時は約 45 MB）、日本語 IME 入力も動作。
日次テキストが数百 KB を超えると RSS が 150 MB を超える制約あり。Windows 実機での確認は未実施。詳細は [docs/PERFORMANCE.md](docs/PERFORMANCE.md)。

## 仕様との差分

- **Undo/Redo を `editor.rs` で実装**: Iced 0.14 の TextEditor に Undo/Redo が無いため、編集のまとまりごとの全文スナップショット（最大100件・16MB）で実装。
- **`src/saver.rs`・`src/lib.rs` を追加**: 自動保存の debounce と書き込みを専用スレッドで行うため（UI をブロックしない）。`lib.rs` は `tests/` から各モジュールを使うため。
- **macOS の Cmd+Q 対策**: winit の標準メニューの Quit は close request を経ずに終了するため、Cmd キー押下時点で未保存分を書き込みに回し、終了処理（`atexit`）で書き込み完了を最大2秒待つ。保存失敗中・読み込み失敗中に終了した場合は、日次ファイルを上書きせず `<data_dir>/recovery/YYYY-MM-DD-HHMMSS.txt`（書けなければ一時ディレクトリ）へ本文を退避する。このため macOS のみ `libc` に直接依存。
- **全文コピー失敗は通知しない**: Iced のクリップボード書き込みは成否を返さないため検出できない。
- **プレフィックス挿入（`Primary+L`）を追加**: `<action item> ` `<remind> ` `<重要> ` などの目印を、カーソルがある行の先頭（インデントの後ろ）へ挿入する。カーソルは元の文字の位置に留まる。config の `[[prefixes]]`（`text` と `key`）で変更でき、`prefixes = []` で無効。これに伴い、作業種別の既定から「リマインド」「アクションアイテム」を外した（既存の config.toml はそのまま）。
- **過去のメモを開く（`Primary+E`）を追加**: 本文がある日次メモの一覧（今日が先頭、以降は新しい順）から選んで開く。開いた日はそのまま編集でき、その日のファイルへ自動保存される。過去の日を開いている間は上部に日付を出し、日付が変わっても今日へ切り替えない。今日へは同じ一覧から戻る。
- **最前面固定と折り畳みを追加**: ウィンドウを最前面に固定でき（`Primary+Shift+T`）、小さなバー（内寸 260×28）へ折り畳める（`Primary+Shift+M`）。バーはウィンドウの左上の位置に残り、展開すると元の大きさに戻る。畳む・開くは明示的な操作でだけ行い、フォーカスの出入りでは変えない。フルスクリーン中は折り畳まない。保存できていない内容や未保存で閉じようとした警告がある間は、バーに「要確認」と出す。
- **本文のズームを追加**: `Primary` を押しながらホイール、または `Primary` と `+` / `-` で、本文のフォントサイズを 1 ずつ変える（8〜72）。`+` は Shift なしの同じキー（US 配列の `=`、JIS 配列の `;`）でもよい。`Primary+0` で config の `font_size` に戻す。変えた大きさは保存せず、次の起動では config の値から始まる。本文が見えていない折り畳み中・セレクタ表示中は変えない。
- **フォントを同梱**: OS 既定のフォントでは Windows で日本語の見た目が揃わないため、HackGen Regular を実行ファイルに埋め込んで既定にした（バイナリが約 10.7 MB、RSS が約 20〜27 MB 増える。[docs/PERFORMANCE.md](docs/PERFORMANCE.md)）。config の `font_family` で本文だけ別のフォントに変えられる。
- **Esc でエディタのフォーカスを外さない**: Iced 既定では Esc でフォーカスが外れ入力できなくなるため無効化。
- **`font_size` を 8〜72 に制限**、**未知の config キーはエラー**（typo の見落とし防止）。config エラー時も、有効な絶対パスの `data_dir` だけは引き継ぐ（メモの保存先が分かれるのを防ぐ）。
- **貼り付け・IME確定の CR は LF へ正規化**して挿入する。
- **日次ファイル読み込み失敗時は保存を停止**し、未編集ならフォーカス復帰時に読み直す。
- **ログは stderr のみ**（本文は出さない）。Windows の release ビルドはコンソールを持たないためログは見えない。
- **古い日次メモの自動削除**: `retention_days`（既定 30、`0` で無効）より前の `daily/YYYY-MM-DD.txt` を起動時・日付切り替え時に削除する。それ以外の名前のファイルと `recovery/` には触れない。config を読めずに内蔵デフォルトで起動した時は削除しない（保持日数の設定を取り違えて消すのを防ぐ）。
- **多重起動を禁止**: config と同じディレクトリの `instance.lock` を排他ロックし、2つ目の起動は「既に起動しています」とだけ表示する（既存ウィンドウの前面化はしない）。ロック自体を作れない環境では起動を優先する。ロックのため `fs4` に依存。

## 既知の制約

- 最前面固定は、macOS ではほかのアプリのフルスクリーン表示の上には出ない。Windows での最前面固定・折り畳みは未検証。
- Windows のログオフ・シャットダウン時に close request が来るかは未検証。
- 日次テキストが数百 KB を超えると RSS が 150 MB を超える（[docs/PERFORMANCE.md](docs/PERFORMANCE.md)）。

## ライセンス

[MIT](LICENSE)

同梱フォント HackGen は [SIL Open Font License 1.1](assets/fonts/LICENSE-HackGen.txt) です（Copyright (c) 2019, Yuko OTAWARA）。無改変で実行ファイルに埋め込んでいます。
