# vnrun — ノベルゲームのエンジンをブラウザで動かす

Windows 向けノベルゲームを、Wine を使わずにブラウザの中で動かすためのプレイヤー。ゲームのデータはvnrun 自身のサーバーから HTTP で読み、エンジンは WebAssembly でブラウザ側で動く。サーバーはファイルとセーブを配るだけなので、非力なホストでも重くならない。

## 対応エンジン

| エンジン | 実装 | 状態 |
|---|---|---|
| SiglusEngine / RealLive / AVG32 / UK2 | [siglus_rs](https://github.com/xmoezzz/siglus_rs) (`third_party/siglus_rs`) | 組み込み済み。Siglus 2 本・RealLive 2 本の体験版で確認 ([docs/handoff.md](docs/handoff.md)) |
| Softpal (PAL) | [sena-rs](https://github.com/xmoezzz/sena-rs) (`third_party/sena-rs`、MPL-2.0) | 組み込み済み。ただし sena-rs は「恋×シンアイ彼女」向けで、試したエレクトリップの体験版は注意書き画面の後で止まる |
| BGI / Ethornell | [ethornell-rs](https://github.com/xmoezzz/ethornell-rs) (`third_party/ethornell-rs`、MPL-2.0) | 組み込み済み。BGI 1 本の体験版で本編の文章まで確認 |
| FVP | [rfvp](https://github.com/xmoezzz/rfvp) (`third_party/rfvp`) | 組み込み済み。さくら、もゆ。体験版で本編の文章とセーブまで確認 |
| Artemis | [art3m1s-core](https://github.com/Alphaly2K/art3m1s-core) (`third_party/art3m1s-core`、MPL-2.0) | 組み込み中。描画はまだ WebGL2 経由、試した体験版は E-Mote の読み込みで止まる |
| NScripter | [ONScripter-Yuri](https://github.com/YuriSizuku/OnscripterYuri) (`third_party/onscripter-yuri`、GPL-2.0) | 組み込み済み。自作のテストスクリプトで文章・選択肢・セーブまで確認 (実物の作品は未確認) |
| 吉里吉里 (KAG) | [KiriKiri SDL2](https://github.com/krkrsdl2/krkrsdl2) (`third_party/krkrsdl2`、MIT ほか) | 組み込み済み。KAG3 のサンプルを改変したテストで文章・選択肢・セーブまで確認 (商用作品は暗号化 xp3 とプラグイン DLL があり未確認) |
| TyranoScript、RPGツクール MV/MZ など (NW.js) | ゲーム自身 (iframe で開くだけ) | 組み込み済み。TyranoScript の公式サンプルでタイトル・本編・セーブ・ロードまで確認 (RPGツクールは未確認、package.nw に固めた作品は未対応) |

今後の候補は [docs/handoff.md](docs/handoff.md) の対応表を参照。

DRM (PlayDRM、ソフト電池など) の付いた製品版は扱わない。そうした作品は vnbox の Wine で、正規の認証を通して遊ぶ。

## 構成

```
web/                 プレイヤーのページ (Vite+ + TypeScript、エンジンに依らない部分)
  src/main.ts        ?game=<URL> のゲームを開き、エンジンに渡す
  src/gamefs.ts      ゲームのファイルとセーブを HTTP で読み書きする
  src/present.ts     CPU で描いたフレームを WebGPU で画面に出す
  src/engines/<id>.ts  エンジンごとのつなぎ
engines/<id>/build.sh  エンジンを wasm にして web/public/engines/<id>/ に置く
third_party/         取り込んだエンジン (git submodule)
crates/devserver     手元で試すためのサーバー
crates/prep          ブラウザでできない下ごしらえ (Siglus の二次鍵の復元など)
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

`--game` はエンジンのファイルがある場所 (Siglus なら `Scene.pck` と `Gameexe.dat` のあるフォルダ) を指す。エンジンは自動で判定し、外れたら `--engine siglus|reallive|avg32|uk2|bgi|softpal|fvp|artemis|nscripter` で指定する。別の端末から開くなら `--addr 0.0.0.0:8090`。

SiglusEngine の作品には暗号の二次鍵が要るものがある (体験版でも)。ブラウザ版は鍵を探さないので、先に一度だけ `vnrun-prep` を流してゲームのフォルダに `key.toml` を作っておく。

```bash
cargo run --release -p vnrun-prep -- ~/games/プリマドール/StartData/gamedata
```

取り込んだエンジンへのブラウザ向けの修正は `patches/<エンジン>/*.patch` に置き、`engines/<id>/build.sh` がビルド前に当てる。

ページだけ直すなら `cd web && npx vp dev` (/game/ は 127.0.0.1:8090 の devserver に流す。`VNRUN_DEVSERVER` で変更可)、確認は `npx vp check`。

動作の確認には `tools/shot.js` (Playwright。WebGPU のためにフル版 Chromium を Vulkan SwiftShader で動かす) が使える: `node tools/shot.js http://127.0.0.1:8090/ out/sion "wait:40,shot:title,at:1050;120,wait:10,shot:story"`。
