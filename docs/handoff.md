# 引き継ぎメモ (2026-10-27 更新)

vnrun の目的、決まったこと、今ある実装、まだ確かめていないことをまとめる。最初のスレッド「独自エンジンをブラウザで動かす」(vnbox プロジェクト) の内容を、次に作業する人 (Claude を含む) のために残したもの。

## 目的と前提

- [vnbox](https://github.com/a-kaibu/vnbox) は Windows のノベルゲームを Linux ホストの Wine で動かし、WebRTC で配信している。ホストが非力で、今の配信でも重い。
- そこで **ゲームをブラウザ側で動かし、ホストはファイルとセーブを配るだけにする**。そのためのプレイヤーとエンジン群が vnrun。
- 個人で遊ぶだけ。再配布はしない。
- PlayDRM などの DRM 付き製品版は vnbox の Wine で正規の認証を通して遊ぶ。
  SDRS の事前展開は、2026-10-26 に追加した `sdrs-core` の前処理で対応する (下の再開後の記録)。
- エンジン自体のアーカイブ暗号 (吉里吉里の xp3、Siglus の二次鍵など) は私的利用の範囲で扱う前提。法的にはグレーが残ることはユーザーに伝えてある。
- Unity と一般的なエンジンは対象外 (Wine のまま)。

## 決まったこと

- vnrun は vnbox と別リポジトリ。**新しく書く部分は Rust** (wasm-bindgen)。
- 既存の Rust 実装は git submodule で取り込み、つなぎの層で共通の口に合わせる。C/C++ で完成している実装 (吉里吉里、ONScripter など) は書き直さず Emscripten で wasm にする。
- vnbox とは独立したプロジェクト。ファイルとセーブの配信も vnrun が持つ (下の「vnbox とは独立」)。
- エンジン間の抽象化は 2 段 (下の「構成」)。

## 構成

```
web/                 プレイヤーのページ (Vite+ + TypeScript、エンジンに依らない部分)
  player.js          ?game=<URL> のゲームを開き、manifest.json の engine から engines/<id>.js を選んで渡す
  vfs.js             GameFS: ファイル一覧・同期読み込み (同期 XHR + 256MB の LRU)・セーブの上書き表示と PUT/DELETE
  engines/siglus_rs.js  siglus_rs のつなぎ (Siglus は start_siglus_from_directory、RealLive/AVG32/UK2 は WebGame のフレームループ)
engines/<id>/build.sh  エンジンを wasm にして web/public/engines/<id>/ に置く
third_party/siglus_rs  submodule (5f358ef、1.0.0)
crates/devserver     手元で 1 本のゲームフォルダを配る Rust のサーバー (tiny_http)
docs/protocol.md     プレイヤーとサーバーの約束事
```

- **ページ側の抽象**: エンジンのモジュールは `start({fs, manifest, ui, fonts})` だけを持つ。ファイル・セーブ・UI (スキップ、メニュー、終了)・エラー表示・フォントは共通部分が持つ。新しいエンジンは `ENGINES` (player.js) に id を足し、`engines/<id>.js` を書く。
- **Rust 側の抽象 (未着手)**: 自作エンジン用の共通 crate に「ゲームのファイル (GameFS)」「入力」「描画面」「音声」の trait を置き、各エンジンはそれを実装する。取り込み品は JS のつなぎで合わせる。siglus_rs は自前で winit のイベントループを回すので、JS 側でしか揃えられない。
- siglus_rs は wasm から `siglusFileExists / siglusReadFile / siglusListDir / siglusKnownFileCount / gameFsWrite / gameFsRemove` という JS のグローバル関数でファイルを読む (crates/game_fs、crates/siglus_scene_vm/src/wasm_vfs.rs)。**読み込みはファイル丸ごと**で、範囲読みは無い。

## vnbox とは独立 (2026-10-24 決定)

- ユーザーの判断で **vnrun は vnbox から完全に独立** させる。ゲームファイルの配信もセーブの保存も vnrun 自身が持つ。vnbox への組み込み (旧 PR a-kaibu/vnbox#2、`runner=vnrun` / `internal/webplay`) は取りやめ、マージしない。
- 今は `crates/devserver` が 1 本のゲームフォルダを配り、セーブを `saves/<name>/` に置く。これを複数タイトルの一覧・セーブ管理を持つ vnrun のサーバーに育てる (旧 PR の webplay の作り、つまりマニフェスト・Range・セーブの原子的な書き込み・容量上限が参考になる)。
- vnbox (Wine + WebRTC) は DRM 付き作品などの別系統として残る。互いに依存しない。

## 確認済みのこと / まだのこと

- 確認済み: siglus_rs の wasm ビルド (約 19MB、gzip で約 8MB)。devserver で、Chromium から「マニフェスト → wasm 起動 → VFS 経由のファイル読み込み → セーブの PUT」まで通る。
- **実物の体験版での確認 (2026-10-24、devserver + ヘッドレス Chromium、`tools/shot.js`)**:

  | 作品 | エンジン | 結果 |
  |---|---|---|
  | シオンの血族 体験版 (`http://suezou.dyndns.org/dl2011/catwalk/sion/sion_try.zip`) | Siglus | 本編の文章まで。ボイスあり |
  | プリマドール 体験版 (`https://va-trialdist.azureedge.net/trial-primadoll-1.zip`) | Siglus | タイトル画面まで。二次鍵が要る (vnrun-prep で用意) |
  | お姉さん×すくらっち 体験版 (`http://suezou.dyndns.org/dl2008/hadashi/scratch2/trial.lzh`) | RealLive | 本編の文章まで |
  | 聖炎天使エレアノール 体験版 (`http://suezou.dyndns.org/dl2007/sagaplanets/eleanor/EleanorTrial.lzh`) | RealLive | タイトル画面まで。メニューのボタンがクリックにもキーにも反応しない (未調査、siglus_rs 側) |
  | 爆乳温泉 体験版 (`http://suezou.dyndns.org/dl2017/erectlip/bakuon/bakuon_try.zip`) | Softpal (sena-rs) | 注意書き画面まで。その後スクリプトがスタック不足で止まる (ネイティブ版も同じ。sena-rs 側の互換性) |
  | クダンノフォークロア 体験版 (b2019_sukerasparo) | BGI (ethornell-rs) | 本編の文章まで。SwiftShader だとタイトルまで 60〜95 秒かかる (実 GPU なら速いはず) |
  | さくら、もゆ。 体験版 (f2019_sakuramoyu) | FVP (rfvp) | 本編の文章まで。セーブ (サムネイル付き) がサーバーに届く |

- 実物で見つかって直したもの (`patches/siglus_rs/`、build.sh が適用する):
  - `0001`: ボイスの時間圧縮 (JITAN) を `std::thread::spawn` でやっていて、wasm では panic してタイトル画面で落ちた。wasm ではその場で処理する。
  - `0002`: wasm の起動部が画面を 1280x720 に固定し、キャンバスの大きさをそのままゲームの座標にしていた (1024x576 の作品は左上に小さく、1920x1080 の作品は切れる)。Gameexe の SCREEN_SIZE をゲームの座標にして、キャンバスに縦横比を保って拡大し、ポインタの座標も換算する (Android / iOS 版と同じやり方)。
- BGI (ethornell-rs) で直したもの:
  - `patches/ethornell-rs/0001`: ブラウザ版はゲームのフォルダ (`bgi-resource/`) への書き込みを拒否していて、BGI.gdb が書けず「データフォルダを作成することができませんでした」で止まった。ゲームのフォルダへの書き込みはユーザー側の同じ相対パスに置き、読むときはユーザー側を先に見る (コピーオンライト)。
  - `web/src/engines/ethornell.worker.ts`: 押してすぐ離すクリックが 1 フレームに収まると、エンジンはボタンから外れたと読み、タイトルの START でゲームが最初からやり直しになった。同じフレームの「離す」は次のフレームに回す。
  - ethornell-rs は wasm でワーカーに置き、ファイルは同期 XHR の Range 読み (256KB ブロック、128MB の LRU)。上流の Web 版 (platform/wasm) のつなぎを元にしている。
  - `patches/ethornell-rs/0008`: 1.7x の BackML (多層背景) にはプロパティ 0x8E (レコード +0x184、ソース移動の補間カーブ) が増えていて、未対応だと「サポートされていないパラメータ番号 [0x8E]」で止まった (放課後シンデレラ2)。exe の SetProperty (0x423DE0、0x41..0x8F のジャンプ表) で確認。
  - 体験版の中には「インストール用メディア」の形のものがある (ゲーム ID と同じ名前の印のファイル、Setup.exe、installer.arc)。ネイティブのデスクトップ版は印のファイルがあると即終了するが、ブラウザ版 (hosted の方針) では問題なく動く。
- FVP (rfvp) で直したもの (`patches/rfvp/`):
  - `0002`: 描画を WebGPU に (wgpu 0.19 の webgpu 機能)。
  - `0003`: ページがウィンドウの大きさを決めるので、スクリプトの全画面切り替えは wasm では無視する (exitFullscreen が例外で落ちた)。
  - `0004`: 直前の画面の読み戻し (セーブのサムネイル、マスク付きディゾルブ) がバッファのマップを同期で待っていて、WebGPU ではマップがフレームの間にしか終わらないのでページが固まった。読み戻しを非同期にして、終わるまでそのフレームを保留する (ゲームは進めない)。
  - ヘッドレスでは最初のクリックがフォーカス取得として捨てられる (rfvp が Focused でクリックを 1 回食う)。実ブラウザでは起動時にキャンバスにフォーカスするので問題になりにくいはず。
- NScripter (ONScripter-Yuri、GPL-2.0、`engines/onscripter/`):
  - Emscripten + Asyncify で SDL のループをそのまま動かす。`patches/onscripter-yuri/0001` で、合成済みの画面 (accumulation_surface、ARGB8888) をページの Presenter (WebGPU) に渡し、SDL のレンダラはソフトウェアにして何も描かせない。ポインタはページがフレームの座標に直して `vnrun_mouse` で SDL のイベントとして入れる。キーボードは SDL が window から読む。
  - ゲームのファイルは `web/src/emfs.ts` の遅延ファイル (Artemis と共用)。Windows 向けの作品はファイル名の大小文字が揃っていないので、小文字のパスでも引けるようにしている。セーブは `/saves` に書かせて GameFS に送る。
  - フォントは作品に入っていないので、ページのフォント (fonts/fonts.json の先頭) を渡す。スクリプトは既定で Shift_JIS (`--enc:sjis`)。
  - 動画は未対応 (飛ばす)。確認は自作の 0.txt (文章・選択肢・savegame) だけで、実物の作品はまだ。
  - この環境のプロキシは GitHub の archive (zip/tarball) を拒むので、Emscripten の ports は `tools/emscripten-ports.sh` で git から用意した。Lua は lua.org に届かないので GitHub のミラーから。
- 吉里吉里 (KiriKiri SDL2、`engines/krkr/`):
  - krkrsdl2 の Emscripten ビルド (CMake + Ninja) をそのまま使う。`patches/krkrsdl2/0001` で SDL のレンダラを作らずウィンドウのサーフェスを `vnrun_present` でページの Presenter (WebGPU) に渡す。サーフェスの形式が何であれ krkr は自分の画素 (メモリ上 B, G, R, A) をそのままコピーするので `bgra8unorm` で見せる (rgba で見せて赤と青が入れ替わっていた)。Web でもウィンドウの大きさはレイヤの大きさのまま (KRKRSDL2_WINDOW_SIZE_IS_LAYER_SIZE を外した)。ポインタは `vnrun_mouse` / `vnrun_wheel`。
  - 起動するのは data.xp3、無ければ data フォルダ、無ければゲームのフォルダ。ファイル名の大小文字は krkr 自身が無視するので小文字の別名は作らない。
  - セーブは System.dataPath (`/libsdl/krkrsdl2`) に書かれる。エンジンがファイルを書いたあと `FS.syncfs` を呼ぶので、Module.FS.syncfs を差し替えてそこで GameFS に送る (emscripten_run_script はモジュール内で eval するので、グローバルの FS ではなく Module.FS が呼ばれる)。
  - 確認は `/home/user/trials/kag3` (krkrsdl2/kag3) と、その first.ks を書き換えた krtest (文章・link の選択肢・`[save]`)。
  - System.exePath はゲームのフォルダ (`/game/game.exe`、`patches/krkrsdl2/0002`)。作品は exePath + "voice.xp3" や .cf を探す。KAG の savedata/ はその横で、ページが `/game/savedata` を data path の savedata へのシンボリックリンクにしてセーブとして残す。
  - 暗号化 xp3: `patches/krkrz/0002` が開いた xp3 ごとに方式を探す。先頭が分かるファイル (png は 16 バイト、ogg、tlg など) と、文字として読めるスクリプトで、次の方式を試して多数決する。固定の鍵で XOR / 「ファイルハッシュの下位バイト XOR 鍵」(xp3dec.tpm の大半、moteyaba など。GARbro の HashCrypt) / ハッシュ >> 12 で XOR、先頭 5 バイトは平文 (sys.tpm) / 奇数オフセットはオフセット、偶数はハッシュで XOR (2017 年ごろの xp3dec.tpm) / 立っているビットの数だけ左に回す (2020 年ごろの xp3dec.tpm)。vnbox の吉里吉里 135 本では、本物の cxdec は tpm 付きの 2 本だけ (lavender.tpm は GARbro の DB にあり、yuyukana.tpm は無い) で未対応。data.xp3.sig 付きの新しめの作品 (exe に暗号が入っている) も未対応。
  - 起動まわり: 97 本が `<exe 名>.cf` を持ち、startup.tjs は System.exeName から .cf を探して無ければ終了する。ページが /game/game.cf をそれへのシンボリックリンクにする。オプション (-debugwin=no など) が違うと exe を起動し直して終了する作品は、`polyfill/SelfRestart.tjs` がその場でオプションを設定して終了を飛ばす。k2compat (吉里吉里 Z の互換層) が Windows のダイアログでデバッグ窓を作らないよう -debugwin=no で起動する。
  - Window.showModal は入れ子のイベントループなのでページでは固まる。`polyfill/ModalDialog.tjs` が、メインウィンドウ以外に描かれた文字を質問としてブラウザの confirm() で聞き、窓の result に入れる (`patches/krkrsdl2/0004` が TJS の vnrunConfirm を足す)。KirikiriEmscriptenInterface (kremscripten.cpp) はリンクされていないが、リンクすると System.shellExecute なども置き換わり、亜衣さん (b2020_pin_point) が起動直後に固まったので使わない。
  - プラグイン DLL は読めないので Plugins.link は何もしない。よく使われるものは TJS の代わり (`engines/krkr/polyfill/plugins/`、Plugins.tjs が Plugins.link を差し替える): csvParser、fstat (ファイル操作はしない)、windowEx (何もしない)、layerExDraw (図形は外接矩形を塗る程度の近似)、scriptsEx、KAGParserEx、addFont (Font.addFont で)、win32dialog (何もしないクラスと定数)。
  - Ogg Vorbis (Windows では wuvorbis プラグイン) は stb_vorbis で読む (`patches/krkrz/0003`、`engines/krkr/stb_vorbis.c` をビルドが krkrz にコピーする)。
  - `patches/krkrz/0004`: TJSInt32ToHex(x, 0) が無限ループでバッファを壊していた (OpenAL のエラーを出すときに落ちた)。
  - 確認: moteyaba 体験版で注意画面 → タイトル → 本編の文章まで。
- 同期 XHR の BOM: `responseText` を読むとブラウザが先頭の BOM を消す/解釈するため、UTF-8 BOM 付きの TJS が 3 バイト欠けて読めなかった。範囲読みに `X-Vnrun-Pad: 1` を付け、サーバーは先頭に 0x00 を 1 バイト足して返す (docs/protocol.md)。全エンジンに効く修正。
- Web ページのゲーム (engine `html`、`web/src/engines/html.ts`): TyranoScript や RPGツクール MV/MZ は元から Web で動くので、ゲームのページを iframe で開くだけにした (描画もゲーム自身。WebGPU 方針の例外として判断を仰ぎ中)。セーブは、サーバーが iframe のページの先頭に `parent.vnrunFrame(window)` を差し込み、プレイヤーがそこで localStorage を GameFS 上のもの (`saves/localStorage/<key>`) に差し替える。IndexedDB はそのまま (RPGツクール MZ は IndexedDB が無いと起動せず、セーブもブラウザの IndexedDB に入る)。起動ページは package.json の main、無ければ www/index.html、無ければ index.html。Tyrano の公式サンプル (github.com/ShikemokuMK/tyranoscript) でタイトル・本編・セーブ・別セッションでのロードまで確認。未対応: package.nw (zip) や exe に固めた NW.js 作品 (展開してから渡す必要がある)、RPGツクールの実作品での確認。
- Softpal (sena-rs):
  - sena-rs は README のとおり「恋×シンアイ彼女」(2017) を対象に作られていて、ほかの作品は命令の引数の数や戻り値の扱いが少し違うところで止まる。エレクトリップ (Softpal の体験版が 2011〜2025 年に毎年ある) で 2 本試してどちらも止まった。直すには、止まった箇所ごとに旧版の Pal.dll の動きを確かめて sena-rs に版の違いを足す必要がある。
  - ブラウザ版のつなぎ (`web/src/engines/sena.ts`) は描画・入力・ファイルの範囲読みまで。上流の Web 版にはセーブと音声が無い (セーブは std::fs に書いていて wasm では失敗する) ので、動く作品が出てきたら足す。
  - `patches/sena-rs/0001`: スクリプトが止まったとき、上流の Web 版は最後の画面のまま黙って止まっていた。理由をページのエラーとして出す。
  - `patches/sena-rs/0002`: sena-rs は CPU で画面を合成する。できたフレームは Canvas2D ではなくページの Presenter (WebGPU) に渡し、ポインタの座標もページがフレームの座標に直して渡す。
  - `patches/sena-rs/0003`: 版によって同じ extcall の引数の数が違い (爆乳淫乱ビーチの 2:48 は 1 個、恋カケは 2 個)、多く取ると呼び出し元のフレーム (引数マーカーと arg_base) を食って「script stack underflow」になる。スクリプト呼び出しごとにフレームの底を覚え、extcall がそれより下を取ったら戻す。これで 2014〜2025 年のエレクトリップ作品がタイトルまで出る。Game.exe の各ハンドラの pop 数を静的に数える方法は、ヘルパー関数経由の pop があって正確に出せなかったので採らなかった。
  - `patches/sena-rs/0004`: ANI (スプライトの動きの小さなバイトコード) は `[fc 13] fd` の後に 00/12 移動、01 待ち、02 拡大、03 不透明度を並べ、`fe` 待ち・`ff` 終わり、ループ (0a など) が続く。上流は先頭 1 項目しか読まなかったので、待ちのあるタイトルボタン (ANI_BTN_EFFECT0n: 遅延 + フェードイン + 移動) が透明のままだった。最初のブロックの項目をまとめて流す (遅延とループは未対応)。
  - `patches/sena-rs/0005`: SYSTEM.INI の DEF_CG_WIDTH/HEIGHT は CG の大きさで、画面の大きさではない作品がある (ドスケベクエスト: CG 1540x980、画面 1280x720)。Game.exe が窓を作るときの `push 高さ; push 幅` を探して画面の大きさにする。ずれていると座標がずれてタイトルのボタンが押せなかった。あわせて文字列を長さで切る extcall (18/7) を Shift-JIS のバイト数で切るようにした (UTF-8 の文字の途中で切ってパニックしていた)。
  - `patches/sena-rs/0006`: 2011-2012 年の版 (りとるびっち体験版で確認)。`sp_set_ex` が z なしの 4 引数なので、スクリプトの呼び出し直前の push の数を数えて見分ける (`CoreAssets::short_extcalls`、`old_softpal()`)。引数の数が違う extcall は足りない後ろの引数を 0 で補い、多い分は捨てる (sp_set_ex 4→5、bgm_play 4→7、se_stop 3→2)。SYSTEM.INI がなく画面は exe の 800x600 で、スクリプト座標から内部の 1920x1080 への倍率を 1.5 固定から画面の比に。ボタンの flags 0 はマウスで押せる扱い、押したボタンの点へは gosub ではなくジャンプ (ハンドラの `ret` がボタン待ちの関数から抜ける)。
  - `patches/sena-rs/0007`: 実装の無い extcall (引数の数も分からないもの) は、スクリプトがその直前に push した数だけ捨てる。残すと呼び出し元が保存した arg_base がずれ、temp_mem に 2^28 番地を書こうとしてブラウザで capacity overflow になった (爆乳遊廓の 0x12_0033)。ネイティブで同じ所に行くには 1500 フレームほど回す必要があった。ブラウザ版の wasm は関数名を残す (`--keep-debug`、約 1MB) ので panic の呼び出し元が見える。
  - 確認には sena のネイティブ版 (`cargo build --release -p pal-vm --bin sena`) の `--headless --diagnostic-png-at <frame>:<png> --trace-script` が便利。
- NeXAS (自作、`crates/nexas`、ブラウザは `crates/nexas-web` + `web/src/engines/nexas.ts`):
  - PAC、コンパイル済みスクリプト (2010 年以降の形) の VM、.dat の表、SPM、.fnt (ビットマップフォント) を読む。組み込み命令の番号は exe ごとに違い、`engine.rs` は 2015 年の版 (恋愛フェイズ) の番号。
  - 版ごとの違いは `builds.rs`: 版の番号から 2015 年の番号への表 (逆コンパイルした分岐の case を、処理の中身の似かたで順に対応づけたもの) と、VM の命令番号のずれ (2012 年の版は 0x2F と 0x39/0x3A が無い)。どの版かは System.bin の呼び出しの引数の数がいちばん合う表で決める。版を足すときは exe を Ghidra で逆コンパイルし、`switch(param_3)` の大きい関数 (組み込み命令の分岐) と `^ 0x8000` のある関数 (VM) を 2015 年のものと比べる。
  - 0x48/0x49 は bank の呼び出し (0x4A で呼んだ所に戻る)、0x45/0x47 は行ったきり。ファイルを読んだら init ブロック (変数の初期値) を先に流す。オブジェクトは作った時点では不透明 (描画モード 0) で、0x159 でモード 3/4 に切り替えると 0 から目標の不透明度へ変わる。
  - 2015 年より前の版は .fnt を持たず Windows のフォントで描くので、`fonts/fonts.json` の TrueType フォント (ネイティブは `NEXAS_FONT`) を縁取りして使う。
  - エンジンは「何をどこに描くか」の一覧 (`Engine::draws`) を出し、ページが WebGPU で四角形として描く (回転・拡大・加算・クリップ)。ネイティブ (`nexas-run <game> <frames> <prefix> [frame[:x:y] ...]`) は同じ一覧を CPU で描いて PNG に書く。
  - 確認: 恋愛フェイズ体験版でロゴ → 注意書き → タイトル → ダイジェスト選択 → 本編 (チャット画面、メッセージウィンドウ、立ち絵) まで。ブラウザでもタイトルと本編を確認。キスベル体験版 (2012) はタイトル → 本編の最初の文まで (ネイティブ)、ブラウザでタイトルまで。リプキス体験版 (2016) は 2015 年と同じ番号で、タイトル → 本編まで (ネイティブ)。
  - 調べるとき: `NEXAS_CALLS=1` で組み込み命令の呼び出しを全部、`NEXAS_TRACE=1` で命令を、`NEXAS_OBJS=1` で画面のオブジェクトを出す。
  - まだ: 音 (BGM・ボイス・効果音)、セーブ、メッセージウィンドウの正しい位置 (System.dat の値を推測で使っている)、主人公の名前 (@u00/@u01 は仮)、パーティクル (0x189 系)、ほかの版の命令表。
  - 2010 年の JINKI 体験版は init と文字列変数名のない旧スクリプト形式。旧式の分岐・ループと一部の組み込み命令を実装し、ネイティブと Chromium でタイトル表示と START 後の遷移を確認した。System.dat の画面寸法を使い 1024x600 で描く。タイトル演出の単発効果音と BGM はアーカイブから読み、Chromium で再生開始、BGM のループ位置と START 後の停止を確認。旧版分岐・ループの仕様は命令列からの推定で、残る命令、他の音声、セーブ、本編と共に要確認。2009 年 crossover の SCRIPT.PAC は `PACK` 形式で、NeXAS の PAC と異なるため別途調査が必要。
- **Siglus の二次鍵はブラウザでは探さない** (siglus_rs の wasm ビルドは key.toml を読むだけ)。`vnrun-prep <ゲームのフォルダ>` (crates/prep、ネイティブ) が鍵を復元して key.toml を書く。プリマドールで 1 秒以下。devserver は `--game` の登録時にこの前処理を自動で呼ぶ (2026-10-26 に実装)。
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
| 吉里吉里 | 133 | krkrz / [krkrsdl2](https://github.com/krkrsdl2/krkrsdl2) (Web 版あり)、Kirikiroid2 | C++ | あり | 組み込み済み (KAG3 で確認)。krkrsdl2 は「未改変の商用ゲームは非対応」と明記。下の xmoezzz の部品が使える |
| NScripter | 4 | [OnscripterYuri](https://github.com/YuriSizuku/OnscripterYuri)、onscripter-ru-wasm | C++ | あり | |
| AliceSoft System4 | 3 | [xsystem4](https://github.com/kichikuou/xsystem4)、system3-sdl2 (Web 版あり) | C | System3 のみ | |
| YU-RIS | 29 | なし | | | 自作候補 |
| NeXAS | 26 | 自作 (`crates/nexas`) | Rust | あり (WebGPU) | 2015 年の版 (恋愛フェイズ) と 2012 年の版 (キスベル) で本編まで。ほかの版は命令表を足す |
| AdvHD | 18 | 自作 (`crates/advhd`, `crates/advhd-web`) | Rust | あり (WebGPU) | 2018 年の版 (Lua 5.3、僕と彼女のご奉仕同棲) でタイトルから本編の文章まで。選択肢・音・セーブ・マスク遷移はまだ。2015 年版は Lua の io、2026 年版は setCallback が足りない |
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

1. **実物の体験版で siglus_rs を確認** (Siglus と RealLive)。
2. ~~ethornell-rs (BGI)~~ (済) と ~~sena-rs (Softpal)~~ (取り込み済み、ただし作品の互換性は上記) 。submodule + `engines/<id>/build.sh` + `web/src/engines/<id>.ts`。
3. rfvp (FVP)、NScripter (OnscripterYuri を Emscripten で)。
4. 吉里吉里: krkrz を Emscripten で wasm にし、xmoezzz の eluna / xp3 / AMV などで商用作品のプラグインを埋める。まず 133 本が使うプラグイン DLL を数えて多い順に。
5. Artemis (art3m1s-core に Web のつなぎを書く)。
6. YU-RIS / NeXAS などは vnrun で Rust で書く (共通 crate の trait を先に作る)。

## 2026-10-26 正午の一時停止時点 (吉里吉里)

- 直近の push: 3a94fb7 (描画デバイスの drawer プロパティ = krkrsdl2 パッチ 0010、PassThroughDrawDevice 定数、startMouseHook の空実装。kai 2012 はタイトルまで確認)、bd7853c (KAG パーサーの属性名を小文字化。nanawind 2015 の選択肢の縁取りが復活)。
- 検証スレッドの最新集計: data.xp3 のある 125 本中 109 本がタイトル以上。3a94fb7 の kai/alcot 2012・lass_pixy と bd7853c の回帰サンプル (12 本) は検証待ち。
- 次の一手: exe 内蔵ストリーム暗号の xp3 (akabeesoft2/3・Hending の 5 本: atata、hakata zombies、roleplayer ×2、hassyakusama) の鍵解析。2026-10-26 にこたさんの了承済み (アーカイブ形式の解析のみ。PlayDRM・ソフト電池などの DRM と認証には触れない)。まだ手を付けていない。
- そのほか残り: cxdec (yuyukana など)、r1_kag3、data.xp3 のない 10 本 (検証スレッドが調査中)。詳しい技術メモはプロジェクトメモリの vnrun-krkr-*.md。

## 2026-10-26〜27 の再開後

- SDRS は `third_party/sdrs/sdrs-core` を submodule で取り込んだ。
  `crates/prep/src/lib.rs` の `prepare` を CLI と devserver で共用し、`--game` 指定時にフォルダ直下の EXE を調べる。
  SDRS を展開して元ファイルの退避先を表示し、その後に Siglus の二次鍵を用意してからエンジンを判定する。
  展開済みの EXE とバックアップ内の EXE は再展開しない。
  結合テスト9件が通った。
- 正午の「次の一手」に挙げた5本の XP3 は、Akabei 方式の32バイト鍵による XOR だった。
  `patches/krkrz/0009` で画像や音声のヘッダーから鍵の種を自動検出する。
  作品ごとの鍵一覧は持たず、元の XP3 も書き換えない。
  方式と検証件数は [docs/xp3-akabei.md](xp3-akabei.md) に記録した。
- ATATA、墓多 DYING ZOMBIES、ROLEPLAYER 小粥姉妹、ROLEPLAYER とろろ姉妹、ソープランドのはっしゃくさまの5本すべてを Chromium でタイトル画面まで確認した。
  保護対象21,896ファイルの Adler-32 がすべて索引と一致した。
  本編の進行やセーブは今回確認していない。
- 復号の固定ベクトル、読み込みの分割位置、32ビットを超えるオフセットは `bash engines/krkr/test.sh` で確認する。
  CI にも追加した。
  Emscripten 4.0.23 で wasm ビルドが通った。
  新しい SDK でも CMake が SDL2 を見つけられるよう、ビルド前に `embuilder build sdl2` を実行する。
- この環境でのブラウザ確認には、公式 Noto Sans CJK JP を `web/public/fonts/` に置いた。
  スクリーンショットとログは `target/research/screens/`、公開体験版は `target/trials/` にあり、いずれも Git 管理外。
- `r1_kag3` は公式 `kr2_232r2.zip` の紹介デモで再確認した。
  `syoukai` を `data` として配置し、隣の `template` と `kag3plugin` も配信対象に含める。
  `new Pad()` をブラウザの編集ダイアログで実装した。
  Config を編集・保存し、再起動後に更新内容を読み、紹介本文まで進むことを確認した。
  保存先はコピーオンライトのセーブ領域で、元の Config は変更しない。
- `yuyukana.tpm` と `lavender.tpm` の cxdec に対応した。
  プラグインから制御表を読み、算術プログラムを評価する。
  Windows の DLL は実行しない。
  保護対象10,440ファイルの Adler-32 が一致し、両作品でタイトルを確認した。
- 花鐘カナデグラム Chapter1 の cxdec は EXE の bres リソース内にあった。
  `vnrun-prep` がパラメーターと制御表を `vnrun-cxdec.bin` に保存し、ブラウザに渡す。
  data.xp3 の保護対象1,955ファイルが一致し、タイトルメニューまで確認した。
  PIMG ソースをキャッシュから複製するときの表示状態を初期化し、背景・キャラクターも描画することを確認した。
- おしおき生意気ギャルは cxdec ではなく、Akabei の31バイト周期の変種 (Mado) だった。
  32/31バイト周期をチェックサムで判別し、保護対象749ファイルが一致した。
  タイトルを確認した。
- data.xp3 のない9本も調査した。
  data.bin、EXE 内蔵 XP3、インストーラの展開を扱い、8本でタイトルメニューまで確認した。
  残る恋嵐スピリッチュの配布 URL は差分パッチのみで、本体が含まれていない。
- NoStrike の3本は平文 UTF-8 を XOR と誤判定していた。
  Adler-32 が一致する平文を除外し、Trap Genesis はタイトルまで進んだ。
  残り2本は非表示の補助ウィンドウへの入力誤配送も修正し、言語選択からタイトルへの遷移を最終ビルドで確認した。
  Motion は既存のスタブを先に読み込むだけで、E-mote の動きを実装したわけではない。
- 詳細と再現コマンドは [krkr-compatibility.md](krkr-compatibility.md) を参照。
  正午時点の回帰サンプル12本は具体的な ID 一覧がこの環境にないため、同じ12本の再実行とは扱わない。
- SDRS の登録時自動準備は `a76be4a`、XP3 復号とブラウザ Pad は `fee9763`、入力と PIMG の修正は `afeaba8` にコミットした。
  push とホスト CI の実行はまだ行っていない。

## 未決のこと

- Rust 側の共通 trait の具体的な形 (描画は WebGL2 か wgpu か、音声の取り回し)。最初の自作エンジンで決める。
- エンジンの wasm をどこで配るか。今は `./build.sh` で手元ビルド。GitHub Actions でビルドして Releases に置く形が候補。
- vnrun のサーバーの形 (ゲームの取り込み、一覧、セーブの保存先とバックアップ)。
- ゲームのファイルの大きいものを丸ごと読む問題 (上記)。
