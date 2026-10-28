# Akabei 方式の XP3 復号

`patches/krkrz/0009-xp3-akabei-rolling-xor.patch` は、ファイルのハッシュから32バイトの XOR 鍵を作る方式を自動検出する。
復号はブラウザ側でファイルを読むときに行い、元の XP3 は書き換えない。
処理は [GARbro の AkabeiCrypt.GetKey](https://github.com/morkt/GARbro/blob/master/ArcFormats/KiriKiri/CryptAlgorithms.cs) と同じ鍵生成式に対応する。

## 鍵の生成と検出

索引の `adlr` チャンクの32ビット値を `hash`、アーカイブ共通の値を `seed` とする。
演算は32ビット符号なし整数として行う。

```text
state = (hash XOR seed) AND 0x7fffffff
state = state OR (state << 31)
for i in 0..31:
    key[i] = state AND 0xff
    state = ((state AND 0xfffffffe) << 23) OR (state >> 8)
plain[offset] = encrypted[offset] XOR key[offset AND 31]
```

最初の鍵4バイトをリトルエンディアンで読むと、初期の `state` が得られる。
PNG や Ogg の既知ヘッダーからその4バイトを求め、`seed = (state XOR hash) AND 0x7fffffff` を候補にする。
残りの既知ヘッダーも一致する候補に票を与え、従来の XOR 方式と同じ検出処理の中で比較する。
作品名や EXE ごとの鍵一覧は持たない。

候補の鍵は保護対象ファイル2つの Adler-32 で確かめる。
1 MiB 以下のファイルを索引順に使い、足りなければ 16 MiB 以下の大きいファイルを小さい順に使う (`0015`)。
生命のスペアの evimage.xp3 はイベント CG だけで、どれも 1 MiB を超える。

鍵の位置は非圧縮ファイルの先頭からのオフセットで決める。
圧縮セグメントを展開した後に復号するため、途中位置からの読み込みでも同じ結果になる。
この処理は `engines/krkr/xp3_akabei.h` に置き、ビルド時にエンジンへコピーする。

## 検証

合成データの固定ベクトルに加え、1バイト目から63バイト目までの各位置で読み込みを分割するテストを用意した。
32ビットを超えるオフセットも確認する。

```bash
bash engines/krkr/test.sh
```

2026-10-26 に取得した公開体験版では、復号した保護対象ファイルの Adler-32 を XP3 索引の値と比較した。
結果は下表のとおり。

| 体験版 | 一致したファイル数 | 不一致 |
|---|---:|---:|
| ATATA | 2,372 | 0 |
| 墓多 DYING ZOMBIES | 4,201 | 0 |
| ROLEPLAYER 小粥姉妹 | 6,195 | 0 |
| ROLEPLAYER とろろ姉妹 | 5,892 | 0 |
| ソープランドのはっしゃくさま | 3,236 | 0 |

5本の合計は21,896ファイルで、不一致はなかった。

各アーカイブには保護対象外の説明ファイルが1つあり、復号対象の件数には含めない。
ブラウザで確認した到達点は [動作確認表](verification.md) に記録する。
