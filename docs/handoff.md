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
- AliceSoft System4 (xsystem4、GPL-2.0-or-later、`engines/system4/`):
  - Emscripten + Asyncify で VM をそのまま動かす (`engines/system4/build.sh`、meson は使わず upstream の meson.build のソース一覧を読む)。xsystem4 は GLES 3 で描くので、WebGL2 の文脈をページに出さない canvas に作り、毎フレーム view のテクスチャを glReadPixels で読み出して `vnrun_present` からページの Presenter (WebGPU) に渡す (`patches/xsystem4/0001`)。ページには WebGL の canvas を置かない。座標はフレームのピクセルのまま (`gfx_update_screen_scale` は等倍)。
  - VM は SDL_Delay (Asyncify で emscripten_sleep) と、フレームごとの `vnrun_yield` でブラウザに番を返す。ポインタは `vnrun_mouse`/`vnrun_wheel` で SDL のイベントとして入れ、位置も覚えておく (SDL_GetMouseState は見ない)。
  - libffi は使わない。WebAssembly の間接呼び出しは型が一致しないと落ちるので、`engines/system4/gen_hll.py` が HLL ライブラリ (src/hll/*.c) ごとに、表の関数と static_library_replace で差し替える関数を clang の AST から型付きで呼ぶラッパーを書き、`shim/ffi.c` の ffi_call が関数のアドレスから引く。ラッパーの無い関数は名前付きで VM_ERROR になる。
  - libsndfile は `shim/sndfile.c` (vorbisfile と PCM の WAV)、libturbojpeg は `shim/turbojpeg.c` (stb_image) で置き換え。libwebp と cglm はビルド時に取ってくる。
  - フォント: 作品の .fnl のほかに TrueType のゴシックと明朝を必ず読むので、ページのフォントか、無ければ build.sh が置く VL Gothic を渡す。セーブは /saves (`--save-folder`)。
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
  - 2010 年の JINKI 体験版は init と文字列変数名のない旧スクリプト形式。旧式の分岐・ループと一部の組み込み命令を実装し、ネイティブと Chromium でタイトル表示と本編最初の台詞を確認した。System.dat の画面寸法を使い 1024x600 で描き、メッセージを画面内に収める。画像・スプライトの作成、移動、拡大、削除も対応。タイトル演出の単発効果音と BGM、本編の最初のボイスとチャンネル付き効果音はアーカイブから読み Chromium で再生開始を確認。BGM のループ位置と START 後の停止も確認した。旧版分岐・ループの仕様は命令列からの推定で、残る命令、他の音声、効果音の停止・フェード、セーブ、本編の続きと共に要確認。2009 年 crossover の SCRIPT.PAC は `PACK` 形式で、NeXAS の PAC と異なるため別途調査が必要。
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
| AliceSoft System4 | 3 | [xsystem4](https://github.com/nunuhara/xsystem4) (`third_party/xsystem4`、GPL-2.0-or-later)、system3-sdl2 (Web 版あり) | C | 組み込み済み (Emscripten、描画は WebGL2 で読み戻して WebGPU) | ランス01・母娘乱館の体験版で本編の文章まで確認 (ブラウザ)。音とセーブは未確認 |
| YU-RIS | 29 | なし | | | 自作候補 |
| NeXAS | 26 | 自作 (`crates/nexas`) | Rust | あり (WebGPU) | 2015 年の版 (恋愛フェイズ) と 2012 年の版 (キスベル) で本編まで。ほかの版は命令表を足す |
| AdvHD | 18 | 自作 (`crates/advhd`, `crates/advhd-web`) | Rust | あり (WebGPU) | 2018 年の版 (Lua 5.3、僕と彼女のご奉仕同棲) でタイトルから本編の文章まで。選択肢・音・セーブ・マスク遷移はまだ。2015 年版は Lua の io、2026 年版は setCallback が足りない |
| Tmr-Hiro ADV System | 13 | 自作 (`crates/tmrhiro`, `crates/tmrhiro-web`) | Rust | あり (WebGPU) | ルネ・マリン・つるみく系 (srp.pac / Agrd・Bgrd・Cgrd.pac)。vnbox では NeXAS に分類されていた。2010 年 (ぱいずりチアリーダー) と 2023 年 (非道狩) の体験版でタイトルから本編の文章・選択肢まで。立ち絵の位置と移動 (exe の処理から)、フラッシュ、ゲームの画像を使ったセーブ・ロード画面、ウィンドウのボタン (Qセーブ/Qロード/オート/スキップ/非表示/ボイス)、右クリックで非表示。2006〜2008 年の旧 srp 形式 (1 行 1 命令、`engine/old.rs`) も対応し、まいみこ体験版でタイトル・本編・選択肢・マップ選択・立ち絵まで。コンフィグ・バックログ・新形式のマップ選択・旧形式の目パチ演出 (0x46)・旧形式のセーブ画面の絵はまだ |
| LiveMaker | 4 | 自作 (`crates/livemaker`, `crates/livemaker-web`) | Rust | あり (WebGPU) | vf アーカイブ (exe 末尾 / game.dat+ext)、.lsb チャート、TpWord 文章、.gal 画像。メッセージ窓やメニューはゲーム同梱のノベルシステムのチャートが組むので、エンジンはスレッド・コンポーネント (Img/Box/Mes/Caption/Menu/PrevMenu(.lpm XML とバイナリ)/Button/Edit/Sound/Timer)・表裏画面と Flip・キャビネットを持つ。セーブはページ頭の状態 (変数・コンポーネント・バックログ・メインチャート) を JSON でセーブ置き場へ、サムネイル (SaveScreen) とキャプションも。バックログは CallHist がピクセル行で出す。キー・ホイールは @KeyClick[257/256] として、Edit には文字入力。体験版 4 本 (RJ129271/RJ125204/RJ284038/RJ341209) で本編の文章まで。vnbox で LiveMaker とされた 9 本は誤判定 (Tyrano/Electron/独自形式など)。.lcm (LiveCinema 108/113) は絵のキー (移動・回転・拡縮・透明度・加算・イージング) を 30fps で再生する (書式は体験版から読み解いたもの。種類 2 のキーは読めない)。ブラウザでは文字入力を隠したテキスト欄で受けるので IME の確定文字も届く。ヘッドレス Chromium で 3 本ともタイトル→本編・ホイールのバックログ・セーブ/ロード・名前入力を確認 |
| CatSystem2 | 3 | 自作 (`crates/catsystem2`, `crates/catsystem2-web`) | Rust | あり (WebGPU) | .int アーカイブ (cs2.exe のリソースの鍵)、.hg3 画像、.fes 画面スクリプト (flow → 注意書き → ロゴ → タイトル)、.cst シナリオ。ゲームの sscript.kcs (KCS バイトコード) は動かさず、シナリオの命令 (bg/cg/fg/eg、pl、rdraw、メッセージ窓、選択肢、音) をエンジン側で実装。春風センセーション！体験版とアマカノ体験版 (exe 名違い、名前入力ダイアログは既定で OK、`fex sel` のマップ)、ニュートンと林檎の樹 体験版＋でタイトルから本編まで。セーブ・ロードとバックログはエンジン側の画面 (`crates/catsystem2/src/menu.rs`: ゲームの `execfes saveload` (SAVEPANEL の画面) の代わりに開き、1000+スロットを返して `$512` 経由で sscript 起動時に読む。物語中は右クリックでセーブ/ロード、ホイール上でログ)。セーブはシーン入口の変数と表示中ページの位置を持ち、ロードはシーン頭から skip で再生してから変数を戻す。コンフィグ、KCS (モーション・パーティクル) はまだ |
| EntisGLS | 8 | 自作 (`crates/entis`, `crates/entis-web`) | Rust | あり (WebGPU) | NOA アーカイブ (exe の鍵、入れ子の .noa、ゲームフォルダのばらの .csx)、ERI 画像 (Nemesis 圧縮を含む)、Cotopha の実行イメージ (.csx) をそのまま動かす VM。スキン (system.inf の XML: 絵・スタイル・ページ・ボタン)、スプライトの合成と BeginActivation の変化、MessageSprite の文章、InputFilter のジョイボタン、Window のコマンド。痴覚過敏 (2010)、リリカル♪りりっく (2007)、夏ノ雨 (2009)、キスと魔王と紅茶 (2009) の体験版でタイトルから本編の文章まで。MIO の音 (ロスレスと LOT/DCT、ERINA 符号) は初めて鳴らすときに WAV にする。セーブは File::SaveContext がグローバル変数・全スレッドのスタック・それらが指すエンジンのオブジェクト (絵は名前、スキンはファイルで) を書き、LoadContext でセーブした呼び出しの直後から再開する (data 変数は今の値のまま、data が持つオブジェクトは子スプライトだけ戻す)。サムネイル・セーブ日時・ボタンの文字も。痴覚過敏・夏ノ雨・キスと魔王と紅茶でクイックセーブ/ロードを確認。3D (RenderSprite/ModelJoint/ParticleModel) は描かない、アニメーション ERI は 1 枚目だけ。2024 年の狙われた優等生はシステムが exe 内のネイティブ、台本が xscript (.srcxml) の新しい世代で未対応 |
| QLIE | 4 | 自作 (`crates/qlie`, `crates/qlie-web`) | Rust | あり (WebGPU) | GameData の .pack アーカイブ (1.0/2.0/3.0/3.1、3.1 は exe の RESKEY か key.fkey の鍵)、.b (ABMP のモーション付き画像)、.s スクリプト (`@@` ラベル、`\` 命令、`^` のストーリー命令は rep の置き換え)。ゲームスレッドとシステムスレッド (メニューは `$$$GameThreadImage` の上に開く)、`\stop` のハンドラ、オブジェクト (img/btn/sbtn/msg/snd/list/bar と sys/eff/cam/inp/dat などの組み込み)。かみぱに！体験版でタイトルから本編 (背景・立ち絵・名前・ルビ) まで。アナベル・メイドガーデン体験版は ImoScripter (`^bg01,file:...` のような命令を table/list オブジェクトで解釈するスクリプト側のライブラリ) で本編まで: table の `\v` `\?v` `\comb` `\asn` `\brep` `\bpos` など、`\exop` と rep の `\?r`、テンプレートのラベルはそれを置いたスクリプトで探す、`\p4` `\alg2` `\col` の位置・拡大・揃え、.b の `*MesSurface` 画像の位置に本文、`\thrd,1,label` でシステムスレッドのメニュー、`mdat` `tdat` はメモリ上のセーブ。セーブ・ロードは `\scp,dat` を自前の形式で (シナリオのファイルと最後の本文の行と変数、ロードはシナリオの @@TOP からゲームのスキップで進める)、バックログはゲームの画面で log オブジェクトから。3.0 の key.fkey は GARbro どおり (pack_keyfile を先に解く、exe の TFORM1 の IconKeyImage があればゲーム鍵も) だが、シグナルハート体験版 (3.0 + key.fkey、IconKeyImage なし) はこれで解けず未対応。音声・効果のマスクは簡略、OnFrame や動画はまだ |
| Escude | 8 | 自作 (`crates/escude`, `crates/escude-web`) | Rust | あり (WebGPU) | ESC-ARC アーカイブ、ゲームの C ソース (ACPX) をコンパイルして自前の VM で実行 (構造体は詰めて配置、unsigned の昇格、setjmp/longjmp)。プレーン (8/16/24/32 bit の DIB) と 36 種の合成モード、スプライト (回転・拡大・子)、gtext、ogg の音、getkey/getpoint の入力。ヴェルディア幻奏曲体験版と Re;Lord 体験版でタイトルから本編まで。セーブ・ロードはゲーム自身の画面のまま (`dir_get` の文書フォルダ以下をセーブとして保存、`file_gettime` の日時は `vnrun-times.txt` に持つ) で、両作品ともセーブ→タイトルからロードで本編に戻る。パターンワイプ (BLT_PATTERN / BLT_PATBLEND) も描く。角度は 1 周 2048 |
| Interheart (Candy Soft) | 11 | 自作 (`crates/interheart`, `crates/interheart-web`) | Rust | あり (WebGPU) | FPK アーカイブ (平文・XOR 索引・"FPK 2.00"、ZLC2 圧縮)、KG / BMP の画像。シナリオは data.fpk の ACT_<字>.TXT (ゲーム本体も文章をここから読む) をそのまま解釈: シーン (***SC)・選択肢 (***SS)・EF_/CG_/DG_/KM_/FCG_/BG_/SE_/BGCV_ の命令と台詞 (名前　（ボイス番号）)。タイトル・メッセージウィンドウ・選択肢は exe 内にあるため自前で描く (タイトルは titlechip.kg と TitleTM1.hmp のボタン領域、立ち絵の位置は exe の表 320/640/960/0/1280/480/800/160/1120)。セーブは自前の 10 スロット画面 (右クリックのメニューから)。いたずら体験版2 (2013) と透けちゃって！！体験版 (2017) でタイトルから本編まで、セーブ→ロード可。立ち絵の揺れ (EF_A, EF_C0040〜)・ムービー・コンフィグ・バックログ・回想はまだ |
| Majiro | ? | 自作 (`crates/majiro`, `crates/majiro-web`) | Rust | あり (WebGPU) | MajiroArc V1〜V3 アーカイブ、.mjo (暗号化を含む) をそのまま動かす VM (スレッド、イベントフック、setskip/デストラクタ)、RCT/RC8 画像 (TC01 の差分、`_.rc8` マスク)、ページ・スプライト (中心基準の _ext)、`$console_*` の文字をページに描いて CONSOLE_WROTE などのイベントで console.mjo に渡す。Closed GAME 体験版 (2015) でタイトルから本編の文章まで (majiro-run と Chromium)。スプライトアニメ・選択肢以降・セーブはまだ |
| Kaguya | ? | 自作 (`crates/kaguya`, `crates/kaguya-web`) | Rust | あり (WebGPU) | 2006 年頃の世代 (GAME_SYS/AMATU.exe + WFL1 の .arc/.ari)。WFL1 アーカイブ (LZSS)、AP/BMP 画像、tblstr の文章と名前、.scr バイトコードの VM (変数・条件分岐・スクリプト移動)、背景・立ち絵 (胴体+表情の合成)・イベント CG・色かけ、ルール画像トランジション・フェード・フラッシュ・揺れ・カットイン、BGM/効果音/音声 (Ogg)、メッセージ窓とタイトルメニューは exe の代わりに vram01/02 の部品で自前描画。アマツ体験版 (2006) が最後まで通る (kaguya-run と Chromium)。2013 年頃の世代 (GAME_SYS.exe + ARC/、1024x600、label.tbl の GOSUB、ローカル変数、選択肢、APS3 部品画像、AO 立ち絵) も肉牝R30 体験版で本編と場面選択まで。2009 年の GAME_SYS.exe (さらさらささら体験版) も最後まで。2008 年 (夏神 体験版２) も場面選択と各場面まで。セーブ・マップ画面 (op 54 1 0)・LINK アーカイブの新しい世代 (Start.exe + params.dat) はまだ |

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

## ShiinaRio (椎名里緒) の途中経過 (2026-10-28)

- 対象: `b2011_guilty` 種憑け村 序章体験版 (`TANE_T.exe`, ini は `[椎名里緒 v2.48]`, `Arc=T1TANE_T0.WAR`, `Scn=start.SCN`, 800x600)。
- できたこと: `crates/shiina` の `warc.rs` が WARC 1.7 の索引と全エントリ (YH1/YPK/YLZ、Decrypt/Decrypt2、helper4 の SHA-1 系と領域 CRC) を復号する。
  `cargo run -p shiina --bin shiina-dump -- list|extract` で確認。T1TANE_*.WAR はすべて展開でき、S25 画像と .SCN が取り出せた。
  WARC 1.2〜1.6 の Kogado レンジコーダは未実装。
- 鍵: `tools/garbro_db.py` が GARbro の Formats.dat (MS-NRBF) を読み、`warc OUT NAME [HELPER]` で `crates/shiina/schemes/*.bin` を書く。
  体験版の EXE は製品版と helper key の文字列が違う (種憑け村体験版は `gy||OWey|diODD`)。
  helper4 のかからない小さいエントリだけ正しく出るときはこれを疑う。
- まだのこと: .SCN の VM。タイトル画面は未到達。RE の道具は `tools/shiina/` (pe.py, dispatch.py, sig.py, dis.py)。
  - インタプリタは TANE_T.exe の 0x423f20〜0x42a447。ctx は ebx で、+4 がスクリプト先頭、+0x10 が pc、+0x18 bit8 が終了、+0x20 が命令、+0xff8 が引数表、+0xffc/+0x1000 がスタック。
  - 命令は u16 で、ハンドラは約718個。<=0x6b はバイト表 0x42a4f8 から飛び先表 0x42a450。0x78〜0x580 は飛び先表 0x42a564。0x582〜0x795 はバイト表 0x42bc30 から 0x42b988。0x6f/0x581/0x796 は個別に処理し、それより上は 0xfe39 までさらに分岐する。共通の次命令は 0x42a3d4、エラーは 0x42a410。
  - オペランド: 読み 0x415690、書き 0x415190、アドレス 0x415500、push 0x4150f0、pop 0x415140。
    先頭1バイト t のうち 0x40 はアドレス指定、0x80 は間接 (変数の値をスクリプト先頭からのオフセットとして使う)。
    t&0x7f は 2/3 が c17ae8[u16]、4/5 が即値 u32、6/7 が 0x7fd0b0[u16] (7 はバイト)、8/9 がスタック局所、10/11 が 0x7fc110[u16]、12/13 が ctx+0xff8[u16]、14/15 が c17ae4[u16]、16 が C 文字列、17 が '.' 付き数値テキスト、18/19 が `}` までの名前付き変数 (`[式]` 添字、0x401600 でハッシュ表 0x5e4e58)。奇数は参照先を読む。
  - 値は32ビットで、ポインタは実アドレス。エミュレートには平坦なメモリ空間が要る。
  - START.SCN の末尾には x86/MMX のネイティブルーチンが埋め込まれている (`61 9d c3` = popad/popfd/ret)。スクリプトからこれを呼ぶため、その部分はハンドラごとに Rust で書き直すか x86 を解釈する必要がある。
  - 線形の逆アセンブルは START.SCN のオフセット 0xe (op 0xa の後の `82 d3 82 aa 48 00`) で崩れる。命令ごとのオペランド数の推定は途中。
  - 判明した命令の例: 0xa は待ち時間 (<=1000)、0x49d はグローバル c17c28 への代入、0x7ee は [v v w]、0x7ef は [v v v]。
- 他の ShiinaRio 体験版: `b2017_onomatope_raspberry` (vnbox では NG で画面が出ない)。VM の後で試す。

## Majiro の実装メモ (2026-10-28)

`crates/majiro`。命令表は majiro-py (MIT) の docs と data/mjs (console.mjs の古いソース) が頼り。exe の syscall 表は `cmp edx, hash` の二分探索から引いた。

- 透明度は Majiro の値では「透明さ」(255 で見えない): `$sprite_alfa_set`/`define`、アルファページ (`$get_alfapage`)、`$page_set_antidata` の相手ページ (色の下位バイト = 青が透明さ) がそう。エンジン内部は普通の不透明度で持ち、出入り口で反転する。
- `$grp_copy`/`$grp_extcopy`/`$grp_modcopy` は (src, x, y, w, h, dst, x, y) の順。extcopy の 9 番目は写す側の透明さ。
- 文章: エンジンは `$console_set` のページに文字を描き (`$console_font` の xl は半角の幅、全角は 2 倍)、CONSOLE_WROTE(page, x, y, w, h, 0) で console.mjo に渡す (console.mjo が少しずつ表示ページへ写す)。改ページ待ちは PAUSE のフック (pause_inter) が自分で待つ。`【名前】` で始まる文は NAME_DISP。`$console_color` の上位バイトは影 (1) / 縁取り (2)。
- `$reset_exec` (新しいゲーム) で HOT_RESET(0, "", 1) を送る (console.mjo が窓の色や絵を作り直す)。
- 終わるスレッド (`$thread_end`) も各フレームのデストラクタ (bsel.5) を実行してから消える (遷移の絵がこれで片付く)。

## 未決のこと

- Rust 側の共通 trait の具体的な形 (描画は WebGL2 か wgpu か、音声の取り回し)。最初の自作エンジンで決める。
- エンジンの wasm をどこで配るか。今は `./build.sh` で手元ビルド。GitHub Actions でビルドして Releases に置く形が候補。
- vnrun のサーバーの形 (ゲームの取り込み、一覧、セーブの保存先とバックアップ)。
- ゲームのファイルの大きいものを丸ごと読む問題 (上記)。
- 描画は WebGPU だけ、という決まりに対する残り: AliceSoft System4 (xsystem4) と Artemis (art3m1s-core) は GLES を見えない canvas の WebGL2 で描き、読み戻した画素を Presenter (WebGPU) で出している。つなぎの間の措置で、GLES の描画を WebGPU に移す (または CPU で描く) のが残りの作業。

## Kaguya の実装メモ (2026-10-28)

- 対象はアトリエかぐやの古い世代 (exe + `scr/bg_/sp_/cg_/cgw/bgm/wav/vo1/vo2/tblstr` の `.arc` と `.ari`)。新しい世代 (Start.exe + `bgd/cgd/spd/scr.arc` の LINK アーカイブ、params.dat) は別物で未対応。devserver は `scr.arc` と `tblstr.arc` があれば kaguya にする。
- WFL1: `.ari` は `.arc` と同じエントリ見出し (i32 名前長、反転した cp932 名、u16 mode、u32 size) を並べたもの。ただし mode 1 (LZSS) の展開後サイズ u32 は `.arc` 側にだけあり、データの直前に置かれる。データ位置は 4 から見出しとデータを足していけば出る。LZSS はビット列 MSB 先頭、1+8 ビットでリテラル、0+12 ビット位置+4 ビット長 (+2)、窓 4096 で最初の書き込み位置は 1。
- 画像: `AP` (u32 幅、u32 高さ、u16、BGRA 下から上) と 24 ビット BMP。.sp_ は 800x600 の全画面に位置込みで描かれている。立ち絵 "nata13" は胴体 "nata00" に表情 "nat013" を重ねたもの (胴体が無ければ名前そのままの 1 枚、または "xxx000")。exe は izu/bai/aya の 3 人だけ別扱いしている (未解明、izub00 系が無い)。
- 文章: tblstr.arc は (u32 種類 0 文/2 名前、u32 長さ、反転した本文、反転していない 0 で終わる) の並び。tblstr.ari は件数のあと各文の位置で、文番号 i は ari の i 番目の u32 (0 番目が件数)。名前はスクリプトが tblstr.arc のバイト位置で指す。
- .scr: 12 バイトの見出し (8 バイト + コード長)、命令は u8 op + u8 長さ (文字列を除く) + 引数、そのあと op ごとに決まった長さ引数の文字列 (反転、0 終わり)。文字列長: 0c/0f/10/12/2c/3d/48/49/50/52/56 は +3、13 は +3 (0xff で無し)、15 は +4、2f は u16 +4、5e は +2、62 は +3 と +8。ジャンプ先はコード先頭 (ファイルの +12) からの位置。インタプリタは AMATU.exe の 0x426ab0、ジャンプ表 0x4286c8 (op 0..0x66)。
- 主な op: 00 変数=定数、01 加算、5b 変数コピー、02 ジャンプ、03..08 比較ジャンプ (+2 のビット 1/2 が左/右を定数にする; eq ne gt lt ge le)、0c スクリプトへ移る、27 終わり (タイトルへ)、2a タイトルメニュー (ゲームモード 0x15)、13 文 (+4 名前位置、+8 文番号、文字列 = 音声 "say0052.voi" → vo1/vo2)、14 窓を消す、15 画像 (+3 種類: 0 背景 (立ち絵も消す) 1 背景だけ 2..5 立ち絵の枠 7 イベント CG 8 ルール画像)、16 トランジション (直前にルール画像があればそれで、無ければクロスフェード)、17 層を消す、58 層の色かけ、1d すぐ表示、19/1a 黒からフェードイン/黒へフェードアウト、1f/20 同じく短く、21/5f 白/色フラッシュ、22 揺れ、57/18 待ち (100ms 単位)、50 BGM (+2 = 2 で .bgm、1 は MIDI)、52 効果音 (+2 = 2 でループ)、53 BGM 停止、55 ループ効果音の停止、2c 音声、62..66 カットイン (出す・動かす・待つ・消す・フェード)。60 (ルールの色?)・3d・47・54 (章の区切り) などは何もしない。
- タイトルの var[2] は exe が 1 にしている (しないとロゴの後 test.scr に行く)。メニュー位置は自前で決めた (右下、「最初から始める」「終了する」だけ)。
- 確認: `kaguya-run <ゲーム> <フォント> <フレーム数> [出力先]` が自動クリックで進めて PNG を書く (KAGUYA_AT=x,y でクリック位置、KAGUYA_PICK=0,2 で選択肢を順に何番目にするか)。アマツ体験版は 000_pro→001..003_op→004_inter を通ってタイトルに戻る (未知 op の警告なし)。

### 2013 年世代 (肉牝R30、GAME_SYS.exe)

- 2009 年の GAME_SYS.exe (さらさらささら) は命令の中身は 2013 年と同じ (変数・比較・GOSUB・選択肢は exec2) だが、文字列長は 2006 年の表に 70 (+2) を足したもの (scr.rs の表 3)。`Engine::table` が文字列長の表、`version` が命令の意味。判定は全 .scr を 3 つの表で読み、一番多く読めた表 (同数なら番号の小さい方)。ラベル表は label.scr (中身は label.tbl と同じ形)、タイトルは 0x2a。
- 2008 年の GAME_SYS.exe (夏神): 命令は 2006 年のもの (op 00 が 8 バイト、比較のビットも 2006 年) で、文字列長は表 3、選択肢 09/0a/0b と GOSUB 70/71 だけ後の世代のもの (exec2 に回す)。`version` は op 00 の長さ (8 か 12) の多い方で決める。parts.arc は APS3 の前の形 (`pic::parts_2008`: 元ファイル名の並び、"パターン1" ごとの 68 バイト (+12 に元矩形)、u16 圧縮 + u32 圧縮長 + u32 長さ + AP) で、画面上の位置を持たない: 窓は下端、タイトルの項目は右の枠 (536,250)/(536,460) に置いた。
- 2008 年の立ち絵は exe がコードで登録している ("kaz_0105_s" → 胴体 "kaz_01_s" (表情のない 800x600)、表情 "kaz0005_s" (位置なしの小さな AP)、表情の位置 319,49)。`exe.rs` が GAME_SYS.exe の `mov r,y; push; mov r,x; push; push 0; push 0; push 表情; push 胴体; push 名前; mov ecx,…` の並びを探して表を作る (3853 件)。名前から機械的には作れない (say_a317 → 表情 say0217)。
- 2009 年: 立ち絵は stand.arc の "sao_c1_0001" = (u8 長さ + "STD01", 胴体 "sao_c1.prs", 8 バイト, 表情 "sao_001.prs", 7 バイト)。絵は sp_.arc の AO で位置は AO のもの (8 バイトの中身は未解明)。カットイン 62 の種類 "ap3"/"prs" は parts.arc の APS3 (最初の部品を使う、その x, y を足す)。60 は次の画面を単色に、77 はミリ秒待ち、78 はカットインの移動待ち。窓は parts の message_window (0,470)、名前は左の丸の中、タイトルは title_back_trial と title_start/title_exit (部品の位置そのまま)。59/5a/6a/6b (画面の揺れ・ズーム?) は未実装。

- アーカイブは `ARC/` の下 (一部大文字 `VO1.ARC`/`.ARI`)。`Engine::locate` が大文字小文字とフォルダを吸収し、エントリ名のフォルダ部分は捨てる。devserver は `ARC/scr.arc` があれば kaguya。
- 世代の判定: scr.arc の全 .scr を 2006 年と 2013 年の文字列長表でそれぞれ読み、きれいに読めた方。2013 年の文字列長: 0c/12/2c/50/91 は +3、13 は +3 (0xff 無し)、0f/15/52 は +4、5e/70 は +2、79/8f/90 は +4 と +8、88 は +2 と +3。exe は (関数, op) の組でハンドラを登録し、文字列は 0x4dfa70 で読む。
- 変数: 00/01 は u16@6 に値 @8、フラグ (+2) のビット 4 でローカル表。5b は @4 ← @6 (&4/&8 でローカル)。比較 03..08 は a = &4 ローカル / &1 変数 / 定数、b = &8 / &2 / 定数 (2006 年とビットの意味が逆)。
- label.tbl: (u32 長さ + "x.sct", u32 長さ + ラベル名, u32 位置) の並び。70 GOSUB (+4 ラベル番号)、71 RETURN、5e far goto。54 0 0 はタイトル (prog.scr の CALL_TITLE)、54 1 0 はマップ画面 (CALL_MAP、exe の中の画面で未実装: 何もせずに次へ進み、R30 では場面選択に戻る)。
- 画像: AO = AP + i32 x, y (画面上の位置)。APS3 (`\x04APS3`) は部品集: 個数、部品ごとに (i32 状態, u8 名前長, 名前, 元矩形 4×i32, 3×i32 (?, x, y))、そのあと u32 サイズ, u16 圧縮, 圧縮なら u32 圧縮長 u32 展開長 + LZSS の AP。parts.arc の avg_mes_window (0,450)・avg_namebg・title_bg_01_t・title_start/title_exit (状態 1 がホバー) で窓とタイトルを組む。
- 立ち絵: 79 (+2 枠, 胴体名, 表情名) で .prs の AO を重ね、91 (名前, dx @4, dy @8) の中央寄せ表で位置をずらす。7a で枠を消す (0 は全部)。16 はトランジション (+2 種類, ms @4)。
- 選択肢: 09 開始、0a 項目 (+4 tblstr のバイト位置)、0b で待つ。結果は var[0] (1 が最初)。
- 何もしない op: 86, 87, 88, 8d, 75, 3d ほか (exec2 の末尾)。

