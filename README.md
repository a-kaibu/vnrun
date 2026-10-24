# vnrun — ノベルゲームのエンジンをブラウザで動かす

Windows 向けノベルゲームを、Wine を使わずにブラウザの中で動かすためのプレイヤー。ゲームのデータはサーバー ([vnbox](https://github.com/a-kaibu/vnbox) など) から HTTP で読み、エンジンは WebAssembly でブラウザ側で動く。サーバーはファイルとセーブを配るだけなので、非力なホストでも重くならない。

## 対応エンジン

| エンジン | 実装 | 状態 |
|---|---|---|
| SiglusEngine / RealLive / AVG32 / UK2 | [siglus_rs](https://github.com/xmoezzz/siglus_rs) (`third_party/siglus_rs`) | 組み込み済み。実物の作品では未確認 |

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
docs/protocol.md     プレイヤーとサーバーの約束事 (vnbox が実装する)
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

SiglusEngine の製品版は暗号の二次鍵が要る。siglus_rs が起動時に総当たりで探し、ゲームのフォルダに `key.toml` を書く (書けなければ毎回探す)。体験版は鍵なしで動くことが多い。
