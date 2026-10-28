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
  | 爆乳遊廓 体験版ほか Softpal 27 本 (b2023_erectlip など、`tools/verify/status.tsv`) | Softpal (sena-rs) | 本文まで。F9/F11 か Q.Save/Q.Load でセーブとロード (2026-10-28、ネイティブの sena と Chromium)。b2012_erectlip は配布 zip が切れていて NG |
  | クダンノフォークロア 体験版 (b2019_sukerasparo) | BGI (ethornell-rs) | 本編の文章まで。SwiftShader だとタイトルまで 60〜95 秒かかる (実 GPU なら速いはず) |
  | さくら、もゆ。 体験版 (f2019_sakuramoyu) | FVP (rfvp) | 本編の文章まで。セーブ (サムネイル付き) がサーバーに届く |

- 実物で見つかって直したもの (`patches/siglus_rs/`、build.sh が適用する):
  - `0001`: ボイスの時間圧縮 (JITAN) を `std::thread::spawn` でやっていて、wasm では panic してタイトル画面で落ちた。wasm ではその場で処理する。
  - `0002`: wasm の起動部が画面を 1280x720 に固定し、キャンバスの大きさをそのままゲームの座標にしていた (1024x576 の作品は左上に小さく、1920x1080 の作品は切れる)。Gameexe の SCREEN_SIZE をゲームの座標にして、キャンバスに縦横比を保って拡大し、ポインタの座標も換算する (Android / iOS 版と同じやり方)。
  - `0006` (RealLive): 選択肢 (select) の選択肢と選択肢の間に、空行の分の行番号 (`\n` + i16) が余分に入ることがあり、「expected '}'」で解読が止まった (Coming×Humming!! の SEEN0100、「始めから」の直後)。選択肢の前のコンマと行番号をまとめて読み飛ばす。
  - `0007` (RealLive): objSetButton (OBJFRONTSET_BUTTON, 1064) に番号の引数が無いと、どのボタンも 0 を返していた。番号が無ければオブジェクト番号を返す (素直くーるのエピソード選択はオブジェクト番号 0/1/2/4 で分岐する)。
- BGI (ethornell-rs) で直したもの:
  - `patches/ethornell-rs/0001`: ブラウザ版はゲームのフォルダ (`bgi-resource/`) への書き込みを拒否していて、BGI.gdb が書けず「データフォルダを作成することができませんでした」で止まった。ゲームのフォルダへの書き込みはユーザー側の同じ相対パスに置き、読むときはユーザー側を先に見る (コピーオンライト)。
  - `web/src/engines/ethornell.worker.ts`: 押してすぐ離すクリックが 1 フレームに収まると、エンジンはボタンから外れたと読み、タイトルの START でゲームが最初からやり直しになった。同じフレームの「離す」は次のフレームに回す。
  - ethornell-rs は wasm でワーカーに置き、ファイルは同期 XHR の Range 読み (256KB ブロック、128MB の LRU)。上流の Web 版 (platform/wasm) のつなぎを元にしている。
  - `patches/ethornell-rs/0008`: 1.7x の BackML (多層背景) にはプロパティ 0x8E (レコード +0x184、ソース移動の補間カーブ) が増えていて、未対応だと「サポートされていないパラメータ番号 [0x8E]」で止まった (放課後シンデレラ2)。exe の SetProperty (0x423DE0、0x41..0x8F のジャンプ表) で確認。
  - `patches/ethornell-rs/0010` (1.7x のセーブ/ロード画面): スプライトの GetProperty 0x10 (主ビットマップ、HOOKSOFT 2019 の syswnd)、解放済みスプライトへの StartAlphaControl は警告で済ませる (2013 年世代のロード画面)、VM のメモリ上限を 256MB→512MB にして 256MB を超えたら 32MB ずつ伸ばす (2013 年世代のセーブ画面が 63MB×3 を確保して「グローバル領域に空きがありません」)、ネイティブでは / で始まるパスを絶対パスとして扱う (WriteFile のパスが二重になった)。
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
  - System.inputString (KAG の [input]、名前入力) はブラウザの prompt() で聞く。`patches/krkrsdl2/0017` が vnrunPrompt を足し、`polyfill/Compat.tjs` が無いときだけ System.inputString に入れる。キャンセルは void。cross_over 3本は始めからの名字・名前入力で止まっていた。
  - プラグイン DLL は読めないので Plugins.link は何もしない。よく使われるものは TJS の代わり (`engines/krkr/polyfill/plugins/`、Plugins.tjs が Plugins.link を差し替える): csvParser、fstat (ファイル操作はしない)、windowEx (何もしない)、layerExDraw (図形は外接矩形を塗る程度の近似)、scriptsEx、KAGParserEx、addFont (Font.addFont で)、win32dialog (何もしないクラスと定数)。
  - Ogg Vorbis (Windows では wuvorbis プラグイン) は stb_vorbis で読む (`patches/krkrz/0003`、`engines/krkr/stb_vorbis.c` をビルドが krkrz にコピーする)。
  - `patches/krkrz/0004`: TJSInt32ToHex(x, 0) が無限ループでバッファを壊していた (OpenAL のエラーを出すときに落ちた)。
  - 確認: moteyaba 体験版で注意画面 → タイトル → 本編の文章まで。
- 同期 XHR の BOM: `responseText` を読むとブラウザが先頭の BOM を消す/解釈するため、UTF-8 BOM 付きの TJS が 3 バイト欠けて読めなかった。範囲読みに `X-Vnrun-Pad: 1` を付け、サーバーは先頭に 0x00 を 1 バイト足して返す (docs/protocol.md)。全エンジンに効く修正。
- Web ページのゲーム (engine `html`、`web/src/engines/html.ts`): TyranoScript や RPGツクール MV/MZ は元から Web で動くので、ゲームのページを iframe で開くだけにした (描画もゲーム自身。WebGPU 方針の例外として判断を仰ぎ中)。セーブは、サーバーが iframe のページの先頭に `parent.vnrunFrame(window)` を差し込み、プレイヤーがそこで localStorage を GameFS 上のもの (`saves/localStorage/<key>`) に差し替える。IndexedDB はそのまま (RPGツクール MZ は IndexedDB が無いと起動せず、セーブもブラウザの IndexedDB に入る)。起動ページは package.json の main、無ければ www/index.html、無ければ index.html。Tyrano の公式サンプル (github.com/ShikemokuMK/tyranoscript) でタイトル・本編・セーブ・別セッションでのロードまで確認。exe 末尾に zip で付けた NW.js の app (オフセットが zip 先頭基準) と TyranoScript の更新パッチ <projectID>.tpatch は vnrun-prep が展開する (適用済みは .tpatch.applied に改名)。Electron の resources/app.asar も vnrun-prep が同じフォルダ (resources/) に展開する。フレームには require (fs/fs-extra/path の同期版、旧 Tyrano 向けの electron.remote.app、nw.gui・child_process などは何もしない代役)、関数の process (RPGツクールの isNwjs は偽のまま)、Electron 版 Tyrano 向けの studio_api (fs は .sav とフォルダだけ存在、path) を置く。RPGツクールは NW.js と同じく画面をフレームに合わせて拡大し (Graphics._defaultStretchMode を load で真に)、読み込み中にプラグインが投げた例外は警告だけにする。dlsite の html 体験版はタイトル→本編まで確認 (docs/verification.md)。未対応: package.nw (zip)。Electron で main.js がサブフォルダのページを開くもの (app/index.html、dist/index.html。根に index.html が無い) は、浅い順で最初の index.html (node_modules 以外) を開き、require・process・studio_api の代役を置かない (contextIsolation の素の Web ページなので要らず、PixiJS の 梨乃ねね はページに process があるとタイトルのボタンが効かなくなった。原因の箇所は未特定)。engine_gap.tsv の Electron 8 本 (Tyrano 3、PixiJS・GDevelop・自作 HTML 5) は本編まで、セーブのあるものは別セッションでの再開まで確認 (2026-10-28)。
- Ren'Py (r1_renpy、dl_RJ01746340 は Ren'Py 8.3.6 で game/*.rpa): 公式の Web 版 (renpyweb) を使う方針だが、2026-10-28 時点でこの環境からは www.renpy.org がプロキシで切られ (SDK も Web 版も置き場はここだけ)、github.com も 403、Emscripten も入っていないため未着手。
- Softpal (sena-rs):
  - 2026-10-28 時点で targets の Softpal 28 本のうち 27 本が本文まで進み、多くは F9/F11 か文章窓の Q.Save/Q.Load で保存と読み込みも確認した (`tools/verify/status.tsv`)。残る b2012_erectlip は配布 zip が途中で切れている。
  - sena-rs は README のとおり「恋×シンアイ彼女」(2017) を対象に作られていて、ほかの作品は命令の引数の数や戻り値の扱いが少し違うところで止まる。エレクトリップ (Softpal の体験版が 2011〜2025 年に毎年ある) で 2 本試してどちらも止まった。直すには、止まった箇所ごとに旧版の Pal.dll の動きを確かめて sena-rs に版の違いを足す必要がある。
  - ブラウザ版のつなぎ (`web/src/engines/sena.ts`) は描画・入力・ファイルの範囲読みまで。上流の Web 版にはセーブと音声が無い (セーブは std::fs に書いていて wasm では失敗する) ので、動く作品が出てきたら足す。
  - `patches/sena-rs/0001`: スクリプトが止まったとき、上流の Web 版は最後の画面のまま黙って止まっていた。理由をページのエラーとして出す。
  - `patches/sena-rs/0002`: sena-rs は CPU で画面を合成する。できたフレームは Canvas2D ではなくページの Presenter (WebGPU) に渡し、ポインタの座標もページがフレームの座標に直して渡す。
  - `patches/sena-rs/0003`: 版によって同じ extcall の引数の数が違い (爆乳淫乱ビーチの 2:48 は 1 個、恋カケは 2 個)、多く取ると呼び出し元のフレーム (引数マーカーと arg_base) を食って「script stack underflow」になる。スクリプト呼び出しごとにフレームの底を覚え、extcall がそれより下を取ったら戻す。これで 2014〜2025 年のエレクトリップ作品がタイトルまで出る。Game.exe の各ハンドラの pop 数を静的に数える方法は、ヘルパー関数経由の pop があって正確に出せなかったので採らなかった。
  - `patches/sena-rs/0004`: ANI (スプライトの動きの小さなバイトコード) は `[fc 13] fd` の後に 00/12 移動、01 待ち、02 拡大、03 不透明度を並べ、`fe` 待ち・`ff` 終わり、ループ (0a など) が続く。上流は先頭 1 項目しか読まなかったので、待ちのあるタイトルボタン (ANI_BTN_EFFECT0n: 遅延 + フェードイン + 移動) が透明のままだった。最初のブロックの項目をまとめて流す (遅延とループは未対応)。
  - `patches/sena-rs/0005`: SYSTEM.INI の DEF_CG_WIDTH/HEIGHT は CG の大きさで、画面の大きさではない作品がある (ドスケベクエスト: CG 1540x980、画面 1280x720)。Game.exe が窓を作るときの `push 高さ; push 幅` を探して画面の大きさにする。ずれていると座標がずれてタイトルのボタンが押せなかった。あわせて文字列を長さで切る extcall (18/7) を Shift-JIS のバイト数で切るようにした (UTF-8 の文字の途中で切ってパニックしていた)。
  - `patches/sena-rs/0006`: 2011-2012 年の版 (りとるびっち体験版で確認)。`sp_set_ex` が z なしの 4 引数なので、スクリプトの呼び出し直前の push の数を数えて見分ける (`CoreAssets::short_extcalls`、`old_softpal()`)。引数の数が違う extcall は足りない後ろの引数を 0 で補い、多い分は捨てる (sp_set_ex 4→5、bgm_play 4→7、se_stop 3→2)。SYSTEM.INI がなく画面は exe の 800x600 で、スクリプト座標から内部の 1920x1080 への倍率を 1.5 固定から画面の比に。ボタンの flags 0 はマウスで押せる扱い、押したボタンの点へは gosub ではなくジャンプ (ハンドラの `ret` がボタン待ちの関数から抜ける)。
  - `patches/sena-rs/0007`: 実装の無い extcall (引数の数も分からないもの) は、スクリプトがその直前に push した数だけ捨てる。残すと呼び出し元が保存した arg_base がずれ、temp_mem に 2^28 番地を書こうとしてブラウザで capacity overflow になった (爆乳遊廓の 0x12_0033)。ネイティブで同じ所に行くには 1500 フレームほど回す必要があった。ブラウザ版の wasm は関数名を残す (`--keep-debug`、約 1MB) ので panic の呼び出し元が見える。
  - `patches/sena-rs/0008`: セーブのスナップショットが 8 MiB を超えるスプライトを落としていて、読み込むと立ち絵 (2048px) が消えた。
  - `patches/sena-rs/0009`: 2011-2012 年版の ext 18:17 は引数なしで、Game.exe が自分に埋めた識別値を返す (exe を読み、最後の `/**/` の後の dword を 0xFEFEFDFC で xor して上下が一致するか確かめ、続く 12 バイトのうち 4 バイトを混ぜる。淫落痴漢電車 chikan_try.exe の sub_403D40 で 6263)。登録やコピー保護の確認ではない。末尾が切れたアーカイブも開く。
  - `patches/sena-rs/0010`: セーブとシステムデータを asset の VFS 経由で書く。ページ (`senaWriteFile`) がサーバーに置く。
  - `patches/sena-rs/0011`: Pal.dll の PalFontDrawText が効果 3 (縁取り) を描く版かを switch の case 数で見る。恋カケ前提で効果 3 を描かず、ちんかも2 の文章が見えなかった。0x010000NN のボタン参照 (レイヤー付き) のフェード。
  - `patches/sena-rs/0012`: (1) 後年の版の 17:43 は「スロット、dx、dy、dz、カーブ番号、時間」のイージング付き相対移動 (種類 26)。カーブ 16〜48 は exe の表 (linear, sine, quad, cubic, quart, quint, expo, circ, back, elastic, bounce の in/out/in-out) と同じ式で、全 33 本を unicorn で動かした exe の値と 2e-3 以内で照合。extended (18:121 の時計を使う版) では 1920 座標のまま足すので 1.5 倍する。17:36 はバンクが動いているか。こはなたの START ボタンが出る。(2) 未登録版でも 0x02G0XXXX のボタン参照に pos_move を効かせる (爆乳淫辱霊媒師のタイトルの並び)。(3) 幅の狭い文章窓は text_set_base の x,y (無ければ PGD のオフセット) に置く (養花師)。(4) 9:24 (list_stack_pop_count) で押した点が残っていなければ呼び出しの戻り先そのものを外し、続く 9:23 はその点を戻り先として積む。2014 年版のタイトルの Start ハンドラはタイトルの待ちループから gosub され、自分の戻り先を外して `ret` でタイトルを抜ける (巨乳不良少女)。(5) Pal.dll の名前が C++ 修飾 (`?PalGetChangeWindowModeTime@@...`) で PalSystemPathInit が無くても、exe が起動時に `push handler; push 0x00CC00II` で extcall を 200 個以上登録していれば登録版として扱う。9:63 は is_active (常に 1)。つまもみの注意書きが進む。
  - ヘッドレスのクリックは押して次のフレームで離すので、get_on_key (前フレームから押されたままのキー) には出ない。つまもみの注意書きのように押しっぱなしを待つ画面は `--diagnostic-key-hold <開始>:<終了>:Enter` で進める。人の操作 (数フレーム押す) なら問題ない。
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
  - 2010 年の JINKI 体験版は init と文字列変数名のない旧スクリプト形式。旧式の分岐・ループと一部の組み込み命令を実装し、ネイティブと Chromium でタイトル表示と本編最初の台詞を確認した。System.dat の画面寸法を使い 1024x600 で描き、メッセージを画面内に収める。画像・スプライトの作成、移動、拡大、削除も対応。タイトル演出の単発効果音と BGM、本編の最初のボイスとチャンネル付き効果音はアーカイブから読み Chromium で再生開始を確認。BGM のループ位置と START 後の停止も確認した。旧版分岐・ループの仕様は命令列からの推定で、残る命令、他の音声、効果音の停止・フェード、セーブ、本編の続きと共に要確認。2009 年 crossover は NeXAS ではない (下記)。
  - 2006 年の版 (angel breath、`builds.rs` の B2006_BUILD): VM に剰余が無く演算子 0xD 以降が 2007 年版より 1 つ小さい。Config の表はヘッダの無い固定長レコード (`dat::parse_2006`)。タイトルのボタンとメッセージ窓は .ani ("ANI\0"、絵の名前、フレーム (x, y, w, h, 中心 x, 中心 y, ?) とパターン)。立ち絵は .bmp の体と同じ大きさの 8 ビットのマスク .fil (上の行から)、表情 .bmp (黒は抜く) を Stand.dat の画面座標で重ねる。0x8d 表示 (行, 位置 17 左/18 右/21 中央, ?, 時間, 待たない)、0x8e 消す、0x8f 位置を移す。System.dat が無いので 800x600。
  - b2009_crossover (CrossOver の自社 Adv.exe): DATA/*.GR2・*.VIC・SCRIPT.PAC は `PACK` (個数, データ開始, 0, 24 バイトの項目 名前[16]・大きさ・位置)。GR2 の中身は "LL5\0" で包まれた "GR2_" (LL5.dll の圧縮)、.SCD は "*Pola*" 圧縮 (GARbro の Tactics の LZ とは別物)。2026-10-28 に専用エンジン `crates/crossover` で対応した (下の「CrossOver (Adv.exe) の実装メモ」)。
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
| Interheart (Candy Soft) | 11 | 自作 (`crates/interheart`, `crates/interheart-web`) | Rust | あり (WebGPU) | FPK アーカイブ (平文・XOR 索引・"FPK 2.00"、ZLC2 圧縮)、KG / BMP / PNG の画像、E-mote (.EMT) は altfile の静止画。シナリオは data.fpk の ACT_<字>.TXT (ゲーム本体も文章をここから読む) をそのまま解釈: シーン (***SC)・選択肢 (***SS)・EF_/CG_/DG_/KM_/FCG_/BG_/SE_/BGCV_ の命令と台詞 (名前　（ボイス番号）)。タイトル・メッセージウィンドウ・選択肢は exe 内にあるため自前で描く (タイトルは titlechip.kg と TitleTM1.hmp のボタン領域。2018 年以降は exe の .data にあるボタン表 (下の「Interheart の 2018 年以降のタイトル」) から titlechip の本物の部品で描く。2018 年以降はメッセージウィンドウ・名前枠・選択肢の位置も exe から読み MesChip の本物の部品で描く (下の「Interheart の 2018 年以降のメッセージウィンドウと選択肢」)。どちらでもなければ自前の半透明パネル。立ち絵の位置は exe の表 320/640/960/0/1280/480/800/160/1120)。セーブは自前の 10 スロット画面 (右クリックのメニューから)。11 本中 10 本 (2013〜2026) でタイトルから本編まで、Chromium でセーブ→ロード可。2006 年のオイラは番台２は脚本の方言が違い NG。2018 年以降はウィンドウのボタン (ログ・オート・スキップ・Q-SAVE/Q-LOAD・セーブ/ロード・ウィンドウ消し・ボイス再生) も動く。残る課題: 設定画面など一部のボタンは無反応。立ち絵の揺れ (EF_A, EF_C0040〜)・ムービー・コンフィグ・バックログ・回想はまだ |
| Crowd / ANIM | 9 | 自作 (`crates/anim`, `crates/anim-web`) | Rust | あり (WebGPU) | `<game>_sce.dat` (GAX 暗号: 先頭 00 00 00 01、16 バイト鍵がブロックごとに変わる) のバイトコードを解釈。ヘッダはコード末尾と文字列表の位置、コードは 0x0C から、文字列は CP932 の 0 区切り。命令の引数の並びは exe の解釈ループ (switch) から読んだ表 (B/H/I、V は 3D/3E なら変数番号、C は個数付きの文字列番号列)。2008〜2010 年は同じ表、2009 年は e6/e7 に数が 1 つ多く 41 が待ち。2011 年は番号が付け直された別表 (V は常に種別バイト付き、0a は行内のラベル文字列、54 は種類で長さが変わる場面転換) を 2008 年の番号に写して動かす。画像は GAX (暗号化 PNG、アルファ付き、1024x1024 の器に 1024x768)、UI は pMenuTrial/pMenuTrialButton/pMessageWindow/pSelect で位置は exe の数値。画像は CWP (署名とチャンク名を抜いた PNG、R/B 逆)、立ち絵 M* は左半分が絵・右半分がマスク。BGM は bgm/NN.eog (CRM ヘッダ + Ogg)、ボイスと効果音は voice/<game>_wave.pck。タイトル (p_trialmenu の下にボタンの点灯版)・メッセージ枠 (p_message)・選択肢 (p_select) はゲームの絵を使い、ボタンの位置は年ごとの表 (2008 斜め 3 つ、2009 1024x768 で縦 5 つ、2010 縦書き 5 つ)。2009 年は 1024x768 の画面に 1024x576 の絵を上下の帯の間に置く。セーブは自前の 10 スロット (右クリックのメニュー)。9 本中 5 本 (2007・2008・2009・2010・2011) でタイトルから本編まで、Chromium でセーブ→ロード可 (2007 年は本編まで)。2007 年も番号の違う別表 (変数の種別が 3E/3F と 1 つ上で、パーサーで 3D/3E にそろえる。7b は時刻を変数へ入れる命令で、待ちはそれを回すループなので読むたびに 1 フレーム譲る)。2007 年の画面は 1024x768 で上 48 の帯 (p_message の上端) と下 144 の枠 (その下) の間に 1024x576 の絵、選択肢は p_select が白いマスクだけなので自前の帯、タイトルは p_menu に自前の文字ボタン (本来の p_entrance のメニューは未対応)。2017 年以降の 4 本はシナリオが Enigma Protector で包まれた exe の中で NG (保護を外すのはコピー保護の回避にあたるため試みない)。残り: 演出 (51 の雲など)、画面効果 4d の種類、コンフィグ・バックログ、2009 年のメッセージ枠 (p_message が無く自前の帯) |
| Majiro | ? | 自作 (`crates/majiro`, `crates/majiro-web`) | Rust | あり (WebGPU) | MajiroArc V1〜V3 アーカイブ、.mjo (暗号化を含む) をそのまま動かす VM (スレッド、イベントフック、setskip/デストラクタ)、RCT/RC8 画像 (TC01 の差分、`_.rc8` マスク)、ページ・スプライト (中心基準の _ext)、`$console_*` の文字をページに描いて CONSOLE_WROTE などのイベントで console.mjo に渡す。Closed GAME 体験版 (2015) でタイトルから本編の文章まで (majiro-run と Chromium)、QSAVE/QLOAD (画面上端のバー) でセーブ→ロードすると保存した文章と場面に戻る (体験版は SAVE/LOAD 画面が「体験版では使用できません」)。スプライトアニメ・選択肢以降はまだ |
| Kaguya | ? | 自作 (`crates/kaguya`, `crates/kaguya-web`) | Rust | あり (WebGPU) | 2006 年頃の世代 (GAME_SYS/AMATU.exe + WFL1 の .arc/.ari)。WFL1 アーカイブ (LZSS)、AP/BMP 画像、tblstr の文章と名前、.scr バイトコードの VM (変数・条件分岐・スクリプト移動)、背景・立ち絵 (胴体+表情の合成)・イベント CG・色かけ、ルール画像トランジション・フェード・フラッシュ・揺れ・カットイン、BGM/効果音/音声 (Ogg)、メッセージ窓とタイトルメニューは exe の代わりに vram01/02 の部品で自前描画。アマツ体験版 (2006) が最後まで通る (kaguya-run と Chromium)。2013 年頃の世代 (GAME_SYS.exe + ARC/、1024x600、label.tbl の GOSUB、ローカル変数、選択肢、APS3 部品画像、AO 立ち絵) も肉牝R30 体験版で本編と場面選択まで。2009 年の GAME_SYS.exe (さらさらささら体験版) も最後まで。2008 年 (夏神 体験版２) も場面選択と各場面まで。LINK の世代 (Start.exe + params.dat + message.dat + LINK6 の .arc、`kaguya::link`) もハラミタマ体験版 (2015) がタイトル→はじめから→プロローグ (A0000..A0070)→共通→マップ (4 人から選ぶ、文字の選択肢で代用)→各ルート①→体験版の終わりまで通る。LoveHolic 体験版 (2018、params v05.7) も同じ実装で最後まで。セーブは自前の 10 スロット画面 (文章表示中に右クリック / Esc、タイトルの「つづきから」系はロード画面)。アマツとハラミタマで Chromium のセーブ→別セッションでタイトルからロードを確認。2013 年のマップ画面 (op 54 1 0)・LINK の他の年 (2010/2011/2014/2019) は未確認 |
| Cyberworks / TinkerBell (Arc0x.dat) | 8 | 自作 (`crates/cyberworks`, `crates/cyberworks-web`) | Rust | あり (WebGPU) | 索引と中身の組 (Arc01→04, 02→05, 03→06, 07→08)。索引は 10 進の桁 (^0x7f) の大きさ + LZSS。種類の文字ごとの 0 始まりの番号で参照。'a'/'d' 画像 (GetInt 符号の見出し、d は直前の土台との差分)、'b'/'c' は末尾の PNG、音声は先頭 "Tink" の Ogg を exe の鍵で XOR。脚本 'a' は文 (M# 命令 / M$ レイヤー命令 / S 文章) を直接解釈し、e0 (ボタン組) と f0 (レイヤー表) からタイトル・ウィンドウ・立ち絵の位置を組む。メッセージ窓の文字・選択肢・セーブ (10 スロット) はエンジンが描く。グランギニョルの夜 体験版 (2018) で警告→タイトル→本編、セーブ/ロード (cyberworks-run と Chromium)。2023 年以降の ChakraCore 入りの世代 (Resources/DataNNN.dat の絵、UTF-16 の文章) もモラトリアム・ムジナ・臭・友恵の探偵物語の体験版で本編とセーブ/ロードまで |
| LiLiM (AOS) | 7 | 自作 (`crates/lilim`, `crates/lilim-web`) | Rust | あり (WebGPU) | .aos アーカイブ (2009 年以降の版 2 と 2006〜2007 年の版 1)、.scr と .cmp のハフマン符号、BMP / ABM 画像 (ABM の詰めた行は上から)。脚本は平文 (Shift-JIS) のテキストで、行ごとに解釈する (^gsb/^go/^jp/^if/^sw、%変数、bgon/cgon/chon/ud/btnset/btnenable/cvon/bgmon など)。ボタンは画像の 3 段 (通常・ポインタの下・押下)。タイトルと選択肢はゲームの脚本のボタンそのもの、メッセージ窓は wnd(0, ...) の設定 (msgwnd1 と窓のボタン) で描く。セーブ・ロードの画面と右クリックのメニューはエンジンが描く。体験版 7 本 (2006〜2021) すべてでタイトルから本編まで (lilim-run、うち 3 本は Chromium も)。ud(2) のマスクはフェードで代用 |
| ShiinaRio (椎名里緒) | 2 | 自作 (`crates/shiina`, `crates/shiina-web`) | Rust | あり (WebGPU) | WARC 1.7 アーカイブ (GARbro の鍵を `schemes/*.bin` に)、S25 画像 (複数フレーム・各フレームに置き場所、差分行)、OGV (Ogg の前に見出し)。ゲームの .SCN は VM で動かさず、シナリオ (A001.TXT.. の平文) を直接解釈する: `$L_BG` `$L_CHR` `$DRAW` `$DRAW_EX` `$VOICE` `$SE` `$MUSIC` `$MUSIC_FADE` `$EX,5`、`【名前】` と文章の段落、`;￥ジャンプ＜a002＞へ` で次のファイル。タイトル (SYSTITLE.S25) とメッセージ窓 (SYS_SYS.S25) はゲームの絵をエンジンが並べる。種憑け村 序章体験版 (2011) でタイトル→本編→体験版の終わり→タイトル (shiina-run と Chromium、セーブ/ロードも)。マスクのワイプはフェードで代用、EFECT (揺れ・フラッシュ) はまだ |
| Silky's (AI6WIN 世代) | 2 | 自作 (`crates/silkys`, `crates/silkys-web`) | Rust | あり (WebGPU) | Script/data/effect/layer/music/voice.arc (索引の大きさ + 名前を n-i ずらした項目、LZSS)、AKB 画像 (下の行から、左と上の差分)、.mes のスタック VM (LIBLARY.LIB の関数、システムコール 1..29 は exe の CFunc の番号)。スプライト・BG・立ち絵・イベント CG はスクリプトの命令どおり、タイトル (29/18)・メッセージ窓・選択肢・セーブ画面はエンジンが描く (窓とボタンの位置はゲームごとの表)。ママのおっぱい 体験版 (2016) と なないろリンカネーション 体験版 (2014) でタイトルから本編、Chromium でセーブ→ロード |
| Ivory (ags.exe) | 2 | 自作 (`crates/ivory`, `crates/ivory-web`) | Rust | あり (WebGPU) | misc/*.pk・sound/*.pk・voice/voice.pk (fPK/fPK2、索引と名前は 32 ビット語の入れ替え + xor)、SG 画像 (cRGB の 4 方式 / 暗号化 JPEG、画面上の位置つき)、PX 音声 (ヘッダー + Ogg)、OCB スクリプト (cTEX/cFNM/cCOD、u16 命令 + u32 引数)。背景・立ち絵・レイヤーの移動はスクリプトどおり、タイトル・窓・選択肢・セーブ画面はゲームの FRAME_*/TITLE_*/SELECT_*/SAVE_BASE の絵でエンジンが描く。お嬢様はイイナリみたいです！？ 体験版 (2016) と はじカノ 体験版 (2020) でタイトルから本編を最後まで、Chromium でセーブ→ロード |
| NonColor (ACV1 / DAT) | 2 | 自作 (`crates/noncolor`, `crates/noncolor-web`) | Rust | あり (WebGPU) | script.dat・arc0..2.dat (ACV1: 件数 xor 0x8B6A4E5F / 古い DAT: xor 0x26ACA46E、名前の CRC-64 で引く、flag 2 は zlib を u32 語で xor (鍵はゲームごと、zlib ヘッダーから総当たり)、flag 1 は名前で xor)、TLG5/TLG6 (TLG0 sds の tags で顔の位置)、NScripter 風テキストスクリプト (DAY の defsub マクロを高水準で解釈)。タイトル・窓・選択肢は sys/title・sys/window の絵、セーブ画面は自前。宿星のガールフレンド２ 体験版 (2018) と 姉小路直子と銀色の死神 体験版 (2015) でタイトルから本編を最後まで、Chromium でセーブ→ロード |
| AZ System (ARC) | 1 | 自作 (`crates/azsys`, `crates/azsys-web`) | Rust | あり (WebGPU) | *.arc ("ARC\x1a"、索引は独自 LZ)、CPB (チャンネルごとの zlib)、ASB (u32 語から鍵を引いて zlib、鍵は zlib ヘッダーと CRC32 から総当たり)、TBL/TBL2 の表 (cg・chara・window)。タイトルのボタン・窓のボタン・クリック待ちはゲームの絵、文章の箱とセーブ画面は自前。Triptych 体験版 (2006) の 4 本のスクリプトを最後まで、Chromium でセーブ→ロード |
| PJADV (GAMEDAT PAC) | 1 | 自作 (`crates/pjadv`, `crates/pjadv-web`) | Rust | あり (WebGPU) | GAMEDAT PAC2 (*.dat)、EPA (チャンネルごとの LZ)、scenario.dat のバイトコード (変数・呼び出し・ボタン群・スプライト層とアニメ)、graphic.def / BGM.ini。タイトル・窓・ボタン・顔はゲームのスクリプトどおり、セーブ画面は自前 (窓の SAVE / LOAD と右クリック)。ななついろ★ドロップス体験版 (2006) を最後まで、Chromium でセーブ→ロード |
| Eomaia (FG REMAKE、.arc) | 1 | 自作 (`crates/eomaia`, `crates/eomaia-web`) | Rust | あり (WebGPU) | .arc (名前を難読化した索引)、SCRE スタック型バイトコード (絵・音・窓・選択肢・タイトル・マップは名前と番号で受けて自前、日・時間・居場所の計算をするライブラリはそのまま VM で)、PNG・JSON シート・.char 立ち絵、1920x1080。セーブは自前 (右クリック / Esc)。夜勤病棟 リメイク体験版を最後まで、Chromium でセーブ→ロード |
| Cyberworks / TinkerBell (Arc0x.dat) | 8 | 自作 (`crates/cyberworks`, `crates/cyberworks-web`) | Rust | あり (WebGPU) | 索引と中身の組 (Arc01→04, 02→05, 03→06, 07→08)。索引は 10 進の桁 (^0x7f) の大きさ + LZSS。種類の文字ごとの 0 始まりの番号で参照。'a'/'d' 画像 (GetInt 符号の見出し、d は直前の土台との差分)、'b'/'c' は末尾の PNG、音声は先頭 "Tink" の Ogg を exe の鍵で XOR。脚本 'a' は文 (M# 命令 / M$ レイヤー命令 / S 文章) を直接解釈し、e0 (ボタン組) と f0 (レイヤー表) からタイトル・ウィンドウ・立ち絵の位置を組む。メッセージ窓の文字・選択肢・セーブ (10 スロット) はエンジンが描く。グランギニョルの夜 体験版 (2018) で警告→タイトル→本編、セーブ/ロード (cyberworks-run と Chromium)。2023 年以降の TinkerBell は ChakraCore 入りの別世代 |
| LiLiM (AOS) | 7 | 自作 (`crates/lilim`, `crates/lilim-web`) | Rust | あり (WebGPU) | .aos アーカイブ (2009 年以降の版 2 と 2006〜2007 年の版 1)、.scr と .cmp のハフマン符号、BMP / ABM 画像 (ABM の詰めた行は上から)。脚本は平文 (Shift-JIS) のテキストで、行ごとに解釈する (^gsb/^go/^jp/^if/^sw、%変数、bgon/cgon/chon/ud/btnset/btnenable/cvon/bgmon など)。ボタンは画像の 3 段 (通常・ポインタの下・押下)。タイトルと選択肢はゲームの脚本のボタンそのもの、メッセージ窓は wnd(0, ...) の設定 (msgwnd1 と窓のボタン) で描く。セーブ・ロードの画面と右クリックのメニューはエンジンが描く。WhiteBlue web体験版 (2019) が最後まで (lilim-run と Chromium)。ud(2) のマスクはフェードで代用 |
| Yuka | 7 | 自作 (`crates/yuka`, `crates/yuka-web`) | Rust | あり (WebGPU) | YKC001 アーカイブ (*.ykc、古い作品は同じ形式の *.dat。索引は 20 バイトずつ、CP932 のパス)、YKS001 スクリプト (データ部は 0xAA の XOR。コードは要素番号の列で、要素は関数・制御 (`{` と `}` が互いを指す、ラベルは自分を指す)・数・文字列・変数。呼び出しは関数・引数の数・引数、代入は対象・`=`・数・演算子をはさんだ項で、引数の数 0 は次の呼び出しの結果。数の要素への代入はその値を書き換える (添字付きの変数))、YKG000 画像 ("GNP" の署名の PNG + 白が透明のマスク PNG + 0x20 バイトのフレーム表、空のフレームでパターンを区切る。立ち絵のマスクは別ファイル *_M1.YKG のことも)。MouseSelect/Select はクリックでブロックを回し、矩形にクリックが入ったケースのブロックへ。MouseCase は 13 個目の数があると全画面の受け皿 (他のケースに当たらないときだけ)、MouseCase2 は先頭が有効フラグで点灯の後に 3 つ目の絵 (クリック音は 13 番目)。MouseKeyCase/KeyTableCase/IndicateCase はクリックでは選ばない。GraphicTextRect/TextWindowSet は普通は左上右下、右下が左上より小さいときは幅と高さ。`@gf(n)` の文字列は GraphicFontSet の絵のパターン n (名前の画像)。SpriteCreate は他のレイヤーのパターンを置く (複数フレームはアニメ)。ゲームのセーブ・ロード画面 (m_save/m_load.yks、savemenu/loadmenu.yks) は自前の 10 スロットのメニューに置き換え、右クリックでも開く。7 本すべてでタイトルから本編まで (yuka-run と Chromium)、セーブ→ロードは 3 本 (FairlyLife・あかね色・星空へ架かる橋AA) で確認。残り: ルビの表示、バックログ・コンフィグ、文字の表示効果、名前入力 (既定の名前のまま) |
| NekoSDK / Waffle (gamedata/*.pak) | 5 | 自作 (`crates/neko`, `crates/neko-web`) | Rust | あり (WebGPU) | NEKOPACK4A/S アーカイブ (GARbro NekoSDK/ArcPAK.cs に従う)、BMP + .alp の絵、NEKOSDK_ADVSCRIPT2 の脚本 (行 = 53 個の整数 + 33 個の文字列、int 36 が次の行)。タイトルとメニューは exe が描くので、obj の絵からエンジンが組む。2012 すくみず食べ放題・2014 巨乳ファンタジー２if・2016 巨乳ファンタジー外伝２after・2017 隣人に壊されていく俺の妻・2010 裸の王様 (セットアップ.EXE は LHa の SFX、7z で展開) の体験版で本編とセーブ/ロード (neko-run と Chromium) |
| DxLib MED (md_*.med) | 3 | 自作 (`crates/med`, `crates/med-web`) | Rust | あり (WebGPU) | MDN アーカイブ (GARbro DxLib/ArcMED.cs)、DXA v4 (ArcDX.cs、12 バイト鍵を見出しから推測)、PRS の絵と WADY の BGM (Marble と同じ、`tmrhiro::marble`)、md_scr のコンパイル済み脚本 (命令表は exe のコンパイラの switch から)。2018 催眠学園１年生 (筆柿そふと)・2012 つぼい君のスイッチ！ (筆柿そふと)・2012 モンスターパーク２ (trois) の体験版で本編とセーブ/ロード (med-run と Chromium) |
| Liar-soft XFL (scr.xfl / grp*.xfl) | 2 | 自作 (`crates/liar`, `crates/liar-web`) | Rust | あり (WebGPU) | XFL アーカイブ (GARbro Liar/ArcXFL.cs、入れ子の .xfl も)、WCG の絵と LWG の絵の組 (ImageWCG.cs / ArcLWG.cs)、GSC のコンパイル済み脚本 (命令の意味は exe のインタプリタから)、Ogg を包んだ WAV (形式 0x6771)。2022 南国プリズン (evoll)・2019 アイドル裏営業 (channel-ubai) の体験版で本編とセーブ/ロード (liar-run と Chromium) |
| CrossOver Adv.exe (DATA/*.GR2, LL5.dll) | 1 | 自作 (`crates/crossover`, `crates/crossover-web`) | Rust | あり (WebGPU) | PACK アーカイブ、LL5 で包んだ GR2_ 画像 (BGR24/15bit、マスクは別の絵)、*Pola* 圧縮の SCD スクリプト (スタック VM、命令の意味は Adv.exe の表 0x42c190 から)、Ogg を包んだ VIC (形式 0x674f)。2009 crossover 体験版でロゴ → タイトル → 本編 → 選択肢 2 つ → 終わりでタイトルへ、セーブ/ロード (crossover-run と Chromium) |
| Propeller (*.mpk, BB.exe / sukima_trial2.exe) | 2 | 自作 (`crates/propeller`, `crates/propeller-web`) | Rust | あり (WebGPU) | MPK アーカイブ (索引を 1 バイト鍵で XOR、.msc は 0x88 で XOR)、LZ 圧縮の BMP を包んだ MGR 画像 (複数コマ)、MSC スクリプト (分類バイト + 命令バイト + 被演算子、7 種のスレッド/文脈、命令の意味は BB.exe から)、素の Ogg (.ogv)。2007 Bullet Butlers 体験版でロゴ → タイトル → 本編 → 選択肢 3 回 → co007 の終わりでタイトルへ、セーブ/ロード (propeller-run と Chromium)。2011 すきま桜体験版２ (幅の広いスクリプト形式、1024x576) で序章 → タイトル → sa_01〜sa_10 (選択肢多数) → タイトル、体験版2 メニューの各ルート、セーブ/ロード (propeller-run と Chromium) |
| System-NNN | 6 | 自作 (`crates/systemnnn`, `crates/systemnnn-web`) | Rust | あり (WebGPU) | Black Cyc・Cyc Rose・metalogiq の System-NNN。GTB/GPK の画像パック (名前の先頭 2 文字がパック、古い作品はばらの dwq/<xx>/*.dwq)、DWQ 画像 (0x40 バイトのヘッダの PACKTYPE: 5 JPEG・7 JPEG+マスク・8 PNG・0/2 BMP (上から、RGB)・1/3 圧縮 BMP (0 とゼロの数、前の行と XOR)、マスクは 8bit 圧縮 BMP のパレットの灰色)、VTB/VPK の声と VAW/WGQ の音 (RIFF・ビット詰め PCM・Ogg)、init/*.xtx か init2/*.fxf (0xFF の XOR) の設定、spt/*.spt (0xFF の XOR) のスクリプト。VM は [長さ, 種類, コード, 引数] のレコード (制御 66660002・選択肢 66660003・ゲーム関数 66660004・式 66660005・文章 66660006・命令 66660007)。変数 30 がバックスクリプトのモード (3 オープニング、4 タイトル)。ゲーム固有: BDD の依頼の掲示板 (bdd/irailist.xtx、関数 4/3 と引数なしの選択肢)、EXTRAVAGANZA のチャート (chart/chart*.xtx/.fxf、関数 7/3/2 と引数なしの選択肢、55560005 で先頭のスクリプトをやり直す)。セーブは自前の 10 スロット。6 本すべてでタイトルから本編まで (nnn-run と Chromium)、セーブ→ロードは BDD で確認。残り: エフェクトの多く (番号の分かるもの以外は素の表示)、ムービー、バックログ・コンフィグ、ミニゲーム |
| Bishop (BSXX) | 6 | 自作 (`crates/bishop`, `crates/bishop-web`) | Rust | あり (WebGPU) | Bishop の BSA アーカイブ (BSArc、版 3 は 12 バイトの索引と名前の塊、版 1/2 は 0x28 バイト、>dir と < でフォルダ)、BSG 画像 (BSS-Graphics、BSS-Composition の包み、BGRA/BGR/パレット、素・RLE・LZ (差分) の面ごと、下から)、bsxx.cfg (Shift-JIS、値に足し算・掛け算) の配置。スクリプトは bsxx.dat の BSXScript 2.0/3.3 (合わなければ全体を NOT、3.3 は表の組が 4 バイトずれて話者の前に表が 2 つ多い、UTF-16) と bsx.dat の BSScript Rev.6 (2007〜2008 年、同じ並びで Shift-JIS、命令番号を 2.0 に読み替え、配置はプログラムの中なので REV6_CFG)。レジスタ VM (即値・文字列・システム/グローバル変数・ローカル・レジスタ) で、1 バイトの命令はローカル 1〜4 を引数にとる。絵は層 (種類 1 背景で全消去、2 名前で追加、3 立ち絵で体と顔を合成、3.3 は命令 39 で x 付き)、変化 (フェード・マスク) で表に出る。メッセージ窓は灰色の形を bg color で塗る。Rev.6 のマップは呼び出しの後の #result の比較から行き先を一覧で選ぶ。セーブは自前の 10 スロット (右クリックのメニュー)。6 本すべてでタイトルから本編まで (bsx-run と Chromium)、セーブ→ロードは裏教師 (2.0) と姫奴隷 (Rev.6) で確認。残り: 選択肢・マップの本来の画面、エフェクト (アクション・画面効果)、目パチ・口パク、ムービー、バックログ・コンフィグ、Rev.6 のタイトル BGM |
| Eushully AGE | 4 | 自作 (`crates/eushully`, `crates/eushully-web`) | Rust | あり (WebGPU) | sys4ini.bin (S4IC/S4AC、LZSS の索引: アーカイブ名、ファイルごとに名前・アーカイブ・位置・大きさ。並び順がスクリプトから見たファイル番号、"@" は空き) と .alf、AGF 画像 (ACGF、LZSS のビットマップ、ACIF の α)、AOG/OGG/WAV の音。スクリプトは SYS4 (SYS4421) の 32 ビット語の命令 (命令番号と引数ごとの型・値、文字列は 0xff で XOR、引数の数は Kelebek1/Eushully-Decompiler の表)。タイトルもメニューもメッセージ窓もスクリプトが描くので、こちらは VM (グローバル/ローカル整数・文字列、lookup-array のポインタ、call/ret、call-script、switch)、テクスチャ (AGF 読み込み・塗り・文字列描画) とスプライト (番号順、位置・α・加算・色の乗算・拡大・移動のアニメ)、テキスト窓 (位置・色・影)、入力コールバック (0x10 の bit を持つマウス番号 (0x10・0x32) を毎ティック、パッド 0〜10)、clear/fill-array (0x6c・0x2d8)、設定の保存 (string-lookup-set / u00428010)。AGE.EXE はパッカー付きなので解析はスクリプトと上の表だけから。姫狩りダンジョンマイスター (2009)・戦女神VERITA (2010)・珊海王の円環 (2016、1280x720)・天冥のコンキスタ (2020、1600x900) でタイトルから本編まで (age-run と Chromium)。メッセージの折り返しは窓の右端 (u0041B820) と画面の右端の近い方。残り: SYS5 (UTF-16、珊海王 2016 はまだ SYS4)、202 の到達値 -1 は呼んだ時点の α (直後の 203 が始まりの α、メッセージ窓の下地がこれで出る。姫狩りの同じ形は未確認)、セーブ・ロード、未実装の命令 (回転・一部のアニメ・クロスフェード)、ゲーム部分 (ダンジョン経営) の画面 |
| Atelier G/H | 1 | 自作 (`crates/ateliergh`, `crates/ateliergh-web`) | Rust | あり (WebGPU) | .arc (u32 個数・1、44 バイトの項目: Shift-JIS の名前 32 バイト・大きさ・格納の大きさ・位置。ヘッダと項目は短い鍵 (msg/dat は 3 バイト、sys は 5 バイト) を位置ごとに足して隠してあり、名前の後の 0 埋めから鍵を求める)、大きさが違うものは LZSS (0xfee、bit 1 が生)。絵は BMP (32 ビットは 4 バイト目が α)、設定は sys.arc の .lib (各バイト +0x82 の CSV)、文章は msg.arc の平文スクリプト (`<Tag a,b>`、`[label]`、`//`)。画像の層 (ImageLoad/ImagePos/ChrInit) は UpDate で前の画面からフェード、メッセージ窓 (win_a.bmp)・顔・名札 (name.bmp と name.csv)、選択肢 (SltAdd/SltRun、自前の枠)、マスク選択 (SltMask*)、Jump/JumpR/Return/If/Mov、BGM・効果音・声。タイトルは組み込み (*_title.bmp と *_title_parts.bmp、ボタン表は system4-1.lib / system4.lib)。ImageShake (正弦波の揺れ)・ImageNegaposi (色を反転)。セーブ・ロードは自前の 12 枠 (右クリック/Escape で開く。本文待ちならセーブ、タイトルならロード。aghsaveNN.txt にページ・変数・絵・窓・BGM を書く)。学園ＮＴＲ体験版 (2012) で本編と選択肢まで。残り: コンフィグ、履歴、ImageRate、名札と顔の正確な位置 |
| DebonosuWorks | 2 | 自作 (`crates/debonosu`, `crates/debonosu-web`) | Rust | あり (WebGPU) | .pak ("PAK\0"、0x18 に個数・目録の大きさ・格納の大きさ、0x28 から raw deflate の目録: u64 位置・大きさ・格納の大きさ、u32 属性 (0x10 フォルダ、0x20 ファイル)、FILETIME 3 つ、Shift-JIS の名前。中身は大きさが違えば raw deflate)。スクリプトは game.pak の Lua 5.1 バイトコード (.scb、`crates/luavm` で実行、コルーチンを使う)。ラベルごとのコルーチンが ("j"/"e"/"c"/"r", スクリプト, ラベル) を yield し、種類なしの yield は待ち。`_cmdlist.scb` のラッパーが本体の関数 (`_layer`・`_effect`・`_page`・`_wait` など約 460) を呼ぶ: 物語に要る分 (層: 画像・色・画面の取り込み、効果: base/offset/opacity/scale/rotate/shake/wait/release/rewind、clear・rename・priority、文章・名前・ページ、立ち絵、選択肢、BGM (music/bgmNNb.wav)・効果音・声、変数・システムフラグ) と tiItemTitle.dat のタイトル。他は nil/false/0/"" を返す。神楽黎明記 参 体験版でロゴ・注意・タイトル・プロローグ (01.scb、47 ページ) を経てダンジョン (map) の手前まで。セーブは自前 (ページで右クリックか Esc、タイトルの「続きから始める」でロード画面、12 枠)。コルーチンの中身は書き出せないので、枠には始めからの操作 (ページ送り・タイトルのボタン・選択肢) を持ち、ロードは始めからそれを待ちなし・音なしで再生する。残り: Live2D (.moc3)、3D シーンとダンジョン・戦闘 (.x メッシュ、本体の C++)、sepia などの減算合成、トランジション |

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

## ShiinaRio (椎名里緒) の実装メモ (2026-10-28)

- 対象: `b2011_guilty` 種憑け村 序章体験版 (`TANE_T.exe`, ini は `[椎名里緒 v2.48]`, `Arc=T1TANE_T0.WAR`, `Scn=start.SCN`, 800x600)。
- できたこと: `crates/shiina` の `warc.rs` が WARC 1.7 の索引と全エントリ (YH1/YPK/YLZ、Decrypt/Decrypt2、helper4 の SHA-1 系と領域 CRC) を復号する。
  `cargo run -p shiina --bin shiina-dump -- list|extract` で確認。T1TANE_*.WAR はすべて展開でき、S25 画像と .SCN が取り出せた。
  WARC 1.2〜1.6 の Kogado レンジコーダは未実装。
- 鍵: `tools/garbro_db.py` が GARbro の Formats.dat (MS-NRBF) を読み、`warc OUT NAME [HELPER]` で `crates/shiina/schemes/*.bin` を書く。
  体験版の EXE は製品版と helper key の文字列が違う (種憑け村体験版は `gy||OWey|diODD`)。
  helper4 のかからない小さいエントリだけ正しく出るときはこれを疑う。
- 実装 (VM なし): 本編の文章は T1TANE_SC.WAR の A001.TXT..A017.TXT (Shift-JIS の平文) にあり、.SCN の VM はこれを 1 行ずつ読むだけなので、
  エンジン (`crates/shiina/src/engine.rs`, `story.rs`) がシナリオを直接解釈する。`shiina-run DIR FRAMES OUT [frame[:x:y[:r|m]]]`
  (SHIINA_FONT / SHIINA_TRACE / SHIINA_START=a005 でタイトルを飛ばす)、`shiina-dump png <file.s25> <out>` で各フレームと置き場所。
  - シナリオ: `;` は注釈 (`;￥ジャンプ＜a002＞へ` が次のファイル。無いファイルなら体験版の終わりでタイトルへ)、`$命令,引数`、`【名前】` の次の段落がその人の台詞、空行までの連続した行が 1 ページ (最大 3 行)。分岐はない。
  - `$L_BG,bg\x.s25,0` 背景、`$L_CHR,層,bu\x.s25,x,y,0` 層ごとに 1 枚 (空の名前で消す。S25 のフレーム自身の位置に x,y を足す。層は番号順に重ねる。tbg01 は上下の黒帯、tbb01* は回想の片側の黒、tm01b は紋の背景)。
    変更はまとめて `$DRAW,ms,待つか` / `$DRAW_EX,種類,マスク,ms,待つか` でフェード表示 (最後の 1 で終わりを待つ、0 は待たずに文章が出る)。マスクのワイプ (cf\eff_*.s25 はグラデーションのルール画像) はフェードで代用。
  - `$VOICE,v\x.ogv,0`、`$SE,se\x.ogv,ループ,?` (空の名前で止める)、`$MUSIC,m\x.ogv,1` (空で止める)、`$MUSIC_FADE,ms`、`$EX,5` は窓を消してクリック待ち (章の題字の後)。`$EFECT,n` (揺れ・フラッシュ) は未実装。
  - .OGV は "OGV" の見出し (fmt/data) の後に Ogg。名前は書庫の中ではフォルダなし (bg\ などは捨てて全 .WAR から引く)。
  - タイトル: SYSTITLE.S25 のフレーム 0 が絵、250..270 が紋の光 (往復)、ボタンは 3 フレームずつ (通常・ポインタの下・使えない): 100 はじめから、103 つづきから (106 一時読込が同じ場所)、109 設定 (112 おまけが同じ場所。体験版では使えない状態で描く)、115 終了。200..214 はおまけの中身、50 は一枚絵。
  - 窓: SYS_SYS.S25 の 10 が窓、12 が名前枠、20..38 がクリック待ちの絵、50..75 が下のボタン (一時保存 53・一時読込 50・設定 73・早送り 62・自動送り 66・読み返し 70・声再生 59・閉じる 56、+1 がポインタの下)。100.. はシステムの画面 (未使用)。
    文字の位置 (44, 492、22px、行送り 26) と名前 (枠の中央) は絵に合わせた値。一時保存/一時読込は "quick" のスロット、設定・読み返しは自前のメニュー、右クリックでもメニュー (セーブ/ロード 10 スロット・タイトルへ)。
  - S25 の差分行 (フラグの bit31) は GARbro どおり、その行を使うフレームの数だけ足し込みを繰り返す。
- b2017_onomatope_raspberry (ビッチ姉妹が清純なはずがないっ！！ 体験版、`[椎名里緒 v2.50]`、BITCHES2_T*.WAR、本編は TK_*.TXT) と同じ年のビッチ学園の体験版: 索引は製品版の鍵 (GARbro の "Bitch Shimai" / "Bitch Gakuen"、姉妹は追加の CountCrypt つき) で読めるが、中身は helper key が製品版と違うため展開できない (先頭 40 バイトが合わない)。体験版の鍵は exe に平文で無く (実行時に組み立てる。周りに難読化した API 名がある)、`shiina-dump key` (製品版の鍵を 1 バイトで XOR し、末尾 2 バイトを総当たり) でも見つからなかった。exe の鍵の組み立ては追っていない。
- 書庫の鍵の選び方: 索引が読める鍵が複数あるときは、署名の分かるエントリ (S25/OGG/OGV/WAV/PNG と Shift-JIS の TXT) を 3 つまで展開して合う鍵を採る。
- 他の作品: S25 と WARC は共通だが、シナリオの書式・ウィンドウの絵の番号・タイトルのボタンは作品ごとに確かめること。
- .SCN の VM の解析メモ (使っていない。他作品で要るとき用):
  - RE の道具は `tools/shiina/` (pe.py, dispatch.py, sig.py, dis.py)。
  - インタプリタは TANE_T.exe の 0x423f20〜0x42a447。ctx は ebx で、+4 がスクリプト先頭、+0x10 が pc、+0x18 bit8 が終了、+0x20 が命令、+0xff8 が引数表、+0xffc/+0x1000 がスタック。
  - 命令は u16 で、ハンドラは約718個。<=0x6b はバイト表 0x42a4f8 から飛び先表 0x42a450。0x78〜0x580 は飛び先表 0x42a564。0x582〜0x795 はバイト表 0x42bc30 から 0x42b988。0x6f/0x581/0x796 は個別に処理し、それより上は 0xfe39 までさらに分岐する。共通の次命令は 0x42a3d4、エラーは 0x42a410。
  - オペランド: 読み 0x415690、書き 0x415190、アドレス 0x415500、push 0x4150f0、pop 0x415140。
    先頭1バイト t のうち 0x40 はアドレス指定、0x80 は間接 (変数の値をスクリプト先頭からのオフセットとして使う)。
    t&0x7f は 2/3 が c17ae8[u16]、4/5 が即値 u32、6/7 が 0x7fd0b0[u16] (7 はバイト)、8/9 がスタック局所、10/11 が 0x7fc110[u16]、12/13 が ctx+0xff8[u16]、14/15 が c17ae4[u16]、16 が C 文字列、17 が '.' 付き数値テキスト、18/19 が `}` までの名前付き変数 (`[式]` 添字、0x401600 でハッシュ表 0x5e4e58)。奇数は参照先を読む。
  - 値は32ビットで、ポインタは実アドレス。エミュレートには平坦なメモリ空間が要る。
  - START.SCN の末尾には x86/MMX のネイティブルーチンが埋め込まれている (`61 9d c3` = popad/popfd/ret)。スクリプトからこれを呼ぶため、その部分はハンドラごとに Rust で書き直すか x86 を解釈する必要がある。
  - 線形の逆アセンブルは START.SCN のオフセット 0xe (op 0xa の後の `82 d3 82 aa 48 00`) で崩れる。命令ごとのオペランド数の推定は途中。
  - 判明した命令の例: 0xa は待ち時間 (<=1000)、0x49d はグローバル c17c28 への代入、0x7ee は [v v w]、0x7ef は [v v v]。
- 他の ShiinaRio 体験版: `b2017_onomatope_raspberry` (vnbox では NG で画面が出ない)。targets.tsv にはまだ無い。

## Majiro の実装メモ (2026-10-28)

`crates/majiro`。命令表は majiro-py (MIT) の docs と data/mjs (console.mjs の古いソース) が頼り。exe の syscall 表は `cmp edx, hash` の二分探索から引いた。

- 透明度は Majiro の値では「透明さ」(255 で見えない): `$sprite_alfa_set`/`define`、アルファページ (`$get_alfapage`)、`$page_set_antidata` の相手ページ (色の下位バイト = 青が透明さ) がそう。エンジン内部は普通の不透明度で持ち、出入り口で反転する。
- `$grp_copy`/`$grp_extcopy`/`$grp_modcopy` は (src, x, y, w, h, dst, x, y) の順。extcopy の 9 番目は写す側の透明さ。
- 文章: エンジンは `$console_set` のページに文字を描き (`$console_font` の xl は半角の幅、全角は 2 倍)、CONSOLE_WROTE(page, x, y, w, h, 0) で console.mjo に渡す (console.mjo が少しずつ表示ページへ写す)。改ページ待ちは PAUSE のフック (pause_inter) が自分で待つ。`【名前】` で始まる文は NAME_DISP。`$console_color` の上位バイトは影 (1) / 縁取り (2)。
- `$reset_exec` (新しいゲーム) で HOT_RESET(0, "", 1) を送る (console.mjo が窓の色や絵を作り直す)。
- 終わるスレッド (`$thread_end`) も各フレームのデストラクタ (bsel.5) を実行してから消える (遷移の絵がこれで片付く)。
- セーブ (`save.rs`): 文の最初の文字が出るときにスレッド (フレーム・スタック・スレッド変数) を控え、console.mjo の PAUSE が `$save_point(文)` を呼んだ時点のセーブ変数 (スコープ 1) と組にする。`$do_save(n)` が save{n:04}.bin に書き (QSAVE は 9999)、`$do_load(n)` はそのスレッドを戻して他のスレッドを終わらせ、`$exec` のたびに生きていたスプライト (窓・メニュー) 以外を消し、HOT_RESET(1, "", 0) を送る。最初の引数が 0 だと pic.mjo は新しいゲームとして黒から作り直し、1 だとセーブ変数の背景・立ち絵を描き直す (`ld` の flags 0x10 は論理否定)。ロードを選んだクリックは捨てる。
- console.mjo は PAUSE ごとに `$get_font_name$` と覚えた名前を比べ、違うと窓を消して作り直す。`$set_font_name` の名前を返さないとロード直後の文章が消える。
- Closed GAME 体験版の上端バー (y < 55 にマウスで出る): SAVE 211 / LOAD 348 / SKIP 485 / AUTO 613 / HISTORY 1026 / OPTION 1186 / QLOAD 1349 / QSAVE 1514 (幅 138〜172)。押すと area の push ($e9da3d95) が $caceeb7f に番号を入れ、PAUSE の後の switch が 8 で QSAVE、9 で QLOAD (確認の YES/NO 付き) をする。


## Cyberworks / TinkerBell の実装メモ (2026-10-28)

- 対象: グランギニョルの夜 体験版 (b2018_tinkerbell、Tngg.exe、1280x720)。exe は読むだけで、命令の意味は逆アセンブルから (命令の振り分け 0x45c450 / 表 0x45cd80、文の解釈 0x46eb20)。
- アーカイブ: Arc01/02/03/07.dat が索引、Arc04/05/06/08.dat が中身 (05a/05b は項目の part バイトで分かれる)。索引の先頭 16 バイトは 8 桁の 10 進 (各桁 ^0x7f、0xff が 0) が 2 つ。桁の並びは作品で違うので両方試し、圧縮後の大きさがファイル長 - 16 に合う方を採る。項目は u32 長さ + id, 展開後, 位置, 圧縮後 (位置と圧縮後の順も連続性で判定), 種類 2 文字, part。
- 種類: a 脚本, b 画像, e ボタン組, f レイヤー表, j 音声, k 効果音, u BGM, n/o マスク, i/y ムービー (未対応)。参照は種類ごとの 0 始まり番号。
- 画像: 'a'/'d' は先頭が 0xef で GetInt 符号 (V1=0xe9, V3=0xfb) の 20 個の整数 (3 番目が幅、5 番目が高さ…`pic.rs` の `Scheme::tinkerbell_2018`)。この作品は上下反転しない。'd' は直前の 'd' でない画像 (a か PNG) を土台にした差分。'b'/'c' は末尾の PNG (1 バイト目から大きさ BE)。
- 音声: 先頭 4 バイト "Tink" を "OggS" に戻し、[4, 0xe1f) を exe 内の鍵文字列 (GUID + " Copyright 2004 Cyberworks ...") で XOR。鍵は末尾の NUL まで含めて 0..=len、以降は 1..=len を繰り返す。BGM (u) は 12 バイトの前置きがある。
- 脚本: 文 = u32 長さ + トークン。M# 命令番号、M$ レイヤー命令 (exe 内部では 9999)、S 文章 (長さの下位バイトで XOR、0xFE で終わり、ルビは ff ff n 読み m 語)。主な命令は `engine.rs` 冒頭の表。35 は全レイヤーを消して変更の溜め込みを始め、13 (ウィンドウ表示) か効果付きの $ で一度に見せる。
- レイヤー命令の絵番号は 1 始まり (e0 のボタンの絵は 0 始まり)。種類 2 はセピア (回想)。
- 残り: ムービー、$ L 9 / 5 (モーション)、コンフィグ・バックログ・オート・スキップ、Arc00 の題名 (展開すると空)。他の TinkerBell / WendyBell 体験版 (targets.tsv の 7 本) は未確認 (2013/2014 の URL はエラーページ)。
- 新しい世代 (2023〜、ChakraCore.dll 入り。b2023_wendybell 1024x768、b2026_tinkerbell / b2026_wendybell 1920x1080) も同じ `crates/cyberworks` で動く (`Engine::newer`)。exe は読んでいない。
  - Arc01-03 の索引は同じ。音声 (j/k/u) は Arc06 にあり、項目の間に最大 64 バイトの RIFF 見出しが挟まるので、位置と圧縮後の順の判定は隙間を許す。音声の鍵は同じ GUID。
  - 絵は Resources/Data00.dat が索引: u32 ファイル数、ファイルごとに (u32 個数 + 項目 (展開後, 格納, 位置)) / (u32 0) / (u32 組数 + 組 (u32, u32 n, n×8 バイト) の別名表、未使用)。位置は 2023 年が u32、2026 年が u64 で、ファイル末尾にぴったり合う方を採る。中身は Resources/DataNNN.dat。絵の番号は (ファイル+1)×10000000 + 項目番号。格納 != 展開後なら LZSS、展開後 0 は素のまま。
  - 絵の種類: 'b'/'c' は末尾の PNG か JPEG。'a'/'d' の見出しは 種類, u32 id (d と flags 1 の a では土台の番号), u32 0, u32 幅, u32 高さ, u32 flags。flags 7 の 'a' は不透明度の面 + 色の面 (行は 4 バイト境界)。flags 7 の 'd' は 2023 年が ビットマップ + 不透明度 + 3 バイト/画素、2026 年が 不透明度 (u32 21 の個数) + u32 大きさ + ビットマップ + u32 大きさ + 3 バイト/画素 (u32 25+個数 の位置がビットマップの大きさに合えばこちら)。幅と高さの欄は d では別の意味で、大きさは土台から。行は上から。
  - 'f' レイヤー表は k の前に整数が 1 つ多く、'e' ボタン組は 1 項目 20 個 (末尾に 1 つ多い)。文章は 'T' + u32 行番号 + u32 n + UTF-16LE n 文字 (各文字の下位バイトを n で XOR)。命令 28 [メニュー, Var(n), 効果] がタイトルのメニュー (答えは変数 n)、64 がロード画面。画面の大きさは絵 10000000 の大きさ、文字の寸法は幅/1280 倍。
  - ウィンドウのボタン: 1..13 のレイヤー表の絵のうち、同じ大きさが続く最初の 2 つをセーブ・ロードとする (2026 TinkerBell は 1/2、WendyBell は 1 が閉じるボタンで 2/3)。他のボタンは押しても進まない。メニューは右クリック。


## NekoSDK (Waffle, gamedata/*.pak) の実装メモ (2026-10-28)

- 対象: Waffle の体験版 (b2017 隣人に壊されていく俺の妻、b2014 巨乳ファンタジー２if、b2016 巨乳ファンタジー外伝２after、b2012 すくみず食べ放題。b2010 裸の王様。5 本とも本編とセーブ/ロードまで確認)。b2010 の zip の中のセットアップ.EXE は LHa の SFX なので 7z で展開する (.pak が直に出る。verify.sh の fetch は .EXE を開かない)。RAR の SFX なので unar で展開する。exe は読んでいない。命令の意味は脚本の 1 つ目の文字列 (エディタの要約、例 "[画像情報変更EX] スプライト：102 | 移動:1 拡縮:0") から。
- アーカイブ: GARbro (MIT) の NekoSDK/ArcPAK.cs に従う。"NEKOPACK4A" は 10 に索引の大きさ、14 から索引。"NEKOPACK4S" は 10 から。項目は u32 名前の長さ、名前、位置と大きさ (どちらも名前の符号付きバイトの和で XOR)。中身は zlib で、先頭 4 バイトを鍵 (大きさ/8 + 0x22、1 バイトごとに鍵 <<= 3) で XOR、末尾 4 バイトが展開後の大きさ。2017 年は名前に "obj\" のような前置きがあり、2014 年は無い (`Archives::read` は両方探す)。
- 絵: BMP (下の行から) + 同名の .alp (幅×高さの不透明度、上の行から)。.alp の無い待ち記号 (2014) は純緑 (0,255,0) を抜き、MsgWaitMarkPattern 枚を横に並べた絵として MsgWaitMarkSpeed 秒ごとに送る。
- 脚本 (NEKOSDK_ADVSCRIPT2): 見出し + NUL、u32 行数、行ごとに 53 個の i32 + 33 個の文字列 (u32 長さ (NUL 込み) + SJIS)。int 0 が行番号、3 が命令、4.. が引数 n(0)..、36 が次の行 (連結リスト)、37 が分かれ道。命令表は `engine.rs` 冒頭。
- 始まり: init.ini の FirstFlow (flow\flow00_000.txt、命令 1 = 脚本の呼び出し、2 = 終わり)。無ければ cfg\chapter_file.ini (2012、#chapter ごとの最初の脚本)、それも無ければ scr.pak の脚本を名前順 (2014)。
- 絵の位置: すべて絵の中心を画面の中心 + (x, y) に置く (n(0) の意味は不明。拡大した CG を中心からずらす演出がこれで合う)。18/25 の n(16)/n(17) が移動/拡縮の有無、n(4)/n(5) が拡縮 (千分率)、n(15) が不透明度 (/256)、25 の n(24) 遅れ・n(25) ミリ秒。
- 画面: 命令 9 (画面更新) までの変更はためておき、9 でフェード (type 0 は即時。マスクの種類は未対応でフェード)。文章の窓は init.ini の MsgWin*・MsgName*・MsgString*・MsgFontSize・MsgLineInterval・MsgMaxCharNum。2014 年の顔窓 (130/131/138) は窓の左下に描く。
- タイトル: exe の中にあるので、obj\d000_000 (背景) + d002_000 (ロゴ) + d006_000 (170x48 のボタンを 8 列 × 5 段: はじめから 0、つづきから 1、… ゲーム終了 6) で組む。2014 年は anime\d000_000 (半分の大きさ) を背景に、obj\d001_000 をロゴにする。2016 年は obj\d007_000 (140x45 を 10 列 × 5 段: Start 0、Load 1、Quit 9) + d003 (右の絵) + d005 (ロゴ) + d006 (著作権表示)。2012 年は obj\d001_000 (104x104 の丸いボタンを 6 列 × 5 段: はじめから 0、つづきから 1、ゲーム終了 4) を右下に横に並べ、d000_101 (体験版のロゴ) を右上に置く。2010 年 (800x600) は obj\a000_000 (机) に obj\c001_000 (239x55 を 5 列 × 5 段: 始めから 0、続きから 1、ゲーム終了 4)。セーブは 10 スロット (Esc / 右クリックのメニュー)。
- 残り: 選択肢 (未確認)、マスクの切り替え効果、モーションブラー (74/75)、画面揺らし (53/54/115)、オナホコントロール (116、無視)、画像アニメ (16、止まった絵)、回想 (60 は常に本編側)。


## LiLiM (AOS) の実装メモ (2026-10-28)

- 対象: targets.tsv の 7 本 (2006 とらぶるっ！、2007 ナースレンジャー / とらいあんぐるＢＬＵＥ、2009 あい☆きゃん、2011 姫様限定！、2019 WhiteBlue、2021 プリンセス☆シスターズ！。800x600 / 1024x600 / 1280x720)。形式は GARbro (MIT) の `Lilim/ArcAOS.cs`・`ImageABM.cs`・`ArcABM.cs`・`HuffmanCompression.cs` に従う。exe は読んでいない。
- アーカイブ: 版 2 は u32 0、データの始まり、索引の大きさ、名前、0x111 から 0x28 バイトの項目 (名前 0x20、データの始まりからの位置、大きさ)。版 1 (2006〜2007) は 0x20 バイトの項目 (名前 0x10、位置、大きさ) と中身が続き、16 個の 0xff の項目が次の項目までの距離を持つ。.scr と .cmp は u32 の展開後の大きさ + ハフマン (1 ビット目が 1 なら節、0 なら 8 ビットの葉。上位ビットから)。
- 画像: 素の BMP (24 ビット、8 ビットのマスク .msk) は下の行から。ABM は BMP の見出しのビット数の欄が種類 (32/24: ランレングス、32 は画素ごとの不透明度つき。1/2: 複数枚。8/-8: パレット) で、詰めた行は上から。.cmp は展開すると BMP か ABM。ボタンの絵は縦 3 段 (通常・ポインタの下・押下)。
- 脚本: start.scr から。`::START`..`::END`、`:ラベル`、`^gsb(名前)` は同名の .scr を呼ぶ (無ければラベル)、`^go` は移る、`^jp` はラベルへ。`^if/^elif/^else/^endi`、`^sw/^case/^df/^ends` (case は次の case で抜ける)、`^macro..^endm` は飛ばす。`[名前]「...」` が台詞、それ以外の行は地の文。どちらも 1 行で 1 クリック。
- 2006〜2009 年の版では音が bgm/・se/ のファイル (アーカイブの外)。文字の選択肢は `/選択肢` の行を続け、最後の行が `//` で終わる (答えは 0 からの番号で %reg)。
- 画面: bgon/cgon が背景、chon(n, 絵, x [, y] | NON | CENTER) が番号つきの絵 (ch(n, PNT, 1) の番号は x が中心)。ch(n, SYS, 1) の絵はメッセージ窓の一部 (窓と一緒に出る。顔窓など)、SYS 2 はボタンの上。fnt の 1 つ目が文字の大きさ、wnd(0, TXT, x, y, 行数, -, 行間, 1 行の字数)。名前は文字の左に場所があれば左 (WhiteBlue)、なければ 1 行上 (窓の名札の位置)。変更は ud で一度に見せる (0 即時、1 フェード、2 マスク = 今はフェード)。mono(1) でセピア。btnset/btnon/btnenable がゲームのボタン (押した番号が %reg)。float(2) をロード画面にした (タイトルの LOAD)。
- 音: cvon (cv.aos)、seon、bgmon/bgmoff、envon/envoff (環境音のループ)。すべて素の Ogg。
- 残り: マスクのワイプ、ef (揺れなど)、バックログ (wbl)、コンフィグ、オート、クイックセーブ (今はセーブ画面を開く)。efex (絵の動き) は飛ばしている。

## Silky's (AI6WIN 世代) の実装メモ (2026-10-28)

- 対象: targets.tsv の 2 本 (b2016_silkys_sakura ママのおっぱい、b2014_silkys_plus なないろリンカネーション、どちらも 1280x720)。アーカイブと AKB は GARbro (MIT) の `Silky/ArcAi6Win.cs`・`ImageAKB.cs`、命令の並びは VNTextPatch (MIT) の `SilkysPlusDisassembler.cs` に従う。システムコールの意味は oppai.exe を Ghidra で読んだ (CFunc* の vtable と各 switch)。
- .arc: 先頭 u32 が索引の大きさ。項目は名前の長さ n、名前 (i 文字目 + (n - i))、BE の詰めた大きさ・大きさ・位置。大きさが違えば LZSS (4K 窓、0xfee、フラグは下位ビットから、1 が素のバイト)。
- AKB: "AKB " / "AKB+" (土台の名前 0x20 バイトつきの差分)、幅・高さ、フラグ (bit30 = 24 ビット、bit31 = アルファ)、背景色、内側の矩形。行は下から LZSS、左と上の差分を足す。
- .mes: u32 メッセージ数、u32 特別数、その数の u32 (メッセージの印の位置)。オペランドは BE。0a は詰めた文字 (SJIS の先頭バイト以外は b - 0x7d62)。定数は Int(10)+Int(n) の足し算で書かれる。システムコールは番号、サブ番号の順に取り、引数は後に積んだものが先 (ライブラリ関数の引数も同じで args[0] が最後に積んだもの)。システムコールは自分の文で積んだものをすべて取る (エンジンが読まない引数も捨て、結果だけ残す)。
- 番号: 1 選択肢 (0 で後に続く `Int(n) Choice(next) 文字 Yield` を集め、2 で待つ。答えは g3[18] に番号 + 1)、2 窓、14 フラグ、15/16/17/18 音、19 スプライト (0 作成、1 読み込み、3 転写、16..28 フェード)、20 画面の色 (時間はフレーム数)、21 スクリプト (0 移る、2 ライブラリ)、25 設定、26 待ち、27 セーブ (4 で g4 の先頭を場面の記録として (種類, 番号) に覚え、2 で読み戻す。ロード時は lib 45 がこれで背景・立ち絵・CG を戻し、27/10 で保存したメッセージの印へ)、29 ゲーム (13 立ち絵、14 BG (4..9 で BG を画面へ、14 で読み込み)、15 名前、18 タイトル、23 タイトル絵のスクロール、3 終了)。
- タイトル: 29/18 の返り値はボタン番号 + 1。ママのおっぱいは trial_Title_p の 400x42 の 3 列 (表で位置)、なないろは title.akb にボタンの絵が TITLE_T.AREA の順に詰めてある (幅に入る最初の段に置く、段の高さは 4 状態分)。2 つ目と 3 つ目のボタン (前回の続き・続きから) はエンジンのロード。
- 窓: ゲームごとの Skin (MESSAGE_P.AKB / MESWAKU.AKB) で、背景のアルファを設定の色で塗り、MES.AREA の各ボタンの絵の位置と働き (SAVE/LOAD/Q.SAVE/Q.LOAD/Skip) を表に持つ。ハートは全角 ＠。
- セーブ: 自前の形式 (SILKYSAV1: スクリプト名、メッセージ番号、g1..g5、場面の記録)。ロードは g3[55] の 0x1000 を立ててスクリプトを頭から動かす (ゲーム自身の復元処理)。
- 確認: `SILKYS_FONT=ipag.ttf silkys-run <game> <frames> <out> [frame:x:y ...]` (SILKYS_LOG=1 でシステムコール)。
- 残り: バックログ・コンフィグ・オート、Q.SAVE は 1 番に保存、ムービー (movie.arc)、スプライトの効果のうちフェード以外、立ち絵の位置は 1280 幅の式 (他の解像度は未確認)。

## Ivory (ags.exe) の実装メモ (2026-10-28)

- 対象: targets.tsv の 2 本 (b2016_evoll お嬢様はイイナリみたいです！？、b2020_caramel_box_strawberry はじカノ、どちらも 1280x720)。アーカイブと SG は GARbro (MIT) の `Ivory/ArcPK.cs`・`ImageSG.cs` に従う。命令の意味は exe を読まずスクリプトの使われ方から決めた (Ghidra は 2G でメモリ不足で落ちた)。
- .pk: "fPK " (u32) / "fPK2" (u64)、全体の大きさ、節 (id、大きさ、ヘッダーの大きさ)。cLST (数・鍵、中身を復号: 名前の位置・位置・大きさ)、cNAM (鍵、名前を復号)、cDAT (基準位置)。復号は 32 語ごとの鍵で 2 ビット組の入れ替え + xor。名前は 2016 版が `SYSTEM/FRAME_BASE.SG` のようなフォルダつき、2020 版はフォルダなし (Packs はどちらでも引ける)。
- SG: "fSG " + cRGB (方式 0 素、1 チャンネルごとの連長、2 パレット (32 ビットは 2+2/10+4+8 ビットの列)、3 画素の連長) か cJPG (pk と同じ復号の JPEG)。どちらも x, y を持ち、その位置に置く (立ち絵も全画面座標)。
- PX: "fPX " + cTRK (ヘッダー 0x24) の後ろが素の Ogg Vorbis。
- OCB: "fAGS" + cTEX (文字列)、cFNM (ファイル名・スクリプト名、`misc/bg.pk:white` のように pk つきもある)、cCOD (28 バイトのヘッダー、u16 命令・u16 長さ・u32 引数)。飛び先はコードの位置か 0xff000000|cFNM の位置 (スクリプト名)。文中の 0xff 's' / 0xff 'n' は改行。
- 命令: 01 地の文、02 名前・文・ボイス、03/04 BGM、05/1d 効果音、16/17 チャンネルのループ音、06 背景 (他のレイヤーと窓も消す。2 つ目の引数 1 で即時)、15 背景だけ、8e レイヤーの絵 (2..6 が立ち絵、0x12 が上に重ねる絵、-1 で消す)、95/96 レイヤーを横/縦に動かす (下位 16 ビットがレイヤー、上位 16 ビットが符号つきの位置、秒)、98 フェード待ち、94 移動待ち、10/1c 秒の待ち、14 変数 (4 文字の u32 名、5 で代入、値は float)、0d 変数の比較で飛ぶ (0xa は ≠)、0a 飛ぶ、0b 呼ぶ、0c 戻る、09 選択肢 (文と飛び先の組)、43 タイトル、25/35 窓を隠す、37 窓を閉じる。52 (揺れ?)、93、36、1f、49 は無視。
- フェード時間: FLG\xf2 / FLG\xf3 (出・入の秒) をスクリプトが変更の前後に設定する。0 は既定 (ここでは 0.4 秒)。
- タイトル・窓: TITLE_BUTTON_NORMAL / FRAME_BUTTON_NORMAL の不透明な列 (行) の並びをボタンとみなす (2020 版の窓は文字だけなので 15 px 未満の隙間はつなぐ)。ボタンの働きはゲームごとの Skin の表。セーブは自前の形式 (IVORYSAV1: 表示中のメッセージのスクリプトと位置、呼び出し履歴、変数、レイヤー、BGM、ループ音) で、ロードはその命令からやり直す。
- 確認: `IVORY_FONT=ipag.ttf ivory-run <game> <frames> <out> [frame:x:y ...]` (IVORY_LOG=1 で命令)、`ivory-dump <game> list | extract | extract-all | png`。
- 残り: バックログ・コンフィグ・オート・ボイス再生ボタン、52 の画面効果、タイトルのおまけ類、FLGd (ロゴを飛ばす印) などシステム変数の保存。

## Interheart の 2018 年以降のタイトル (2026-10-28)

- 2018 年以降の titlechip.kg は部品の並びが変わり、titlem*.hmp・chip.def・file.def は古いテンプレートのまま (使われていない)。ボタンは exe の .data の表にある。
- 表の見つけ方 (`crates/interheart/src/exe.rs`): 単独の文字列 "Title\0" を push するコード → 直後の call (グループを開く) → その次の `mov r32, 表` (b8..bf) → ループの `cmp r32, 終わり` (81 f8+r)。1 項目は i32 × 33: id、表示先の矩形 (x, y, w, h)、0、部品番号 3 つ、titlechip 上の元矩形 3 つ (通常 / 光る / 押した)、あとは 0。
- id: 1 はじめから、2 ロード画面、3 最後のセーブから、4 コンフィグ、5 おまけ。最後の id が終了 (2018 の hachimitsu は 6、2019 以降は 7 で 6 は初期化)。エンジンは 1..3 をそのまま、6 以上の最大を終了、他は何もしないボタンとして描く。
- exe は uninst/setup/syscfg/getpin 以外を順に試して id 1 がある表を使う。`cargo run -p interheart --example fpkls -- --title <exe>` で表を出せる。hachimitsu 2018・marshmallow 2019/2020・shiba 2020・dessert 2021/2026 で確認。

## Interheart の 2018 年以降のメッセージウィンドウと選択肢 (2026-10-28)

- MesChip.kg の部品の位置も exe にある (`exe::message_window`、`exe::choices`)。`fpkls -- --window <exe>` で出せる。
- ウィンドウの部品は .data に並んだ矩形 (x0, y0, x1, y1): ウィンドウ (0, 0, w, h)、+0x10 その右の続き (画面が 1280 でシートが 1024 のとき。無ければ 0,0,0,0)、+0x20/+0x30 別物、+0x40 名前枠。描く所は `push 1; push 部品; lea eax, [ebp+d]; push eax; call 描画` の直前でグローバル変数から作る (`mov r, [g]` / `add` / `sub` / `mov [ebp+d], r`)。exe.rs はこの数命令だけを追う小さなレジスタ追跡で表示先を得る。
- 文字と名前の位置はウィンドウの状態を初期化する関数の即値: `mov [r+s], 5` の後の `[s+0x20]` = ウィンドウの y、`[s+0x2c]/[s+0x30]` = 本文の x/y、`[s+0x34]/[s+0x38]` = 名前の中央 x/上端 y (どれもウィンドウ左上から)。
- 選択肢は "SELECT" グループの表 (`imul eax, eax, 0x84; add eax, 表`): 選択肢の数だけ上から使う。id 19.. の連番、元矩形は通常 / 光る。後ろに別モード用の同じ id の並びが続く。
- 実測: hachimitsu 2018 (ノットフィクション) ウィンドウ (0,563) 1024x157 + 右の続き、名前枠 (170,533)、本文 (231,600)。shiba 2020 (0,554) 1024x166、名前枠 (185,518)、本文 (247,571)。marshmallow 2020 (あまママ) (132,557) 1017x147 (続きなし)、名前枠 (191,516)、本文 (247,571)、選択肢 (241,66+65n) 798x53。
- 2018 の MesChip は旧来の判定 (`classic_window_chip`) にも通ってしまうので、exe からウィンドウが取れたらそちらを先に使う。
- ウィンドウ上のボタンは "Message" 表 (`buttons(exe, "Message")`、呼び出しの後に `xor edi, edi` が入る)。id はどの年も共通 (knot の exe のボタン処理の switch で確認): 1 ログ、2 オート、3 スキップ、4 ウィンドウを消す、5 Q-SAVE、6 Q-LOAD、7 ツールチップ、8 設定、9 ボイス再生、13 セーブ画面、14 ロード画面、15 ボタン固定、16 次の未読へ、17/18 前の選択肢へ・ツイート (年で違う)。1〜6・9・13・14 を実装 (Q-SAVE は vnrun-saves/quick.txt、ログはホイール上でも開く。オート/スキップ中はボタンを光らせ、クリックで止める)、他は描くだけ。

## 未決のこと

- Rust 側の共通 trait の具体的な形 (描画は WebGL2 か wgpu か、音声の取り回し)。最初の自作エンジンで決める。
- エンジンの wasm をどこで配るか。今は `./build.sh` で手元ビルド。GitHub Actions でビルドして Releases に置く形が候補。
- vnrun のサーバーの形 (ゲームの取り込み、一覧、セーブの保存先とバックアップ)。
- ゲームのファイルの大きいものを丸ごと読む問題 (上記)。
- 描画は WebGPU だけ、という決まりに対する残り: AliceSoft System4 (xsystem4) と Artemis (art3m1s-core) は GLES を見えない canvas の WebGL2 で描き、読み戻した画素を Presenter (WebGPU) で出している。つなぎの間の措置で、GLES の描画を WebGPU に移す (または CPU で描く) のが残りの作業。

## Kaguya の実装メモ (2026-10-28)

- 対象はアトリエかぐやの古い世代 (exe + `scr/bg_/sp_/cg_/cgw/bgm/wav/vo1/vo2/tblstr` の `.arc` と `.ari`)。新しい世代 (Start.exe + `bgd/cgd/spd/scr.arc` の LINK アーカイブ、params.dat) は別物で `kaguya::link` (下の「LINK 世代」)。`kaguya::Game` が params.dat と message.dat の有無で振り分ける。devserver は `scr.arc` と `tblstr.arc` (または params.dat と message.dat) があれば kaguya にする。
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

### LINK 世代 (ハラミタマ 2015 / LoveHolic 2018、Start.exe + params.dat)

- `crates/kaguya/src/link/`: `arc.rs` (LINK6 アーカイブ、BMR 展開、鍵の XOR、AP-2/AP-0/BMP)、`data.rs` (params.dat、message.dat、[SCR-Ver5.1])、`engine.rs` (VM と描画、命令表は冒頭のコメント)。アーカイブ・BMR・暗号は GARbro ArcLINK.cs (MIT) に沿う。
- LINK6: データは 8 + byte[7] から、エントリ = u32 サイズ (0 で終わり)、u16 フラグ、7 バイト、u16 名前長 + UTF-16 名。フラグ &3 で "BMR" なら BMR (Huffman → move-to-front → BWT の逆 → 段差付き RLE)、&4 は画素を params.dat の鍵で XOR (AP-2/AP-3 は 0x18 から、BM は 0x36、AP は 0xC、鍵の先頭から循環)。スクリプトは資源を "X.spd" と呼び、spd.arc では "X.alp" なので拡張子を除いた名前で引く。
- 画像: AP-2 = "AP-2", i32 x, i32 y, u32 幅, u32 高さ, u32 0x18, BGRA 下から上 (x, y が画面上の位置)。AP-0 (ルール画像) = "AP-0", u32 幅, u32 高さ, 1 バイトの灰色 (下から上、明るい所から変わる)。背景は 24 ビット BMP (多くは BMR)。
- params.dat: 0x13 に幅・高さ (1024x600)、0x1b から (u8 長さの塊、題名、空文字列、メーカー、1 バイト、主人公の名 "有貴"、姓 "賀茂")。鍵は u32 (幅×高さ×4) の直後 (ファイル中の最初の一致)。その後ろに資源名表 (u32 個数、[u16 長さ, UTF-16, u8]、0 番は空) と絵の表 (u32 個数、[u8 n, n×u32 資源番号]: 胴体・空・表情などの重ね、0 は無し) が続く。資源名表は「個数の直後が空の名前で、全部が拡張子付き」の最初の位置で見つける (途中から始まる偽の一致を避けるため UTF-16 は厳密に読む)。v05.7 (2018) では u8 が 1 の項目がグループ (u32 n + n 個の名前、"地下室点灯甲" = ["ＢＧ洋館地下室点灯_モ.bgd"] のような差分) で、最初の名前を使う。
- message.dat: 各文字列は全バイト反転。0x15 から話者名 (u16 長さ + cp932、主人公は外字 F040 = U+E000 で params.dat の名に置き換える。2015 は 0 番、2018 は 2 番)、選択肢、文 (u32 サイズ + [i32 長さ, 本文, 0x01 + UTF-16 の音声名 "sei0038.voice" か 0x00])、そのあと話者表 (u32 個数、[i32 話者 (-1 無し), u8 n, n×u32 文番号])。
- スクリプト: u16 op + u16 長さ (自身を含む)、ジャンプ先はファイル内の位置。後ろの [LAYER] が場面 (状態) の表で、各場面は層 (u32 絵番号、u8 枠 (0 背景、1.. 人物、2/3 霧などの効果)、i16 x, i16 y、23 バイト版は 12 バイト追加、未解明) の並び。06 が場面の切り替え (0 即時、1 フェード、4 ルール画像)、07 (文) も自分の場面・BGM・ループ効果音を持つので、違えば即時に合わせる。
- 流れ: スタート → 初期設定 → (0c で var[3] の) プロローグ → 1a で A0000..A0070 → ゲームスタート → ルート処理 (B0000 の後、14 01 02 のマップで var[0x0c] = 0 星花 1 ルル 2 由夏 3 倫寧 → 各マップルート①) → 体験版の終わり。マップ画面 (vrm.arc の MAPキャラ選択_*) は未実装で、資源名の「マップルート①」から作った文字の選択肢で代用。14 01 0e (人間/妖怪) も文字の選択肢 (1/2)。08 の選択肢は message.dat の選択肢の先頭から n 個 (体験版では 1 か所だけ、初回は通らない)。
- 2018 で増えたもの: 0e (変数が 0 でなければ飛ぶ)、01 の kinds 0x22 (b も変数)、01 の op 7 (剰余と推測)、08 の形 (u8, u8 n, n×i32 選択肢番号, -1, ..)。マップの行き先は資源名の "C0000　名前　…" (C 以降の 0000) から作る。14 の 1/0x12/0x13/0x18/0x19 は不明で何もしない。
- 比較 0d の op 2..5 の向き (<, <=, >, >=) とループ効果音 09 02 / 単発 09 05 は推測。05 の待ちはフレーム数として扱った (0x80 付きは待たない)。16/18/19/1b は何もしない。
- 窓は vrm.arc の メッセ枠 (0,412)、名前 (56,454)、本文 (96,484) は目で合わせた位置。タイトルは タイトル画面 + タイトル画面{はじめから,…}オフ/ターゲット (位置は画像のもの)。つづきから・CG・回想・音楽は何もしない。
- 確認: `KAGUYA_AT=60,215 kaguya-run G ipag.ttf N [出力先]` (はじめからの位置)。KAGUYA_PICK=1 などでマップの行き先。

## NonColor (ACV1 / DAT) の実装メモ (2026-10-28)

- 対象: targets.tsv の 2 本 (b2018_mirai 宿星のガールフレンド２、b2015_minato_carnival 姉小路直子と銀色の死神、どちらも 960x540)。アーカイブの形は GARbro (MIT) の `NonColor/ArcDAT.cs`・`ArcACV.cs` に従う。Launcher.exe・Checking.exe/.dat (改ざん検査) は使わない。
- 索引: エントリ 21 バイト (u64 ハッシュ、u8 flags ^ (u8)hash、offset/size/unpacked ^ hash の下位 32 ビット、ACV1 の offset はさらに ^ 0x8B6A4E5F)。unpacked は実サイズではないので使わない。ハッシュは cp932 の名前の CRC-64/WE (多項式 0x42F0E1EBA9EA3693、非反転、初期値・最終 xor ~0)。flag 2 が無いものは offset ^= name[len/2]、size ^= name[len/4]。
- flag 2: u32 語を (hash 下位 ^ 鍵) で xor してから zlib。鍵は exe の中なので、小さい 3 件で zlib ヘッダーから下位 16 ビット、上位 16 ビットは総当たり (0.1 秒)。mirai 02d222b4、minato f3973c45。script.dat はすべて flag 2 なので名前なしで読める。
- 名前: bg/X.tlg・ev/X.tlg・cg/X.tlg、bgm/X.ogg・se/X.ogg、vo/<話者番号>/<ボイス>.ogg、sys/title/*・sys/window/*。立ち絵 `S001_1AA2AA_010_M` は 2018 では顔 `st/001M/2_010` (ポーズ 4 文字目 + 表情) を体 `st/001M/S001_1AA2AA_000_M` に重ねる (位置は顔の TLG0 tags のキー 0x02 = u16 x, y)。2015 は `st/001M/<コード>` 1 枚。
- スクリプト: 全ファイルでラベル共有、`*start` から。DAY ファイルの defsub (BG/EV/ST/ST2/CG/SE0/BGM/SELECT ...) をエンジン側で直接解釈し、エフェクト (SC.*・ST.EM・揺れ・フラッシュ・ズームのアニメーション) は省く。BG/EV は time に `@1` が無ければ立ち絵と CG を消す。立ち絵は下端 = StY + y + StH/2 (定数は flag_format の `int` 行から)、ST2 の x は `x<n>` 表、既存の立ち絵は ST2 の移動指定が 0 なら位置据え置き。同じ位置に別の立ち絵が出たら前のを消す (ST.DD)。
- タイトルのボタン位置・窓・名前・文字サイズは pos_setting の値を 2 つの Skin (2018 / 2015、sys/title/tutorial の有無で判定) に写した。TL でタイトル、ED で終了。
- 確認: `NONCOLOR_FONT=ipag.ttf noncolor-run <game> <frames> <out> [frame:x:y ...]` (NONCOLOR_LOG=1 で行ごとのログ)、`noncolor-dump <game> scripts <dir> | extract <name> <out> | exists <name>... | png <name.tlg> <out>`。
- 残り: 立ち絵の目パチ (_E)・エモート、SC.* の画面効果、BG.XY/CG.XY などの移動アニメーション (最終位置へ即移動)、MV のムービー (飛ばす)、バックログ・コンフィグ。

## DxLib MED (筆柿そふと / trois、md_*.med) の実装メモ (2026-10-28)

- 対象: b2018_fudegaki_soft (催眠学園１年生、1280x720)、b2012_fudegaki_soft (つぼい君のスイッチ！、1024x768)、b2012_trois (モンスターパーク２、1280x720、md_gra2.med と md_jpg.med もある)。3 本とも同じエンジン。
- アーカイブ: md_scr / md_gra / md_bgm (と gra2 / jpg) は MDN ("MD" + 版の文字、u16 項目長、u16 件数、0x10 から [名前 (項目長-8)、u32 大きさ、u32 位置])。GARbro の ArcMED.cs の FudegakiEncryption は、この 3 本の脚本には掛かっていない (平文)。md_cv / md_se は DxLib の DXA v4 で、ファイル全体を 12 バイトの鍵で XOR (位置 % 12)。鍵は見出しが "DX" 4 0、+8 が 0x1C、索引がファイルの最後まで、という前提から求める (ArcDX.cs の GuessKey)。索引の名前は索引の先頭からの位置 (u16 で 4 バイト単位の長さ、大文字の名前、元の名前)。中身は .ogg。
- 絵: md_gra は PRS ("YB")、md_jpg は JPEG。BGM は WADY (M01..)。どちらも `tmrhiro::marble` を使う。画面の大きさは BG_BLACK の大きさ。
- 脚本: u32 大きさ、u32 コードの長さ a、u32 (ラベル数 << 16 | 文字列数)、u32。0x10 から a-1 バイトの記録 [u16 行、u8 n、op、引数 n-1 バイト]、その後 u16 × ラベル数 (コード内の位置)、NUL 区切りの文字列、ラベル名。データ用の脚本 (_BGSET、_SPRSET、_MAP、_SYSTEM、...) はコードが無く、文字列が元の行。命令表は `engine.rs` 冒頭 (2018 の exe のコンパイラの switch から取り、2012 の 2 本も同じ番号。JMP だけ 2012 は u8 フラグが無い)。
- 式: 変数は 2 バイト (種類 00 / c3 / cf / d7 + 番号)、eb u8・ec i16・ed i32 の数、ee.. が + - * / & | % = <> > >= < <= && || ( ) (exe の字句表の順)。条件の "==" は = が 2 つ。d7 45 / 47 は乱数 (ブランドコールの選択)。
- 制御: 29 IF の次が 8 (フラグ 1) ならブロック (偽で 8 の行き先、それが 30 ELSE なら ELSE と 8 の後へ)、無ければ次の 1 記録だけ。30 ELSE に上から来たら次の 8 の行き先へ。33 SELECT / 34 CLICKMAP / 42 SWITCH は [n、場所 × n、終わり] の後に 8 (ブロックの出口)、各場合 (10) に上から来たら出口へ。28 GO は同じ脚本のラベルへ飛ぶ (呼び出しではない)、27 CALL / 108 CALL2 は "脚本" か "脚本:ラベル" を呼び、31 で戻る。
- タイトル: 脚本 (STITLE / TITLE) が自分で描く。34 CLICKMAP の _MAP (":名前"、"GRA:通常,選択時,3"、"0,x0,y0,x1,y1" の矩形、または "x,y,絵,選択時の絵") で、ポインタの下の矩形に選択時の絵の同じ部分を重ねる。51 LOAD はロード画面、50 EXITGAME は終了、54 CONFIG は無視。
- 背景 16: "#名前" は _BGSET (#名前 の次の行が絵) で引く。絵の無い名前 (SGGG、S000) は黒。0x80 付きはフェード (0.5 秒)、背景を変えると立ち絵と 56 の重ね絵は消える。立ち絵 18 (mode、数、"#名前" × 数): _SPRSET の PREFIX + BODY + FACE の名前を連結した名前で、絵は PREFIX の絵 + BODY / FACE の後置 (FACE は体の上の x, y、2012 は 0, 0)。並びは画面幅を数 + 1 等分した位置に下揃え。
- 文章: 1 で行に足し、0 で改行、4 でクリック待ち (その後に文章と名前を消す)、13 が名前。窓は SYS_TW の絵 (画面幅なら下端、狭ければ下に少し空けて中央)、上の透明な部分を除いた所に文字。待ち記号は SYS_CLICKW (正方形のコマの横並び)。選択肢は SYS_SEL / SYS_SEL2。
- セーブ: 文章の待ちで右クリック / Esc のメニューから 10 スロット。脚本の場所 (スタック)、変数、背景、立ち絵、重ね絵、BGM、ループ SE、名前と文章を保存。
- 確認: `MED_FONT=ipag.ttf MED_LOG=1 med-run <game> <frames> <out> [frame:x:y ...]`、`med-dump list|x|png <game> ...`。
- 残り: 切り替え効果の種類 (mode の下位ビット、すべてフェード)、56 の連番 (EFFECT の MC_IN など、最後のコマだけ見える)、86 MFADE、22 SELOOP の音量、35 SHAKE、バックログ・オート・コンフィグ。

## Liar-soft (XFL / WCG / GSC) の実装メモ (2026-10-28)

- 対象: b2022_evoll (南国プリズン、1280x720)、b2019_channel_ubai (アイドル裏営業、1280x720)。同じエンジン。scr.xfl・grpe/grpo/grpo_bu0/bu1/cu/ef/grps.xfl・bgm/voice/wav.xfl が並ぶ。
- XFL: "LB" 1 0、u32 索引の長さ、u32 件数、12 から 40 バイト [名前 32、u32 位置 (12 + 索引の長さから)、u32 大きさ]。voice.xfl の中に 2.xfl などの入れ子があり、`2.xfl/0001.wav` として引く。
- WCG: "WG" フラグ 0x20 ..、+8 幅、+12 高さ、16 から 2 面 (R と A の語、次に B と G の語)。各面は [u32 2*画素数、u32 データ長、u16 索引数、u16]、u16 の索引、MSB 先頭のビット列 (3/4 ビットの長さ 0 なら 4 ビットの連続数 + 2、索引番号は長さ別の可変長)。A は反転して保存 (+7 が 0 の絵も反転)。LWG ("LG" 1 0、高さ、幅、件数、?, 索引長、24 から [x、y、bpp、位置、大きさ、名前長、名前]) は窓 (tbox00)・待ち記号 (wait00)・選択肢 (sel_a00) の部品。
- 音: bgm (Track%02d.wav) と wav.xfl (効果音 %04d.wav) は fmt 0x6771 の WAV に Ogg がそのまま入っている ("OggS" から切り出してブラウザで再生)。voice.xfl は PCM の WAV、番号 n*10000+k は n.xfl/%04d.wav。
- GSC: i32 [ファイル長、見出し長 0x24、コード長、文字列表の長さ、文字列の長さ、..]。コードは u16 命令 + 引数 (i32 / i16)。命令ごとの引数の形は全脚本を余り無く読めるように決めた (`gsc.rs` の表)。i32 の引数は下位 16 ビットが値、上位 16 ビットが変数として引く回数 (0x11E75 = 変数 7797)。上位バイト付きの命令は計算 (dst, a, b を一時変数 dst へ、ビット 10-11 / 8-9 が a / b の種類 (数・一時変数・変数)、上位 4 ビットが演算: 1 代入、2 ||、3 &&、4 ==、5 >=、6 >、7 <=、8 <、9 !=、A..E + - * / %)。0xF1xx は一時変数への代入。03 / 04 は一時変数 0 で分岐、05 は無条件。場所はコード先頭からのバイト位置。
- 呼び出し: 0C は別の脚本へ移る (スタックは積まない)、0F は脚本を引数 10 個付きで呼ぶ (変数 10..19)、C8 は同じ脚本内の場所を引数付きで呼ぶ、10 v で戻る (変数 0 = v)、08 で終わり。立ち絵 (体 + 顔) は脚本自身のサブルーチン (C8) が下位の命令で組み立てるので、下位の命令だけ正しければよい。
- 絵: 層 0..99 (0 が 14 IMAGE_GET の背景、grpe)。1E [層、絵、x、y、フェード、?]、24 [層、フェード] で消す (0 は 1..99 全部、100 超はグループ)、25 で表示 / 非表示、26 [層、cx、cy、clamp] で x, y を絵の中心にする (立ち絵)、28 でグループ、79 [層、文字列] で層の絵のアーカイブ (無ければ全 grp*.xfl から探す)。1A で画面を止め、1B ですぐ、1C [種類、時間、時間] でフェードして出す (時間の単位は 25 ms とした、マスクの種類はすべてクロスフェード)。0D n は n * 100 ms。
- タイトル: 0001.gsc が 1E でボタンの絵、4B で選択時の絵、46 [層、値] (値は変数 0 へ) / 47 [層、種類] (ゲーム自身の機能: 2 ロード、3 設定、0 終了) で登録し、49 で待つ。47 の 2 は自前のロード画面、3 と 0 は押せない。
- 文章: 51 [?, ボイス、?, ?, 名前、文章、?]、52 は追記、`^n` は改行。窓は tbox00.lwg の back を 5A の位置 (y 503)、文字は 5B の矩形 (160, 34, 幅 980) に 60 の大きさ (32) で。選択肢 0E [数、問い、場所 × 5、文字列 × 5、..] は sel_a00 の body を並べる。
- セーブ: 文章の待ちで右クリック / Esc のメニューから 10 スロット。スタック、変数 (0 でないもの)、一時変数、全層、BGM、ループの効果音、名前と文章。
- 確認: `LIAR_FONT=ipag.ttf LIAR_LOG=1 liar-run <game> <frames> <out> [frame:x:y ...]`、`liar-dump list|png|gsc <game> ...`。tools/shot.js の at: はページ座標 (1280x800、キャンバスは y +20 に 1:1)。
- 残り: 17 (揺れ?)、16 / 22 (動き)、2D / 2E、マスク遷移 (grps の efNN.msk は BMP)、mov/*.mpg、設定・バックログ・オート、47 の設定ボタン、4F 以降の窓の細かい設定 (名前の位置など)。

## AZ System (ARC / CPB / ASB) の実装メモ (2026-10-28)

- 対象: targets.tsv の b2006_alcot (Triptych 体験版、800x600、LZH の自己展開 exe を 7z で展開)。形は GARbro (MIT) の `AZSys/ArcAZSys.cs`・`ImageCPB.cs` に従い、違うところは実データに合わせた。
- ARC: "ARC\x1a"、u32@8 件数、u32@12 索引の長さ、0x30 から索引。索引は u32 crc・制御長・参照長・リテラル長・展開長の後に LZ (制御ビット 0x80 から、1 なら u16 参照 (長さ v>>13 + 3、距離 (v&0x1fff)+1)、0 ならバイト数 + 1 のリテラル)。展開すると 0x40 バイトのエントリ (offset (+0x30+索引長)、size、0x10 から名前)。
- ASB: "ASB\x1a"、u32 packed、u32 unpacked、12 から u32 語を鍵で引く (端数バイトはそのまま)。先頭の語が残りの CRC32、その後 zlib。鍵 = A ^ unpacked、k ^= ((k<<12)|k)<<11 (A はゲームの鍵、Triptych は 0xf389842d)。鍵は zlib ヘッダー (78 da など) から下位 16 ビット、上位 16 ビットは途中まで inflate して CRC32 で確かめる総当たり (ネイティブ 0.1 秒)。一度見つけたら A に戻して次からはすぐ。
- 記録: u32 op、u32 長さ (8 バイト込み)、引数。2a (文章: `\n` は改行、`[字:ルビ]`)・29 (名前)・04 (章題)・75 (SE)・79 (ボイス) は数 4 個 + cp932 文字列、他は数だけ。4d cg y (背景、cg.tbl の番号、y だけ上に)・3b 3 chara (立ち絵、中央下寄せ)・3d (立ち絵を消す) は 38 でまとめてフェード (最後の数がフレーム数)。3e/40 は立ち絵を即出し/消し、4f は縦長の背景を下から上へパン、16 ms 待ち、6e track_NN.ogg、74 BGM 停止、78 SE 停止、36 n で窓の型 (window.tbl の n-1 行: 色・位置・桁数・行数・文字サイズ・行送り)、68 窓を消す、06 で終わり (タイトルへ)。
- 表: "TBL\x1a" (+4 の長さは crc から、+8 展開長、+12 crc、+16 zlib、48 バイトのセル)。"TBL2" は行数・列数の後に各行が "TBL\x1a" 付きで並ぶ。chara.tbl の行 (id-1) は [, 体, , 目, x, y, , , , , 口, x, y, ...]、空欄は前の行を引き継ぐ。目と口は 3 コマ縦並びで、1 コマ目を体に重ねる。
- CPB: "CPB\x1a"、+5 ビット数、+8 幅、+14 高さ、+16 から 4 本の長さ、32 から各チャンネル (crc + zlib) が A, B, G, R の順。
- タイトルのボタン位置は exe から読めなかったので右下に縦に並べた (Prologue / Digest1 / Digest2 / H scene / DataLoad / End)。窓は半透明の黒い箱、SAVE / LOAD / EXIT は adv_* の絵を右下に。
- 確認: `AZSYS_FONT=ipag.ttf AZSYS_LOG=1 azsys-run <game> <frames> <out> [frame:x:y ...]`、`azsys-dump <game> script tr_01.asb | extract <name> <out> | png <name.cpb> <out.png>`。
- 残り: .map のマスク遷移 (クロスフェードで代用)、.tmc の口パク、4b の色、バックログ・オート・コンフィグ。

## PJADV (GAMEDAT PAC / EPA / scenario.dat) の実装メモ (2026-10-28)

- 対象: targets.tsv の b2006_unisonshift (ななついろ★ドロップス 体験版、800x600、bootUP!/Pajamas の PJADV)。アーカイブと EPA は GARbro (MIT) の `Pajamas/ArcGameDat.cs`・`ImageEPA.cs` どおり。命令の意味は nana_trial.exe の命令表 (0x405050 の比較木) を capstone で読み、実際の使われ方に合わせた。
- GAMEDAT PAC: "GAMEDAT PAC2" (名前 32 バイト、"PACK" 版は 16)、u32@12 件数、名前、(offset, size) の組。offset は索引の後から。game_sys.dat (scenario.dat・textdata.dat・filename.dat・graphic.def・BGM.ini・システム絵)、graph.dat (EPA)、bgm/se/voice.dat (ogg)。名前は大小文字を区別しない。
- EPA: "EP"、+3 モード (2 なら +16 に x, y のずれ、データは 0x18 から)、+4 色 (0 パレット、1 BGR、2 BGRA、4 パレット + α)、+8 幅、+12 高さ。チャンネルごと (B, G, R, A の順に平面) の LZ: 上位 4 ビットが 0 ならリテラル、他は 16 通りの距離表 [0, 1, w, w+1, 2, w-1, 2w, 3, 2(w+1), w+2, 2w+1, 2w-1, 2(w-1), w-2, 3w, 4] からコピー (長さ flag&7、flag&8 なら次のバイトと合わせて 11 ビット)。
- textdata.dat ("PJADV_TF0001") は cp932 の文字列の並び (オフセットで引く)。textdata.bin が 95 6B 3C 9D 63 で始まるときは鍵 0xC5 から 1 バイトごとに +0x5C の xor。filename.dat ("PJADV_FL0001") は u32@12 件数、0x10 から 32 バイトの名前。スクリプトの絵・音の引数はこの表の 1 始まりの番号。名前は graphic.def の節 (src・xpos/ypos・filepos (x,y,w,h)・xcenter/ycenter) か <名前>.epa、音は <名前>.ogg、BGM は BGM.ini の節 (ogg_intro_src → ogg_loop_src)。
- scenario.dat ("PJADV_SF0001"、0x20 から): u32 語。命令の先頭語は上位 24 ビットが命令、下位 8 ビットが語数 (0 の語は 1 語飛ばす)。引数の上位 4 ビット: 0 即値 (28 ビット符号付き)、1 システム変数、2 グローバル、3 一時、4 直前の結果。飛び先はファイルのバイト位置。
  - 0200xx dst src (mov/add/sub/mul/div/mod/and/or)、0201xx 比較 (結果は dst)。010002 jmp、010003 call、010004 ret、010007 v at (v が 0 なら飛ぶ)、010005 ms 待ち、010013 jmp、010000 終わり、01000d 章の名前。
  - 010200 g ボタン群、010201 g id x0 y0 x1 y1 有効 絵 処理 (クリックで処理を call、処理は g96=1, g399=id を入れて ret)、010202 g 1 で有効、スクリプトは g96 を `800100 0` (1 フレーム) で回して待つ。800204 g は窓のボタン群 (Q.SAVE, Q.LOAD, SAVE, LOAD, SYS, AUTO, SKIP, LOG, HIDE)。
  - 800101 背景、800102 _ 絵 位置 (2/3/4 = 中心 x 200/400/600、下揃え: exe 0x420230 の表)。800100 効果 ms で見せる (効果 0 は即時)。800100 の後の最初の 800101/800102 で立ち絵は空から。800003 声 名前 文章 番号 フラグ (0x20000 でクリック待ち、0 は待たない)、800202 窓を消す。
  - 040000 層 絵 (層 0 なら新しい番号を返す: 顔 FA_* と HANA はこれで sys9c/sys9d に入る)、040001 消す、040003 位置、040005 α、040007 拡大 (%)。040100〜040101 ms でアニメをまとめて待つ。040102 tr 層 dx dy (ms) は相対移動、040103 は相対の from→to、040105 α、040106 は拡大の増減 (EV001A は 400% から -290、-10 で 100%)。同じ tr のアニメは順番に。040107/040109/04010b (回転らしきもの)・040200 (揺れ) は省略。
  - 050000 bgm loop、050001 ms 停止、050005 フェードイン、050100 se ch、050101 ch 停止。
- 体験版の起動直後 (0x20) からロゴ・注意書き・タイトル (0x434) までスクリプトどおり。ロード画面 (0x3f774) と設定 (0x38828) の呼び出しは自前のスロット画面に置き換え。
- 確認: `PJADV_FONT=ipag.ttf PJADV_LOG=1 pjadv-run <game> <frames> <out> [frame:x:y[:r|m|s] ...]` (s はスキップ)、`pjadv-dump <game> list | extract <name> <out> | png <name.epa> <out.png>`。
- 残り: .anm/.bnm のアニメ (タイトルの雲など)、回転・揺れ、ワイプ (クロスフェードで代用)、色型 10 の EPA、バックログ・オート・Q.SAVE。

## CrossOver (Adv.exe: PACK / GR2 / *Pola* SCD) の実装メモ (2026-10-28)

- 対象: b2009_crossover 体験版 (CrossOver 自社の Adv.exe + LL5.dll)。Adv.exe の解析だけで書いた (ファイル位置 + 0x400c00 = VA)。
- PACK: "PACK", 個数, データ開始, 0、24 バイトの項目 (名前[16], 大きさ, 絶対位置)。スクリプトからは 1 始まりの番号 (0x407580 が 1 引く)、声は名前で引く。
- GR2: "LL5\0" + 連長 (符号付き c: c >= 0 は次の 1 バイトを c 回、c < 0 は -c バイトをそのまま)。中身は "GR2_", w, h, 色キー (-1 なし), 深さ (3 BGR24、2 15bit)、行は上から 4 バイト境界。マスクは別の絵 (明るさ = α)。立ち絵は CHR.GR2 (who%4==1) か CHR2.GR2 の pose*2+1 と マスク pose*2+2、全部 800x600。SSAM はサムネイル 160x120。
- *Pola*: 0x14 からのビット列の LZ (`crates/crossover/src/pola.rs`、exe の展開ルーチンを直訳)。
- VIC: RIFF WAV (fmt 0x674f) の中の Ogg ("OggS" から)。
- SCD: "SCD_", コード開始, その後ろにラベル ([u32 位置][名前\0])。ラベルと飛び先はコード開始からの相対。
- VM (主ループ 0x417180): 00 文章 (文字列\0、続く 00 で次の文字列)、01 代入、02 積む、03 文の終わり、04 分岐 (k=0 無条件、1 真で、2 真なら 1 を積んで、3 偽で、4 偽なら 0 を積んで)、05 比較 (== != < <= > >=、符号なし)、06 演算 (+ - * / % & |、7 符号反転)。被演算子の型 1 u8、2 u16、3 変数、4 変数名の変数、5/7 番号、6 変数の値の番号、8 文字列。07 以上は表 0x42c190 の命令。全命令の意味は `engine.rs` の先頭の表にある。
- 命令は 0 を積む (1A〜1F は積まない、22 は戻り値、0x47 はラベルへ飛ぶ)。呼び出しの引数は変数 6000〜 (最初に降ろしたものが 6000)。1F は SYSTEM.SCD の呼び出し。
- 面: 10000〜10003 → 0〜3 (10002 作業画面、10003 PARTS = SYS の絵 2/3 とマスク)、10004 は一番上の窓の面、他は 0x22+n。
- 窓: 0x38 定義 (積む順 kind, pady, padx, h, w, y, x, ?, 色, id)、0x3b 開く・0x3c 閉じる (窓の積み)。ホットスポットの組は窓 id。0x44 矩形 (x1=p7, y1=p6, x2=p5, y2=p0)、0x45 on/off/sel の呼び先。メニューの呼び先は 0x4d(0) まで入れ子で走らせる。0x43 の選択肢は窓の文章の "!0" 行。時間の単位は 10 ms。
- 主人公の名前: ＠＠＠＠＠ → 彰、＃＃＃＃＃ → 市ノ瀬 (既定のまま)。
- メッセージ窓: PARTS の 0〜179 行 (α のグラデーション) を y=421 に、文章は窓 1 (0,478) 幅 800 余白 35、24px・行送り 32、名前は文章の 36px 上。
- セーブ: modes[1]==1 の文章ごとに状態を記録、自前の 10 スロット (Esc/右クリックのメニュー: セーブ・ロード・タイトル・戻る)。ゲーム自身のロード用に v10 背景、v11〜13 立ち絵、v14〜16 ポーズ、v17 イベントを書く。タイトルへ戻るときは 5000 以上の変数を残し v46=0 で SYSMENU の #st_menu へ。0x4d(0) で呼び先が無いとき・0x1c で無いスクリプト (ASA_01) はタイトルへ。
- 確認: `CROSSOVER_FONT=<ttf> CROSSOVER_LOG=1 crossover-run <game> <frames> <out> [frame:x:y[:m|r|s]]` (s はスキップの切替)。ネイティブで OP_01〜04、選択肢 2 つ、終わりからタイトルまで、セーブ→タイトル→ロード。Chromium でタイトル → 本編 → Esc メニューでセーブ → タイトルの「続きから」でロード。Chromium の座標はページ (133 + 1.2667x, 1.2667y)。
- まだ: 0x42 の選択肢の配置、メッセージ窓のボタン (組 1)、0x9d のオプション/CG 画面、マスクの変化 (mode 3/4、MASK.GR2)、BGM のフェードイン (mode 2/3)、0x2b、色キー、バックログ。

## Propeller (MPK / MGR / MSC) の実装メモ (2026-10-28)

- 対象: b2007_propeller (Bullet Butlers Web 体験版 ver1.1、BB.exe)。GARbro (MIT) の ArcMPK・ImageMGR を参考に、スクリプトは BB.exe の解析だけで書いた (ファイル位置 = RVA、イメージベース 0x400000、圧縮なし)。dl_VJ01002202 (同じ作品の DLsite 体験版) は PlayDRM の作品なので vnrun では扱わない (対象外)。
- MPK: u32 索引位置, u32 個数。索引は 0x28 バイトの項目 (名前[0x20] 先頭の '\' は落とす, 位置, 大きさ) で、索引の 0x1f バイト目 (= 鍵、BB は 0x58) と XOR。.msc は先頭が 0x88 なら全体を 0x88 と XOR。音声は素の Ogg (.ogv という名前)。
- MGR: i16 コマ数 (>1 なら u32 位置の表)、コマごとに u32 展開後, u32 圧縮後, LZ。LZ: c < 0x20 は c+1 バイトをそのまま、それ以外は長さ (c>>5、7 なら次のバイトを足す) + 2、距離 ((c&0x1f)<<8) + 次のバイト + 1。中身は BMP (32bit は α あり、24bit は不透明)。スクリプトのコマは 1 始まり (0 は 1)。
- MSC: u16 0, u32 コード開始, u32 ラベル表の大きさ, ラベル ([種類 u8][id u16][位置 u32] の 7 バイト), u32 大域ラベル表の大きさ, 大域ラベル, コード。位置はコード開始から。大域ラベルは読み込んだときにエンジン全体の表に登録され、スレッド (0.20) の入口になる。
- 命令 = 分類 (0〜6) + 命令バイト + 被演算子 (B u8、W i16、S u16 長さ + SJIS、V 種類 u8 + i16: 0 数、1 大域変数、2 システム変数、3 乱数 % n、4 基準スプライト (1.02))。全命令の表は `crates/propeller/src/engine.rs` の先頭。0 制御 (0.07 のモード切替: 0 オープニング、1 タイトル、2 本編 = スプライトを消して 01game を最後まで、3 ダイアログ、5 セーブ、6 ロード)、1 設定、2 スプライト、3 画面切替 (始めの画面を写して重ねる)、4 イベント (0 離れた、1 乗った、2 押した = 持ち主のスレッドを飛ばす、3 ロック中)、5 文章 (【表示名/本名】、_r 改行、<本文,ルビ>)、6 音。
- 起動: "system" → System.mpk を登録、System\system.msc を最後まで (アーカイブを登録して 0.07 0 でオープニングへ)、モード 3 のスクリプト (06dialog) を 1 回、モード 7 (マクロ) を読み込み。
- 体験版の選択肢は片方のボタンの 3 つのイベントが同じラベル (例 `4.00 170 0 1 / 170 1 1 / 170 2 1`) で、押しても何も起きない。これは体験版でその分岐を塞いだもので、エンジンの誤りではない。
- セーブ: 本編スレッドの 5.00 ごとに状態 (スプライト、変数、子スレッド、BGM/SE、位置) を記録、自前の 10 スロット (Esc/右クリックのメニュー、ゲームの SAVE/LOAD ボタンも同じ画面)。ロードはモード 2 に入り直して (01game を走らせて) から戻す。
- 確認: `PROPELLER_FONT=<ttf> PROPELLER_LOG=1 propeller-run <game> <frames> <out> [frame[:x:y[:m|r|s]]]`。すきま桜はネイティブで序章 → sa_01〜sa_10 → タイトル、体験版2 の鈴ルート、セーブ→タイトル→ロード、Chromium で序章 (Ctrl でスキップ) → タイトル → 本編 → Esc メニューでセーブ → タイトルの「続きから」でロード (座標はページ (1.25x, 40 + 1.25y))。BB はネイティブで co001〜co007、選択肢 3 回、終わりからタイトルまで、セーブ→タイトル→ロード。Chromium でロゴ → タイトル → 本編 → Esc メニューでセーブ → タイトルの LOAD でロード。Chromium の座標はページ (106.7 + 1.3333x, 1.3333y)。
- 2011 世代 (s2011_propeller_sukima、すきま桜とうその都会 体験版２、sukima_trial2.exe): 同じ命令体系でフィールドが広い。ラベルは [種類 u8][id u32][位置 u32] の 9 バイト、W は i32、S は u32 長さ、V は 種類 u8 + i32 (種類 7/8 はポインタの x/y、9 以上はコンフィグ画面の設定値など)。0.20 に 1 バイト増え、0.33〜0.37・1.18〜1.1f・2.17・5.05〜5.07・6.0a〜6.0e が増えた (被演算子は exe の各分類の処理 0x4597e0/0x4580b0/0x4500d0/0x451680/0x44fd20/0x455c60/0x451b10 から)。どちらの形式かは system.msc がどちらで最後まで読めるかで決め、画面は exe の定数 (BB 800x600、2011 は 1024x576)。開始は Scenario.mpk の Scenario\system.msc (BB は System.mpk)。logo.mpk・sukimaop.mpk は MPEG-PS の動画で飛ばす。初回は序章 (jyo.msc) を流してからタイトル、タイトルの「体験版2」は att.msc の選択画面から各ルートの抜粋へ。
- 2.03 の矩形は「絵のどの部分を出すか」で、スプライトの位置にその部分を置き、拡大はその部分の中心まわり (BB co007 の上下分割画面、すきま桜のカットイン窓で確認)。
- まだ: ルール画像の画面切替 (クロスフェードで代用)、2.15 の波、2.06 の曲線移動、3.02 の画面効果、2.14/2.16、色調 (2.0f) は足し合わせで近似、文章の行送り (大きさ + 7 の推定)、メッセージ窓の透明度、バックログ、コンフィグ画面。

## 未着手の 2 本の調べ (2026-10-28)

- b2020_rikuzendo (ティーリオ傭兵団の初仕事 体験版): data.dts ("SDTS")・runtime.rts ("SRTS")・environment.evs・game.ini は **SRPG Studio**。exe は IScript* の COM 風インターフェース (200 以上) と CryptDeriveKey/CryptDecrypt (data.dts の資源ごとの暗号) を持ち、ゲームの進行は SRPG Studio 本体の JScript (戦闘・マップ・会話イベント) が組み立てる。本編に届くにはこのホスト API 一式を作り直すことになるので見送り。
- b2023_fg_remake (夜勤病棟 リメイク 体験版): 下の「Eomaia の実装メモ」で本編まで動くようになった。

## Eomaia (FG REMAKE: .arc / SCRE / systeminfo.json) の実装メモ (2026-10-28)

- 対象: targets.tsv の b2023_fg_remake (夜勤病棟 リメイク 体験版、1920x1080)。エンジン名 Eomaia は exe の pdb から。DRM・認証には触れていない (体験版にもない)。
- .arc (script / image / data / music / voice / effect): u32 LE 索引の長さ、各項目は u8 名前長・名前・BE size・BE size・BE offset (ファイル先頭から)。名前の i 文字目は c - (名前長 + 0x20 - i) で、小文字以外はさらに + 0x20。戻すときは足して、a..z でなければ 0x20 引く (大文字は使われていない)。名前に "image/" などのフォルダが付くので、最後の部分で大小文字なしに引く。
- 絵は普通の PNG、.json は TexturePacker 形式のシート (frames の名前 `<シート>_<n>`)、.char は立ち絵 (size・base の fileid と位置・sub の顔パーツ (x, y, シート)・sub_map)、.style / .pck は UI のレイアウト (JSON)。
- SCRE: "SCRE"、版、数、+12 にブロック表の位置 (0x14)。コードは表の後から、飛び先はすべてコード先頭からの BE u32。
  - 0x32 BE i32、0x33 UTF-8 文字列、0x14 本文 (メッセージに足す)、0x1a 0 改行、0x15 jz、0x16 jmp、0x18 a b 行 (a は行番号)、0x19 選択肢 (番号を pop、次の 0x00 までがその文字列、続きは飛び先)、0x01 ret、0x00 終わり。
  - 変数: 0x0a/0x0b/0x0c/0x0d は (k, i) を pop して S (グローバル)・B・L (スクリプト / ライブラリごと)・A (引数) を積む。0x0e/0x0f/0x10 は (値, i) で S・B・L に入れ、0x11 は参照で渡された引数 R[i] に書く。
  - 演算 (exe 0x475250 の表で確認): 0x34 + (文字列なら連結)、0x35 -、0x36 *、0x37 /、0x38 %、0x39 乱数 (1 個 pop)、0x3a &&、0x3b ||、0x3c &、0x3d |、0x3e <、0x3f >、0x40 <=、0x41 >=、0x42 ==、0x43 !=。
  - 0x17 組み込み (exe 0x42bfd0、クラス番号 0〜52・100・200 の switch): クラス、その中の番号の順に pop。11.2 / 12.2 は「引数… 番号 'ライブラリ名'」でライブラリ / パッケージの関数 (定義は `flag nargs 番号 '名前' 1 11|12 call` + 終わりの u32、引数は最後に積んだのが A[0])。12.3 はパッケージのイベント、10.0 / 10.1 は別のスクリプトへ / を呼ぶ、0.2 (flag, on)・0.3 フラグ、0.13 戻り値、2.0 (n, 書式, 引数) printf、2.3 / 2.4 split / atoi、17.0 (2, 先頭, 数) は L の変数の参照を積む (先頭が一番上)、22.0 選択肢の始まり、23.7 名前、23.2〜23.4 / 2.1 はメッセージの区切りと既読番号。100.x は Eomaia 固有の絵・立ち絵・フェード・音、28.x はセーブ用の退避、38.x は .style のレイアウト、15.x は設定。
- 方針: 絵と音のライブラリ (eomaia_image / eomaia_char / eomaia_fade / cross_anime / audio / eomaia_movie) とパッケージ (meswnd / select / keywait / title_try / map) は名前と番号で受けて自前で描く。日・時間・場所の計算をする map / mapfindcharevent0〜5 / statuswnd / library / util はバイトコードのまま VM で動かす (中の 38.x などの UI 呼び出しは何もしないで 0 を返す)。initialize.script / setting.script (ライブラリの読み込みと設定) は飛ばす。
  - eomaia_image #103 (file) 背景 (重ね絵は消える)、#106 (file, n) / #107 (file, x, y, n) 重ね絵、#109 すぐ見せる、#110 (ms) クロスフェード、#117 (x, y, 回数) 揺れ。eomaia_char #102 (_, file.char, _, _, x, _, slot)、#103 (_, 'face', n, slot) 顔 (シートの `<fileid>_<n>`)、#105 (slot, ms) 見せる、#106 (_, slot, ms) / #107 (_, ms) 消す。cross_anime #102 はクロスフェードを待つ。eomaia_fade #102 (1 なら色 ARGB へ / 0 なら戻す, 色, ms)、#101 は真っ暗 (色) かどうか。audio #105 (file, loop) BGM、#101 (ms) 止める、#118 SE、#130 ボイス。-1 ms は 500 ms。
  - 窓は meswnd.style どおり (meswnd_base.png を y 716、名前 (374, 776)、本文 (374, 822) 44px)。選択肢は select_select のシート、タイトルは title_try.style (はじめから・前回の続きから / 続きから (自前のロード)・終了)、マップは map.style (map_base、場所 map_0..10、アイコン map_tk / map_ev の 文字 * 3 + 点灯、仕事をする)。マップの答えは "場所:枠" (枠 0/1 はキャラ、2 は誰もいない場所)、"work" で一日の巡回を終える。キャラの居場所は map.lib の L[場所 * 8 + 枠] (キャラ) と L[場所 * 8 + 2 + 枠] (1 イベント / 2 会話)。
  - セーブは自前 (右クリック / Esc、12 枠、JSON): 呼び出しの積み (スクリプト・位置・L の置き場・引数)・スタック・S/B/L・フラグ・見えている背景と立ち絵・フェード色・BGM・メッセージ。メッセージ待ちでだけセーブできる。
- 確認: `EOMAIA_FONT=ipag.ttf EOMAIA_LOG=1 eomaia-run <game> <frames> <out> [frame:x:y[:r|m|s|o] ...]` (o は選択肢かマップが出ているときだけクリック)。体験版はプロローグ→ryu101→nar301→ryu102→マップ (1 日目)→ryu103・104→マップ (2 日目)→ryu105〜107→tr_end.png→タイトルで、410 メッセージ。
- 残り: マップ右上の週・曜日・残り時間の数字、ステータス窓 (panel)、バックログ・オート・スキップの UI、ムービー (a_*.dat、ASF/WMV) と OP、アイキャッチの動き、h シーン回想 (title の sub)。

## CMVS (Purple software, *.cpz / PB3 / PS2) と Musica の調べ (2026-10-28)

s2011_purplesoftware_miranosu (未来ノスタルジア web 体験版 v2、cmvs32.exe、1024x768) は形式の調べまで。エンジンは未着手で、status.tsv の行もまだ足していない。試作の Python (tools/cmvs/cpz.py で一覧・展開、tools/cmvs/dis.py で PS2 の逆アセンブル) は残してある。

- CPZ5 の展開は GARbro ArcCPZ.cs (MIT) の流れどおりで読めた (9 つの .cpz すべて、ogg は OggS、PS2A は展開できる)。ただし鍵の組は GARbro に無い。
  - cmvs32.exe には CPZ5 の読み手のクラスが 4 つあり、スクリプトの命令 (0x4512f0 の手前、値は大域 0x55ae2c) がどれを使うかを決める (3 は CPZ3、5 / 6 / その他)。このタイトルは 4 つ目 (vtable 0x525f30、索引の読み手 0x44a030 付近)。
  - 4 つ目の MD5 変種: 初期値 0x64FE9A8C, 0xC2F93EA8, 0xEF81BA5D, 0x92C8A72F、結果は [s1 ^ 0x49876325, s2 + 0x54F46D7C, s3 ^ 0xAC7958B7, s0 + 0x1C0638BD] (変換本体 0x447270 は普通の MD5)。
  - 秘密の文字列は exe の 3 本目 (VA 0x525f48、「解析する悪い子はリコがお仕置きしちゃいます。メッ！かわいいとか言っても許してあげないんだからぁ！」の SJIS 96 バイト = 24 語)。GARbro の既定 (〜呪っちゃいました♪) は 2 本目で、前半 11 語だけ同じ。
  - そのほかの定数 (ヘッダの XOR、0x3795B39A、表の係数 0x1A743125、0x3A / 0x7E、ディレクトリ・エントリの種、エントリ鍵 0x2547A39E / 0xBC / 0x5C29E87B、PB3 の頭の復号) は GARbro の CPZ5 と同じ。ヘッダ 0x38 の語は 0x37ACF831 で XOR されるが、このクラスでは使われていない。
  - exe の .rdata は VA = ファイル位置 + 0x401600 (.text は + 0x400C00)。
- 中身: script.cpz に 18 本の .ps2 (start.ps2 は pack 直下に生で置かれる)、bg / event / chip / stand / balloon は PB3B。種類は 1 (event・chip)、2 と 3 (bg・chip、JBP1 系)、5 と 6 (stand・balloon、6 は 5 の基底画像に差分を重ねる)。一部の背景は data/imgvideo/*.cmv (22 本の動く背景)。se / voice は ogg、BGM は data/music/*.ogg。
- PS2 スクリプト (展開後): 0x30 のヘッダ、ラベル表 (u32 × [0x10])、コード ([0x14] バイト)、データ ([0x18])、文字列 ([0x1c])。[0x20] はシナリオ本体の入口で、その前は共通ライブラリ (コンパイル済みの C 風の関数群、版が 2 つ: 入口 0x2e352 と 0x2f402)。命令は u16 の語で、fetch は 0x44d480。
  - 0x200 / 0x201 / 0x202 は後置記法の式 (整数 / 文字列 / 浮動小数、0x20f で終わる)。結果は reg (0x13c80) と flag bit 0。字句: 0x100 即値、0x101 G[式] (大域 int)、0x102 F[式] (大域ビット)、0x103 引数、0x104/0x106/0x107 データ域、0x105/0x109/0x110/0x112 は配列 (+4 * 式)、0x108/0x10a/0x10b は局所、0x10e ラベル、0x10f システム値、0x120 文字列表、0x121 大域文字列 (128 × 1024)、0x12a 浮動小数の即値、0x12b 大域 float。演算子 0x160〜0x172 (* / % + - & | ^ << >> > >= < <= && || = == !=)、0x173/0x174 (++ / --)、0x175〜0x17e (+= など)。入れ子の式は字句のあとに 0x200 の語を 1 つ挟む。
  - 制御: 0x400 jmp、0x401 jz、0x402 jnz、0x403 case、0x405 ラベルへ jmp、0x410 ラベルを call、0x411 ret (n バイト捨てる)、0x412 pop、0x414 別スクリプトからの戻り、0x430 reg を積む、0x440 即値を積む、0x442 float を積む。
  - 0x2000〜0x27ff は命令 (下 11 ビット、0x461b20 の表 0x462ee0、0x360 個)。戻り値の 0x4000 は pc + 2 して下位バイト分の引数を捨てる、0x8000 はこのフレームで止まる (0 番は 0xc000 で 1 フレーム待つ)、0x2000 は同じ命令をもう一度。引数の数は戻り値の下位バイト / 4 で、ほぼすべての命令で積んだ数と合う。
  - 流れ: start.ps2 (pack 直下) が 0x1f (値 8) で CPZ の読み手を選び、0x81 で intproc.ps2 / intcode.ps2 を別スロットに読み、0x80 で main.ps2 へ。シナリオは snky00b → snky01〜12 を 0x80 でつなぐ。本文はライブラリの関数 (例 snky01 の L66 = (本文, ?, 名前, ?, ?, ?, ボイス番号)) に渡すだけで、窓・選択肢・セーブ画面などの UI は intproc.ps2 (558 KB) がスクリプトで組む。
- 見積り (2026-10-28): 1 日では終わらない。忠実に VM と命令を作る道は、走る命令が 150〜200 種 (スクリプト全体では 358 種) あり、意味はそれぞれ exe を読むことになるので 5〜7 日。ライブラリ関数を横取りする道 (シナリオから呼ばれる 60 前後の関数 × ライブラリ 2 版を自前で実装し、タイトル・窓・セーブは自前の UI) は、VM 0.3 日、PB3 (GARbro の ImagePB3.cs の移植、種類 1 / 2 / 3 / 5 / 6) 0.5 日、関数 1〜1.5 日、UI 0.5 日、web とブラウザ確認 0.5 日で 3〜4 日。CMV は最初の 1 枚で代用する前提。
- r1_musica (Musica .paz): 配布元 (minori.ph) は日本以外からだと英語のページへ飛ばされ、archive.org にも写しが無く (web.archive.org は接続が切られる)、体験版を手に入れられなかった。未着手。
