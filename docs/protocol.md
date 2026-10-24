# プレイヤーとサーバーの約束事

プレイヤー (`web/`) は静的なページで、ゲームのファイルとセーブを HTTP で受け取る。サーバー側 (`crates/devserver`) は下の URL を用意するだけでよい。

ページは `index.html?game=<base>` で開く。`<base>` はゲーム 1 本の URL の根 (例: `/p/b2011_catwalk`)。以下のパスはすべて `<base>` からの相対。

| メソッド | パス | 中身 |
|---|---|---|
| GET | `manifest.json` | ゲームの説明とファイル一覧 (下記) |
| GET | `files/<path>` | ゲームのファイル。`Range` に対応すること |
| GET | `saves/` | セーブの一覧: `[{"p": "<path>", "s": <バイト数>, "m": <更新時刻 ms>}]` |
| GET | `saves/<path>` | セーブ 1 つ |
| PUT | `saves/<path>` | セーブを書く (本文がそのまま中身) |
| DELETE | `saves/<path>` | セーブを消す |

`<path>` はゲームの根からの相対パスで、区切りは `/`、各要素は URL エンコードする。`..` や先頭の `/` は受け付けない。

## manifest.json

```json
{
  "title": "シオンの血族 体験版",
  "engine": "siglus",
  "nls": "sjis",
  "files": [{"p": "Scene.pck", "s": 1234567}, {"p": "g00/bg001.g00", "s": 34567}]
}
```

- `engine`: `siglus` / `reallive` / `avg32` / `uk2` (siglus_rs で動かすもの)。今後 `onscripter` / `kirikiri` / `artemis` / `bgi` を足す。
- `nls`: 省略可。RealLive / AVG32 の文字コード (`sjis` など)。Siglus では使わない。
- `files`: ゲームの根以下の全ファイル。大文字小文字はそのまま (プレイヤー側で区別せずに引く)。

## セーブ

ゲームが書いたファイルはすべてセーブとして扱う。プレイヤーは起動時に `saves/` の一覧を読み、ゲームのファイルより優先して見せる。書き込みは届いた順に `PUT` する。

## フォント

RealLive / AVG32 は文字を描くのにフォントが要る (ブラウザから OS のフォントは見えない)。プレイヤーは自分と同じ場所の `fonts/fonts.json` (ファイル名の配列) を読み、並んだフォントを登録する。無ければ登録しない。
