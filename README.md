# vnrun — ノベルゲームのエンジンをブラウザで動かす

Windows 向けノベルゲームを、Wine を使わずにブラウザの中で動かすためのプレイヤー。ゲームのデータはvnrun 自身のサーバーから HTTP で読み、エンジンは WebAssembly でブラウザ側で動く。サーバーはファイルとセーブを配るだけなので、非力なホストでも重くならない。

## 対応エンジン

| エンジン | 実装 | 状態 |
|---|---|---|
| SiglusEngine / RealLive / AVG32 / UK2 | [siglus_rs](https://github.com/xmoezzz/siglus_rs) (`third_party/siglus_rs`) | 組み込み済み。Siglus 2 本・RealLive 2 本の体験版で確認 ([docs/handoff.md](docs/handoff.md)) |
| Softpal (PAL) | [sena-rs](https://github.com/xmoezzz/sena-rs) (`third_party/sena-rs`、MPL-2.0) | 組み込み済み。ただし sena-rs は「恋×シンアイ彼女」向けで、試したエレクトリップの体験版は注意書き画面の後で止まる |
| BGI / Ethornell | [ethornell-rs](https://github.com/xmoezzz/ethornell-rs) (`third_party/ethornell-rs`、MPL-2.0) | 組み込み済み。BGI 1 本の体験版で本編の文章まで確認 |

今後の候補は [docs/handoff.md](docs/handoff.md) の対応表を参照。

DRM (PlayDRM、ソフト電池など) の付いた製品版は扱わない。そうした作品は vnbox の Wine で、正規の認証を通して遊ぶ。

## 構成

```
web/                 プレイヤーのページ (エンジンに依らない部分)
  player.js          ?game=<URL> のゲームを開き、エンジンに渡す
  vfs.js             ゲームのファイルとセーブを HTTP で読み書きする
  engines/<id>.js    エンジンごとのつなぎ
engines/<id>/build.sh  エンジンを wasm にして dist/engines/<id>/ に置く
third_party/         取り込んだエンジン (git submodule)
crates/devserver     手元で試すためのサーバー
crates/prep          ブラウザでできない下ごしらえ (Siglus の二次鍵の復元など)
patches/<id>/        取り込んだエンジンへの修正
tools/shot.js        ヘッドレス Chromium で起動・操作・スクショ
docs/protocol.md     プレイヤーとサーバーの約束事
```

## ビルドと試し方

Rust (rustup) が要る。

```bash
git submodule update --init
./build.sh                                   # → dist/
cargo run --release -p vnrun-devserver -- --game ~/games/シオンの血族_体験版/StartData/GameData
# → http://127.0.0.1:8090/ をブラウザで開く。セーブは ./saves/<フォルダ名>/ に残る
```

`--game` はエンジンのファイルがある場所 (Siglus なら `Scene.pck` と `Gameexe.dat` のあるフォルダ) を指す。エンジンは自動で判定し、外れたら `--engine siglus|reallive|avg32|uk2` で指定する。別の端末から開くなら `--addr 0.0.0.0:8090`。

SiglusEngine の作品には暗号の二次鍵が要るものがある (体験版でも)。ブラウザ版は鍵を探さないので、先に一度だけ `vnrun-prep` を流してゲームのフォルダに `key.toml` を作っておく。

```bash
cargo run --release -p vnrun-prep -- ~/games/プリマドール/StartData/gamedata
```

取り込んだエンジンへのブラウザ向けの修正は `patches/<エンジン>/*.patch` に置き、`engines/<id>/build.sh` がビルド前に当てる。

動作の確認には `tools/shot.js` (Playwright) が使える: `node tools/shot.js http://127.0.0.1:8090/ out/sion "wait:40,shot:title,at:1050;120,wait:10,shot:story"`。
