# 引き継ぎメモ (2026-10-24 時点)

vnrun の目的、決まったこと、今ある実装、まだ確かめていないことをまとめる。最初のスレッド「独自エンジンをブラウザで動かす」(vnbox プロジェクト) の内容を、次に作業する人 (Claude を含む) のために残したもの。

## 目的と前提

- [vnbox](https://github.com/a-kaibu/vnbox) は Windows のノベルゲームを Linux ホストの Wine で動かし、WebRTC で配信している。ホストが非力で、今の配信でも重い。
- そこで **ゲームをブラウザ側で動かし、ホストはファイルとセーブを配るだけにする**。そのためのプレイヤーとエンジン群が vnrun。
- 個人で遊ぶだけ。再配布はしない。
- **DRM (PlayDRM、ソフト電池など) の解析・解除・移植はしない**。ユーザーから「DRM 部分も移植すれば」と提案があったが断り、ユーザーも了承した。DRM 付きの製品版は vnbox の Wine で正規の認証を通して遊ぶ。vnrun の対象は体験版と DRM 無しの作品。
- エンジン自体のアーカイブ暗号 (吉里吉里の xp3、Siglus の二次鍵など) は私的利用の範囲で扱う前提。法的にはグレーが残ることはユーザーに伝えてある。
- Unity と一般的なエンジンは対象外 (Wine のまま)。

## 決まったこと

- vnrun は vnbox と別リポジトリ。**新しく書く部分は Rust** (wasm-bindgen)。
- 既存の Rust 実装は git submodule で取り込み、つなぎの層で共通の口に合わせる。C/C++ で完成している実装 (吉里吉里、ONScripter など) は書き直さず Emscripten で wasm にする。
- vnbox (Go) はサーバー側のまま。パッケージ化、Wine での代替実行、ファイルとセーブの配信、S3。
- エンジン間の抽象化は 2 段 (下の「構成」)。

## 構成

```
web/                 プレイヤーのページ (エンジンに依らない部分)
  player.js          ?game=<URL> のゲームを開き、manifest.json の engine から engines/<id>.js を選んで渡す
  vfs.js             GameFS: ファイル一覧・同期読み込み (同期 XHR + 256MB の LRU)・セーブの上書き表示と PUT/DELETE
  engines/siglus_rs.js  siglus_rs のつなぎ (Siglus は start_siglus_from_directory、RealLive/AVG32/UK2 は WebGame のフレームループ)
engines/<id>/build.sh  エンジンを wasm にして dist/engines/<id>/ に置く
third_party/siglus_rs  submodule (5f358ef、1.0.0)
crates/devserver     手元で 1 本のゲームフォルダを配る Rust のサーバー (tiny_http)
docs/protocol.md     プレイヤーとサーバーの約束事
```

- **ページ側の抽象**: エンジンのモジュールは `start({fs, manifest, ui, fonts})` だけを持つ。ファイル・セーブ・UI (スキップ、メニュー、終了)・エラー表示・フォントは共通部分が持つ。新しいエンジンは `ENGINES` (player.js) に id を足し、`engines/<id>.js` を書く。
- **Rust 側の抽象 (未着手)**: 自作エンジン用の共通 crate に「ゲームのファイル (GameFS)」「入力」「描画面」「音声」の trait を置き、各エンジンはそれを実装する。取り込み品は JS のつなぎで合わせる。siglus_rs は自前で winit のイベントループを回すので、JS 側でしか揃えられない。
- siglus_rs は wasm から `siglusFileExists / siglusReadFile / siglusListDir / siglusKnownFileCount / gameFsWrite / gameFsRemove` という JS のグローバル関数でファイルを読む (crates/game_fs、crates/siglus_scene_vm/src/wasm_vfs.rs)。**読み込みはファイル丸ごと**で、範囲読みは無い。

## vnbox 側 (PR [a-kaibu/vnbox#2](https://github.com/a-kaibu/vnbox/pull/2))

- `launch.ini` に `runner=vnrun` と `root=` (エンジンのファイルがあるフォルダ、drive_c からの相対)。
- `vnbox add` が SiglusEngine / RealLive (`Scene.pck` / `Seen.txt` がある) で DRM 無しのものに自動で付ける。追加済みには `vnbox browser -all`。
- `internal/webplay`: `/player/` (vnrun の dist、`vnbox serve -player` / `VNBOX_PLAYER` / 既定 `<データ>/player`)、`/player/fonts/` (ホストの IPA / Noto CJK)、`/p/<name>/manifest.json|files/|saves/`。
- セーブは `<データ>/websaves/<name>/`、S3 があれば書き込み停止 30 秒後に `websaves/<user>/<name>/` へ。
- ギャラリーと詳細画面に「ブラウザで遊ぶ」。Wine 起動は「Wine で遊ぶ」として残る。
- 別スレッドの PR #1 (WebRTC の遅延・負荷チューニング) とは独立。

## 確認済みのこと / まだのこと

- 確認済み: siglus_rs の wasm ビルド (約 19MB、gzip で約 8MB)。devserver と vnbox のポータルの両方で、Chromium から「マニフェスト → wasm 起動 → VFS 経由のファイル読み込み → セーブの PUT」まで通る。
- **実物の体験版での確認 (2026-10-24、devserver + ヘッドレス Chromium、`tools/shot.js`)**:

  | 作品 | エンジン | 結果 |
  |---|---|---|
  | シオンの血族 体験版 (`http://suezou.dyndns.org/dl2011/catwalk/sion/sion_try.zip`) | Siglus | 本編の文章まで。ボイスあり |
  | プリマドール 体験版 (`https://va-trialdist.azureedge.net/trial-primadoll-1.zip`) | Siglus | タイトル画面まで。二次鍵が要る (vnrun-prep で用意) |
  | お姉さん×すくらっち 体験版 (`http://suezou.dyndns.org/dl2008/hadashi/scratch2/trial.lzh`) | RealLive | 本編の文章まで |
  | 聖炎天使エレアノール 体験版 (`http://suezou.dyndns.org/dl2007/sagaplanets/eleanor/EleanorTrial.lzh`) | RealLive | タイトル画面まで。メニューのボタンがクリックにもキーにも反応しない (未調査、siglus_rs 側) |

- 実物で見つかって直したもの (`patches/siglus_rs/`、build.sh が適用する):
  - `0001`: ボイスの時間圧縮 (JITAN) を `std::thread::spawn` でやっていて、wasm では panic してタイトル画面で落ちた。wasm ではその場で処理する。
  - `0002`: wasm の起動部が画面を 1280x720 に固定し、キャンバスの大きさをそのままゲームの座標にしていた (1024x576 の作品は左上に小さく、1920x1080 の作品は切れる)。Gameexe の SCREEN_SIZE をゲームの座標にして、キャンバスに縦横比を保って拡大し、ポインタの座標も換算する (Android / iOS 版と同じやり方)。
- **Siglus の二次鍵はブラウザでは探さない** (siglus_rs の wasm ビルドは key.toml を読むだけ)。`vnrun-prep <ゲームのフォルダ>` (crates/prep、ネイティブ) が鍵を復元して key.toml を書く。プリマドールで 1 秒以下。vnbox は `vnbox add` のときにこれを呼ぶ想定 (未実装)。
- movie (OMV / MPEG2 / WMV) の再生も siglus_rs の中で `thread::spawn` を使う箇所が残っている。ムービーのある作品で同じ panic が出るはずなので、出たら同様に直す。
- 気にしておくこと: 丸ごと読みなので、大きい音声アーカイブ (Siglus の .ovk など) で同期 XHR が重くなる可能性。必要なら siglus_rs 側に範囲読みを足す。音声の自動再生制限 (最初のタップ) の扱いも未確認。

## エンジンと実装の対応

本数は vnbox で動作確認した 690 本での数。

| エンジン | 本数 | 実装 | 言語 | Web | メモ |
|---|---|---|---|---|---|
| SiglusEngine | 18 | [xmoezzz/siglus_rs](https://github.com/xmoezzz/siglus_rs) | Rust | あり | 組み込み済み。二次鍵は総当たりで探して key.toml に書く |
| RealLive / AVG32 | 8 | 同上 | Rust | あり | 組み込み済み。文字にホストのフォントが要る |
| BGI | 21 | [xmoezzz/ethornell-rs](https://github.com/xmoezzz/ethornell-rs) | Rust | あり (WebGL2) | 千の刃濤の体験版がブラウザで動いた画像あり。2026-10-23 更新 |
| Softpal | 31 | [xmoezzz/sena-rs](https://github.com/xmoezzz/sena-rs) | Rust | あり | README に「特に『Koikake』1 作品が対象」。他作品は差分を埋める前提 |
| FVP | 6 | [xmoezzz/rfvp](https://github.com/xmoezzz/rfvp) | Rust | あり (platform/wasm) | 全バージョン互換を目指すと記載 |
| Artemis | 5 | [Alphaly2K/art3m1s-core](https://github.com/Alphaly2K/art3m1s-core) | Rust | 無し | Flutter 版あり。商用作品の互換表なし |
| 吉里吉里 | 133 | krkrz / [krkrsdl2](https://github.com/krkrsdl2/krkrsdl2) (Web 版あり)、Kirikiroid2 | C++ | あり | krkrsdl2 は「未改変の商用ゲームは非対応」と明記。下の xmoezzz の部品が使える |
| NScripter | 4 | [OnscripterYuri](https://github.com/YuriSizuku/OnscripterYuri)、onscripter-ru-wasm | C++ | あり | |
| AliceSoft System4 | 3 | [xsystem4](https://github.com/kichikuou/xsystem4)、system3-sdl2 (Web 版あり) | C | System3 のみ | |
| YU-RIS | 29 | なし | | | 自作候補 |
| NeXAS | 26 | なし | | | 自作候補 |
| AdvHD | 18 | なし | | | |
| LiveMaker / WillPlus | 各 9 | なし | | | |
| CatSystem2 / Escude / EntisGLS | 各 8 | なし | | | |

Rust の土台があるのは計 89 本 (Siglus 18、RealLive 8、BGI 21、Softpal 31、FVP 6、Artemis 5)。

### xmoezzz さんのリポジトリ (GitHub API で 2021-11 以降を確認)

エンジンの実行系:
- siglus_rs (Siglus/RealLive/AVG32/UK2)、ethornell-rs (BGI)、sena-rs (Softpal)、rfvp (FVP)
- shin は DCNick3/shin のフォーク (Entergram / 07th Expansion のコンソール版向け。vnbox の対象外)

吉里吉里の部品 (Rust の実行系はまだ無い):
- [eluna](https://github.com/xmoezzz/eluna): E-mote / PSB の解析と再実装 (Rust)。商用の吉里吉里作品で多い E-mote プラグインの代わりになりうる
- [xp3-brute](https://github.com/xmoezzz/xp3-brute): xp3 の展開・作成 (Rust)
- [tjs2Decompiler](https://github.com/xmoezzz/tjs2Decompiler): TJS2 バイトコードの読み込みと逆コンパイル (Rust)
- [amv_decoder](https://github.com/xmoezzz/amv_decoder): 吉里吉里の AMV 動画 (Rust)。AlphaMovieDecoder (Common Lisp) もある
- KrkrzExtract (C)、KrkrExtract (C++、アーカイブ済み)

その他の部品: omvdecoder (Siglus の OMV 動画)、theora-rs、wmvkit (ASF/WMV の分離)、mp4_openh264、artemis_ast (Artemis の AST スクリプト編集、Rust)、ShinkuToolset (FVP の翻訳)、cmvs-engine (Purple software CMVS の展開ツール、2017)、siglus_static_key_tool、SiglusExtract。

中身が分からなかったもの: CrySirror (説明なし、2026-10-24 更新)、gow3-rs、d_Failure_Information。2021-11 より古いページ (API の 10 ページ目以降) は見ていない。

## 決まった順番と次にやること

1. **実物の体験版で siglus_rs を確認** (Siglus と RealLive)。vnbox の PR #2 も実機で通す。
2. **ethornell-rs (BGI) と sena-rs (Softpal) を取り込む**。submodule + `engines/<id>/build.sh` + `web/engines/<id>.js`、vnbox の `webplay.Engines` と `markers` にも足す。
3. rfvp (FVP)、NScripter (OnscripterYuri を Emscripten で)。
4. 吉里吉里: krkrz を Emscripten で wasm にし、xmoezzz の eluna / xp3 / AMV などで商用作品のプラグインを埋める。まず 133 本が使うプラグイン DLL を数えて多い順に。
5. Artemis (art3m1s-core に Web のつなぎを書く)。
6. YU-RIS / NeXAS などは vnrun で Rust で書く (共通 crate の trait を先に作る)。

## 未決のこと

- Rust 側の共通 trait の具体的な形 (描画は WebGL2 か wgpu か、音声の取り回し)。最初の自作エンジンで決める。
- エンジンの wasm をどこで配るか。今は `./build.sh` で手元ビルド。GitHub Actions でビルドして Releases に置き、vnbox が落とす形 (Wine と同じ) が候補。
- ゲームのファイルの大きいものを丸ごと読む問題 (上記)。
