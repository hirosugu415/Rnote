# Rnote 設計メモ

このファイルは、セッションが途中で切れても作業を再開できるように、方針をプロジェクト内に残す
ためのドキュメント。実装が進んだら「実装状況」セクションを更新すること。

## 目的

Windows標準メモ帳の代わりに使う、個人用の一元化メモアプリ。玉石混交の情報（大事なアイデアと、
使い捨ての下書き・作業メモ）を同じ場所に書けるが、重要度で区別・整理できるようにする。

## ストレージ設計（安定・変更なし）

- 保存先: `%USERPROFILE%\Documents\Rnote\keep\` と `...\scratch\`。設定で変更可
  （`%APPDATA%\Rnote\config.json`）。
- 各メモは1ファイル1`.md`。ファイル名 `YYYY-MM-DD_HHmm - 本文1行目のスラッグ.md`（分単位、
  区切りは` - `。`naming.rs`の`TIMESTAMP_FORMAT`/`SEPARATOR`定数、`scan.rs`側もこれを再利用）。
  **ファイル名は作成時に確定し、以後リネームしない**（内容が変わってもファイル名は不変）。
  同じ分内の衝突は`_2`, `_3`...を付与。旧形式（秒単位・区切りが半角スペース1つ）のファイルも、
  タイムスタンプ部分の先頭15バイトとスラッグ開始オフセット(18バイト目)がたまたま同じ位置に
  一致するため、`scan.rs`のフォールバックなしでそのまま正しく読める（移行不要）。
- フロントマターなし。keep/scratchの区別は「どちらのフォルダに入っているか」そのもの
  （物理的な状態。フラグではない）。**トピック分類はフォルダを作らず、本文中の `#tag`
  （Unicode対応、日本語可）＋アプリ内検索に任せる方針**（軸B: トピック — フォルダを増やすと
  保存先選択の手間が戻ってくるため導入しない、という当初方針どおり）。
- 作成日時はファイル名のタイムスタンプが第一情報源（コピー/バックアップで壊れないため）。
  ファイル名が命名規則に合わない場合のみ `fs::metadata().created()` → `.modified()` にフォールバック。
- Scratchが `expiryDays`（既定14日）を超えると一覧で「まもなく期限切れ」バッジ表示。**自動削除はしない**。

## Rustバックエンド構成

```
src-tauri/src/
  main.rs, lib.rs       トレイ/グローバルホットキー/単一インスタンス/自動起動/非フォーカスで隠す
  config.rs             Settings { notesDir, hotkey, hideOnBlur, expiryDays, lastStatus }
  state.rs               AppState { config, notes(常駐キャッシュ), watch_generation, focused, our_hwnd }
  watcher.rs               notify-debouncer-full で keep/scratch を監視 → 変更があれば全体再スキャン
                           → キャッシュ更新 → "notes-changed" イベントをフロントへemit
  notes/
    mod.rs                 Note / NoteSummary 型
    naming.rs                ファイル名生成（タイムスタンプ+スラッグ、衝突時は _2, _3...）
    tags.rs                   本文から #tag を抽出（正規表現非使用、Unicode対応）
    scan.rs                    フォルダ→Vec<Note>。ファイル名からの日時パース＋フォールバック
    store.rs                    create/update/set_status(rename)/delete の実ファイル操作
    search.rs                    キャッシュに対するインメモリのフィルタ+全文検索（ディスク再走査なし）
  commands.rs             #[tauri::command] 一覧（次のセクション）
```

### コマンド一覧（フロントからの呼び出し方は `src/api.ts`）
`list_notes(filter)` / `get_note(filename)` / `create_note(body, status)` /
`update_note(filename, body)` / `set_status(filename, status)` / `delete_note(filename)` /
`open_notes_folder()` / `get_settings()` / `set_settings(next)` / `hide_window()`。

いずれも Phase 2 でシグネチャ変更なし。`hide_window` は Phase 2 で「隠す前に `window-hiding`
イベントをemitしてからhideする」処理に一本化する（後述）。

## UI設計（Phase 2 — 実装中）

主体は「書く」ことなので、**デフォルト画面は全画面のcompose（作文）画面**。検索は専用画面に
切り離し、`Ctrl+F` で行き来する。

### 画面はステートマシン（`App.tsx` の `view`）
`view: "compose" | "search" | "settings"`。デフォルト `"compose"`。

- **compose**: 全画面 `<textarea>`（`flex:1`）＋一番下にKeep/Scratchスイッチ。検索バーは常駐しない。
- **search**: `Ctrl+F` で入る。検索入力＋Keep/Scratch/Allチップ＋結果一覧。矢印キーで選択移動、
  Enter/クリックで選択中メモをcompose画面に「開く」（インライン編集は廃止）。設定ギアもここ。
- **settings**: 既存の `SettingsPanel`。`Ctrl+,` からも直接遷移。

### compose の状態（`App.tsx`）
```
composeBody: string          // 制御textareaの値
composeStatus: "keep" | "scratch"
openFilename: string | null  // null=未保存の新規メモ、非null=既存メモを編集中
lastSavedBodyRef              // dirty判定用（直前に保存した内容）
loadToken / pendingSelection  // 検索から開いた時のカーソル位置復元用
```

### キーボード操作
- **`Ctrl+S`（一時保存）**: 空欄なら書き込みせず新規状態に戻す（安全策、既存メモを空で上書きしない）。
  非空なら `openFilename` の有無で create/update を選択。**textareaはクリアしない。**
- **`Ctrl+Enter`（保存して閉じる）**: `Ctrl+S`と同じ書き込み後、`composeBody=""`, `openFilename=null`
  にリセット（空欄なら書き込みなしでリセットのみ）。
- **ウィンドウが隠れる瞬間の自動一時保存**: dirty（`composeBody !== lastSavedBodyRef.current`）かつ
  非空の場合のみ `Ctrl+S`と同じ書き込みを実行。Rust側で「隠す前に必ず `window-hiding` イベントを
  emit」するよう一本化し（`toggle_window` / `spawn_focus_watch` の hide-on-blur / `CloseRequested` /
  `commands::hide_window` の4箇所すべて）、フロントは `listen("window-hiding", flushDraft)` を
  1回登録するだけでよい。ウィンドウ非表示後もWebViewのJSは生き続けるため、emit直後にhideしてよい
  （非同期保存はバックグラウンドで完了する）。
- **Escapeの階層**（1回ごとに1段階戻る）: `search`→`compose`。`settings`→`compose`。`compose`で
  既存メモ編集中→保存せず新規状態に戻す。`compose`で既に新規状態→`hideWindow()`。
- **`Ctrl+F`**: どの画面からでも検索画面へ。`preventDefault()` 必須（WebView2標準の検索バー抑止）。
- **`Ctrl+K`（Keep/Scratch切り替え）**: compose画面では現在の状態をトグル（既存メモなら即
  `setStatus`実際に移動、新規メモならローカルの選択のみ変更）。search画面ではキーボード選択中の
  行に対して同じトグルを行う。settings画面では無効。

### 検索ヒット→compose画面へのカーソル位置復元
バックエンド変更なし。`getNote`で本文取得後、フロントで
`body.toLowerCase().indexOf(query.trim().toLowerCase())` を計算し、ヒットすれば
`{start, end}` を `ComposeBox` に渡して `setSelectionRange`。見つからなければ先頭 (`0,0`)。
Rustのバイト位置とJSのUTF-16インデックスはズレるため、オフセット計算は**必ずクライアント側**で行う。

## Keep/Scratch誤認防止の配色（Phase 4）

「Keepだと思って編集してたらScratchだった」事故を防ぐため、compose画面のクロム（枠線・背景の
薄い色味・ツールバー境界線・Keep/Scratchスイッチのアクティブ色・保存ボタン）を、現在の
`composeStatus`に応じて色分け。Keepは深みのあるワインレッド、Scratchは深みのある濃紺
（`styles.css`の`--keep-accent`/`--scratch-accent`とそのwash版、ライト/ダーク両対応）。

- `App.tsx`: `.app`ルートに`data-status={view==="compose" ? composeStatus : undefined}`を付与。
  search/settings画面では属性なし＝中立色（`--status-accent`未定義→各所のCSSフォールバックで
  通常の`--accent`/`--line`に戻る）。
- `styles.css`: `.app[data-status="keep"|"scratch"]`が`--status-accent`/`--status-wash`を
  定義。本文の可読性を落とさないよう、textarea自体（`.compose-input`）は透明のままで、その
  親`.compose`の背景にごく薄いwashを敷く方式（文字色・コントラストには一切触れない）。

## タグをトピック軸として使う（Phase 3）

「軸B（トピック）はフォルダではなくタグ or 検索に任せる」を、実際にクリックで辿れる形にした。
バックエンド変更なし（`Note.tags`は既存、`list_notes`の`query`は既にタグにも部分一致する）。

- **タグクラウド**（`SearchView.tsx`）: 検索欄が空のとき、全メモ（keep/scratch問わず）を横断した
  タグの頻度一覧をチップで表示。`listNotes({})`を検索画面に入るたびに1回取得して集計するだけ
  （専用バックエンドコマンドは作らない、この規模なら十分速い）。クリックすると
  `statusFilter="all"` にリセットしつつ `query` にそのタグ名をセット — 通常の全文検索経路を
  そのまま使う。
- **検索結果一覧のタグ**（`NoteListItem.tsx`）: 各行のタグピルがクリック可能になり、同じ
  「query にセットして絞り込む」動作。
- **compose画面のタグプレビュー**（`ComposeBox.tsx`）: 書いている本文からクライアント側で
  `#tag` をリアルタイム抽出（`api.ts`の`extractTags` — Rust側`notes::tags::extract`のJS版、
  表示専用でファイルへの書き込みには関与しない）してピル表示。クリックすると検索画面へ
  ジャンプしそのタグで絞り込む（`App.tsx`の`searchForTag`）。

### コンポーネント構成（Phase 2 後）
- `ComposeBox.tsx` — 制御コンポーネント化。全画面レイアウト。`onSaveDraft`/`onSaveAndClose` を
  親から受け取る。`openTitle` で編集中メモ名を表示。`loadToken`/`pendingSelection` でカーソル復元。
- `SearchView.tsx`（新規）— 検索入力＋チップ＋ギア＋結果一覧＋`selectedIndex`管理＋矢印キー/Enter。
  `SearchBar.tsx` を統合して置き換え（削除）。
- `NoteList.tsx` / `NoteListItem.tsx` — インライン編集を除去。`selected`（キーボード選択中の
  ハイライト）と `onOpen` を追加。Keep/Scratchボタン・削除confirmは維持。
- `NoteEditor.tsx` — 削除（compose画面が編集画面を兼ねる）。
- `SettingsPanel.tsx` — 変更なし。

## 実装状況

- [x] Phase 1: バックエンド一式、フロント初期版（検索バー常駐＋インライン編集の旧UI）
- [x] Phase 2 step 1: このDESIGN.md作成
- [x] Phase 2 step 2: Rust `lib.rs` に `hide_window` ヘルパー集約（`window-hiding` イベント）。
      `toggle_window` / `spawn_focus_watch`(hide-on-blur) / `CloseRequested` / `commands::hide_window`
      の4箇所すべてがこの一本化ルート経由。`cargo check` / `cargo test` 通過。
- [x] Phase 2 step 3: `ComposeBox.tsx` 制御化＋全画面化＋`loadToken`/`pendingSelection` によるカーソル
      復元対応。Ctrl+S/Ctrl+Enter/Ctrl+K はこのコンポーネント内でキー処理（IMEガード込み）。
- [x] Phase 2 step 4: `SearchView.tsx` 新規作成（`SearchBar.tsx` を統合）、`NoteList`/`NoteListItem`
      からインライン編集を除去し `selected`/`onOpen` に変更。`SearchBar.tsx`/`NoteEditor.tsx` 削除済み。
- [x] Phase 2 step 5: `App.tsx` 書き直し完了。`view` ステートマシン、compose状態管理
      （`composeBody`/`composeStatus`/`openFilename`/`openTitle`/`lastSavedBodyRef`）、Escape階層、
      `Ctrl+F`/`Ctrl+S`/`Ctrl+Enter`/`Ctrl+K`/`Ctrl+,` ショートカット、`window-hiding` リスナー
      （`flushDraftRef` 経由で常に最新のクロージャを参照）実装済み。
- [x] Phase 2 step 6: `styles.css` 調整済み（`.compose` 全画面化、`.search-view`、
      `.note-row-selected`、旧 `.note-editor` 系スタイル削除）。
- [~] Phase 2 step 7: 検証。自動チェックは完了 — `cargo check`/`cargo test`/`tsc --noEmit`/
      `vite build` すべて通過、`npm run tauri dev` で起動しウィンドウ表示・ホットキーでの
      トグル（実際に `Ctrl+Alt+N` で表示⇄非表示を確認済み）・`Documents\Rnote` 直下への
      `keep`/`scratch` 生成を確認済み。**plan記載の8項目のうち、実際にキー入力してのCtrl+S/
      Ctrl+Enter/空欄セーフティ/Ctrl+F遷移/検索→compose復元/Escape階層/Ctrl+Kトグルの手動確認は
      未実施**（ユーザー環境で実際に操作しての確認が必要）。

- [x] Phase 3: ファイル名を `YYYY-MM-DD_HHmm - スラッグ.md` に変更（`naming.rs`/`scan.rs`、
      テスト更新済み、`cargo test`通過、既存ファイルも読める）。タグクラウド・検索結果のタグ
      クリック・composeのタグプレビューをすべて実装。`tsc --noEmit`/`vite build`通過、
      dev環境へホットリロードでエラーなく反映済み。
- [x] Phase 4: Keep/Scratch誤認防止の配色（ワインレッド/濃紺）。`vite build`通過、dev環境へ
      ホットリロードでエラーなく反映済み。

## 次にやること（再開時にまずここを見る）

- Phase 2 step 7 の残り: ユーザー自身が触って、DESIGN.md記載の挙動（特にCtrl+S/Ctrl+Enterの
  一時保存⇄保存して閉じるの違い、検索→compose復元時のカーソル位置、Escapeの段階的な戻り方、
  Ctrl+Kでのkeep/scratch切り替え、タグクラウド/タグクリックでの絞り込み）が意図通りか確認して
  もらう。
- 問題があれば挙動を修正し、このチェックリストを更新する。
- 既知の未対応事項（要望が出ない限り対応不要）: 削除のソフトデリート/ゴミ箱化、`notesDir` 変更時の
  既存メモ自動移行、アプリアイコンの差し替え（まだRclip由来のまま）。

## R経済圏ローカルAPI（`api.rs`）

Rhub 等の他 R アプリからメモを引く／作るためのループバック HTTP API。
Rfinder / Rclip の `api.rs` と構造は同一（`route` はメソッド＋パスで分岐、POST は body を読む）。

- `GET /v1/notes?q=&status=keep|scratch&limit=&offset=` → `{ apiVersion, total, items }`。
  各 item は `NoteSummary` ＋ 解決済み絶対 `path`（`notesDir` を知らなくても開ける追加フィールド）。
  `notes::search::apply` をそのまま呼び、ページングだけ `api.rs` 側でスライス。
- `POST /v1/notes { content, tag?, status? }` → 作成した `Note`（＋ `apiVersion`）。Rhub のレシピ
  `rnote.create` ステップ用。`tag` は本文に既出でなければ末尾に `#tag` 行として追記
  （Rnote のタグは本文内 `#tag`）。`status` は `scratch`（既定）/ `keep`。内部は
  `notes::store::create` ＋ `state.notes` へ push（`commands::create_note` と同じ手順）。
- `GET /v1/health` は認証不要（名前とバージョンのみ）。それ以外は `%APPDATA%\R\token` の
  Bearer トークン必須。127.0.0.1 のみ、CORS ヘッダなし。
- 既定ポート `41440`（vite dev ポート + 40000）。`config.json` の `apiPort` / `apiEnabled` で上書き。
  bind 失敗時は API を無効化してアプリ起動は続行。
- 起動時に `%APPDATA%\R\services\rnote.json` を書き、終了時（新設した `RunEvent::Exit` アーム）に削除。
