# 吉里吉里の追加対応と検証 (2026-10-27)

SDRS の自動展開に続き、handoff の残件だった cxdec、Pad、data.xp3 のない構成を確認した。
体験版の元の EXE と XP3 は、SDRS による明示的な展開を除いて変更しない。

## 追加した読み込み経路

| 対象 | 処理 |
|---|---|
| SDRS | devserver がエンジン判定前に `vnrun-prep::prepare` を呼ぶ。フォルダ直下の EXE を検出・展開し、元ファイルはバックアップに退避 |
| Akabei / Mado | PNG・Ogg・TLG のヘッダーから種を復元し、32/31バイト周期を完全な2ファイルの Adler-32 で判定 |
| TPM の cxdec | Yuyukana / Lavender の制御表をゲームのプラグインから読み、検証済みの算術プログラムを評価 |
| EXE 内蔵 cxdec | prep が PE の bres リソースを読み、SHA3-384 と ChaCha8、zlib を使ってパラメーターと制御表を取り出す |
| 平文の日本語スクリプト | Adler-32 が既に一致するファイルを XOR の文字種推定から除外 |
| data.bin | XP3 の11バイト署名を確認してエンジンを検出し、メインアーカイブとして選択 |
| EXE 内蔵 XP3 | 16バイト境界の署名と索引位置を確認し、EXE をプロジェクトとして選択 |

cxdec は Windows の EXE や DLL を実行しない。
TPM の方式は既知の2種類と Lavender の製品版候補を対象とする。
EXE 内蔵の方式は、22バイトの PARAMS と4,096バイトの TABLE を持つ classic LCG 版が対象で、他の乱数生成器は採用しない。

prep が生成する `vnrun-cxdec.bin` は、8バイトの `VNCXDEC1`、22バイトの PARAMS、4,096バイトの TABLE からなる。
生成済みと同じ内容なら書き直さない。
ブラウザは各アーカイブ内の完全な2ファイルを復号し、Adler-32 が索引と一致する場合だけ使う。
ゲームごとの固定鍵や制御表はリポジトリに含めない。

bres の処理は [xp3-brute の krkr_exe.rs](https://github.com/xmoezzz/xp3-brute/blob/c82cb14302138125640478556723705aa720007b/src/krkr_exe.rs) を参照した。
cxdec の算術評価は [GARbro の KiriKiriCx.cs](https://github.com/morkt/GARbro/blob/master/ArcFormats/KiriKiri/KiriKiriCx.cs) を元にしている。
いずれも MIT の表示を実装側に残している。

## 復号結果の照合

| 対象 | 保護対象ファイル | 結果 |
|---|---:|---|
| ATATA、墓多 DYING ZOMBIES、ROLEPLAYER 2本、はっしゃくさま | 21,896 | 全件一致 ([Akabei の記録](xp3-akabei.md)) |
| ユユカナ | 4,410 | 全件一致 |
| 光輪の町、ラベンダーの少女の体験版 | 6,030 | 全件一致 |
| 花鐘カナデグラム Chapter1 の data.xp3 | 1,955 | 全件一致 |
| おしおき生意気ギャル | 749 | 全件一致 |
| 合計 | 35,040 | 不一致なし |

チェックサムの一致は、シナリオ全編・セーブ・動画の動作保証を意味しない。
タイトルまで進んだ作品の本編全体は、今回通していない。

## Pad とスクリプトの互換性

`new Pad()` はブラウザのモーダル編集画面に対応する。
内容を UTF-16LE の BOM 付きで保存し、コピーオンライトの GameFS を通してサーバーに送る。
保存先はゲームフォルダ内のパスに限る。
保存に失敗した場合は失敗を表示し、再試行できる。
ダイアログのキー入力はゲームに渡さず、閉じたらゲームのキャンバスにフォーカスを戻す。

公式 KAG3 の syoukai デモで、Config の編集・保存、再起動後の読み込み、紹介本文までの進行を確認した。
保存済み Config の長さが元のファイルと違っていても、Emscripten へのマウントは保存済みの長さを使う。

KAGParser は `@... cond=mp.bg === void` のような、引用符のない末尾の比較式を読む。
`cond=f.ready page=fore` のように次の属性がある場合は分けて読む。
開発機の `H:/...` などを残した検索パスは無視する。
Motion を組み込みクラスとして期待する作品には既存の motionPlayer スタブを先に読み込む。
これは E-mote の描画対応を追加するものではない。

## data.xp3 のなかった配布物

8本でタイトルメニューまで確認した。
具体的な ID と結果は [verification.md](verification.md) と `tools/verify/status.tsv` にある。

- ことのはアムリラートは EXE 内蔵 XP3 を選択する。
- 生命のスペアと少女と年の差、ふたまわり。は data.bin を使う。
- 魔法少女沙枝 Vol.2 は配布物の data1.cab を展開した。
- ソウルレイザー☆ユナは InstallShield 内の MSI/CAB を展開し、MSI の File テーブルに従って名前を戻した。
- G線上の魔王の2体験版は Inno Setup を展開した。
- 恋嵐スピリッチュの対象 URL は差分パッチのみで、起動用本体を含まないため対象外とした。

インストーラの展開は検証時の事前作業であり、devserver にインストーラ全般の自動展開を実装したわけではない。

## 再現用テスト

元のゲームを必要としない検証を用意した。

```bash
cargo test --locked -p vnrun-prep -p vnrun-devserver
cargo clippy -p vnrun-prep -p vnrun-devserver --all-targets -- -D warnings
bash engines/krkr/test.sh
```

Rust は SDRS の検出・バックアップ・再実行・失敗時の保持、合成 PE のリソース抽出、破損・未対応モードの拒否、data.bin の検出を確認する。
C++ は固定ベクトル、cxdec の全128プログラム、分割読み込み、32ビットを超えるオフセット、PARAMS の検証を確認する。
Rust の13件、C++、合成 XP3 の4構成と補助ウィンドウ入力、Pad のブラウザテストが通った。
Emscripten 4.0.23 の wasm ビルド、`vp check --fix`、`vp build` も通った。
`vp check` は既存の wgpu-compat.ts に unbound-method の警告が2件あり、エラーはない。
CI は Rust と C++ のテストを実行する。

以下のブラウザテストには、`tools/shot.js` と同様に Node.js から参照できる Playwright と Chromium が必要になる。
`VNRUN_CHROMIUM` で Chromium の実行ファイルを指定できる。

```bash
# KiriKiri wasm とページをビルドした後に実行する。
cargo build -p vnrun-devserver
node tools/verify/krkr-smoke.cjs
```

このテストは `target/krkr-smoke/` に5種類の合成 XP3 を生成し、実際の wasm で読む。
圧縮データ、Mado / Akabei、TLG のみのアーカイブ、平文 UTF-8、KAG の末尾条件式と後続属性を確認する。
ログも同じディレクトリに残す。

```bash
# 別ターミナルで `cd web && npx vp dev --port 5173 --strictPort` を実行しておく。
node tools/verify/krkr-pad.cjs http://127.0.0.1:5173
```

Pad のテストは HTTP 503 からの再試行、日本語の UTF-16 保存、保存後のファイルサイズ、キー入力の分離、Escape とフォーカス復帰、範囲外パスの拒否を確認する。

## 入力と PIMG の表示

非表示の補助ウィンドウを後から作ると、ページのマウス入力がそのウィンドウに送られていた。
入力先を最後に描画したウィンドウへ合わせ、NoStrike の言語選択を操作できるようにした。
合成ゲームでも、非表示ウィンドウの作成後に表示中のウィンドウへ正しい座標が届くことを確認する。

KAGEX は設定済みの PIMG ソースを複製し、`initOptions` を呼んで再利用する。
継承元のビットマップ用初期化だけでは PSD の表示状態が残り、ロゴ専用ソースから背景を複製すると画像全体が透明になっていた。
psbFile のポリフィルで表示状態と描画済み状態を初期化し、ファイル内の可視状態へ戻す。
通常のビットマップの初期化と、複製元の状態を保持する回帰テストも追加した。
最終ビルドで NoStrike の2024年・2026年の2本は言語選択からタイトルへの遷移を確認した。
花鐘カナデグラムは背景・キャラクター・メニュー・花びらの描画を確認した。

scriptsEx は [公式実装](https://github.com/wamsoft/scriptsEx/blob/master/Main.cpp) に合わせ、キーをソートし、辞書をキー単位で比較する。
`foreach` は false・0・null を含む非 void の戻り値で停止する。
複製は配列・辞書を再帰的に処理し、その他のオブジェクトは clone メソッドまたは元の参照を使う。

## 残っている制約

- 動画、E-mote、本編全体、各作品のゲーム内セーブは今回の検証範囲外。
- 正午の handoff にある「回帰サンプル12本」の ID 一覧はこの環境にないため、同じ12本を再実行したとは扱わない。
  最終 wasm では Akabei の5本、ユユカナ、ラベンダー、ことのはアムリラートの計8本を再起動し、タイトルを確認した。
- 2026-10-27 にローカルで CI と同じ全ワークスペースの `cargo fmt --check` と `cargo clippy --release -- -D warnings` を確認したが、今回の変更前からある `luavm`、`nexas`、`yuris` などの書式・Clippy エラーで失敗する。
  対象の `vnrun-prep` と `vnrun-devserver` の書式・Clippy とテストは通る。
- push とホスト CI の実行はしていない。
