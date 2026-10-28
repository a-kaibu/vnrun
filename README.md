# vnrun — ノベルゲームのエンジンをブラウザで動かす

Windows 向けノベルゲームを、Wine を使わずにブラウザの中で動かすためのプレイヤー。ゲームのデータはvnrun 自身のサーバーから HTTP で読み、エンジンは WebAssembly でブラウザ側で動く。サーバーはファイルとセーブを配るだけなので、非力なホストでも重くならない。

## 対応エンジン

| エンジン | 実装 | 状態 |
|---|---|---|
| SiglusEngine / RealLive / AVG32 / UK2 | [siglus_rs](https://github.com/xmoezzz/siglus_rs) (`third_party/siglus_rs`) | 組み込み済み。Siglus 2 本・RealLive 2 本の体験版で確認 ([docs/handoff.md](docs/handoff.md)) |
| Softpal (PAL) | [sena-rs](https://github.com/xmoezzz/sena-rs) (`third_party/sena-rs`、MPL-2.0) | 組み込み済み。ただし sena-rs は「恋×シンアイ彼女」向けで、試したエレクトリップの体験版は注意書き画面の後で止まる |
| BGI / Ethornell | [ethornell-rs](https://github.com/xmoezzz/ethornell-rs) (`third_party/ethornell-rs`、MPL-2.0) | 組み込み済み。2013 年以降の BGI 1.654 系も修正済み (patches/ethornell-rs/0003)。結果は [docs/verification.md](docs/verification.md) |
| YU-RIS | 自作 (`crates/yuris`、ブラウザ用の出口は `crates/yuris-web`) | 組み込み済み。レイヤーは WebGPU で合成。「彼女が異種族だった場合」体験版で本編・立ち絵まで確認 |
| FVP | [rfvp](https://github.com/xmoezzz/rfvp) (`third_party/rfvp`) | 組み込み済み。さくら、もゆ。体験版で本編の文章とセーブまで確認 |
| Artemis | [art3m1s-core](https://github.com/Alphaly2K/art3m1s-core) (`third_party/art3m1s-core`、MPL-2.0) | 組み込み中。描画はまだ WebGL2 経由、試した体験版は E-Mote の読み込みで止まる |
| NScripter | [ONScripter-Yuri](https://github.com/YuriSizuku/OnscripterYuri) (`third_party/onscripter-yuri`、GPL-2.0) | 組み込み済み。自作のテストスクリプトで文章・選択肢・セーブまで確認 (実物の作品は未確認) |
| AliceSoft System4 | [xsystem4](https://github.com/nunuhara/xsystem4) (`third_party/xsystem4`、GPL-2.0-or-later) | 組み込み済み。ランス01・母娘乱館の体験版で本編の文章まで確認 (音とセーブは未確認) |
| 吉里吉里 (KAG) | [KiriKiri SDL2](https://github.com/krkrsdl2/krkrsdl2) (`third_party/krkrsdl2`、MIT ほか) | 組み込み済み。KAG3 のテストで文章・選択肢・セーブまで確認。Akabei / Mado と検証済み cxdec の XP3 を自動復号。Pad の設定編集と保存にも対応。体験版の結果は [docs/verification.md](docs/verification.md) |
| TyranoScript、RPGツクール MV/MZ など (NW.js) | ゲーム自身 (iframe で開くだけ) | 組み込み済み。TyranoScript の公式サンプルでタイトル・本編・セーブ・ロードまで確認 (RPGツクールは未確認、package.nw に固めた作品は未対応) |

今後の候補は [docs/handoff.md](docs/handoff.md) の対応表を参照。

SDRS で包まれた EXE とデータは、ゲームフォルダの指定時に自動検出して展開する。PlayDRM など、それ以外の DRM の付いた製品版は vnbox の Wine で、正規の認証を通して遊ぶ。

## 構成

```
web/                 プレイヤーのページ (Vite+ + TypeScript、エンジンに依らない部分)
  src/main.ts        ?game=<URL> のゲームを開き、エンジンに渡す
  src/gamefs.ts      ゲームのファイルとセーブを HTTP で読み書きする
  src/present.ts     CPU で描いたフレームを WebGPU で画面に出す
  src/engines/<id>.ts  エンジンごとのつなぎ
engines/<id>/build.sh  エンジンを wasm にして web/public/engines/<id>/ に置く
third_party/         取り込んだエンジン・前処理ライブラリ (git submodule)
crates/devserver     手元で試すためのサーバー
crates/prep          ブラウザでできない下ごしらえ (SDRS の展開、cxdec のパラメーター抽出、Siglus の二次鍵の復元)
patches/<id>/        取り込んだエンジンへの修正
tools/shot.js        ヘッドレス Chromium で起動・操作・スクショ
docs/protocol.md     プレイヤーとサーバーの約束事
```

## ビルドと試し方

Rust (rustup) と Node.js が要る。Artemis、NScripter、吉里吉里には Emscripten (emcc) も要る。Emscripten が GitHub の archive からの取得を拒まれる環境では、先に `tools/emscripten-ports.sh` で依存ライブラリを git から用意する。描画はすべて WebGPU で、WebGPU の無いブラウザには対応しない。

```bash
git submodule update --init
./build.sh                                   # → dist/ (エンジンを作り、web/ を vp build)
cargo run --release -p vnrun-devserver -- --game ~/games/シオンの血族_体験版/StartData/GameData
# → http://127.0.0.1:8090/ をブラウザで開く。セーブは ./saves/<フォルダ名>/ に残る
```

`--game` はエンジンのファイルがある場所 (Siglus なら `Scene.pck` と `Gameexe.dat` のあるフォルダ) を指す。エンジンは自動で判定し、外れたら `--engine siglus|reallive|avg32|uk2|bgi|softpal|fvp|artemis|nscripter|kirikiri|yuris|nexas|tmrhiro|livemaker|catsystem2|entis|system4|qlie|advhd|html` で指定する。別の端末から開くなら `--addr 0.0.0.0:8090`。

`vnrun-devserver --game` は、配信を始める前に SDRS の展開、EXE 内蔵 cxdec のパラメーター抽出、Siglus の二次鍵の前処理を自動で行う。前処理が失敗した場合はエラーで止まり、配信しない。

複数の作品を一覧から開く場合は、作品ごとのフォルダをまとめたディレクトリを指定する。

```sh
cargo run --release -p vnrun-devserver -- --library ~/games
```

一覧の更新時に追加された作品を検出し、同じ前処理を行う。作品ごとのセーブは `saves/<フォルダ名>/` に分けて保存される。前処理やエンジンの検出に失敗した作品は一覧にエラーが表示される。

SiglusEngine の作品には暗号の二次鍵が要るものがある (体験版でも)。必要ならゲームのフォルダに `key.toml` を作る。サーバーを起動せずに前処理だけを行う場合は `vnrun-prep` を使う。

```bash
cargo run --release -p vnrun-prep -- ~/games/プリマドール/StartData/gamedata
```

[sdrs-core](https://github.com/a-kaibu/sdrs/tree/main/sdrs-core) は `third_party/sdrs` に submodule として取り込んでいる。SDRS はフォルダ直下の EXE のヘッダーで判定し、検出したものだけを展開する。`vnrun-prep <ゲームフォルダ>` でも同じ自動検出を行う。EXE を個別に指定することもできる。

```bash
cargo run --release -p vnrun-prep -- --sdrs ~/games/作品/主プログラム.exe
```

展開した EXE と復号したデータは同じゲームフォルダに配置し、元ファイルはその中の `sdrs_backup_<番号>_<番号>/` に退避する。退避先は処理後に表示する。復号に失敗した場合は元ファイルを変更しない。展開後は同じフォルダの Siglus 向け前処理も続けて行う。この後続処理で失敗しても、展開結果とバックアップは残る。展開済みの EXE とバックアップ内の EXE は再展開しない。展開できても、ゲームを遊べるかは各エンジンの対応状況による。

吉里吉里の EXE 内蔵 cxdec は、対応する bres リソースを見つけると `vnrun-cxdec.bin` にパラメーターと制御表を保存する。
元の EXE と XP3 は変更せず、Windows の EXE や DLL も実行しない。
ブラウザではアーカイブ内の2ファイルを復号し、Adler-32 が一致する場合だけ利用する。
対応範囲、Pad、再現用テストは [docs/krkr-compatibility.md](docs/krkr-compatibility.md) に記録している。

取り込んだエンジンへのブラウザ向けの修正は `patches/<エンジン>/*.patch` に置き、`engines/<id>/build.sh` がビルド前に当てる。

ページだけ直すなら `cd web && npx vp dev` (/game/ は 127.0.0.1:8090 の devserver に流す。`VNRUN_DEVSERVER` で変更可)、確認は `npx vp check`。

動作の確認には `tools/shot.js` (Playwright。WebGPU のためにフル版 Chromium を Vulkan SwiftShader で動かす) が使える: `node tools/shot.js http://127.0.0.1:8090/ out/sion "wait:40,shot:title,at:1050;120,wait:10,shot:story"`。
