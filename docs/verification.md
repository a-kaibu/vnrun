# 実物の作品での動作確認

vnbox に登録されている作品 (vnbox で Wine 上の動作を確かめた 690 本。すべて体験版で、配布元の URL が vnbox の `results/batch/recheck_all.tsv` にある) を vnrun で動かした記録。vnrun に実装があるエンジンの作品を対象にする。

## やり方

- 対象の一覧は `tools/verify/targets.tsv` (id、vnrun のエンジン、作品名、URL)。vnbox の台帳のエンジン名から引いたもの。エンジン名が空の作品 (約 200 本) はまだ入れていない。
- `tools/verify/verify.sh all` が 1 本ずつ「配布元から取得 → 展開 → `vnrun-prep` → `vnrun-devserver` → ヘッドレス Chromium (`tools/shot.js`) で起動 → スクリーンショット → 削除」を回す。45 秒後と 105 秒後に撮り、画面の中央をクリックしてもう 1 枚撮る。
- スクリーンショットを目で見て `tools/verify/status.tsv` に結果を書き、`python3 tools/verify/matrix.py` で下の表を作る。
- 本編まで進めるのは、タイトルのボタンの位置を見て `verify.sh run <id> <steps>` で個別に。
- 環境: クラウドのコンテナ (4 コア、GPU なし)。Chromium の WebGPU は SwiftShader (Vulkan) で動かしているので、実機より大幅に遅い。起動に時間がかかる作品は「要再確認」にして待ち時間を延ばして撮り直す。
- RealLive は文字を描くフォントが要る。`web/public/fonts/` に IPAex ゴシック / 明朝を置いて確かめた (docs/protocol.md の「フォント」)。
- 2026-10-26〜27 の再確認は、ローカル Linux、Chromium 1248、SwiftShader、Noto Sans CJK JP で実行した。[Akabei の復号結果](xp3-akabei.md) と [cxdec・Pad・data.bin などの検証記録](krkr-compatibility.md) も記録している。

結果の意味:

| 結果 | 意味 |
|---|---|
| 本編 | 本編の文章 (または選択肢・体験版メニュー) まで出た |
| タイトル | タイトル画面 (注意書き画面やエピソード選択を含む) まで出た |
| 起動 | 何か描かれたがタイトルまで行かない |
| 要再確認 | 確認の仕方の問題で判断できなかった |
| NG | エンジンのエラーで止まった |
| 対象外 | 展開できない (インストーラ形式など) |

## 見つかったこと

- **vnbox の「FVP」4 本は FVP ではない** (らぶでれーしょん!、いたずら極リアル、小悪魔発育チュ～!、お姉ちゃんのあまエロレッスン 2。どれも `.hcb` が無い)。vnbox の台帳には FVP の作品が無いので、FVP は表に入れていない (さくら、もゆ。の体験版で別に確認済み)。
- Siglus: サヤガタリの起動直後の `RuntimeError: unreachable` (1080p のムービーを全フレーム一括でデコードしてメモリが尽きていた) と、プリマドールのタイトルの「最初から」が効かない件は 9fd1333 で直った。
- Siglus: 起動して黒画面のまま待ち、クリックで初めてタイトルが出る作品がある (魔法少女アリサ、淫紋闘技都市ソドム、娼姫レティシア、魔法少女コノカ)。
- BGI: 21 本中 11 本が、起動時に BGI 自身のエラー「無効なサイズ [ 0 ] byte のメモリ領域を確保しようとしました / メモリサイズは最大で 67108864 byte までしか確保できません」で止まる。2013 年以降の作品 (HOOKSOFT、Lump of Sugar、SMEE、すけべらそめろ) に多く、タイトルまで出たのは 2006〜2011 年の作品とクダンノフォークロア。ほかに黒画面のまま進まないもの 2 本、`UnknownDispatch` で止まるもの 1 本。PrismRhythm (800x600) は画面が拡大されず左上に小さく出る。
- BGI (c27e445 の後): 11 本のうち 7 本がタイトル以上に。残りは「unsupported filesystem/platform/installer host capability」(連想リレイション)、「無効なスクリーンサイズ」(ピュアコネクト HD)、「sysgrp.arc : caret が無い」(若葉色のカルテット)、「無効なサイズ [ 0 ]」(推しのラブより恋のラブ)。
- Artemis: E-Mote を使う作品 (フユキス) は PSB の読み込み (`truncated_width`) で止まる。セレクトオブリージュは `system/extend/user/mw_save.lua` が見つからず止まる。
- Softpal: 2017 年以降の作品はほぼタイトルまで出る。エレクトリップの一部は「script stack underflow」、2011〜2012 年の作品は最初の `sp_set_ex` で止まる。
- NScripter: PONScripter (うみねこ咲) は台本の形式が違って読めない。ナルキッソス 2nd・トライアンソロジーはタイトルまで (後者は anm.dll などのレイヤ効果が未実装で背景が出ない)。
- RealLive: `fonts/` が無いと文章の窓が空になる。
- EntisGLS: 5 本中 4 本 (Cotopha の .csx を持つ 2007〜2010 年の作品) は本編の文章まで。狙われた優等生 (2024) は .csx が無く、システムが exe に組み込まれ台本が xscript (`.srcxml`) の新しい世代なので NG。

## 結果

<!-- matrix -->
| エンジン | 本数 | 本編 | タイトル | 起動 | 要再確認 | NG | 対象外 | 未確認 |
|---|---|---|---|---|---|---|---|---|
| SiglusEngine (siglus_rs) | 19 | 5 | 14 |  |  |  |  |  |
| RealLive (siglus_rs) | 8 | 6 |  |  | 1 |  | 1 |  |
| BGI (ethornell-rs) | 21 | 11 | 10 |  |  |  |  |  |
| Softpal (sena-rs) | 28 | 27 |  |  |  | 1 |  |  |
| Artemis (art3m1s-core) | 6 | 1 | 5 |  |  |  |  |  |
| NScripter (ONScripter-Yuri) | 5 |  | 2 |  |  | 1 | 2 |  |
| HTML (TyranoScript、RPGツクール MV/MZ) | 43 | 42 | 1 |  |  |  |  |  |
| 吉里吉里 (KiriKiri SDL2) | 135 | 121 | 13 |  |  |  | 1 |  |
| YU-RIS (vnrun) | 29 | 28 |  |  |  |  | 1 |  |
| NeXAS (vnrun) | 15 | 13 |  |  | 1 |  | 1 |  |
| AdvHD (vnrun) | 35 | 30 |  |  |  |  |  | 5 |
| Tmr-Hiro (vnrun) | 22 | 22 |  |  |  |  |  |  |
| LiveMaker (vnrun) | 7 | 4 |  |  |  |  |  | 3 |
| CatSystem2 (vnrun) | 8 | 3 |  |  |  |  |  | 5 |
| EntisGLS (vnrun) | 9 | 4 |  |  |  | 1 |  | 4 |
| AliceSoft System4 (xsystem4) | 2 | 2 |  |  |  |  |  |  |
| QLIE (vnrun) | 5 | 2 |  |  |  | 1 |  | 2 |
| Escude (vnrun) | 8 | 2 |  |  |  |  |  | 6 |
| Majiro (vnrun) | 4 | 1 |  |  |  |  |  | 3 |
| Kaguya (vnrun) | 12 | 6 |  |  |  |  |  | 6 |
| Interheart (vnrun) | 12 | 10 |  |  |  | 1 |  | 1 |
| Crowd/ANIM (vnrun) | 10 | 5 |  |  |  | 4 |  | 1 |
| Yuka (vnrun) | 8 | 7 |  |  |  |  |  | 1 |
| System-NNN (vnrun) | 6 | 6 |  |  |  |  |  |  |
| Bishop (vnrun) | 6 | 6 |  |  |  |  |  |  |
| Eushully AGE (vnrun) | 4 | 4 |  |  |  |  |  |  |
| Atelier G/H (vnrun) | 1 | 1 |  |  |  |  |  |  |
| DebonosuWorks (vnrun) | 1 | 1 |  |  |  |  |  |  |
| Cyberworks (vnrun) | 9 | 4 |  |  |  |  |  | 5 |
| LiLiM (vnrun) | 7 | 7 |  |  |  |  |  |  |
| Silky's (vnrun) | 2 | 2 |  |  |  |  |  |  |
| Ivory (vnrun) | 2 | 2 |  |  |  |  |  |  |
| NonColor (vnrun) | 2 | 2 |  |  |  |  |  |  |
| AZ System (vnrun) | 1 | 1 |  |  |  |  |  |  |
| PJADV (vnrun) | 1 | 1 |  |  |  |  |  |  |
| Eomaia (vnrun) | 1 | 1 |  |  |  |  |  |  |
| ShiinaRio (vnrun) | 7 | 1 |  |  |  |  |  | 6 |
| NekoSDK / Waffle (vnrun) | 6 | 5 |  |  |  |  |  | 1 |
| DxLib MED (vnrun) | 3 | 3 |  |  |  |  |  |  |
| Liar-soft XFL (vnrun) | 2 | 2 |  |  |  |  |  |  |
| CrossOver Adv.exe (vnrun) | 1 | 1 |  |  |  |  |  |  |
| Propeller MPK (vnrun) | 3 | 2 |  |  |  |  | 1 |  |
| SakanaGL (vnrun) | 3 | 3 |  |  |  |  |  |  |

### SiglusEngine (siglus_rs)

| ID | 作品 | 結果 | メモ |
|---|---|---|---|
| b2011_catwalk | [シオンの血族](http://suezou.dyndns.org/dl2011/catwalk/sion/sion_try.zip) | 本編 |  |
| b2015_hamham | [ろーらいず!!! 体験版](http://suezou.dyndns.org/dl2015/hamham/lr/LR_taiken.zip) | 本編 | 体験版メニュー画面まで |
| dl_VJ01006708 | [【特典同梱版】FORTUNE×WORLD ～僕らがゲームを作る理由～](https://trial.dlsite.com/professional/VJ01007000/VJ01006708_trial.zip) | 本編 | プロローグの文章が自動で流れる |
| dl_VJ01006709 | [【通常版】FORTUNE×WORLD ～僕らがゲームを作る理由～](https://trial.dlsite.com/professional/VJ01007000/VJ01006709_trial.zip) | 本編 | プロローグの文章が自動で流れる |
| s2017_sagaplanets_kinkoi | [金色ラブリッチェ体験版](http://suezou.dyndns.org/dl2017/sagaplanets/kinkoi/kinkoi_trial.zip) | 本編 | プロローグの文章 |
| dl_VJ01000468 | [プリマドール 冬空花火／雪華文様](https://trial.dlsite.com/professional/VJ01001000/VJ01000468_trial.zip) | タイトル | プリマドール体験版と同じ中身 |
| dl_VJ01000672 | [【通常版】みことのり！ ～妹巫女と中出し除霊～](https://trial.dlsite.com/professional/VJ01001000/VJ01000672_trial.zip) | タイトル |  |
| dl_VJ01000673 | [【限定版】みことのり！ スペシャルパック](https://trial.dlsite.com/professional/VJ01001000/VJ01000673_trial.zip) | タイトル |  |
| dl_VJ01002167 | [プリマドール 無名典礼](https://trial.dlsite.com/professional/VJ01003000/VJ01002167_trial.zip) | タイトル |  |
| dl_VJ01003134 | [淫紋闘技都市ソドム ～大観衆の前で強制受胎させられる聖戦姫～](https://trial.dlsite.com/professional/VJ01004000/VJ01003134_trial.zip) | タイトル | 黒画面のまま待つ。クリックでタイトル |
| dl_VJ01005112 | [【限定版】魔法少女コノカ －異種触手・無限煉獄－](https://trial.dlsite.com/professional/VJ01006000/VJ01005112_trial.zip) | タイトル | 最初は黒画面、クリックでタイトル |
| dl_VJ01005116 | [【限定版】魔法少女アリサ ～異種触手・無限アクメ受胎～](https://trial.dlsite.com/professional/VJ01006000/VJ01005116_trial.zip) | タイトル | 黒画面のまま待つ。クリックでタイトル |
| dl_VJ01006176 | [【限定版】モザイクの天使【オナサポボイスドラマ＆デジタル原画・設定資料集付き】](https://trial.dlsite.com/professional/VJ01007000/VJ01006176_trial.zip) | タイトル | 注意画面まで (6181 と同じ作品) |
| dl_VJ01006181 | [【限定版】モザイクの天使【デジタル原画・設定資料集付き】](https://trial.dlsite.com/professional/VJ01007000/VJ01006181_trial.zip) | タイトル |  |
| dl_VJ01006508 | [娼姫レティシア ～今宵、王女は春を売る～ リマスター豪華特典版](https://trial.dlsite.com/professional/VJ01007000/VJ01006508_trial.zip) | タイトル | 黒画面のまま待つ。クリックでタイトル |
| dl_VJ01006640 | [サヤガタリ -妖刀の約束-](https://trial.dlsite.com/professional/VJ01007000/VJ01006640_trial.zip) | タイトル | 9fd1333 でクラッシュが直りタイトルまで (開発スレッドでは本編まで確認) |
| r1_siglus_primadoll | [プリマドール 体験版](https://va-trialdist.azureedge.net/trial-primadoll-1.zip) | タイトル | 9fd1333 で「最初から」→エピソード選択まで進むようになった |
| s2013_sagaplanets_karumaruka | [カルマルカ＊サークル 体験版](http://suezou.dyndns.org/dl2013/sagaplanets/karumaruka/karumaruka_circle_trial.zip) | タイトル |  |
| s2023_hadashi_harem_shangrila | [shangrila_trial.exe](http://suezou.dyndns.org/dl2023/hadashi/harem_shangrila/shangrila_trial.exe) | タイトル | タイトルの後に「siglus wasm step failed: unhandled form command chain [38, 2, 3]」 |

### RealLive (siglus_rs)

| ID | 作品 | 結果 | メモ |
|---|---|---|---|
| b2007_hadashi | [ハーレム×すくらっち体験版](http://suezou.dyndns.org/dl2007/hadashi/scratch/scratch_taiken.zip) | 本編 |  |
| b2007_sagaplanets | [聖炎天使エレアノール 体験版](http://suezou.dyndns.org/dl2007/sagaplanets/eleanor/EleanorTrial.lzh) | 本編 | fonts/ が要る |
| b2008_hadashi | [お姉さん×すくらっち体験版](http://suezou.dyndns.org/dl2008/hadashi/scratch2/trial.lzh) | 本編 |  |
| s2008_sagaplanets_cominghumming | [Coming×Humming!! -カミング・ハミング- （体験版）](http://suezou.dyndns.org/dl2008/sagaplanets/cominghumming/ComingHumming_Trial.zip) | 本編 | Chromium で本編のセーブ/ロード確認 (siglus_rs 0006: 選択肢の間の行番号を読み飛ばす) |
| s2009_hadashi_cool | [素直くーる体験版](http://suezou.dyndns.org/dl2009/hadashi/cool/trial.lzh) | 本編 | Chromium で本編のセーブ/ロード確認 (siglus_rs 0007: 番号なしのボタンはオブジェクト番号を返す) |
| s2014_hadashi_3pl | [3Ping Lovers!☆一夫二妻の世界へようこそ♪　体験版](http://suezou.dyndns.org/dl2014/hadashi/3pl/trial.zip) | 本編 | 最初の選択肢まで |
| s2010_sagaplanets_kisaragi | [キサラギGOLD★STAR](http://suezou.dyndns.org/dl2010/sagaplanets/kisaragi/kisaragi_goldstar_trial.zip) | 要再確認 | 展開後にエンジンの印のファイル (Seen.txt) が見つからない |
| r1_reallive_five | [5 -ファイブ- 体験版](http://ram.product.co.jp/5/DL/Five_trial_setup.exe) | 対象外 | インストーラ形式 (展開できない) |

### BGI (ethornell-rs)

| ID | 作品 | 結果 | メモ |
|---|---|---|---|
| b2006_lumpofsugar | [いつか、届く、あの空に。【体験版】](http://suezou.dyndns.org/dl2006/lumpofsugar/itsusora/itsusora_trial.lzh) | 本編 | ネイティブ (ethornell headless) で はじめから→本編→右クリックのメニューからセーブ/ロード確認 |
| b2010_lumpofsugar | [Hello,good-bye 体験版](http://suezou.dyndns.org/dl2010/lumpofsugar/hello_good-bye/Hello_goodbye_trial_web.zip) | 本編 | ネイティブ (ethornell headless) で 最初から→本編→ウィンドウの「記録」でセーブ、「読込」でロード確認 |
| b2011_lumpofsugar | [DiamicDays_Trial.zip](http://suezou.dyndns.org/dl2011/lumpofsugar/diamicdays/DiamicDays_Trial.zip) | 本編 | ネイティブ (ethornell headless) で はじめから→本編→セーブ/ロード確認 |
| b2013_lumpofsugar | [sekachu_trial_WEB2.zip](http://suezou.dyndns.org/dl2013/lumpofsugar/sekachu/sekachu_trial_WEB2.zip) | 本編 | ethornell-rs 0010 (解放済みスプライトのフェード、VM メモリ 512MB) でセーブ/ロード画面が開くようになり、本編のセーブ/ロード確認 (ネイティブ) |
| b2015_hooksoft | [トラベリングスターズ 体験版02 Ver1.00](http://suezou.dyndns.org/dl2015/hooksoft/traveling_stars/travelingstars_trial_02.zip) | 本編 | ネイティブ (ethornell headless) で はじめから→本編→右クリックのメニューからセーブ/ロード確認 |
| b2015_lump_of_sugar | [rensou_trial_WEB.zip](http://suezou.dyndns.org/dl2015/lump-of-sugar/rensourelation/rensou_trial_WEB.zip) | 本編 | ethornell-rs 0010 (ネイティブの絶対パス) で 最初から→本編→歯車のメニューからセーブ/ロード確認 (ネイティブ) |
| b2019_hooksoft | [どっちのｉが好きですか？　Ｈシーン体験版　Ver1.00](http://suezou.dyndns.org/dl2019/hooksoft/isuki/isuki_h_trial.zip) | 本編 | ethornell-rs 0010 (Sprite の GetProperty 0x10) でセーブ画面が開くようになり、本編 (H シーン体験版) のセーブ/ロード確認 (ネイティブ) |
| b2019_sukerasparo | [kudan_trial.zip](http://suezou.dyndns.org/dl2019/sukerasparo/kudan/kudan_trial.zip) | 本編 | ネイティブ (ethornell headless) で START→本編→SAVE&LOAD でセーブ/ロード確認 |
| b2020_sukerasomero | [oshilove_trial.zip](http://suezou.dyndns.org/dl2020/sukerasomero/oshi_love/oshilove_trial.zip) | 本編 | ネイティブ (ethornell headless) で START→本編→右端のメニューからセーブ/ロード確認 (約 7000 フレーム、遅い) |
| r1_bgi_kudan | [クダンノフォークロア 体験版](http://sukerasparo.com/data/kudan_trial.zip) | 本編 | b2019_sukerasparo と同じ作品 (クダンノフォークロア)。そちらで本編のセーブ/ロード確認 |
| s2016_hooksoft_amenitys_life | [AmenitysLife Ver1.00](http://suezou.dyndns.org/dl2016/hooksoft/amenitys_life/AmenitysLife_trial_02.zip) | 本編 | f81eded 以降も本編まで |
| dl_VJ01004381 | [顔のない月 -待宵の双椿-](https://trial.dlsite.com/professional/VJ01005000/VJ01004381_trial.zip) | タイトル | 28374cc (0009 更新) でタイトルまで |
| s2010_lumpofsugar_prismrhythm | [PrismRhythm-プリズムリズム- Web特別体験版](http://suezou.dyndns.org/dl2010/lumpofsugar/prismrhythm/PrismRhythm_trial_Web.lzh) | タイトル | 28374cc (0009 更新) でタイトルまで。800x600 の画面が中央に小さく出る |
| s2011_lumpofsugar_gackoh | [学☆王 - THE ROYAL SEVEN STARS - 体験入学版](http://suezou.dyndns.org/dl2011/lumpofsugar/gackoh/Gackoh_Trial.zip) | タイトル | 28374cc (0009 更新) でタイトルまで |
| s2014_hooksoft_meltymoment | [MeltyMoment 体験版 Ver1.00](http://suezou.dyndns.org/dl2014/hooksoft/meltymoment/MeltyMoment_trial.zip) | タイトル | c27e445 で直った |
| s2015_smee_pure_connect | [ピュア×コネクト  Ver1.00](http://suezou.dyndns.org/dl2015/smee/pure_connect/pureconnect_trial.zip) | タイトル | c27e445 で直った |
| s2019_lump_of_sugar_wakabairono_quartet | [若葉色のカルテット 体験版](http://suezou.dyndns.org/dl2019/lump-of-sugar/wakabairono_quartet/WakabaironoQuartet_trial.zip) | タイトル | 3b42c00 で解消 |
| s2020_hooksoft_houkago | [放課後シンデレラ 体験版 Ver1.00](http://suezou.dyndns.org/dl2020/hooksoft/houkago/houkago_trial.zip) | タイトル | c27e445 で直った |
| s2021_smee_pureconnect_hd | [pureconnect_fhd_trial.zip](http://suezou.dyndns.org/dl2021/smee/pureconnect_hd/pureconnect_fhd_trial.zip) | タイトル | 3b42c00 で解消 |
| s2022_hooksoft_houkago2 | [放課後シンデレラ２ 体験版 Ver1.01](http://suezou.dyndns.org/dl2022/hooksoft/houkago2/HoukagoCinderella2_trial_v1.01.zip) | タイトル | f81eded (BackML 0x8E) でタイトルまで |
| s2023_quince_soft_little_proncess_go | [リトルプリンセスＧＯ！ 体験版](http://suezou.dyndns.org/dl2023/quince-soft/little-proncess_go/LittlePrincessGO_TR.zip) | タイトル | 1c16d6c でもタイトルまで |

### Softpal (sena-rs)

| ID | 作品 | 結果 | メモ |
|---|---|---|---|
| b2011_erectlip | [りとる†びっち ～Little Bitch Girls in Hog Farm～ 体験版](http://suezou.dyndns.org/dl2011/erectlip/littlebitch/LittleBitch_trial.zip) | 本編 | 本文まで。体験版にセーブは無い (右クリックは設定) |
| b2015_erectlip | [巨乳大魔王のドスケベクエスト ～完全敗北した少年勇者クンＵＣ～　【体験版】](http://suezou.dyndns.org/dl2015/erectlip/dosukebe/dosukebe_try.zip) | 本編 | 本文まで |
| b2017_unisonshift | [あなたに恋する恋愛ルセット　【体験版】](http://suezou.dyndns.org/dl2017/unisonshift/recette/recette_try.zip) | 本編 | 本文まで。F9/F11 で保存と読み込み |
| b2018_erectlip | [爆乳催眠女教師　【体験版】](http://suezou.dyndns.org/dl2018/erectlip/bakusai/bakusai_try.zip) | 本編 | 本文まで |
| b2020_erectlip | [爆乳淫辱霊媒師　【体験版】](http://suezou.dyndns.org/dl2020/erectlip/injyoku_reibaishi/injyoku_reibaishi_try.zip) | 本編 | sena 0012 (未登録版のボタン位置 pos_move) でタイトルの並びが直り本文まで。セーブとロード |
| b2020_milimili | [shin-neko ～再会した妹との新たな関係～　【体験版】](http://suezou.dyndns.org/dl2020/milimili/shin_neko/shin-neko_try.zip) | 本編 | 本文まで |
| b2021_milimili | [ちんかもツインズ２！　～夏だ！海だ！ふたごの妹おっぱい対決だ！～　【体験版】](http://suezou.dyndns.org/dl2021/milimili/chinkamo2/chinkamo2_try.zip) | 本編 | sena 0011 (Pal.dll が効果 3 の縁取りを描くかを見る) で文章が出る。Q.Save/Q.Load |
| b2022_amuse_craft_erotica | [黒ギャル子ちゃんとしっぽり温泉旅行 ～Hしまくり三泊四日の旅～　【体験版】](http://suezou.dyndns.org/dl2022/amuse_craft-erotica/kurogal_onsen/kurogal_onsen_try.zip) | 本編 | 本文まで。Q.Save/Q.Load |
| b2023_erectlip | [爆乳遊廓 ～癒され太夫孕ませの夜鶴～　【体験版】](http://suezou.dyndns.org/dl2023/erectlip/bakunyu_yukaku/yukaku_try.zip) | 本編 | 本文まで、セーブとロード |
| b2024_milimili | [ちょっとナマイキなトイレの花子さん ～えっちぃ学園のおばけとエロエロ搾精性活～ 【体験版】](http://suezou.dyndns.org/dl2024/milimili/hanako/hanako_try.zip) | 本編 | プロローグの文章 (r1_softpal_hanako と同じ作品) |
| b2025_erectlip | [爆乳淫乱ビーチ　体験版](http://suezou.dyndns.org/dl2025/erectlip/inran_beach/inran_beach_try.zip) | 本編 | 本文まで、セーブとロード (Chromium) |
| b2025_unisonshift | [小日向柚月と初夜したいっ！　【体験版】](http://suezou.dyndns.org/dl2025/unisonshift/kohinata/kohinata_try.zip) | 本編 | sena 0012 (17:43 イージング付き相対移動、17:36) で START が出て本文まで。冒頭の演出が長い |
| b2026_crystalia | [ディメンション凸ラバース!! 【体験版】](http://suezou.dyndns.org/dl2026/crystalia/totsu_lover/totsulover_try.zip) | 本編 | 本文まで。F9/F11 で保存と読み込み |
| dl_VJ01002655 | [爆乳メイド調教 ～夜姦緊縛教育～](https://trial.dlsite.com/professional/VJ01003000/VJ01002655_trial.zip) | 本編 | 本文まで。F9/F11 で保存と読み込み |
| dl_VJ01004424 | [爆乳淫乱ビーチ](https://trial.dlsite.com/professional/VJ01005000/VJ01004424_trial.zip) | 本編 | b2025_erectlip と同じ作品。本文まで |
| r1_softpal_hanako | [トイレの花子さん 体験版](https://amusecraft.com/milimili/try/hanako_try.zip) | 本編 | 654935e 以降も本編の文章まで |
| r1_softpal_tenmao | [転性魔王さま 体験版](https://kumanomi-soft.amusecraft.com/tenmao/dl/tenmao_try.zip) | 本編 | 本文まで。文章窓の Q.Save/Q.Load で保存と読み込み (sena 0011) |
| s2013_erectlip_katekyo | [爆乳淫乱家庭教師　【体験版】](http://suezou.dyndns.org/dl2013/erectlip/katekyo/KATEKYO_try.zip) | 本編 | 本文まで。F9 で保存、F11 で読み込み |
| s2014_erectlip_furyou | [学園の生贄――慰み者と化した巨乳不良少女 ～白濁に侵される褐色＆堕肉の狂宴～【体験版】](http://suezou.dyndns.org/dl2014/erectlip/furyou/furyou_try.zip) | 本編 | sena 0012 (9:24 が呼び出し戻り先を外す) で Start から本文まで。体験版のメニューにセーブは無い |
| s2014_erectlip_gyokumajyo | [逆転魔女裁判－痴女な魔女に裁かれちゃう－　【体験版】](http://suezou.dyndns.org/dl2014/erectlip/gyokumajyo/gyakumajyo_try.zip) | 本編 | 本文まで。F9/F11 で保存と読み込み |
| s2014_qoobrand_majokoi | [魔女こいにっき 体験版 (第一弾)](http://suezou.dyndns.org/dl2014/qoobrand/mazyokoi/MAJOKOI_try.zip) | 本編 | 本文まで。F9/F11 で保存と読み込み |
| s2015_erectlip_maou | [巨乳魔王の敗北 魔王の挑戦](http://suezou.dyndns.org/dl2015/erectlip/maou/maou_try.zip) | 本編 | 最初の文章 |
| s2020_amuse_craft_erotica_kinbaku | [緊縛性奴会長 ～カレに知られたワタシの秘密～　【体験版】](http://suezou.dyndns.org/dl2020/amuse_craft-erotica/kinbaku/kinbaku_s_try.zip) | 本編 | 本文まで。F9/F11 で保存と読み込み |
| s2022_amuse_craft_erotica_tsumamomi | [つまもみ ～欲求不満の奥さんとなし崩しオイルマッサージ～　【体験版】](http://suezou.dyndns.org/dl2022/amuse_craft-erotica/tsumamomi/tsumamomi_try.zip) | 本編 | sena 0012 (装飾名の Pal.dll でも登録版として扱う、9:63 is_active) で注意書きの先の本文まで。保存は未確認 |
| s2023_milimili_onaho_yokai | [巨乳オナホ妖怪と田舎ライフもHも満喫生活　【体験版】](http://suezou.dyndns.org/dl2023/milimili/onaho_yokai/onaho_yokai_try.zip) | 本編 | 本文まで。F9/F11 で保存と読み込み |
| s2025_crystalia_crack_trick | [CRACK≡TRICK!](http://suezou.dyndns.org/dl2025/crystalia/crack_trick/cratri_try.zip) | 本編 | 本文まで。F9/F11 で保存と読み込み |
| s2026_amuse_craft_erotica_youkashi | [養花師の背徳愛～純真天使は抱かれて色めく～ 【体験版】](http://suezou.dyndns.org/dl2026/amuse_craft-erotica/youkashi/youkashi_try.zip) | 本編 | sena 0012 (幅の狭い文章窓を text_set_base の位置に置く) で文章窓が直る。セーブとロード |
| b2012_erectlip | [inrakuchikan_trial.zip](http://suezou.dyndns.org/dl2012/erectlip/inrakuchikan/inrakuchikan_trial.zip) | NG | 18:17 は exe の自己識別スタンプ (登録・コピー保護ではない、sena 0009 で実装)。配布 zip が途中で切れていて ev.pac などが欠ける |

### Artemis (art3m1s-core)

| ID | 作品 | 結果 | メモ |
|---|---|---|---|
| s2021_giga_fuyukiss | [フユキス　体験版 - Ver1.0.0](http://suezou.dyndns.org/dl2021/giga/fuyukiss/fuyukiss_trial.zip) | 本編 | E-Mote 旧形式 (truncated_width なし, RGBA8 など) に対応してタイトルから本編まで |
| r1_artemis | [遥かなるニライカナイ 体験版](https://project-navel.com/niraikanai/niraikanai_Trial.zip) | タイトル |  |
| s2020_wonder_fool_maokatsu | [まおかつ！　体験版 - Ver1.1](http://suezou.dyndns.org/dl2020/wonder-fool/maokatsu/maokatsu_webtrial1.1.zip) | タイトル |  |
| s2023_j_studio_reaero_dt | [リアルエロゲシチュエーション！DT　体験版 - Ver1.0.0](http://suezou.dyndns.org/dl2023/j-studio/reaero_dt/reaerodt_Trial2.7z) | タイトル |  |
| s2024_madosoft_selectoblige | [セレクトオブリージュ　体験版 - Ver1.0.0](http://suezou.dyndns.org/dl2024/madosoft/selectoblige/selectoblige_Trial.zip) | タイトル | f508d03 (欠けた include を飛ばす) でタイトルまで |
| s2025_j_studio_galsfiction | [GALSFICTION_Trial.zip](http://suezou.dyndns.org/dl2025/j-studio/galsfiction/GALSFICTION_Trial.zip) | タイトル | blur_k のシェーダーのコンパイルに失敗 (表示には出ていない) |

### NScripter (ONScripter-Yuri)

| ID | 作品 | 結果 | メモ |
|---|---|---|---|
| r1_naru2 | [ナルキッソス SIDE 2nd](https://stage-nana.sakura.ne.jp/download/naru2.lzh) | タイトル |  |
| r1_trt | [トライアンソロジー 体験版](https://07th-expansion.net/trt/trt_trial.zip) | タイトル | ボタンだけで背景が出ない (anm.dll などのレイヤ効果は未実装) |
| r1_saku | [うみねこのなく頃に咲 体験版](https://07th-expansion.net/umi_saku/saku_trial.zip) | NG | PONScripter の作品。0.txt / nscript.dat が無く読めない (0.utf などに未対応) |
| dl_RJ01724681 | [人妻のオナサポを受けて白濁生汁ピュッピュッピュッしなさい！](https://trial.dlsite.com/doujin/RJ01725000/RJ01724681_trial.zip) | 対象外 | 中身は Unity 製 (開発スレッドで確認) |
| r1_higanbana | [彼岸花の咲く夜に お試し版](https://07th-expansion.net/rose/higanbana_otameshi.zip) | 対象外 | インストーラ形式 (CreateInstall) で展開できない |

### HTML (TyranoScript、RPGツクール MV/MZ)

| ID | 作品 | 結果 | メモ |
|---|---|---|---|
| b2021_stick | [人妻LovelyMoment 体験版](http://suezou.dyndns.org/dl2021/stick/hitozuma_lovelymoment/hitozuma_lovelymoment-trial.zip) | 本編 | 682b666 (app.asar を vnrun-prep が展開、electron の代役) でタイトル→はじめから→名前入力→本編の背景・文章まで (Chromium) |
| b2022_lunaprism | [羞恥隷嬢学園 体験版](http://suezou.dyndns.org/dl2022/lunaprism/reijyo_gakuen/Reijyo_trial.zip) | 本編 | 682b666 (app.asar を vnrun-prep が展開) でタイトル→はじめから→本編の契約書の場面・文章まで (Chromium) |
| dl_RJ01529614 | [まぐわれ異世界珍道中](https://trial.dlsite.com/doujin/RJ01530000/RJ01529614_trial.zip) | 本編 | 検証スレッドでも Chromium でタイトル画面まで確認。タイトル→ニューゲーム→本編のマップ・会話まで (Chromium) |
| dl_RJ01604592 | [HEART OVERLOAD! 欲望でつながる戦術部隊](https://trial.dlsite.com/doujin/RJ01605000/RJ01604592_trial.zip) | 本編 | 検証スレッドでも Chromium でタイトル画面まで確認。タイトル→ニューゲーム→本編の会話まで (Chromium) |
| dl_RJ01612664 | [雪音の仇討ち物語](https://trial.dlsite.com/doujin/RJ01613000/RJ01612664_trial.zip) | 本編 | 検証スレッドでも Chromium でタイトル画面まで確認。f41e050 (フレームに合わせて拡大) でタイトル→ニューゲーム→難易度選択→本編まで (Chromium) |
| dl_RJ01618221 | [アリナと淫魔の呪い](https://trial.dlsite.com/doujin/RJ01619000/RJ01618221_trial.zip) | 本編 | 検証スレッドでも Chromium でタイトル画面まで確認。タイトル→ニューゲーム→オープニング選択→本編の文章まで (Chromium、タイトルまで約 200 秒) |
| dl_RJ01618747 | [スペースパトロール パニカ](https://trial.dlsite.com/doujin/RJ01619000/RJ01618747_trial.zip) | 本編 | 検証スレッドでも Chromium でタイトル画面まで確認。タイトル→ニューゲーム→本編の会話まで (Chromium) |
| dl_RJ01623688 | [まもの博物館](https://trial.dlsite.com/doujin/RJ01624000/RJ01623688_trial.zip) | 本編 | 検証スレッドでも Chromium でタイトル画面まで確認。cb9b86f (MZ もフレームに合わせて拡大) でタイトル→はじめから→体験版の案内→あらすじの言語選択まで (Chromium) |
| dl_RJ01642779 | [戦闘補習3～武闘派冒険者、魔術を学ぶ～](https://trial.dlsite.com/doujin/RJ01643000/RJ01642779_trial.zip) | 本編 | 検証スレッドでも Chromium でタイトル画面まで確認。f41e050 (フレームに合わせて拡大) でタイトル→ニューゲーム→本編のマップ・会話まで (Chromium) |
| dl_RJ01648691 | [測り難きは下心](https://trial.dlsite.com/doujin/RJ01649000/RJ01648691_trial.zip) | 本編 | 検証スレッドでも Chromium でタイトル画面まで確認。416970b (IndexedDB) 以降。クリックでマップ画面まで進む |
| dl_RJ01650072 | [どこでもHな世界～地味子のバイト日記～](https://trial.dlsite.com/doujin/RJ01651000/RJ01650072_trial.zip) | 本編 | 検証スレッドでも Chromium でタイトル画面まで確認。fcaacff (nw.gui などは代役) でタイトル→本編まで。既定の言語は中国語 (GameCreator、Chromium) |
| dl_RJ01655345 | [エリア404](https://trial.dlsite.com/doujin/RJ01656000/RJ01655345_trial.zip) | 本編 | Electron 版 TyranoScript (app.asar を vnrun-prep が展開) でタイトル→START→章選択「少女と百合の丘」→本編の文章まで、歯車メニューからセーブし、別セッションでタイトルの歯車→LOAD で同じ文章に戻る (Chromium) |
| dl_RJ01686418 | [秘密 女騎士NTR](https://trial.dlsite.com/doujin/RJ01687000/RJ01686418_trial.zip) | 本編 | 検証スレッドでも Chromium でタイトル画面まで確認。タイトル→ニューゲーム→本編の会話まで (Chromium) |
| dl_RJ01696742 | [サモンスピリッツ](https://trial.dlsite.com/doujin/RJ01697000/RJ01696742_trial.zip) | 本編 | 検証スレッドでも Chromium でタイトル画面まで確認。タイトル→ニューゲーム→本編の会話まで (Chromium) |
| dl_RJ01698597 | [かんもく:場面緘黙女子](https://trial.dlsite.com/doujin/RJ01699000/RJ01698597_trial.zip) | 本編 | 検証スレッドでも Chromium でタイトル画面まで確認。0df7c53 でタイトル→ニューゲーム→本編の文章 (名前入力の案内) まで (Chromium、タイトルまで約 240 秒) |
| dl_RJ01699647 | [とろぴかる・ちぇいす！長年追っていた指名手配犯をヌーディストビーチで発見したクール系女刑事](https://trial.dlsite.com/doujin/RJ01700000/RJ01699647_trial.zip) | 本編 | 検証スレッドでも Chromium でタイトル画面まで確認。タイトル→はじめから→導入の立ち絵・文章まで。「reading 'unlockInfo'」は実績プラグインがセーブ無しを読んだときのもので、プラグイン自身が拾って続行する |
| dl_RJ01699765 | [星直しのブレイブロード -淫欲の街と堕落の聖女候補-](https://trial.dlsite.com/doujin/RJ01700000/RJ01699765_trial.zip) | 本編 | 検証スレッドでも Chromium でタイトル画面まで確認。タイトル→ニューゲーム→本編の会話まで (Chromium) |
| dl_RJ01701172 | [ラッキィ・ホール-ハズレは淫魔の搾精穴-](https://trial.dlsite.com/doujin/RJ01702000/RJ01701172_trial.zip) | 本編 | 検証スレッドでも Chromium でタイトル画面まで確認。タイトル→挑戦する→導入の文章まで (Chromium) |
| dl_RJ01701742 | [【Live2D】逃亡中～ミッションをクリアしてザーメンの放出を阻止せよ！～](https://trial.dlsite.com/doujin/RJ01702000/RJ01701742_trial.zip) | 本編 | 検証スレッドでも Chromium でタイトル画面まで確認。タイトル→ニューゲーム→導入の文章まで (Chromium) |
| dl_RJ01702392 | [いとこのお姉ちゃんに睡眠●シミュレーション](https://trial.dlsite.com/doujin/RJ01703000/RJ01702392_trial.zip) | 本編 | 検証スレッドでも Chromium でタイトル画面まで確認。f41e050 (RPGツクールを NW.js と同じくフレームに合わせて拡大) でタイトル→ニューゲーム→色選択→本編の背景・文章まで (クリックで進む) |
| dl_RJ01702800 | [ドット勇者](https://trial.dlsite.com/doujin/RJ01703000/RJ01702800_trial.zip) | 本編 | 検証スレッドでも Chromium でタイトル画面まで確認。タイトル→本編の放置ゲーム画面で操作できる (Chromium) |
| dl_RJ01703802 | [決着はバトルファックで -初回特典付き-](https://trial.dlsite.com/doujin/RJ01704000/RJ01703802_trial.zip) | 本編 | 検証スレッドでも Chromium でタイトル画面まで確認。タイトル→ニューゲーム→本編のマップ・会話まで (Chromium、タイトルまで約 200 秒) |
| dl_RJ01706991 | [ショタおねRPG 爆乳お姉さんたちと催〇でドスケベ三昧 爆乳(クソデカボイン)の桃源郷(パラダイス)](https://trial.dlsite.com/doujin/RJ01707000/RJ01706991_trial.zip) | 本編 | 検証スレッドでも Chromium でタイトル画面まで確認。タイトル→ニューゲーム→名前入力→本編まで (Chromium) |
| dl_RJ01706994 | [戦術バトルは不○火舞に任せなさい♪](https://trial.dlsite.com/doujin/RJ01707000/RJ01706994_trial.zip) | 本編 | Electron の自作 HTML ゲーム (app.asar を vnrun-prep が展開、ページ遷移あり) でタイトル→START→街 (本編の拠点画面)→訓練所まで。localStorage の自動セーブ (tougij_save_v1) がサーバーに残り、別セッションで進めた時刻 (1日目 朝→昼) から再開 (Chromium) |
| dl_RJ01707379 | [Alice In Distress RE](https://trial.dlsite.com/doujin/RJ01708000/RJ01707379_trial.zip) | 本編 | Electron 版 TyranoScript (app.asar を vnrun-prep が展開) でタイトル→はじめる→導入の文章→着せ替え・拘束の本編画面まで (Chromium)。ゲームにセーブ機能が無い ([hidemenubutton] のみ) |
| dl_RJ01712071 | [魔胚受胎 ～古代触手と異種交配の実験記録～](https://trial.dlsite.com/doujin/RJ01713000/RJ01712071_trial.zip) | 本編 | Electron 版 TyranoScript (app.asar を vnrun-prep が展開) でロゴ (クリック待ち)→タイトル→初めから→難易度選択と確認→本編の文章まで。メニューからセーブし、別セッションでタイトルの「続きから」で同じ文章に戻る (Chromium) |
| dl_RJ01715225 | [梨乃ねね ～子種を乞う村～](https://trial.dlsite.com/doujin/RJ01716000/RJ01715225_trial.zip) | 本編 | Electron の PixiJS ゲーム (app.asar を vnrun-prep が展開、dist/index.html を開く。ページに process を置くとタイトルのボタンが効かなくなるため、サブフォルダのページには Node の代役を置かない) で成人確認→タイトル→迷い込む→本編の文章まで (Chromium)。体験版はセーブを書かない (ElectronBridge の JSON 保存は終了処理しか使われていない) |
| dl_RJ01720493 | [Moeのファンサービスリフト♡～つり出し](https://trial.dlsite.com/doujin/RJ01721000/RJ01720493_trial.zip) | 本編 | 検証スレッドでも Chromium でタイトル画面まで確認。782563a (studio_api.path、.tpatch は無いものとする) でタイトル→はじめから→本編の 3D 立ち絵・文章まで (Electron 版 Tyrano、Chromium) |
| dl_RJ01720685 | [アイドルダークダンジョン](https://trial.dlsite.com/doujin/RJ01721000/RJ01720685_trial.zip) | 本編 | 検証スレッドでも Chromium でタイトル画面まで確認。クリックで最初の会話まで |
| dl_RJ01720958 | [魔法戦士 NANAMI 前編](https://trial.dlsite.com/doujin/RJ01721000/RJ01720958_trial.zip) | 本編 | 検証スレッドでも Chromium でタイトル画面まで確認。タイトル→ニューゲーム→本編のマップまで (Chromium) |
| dl_RJ01722380 | [むぎゅっと☆搾精ナイトメア！](https://trial.dlsite.com/doujin/RJ01723000/RJ01722380_trial.zip) | 本編 | 検証スレッドでも Chromium でタイトル画面まで確認。タイトル→ニューゲーム→名前入力→本編まで (Chromium) |
| dl_RJ01727123 | [監禁部屋から逃げれない1](https://trial.dlsite.com/doujin/RJ01728000/RJ01727123_trial.zip) | 本編 | 検証スレッドでも Chromium でタイトル画面まで確認。タイトル→ニューゲーム→本編のマップまで (Chromium) |
| dl_RJ01730521 | [大魔法少女カサネ](https://trial.dlsite.com/doujin/RJ01731000/RJ01730521_trial.zip) | 本編 | 検証スレッドでも Chromium でタイトル画面まで確認。タイトル→ニューゲーム→本編の会話まで (Chromium) |
| dl_RJ01731048 | [ヤリステナイメスブター 世界を救った後の ボクの日常『性』活](https://trial.dlsite.com/doujin/RJ01732000/RJ01731048_trial.zip) | 本編 | 検証スレッドでも Chromium でタイトル画面まで確認。タイトル→ニューゲーム→本編の会話まで。「Cannot find module ./js/libs/greenworks」は出るが続行する (Chromium) |
| dl_RJ01735938 | [【ver 1.0.6】転生の間へようこそ](https://trial.dlsite.com/doujin/RJ01736000/RJ01735938_trial.zip) | 本編 | Electron の HTML ゲーム (app/index.html を開く) で導入の説明→最初の魂を迎える→転生先・生まれ方を決める本編画面まで (Chromium)。体験版はセーブ無し (説明に「セーブと図鑑は製品版」) |
| dl_RJ01744242 | [同じ階に住む美巨乳団地妻「改」](https://trial.dlsite.com/doujin/RJ01745000/RJ01744242_trial.zip) | 本編 | Electron の GDevelop ゲーム (app/index.html を開く) でタイトル→NEW GAME→はじめる→PLAY→本編の会話まで (Chromium)。ゲームにセーブ機能が無い (イベントに storage 系の命令が無い) |
| dl_RJ01746541 | [女魔法使いセーラは俺から逃げられない！ ～便利なエロアイテムと孕むまでヤれる封鎖村～](https://trial.dlsite.com/doujin/RJ01747000/RJ01746541_trial.zip) | 本編 | 検証スレッドでも Chromium でタイトル画面まで確認。タイトル→ニューゲーム→本編のマップ・立ち絵・会話まで (Chromium) |
| dl_RJ01747016 | [原○カードクワルドトライアド Vol.2](https://trial.dlsite.com/doujin/RJ01748000/RJ01747016_trial.zip) | 本編 | Electron の HTML ゲーム (dist/index.html を開く) で導入→冒険者の町のマップ→案内さんとの会話まで。localStorage の自動セーブ (ffcards_adventure_v1) がサーバーに残り、別セッションで同じ位置から再開 (Chromium) |
| dl_RJ357058 | [Last12 - 勇者の恥ずかしい育児日記 -](https://trial.dlsite.com/doujin/RJ358000/RJ357058_trial.zip) | 本編 | 検証スレッドでも Chromium でタイトル画面まで確認。a11563d (読み込み中のプラグイン例外は警告だけ) でタイトル→ニューゲーム→序章のマップまで (Chromium) |
| dl_VJ01000810 | [エッチなお姉ちゃんが、褐色巨乳の悪魔だった件](https://trial.dlsite.com/professional/VJ01001000/VJ01000810_trial.zip) | 本編 | exe 末尾の app を vnrun-prep が展開。タイトル→はじめから→本編の CG・文章まで (Chromium) |
| dl_VJ01001309 | [エッチなお姉ちゃんが、褐色巨乳の悪魔だった件2 ～新たな悪魔ミリス登場♪～](https://trial.dlsite.com/professional/VJ01002000/VJ01001309_trial.zip) | 本編 | e7626d4 (EXE 基準オフセットの app zip、.tpatch を vnrun-prep が適用) でタイトル→はじめから→本編の立ち絵・文章まで (Chromium) |
| r1_tyrano | [転生した世界で… 体験版](http://www.gocha-mix.com/gp/gxgp01/dl/gxgp001_trial.zip) | 本編 | dadce4b (exe 末尾の app を vnrun-prep が展開) でタイトル→はじめから→本編の背景・文章まで (Chromium、タイトルまで約 200 秒) |
| dl_RJ01741863 | [淫らに染めて ～叔母が堕ちるまで～](https://trial.dlsite.com/doujin/RJ01742000/RJ01741863_trial.zip) | タイトル | タイトルは出る。はじめから後は黒画面のまま (VisuMZ EventTitleScene→Map011 の場所移動待ちで止まる、要調査) |

### 吉里吉里 (KiriKiri SDL2)

| ID | 作品 | 結果 | メモ |
|---|---|---|---|
| b2006_akabeesoft | [その横顔を見つめてしまう ～a profile完全版～ web体験版](http://suezou.dyndns.org/dl2006/akabeesoft/sonoyoko/sonoyoko_trial.zip) | 本編 | 検証スレッドでも Chromium で本編の文章まで確認。2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: dc42573 でタイトル画面まで表示 |
| b2006_mille_feuille | [ソウルレイザー☆ユナ 体験版](http://suezou.dyndns.org/dl2006/mille-feuille/yuna/yuna_taiken.zip) | 本編 | 2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: 2026-10-26: InstallShield の MSI/CAB を展開してファイル名を復元。タイトルを確認 |
| b2006_ninetail | [機械仕掛けのイブ　～Ｄｅａ　Ｅｘ　Ｍａｃｈｉｎａ～](http://suezou.dyndns.org/dl2006/ninetail/eve/0527_eve_trial_ver100.exe) | 本編 | 検証スレッドでも Chromium で本編の文章まで確認。2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: 5d1348e でタイトル画面まで表示 |
| b2007_akabeesoft | [g_web_taiken.zip](http://suezou.dyndns.org/dl2007/akabeesoft/gsen/g_web_taiken.zip) | 本編 | 検証スレッドでも Chromium で本編の文章まで確認。2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: 2026-10-26: G線上の魔王の最初の体験版。開発機のドライブパスを無視してタイトルを確認 |
| b2008_akabeesoft | [ＷＬＯ-世界恋愛機構- web用体験版](http://suezou.dyndns.org/dl2008/akabeesoft/wlo/wlo_webtrial.zip) | 本編 | 検証スレッドでも Chromium で本編の文章まで確認。2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: d40622e で注意書き・ロゴをクリックで送るとタイトル画面まで表示 |
| b2008_asa_project | [Hime のち Honey - 体験版](http://suezou.dyndns.org/dl2008/asa-project/himehoney/hime_web_trial1.zip) | 本編 | 検証スレッドでも Chromium で本編の文章まで確認。2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: b9e72ec でロゴと注意書き 2 枚をクリックで送るとタイトル画面まで表示 |
| b2008_dualtail | [Venus Blood Chimera](http://suezou.dyndns.org/dl2008/dualtail/vbc/VBC_trial_SLGver100.zip) | 本編 | 検証スレッドでも Chromium で本編の文章まで確認。2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: e7d5b88 でタイトル画面まで表示 |
| b2008_mille_feuille | [魔法少女沙枝 Vol.2 体験版](http://suezou.dyndns.org/dl2008/mille-feuille/sae2/sae2_trial2.zip) | 本編 | 検証スレッドでも Chromium で本編の文章まで確認。2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: 2026-10-26: インストーラの data1.cab を展開し、タイトルを確認 |
| b2009_dualtail | [魔法少女ラヴィリオン 体験版](http://suezou.dyndns.org/dl2009/dualtail/ravilion/ravi_trial.exe) | 本編 | 検証スレッドでも Chromium で本編の文章まで確認。2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: dc42573 でタイトル画面まで表示 (クリックで設定画面も開く) |
| b2010_akabeesoft | [置き場がない！ web体験版](http://suezou.dyndns.org/dl2010/akabeesoft/okiba/okiba_webTRIAL.zip) | 本編 | 検証スレッドでも Chromium で本編の文章まで確認。2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: d084599 (setLayerPos) でタイトル画面まで表示 |
| b2010_grand_cru | [Reunion～３日間だけのプチ同棲～　体験版](http://suezou.dyndns.org/dl2010/grand-cru/reunion/Reunion_trial.lzh) | 本編 | a29c215 で Start から本編テキストまで |
| b2011_grand_cru | [はっぴぃふぁくとりー　体験版](http://suezou.dyndns.org/dl2011/grand-cru/hf/hp_trial.zip) | 本編 | 検証スレッドでも Chromium で本編の文章まで確認。2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: 591d0a0 で黒地にタイトルのメニュー文字だけ出る |
| b2011_nanawind | [ユユカナ 体験版](http://suezou.dyndns.org/dl2011/nanawind/yuyukana/yuyu_taiken01.zip) | 本編 | 検証スレッドでも Chromium で本編の文章まで確認。2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: 2026-10-26: yuyukana.tpm の cxdec を自動検出。保護対象4,410ファイルの Adler-32 が一致 |
| b2012_akabeesoft3 | [JOKER-死線の果ての道化師-](http://suezou.dyndns.org/dl2012/akabeesoft3/joker/joker_web_trial2.zip) | 本編 | 検証スレッドでも Chromium で本編の文章まで確認。2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: d40622e で注意書き・ロゴをクリックで送るとタイトル画面まで表示 |
| b2012_alcot | [中の人などいない！　体験版](http://suezou.dyndns.org/dl2012/alcot/naka/nakanohito_trial.exe) | 本編 | 検証スレッドでも Chromium で本編の文章まで確認。2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: bd7853c でタイトル表示 (GAME START 等のメニュー描画まで) |
| b2012_cross_over | [美脚隷嬢優奈「脚もお尻もこんなに穢すなんて……絶対に許さない！」体験版](http://suezou.dyndns.org/dl2012/cross-over/yuna/br_yuuna_trial2.zip) | 本編 | 検証スレッドでも Chromium で本編の文章まで確認。2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: e7d5b88 でタイトル画面まで表示 |
| b2012_kai | [リヴォルバーガール☆ハンマーレディ　体験版](http://suezou.dyndns.org/dl2012/kai/rghl/RGHLtrial-WEB.zip) | 本編 | 検証スレッドでも Chromium で本編の文章まで確認。2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: bd7853c でタイトル表示 (START/LOAD 等の看板メニュー描画まで) |
| b2012_parasol | [恋妹SWEET☆DAYS 体験版](http://suezou.dyndns.org/dl2012/parasol/koiimo/koiimo_taiken.zip) | 本編 | 検証スレッドでも Chromium で本編の文章まで確認。2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: 5d1348e でタイトル画面まで表示。クリック後に黒画面 |
| b2013_cross_over | [花嫁狩凌 ～絶対快楽の白濁淫毒～ 体験版](http://suezou.dyndns.org/dl2013/cross-over/hana/hanayome_trial.zip) | 本編 | 検証スレッドでも Chromium で本編の文章まで確認。2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: 591d0a0 でタイトルまで |
| b2013_dualtail | [VenusBlood-GAIA- WEB専用ADV体験版](http://suezou.dyndns.org/dl2013/dualtail/vbg/VBG_WEB_AVGTrial.zip) | 本編 | 検証スレッドでも Chromium で本編の文章まで確認。2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: d084599 (setLayerPos) でタイトル画面まで表示 |
| b2013_hulotte | [妹のおかげでモテすぎてヤバい。 体験版](http://suezou.dyndns.org/dl2013/hulotte/moteyaba/moteyaba_trial.zip) | 本編 | 検証スレッドでも Chromium で本編の文章まで確認。2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: 591d0a0 で注意画面からタイトルまで (XOR 暗号の xp3) |
| b2013_madosoft | [ナマイキデレーション体験版 WEB Ver.](http://suezou.dyndns.org/dl2013/madosoft/namadere/namadere_trial_web.zip) | 本編 | 検証スレッドでも Chromium で本編の文章まで確認。2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: 591d0a0 でタイトルまで |
| b2014_hibiki_works | [ＰＲＥＴＴＹ×Ｃ∧ＴＩＯＮ](http://suezou.dyndns.org/dl2014/hibiki-works/pretty_cation/pc_web-trial1.01.zip) | 本編 | 検証スレッドでも Chromium で本編の文章まで確認。2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: a29c215 でロゴと注意書き 2 枚をクリックで送るとタイトル画面まで表示 |
| b2014_undermoon | [奪われた学園～罠に落ちた優等生～　体験版](http://suezou.dyndns.org/dl2014/undermoon/ubaware/ubaware_trial.zip) | 本編 | 2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: 5d1348e でタイトル画面まで表示 |
| b2015_ammolite | [お兄ちゃん、教えて……～義妹とヒミツの保健実習～　体験版　Ver1.00](http://suezou.dyndns.org/dl2015/ammolite/oni/amml011_tr.zip) | 本編 | 検証スレッドでも Chromium で本編の文章まで確認。2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: 5d1348e でタイトル画面まで表示 |
| b2015_appetite | [愛娘は、マゾ性隷～娘だからって遠慮しないでね？～　体験版　Ver1.00](http://suezou.dyndns.org/dl2015/appetite/musume/apt054_tr.zip) | 本編 | 検証スレッドでも Chromium で本編の文章まで確認。2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: 5d1348e でタイトル画面まで表示 |
| b2015_mille_feuille | [極煌戦姫ミストルティア 体験版](http://suezou.dyndns.org/dl2015/mille-feuille/mistletear/blazingangel_mistletear_trial.zip) | 本編 | 検証スレッドでも Chromium で本編の文章まで確認。2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: b9e72ec でタイトル画面まで表示 |
| b2015_nanawind | [ALIA’s CARNIVAL! Flowering Sky　体験版(WEB.ver)](http://suezou.dyndns.org/dl2015/nanawind/alias_fs/ALIAs_CARNIVAL_floweringsky_trial.zip) | 本編 | 検証スレッドでも Chromium で本編の文章まで確認。2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: cf28cf7 (開発サーバーの接続修正) 後は単独3回ともタイトル画面まで。Start 後の選択肢ボタンの文字が出ない件は未確認 |
| b2015_pachipachi_soft | [おしおき生意気ギャル ～エロいオシオキをしたら自分からエッチを求めてきた～ 体験版](http://suezou.dyndns.org/dl2015/pachipachi-soft/oshioki/oshioki_trial.zip) | 本編 | 2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: 2026-10-26: Mado の31バイト周期 XOR を自動検出。保護対象749ファイルの Adler-32 が一致 |
| b2016_alcot | [よめがみ My Sweet Goddess! リコＨシーン体験版](http://suezou.dyndns.org/dl2016/alcot/yomegami/yomegami_trial3.exe) | 本編 | 2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: ae3efe1 でタイトル画面まで表示 (起動から約 2 分) |
| b2016_apadash | [満淫オーク電車～くっ！　任務じゃなければこんな場所…～　体験版　Ver1.00](http://suezou.dyndns.org/dl2016/apadash/oak_train/aptd005_tr.zip) | 本編 | 検証スレッドでも Chromium で本編の文章まで確認。2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: 5d1348e でタイトル画面まで表示 |
| b2016_appetite | [アナル＊ティーチャー～感度良肛！？　逆レッスン～　体験版　Ver1.00](http://suezou.dyndns.org/dl2016/appetite/anaru_teacher/apt068_tr.zip) | 本編 | 検証スレッドでも Chromium で本編の文章まで確認。2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: 5d1348e でタイトル画面まで表示 |
| b2016_cube | [間宮くんちの五つ子事情 体験版](http://suezou.dyndns.org/dl2016/cube/mamiya/mamiya_trial.zip) | 本編 | 検証スレッドでも Chromium で本編の文章まで確認。2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: 0c62865 で白画面からクリック 2 回でタイトル画面まで表示 |
| b2017_appetite | [催眠アプリで寝取りハメ～これまでの恨み、ヤり返してやる！　体験版　Ver1.00](http://suezou.dyndns.org/dl2017/appetite/hypnotism_application/apt098_tr.zip) | 本編 | 検証スレッドでも Chromium で本編の文章まで確認。2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: 5d1348e でタイトル画面まで表示 |
| b2017_sukerasparo | [ことのは・アムリラート](http://suezou.dyndns.org/dl2017/sukerasparo/kotonoha/kotonoha_trial.zip) | 本編 | 2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: 2026-10-26: EXE 内蔵 XP3 をプロジェクトとして選び、タイトルを確認 |
| b2018_appetite | [P.S.あなたを心から愛しています～最期まで気づけなかった妻の苦しみ～　体験版](http://suezou.dyndns.org/dl2018/appetite/ps_iloveyou/apt125_tr.zip) | 本編 | 検証スレッドでも Chromium で本編の文章まで確認。2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: 5d1348e でタイトル画面まで表示 |
| b2019_appetite | [女体化パニック！３～お、俺は男だ！　でも、悔しいけど気持ちいいっ……のか！？　体験版](http://suezou.dyndns.org/dl2019/appetite/nyotaika3/apt133_tr.zip) | 本編 | 検証スレッドでも Chromium で本編の文章まで確認。2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: 5d1348e でタイトル画面まで表示 |
| b2019_dualtail | [VenusBlood FRONTIER International SLG Trial 日本語版](http://suezou.dyndns.org/dl2019/dualtail/vbfi/VBFI_New_SLG_Trial.zip) | 本編 | 検証スレッドでも Chromium で本編の文章まで確認。2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: 5d1348e でタイトル画面まで表示 |
| b2019_sator | [モウソウスピーカー～Ｈな妄想が聴こえてくるよ～　体験版](http://suezou.dyndns.org/dl2019/sator/mousou_speaker/mousou_tr.zip) | 本編 | 2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: 5d1348e でタイトル画面まで表示 |
| b2019_tinkle_position | [おにギュＨ＿ＨＣＢ体験版](http://suezou.dyndns.org/dl2019/tinkle-position/onigyu_h/onigyuh_hcbtrial.zip) | 本編 | 1929e7a で「はじめる」からハグクリックの説明画面まで |
| b2020_akabeesoft3 | [墓多 DYING ZOMBIES -体験版-](http://suezou.dyndns.org/dl2020/akabeesoft3/hakatadyingzombies/ab3_hakatadyingzombies_webtrial.zip) | 本編 | 検証スレッドでも Chromium で本編の文章まで確認。2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: 0009: Akabei 方式を自動復号し、Chromium でタイトルまで確認 (2026-10-26)。保護対象 4,201 ファイルの Adler-32 一致 |
| b2020_at_sakura | [優来（ゆら）があいつに抱かれる理由（わけ） 体験版](http://suezou.dyndns.org/dl2020/at-sakura/yura_wake/aspc-0112_taiken_ver1.0.zip) | 本編 | 検証スレッドでも Chromium で本編の文章まで確認。2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: 5d1348e でタイトル画面まで表示 |
| b2020_chaos_r | [悪堕ち 魔導処女～絶対支配の触手調教～　体験版](http://suezou.dyndns.org/dl2020/chaos-r/madousyojo/madousyojo_tr.zip) | 本編 | 検証スレッドでも Chromium で本編の文章まで確認。2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: 5d1348e でタイトル画面まで表示 |
| b2020_lass_pixy | [リトルシックガール　～桃蜜は妹の香り～　体験版](http://suezou.dyndns.org/dl2020/lass-pixy/lsg03/LSG3_tr.zip) | 本編 | 検証スレッドでも Chromium で本編の文章まで確認。2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: bd7853c でタイトル表示 (Start/Continue 等のボタン描画まで) |
| b2020_pin_point | [世話焼き奥さんで人の頼みを断れない亜衣さんにお願いして中出しハメ放題のドスケベ妻になってもらった 体験版](http://suezou.dyndns.org/dl2020/pin-point/wife_ai/PP34_trial.zip) | 本編 | 検証スレッドでも Chromium で本編の文章まで確認。2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: 5d1348e でタイトル画面まで表示。起動時に「フルスクリーンで表示しますか？」の確認ダイアログが出る |
| b2021_at_sakura | [愛する彼女は画面の中、他の男の上で腰をふる 体験版](http://suezou.dyndns.org/dl2021/at-sakura/ai_gamen/aspc-0123_taiken_ver1.0.zip) | 本編 | 検証スレッドでも Chromium で本編の文章まで確認。2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: 591d0a0 でロゴからタイトルまで。色も正しい |
| b2021_chaos_r | [神聖昂燐ダクリュオン～正しい天使の育てかた～　体験版](http://suezou.dyndns.org/dl2021/chaos-r/dakr/dakr_tr.zip) | 本編 | 検証スレッドでも Chromium で本編の文章まで確認。2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: 5d1348e でタイトル画面まで表示 |
| b2021_kelp | [アイドル姉妹をエッチにプロデュース！ 体験版](http://suezou.dyndns.org/dl2021/kelp/idol_simai/IDOL_SIMAI_TR.zip) | 本編 | 検証スレッドでも Chromium で本編の文章まで確認。2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: 5d1348e でタイトル画面まで表示 |
| b2021_pin_point | [売国王姫～堕落のメス豚母娘～体験版](http://suezou.dyndns.org/dl2021/pin-point/baikoku_ouhi/PP36_trial.zip) | 本編 | 検証スレッドでも Chromium で本編の文章まで確認。2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: 5d1348e でタイトル画面まで表示。起動時に「フルスクリーンで表示しますか？」の確認ダイアログが出る |
| b2022_at_sakura | [愛する恋人を大嫌いな旧友に寝取られた件 体験版](http://suezou.dyndns.org/dl2022/at-sakura/ai_tomo/aspc-0125_taiken_ver1.0.zip) | 本編 | 検証スレッドでも Chromium で本編の文章まで確認。2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: 591d0a0 でタイトルまで (b2021_at_sakura と同じ挙動) |
| b2022_chaos_r | [進軍ゴブリン軍団～人類総苗床化作戦～ 体験版](http://suezou.dyndns.org/dl2022/chaos-r/gobblin_gundan/GOBBLIN_TR.zip) | 本編 | 検証スレッドでも Chromium で本編の文章まで確認。2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: 5d1348e でタイトル画面まで表示 |
| b2022_hending | [リンパにATATA! ～メス牡蠣ミルクどぴゅらっしゅ♡～](http://suezou.dyndns.org/dl2022/hending/atata/atata_trial.zip) | 本編 | 2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: 0009: Akabei 方式の鍵を自動検出し、Chromium でタイトルまで確認 (2026-10-26)。保護対象 2,372 ファイルの Adler-32 一致 |
| b2022_syoku | [触災の島～Island of the Dead～体験版　Ver1.00](http://suezou.dyndns.org/dl2022/syoku/syokusai_island/Syokusai_Island_tr.zip) | 本編 | 2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: 5d1348e でタイトル画面まで表示 |
| b2023_apadash | [最強の人妻ヒロインが寝取られるまで～お願い！夫にはバラさないで…～　体験版　Ver1.00](http://suezou.dyndns.org/dl2023/apadash/hitoduma_heroine/aptd027_tr.zip) | 本編 | 検証スレッドでも Chromium で本編の文章まで確認。2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: 5d1348e でタイトル画面まで表示 |
| b2023_appetite | [洗脳航路～エイリアンの苗床に導かれるエリート女軍人たち～　体験版　Ver1.00](http://suezou.dyndns.org/dl2023/appetite/senno_kouro/apt246_tr.zip) | 本編 | 検証スレッドでも Chromium で本編の文章まで確認。2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: 5d1348e でタイトル画面まで表示 |
| b2023_at_sakura | [堕とされた義姉 体験版](http://suezou.dyndns.org/dl2023/at-sakura/oti_ane/aspc-0138_taiken_ver1.0.zip) | 本編 | 検証スレッドでも Chromium で本編の文章まで確認。2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: 5d1348e でタイトル画面まで表示 |
| b2023_chaos_r | [神聖昂燐エストランジェ体験版](http://suezou.dyndns.org/dl2023/chaos-r/strncxe/strncxe_tr.zip) | 本編 | 検証スレッドでも Chromium で本編の文章まで確認。2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: 5d1348e でタイトル画面まで表示 |
| b2023_lump_of_sugar_all | [瑠璃桜](http://suezou.dyndns.org/dl2023/lump-of-sugar_all/rurizakura/Rurizakura_trial.zip) | 本編 | 2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: dc42573 でタイトル画面まで表示 |
| b2024_at_sakura | [性調教済だった愛する彼女 体験版](http://suezou.dyndns.org/dl2024/at-sakura/seikyo_kano/aspc-0150_taiken_ver1.0.zip) | 本編 | 検証スレッドでも Chromium で本編の文章まで確認。2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: 591d0a0 でタイトルまで (b2021_at_sakura と同じ挙動) |
| b2025_at_sakura | [学生会長・紫藤喜那の淫鬱 体験版](http://suezou.dyndns.org/dl2025/at-sakura/seito_sidou/aspc-0161_taiken_ver1.0.zip) | 本編 | 検証スレッドでも Chromium で本編の文章まで確認。2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: 5d1348e でタイトル画面まで表示 |
| b2025_monoceros_plus | [Vanilla Android　シコ猿DT大学生の俺と美少女アンドロイドがいろんなプレイでハメパコミッション　体験版](http://suezou.dyndns.org/dl2025/monoceros_plus/vanilla_android/VanillaAndroid-trial.zip) | 本編 | 検証スレッドでも Chromium で本編の文章まで確認。2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: 5d1348e でタイトル画面まで表示 |
| dl_RJ01677912 | [とける風花とシロうさぎ](https://trial.dlsite.com/doujin/RJ01678000/RJ01677912_trial.zip) | 本編 | 1929e7a で START から本編テキストまで (立ち絵・名前欄・文章) |
| r1_kag3 | [KAG3 紹介デモ](https://krkrz.github.io/download/kr2_232r2.zip) | 本編 | 2026-10-26: 公式 syoukai デモ。Pad で Config を編集・保存し、再起動後に読み込み、紹介本文まで確認 |
| r1_krkr_apt306 | [先輩、好きです……押忍！ 体験版](https://www.appetite-game.com/apt306/image/apt306_tr.zip) | 本編 | 2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: e7d5b88 でタイトル画面まで表示 |
| s2006_akabeesoft_conboku | [こんな娘がいたら僕はもう～web体験版第2弾～](http://suezou.dyndns.org/dl2006/akabeesoft/conboku/conboku_taiken_02.zip) | 本編 | 検証スレッドでも Chromium で本編の文章まで確認。2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: bd7853c でインストーラ (Inno) 展開後に注意書き→タイトルメニューまで表示 |
| s2008_akabeesoft_gsen | [G線上の魔王](http://suezou.dyndns.org/dl2008/akabeesoft/gsen/g_trial2.zip) | 本編 | 検証スレッドでも Chromium で本編の文章まで確認。2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: 2026-10-26: 体験版2。開発機のドライブパスを無視してタイトルを確認 |
| s2008_dualtail_clarice | [奴隷将校クラリス　－　白濁のグローリエ　－　ＡＤＶパート体験版](http://suezou.dyndns.org/dl2008/dualtail/clarice/Clarice_ADVtrial.exe) | 本編 | 検証スレッドでも Chromium で本編の文章まで確認。2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: e7d5b88 でタイトル画面まで表示 |
| s2008_ninetail_amatu | [天ツ風　～傀儡陣風帖～](http://suezou.dyndns.org/dl2008/ninetail/amatu/amatu_trial_0901ver100.exe) | 本編 | 2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt) |
| s2009_dualtail_haiironosora | [灰色の空に堕ちた翼　体験版 Ver.1.00](http://suezou.dyndns.org/dl2009/dualtail/haiironosora/TD06_TRIAL100.exe) | 本編 | 検証スレッドでも Chromium で本編の文章まで確認。2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: 5d1348e でタイトル画面まで表示 |
| s2010_akabeesoft_lavender | [光輪の町、ラベンダーの少女　体験版](http://suezou.dyndns.org/dl2010/akabeesoft/lavender/lavender_webTRIAL.zip) | 本編 | 検証スレッドでも Chromium で本編の文章まで確認。2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: 2026-10-26: 体験版の cxdec を自動検出。保護対象6,030ファイルの Adler-32 が一致 |
| s2011_grand_cru_imostar | [しゅ～てぃんぐ妹スター☆ ～おにいちゃん、覚悟しててよね♪～　体験版](http://suezou.dyndns.org/dl2011/grand-cru/imostar/imosta_trial.zip) | 本編 | 2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: d2edfde でタイトル画面まで表示 |
| s2011_hulotte_with_ribbon | [With Ribbon Hシーン体験版](http://suezou.dyndns.org/dl2011/hulotte/with-ribbon/with-ribbon_trial2.zip) | 本編 | 検証スレッドでも Chromium で本編の文章まで確認。2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: d2edfde で注意書きの後クリックでタイトル画面まで表示 (OP ムービーは飛ばす) |
| s2011_parasol_delivara | [でりばらっ！-deliverance of strays- 体験版](http://suezou.dyndns.org/dl2011/parasol/delivara/delivara_taiken.zip) | 本編 | 2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: 5d1348e でタイトル画面まで表示 |
| s2012_ninetail_gears_of_dragoon | [Gears of Dragoon  -RPG体験版 ver.1.00-](http://suezou.dyndns.org/dl2012/ninetail/gears_of_dragoon/GoD_RPG_trial_100.zip) | 本編 | 2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: e2e9bbf + cf28cf7 で完全な data.xp3 (657,587,992 バイト) からタイトル画面まで |
| s2013_hibiki_works_lovely_cation2 | [ＬＯＶＥＬＹ×Ｃ∧ＴＩＯＮ２](http://suezou.dyndns.org/dl2013/hibiki-works/lovely_cation2/LxC2_trial_web.zip) | 本編 | 2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: a29c215 でロゴと注意書き 2 枚をクリックで送るとタイトル画面まで表示 |
| s2014_cross_over_tori | [鳥籠の姫 ～絶対妊娠の姉妹悦辱～　体験版](http://suezou.dyndns.org/dl2014/cross-over/tori/torikago_trial1_2.zip) | 本編 | 検証スレッドでも Chromium で本編の文章まで確認。2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: 5d1348e でタイトル画面まで表示 |
| s2014_cube_koikano | [恋する彼女の不器用な舞台 体験版](http://suezou.dyndns.org/dl2014/cube/koikano/koikanoTrial.zip) | 本編 | 2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: d2edfde でクリック 3〜4 回の後タイトル画面まで表示 |
| s2015_appetite_inbo | [発情淫母～疼く熟体は、その欲情を抑えきれない～　体験版　Ver1.00](http://suezou.dyndns.org/dl2015/appetite/inbo/apt050_tr.zip) | 本編 | 検証スレッドでも Chromium で本編の文章まで確認。2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: 5d1348e でタイトル画面まで表示 |
| s2015_hulotte_yomeyaba | [嫁探しが捗りすぎてヤバい。 体験版](http://suezou.dyndns.org/dl2015/hulotte/yomeyaba/yomeyaba_trial.zip) | 本編 | 検証スレッドでも Chromium で本編の文章まで確認。2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: 5d1348e でタイトル画面まで表示 |
| s2016_akabeesoft3_inosupe | [生命のスペア](http://suezou.dyndns.org/dl2016/akabeesoft3/inosupe/inosupe_webtrialver.zip) | 本編 | 検証スレッドでも Chromium で本編の文章まで確認。2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: 2026-10-26: data.bin と TLG のみのアーカイブを検出。タイトルメニューを確認 |
| s2016_ammolite_little_twins | [りとる☆ついんず　～すぐにトリコにしてあげる♪～　体験版　Ver1.00](http://suezou.dyndns.org/dl2016/ammolite/little_twins/amml012_tr.zip) | 本編 | 2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: 5d1348e でタイトル画面まで表示 |
| s2016_appetite_husband | [夫に言えない義父との姦淫～嫌なのに忘れられなくなりそう～　体験版　Ver1.00](http://suezou.dyndns.org/dl2016/appetite/husband/apt073_tr.zip) | 本編 | 検証スレッドでも Chromium で本編の文章まで確認。2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: 5d1348e でタイトル画面まで表示 |
| s2016_appetite_wawatasi | [わ、わたしを縛りたい？～僕と地味娘の秘密の関係～　体験版　Ver1.00](http://suezou.dyndns.org/dl2016/appetite/wawatasi/apt081_tr.zip) | 本編 | 検証スレッドでも Chromium で本編の文章まで確認。2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: 5d1348e でタイトル画面まで表示 |
| s2017_akabeesoft3_tonofuta | [少女と年の差、ふたまわり。](http://suezou.dyndns.org/dl2017/akabeesoft3/tonofuta/syoujyo_to_toshinosa_hutamawari_webtrial.zip) | 本編 | 検証スレッドでも Chromium で本編の文章まで確認。2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: 2026-10-26: data.bin の検出と引用符なしの末尾 cond 式に対応し、タイトルメニューを確認 |
| s2017_appetite_evil_monmusu | [ゆけゆけ悪のモンむす軍団！～怪人娘に搾り取られるえっちで過酷な世界性服大作戦～　体験版　Ver1.00](http://suezou.dyndns.org/dl2017/appetite/evil_monmusu/apt101_tr.zip) | 本編 | 検証スレッドでも Chromium で本編の文章まで確認。2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: d51f3e8 系のページでタイトル画面まで表示 |
| s2017_appetite_immorality_don | [背徳の母娘丼～淫欲に沈むママといもうと～　体験版　Ver1.00](http://suezou.dyndns.org/dl2017/appetite/immorality_don/apt090_tr.zip) | 本編 | 検証スレッドでも Chromium で本編の文章まで確認。2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: 5d1348e でタイトル画面まで表示 |
| s2017_atelier_gh_neneki | [粘液地獄　～巨大ヌルヌル生物に汚された女学生～ 体験版](http://suezou.dyndns.org/dl2017/atelier-gh/neneki/neneki_TR.zip) | 本編 | 2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: 5d1348e でタイトル画面まで表示 |
| s2017_nanawind_alice_gram | [春音アリス＊グラム　体験版](http://suezou.dyndns.org/dl2017/nanawind/alice_gram/harugra_trial.zip) | 本編 | a29c215 で STORY から本編テキストまで。cf28cf7 (開発サーバーの接続修正) 後は単独3回とも起動 (以前の「読み込み中」は開発サーバー側の不具合) |
| s2018_appetite_boku_mother | [欲求不満なボクのママのＳＥＸレッスン～パパには言えない家庭内情事～　Trial](http://suezou.dyndns.org/dl2018/appetite/boku_mother/apt106_tr.zip) | 本編 | 検証スレッドでも Chromium で本編の文章まで確認。2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: d51f3e8 系のページでタイトル画面まで表示 |
| s2018_appetite_shokushu_ouji | [触手王子の肉嫁探し～母と姉弟の終わらない絶頂地獄～　体験版](http://suezou.dyndns.org/dl2018/appetite/shokushu_ouji/apt116_tr.zip) | 本編 | 検証スレッドでも Chromium で本編の文章まで確認。2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: 5d1348e でタイトル画面まで表示 |
| s2018_pin_point_yakyurare | [彼女が野球部の性処理マネージャーに…～体育会系セックスで寝取られて～体験版](http://suezou.dyndns.org/dl2018/pin-point/yakyurare/yakyurare_trial.zip) | 本編 | 検証スレッドでも Chromium で本編の文章まで確認。2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: 5d1348e でタイトル画面まで表示。起動時に「フルスクリーンで表示しますか？」の確認ダイアログが出る |
| s2019_apadash_chuni_saimin | [厨二病が催眠術を使えたら～勘違い転校生のエッチな悪神召喚術～　体験版　Ver1.00](http://suezou.dyndns.org/dl2019/apadash/chuni_saimin/aptd015_tr.zip) | 本編 | 検証スレッドでも Chromium で本編の文章まで確認。2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: d51f3e8 系のページでタイトル画面まで表示 |
| s2019_appetite_aniyome_furin | [兄嫁に誘われて～義姉がエロ過ぎて家庭内不倫～　体験版　Ver1.00](http://suezou.dyndns.org/dl2019/appetite/aniyome_furin/apt144_tr.zip) | 本編 | 検証スレッドでも Chromium で本編の文章まで確認。2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: 5d1348e でタイトル画面まで表示 |
| s2019_appetite_dt_kyoiku | [[2019/05/08 01][mode_title.ks - 164]ママの妹の童貞教育～ひきこもった僕は外出せずに中出しする～　体験版](http://suezou.dyndns.org/dl2019/appetite/dt_kyoiku/apt148_tr.zip) | 本編 | 検証スレッドでも Chromium で本編の文章まで確認。2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: 5d1348e でタイトル画面まで表示 |
| s2019_sukerasparo_itsukano | [いつかのメモラージョよりぬき体験版](http://suezou.dyndns.org/dl2019/sukerasparo/itsukano/itsukano_trial2.zip) | 本編 | 2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: 5d1348e でタイトル画面まで表示 |
| s2020_at_sakura_lover_swapping | [恋人スワッピング 体験版](http://suezou.dyndns.org/dl2020/at-sakura/lover_swapping/aspc-0107_taiken_ver1.0.zip) | 本編 | 検証スレッドでも Chromium で本編の文章まで確認。2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: 5d1348e でタイトル画面まで表示 |
| s2020_monoceros_plus_kanosen | [ボクのカノジョはえっちな先生　体験版](http://suezou.dyndns.org/dl2020/monoceros_plus/kanosen/kanosen_TR.zip) | 本編 | 2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: 5d1348e でタイトル画面まで表示 |
| s2020_pajamas_ex_paizuri | [最高に都合のいいパイズリ上手のマリーさん－体験版－](http://suezou.dyndns.org/dl2020/pajamas-ex/paizuri/paizuri_tr.zip) | 本編 | 2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: 5d1348e でタイトル画面まで表示 |
| s2020_sagaplanets_kakenuke | [かけぬけ★青春スパーキング！ 体験版](http://suezou.dyndns.org/dl2020/sagaplanets/kakenuke/kakenuke_trial.zip) | 本編 | 2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: d2edfde でクリック後タイトル画面まで表示 (Mojimo フォントは使わずページのフォント) |
| s2021_akabeesoft2_roleplayer | [ROLEPLAYER：小粥姉妹の粘膜ポトレ ぐりぐちゃLIVE！](http://suezou.dyndns.org/dl2021/akabeesoft2/roleplayer/ab2_roleplayer_okayushimai_webtrial.zip) | 本編 | 検証スレッドでも Chromium で本編の文章まで確認。2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: 0009: Akabei 方式を自動復号し、Chromium でタイトルまで確認 (2026-10-26)。保護対象 6,195 ファイルの Adler-32 一致 |
| s2021_at_sakura_kodane | [親友に子種を注がれる俺の妻 体験版](http://suezou.dyndns.org/dl2021/at-sakura/kodane/aspc-0121_taiken_ver1.0.zip) | 本編 | 検証スレッドでも Chromium で本編の文章まで確認。2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: d51f3e8 系のページでタイトル画面まで表示 |
| s2021_at_sakura_tuma_10 | [愛する妻と１０人の過去男 体験版](http://suezou.dyndns.org/dl2021/at-sakura/tuma_10/aspc-0117_taiken_ver1.0.zip) | 本編 | 検証スレッドでも Chromium で本編の文章まで確認。2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: 2026-10-26: XP3 が data.xp3 以外の配置でも起動し、タイトルを確認 |
| s2021_dualtail_vbhi | [VenusBlood HOLLOW International SRPG Trial ENG Ver.](http://suezou.dyndns.org/dl2021/dualtail/vbhi/VBHI_SRPGTrial_EN.zip) | 本編 | 検証スレッドでも Chromium で本編の文章まで確認。2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: d51f3e8 系のページでタイトル画面まで表示。クリック後に「RuntimeError: null function」で停止 |
| s2021_nanawind_kanade_gram_ch1 | [花鐘カナデ＊グラム Chapter1 -小桜結- 体験版](http://suezou.dyndns.org/dl2021/nanawind/kanade-gram_ch1/kanade-gram_ch1-trial.zip) | 本編 | 検証スレッドでも Chromium で本編の文章まで確認。2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: 2026-10-27: EXE の bres リソースから cxdec を自動準備。data.xp3 の保護対象1,955ファイルが一致。PIMG の表示状態を初期化し、背景・キャラクター・メニュー・花びらを表示 |
| s2021_no_strike_Trap_Genesis | [Jyosou Souseiki Trial Ver.](http://suezou.dyndns.org/dl2021/no-strike/Trap_Genesis/Jyosou_Souseiki_Trial.zip) | 本編 | 検証スレッドでも Chromium で本編の文章まで確認。2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: 2026-10-26: 平文スクリプトの誤判定を修正。タイトルメニューを確認 |
| s2022_akabeesoft2_roleplayer_tororo | [ROLEPLAYER：とろろ姉妹の粘膜ポトレ ぐりぐちゃLIVE！](http://suezou.dyndns.org/dl2022/akabeesoft2/roleplayer_tororo/ab2_roleplayer_tororoshimai_webtrial.zip) | 本編 | 検証スレッドでも Chromium で本編の文章まで確認。2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: 0009: Akabei 方式を自動復号し、Chromium でタイトルまで確認 (2026-10-26)。保護対象 5,892 ファイルの Adler-32 一致 |
| s2022_at_sakura_konya_aitu | [今夜もあいつに抱かれる彼女 体験版](http://suezou.dyndns.org/dl2022/at-sakura/konya_aitu/aspc-0129_taiken_ver1.0.zip) | 本編 | 検証スレッドでも Chromium で本編の文章まで確認。2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: 5d1348e でタイトル画面まで表示 |
| s2022_at_sakura_ntr_tuma_miina | [ネトラセ妻、美衣奈 体験版](http://suezou.dyndns.org/dl2022/at-sakura/ntr_tuma-miina/aspc-0132_taiken_ver1.0.zip) | 本編 | 検証スレッドでも Chromium で本編の文章まで確認。2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: 5d1348e でタイトル画面まで表示 |
| s2022_mille_feuille_excelchiffon | [燦然可憐エクセルシフォン 体験版](http://suezou.dyndns.org/dl2022/mille-feuille/excelchiffon/excelchiffon_trial.zip) | 本編 | 検証スレッドでも Chromium で本編の文章まで確認。2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: b9e72ec でタイトル画面まで表示 (ウィンドウより大きいレイヤの修正) |
| s2023_chaos_r_bian_rr | [魔子宮聖女ビアンカ∽ノエイン～終焉なき破瓜と悦楽の煉獄～　体験版](http://suezou.dyndns.org/dl2023/chaos-r/bian_rr/bianRR_TR.zip) | 本編 | 検証スレッドでも Chromium で本編の文章まで確認。2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: 5d1348e でタイトル画面まで表示 |
| s2023_cross_over_isekai_ma | [異世界行ってマッサージで生き残ろうとしたら美脚美女とイチャ×2ライフ～転移ボーナスが美味しすぎた件～体験版](http://suezou.dyndns.org/dl2023/cross-over/isekai_ma/isekai_ma_taiken.zip) | 本編 | 検証スレッドでも Chromium で本編の文章まで確認。2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: 5d1348e でタイトル画面まで表示 |
| s2023_hending_hassyakusama | [ソープランドのはっしゃくさま](http://suezou.dyndns.org/dl2023/hending/hassyakusama/hending_soapland_no_hassyakusama_webtrial.zip) | 本編 | 2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: 0009: Akabei 方式を自動復号し、Chromium でタイトルまで確認 (2026-10-26)。保護対象 3,236 ファイルの Adler-32 一致 |
| s2024_appetite_inyoku_train | [淫欲の通学電車～嫌なのにされたい私は堕ちていく～　体験版](http://suezou.dyndns.org/dl2024/appetite/inyoku_train/apt249_tr.zip) | 本編 | 検証スレッドでも Chromium で本編の文章まで確認。2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: d51f3e8 系のページでタイトル画面まで表示 |
| s2024_appetite_isekai_cyoroin | [異世界チョロインは、いとも簡単に堕とされる～あれ、ボクの彼女…寝取られちゃいました？～　体験版](http://suezou.dyndns.org/dl2024/appetite/isekai_cyoroin/apt251_tr.zip) | 本編 | 検証スレッドでも Chromium で本編の文章まで確認。2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: d51f3e8 系のページでタイトル画面まで表示 |
| s2024_at_sakura_iede_takamura | [家出少女・高邑緋雪の秘蜜 体験版](http://suezou.dyndns.org/dl2024/at-sakura/iede_takamura/aspc-0155_taiken_ver1.0.zip) | 本編 | 検証スレッドでも Chromium で本編の文章まで確認。2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: 5d1348e でタイトル画面まで表示 |
| s2024_no_strike_yuribatake | [Jyosou Yuribatake Trial](http://suezou.dyndns.org/dl2024/no-strike/yuribatake/jyosou_yuribatake_japantrial.zip) | 本編 | 検証スレッドでも Chromium で本編の文章まで確認。2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: 2026-10-27: 平文 UTF-8 の誤判定と、補助ウィンドウへの入力誤配送を修正。言語選択からタイトルメニューへの遷移を確認 |
| s2025_akabeesoft2_roleplayer_ikura | [Role player：いくら姉妹の粘膜ポトレ　ぐりぐちゃLIVE！　体験版](http://suezou.dyndns.org/dl2025/akabeesoft2/roleplayer_ikura/Role_player_ikura_trial.zip) | 本編 | 検証スレッドでも Chromium で本編の文章まで確認。2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: f99fe3f でタイトル画面まで表示 (起動から約 2 分) |
| s2025_poison_extasy_paizuri_me | [最高に都合の良いパイズリ上手のマリーさん - MOTION EDITION - 体験版](http://suezou.dyndns.org/dl2025/poison-extasy/paizuri_me/paizuri_me_tr.zip) | 本編 | 検証スレッドでも Chromium で本編の文章まで確認。2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: d40622e で注意書き・ロゴをクリックで送るとタイトル画面まで表示 |
| s2026_no_strike_jyosou_yuriarashi | [Jyosou Yuriarasi Trial](http://suezou.dyndns.org/dl2026/no-strike/jyosou_yuriarashi/jyosouyuriarashi_trial.zip) | 本編 | 検証スレッドでも Chromium で本編の文章まで確認。2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: 2026-10-27: 平文 UTF-8 の誤判定と、補助ウィンドウへの入力誤配送を修正。言語選択からタイトルメニューへの遷移を確認 |
| s2026_poison_motion_sxs2_me | [姉妹X催眠２ - MOTION EDITION - 体験版](http://suezou.dyndns.org/dl2026/poison-motion/sxs2-me/sxs2me_trial.zip) | 本編 | 検証スレッドでも Chromium で本編の文章まで確認。2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: d40622e で注意書き・ロゴをクリックで送るとタイトル画面まで表示 |
| s2026_poison_motion_sxs_me | [姉妹X催眠 - MOTION EDITION - 体験版](http://suezou.dyndns.org/dl2026/poison-motion/sxs-me/sxsme_trial.zip) | 本編 | 2026-10-28: タイトル→開始→本編の文章を数ページ送るまで Chromium で確認 (手順は vnrun-checks/kirikiri_story_steps.txt)。以前: d40622e で注意書き・ロゴをクリックで送るとタイトル画面まで表示 |
| dl_VJ01000429 | [【DLsite限定版】進軍ゴブリン軍団2 ～淫獣軍VS.INTRUDER～](https://trial.dlsite.com/professional/VJ01001000/VJ01000429_trial.zip) | タイトル | 5d1348e でタイトル画面まで表示 |
| dl_VJ01000497 | [【DLsite限定版】性感エステ 灯 -AKARI- マキ（28）](https://trial.dlsite.com/professional/VJ01001000/VJ01000497_trial.zip) | タイトル | 5d1348e でタイトル画面まで表示 |
| dl_VJ01000502 | [堕とされていくお姉ちゃんと、何もできないボク ～憧れの彼女は、淫落し痴態をさらけ出す～](https://trial.dlsite.com/professional/VJ01001000/VJ01000502_trial.zip) | タイトル | d51f3e8 系のページでタイトル画面まで表示 |
| dl_VJ01000564 | [VenusBlood GAIA International R-18版](https://trial.dlsite.com/professional/VJ01001000/VJ01000564_trial.zip) | タイトル | 5d1348e でタイトル画面まで表示 |
| dl_VJ01000639 | [【デラックス版】調教カテイ ～性開発された肢体は元カレを忘れられない～](https://trial.dlsite.com/professional/VJ01001000/VJ01000639_trial.zip) | タイトル | d40622e で注意書き・ロゴをクリックで送るとタイトル画面まで表示 |
| dl_VJ01000679 | [居座りカノジョとのドキドキ同居生活 ～ねぇねぇ、おじさんしたいでしょ♪～](https://trial.dlsite.com/professional/VJ01001000/VJ01000679_trial.zip) | タイトル | 5d1348e でタイトル画面まで表示 |
| dl_VJ01001109 | [最強の人妻ヒロインが寝取られるまで ～お願い！夫にはバラさないで…～](https://trial.dlsite.com/professional/VJ01002000/VJ01001109_trial.zip) | タイトル | 5d1348e でタイトル画面まで表示 |
| dl_VJ01002065 | [快楽セックスで堕とされていた愛する妻 ～他の男の手で身も心もイキ狂っていく最愛妻](https://trial.dlsite.com/professional/VJ01003000/VJ01002065_trial.zip) | タイトル | 5d1348e でタイトル画面まで表示 |
| dl_VJ01003070 | [夏色果実 ～えっちなカノジョと溶け合う心～](https://trial.dlsite.com/professional/VJ01004000/VJ01003070_trial.zip) | タイトル | 5d1348e でタイトル画面まで表示 |
| dl_VJ01003347 | [【DLsite限定版】NEW BREEDER ～エッチな獣娘を狩って⾃分好みに調教！～](https://trial.dlsite.com/professional/VJ01004000/VJ01003347_trial.zip) | タイトル | 5d1348e でタイトル画面まで表示 |
| dl_VJ01004136 | [早乙女結衣の淫らな隠しごと ～隠されたアイドルのビッチな裏事情～](https://trial.dlsite.com/professional/VJ01005000/VJ01004136_trial.zip) | タイトル | 5d1348e でタイトル画面まで表示 |
| dl_VJ01004598 | [女王社長の僕争奪戦 ～カラダもリードも譲らない！ で、どっちのが好きなのっ！？～](https://trial.dlsite.com/professional/VJ01005000/VJ01004598_trial.zip) | タイトル | 5d1348e でタイトル画面まで表示 |
| dl_VJ01006612 | [背徳の母乳 ～そんなに出しちゃダメぇ～](https://trial.dlsite.com/professional/VJ01007000/VJ01006612_trial.zip) | タイトル | d51f3e8 系のページでタイトル画面まで表示 |
| s2019_parasol_renran_p | [恋嵐スピリッチュ 体験版](https://trial.dlsite.com/professional/VJ013000/VJ012599_trial.zip http://suezou.dyndns.org/dl2019/parasol/renran-p/renran_trial_101.zip) | 対象外 | 2026-10-26: 配布 URL は体験版の差分パッチのみ。起動用 EXE/startup と本体が含まれない |

### YU-RIS (vnrun)

| ID | 作品 | 結果 | メモ |
|---|---|---|---|
| b2006_whirlpool | [いな☆こい！ ～お稲荷さまとモテモテのたたり～ 体験版 ver 1.00](http://suezou.dyndns.org/dl2006/whirlpool/inakoi/inakoi_trial_100.exe) | 本編 | Chromium で はじめから→本編の文章まで (222 形式の pac/*.ypf) |
| b2007_whirlpool | [magus_trial.exe](http://suezou.dyndns.org/dl2007/whirlpool/magus/magus_trial.exe) | 本編 | Chromium で はじめから→本編の文章まで (263 形式。音声と BGM の ypf は除いて確認) |
| b2010_whirlpool | [涼風のメルト -Where wishes are drawn to each other- WEB体験版](http://suezou.dyndns.org/dl2010/whirlpool/melt/webtrial_melt.exe) | 本編 | Chromium (3f99c53) で START→本編の文章→体験版終了の選択肢まで |
| b2013_anim | [息子の友達に堕とされて　体験版](http://suezou.dyndns.org/dl2013/anim/musuko2/musuko2_trial.zip) | 本編 | Chromium (3f99c53+) で START→本編の文章まで |
| b2014_whirlpool | [G.I.B. ガールズ・イン・ブラック  WEB体験版](http://suezou.dyndns.org/dl2014/whirlpool/gib/gib_webtrial.exe) | 本編 | Chromium (3f99c53) で START→体験版メニュー (本編/Hシーン) と説明文まで |
| b2017_no_strike | [女装千年王国　体験版](http://suezou.dyndns.org/dl2017/no-strike/jyosoumille/jyosoumille_trial.zip) | 本編 | Chromium (3f99c53+) で START→本編の文章まで |
| b2018_casket | [彼女が異種族だった場合～クールな先輩ダークエルフとの甘やかし同棲日誌～体験版](http://suezou.dyndns.org/dl2018/casket/dark_elf/dark_elf-trial.zip) | 本編 | Chromium (3f99c53+) で START→本編の文章まで |
| b2021_whirlpool | [けもの道☆ガーリッシュスクエア　体験版](http://suezou.dyndns.org/dl2021/whirlpool/kemonomichi-gs/kemonomichi-gs_trial.zip) | 本編 | 検証スレッドでも Chromium でタイトル画面まで確認。d98165b で「short arguments」は出ない。Chromium で START→シナリオ選択 (scenario/0_tr.txt の文章と選択肢) まで、ネイティブでは「本編を始める」→ maho1_01 の本文まで (Chromium では devserver が途中で止められ本文の撮影はできず) |
| b2022_casket | [あやかし遊郭～花魁道中～体験版](http://suezou.dyndns.org/dl2022/casket/ayakashi_pack/ayakashipack_trial.zip) | 本編 | Chromium (3f99c53+) で START→本編の文章まで |
| dl_RJ01708454 | [こんな安楽椅子探偵は嫌だ！](https://trial.dlsite.com/doujin/RJ01709000/RJ01708454_trial.zip) | 本編 | Chromium で はじめから→本編の文章まで |
| dl_RJ01721024 | [むかしグラビアアイドルだった団地妻](https://trial.dlsite.com/doujin/RJ01722000/RJ01721024_trial.zip) | 本編 | Chromium (3f99c53+) で START→本編の文章まで |
| dl_VJ01000315 | [【限定版】エルフェンキング](https://trial.dlsite.com/professional/VJ01001000/VJ01000315_trial.zip) | 本編 | Chromium で START→本編の文章まで (名前入力 DIALOG は既定名で進む) |
| dl_VJ01000407 | [画面から出てきた推しVtuberとHな同棲生活 ～夢魔かわASMR配信者は地雷系？～](https://trial.dlsite.com/professional/VJ01001000/VJ01000407_trial.zip) | 本編 | Chromium (3f99c53+) で START→本編の文章まで |
| dl_VJ01001385 | [【豪華版】ボクが当主になるために今日からメイドとHします2](https://trial.dlsite.com/professional/VJ01002000/VJ01001385_trial.zip) | 本編 | Chromium (3f99c53+) で START→本編の文章まで |
| dl_VJ01001386 | [【通常版】ボクが当主になるために今日からメイドとHします2](https://trial.dlsite.com/professional/VJ01002000/VJ01001386_trial.zip) | 本編 | Chromium (3f99c53+) で START→本編の文章まで |
| dl_VJ01002327 | [竜翼のメロディア −Diva with the blessed dragonol−](https://trial.dlsite.com/professional/VJ01003000/VJ01002327_trial.zip) | 本編 | Chromium (3f99c53+) で START→本編の文章まで |
| dl_VJ01002338 | [いな☆こい！ ～お稲荷さまとモテモテのたたり～](https://trial.dlsite.com/professional/VJ01003000/VJ01002338_trial.zip) | 本編 | Chromium で はじめから→本編の文章まで (b2006_whirlpool と同じ 222 形式) |
| dl_VJ01002863 | [【限定版】巨乳軍人支配催眠「私の任務は貴方にご奉仕する事です。色んな穴をご自由にお使い下さい」](https://trial.dlsite.com/professional/VJ01003000/VJ01002863_trial.zip) | 本編 | Chromium で START→本編の文章まで (名前入力 DIALOG は既定名で進む) |
| dl_VJ01004582 | [【豪華版】家族は（俺以外）サキュバスでした ～二人の姉と母との性活で精液を搾り取られます～](https://trial.dlsite.com/professional/VJ01005000/VJ01004582_trial.zip) | 本編 | Chromium で START→本編の文章まで (名前入力 DIALOG は既定名で進む) |
| s2011_whirlpool_maikaze | [舞風のメルト -Where leads to feeling destination- WEB体験版 (ver1.01)](http://suezou.dyndns.org/dl2011/whirlpool/maikaze/webtrial_maikaze_ver101.exe) | 本編 | Chromium (3f99c53) で START→本編の文章まで (クリック位置ずれで LOAD 画面も開く) |
| s2014_whirlpool_tearstilla | [鯨神のティアスティラ 体験版](http://suezou.dyndns.org/dl2014/whirlpool/tearstilla/kujira_trial_web.zip) | 本編 | Chromium (3f99c53+) で START→本編の文章まで |
| s2016_no_strike_jyogaku | [女装学園（孕）　体験版](http://suezou.dyndns.org/dl2016/no-strike/jyogaku/jyogaku_trial.zip) | 本編 | Chromium (3f99c53+) で START→本編の文章まで |
| s2019_no_strike_jyosou_jinjya | [josou jinjya_trial](http://suezou.dyndns.org/dl2019/no-strike/jyosou_jinjya/josou_jinjya_trial_TW.zip) | 本編 | Chromium (675fd0a 以降) で START→本編の文章まで、台湾版の Big5 文章も表示 |
| s2020_casket_ayakashi03 | [あやかし遊郭～花魁アマビエ民譚集～体験版](http://suezou.dyndns.org/dl2020/casket/ayakashi03/ayakashi03_trial.zip) | 本編 | Chromium (3f99c53+) で START→本編の文章まで |
| s2021_casket_bellaca | [リンカーベル・アカデミア～落第魔女とヒミツの儀式～体験版](http://suezou.dyndns.org/dl2021/casket/bellaca/bellaca01_trial.zip) | 本編 | Chromium (3f99c53+) で START→本編の文章まで |
| s2021_casket_bellaca02 | [リンカーベル・アカデミア～龍族のフィアンセ～Hシーン体験版](http://suezou.dyndns.org/dl2021/casket/bellaca02/bellaca02_trial.zip) | 本編 | Chromium で Hシーン体験版→本編の文章まで |
| s2023_casket_oshitona_vtuber02 | [画面から出てきた推しVtuberとHな同棲生活 ～庶民派吸血鬼お嬢様は契約したい～体験版](http://suezou.dyndns.org/dl2023/casket/oshitona_vtuber02/oshitona02_trial.zip) | 本編 | Chromium (3f99c53+) で START→本編の文章まで |
| s2023_casket_oshitona_vtuber03 | [画面から出てきた推しVtuberとHな同棲生活 ～夢魔かわASMR配信者は地雷系？～体験版](http://suezou.dyndns.org/dl2023/casket/oshitona_vtuber03/oshitona03_trial.zip) | 本編 | Chromium (3f99c53+) で START→本編の文章まで |
| r1_yuris | [YU-RIS サンプル](https://yu-ris.net/download/yu-ris_0495_032.zip) | 対象外 | YU-RIS の SDK のサンプル (コンパイル前で ysbin.ypf が無い) |

### NeXAS (vnrun)

| ID | 作品 | 結果 | メモ |
|---|---|---|---|
| b2010_giga | [JINKI EXTEND Re:VISION -SECOND IMPACT-](http://suezou.dyndns.org/dl2010/giga/jinkirv/jinki_webtrial.zip) | 本編 | ネイティブと Chromium でタイトル、最初の台詞とボイスの再生開始を確認。背景演出、続き、セーブ等は未完成 |
| b2011_giga | [ホチキス - 体験版](http://suezou.dyndns.org/dl2011/giga/hotchkiss/hotchkiss_trial.zip) | 本編 | 6875c68 でタイトルから はじめから で本編テキストまで |
| b2012_giga | [キスベル　体験版](http://suezou.dyndns.org/dl2012/giga/kissbell/kissbell_trial.zip) | 本編 | b425e77 (2012 年版の表) で New Game から本編の文章まで |
| b2013_giga | [キスアト体験版](http://suezou.dyndns.org/dl2013/giga/kissato/kissato_trial.zip) | 本編 | 6875c68 でタイトルから new game で本編テキストまで (冒頭は白背景+メッセージウィンドウ) |
| b2016_giga | [リプキス　体験版](http://suezou.dyndns.org/dl2016/giga/repkiss/repkiss_trial.zip) | 本編 | 6875c68 でタイトルから New Game で本編テキストまで |
| b2019_giga | [アイキス 豪華体験版](http://suezou.dyndns.org/dl2019/giga/aikiss/aikiss_premium_trial.zip) | 本編 | 6875c68 でタイトルから本編テキストまで。メッセージウィンドウ画像は未表示 (既知) |
| s2006_giga_ab | [angel breath - エンジェルブレス体験版](http://suezou.dyndns.org/dl2006/giga/ab/ab_trial.zip) | 本編 | 検証スレッドでも Chromium でタイトル画面まで確認。2006 年版の VM (演算子 0xD 以降が 1 つずれる、ヘッダの無い .dat、.ani ボタン) に対応。タイトルの Start から体験版01.bin の本文まで (Chromium) |
| s2007_giga_himesama | [姫さまっ、お手やわらかに！ -体験版-](http://suezou.dyndns.org/dl2007/giga/himesama/hime_trialc.zip) | 本編 | Chromium (3f99c53) で START→本編の文章まで |
| s2008_giga_sakahari | [さかあがりハリケーン 体験版 Ver.2](http://suezou.dyndns.org/dl2008/giga/sakahari/sakahari_trial2.zip) | 本編 | Chromium (3f99c53) で START→本編の文章まで |
| s2009_giga_curefull | [キュアフル！ 体験版](http://suezou.dyndns.org/dl2009/giga/curefull/curefull_trial.zip) | 本編 | Chromium (21b28be 以降) で START→本編の文章まで。メッセージ枠は中央に表示 |
| s2010_giga_bittersmile | [bitter smile. ～ビタースマイル～ 体験版２](http://suezou.dyndns.org/dl2010/giga/bittersmile/bittersmile_trial2.zip) | 本編 | Chromium (21b28be 以降) で START→本編の文章まで。メッセージ枠は中央に表示 |
| s2011_giga_hinataterrace | [ひなたテラス -We don't abandon you.- web体験版２](http://suezou.dyndns.org/dl2011/giga/hinataterrace/hinata_webtrial02c.zip) | 本編 | Chromium (3f99c53) で START→本編の文章まで |
| s2015_giga_renaiphase | [恋愛フェイズ 体験版](http://suezou.dyndns.org/dl2015/giga/renaiphase/RP_trial.zip) | 本編 | 検証スレッドでも Chromium でタイトル画面まで確認。d98165b で確認。タイトルの「Digest Story」→「Digest-1」から 共通00.bin のチャット画面と本文まで (Chromium) |
| s2006_giga_cheerfull | [チアフル！ ～cheerfull!～ 【 体験版 】](http://suezou.dyndns.org/dl2006/giga/cheerfull/cheerfull_trial.exe) | 要再確認 | ダウンロードはできるが、自己展開 exe が GIGA 独自の暗号化形式 (GFL SecureCryptograph) で 7z・unar では展開できない |
| s2025_milimili_hanako | [ちょっとナマイキなトイレの花子さん ～えっちぃ学園のおばけとエロエロ搾精性活～ 【体験版】](http://suezou.dyndns.org/dl2025/milimili/hanako/hanako_try2.zip) | 対象外 | 中身は Unity 製 (vnbox の NeXAS 判定が誤り) |

### AdvHD (vnrun)

| ID | 作品 | 結果 | メモ |
|---|---|---|---|
| b2013_ensemble | [お嬢様はご機嫌ナナメ 体験版](http://suezou.dyndns.org/dl2013/ensemble/ojyonana/ojyonana_trial.exe) | 本編 | 14cde42 で START から本編テキストまで (START 後しばらく白画面) |
| b2014_ensemble | [goldenmarriage_trial.exe](http://suezou.dyndns.org/dl2014/ensemble/golden_marriage/goldenmarriage_trial.exe) | 本編 | b75e0c9 で START から本編テキストまで |
| b2014_ensemble_sweet | [hentai_trial.exe](http://suezou.dyndns.org/dl2014/ensemble-sweet/hentai/hentai_trial.exe) | 本編 | 71cd396 で START から本編テキストまで |
| b2014_guilty | [ANEWO_trial2.exe](http://suezou.dyndns.org/dl2014/guilty/anewo/ANEWO_trial2.exe) | 本編 | 14cde42 で START から本編テキストまで |
| b2014_pulltop_latte | [TotoRezo_WFD_Htrial02.exe](http://suezou.dyndns.org/dl2014/pulltop-latte/totoreso/TotoRezo_WFD_Htrial02.exe) | 本編 | b076f1f で START からキャラクター選択画面まで |
| b2015_ensemble | [marriagefd_trial.exe](http://suezou.dyndns.org/dl2015/ensemble/golden_marriage_jd/marriagefd_trial.exe) | 本編 | b076f1f で START から本編テキストまで |
| b2015_guilty | [淫らに喘ぐ七人の夜這いする孕女　体験版](http://suezou.dyndns.org/dl2015/guilty/yobai3/yobai3_trial.zip) | 本編 | b076f1f で START から本編テキストまで |
| b2018_ensemble | [恋はそっと咲く花のように　体験版](http://suezou.dyndns.org/dl2018/ensemble/koihana/KOIHANA_WEB_TRIAL.zip) | 本編 | b076f1f で START から本編テキストまで |
| b2018_guilty | [俺の上であがく六人の伽女　体験版２](http://suezou.dyndns.org/dl2018/guilty/togime/togime_trial2.zip) | 本編 | b076f1f で START からゲーム内の選択肢「どの体験版をプレイしますか？」まで |
| b2018_tokohana | [僕と彼女のご奉仕同棲　体験版](http://suezou.dyndns.org/dl2018/tokohana/gohoushidousei/gohoushidousei_trial.zip) | 本編 | b076f1f で START から本編テキストまで |
| b2022_ensemble | [華は短し、踊れよ乙女 - 体験版](http://suezou.dyndns.org/dl2022/ensemble/hanaoto/hanaoto_trial.zip) | 本編 | b076f1f で START から本編テキストまで |
| b2025_ensemble_sweet | [のーぶるバトラー TRIAL EDITION](http://suezou.dyndns.org/dl2025/ensemble-sweet/noblebutler/noblebutler_trial.zip) | 本編 | b076f1f で START から本編テキストまで |
| b2025_guilty | [虜ノ焔 ～淫欲の炎に満ちた地下闘技場～ 体験版](http://suezou.dyndns.org/dl2025/guilty/torikonohomura/torikonohomura_trial.zip) | 本編 | b076f1f で START から本編テキストまで |
| b2026_tokohana | [OfficeLove -クールな上司と秘密のイキ抜き- Trial Edition](http://suezou.dyndns.org/dl2026/tokohana/office_love/OfficeLove_trial.zip) | 本編 | 3fe627e で START → 名前入力の OK → 本編テキストまで |
| dl_VJ01002397 | [僕の好きな人の好きな人は、女装した僕でした。](https://trial.dlsite.com/professional/VJ01003000/VJ01002397_trial.zip) | 本編 | b076f1f で START から本編テキストまで |
| dl_VJ01003020 | [幸乃下蛍の恋青日和 ～お義姉ちゃんとの秘密同棲～](https://trial.dlsite.com/professional/VJ01004000/VJ01003020_trial.zip) | 本編 | 14cde42 で START → 名前入力の OK → 本編テキストまで |
| dl_VJ01004020 | [のーぶる・バトラー](https://trial.dlsite.com/professional/VJ01005000/VJ01004020_trial.zip) | 本編 | b076f1f で START から本編テキストまで |
| dl_VJ01004594 | [宿りし乙女の誓いと魔法](https://trial.dlsite.com/professional/VJ01005000/VJ01004594_trial.zip) | 本編 | b076f1f で START から本編テキストまで |
| dl_VJ01004836 | [俺だけに傅く孕メイドたち ～精子を貪る五人の欲心女～](https://trial.dlsite.com/professional/VJ01005000/VJ01004836_trial.zip) | 本編 | b076f1f で START から本編テキストまで |
| dl_VJ015399 | [恋するあまいろホームステイ -留学生はわんこ系幼馴染-](https://trial.dlsite.com/professional/VJ016000/VJ015399_trial.zip) | 本編 | 71cd396 で START → 名前入力の OK → 本編テキストまで (名前も表示) |
| r1_advhd | [有閑夫人倶楽部 体験版](https://www.willplus-download.com/Empress/yukanmadam/yukanmadam_trialedition.exe) | 本編 | b75e0c9 で START から「1 Day」を経て本編テキストまで |
| s2013_guilty_yobai | [夜這いする七人の孕女～体験版2～](http://suezou.dyndns.org/dl2013/guilty/yobai/yobai_trial2.exe) | 本編 | 71cd396 で START から「誰の所へ行きますか？」のマップ選択まで (背景も表示) |
| s2013_pulltop_cocoro | [ココロ＠ファンクション！―えっちシーン体験版](http://suezou.dyndns.org/dl2013/pulltop/cocoro/CocoroFunction_Hscene_trial.exe) | 本編 | 3fe627e で START からキャラクター選択画面まで |
| s2015_ensemble_koikata | [恋する気持ちのかさねかた_Hシーン体験版](http://suezou.dyndns.org/dl2015/ensemble/koikata/koikata_H_trial.zip) | 本編 | b076f1f で START からキャラクター選択画面まで |
| s2015_ensemble_sweet_yakimochi | [エッチでヘンタイ！ヤキモチお嬢様！！　ヘンタイ性癖解放Ver．体験版](http://suezou.dyndns.org/dl2015/ensemble-sweet/yakimochi/Dohentai_trial.exe) | 本編 | b076f1f で START からキャラクター選択画面まで |
| s2016_ensemble_sweet_hanayome | [恋するお嬢様はエッチな花嫁　体験版](http://suezou.dyndns.org/dl2016/ensemble-sweet/hanayome/hanayome_trial.zip) | 本編 | b076f1f で START から本編テキストまで |
| s2018_ensemble_otostuki | [乙女が結ぶ月夜の煌めき　体験版](http://suezou.dyndns.org/dl2018/ensemble/otostuki/ototsuki_trial.zip) | 本編 | b75e0c9 で注意画面 → タイトル → START から本編テキストまで (zip を取り直し) |
| s2019_guilty_harame | [孕女～精子を欲しがる淫らな教え子～ 体験版](http://suezou.dyndns.org/dl2019/guilty/harame/harame_trial.zip) | 本編 | b076f1f で START から本編テキストまで |
| s2022_ensemble_sweet_otoshita | [乙女とふれあう、ひとつ屋根の下 TRIAL EDITION](http://suezou.dyndns.org/dl2022/ensemble-sweet/otoshita/otoshita_trial.zip) | 本編 | b076f1f で START から本編テキストまで |
| s2026_ensemble_sweet_ribichu | [リビドーアバンちゅ～る TRIAL EDITION](http://suezou.dyndns.org/dl2026/ensemble-sweet/ribichu/ribichu_trial.exe) | 本編 | b076f1f で START から本編テキストまで |
| b2011_pulltop | [shinsei_digest_trial.exe](http://suezou.dyndns.org/dl2011/pulltop/shinse/shinsei_digest_trial.exe) | 未確認 |  |
| s2006_pulltop_haruka | [遥かに仰ぎ、麗しの - 体験版](http://suezou.dyndns.org/dl2006/pulltop/haruka/Haruka_Trial.exe) | 未確認 |  |
| s2009_pulltop_bellstars | [しろくまベルスターズ♪－体験版](http://suezou.dyndns.org/dl2009/pulltop/bellstars/Bellstars_Trial.exe) | 未確認 |  |
| s2010_pulltop_lovekami | [恋神　-ラブカミ-（Web用体験版）](http://suezou.dyndns.org/dl2010/pulltop/lovekami/lovekami_webtrial.exe) | 未確認 |  |
| s2012_pulltop_latte_kanojo | [彼女と俺と恋人と。―体験版](http://suezou.dyndns.org/dl2012/pulltop-latte/kanojo/tototo_trial.zip) | 未確認 |  |

### Tmr-Hiro (vnrun)

| ID | 作品 | 結果 | メモ |
|---|---|---|---|
| b2006_lune | [ソウルイーター　体験版　Game Edition](http://suezou.dyndns.org/dl2006/lune/souleater/souleater_trial_g.exe) | 本編 | 961c800 (Marble) で START から本編の文章・立ち絵まで (Chromium) |
| b2006_marine | [人妻搾乳飯店－体験版－](http://suezou.dyndns.org/dl2006/marine/hanten/hanten_trial.exe) | 本編 | 961c800 (Marble) で START から本編の文章・立ち絵まで (Chromium) |
| b2007_lune | [風輪奸山　体験版](http://suezou.dyndns.org/dl2007/lune/kanzan/kanzan_trial.exe) | 本編 | 961c800 (Marble) で START から本編の文章・立ち絵まで (Chromium) |
| b2007_marine | [しおふきマーメイド体験版](http://suezou.dyndns.org/dl2007/marine/siofuki/siofuki_trial.exe) | 本編 | 961c800 (Marble) で START から本編の文章・立ち絵まで (Chromium) |
| b2008_marine | [びんかんアスリート](http://suezou.dyndns.org/dl2008/marine/binkan/binkan_trial.exe) | 本編 | 961c800 (Marble) で START から本編の文章・立ち絵まで (Chromium) |
| b2010_marine | [paicheer_trial.exe](http://suezou.dyndns.org/dl2010/marine/paicheer/paicheer_trial.exe) | 本編 | 6eba683 (Tmr-Hiro) で START から本編の文章・名前・立ち絵/CG まで (Chromium) |
| b2011_marine | [kyonyutry_trial.exe](http://suezou.dyndns.org/dl2011/marine/kyonyutry/kyonyutry_trial.exe) | 本編 | 6eba683 (Tmr-Hiro) で START から本編の文章・名前・立ち絵/CG まで (Chromium) |
| b2023_turumiku_another | [非道狩～ヒトカリ～ 体験版](http://suezou.dyndns.org/dl2023/turumiku-another/hitokari/hitokari-trial.zip) | 本編 | 6eba683 (Tmr-Hiro) で START から本編の文章・名前・立ち絵/CG まで (Chromium) |
| s2006_lune_maimiko | [妹巫女・萌](http://suezou.dyndns.org/dl2006/lune/maimiko/maimiko_trial.exe) | 本編 | 6eba683 (Tmr-Hiro) で START から本編の文章・名前・立ち絵/CG まで (Chromium) |
| s2006_lune_sakuya | [くのいち・咲夜－体験版－](http://suezou.dyndns.org/dl2006/lune/sakuya/kunoichi_trial.exe) | 本編 | 961c800 (Marble) で START から本編の文章・立ち絵まで (Chromium) |
| s2007_marine_succubus | [とらぶるサッキュバス](http://suezou.dyndns.org/dl2007/marine/succubus/succubus_trial.exe) | 本編 | 961c800 (Marble) で START から本編の文章・立ち絵まで (Chromium) |
| s2008_lune_inmu | [淫夢学園－体験版](http://suezou.dyndns.org/dl2008/lune/inmu/inmu_trial.exe) | 本編 | 961c800 (Marble) で START から本編の文章・立ち絵まで (Chromium) |
| s2011_marine_marching | [おっぱい学園マーチングバンド部！　体験版](http://suezou.dyndns.org/dl2011/marine/marching/marching_trial.zip) | 本編 | 6eba683 (Tmr-Hiro) で START から本編の文章・名前・立ち絵/CG まで (Chromium) |
| s2012_marine_deka_ane | [デカくてエッチな俺の姉　体験版](http://suezou.dyndns.org/dl2012/marine/deka_ane/dekaane_trial.zip) | 本編 | 961c800 でタイトル→本編の文章・名前・立ち絵まで (Chromium) |
| s2013_lune_kazokusaimin | [巨乳家族催眠　体験版](http://suezou.dyndns.org/dl2013/lune/kazokusaimin/kazokusaimin_trial.zip) | 本編 | 6eba683 (Tmr-Hiro) で START から本編の文章・名前・立ち絵/CG まで (Chromium) |
| s2014_lune_revenge_room | [リベンジルーム　体験版](http://suezou.dyndns.org/dl2014/lune/revenge_room/revenge_room_trial.zip) | 本編 | 6eba683 (Tmr-Hiro) で START から本編の文章・名前・立ち絵/CG まで (Chromium) |
| s2015_lune_kunoichi_suzaku | [巨乳くのいち朱雀　体験版](http://suezou.dyndns.org/dl2015/lune/kunoichi_suzaku/kunoichisuzaku_trial.zip) | 本編 | 6eba683 (Tmr-Hiro) で START から本編の文章・名前・立ち絵/CG まで (Chromium) |
| s2016_lune_dai_kazoku | [巨乳大家族催眠「家族みんなでイキまくるセックスがやめられないのぉ」](http://suezou.dyndns.org/dl2016/lune/dai_kazoku/dai_kazoku_saimin_trial.zip) | 本編 | 6eba683 (Tmr-Hiro) で START から本編の文章・名前・立ち絵/CG まで (Chromium) |
| s2016_lune_meikoku | [冥刻学園受胎偏・体験版](http://suezou.dyndns.org/dl2016/lune/meikoku/meikoku_jutai_trial.zip) | 本編 | 6eba683 (Tmr-Hiro) で START から本編の文章・名前・立ち絵/CG まで (Chromium) |
| s2016_marine_cospure_harem | [お嬢様学園こすぷれハーレム部！「今日はどの服でエッチしよっか」](http://suezou.dyndns.org/dl2016/marine/cospure_harem/cospure_harem_trial.zip) | 本編 | 6eba683 (Tmr-Hiro) で START から本編の文章・名前・立ち絵/CG まで (Chromium) |
| s2018_lune_elf_saimin | [巨乳エルフ母娘催眠体験版](http://suezou.dyndns.org/dl2018/lune/elf_saimin/elfsaimin_trial.zip) | 本編 | 6eba683 (Tmr-Hiro) で START から本編の文章・名前・立ち絵/CG まで (Chromium) |
| s2019_lune_dogeza_saimin | [巨乳女戦士・土下座催眠「ちくしょうっ……アタシはお前の思い通りになんか、ならないからな……！」](http://suezou.dyndns.org/dl2019/lune/dogeza_saimin/dogeza_saimin_trial.zip) | 本編 | 6eba683 (Tmr-Hiro) で START から本編の文章・名前・立ち絵/CG まで (Chromium) |

### LiveMaker (vnrun)

| ID | 作品 | 結果 | メモ |
|---|---|---|---|
| dl_RJ125204 | [保健室の女神 体験版](https://trial.dlsite.com/doujin/RJ126000/RJ125204_trial.zip) | 本編 | タイトル (バイナリ .lpm のメニュー)→初めから→本編の文章まで (livemaker-run)。Chromium (3f99c53) で Enter のクラッシュ解消を確認 |
| dl_RJ129271 | [とししたハニー 体験版](https://trial.dlsite.com/doujin/RJ130000/RJ129271_trial.zip) | 本編 | タイトル→はじめから→名前入力→選択肢→本編の文章まで (livemaker-run とブラウザ) |
| dl_RJ284038 | [彼女と俺と留学生 体験版](https://trial.dlsite.com/doujin/RJ285000/RJ284038_trial.zip) | 本編 | タイトル (背景つき、文字だけのメニューは体験版本来のもの)→はじめから→本編の文章まで (livemaker-run)。Chromium (3f99c53) で Enter のクラッシュ解消を確認 |
| dl_RJ341209 | [スタイホリック 体験版](https://trial.dlsite.com/doujin/RJ342000/RJ341209_trial.zip) | 本編 | タイトル→Start→名前入力 (打ち替えも可)→本編の文章まで、セーブ画面も開く (livemaker-run)。.lcm の演出 (ロゴ・タイトル前のアニメ) も再生。Chromium (3f99c53) で Enter のクラッシュ解消を確認 |
| b2006_pulltop | [PRINCESS WALTZ WEB体験版2](http://suezou.dyndns.org/dl2006/pulltop/princesswaltz/PrincessWaltz_WebTrial2.exe) | 未確認 |  |
| dl_RJ01720600 | [ダーク・デザイア ～隷属おっぱい揉みゲーム～](https://trial.dlsite.com/doujin/RJ01721000/RJ01720600_trial.zip) | 未確認 |  |
| s2019_alumi_soft_inuyome2 | [わんこの嫁入り ～新たな二本のしっぽ～ 体験版 version 1.00](http://suezou.dyndns.org/dl2019/alumi-soft/inuyome2/inuyome2-trial.zip) | 未確認 |  |

### CatSystem2 (vnrun)

| ID | 作品 | 結果 | メモ |
|---|---|---|---|
| b2014_azarashi_soft | [アマカノ 体験版 甘えてくれる恋人とのエッチなひととき編](http://suezou.dyndns.org/dl2014/azarashi-soft/amakano/amakano_trialH.zip) | 本編 | 360b26c (CatSystem2) でタイトル→はじめから→本編の文章とマップ画面まで (Chromium) |
| b2014_windmill_oasis | [春風センセーション！体験版](http://suezou.dyndns.org/dl2014/windmill-oasis/harukaze/harukaze_trial.exe) | 本編 | 360b26c (CatSystem2) で注意書き→タイトル→START→本編の文章・名前まで (Chromium) |
| b2017_laplacian | [ニュートンと林檎の樹 体験版＋](http://suezou.dyndns.org/dl2017/laplacian/newrin/newrin-trial-plus.zip) | 本編 | 360b26c (CatSystem2) でタイトル→はじめから→体験版メニュー (体験版本編 / えっちしーん) まで (Chromium、完全なダウンロードで確認) |
| r1_cs2_hatsumira | [果つることなき未来ヨリ 体験版](http://suezou.dyndns.org/dl2015/frontwing/hatsumira/Hatsumira_Trial_web.zip) | 未確認 |  |
| s2008_clochette_suzunone7 | [スズノネセブン！体験版](http://suezou.dyndns.org/dl2008/clochette/suzunone7/suzunone7_trial.zip) | 未確認 |  |
| s2011_clochette_kamikaze | [カミカゼ☆エクスプローラー！WEB体験版](http://suezou.dyndns.org/dl2011/clochette/kamikaze/kamikaze_trial_WEB.zip) | 未確認 |  |
| s2014_clochette_sakigake | [サキガケ⇒ジェネレーション！ WEB体験版](http://suezou.dyndns.org/dl2014/clochette/sakigake/sakigake_web_trial.zip) | 未確認 |  |
| s2015_clochette_kokonatsu | [ここから夏のイノセンス！ WEB体験版](http://suezou.dyndns.org/dl2015/clochette/kokonatsu/kokonatsu_web_trial.zip) | 未確認 |  |

### EntisGLS (vnrun)

| ID | 作品 | 結果 | メモ |
|---|---|---|---|
| b2007_marmalade | [リリカル♪りりっく 体験版](http://suezou.dyndns.org/dl2007/marmalade/lyric/lyric_trial.zip) | 本編 | ロゴ→タイトル (3D の演出は描かない)→START→注意書き→本編の文章 (entis-run とブラウザ)。検証スレッドでも cbb2054 の Chromium で START→本編を確認 |
| b2009_cube | [夏ノ雨 体験版](http://suezou.dyndns.org/dl2009/cube/natsunoame/natsunoame_trial.zip) | 本編 | 注意書き→タイトル→初めから→本編の文章 (entis-run とブラウザ)。検証スレッドでも cbb2054 の Chromium で START→本編を確認 |
| b2009_marmalade | [キスと魔王と紅茶 体験版](http://suezou.dyndns.org/dl2009/marmalade/kissmao/kissmao_tr.zip) | 本編 | タイトル→はじめから→本編の文章 (entis-run とブラウザ)。検証スレッドでも cbb2054 の Chromium で START→本編を確認 |
| b2010_undermoon | [痴覚過敏 体験版](http://suezou.dyndns.org/dl2010/undermoon/tikaku/chikakukabin_trial.zip) | 本編 | ロゴ→タイトル→START→本編の文章 (entis-run とブラウザ)。検証スレッドでは cbb2054 の Chromium でタイトルまで確認 |
| b2024_syrup_mm | [狙われた優等生 身代わりの代償 体験版](http://suezou.dyndns.org/dl2024/syrup-mm/yutosei/yutosei_taikenban.zip) | NG | .csx が無い新しい世代 (システムは exe 内、台本は xscript の .srcxml) |
| s2017_marmalade_mashimaro | [mashimaro_trial.zip](http://suezou.dyndns.org/dl2017/marmalade/mashimaro/mashimaro_trial.zip) | 未確認 |  |
| s2022_marmalade_studysteady2 | [スタディ§ステディ２体験版](http://suezou.dyndns.org/dl2022/marmalade/studysteady2/stst2_trial.zip) | 未確認 |  |
| s2025_marmalade_bakaple | [バカップル・サプリメント体験版](http://suezou.dyndns.org/dl2025/marmalade/bakaple/bakaple-trial.rar) | 未確認 |  |
| s2025_syrup_mm_angel_tear_fhd | [狙われた女神天使エンゼルティアーFHD版（体験版）](http://suezou.dyndns.org/dl2025/syrup-mm/angel-tear_fhd/angeltearfhd_trial.zip) | 未確認 |  |

### AliceSoft System4 (xsystem4)

| ID | 作品 | 結果 | メモ |
|---|---|---|---|
| b2012_alicesoft | [母娘乱館 体験版](http://suezou.dyndns.org/dl2012/alicesoft/oyakorankan/oyakorankan_trialver.zip) | 本編 | Chromium で シーン選択 (琴音 純愛)→本編の文章まで (vnrun 検証スレッドでも確認) |
| b2013_alicesoft | [ランス01体験版](http://suezou.dyndns.org/dl2013/alicesoft/rance01/rance01trial.zip) | 本編 | Chromium で注意書き→ゲーム開始→本編の文章まで (vnrun 検証スレッドでも確認) |

### QLIE (vnrun)

| ID | 作品 | 結果 | メモ |
|---|---|---|---|
| b2007_clochette | [かみぱに！体験版](http://suezou.dyndns.org/dl2007/clochette/kamipani/kamipani_trial.zip) | 本編 | QLIE (vnrun) でロゴ→タイトル→Start→本編の文章・名前・背景・立ち絵まで。セーブ→タイトルからロード、バックログも (Chromium)。検証スレッドでもタイトル画面まで確認 |
| b2020_barista_lab | [アナベル・メイドガーデン体験版](http://suezou.dyndns.org/dl2020/barista-lab/annabel/annabel_trial.zip) | 本編 | QLIE (vnrun) で注意書き→タイトル→はじめから→本編の文章・名前・背景・立ち絵まで (ImoScripter の ^ 命令)。セーブ→タイトルの「つづきから」でロードも (Chromium)。検証スレッドでもタイトル画面まで確認 |
| b2009_purplesoftware | [シグナルハート 体験版 Ver.01](http://suezou.dyndns.org/dl2009/purplesoftware/signalheart/SignalHeart_Trial01.exe) | NG | QLIE (vnrun) では FilePackVer3.0 + DLL/key.fkey の暗号が解けず「startup.s: no @@TOP」(GARbro の 3.0 鍵ファイル方式・アイコン鍵を試しても 1PC にならない) |
| r1_qlie | [天翼のアスクレイン 体験版](https://www.route2.co.jp/Triangle/Products/Asklein/download/Asklein_trial.zip) | 未確認 |  |
| s2026_lump_of_sugar_tayutama_zero | [tayutamazero_trial.zip](http://suezou.dyndns.org/dl2026/lump-of-sugar/tayutama_zero/tayutamazero_trial.zip) | 未確認 |  |

### Escude (vnrun)

| ID | 作品 | 結果 | メモ |
|---|---|---|---|
| b2008_escude | [ヴェルディア幻奏曲体験版](http://suezou.dyndns.org/dl2008/escude/verdia/verdia_trial.zip) | 本編 | Escu:de (ACPX の C を自前 VM で実行) でタイトル→NEW GAME→本編の背景・文章まで (escude-run と Chromium)。セーブ (スロットの日時・サムネイル)→タイトルからロードで本編に戻る (escude-run) |
| b2014_escude | [Re;Lord ～ヘルフォルトの魔女とぬいぐるみ～ 体験版](http://suezou.dyndns.org/dl2014/escude/relord/ReLord_Trial.zip) | 本編 | Escu:de でロゴ→タイトル→はじめから→プロローグの文章まで (Chromium)、CG と文章ウィンドウの場面まで (escude-run)。セーブ→タイトルからロードでプロローグに戻る (Chromium と escude-run) |
| dl_VJ01002637 | [悠刻のファムファタル](https://trial.dlsite.com/professional/VJ01003000/VJ01002637_trial.zip) | 未確認 |  |
| dl_VJ01006026 | [姫と艶欲のインペリウム](https://trial.dlsite.com/professional/VJ01007000/VJ01006026_trial.zip) | 未確認 |  |
| s2009_escude_prister | [乙女恋心プリスター体験版](http://suezou.dyndns.org/dl2009/escude/prister/prister_trial.zip) | 未確認 |  |
| s2012_escude_gurenka | [紅蓮華 - 体験版](http://suezou.dyndns.org/dl2012/escude/gurenka/gurenka_trial_net.zip) | 未確認 |  |
| s2014_escude_hanaou | [花嫁と魔王 - 体験版 Ver.](http://suezou.dyndns.org/dl2014/escude/hanaou/28hanaou_trial.zip) | 未確認 |  |
| s2025_escude_haison2 | [廃村少女［弐］　～陰り誘う秘姫の匣～　体験版](http://suezou.dyndns.org/dl2025/escude/haison2/haison2_trial.zip) | 未確認 |  |

### Majiro (vnrun)

| ID | 作品 | 結果 | メモ |
|---|---|---|---|
| b2015_empress | [Closed GAME 体験版](http://suezou.dyndns.org/dl2015/empress/closed_game/ClosedGameTrial.zip) | 本編 | 検証スレッドでも Chromium で START→本編の文章 (または体験版メニュー) まで確認。Majiro (vnrun) でロゴ→タイトル→New Game→本編の背景・立ち絵・名前・文章まで (majiro-run と Chromium)。QSAVE→QLOAD で保存した文章と場面に戻る (majiro-run と Chromium) |
| b2011_empress | [Starless 体験版](http://suezou.dyndns.org/dl2011/empress/starless/starless_trial2_web.zip) | 未確認 |  |
| r1_majiro | [あいれぼ 体験版](http://suezou.dyndns.org/dl2008/siesta/irevo/irevotrial.zip) | 未確認 |  |
| s2012_empress_lewdness | [LEWDNESS ～Vita sexualis～](http://suezou.dyndns.org/dl2012/empress/lewdness/Lewdness_trial.zip) | 未確認 |  |

### Kaguya (vnrun)

| ID | 作品 | 結果 | メモ |
|---|---|---|---|
| b2006_kaguya | [淫皇覇伝アマツ ～白濁の呪印～ 体験版](http://suezou.dyndns.org/dl2006/kaguya/amatsu/amatu_trial.EXE) | 本編 | 検証スレッドでも Chromium で START→本編の文章 (または体験版メニュー) まで確認。Kaguya (vnrun) でロゴ→タイトル→最初から→プロローグ→本編の背景・立ち絵・名前・文章・カットインまで、体験版の最後まで通る (kaguya-run と Chromium)。文章中にセーブ→別セッションでタイトルからロードで同じ文章に戻る (Chromium) |
| b2008_kaguya | [夏神 体験版２](http://suezou.dyndns.org/dl2008/kaguya/natsukami/natsukami_trial02.exe) | 本編 | 検証スレッドでも Chromium で START→本編の文章 (または体験版メニュー) まで確認。Kaguya (vnrun) の 2008 年世代でロゴ→タイトル→最初から→場面選択 (6 項目)→本編の背景・立ち絵 (exe の表から胴体+表情)・名前・文章まで、場面の後は選択に戻る (kaguya-run と Chromium) |
| b2009_kaguya | [さらさらささら 体験版](http://suezou.dyndns.org/dl2009/kaguya/sasara/Srasrasasara_trial.exe) | 本編 | 検証スレッドでも Chromium で START→本編の文章 (または体験版メニュー) まで確認。Kaguya (vnrun) の 2009 年世代 (GAME_SYS.exe + 大文字の .ARC) でロゴ→タイトル→はじめから→本編の背景・立ち絵 (stand.arc の胴体+表情)・名前・文章・アイキャッチまで、体験版の最後 (01_OP09) からタイトルに戻る (kaguya-run と Chromium) |
| b2013_kaguya | [肉牝R30 ～肉欲に堕ちた牝たち～ 体験版](http://suezou.dyndns.org/dl2013/kaguya/r30/nikumesu_R30_trial.zip) | 本編 | 検証スレッドでも Chromium で START→本編の文章 (または体験版メニュー) まで確認。Kaguya (vnrun) の 2013 年世代 (GAME_SYS.exe + ARC/) でロゴ→タイトル→初めから→導入→本編の背景・立ち絵・名前・文章・音声、場面選択の 6 項目 (5 人の場面と製品告知) まで (kaguya-run と Chromium)。場面の後のマップ (op 54 1 0) は未実装で選択に戻る |
| b2015_kaguya | [ハラミタマ 体験版](http://suezou.dyndns.org/dl2015/kaguya/haramitama/haramitama_trial.zip) | 本編 | 検証スレッドでも Chromium で START→本編の文章 (または体験版メニュー) まで確認。Kaguya (vnrun) の LINK 世代 (Start.exe + params.dat + message.dat) でタイトル→はじめから→プロローグ (A0000..A0070) の背景・立ち絵・名前・文章・音声→共通→マップ (文字の選択肢で代用)→各ルート①→体験版の終わりまで通る (kaguya-run と Chromium)。セーブ→タイトルの「つづきから」でロードして同じ文章に戻る (kaguya-run と Chromium) |
| b2018_kaguya | [LoveHolic ～魅惑の乙女と白濁カンケイ～ 体験版](http://suezou.dyndns.org/dl2018/kaguya/love_holic/LoveHolic_trial.zip) | 本編 | 検証スレッドでも Chromium で START→本編の文章 (または体験版メニュー) まで確認。Kaguya (vnrun) の LINK 世代 (params v05.7) でタイトル→はじめから→オープニング (A0000..A0070) の背景・立ち絵・名前 (主人公名の置き換え)・文章・音声→共通→行き先 (文字の選択肢で代用)→紅音ルート→体験版の終わりまで通る (kaguya-run と Chromium) |
| b2007_kaguya | [でるた！（体験版）](http://suezou.dyndns.org/dl2007/kaguya/delta/delta_trial.EXE) | 未確認 |  |
| b2016_kaguya | [小悪魔発育チュ～！（体験版）](http://suezou.dyndns.org/dl2016/kaguya/koakuma/koakuma_trial.zip) | 未確認 |  |
| s2006_kaguya_dungeon | [ダンジョンクルセイダーズ（プロローグ体験版）](http://suezou.dyndns.org/dl2006/kaguya/dungeon/dc_trial.EXE) | 未確認 |  |
| s2006_kaguya_hitocos2 | [人妻コスプレ喫茶２(体験版)](http://suezou.dyndns.org/dl2006/kaguya/hitocos2/hitocos2_trial.EXE) | 未確認 |  |
| s2007_kaguya_toriko | [虜ノ姫（体験版）](http://suezou.dyndns.org/dl2007/kaguya/toriko/toriko_trial.exe) | 未確認 |  |
| s2008_kaguya_dc2 | [ダンジョンクルセイダーズ２（体験版）](http://suezou.dyndns.org/dl2008/kaguya/dc2/DC2_Trial.exe) | 未確認 |  |

### Interheart (vnrun)

| ID | 作品 | 結果 | メモ |
|---|---|---|---|
| b2013_inter_heart | [いたずら ～僕だけの絶対領域～ 体験版2](http://suezou.dyndns.org/dl2013/inter-heart/itazura/itazura_trial2.zip) | 本編 | Interheart (ACT_*.TXT を自前で実行) でタイトル→はじめから→キャラ選択→本編のイベント CG・文章・ボイスまで (interheart-run と Chromium)。セーブ→タイトルからロードで本編に戻る (両方) |
| b2016_inter_heart_glossy | [はらかつ！ ～気になるあの子と子作りエッチ～ 体験版](http://suezou.dyndns.org/dl2016/inter-heart_glossy/harakatsu/harakatsu-trial.zip) | 本編 | 検証スレッドでも Chromium で START→本編の文章 (または体験版メニュー) まで確認。Interheart でタイトル→はじめから→本編の E-mote 静止画 (EMT altfile)・立ち絵・文章まで (interheart-run と Chromium)。セーブ→タイトルからロードで本編に戻る (Chromium) |
| b2017_inter_heart_glossy | [はらかつ２ ～子作りエッチでお悩み解決～ 体験版](http://suezou.dyndns.org/dl2017/inter-heart_glossy/harakatsu2/harakatu2-trial.zip) | 本編 | 検証スレッドでも Chromium で START→本編の文章 (または体験版メニュー) まで確認。Interheart でタイトル→はじめから→本編の背景・立ち絵・文章まで (interheart-run と Chromium)。セーブ→タイトルからロードで本編に戻る (Chromium) |
| b2017_sweet_heart | [透けちゃって！！～九死に一生を得て濃密セカンドライフ～ 体験版](http://suezou.dyndns.org/dl2017/sweet-heart/sukechatte/suketyatte-trial.zip) | 本編 | 検証スレッドでも Chromium で START→本編の文章 (または体験版メニュー) まで確認。Interheart でタイトル→はじめから→本編の背景・立ち絵・文章まで (interheart-run と Chromium)。セーブ→タイトルからロードで本編に戻る (Chromium) |
| b2018_hachimitsu_soft | [ノットフィクション！ 体験版](http://suezou.dyndns.org/dl2018/hachimitsu-soft/knotfc/knotfiction-trial.zip) | 本編 | 検証スレッドでも Chromium で START→本編の文章 (または体験版メニュー) まで確認。Interheart でタイトル (exe のボタン表から titlechip の本物のボタン)→はじめから→本編の背景・立ち絵・文章まで (interheart-run と Chromium)。セーブ→タイトルからロードで本編に戻る (Chromium) |
| b2019_marshmallow_soft | [らぶこみゅ 体験版2](http://suezou.dyndns.org/dl2019/marshmallow-soft/love_commu/lovecommu-trial2.zip) | 本編 | 検証スレッドでも Chromium で START→本編の文章 (または体験版メニュー) まで確認。Interheart でタイトル (exe のボタン表から titlechip の本物のボタン)→はじめから→体験版の選択肢→本編の背景・立ち絵・イベント CG・文章まで (interheart-run と Chromium)。セーブ→タイトルからロードで本編に戻る (Chromium) |
| b2020_marshmallow_soft | [あまママほりっく 体験版](http://suezou.dyndns.org/dl2020/marshmallow-soft/amamama/amamama-trial.zip) | 本編 | 検証スレッドでも Chromium で START→本編の文章 (または体験版メニュー) まで確認。Interheart でタイトル→はじめから→本編の背景・立ち絵・文章まで (interheart-run と Chromium)。セーブ→タイトルからロードで本編に戻る (Chromium)。00.fpk は WMV のため読まない |
| b2020_shiba_soft | [ママ×カノ＋ 体験版](http://suezou.dyndns.org/dl2020/shiba-soft/mamakano_plus/mamakanoPlus_trial.zip) | 本編 | 検証スレッドでも Chromium で START→本編の文章 (または体験版メニュー) まで確認。Interheart でタイトル (exe のボタン表から titlechip の本物のボタン)→はじめから→本編の背景・立ち絵・顔グラ・文章まで (interheart-run と Chromium)。セーブ→タイトルからロードで本編に戻る (Chromium) |
| b2021_dessert_soft | [彼女は友達ですか？恋人ですか？それともトメフレですか？Second 体験版](http://suezou.dyndns.org/dl2021/dessert-soft/tomefure_second/TomefureS-trial.zip) | 本編 | 検証スレッドでも Chromium で START→本編の文章 (または体験版メニュー) まで確認。Interheart でタイトル (exe のボタン表から titlechip の本物のボタン)→はじめから→スキップ確認の選択肢→本編の背景 (PNG)・立ち絵・文章まで (interheart-run と Chromium)。セーブ→タイトルからロードで本編に戻る (Chromium) |
| b2026_dessert_soft | [どいなかガールズゆーとぴあ 体験版](http://suezou.dyndns.org/dl2026/dessert-soft/doinaka_girl/doinaka-trial.rar) | 本編 | 検証スレッドでも Chromium で START→本編の文章 (または体験版メニュー) まで確認。Interheart でタイトル (exe のボタン表から titlechip の本物のボタン)→はじめから→本編の背景・立ち絵・イベント CG・文章まで (interheart-run と Chromium)。セーブ→タイトルからロードで本編に戻る (Chromium) |
| b2006_interheart | [オイラは番台２ 体験版](http://suezou.dyndns.org/dl2006/interheart/bandai2/bandai2-trial.lzh) | NG | data.fpk 方式だが脚本が古い ACT_00.txt (SC_DATA 等の銭湯経営シミュレーション) で、Interheart エンジンの ACT_<letter>.TXT 方言と違う |
| b2008_interheart | [いたずら極―リアル―　体験版](http://suezou.dyndns.org/dl2008/interheart/itazura-real/ItazuraR-Trial.zip) | 未確認 |  |

### Crowd/ANIM (vnrun)

| ID | 作品 | 結果 | メモ |
|---|---|---|---|
| b2007_anim | [必殺痴漢人　- TRIAL VERSION -](http://suezou.dyndns.org/dl2007/anim/chikan3/ci3_trial.lzh) | 本編 | 検証スレッドでも Chromium で START→本編の文章 (または体験版メニュー) まで確認。Crowd/ANIM の 2007 年版 (番号と引数が違う別表、exe の解釈ループから。全ジャンプが命令の頭に落ちるのを確認) でタイトル (p_menu)→はじめから→本編の文章・名前・背景・立ち絵・選択肢 (anim-run と Chromium)。上 48 の帯と下 144 の枠は p_message から。タイトルのメニューは自前の文字ボタン |
| b2008_anim | [息子の友達に犯されて - TRIAL VERSION -](http://suezou.dyndns.org/dl2008/anim/musuko/musuko_trial.lzh) | 本編 | 検証スレッドでも Chromium で START→本編の文章まで確認。Crowd/ANIM (<game>_sce.dat を自前で実行) でタイトル→START→説明文→蓉子編/一志編の選択肢→本編の背景・立ち絵・イベント CG・文章・ボイスまで、体験版の最後 (タイトルに戻る) まで (anim-run と Chromium)。セーブ→タイトルからロードで本編に戻る (両方) |
| b2009_anim | [愛娘という名の玩具　- TRIAL VERSION -](http://suezou.dyndns.org/dl2009/anim/eva/eve_trial.lzh) | 本編 | 検証スレッドでも Chromium で START→本編の文章まで確認。Crowd/ANIM で 1024x768 のタイトル (Start/Load/Exit)→本編の背景 (1024x576 を上下の帯の間に)・立ち絵・イベント CG・文章まで (anim-run と Chromium)。セーブ→タイトルからロードで本編に戻る (Chromium)。メッセージ枠は自前の半透明の帯 (p_message が無い) |
| b2010_anim | [贄姫 - TRIAL VERSION -](http://suezou.dyndns.org/dl2010/anim/niehime/nime_trial.lzh) | 本編 | Crowd/ANIM でタイトル (縦書きの 5 ボタン)→初めから→本編の背景・立ち絵・文章まで、体験版の最後まで (anim-run と Chromium)。セーブ→タイトルからロードで本編に戻る (Chromium) |
| b2011_anim_team_mm | [NTR彼女はいじめられっ娘 - TRIAL VERSION -](http://suezou.dyndns.org/dl2011/anim-team-mm/ntr/ntr_trial_02.zip) | 本編 | 検証スレッドでも Chromium で START→本編の文章まで確認。Crowd/ANIM の 2011 年版 (命令の番号が付け直された別表、exe の解釈ループから) でタイトル (pMenuTrial と pMenuTrialButton、位置は exe の数値)→はじめから→本編の背景・立ち絵 (GAX)・文章・ボイス・選択肢まで (anim-run と Chromium)。セーブ→タイトルからロードで本編に戻る (Chromium) |
| b2017_anim | [いつまでも俺は母に恋してる！ - 体験版２ -](http://suezou.dyndns.org/dl2017/anim/mamakoi/mamakoi_trial_v2.zip) | NG | シナリオが Enigma Protector で包まれた exe の中にあり、<game>_sce.dat が無い。保護を外すのはコピー保護の回避にあたるため試みない |
| b2019_anim | [いつまでも息子のままじゃいられない！５ - 体験版 -](http://suezou.dyndns.org/dl2019/anim/musumama5/musu_mama5_trial.zip) | NG | シナリオが Enigma Protector で包まれた exe の中にあり、<game>_sce.dat が無い。保護を外すのはコピー保護の回避にあたるため試みない |
| b2022_anim | [愛妻Ｘ交姦妊活スワッピング - 体験版 -](http://suezou.dyndns.org/dl2022/anim/aisaikoukan/aisaikoukan_trial.zip) | NG | シナリオが Enigma Protector で包まれた exe の中にあり (cg/psbpack.dat と E-mote)、<game>_sce.dat が無い。保護を外すのはコピー保護の回避にあたるため試みない |
| b2026_anim | [ママラブ！２～俺の彼女は俺のママ☆世話焼き巨乳凛桜ママ～ - 体験版 -](http://suezou.dyndns.org/dl2026/anim/mamalove2/ANIM_mamalove2_trial.zip) | NG | シナリオが Enigma Protector で包まれた exe の中にあり (cg/psbpack.dat と E-mote)、<game>_sce.dat が無い。保護を外すのはコピー保護の回避にあたるため試みない |
| b2025_anim | [お姉ちゃんのあまエロレッスン２ ～お隣の巨乳お姉ちゃんにイッパイ甘やかされるめちゃエロライフ～  - 体験版 -](http://suezou.dyndns.org/dl2025/anim/ane_amalesson2/ANIM_amaerolesson2_trialver.zip) | 未確認 |  |

### Yuka (vnrun)

| ID | 作品 | 結果 | メモ |
|---|---|---|---|
| b2007_feng | [あかね色に染まる坂 体験版](http://suezou.dyndns.org/dl2007/feng/akaneiro/romasaka_try.zip) | 本編 | Yuka (vnrun) でタイトル→はじめから→本編の背景・立ち絵・文章・名前まで (yuka-run と Chromium)。セーブ→ロードで本編に戻る (Chromium) |
| b2008_hook | [FairlyLife WEB体験版02](http://suezou.dyndns.org/dl2008/hook/fairlylife/fairlylife_trial_02.zip) | 本編 | Yuka (vnrun) で .dat (YKC001) からタイトル→本編の背景・立ち絵・文章まで (yuka-run と Chromium)。セーブ→タイトルからロード・メニューからロードで本編に戻る (Chromium) |
| b2008_smee | [晴れハレはーれむ 体験版](http://suezou.dyndns.org/dl2008/smee/harehare/harehare_trial.zip) | 本編 | Yuka (vnrun) でタイトル→本編の背景・文章まで (yuka-run と Chromium) |
| b2011_hook | [Strawberry Nauts 体験版01](http://suezou.dyndns.org/dl2011/hook/strawberry_nauts/strawberrynauts_trial01.exe) | 本編 | Yuka (vnrun) で自己展開 exe (zip) の DATA01..03.ykc から 1024x576 のタイトル→はじめから→本編の背景・立ち絵・文章・名前まで (yuka-run と Chromium)。ルビ (@r) は本文だけ表示 |
| b2011_smee | [ラブラブル 体験版 vol.01](http://suezou.dyndns.org/dl2011/smee/loveable/loverable_trial_01.zip) | 本編 | Yuka (vnrun) でタイトル (SpriteCreate のパターンのボタン、MouseCase2)→名前入力 (既定の名前で「はい」)→本編の背景・立ち絵・文章・名前まで (yuka-run と Chromium) |
| b2012_feng | [星空へ架かる橋ＡＡ 円佳スペシャルアフター体験版](http://suezou.dyndns.org/dl2012/feng/hoshikaka-aa/hoshiaamadotry.zip) | 本編 | Yuka (vnrun) で 1280x960 の注意書き→タイトル→はじめから→本編の背景・立ち絵・文章・名前 (@gf の名前画像) まで (yuka-run と Chromium)。セーブ→タイトルのつづきから (自前のロード画面) で本編に戻る (Chromium) |
| b2013_hooksoft | [PriministAr 体験版](http://suezou.dyndns.org/dl2013/hooksoft/priministar/primini_trial.zip) | 本編 | Yuka (vnrun) で 1280x720 のタイトル→START GAME→体験版の説明→本編の背景・立ち絵・文章まで (yuka-run と Chromium) |
| b2009_smee | [らぶでれーしょん！ Ver1.00](http://suezou.dyndns.org/dl2009/smee/love-delation/ld_trial_100.zip) | 未確認 |  |

### System-NNN (vnrun)

| ID | 作品 | 結果 | メモ |
|---|---|---|---|
| b2006_blackcyc | [EXTRAVAGANZA ～蟲愛でる少女～ 体験版2](http://suezou.dyndns.org/dl2006/blackcyc/extravaga/extravaganza_trial2.exe) | 本編 | System-NNN (vnrun) で自己展開 exe (LZH) からオープニング→タイトル→はじめから→チャート (chart/*.xtx のスロット選択)→本編の背景・立ち絵・文章・名前まで (nnn-run と Chromium) |
| b2008_blackcyc | [ＢＤＤ 体験版Ⅱ](http://suezou.dyndns.org/dl2008/blackcyc/bdd/bdd_taiken2.exe) | 本編 | System-NNN (vnrun) でオープニング→タイトル→はじめから→依頼の掲示板 (bdd/irailist.xtx)→本編の背景・立ち絵・文章・名前まで (nnn-run と Chromium)。セーブ→ロードで本編に戻る (Chromium) |
| b2011_blackcyc | [獄淫の尖塔 体験版](http://suezou.dyndns.org/dl2011/blackcyc/gokuin/gokuin_trial.exe) | 本編 | System-NNN (vnrun) でオープニング→タイトル→はじめから→本編の背景・立ち絵・文章・名前まで (nnn-run と Chromium) |
| b2016_blackcyc | [EXTRAVAGANZA ～蟲愛でる少女～ 体験版 (幼虫)](http://suezou.dyndns.org/dl2016/blackcyc/extravaganza/EXTRAVAGANZA_trial_youtyu.zip) | 本編 | System-NNN (vnrun) で zip 内の自己展開 exe (RAR) からオープニング→タイトル→NEW GAME→チャート (chart/*.fxf)→Prologue の背景・文章・名前まで (nnn-run と Chromium) |
| b2021_metalogiq | [魔法少女消耗戦線 DEADΩAEGIS 体験版](http://suezou.dyndns.org/dl2021/metalogiq/dead_end_aegis/DeadendAegis_trial.exe) | 本編 | System-NNN (vnrun) で 1280x720 のロゴ→タイトル→新規スタート→本編の背景・立ち絵・文章・名前まで (nnn-run と Chromium)。ディスク容量のため ev0.gpk・cdvaw・movie を除いて展開して確認 |
| b2023_cyc_rose | [薔薇ノ木ニ薔薇ノ花咲ク 体験版](http://suezou.dyndns.org/dl2023/cyc-rose/bara_re/baraRe_trial.zip) | 本編 | System-NNN (vnrun) で zip 内の自己展開 exe (RAR5) からオープニング→タイトル→本編の背景 (dwq/<xx>/ のばらの dwq)・文章・名前まで (nnn-run と Chromium) |

### Bishop (vnrun)

| ID | 作品 | 結果 | メモ |
|---|---|---|---|
| b2007_bishop | [姫奴隷 体験版](http://suezou.dyndns.org/dl2007/bishop/hd/hd_trial2.zip) | 本編 | Bishop (vnrun) で BSScript Rev.6 (bsx.dat、Shift-JIS) のロゴ→タイトル→はじめから→本編の背景・立ち絵・文章・名前→マップ (#result の分岐を一覧で選ぶ)→エステルのイベントまで (bsx-run と Chromium)。イベント中でセーブ→ページを開き直してタイトルの「続きから」でロード→続きを読める (Chromium) |
| b2008_bishop | [毎日けだものっ!! WEB体験版](http://suezou.dyndns.org/dl2008/bishop/kedamono/kedamono_web_trial.zip) | 本編 | Bishop (vnrun) で BSScript Rev.6 のロゴ→タイトル (ボタンの位置は画像側)→はじめから→本編の背景・立ち絵・文章・名前まで (bsx-run と Chromium)。movie を除いて展開して確認 |
| b2010_bishop | [学園3～華麗なる悦辱～ 体験版 WEB edition](http://suezou.dyndns.org/dl2010/bishop/gk3/gk3_trial.zip) | 本編 | Bishop (vnrun) で BSXScript 2.0 の注意書き→タイトル→始めから→本編の背景・CG・立ち絵・文章・名前、体験版の最後まで (bsx-run と Chromium) |
| b2011_bishop | [裏教師～背徳の淫悦授業～ WEB体験版](http://suezou.dyndns.org/dl2011/bishop/uk/uk_trial.zip) | 本編 | Bishop (vnrun) で BSXScript 2.0 の注意書き→タイトル→始めから→本編の背景・CG・立ち絵 (体と顔の合成)・文章・名前、体験版の最後からタイトルへ (bsx-run と Chromium)。セーブ→ページを開き直してタイトルの「続きから」でロード→同じ CG・文章に戻る (Chromium) |
| b2012_bishop | [しりこん☆まじっく WEB体験版](http://suezou.dyndns.org/dl2012/bishop/sl/silimagi_trial.zip) | 本編 | Bishop (vnrun) で BSXScript 2.0 の 1024x576 の注意書き→タイトル→始めから→本編の背景・CG・立ち絵・文章・名前まで (bsx-run と Chromium)。movie を除いて展開して確認 |
| b2021_bishop | [屈辱3 体験版](http://suezou.dyndns.org/dl2021/bishop/ktjk3/ktjk3_trial.zip) | 本編 | Bishop (vnrun) で BSXScript 3.3 の 1024x768 の注意書き→タイトル→最初から始める→本編の背景・CG・立ち絵 (命令 39、近景の x -663)・文章・名前まで (bsx-run と Chromium)。movie を除いて展開して確認 |

### Eushully AGE (vnrun)

| ID | 作品 | 結果 | メモ |
|---|---|---|---|
| b2009_eushully | [姫狩りダンジョンマイスター 体験版](http://suezou.dyndns.org/dl2009/eushully/himegari/Himegari_Trial.zip) | 本編 | Eushully AGE (vnrun、SYS4 スクリプトを自前で実行) でタイトル→Game Start→引継ぎ設定→ゲームスタート→プロローグの CG・背景・立ち絵・メッセージ窓・名前・文章まで (age-run と Chromium)。InstallShield の setup は tools/verify/isx.py → 7z (MSI) → Data1.cab で展開 |
| s2010_eushully_ikusa_verita | [戦女神VERITA 体験版](http://suezou.dyndns.org/dl2010/eushully/ikusa-verita/100225Verita_Trial.zip) | 本編 | Eushully AGE (vnrun) でタイトル→Game Start→序章の地図・CG・立ち絵・メッセージ窓・文章まで (age-run と Chromium)。setup は InstallShield (PE の後に NB10 の記録、MSI は zlib で包まれている) を展開してから |
| s2016_eushully_sankaio | [珊海王の円環 体験版](http://suezou.dyndns.org/dl2016/eushully/sankaio/sankai_trial.zip) | 本編 | Eushully AGE (vnrun) でタイトル→Game Start→シナリオ選択 (チュートリアル)→開始→アナスタシアの CG・名前・文章まで、クリックで文章が進む (age-run と Chromium)。画面は 1280x720 (SYSTEM4 が左上に開く最大の窓から)。setup は InstallShield (ISSetupStream、型 3 は属性 24 バイト)。メッセージ窓の下地 (sprite 204000) は 202 の到達値 -1 の扱いを直して天冥のコンキスタで出ることを確認 (同じ処理、珊海王では未再確認) |
| s2020_eushully_tenmei | [天冥のコンキスタ 体験版](http://suezou.dyndns.org/dl2020/eushully/tenmei/tenmei_trial.zip) | 本編 | Eushully AGE (vnrun) でタイトル→GAME START→体験版開始→ヘルミィナの CG・背景・メッセージ窓・名前・文章まで、クリックで文章が進む (age-run と Chromium)。画面は 1600x900。setup は InstallShield (MSI の中の Data1.cab) |

### Atelier G/H (vnrun)

| ID | 作品 | 結果 | メモ |
|---|---|---|---|
| b2012_atelier_gh | [学園ＮＴＲ -僕の知らない彼女の淫顔- 体験版](http://suezou.dyndns.org/dl2012/atelier-gh/ntr/ntr_trial.exe) | 本編 | Atelier G/H (vnrun、crates/ateliergh) でロゴ→注意書き→タイトル→すた～と→A010 の背景・顔・名前・文章→A020 の終わりの選択肢まで (agh-run と Chromium)。えろえろも～ど の場面選択 (マスク画像の当たり判定) も表示。setup は 7z の自己展開 |

### DebonosuWorks (vnrun)

| ID | 作品 | 結果 | メモ |
|---|---|---|---|
| dl_VJ01000787 | [神楽黎明記 参 ～紫の章～ 体験版](https://trial.dlsite.com/professional/VJ01001000/VJ01000787_trial.zip) | 本編 | DebonosuWorks (vnrun、crates/debonosu) でロゴ→注意書き→タイトル→最初から始める→プロローグ (01.scb) の背景・名前・文章・声・BGM まで (deb-run と Chromium)。ダンジョン (map) に入る所で止まり、Live2D の立ち絵・3D・戦闘はなし。自前のセーブ・ロード (Esc で保存、タイトルの「続きから始める」で読み込み) も Chromium で確認 |

### Cyberworks (vnrun)

| ID | 作品 | 結果 | メモ |
|---|---|---|---|
| b2018_tinkerbell | [グランギニョルの夜 体験版](http://suezou.dyndns.org/dl2018/tinkerbell/grand_guignol/gguignol_taiken001.zip) | 本編 | 検証スレッドでも Chromium で START→本編の文章まで確認。Cyberworks (vnrun) で警告→タイトル (e0 のボタン)→Game Start→プロローグの背景 (移動)・立ち絵 (表情差分)・名前・文章・回想のセピアまで (cyberworks-run と Chromium)。右クリックのメニューからセーブ→タイトルの Load Game からロードで本編に戻る (Chromium) |
| b2023_wendybell | [モラトリアム 体験版1.1b](http://suezou.dyndns.org/dl2023/wendybell/moratorium/moratorium_taiken110b.zip) | 本編 | Cyberworks 新形式 (Resources/Data00.dat の索引と DataNNN.dat の絵、UTF-16 の文章) で警告→タイトル (初めから)→本編の背景・立ち絵・表情差分・文章・音声 (cyberworks-run と Chromium)。セーブ→タイトルの続きからロードで本編に戻る (Chromium) |
| b2026_tinkerbell | [ムジナ・臭 体験版](http://suezou.dyndns.org/dl2026/tinkerbell/mujina_syu/mujina_syu_taiken100.zip) | 本編 | Cyberworks 新形式 (1920x1080) で警告→タイトル (NEW GAME)→本編の背景・立ち絵・顔窓 (サイズ付きの差分)・名前・文章 (cyberworks-run と Chromium)。下のバーの SAVE でセーブ→新しいセッションのタイトルの LOAD GAME からロードで同じ行に戻る (Chromium) |
| b2026_wendybell | [友恵の探偵物語～のぞき魔な私の天職～ 体験版](http://suezou.dyndns.org/dl2026/wendybell/tomoe/tomoe_taiken100.zip) | 本編 | Cyberworks 新形式 (1920x1080、全画面の窓) で警告→タイトル (初めから)→本編の CG・背景・文章を数場面 (cyberworks-run と Chromium)。右のバーの Save でセーブ→新しいセッションのタイトルの続きからロードで同じ行に戻る (Chromium) |
| b2012_tinkerbell | [籠女の繭 Web体験版1.1](http://suezou.dyndns.org/dl2012/tinkerbell/kagome/kagome_trial_1_1.zip) | 未確認 |  |
| b2013_tinkerbell | [ぜったいマジラブ！あまイチャはぷにんぐ！ 体験版](http://suezou.dyndns.org/dl2013/tinkerbell/amaicha/amaicha_taiken100.zip) | 未確認 |  |
| b2014_wendybell | [こすぷれえっち 体験版](http://suezou.dyndns.org/dl2014/wendybell/cosplay/cosplay_taiken100.zip) | 未確認 |  |
| b2021_tinkerbell | [淫妖蟲穢 体験版](http://suezou.dyndns.org/dl2021/tinkerbell/inyou_wai/inyou_wai_taiken100.zip) | 未確認 |  |
| s2017_wendybell_oreyome | [俺嫁！？　体験版Ver.2](http://suezou.dyndns.org/dl2017/wendybell/oreyome/oreyome_taiken200.zip) | 未確認 |  |

### LiLiM (vnrun)

| ID | 作品 | 結果 | メモ |
|---|---|---|---|
| b2006_lilim | [とらぶるっ！ 序章体験版](http://suezou.dyndns.org/dl2006/lilim/toraburu/toraburu_trial_pre.lzh) | 本編 | 検証スレッドでも Chromium で START→本編の文章 (または体験版メニュー) まで確認。LiLiM (vnrun) の AOS 版 1 (sfa9.exe、800x600、音は bgm/・se/ のファイル) でタイトル→START→0001..0008 の背景・立ち絵・イベント CG・名前・文章・音声を最後まで通り、タイトルに戻る (lilim-run)。Chromium でタイトル→本編の文章まで |
| b2007_lilim | [看護戦隊ナースレンジャー 体験版](http://suezou.dyndns.org/dl2007/lilim/nasuren/nasu_taiken.lzh) | 本編 | 検証スレッドでも Chromium で START→本編の文章 (または体験版メニュー) まで確認。LiLiM (vnrun) でタイトル→START→体験版の場面 (22_04 から s22_21r/b/g、バッドエンド) の背景・立ち絵・名前・文章・音声を最後まで通り、タイトルに戻る (lilim-run) |
| b2007_lilim_darkness | [とらいあんぐるＢＬＵＥ 体験版](http://suezou.dyndns.org/dl2007/lilim-darkness/toraiangurublue/toraianguru_blue_taiken.lzh) | 本編 | 検証スレッドでも Chromium で START→本編の文章 (または体験版メニュー) まで確認。LiLiM (vnrun) の AOS 版 1 でタイトル→START→0001..0008 の背景・立ち絵・顔窓・イベント CG・名前・文章・音声・アイキャッチを最後まで通り、タイトルに戻る (lilim-run) |
| b2009_lilim | [あい☆きゃん 体験版](http://suezou.dyndns.org/dl2009/lilim/ai-kyan/ican_trial.zip) | 本編 | 検証スレッドでも Chromium で START→本編の文章 (または体験版メニュー) まで確認。LiLiM (vnrun) でタイトル→START→美夕の挨拶 (文字の選択肢 /...//)→体験版メニュー→プロローグ (00pro01, 00pro02, 010101) の背景・立ち絵・顔窓・名前・文章・音声→「体験版は、ここで終了です」→メニューに戻る (lilim-run)。OP ムービーは飛ばす |
| b2011_princess_sugar | [姫様限定！～PrincessLimited～ web体験版ver1.0](http://suezou.dyndns.org/dl2011/princess-sugar/princess/hime_trial_1.zip) | 本編 | 検証スレッドでも Chromium で START→本編の文章 (または体験版メニュー) まで確認。LiLiM (vnrun) でタイトル (1024x600)→START→体験版メニュー (slctwnd のボタン)→プロローグ (00_01..00_10) の背景・立ち絵・名前 (窓の名札)・文章・音声を最後まで通り、メニューに戻る (lilim-run)。OP ムービーは飛ばす |
| b2019_lilim_darkness | [WhiteBlue web体験版ver2.0](http://suezou.dyndns.org/dl2019/lilim-darkness/whiteblue/WhiteBlue_trial_2.zip) | 本編 | 検証スレッドでも Chromium で START→本編の文章 (または体験版メニュー) まで確認。LiLiM (vnrun) でタイトル (btn_play などの画像ボタン)→START→注意書き→体験版メニュー (slctwnd の選択肢)→旭ルートのダイジェスト (D_07..D_09) の背景・立ち絵・イベント CG・名前・文章・音声・回想のセピアを最後まで通り、メニューに戻る (lilim-run)。Chromium でタイトル→本編、右クリックのメニューからセーブ→タイトルの LOAD からロードで本編に戻る |
| b2021_princess_sugar | [プリンセス☆シスターズ！～四姉妹は全員あなたの許嫁～ web体験版ver1.0](http://suezou.dyndns.org/dl2021/princess-sugar/prista/PriSta_trial_1.zip) | 本編 | LiLiM (vnrun) でタイトル→START→注意書き→体験版メニュー (slctwnd のボタン)→共通パート (01_01..02_08、途中のボタンの選択肢を含む) の背景・立ち絵・イベント CG・名前・文章・音声を通る (lilim-run)。Chromium でタイトル→メニュー→本編の文章まで |

### Silky's (vnrun)

| ID | 作品 | 結果 | メモ |
|---|---|---|---|
| b2014_silkys_plus | [なないろリンカネーション 体験版 ver.1.2](http://suezou.dyndns.org/dl2014/silkys-plus/nanairo/nanairo_trial1.2.zip) | 本編 | Silky's (vnrun) でロゴ・注意書き→タイトル (title.akb のボタン)→はじめから→nanairo_01..03 の背景・立ち絵・名前・文章・選択肢を進む (silkys-run、70000 フレーム)。Chromium でタイトル→本編、SAVE からセーブ→タイトルの「つづきから」でロードして本編に戻る |
| b2016_silkys_sakura | [ママのおっぱい～俺の童貞ミルクが搾り取られた件～ 体験版](http://suezou.dyndns.org/dl2016/silkys-sakura/mama_oppai/oppai_trial.zip) | 本編 | Silky's (vnrun) でロゴ・注意書き→タイトル (Title_scroll のスクロール、trial_Title_p のボタン)→はじめから→mmsc_000..006 の背景・立ち絵・イベント CG・名前・文章を最後まで進む (silkys-run、45000 フレーム)。Chromium でタイトル→本編、SAVE ボタンからセーブ→タイトルの「続きから始める」でロードして本編 (背景・イベント CG・立ち絵込み) に戻る |

### Ivory (vnrun)

| ID | 作品 | 結果 | メモ |
|---|---|---|---|
| b2016_evoll | [お嬢様はイイナリみたいです！？ 体験版](http://suezou.dyndns.org/dl2016/evoll/oinari/trial_oinari.zip) | 本編 | Ivory (vnrun) で注意書き・ロゴ→タイトル (TITLE_BASE_TRIAL とボタン)→START→A01..A12_H の背景・立ち絵・イベント CG・名前・文章・選択肢 (A05_H) を進み、trial_finish の紹介→タイトルに戻る (ivory-run、31000 フレーム)。Chromium でタイトル→本編、SAVE ボタンからセーブ→LOAD でロードして保存したメッセージに戻る |
| b2020_caramel_box_strawberry | [はじカノ 体験版](http://suezou.dyndns.org/dl2020/caramel-box-strawberry/hajikano/hk_trial.zip) | 本編 | Ivory (vnrun) でブランド・注意書き→タイトル (横並びのボタン)→NEW GAME→00_CM_01..09 (アイキャッチの呼び出し、2 択・3 択の選択肢)→02_YN_01→02_YN_01_H01→タイトルに戻る (ivory-run、49000 フレーム)。Chromium でタイトル→本編、SAVE からセーブ→LOAD でロードして保存したメッセージに戻る |

### NonColor (vnrun)

| ID | 作品 | 結果 | メモ |
|---|---|---|---|
| b2015_minato_carnival | [姉小路直子と銀色の死神 体験版](http://suezou.dyndns.org/dl2015/minato-carnival/anekouji/anegin_trial.zip) | 本編 | NonColor (vnrun) でタイトル (縦のボタン)→スタート→A1_151013_0..A1_151016_3 の背景・立ち絵 (ST2 の位置表)・CG・名前欄・文章・ボイス (複数同時も)・選択肢 (4 択・3 択・5 択) を進み、体験版ラストの絵→TL でタイトルに戻る (noncolor-run、36500 フレーム)。Chromium でタイトル→本編、SAVE からセーブ→LOAD でロードして保存したメッセージに戻る |
| b2018_mirai | [宿星のガールフレンド２ 体験版](http://suezou.dyndns.org/dl2018/mirai/syukugar2/syukugar2_trialedition.zip) | 本編 | NonColor (vnrun) でタイトル (sys/title の絵とボタン)→START→B01A_180502_0..B02B_180728_0 の背景・立ち絵 (顔 + 体の合成)・CG・名前・文章・ボイス・選択肢 (3 択・2 択) を進み、B07B_180927_1→X02A のおまけトーク (rand のガチャ分岐)→予約する→バナー→TL でタイトルに戻る (noncolor-run、23000 フレーム)。Chromium でタイトル→本編、SAVE からセーブ→LOAD でロードして保存したメッセージに戻る |

### AZ System (vnrun)

| ID | 作品 | 結果 | メモ |
|---|---|---|---|
| b2006_alcot | [Triptych -トリプティック- 体験版](http://suezou.dyndns.org/dl2006/alcot/triptych/tri_trial100.exe) | 本編 | AZ System (vnrun) でタイトル (title_bg_tr とボタン 6 個)→Prologue (tr_01) の背景 (縦長の絵のパン)・立ち絵 (体 + 目 + 口の合成)・名前・文章・ボイス・BGM・SE を最後まで進めてタイトルに戻る。Digest1 (tr_02)・Digest2 (tr_03)・H scene (tr_04) も最後まで (azsys-run、各 9000〜18000 フレーム)。Chromium でタイトル→Prologue、SAVE からセーブ→LOAD でロードして保存したメッセージに戻る |

### PJADV (vnrun)

| ID | 作品 | 結果 | メモ |
|---|---|---|---|
| b2006_unisonshift | [ななついろ★ドロップス 体験版](http://suezou.dyndns.org/dl2006/unisonshift/nanatsuiro/nanatsuiro_trial.zip) | 本編 | PJADV (vnrun) でロゴ→注意書き→タイトル (はじめから)→体験版の挨拶・予告→本編の背景・立ち絵・顔・イベント絵 (拡大と移動)・名前・文章・ボイス・BGM・SE を最後まで進めてタイトルに戻る (pjadv-run、スキップで 4357 メッセージ)。窓の SAVE / LOAD から自前のスロットでセーブ→ロードして同じメッセージに戻る (ネイティブと Chromium) |

### Eomaia (vnrun)

| ID | 作品 | 結果 | メモ |
|---|---|---|---|
| b2023_fg_remake | [夜勤病棟 リメイク 体験版](http://suezou.dyndns.org/dl2023/fg-remake/yakin_re/yakin_re_trial.zip) | 本編 | Eomaia (vnrun) でロゴ→注意書き→タイトル (はじめから)→プロローグ・各話の背景・立ち絵・顔・イベント絵・名前・文章・ボイス・BGM・SE・選択肢→マップ (場所とキャラのアイコン、日・時間の計算はゲームの map / statuswnd ライブラリのまま) で 2 日分を回り→体験版の終わりの絵→タイトルに戻る (eomaia-run、410 メッセージ)。右クリック / Esc の自前スロットでセーブ→ロードして同じメッセージに戻る (ネイティブと Chromium) |

### ShiinaRio (vnrun)

| ID | 作品 | 結果 | メモ |
|---|---|---|---|
| b2011_guilty | [種憑け村 序章体験版](http://suezou.dyndns.org/dl2011/guilty/tanetsukemura/tanetsukemura_taiken.zip) | 本編 | ShiinaRio (vnrun) でタイトル (SYSTITLE.S25 の絵とボタン)→はじめから→A001..A017 の背景・立ち絵・名前・文章・音声を最後まで通り、体験版の終わりでタイトルに戻る (shiina-run)。Chromium でタイトル→本編、右クリックのメニューからセーブ→タイトルのつづきからロードで本編に戻る |
| b2017_onomatope_raspberry | [ビッチ姉妹が清純なはずがないっ！！ 体験版](http://suezou.dyndns.org/dl2017/onomatope-raspberry/bitch_sister/bitchsister_trial_101.zip) | 未確認 |  |
| b2017_onomatope_raspberry_gakuen | [ビッチ学園が清純なはずがないっ！！？ 体験版](http://suezou.dyndns.org/dl2017/onomatope-raspberry/bitch_gakuen/bitchigakuen_trial.zip) | 未確認 |  |
| s2008_alcot_engagekinks | [エンゲージリンクス　体験版](http://suezou.dyndns.org/dl2008/alcot/engagekinks/el_trial.exe) | 未確認 |  |
| s2008_guilty_wana2 | [輪罠Ⅱ 体験版](http://suezou.dyndns.org/dl2008/guilty/wana2/wana2trial.exe) | 未確認 |  |
| s2009_ensemble_hanaotome | [花と乙女に祝福を　体験版](http://suezou.dyndns.org/dl2009/ensemble/hanaotome/HANAOTO_TRIAL.EXE) | 未確認 |  |
| s2022_onomatope_raspberry_yorozuya | [勇者を引退してよろず屋をオープンしたけど、なぜかアダルトグッズに人気が集中している件。体験版](http://suezou.dyndns.org/dl2022/onomatope-raspberry/yorozuya/yorozuya_trial.zip) | 未確認 |  |

### NekoSDK / Waffle (vnrun)

| ID | 作品 | 結果 | メモ |
|---|---|---|---|
| b2010_waffle | [裸の王様 体験版](http://suezou.dyndns.org/dl2010/waffle/hadaka/hadaka_taiken.zip) | 本編 | zip の中のセットアップ.EXE は LHa の SFX で、7z で展開すると .pak が直に出る (800x600)。NekoSDK (vnrun) でタイトル (obj\a000 の机、c001 の 5 列のボタン)→始めから→scr.pak の ep_00_00_* を名前順に本編の背景・立ち絵・名前・文章 (neko-run 8000 フレーム)。Chromium でタイトル→本編、Esc のメニューからセーブ→ロードで保存した行に戻る |
| b2012_waffle | [すくみず食べ放題 体験版](http://suezou.dyndns.org/dl2012/waffle/sukumizu/sukumizu_taiken.exe) | 本編 | NekoSDK (vnrun) でタイトル (d001 の丸いボタン、d000_101 のロゴ)→はじめから→cfg\chapter_file.ini の最初の脚本の背景・CG・名前・文章 (neko-run 8000 フレーム)。Chromium でタイトル→本編、Esc のメニューからセーブ→ロードで保存した行に戻る |
| b2014_waffle | [巨乳ファンタジー２if 体験版](http://suezou.dyndns.org/dl2014/waffle/kyonyu2if/kyonyu2if_taiken.exe) | 本編 | NekoSDK (vnrun) でタイトル (anime の背景・ロゴ)→はじめから→scr.pak の ep_* を名前順に本編の背景・立ち絵・顔窓・名前・文章 (neko-run 15000 フレーム)。Chromium でタイトル→本編、Esc のメニューからセーブ→ロードで保存した行に戻る |
| b2016_waffle | [巨乳ファンタジー外伝２after -オスタシアの野望- 体験版](http://suezou.dyndns.org/dl2016/waffle/kyonyu_if2/kyonyu_gaiden2after_taiken.exe) | 本編 | NekoSDK (vnrun) でタイトル (d007 の 10 列のボタン、d003 の絵、d005 のロゴ)→Start→flow00_000 の順に本編の背景・立ち絵・名前・文章 (neko-run 8000 フレーム)。Chromium でタイトル→本編、Esc のメニューからセーブ→ロードで保存した行に戻る |
| b2017_waffle | [隣人に壊されていく俺の妻 体験版](http://suezou.dyndns.org/dl2017/waffle/rinzin/rinjin_taiken.exe) | 本編 | NekoSDK (vnrun) で文字の序章→タイトル (obj の背景・ロゴ・ボタン)→はじめから→flow00_000 の順に本編の背景・立ち絵・CG (拡大・移動)・名前・文章 (neko-run 6000 フレーム)。Chromium でタイトル→本編、Esc のメニューからセーブ→ロードで保存した行に戻る |
| s2011_waffle_sex_friend | [都合のよいセックスフレンド？体験版](http://suezou.dyndns.org/dl2011/waffle/sex-friend/sex_friend_taiken.zip) | 未確認 |  |

### DxLib MED (vnrun)

| ID | 作品 | 結果 | メモ |
|---|---|---|---|
| b2012_fudegaki_soft | [つぼい君のスイッチ！ 体験版](http://suezou.dyndns.org/dl2012/fudegaki-soft/tsuboi/TSUBOIKUN_NO_SWITCH_trial.zip) | 本編 | DxLib MED (vnrun) で 1024x768 のロゴ→タイトル (CBLMAP_TITLE_MENU)→はじめから→体験版メニュー (CBLMAP_TRIAL_MENU)→P_001→R_001 の背景・CG・名前・文章 (med-run 6000 フレーム)。Chromium でタイトル→本編、Esc のメニューからセーブ→ロードで保存した行に戻る |
| b2012_trois | [モンスターパーク２ ～神々を宿した乙女～ 体験版](http://suezou.dyndns.org/dl2012/trois/mop2/MOP2_TRIAL.zip) | 本編 | DxLib MED (vnrun) でロゴ→タイトル (TITLE のクリックマップ)→START→M2TMAIN→MOP2-T のプロローグ (md_jpg の絵、立ち絵・名前・文章)→選択肢→MA02XX (med-run 8000 フレーム)。Chromium でタイトル→本編、Esc のメニューからセーブ→ロードで保存した行に戻る |
| b2018_fudegaki_soft | [催眠学園１年生 体験版](http://suezou.dyndns.org/dl2018/fudegaki-soft/saimin1nen/SG1nensei_trial.zip) | 本編 | DxLib MED (vnrun) で注意書き→ロゴ・ブランドコール→タイトル (STITLE のクリックマップ、MENU03_N/F)→NEW GAME→SMAIN から 000_PRO000A..E の背景・立ち絵 (体 + 顔)・名前・文章・選択肢・ボイス・BGM (med-run 20000 フレーム)。Chromium でタイトル→本編、Esc のメニューからセーブ→ロードで保存した行に戻る |

### Liar-soft XFL (vnrun)

| ID | 作品 | 結果 | メモ |
|---|---|---|---|
| b2019_channel_ubai | [アイドル裏営業～汚されたステージ衣装～ 体験版](http://suezou.dyndns.org/dl2019/channel-ubai/idol_ura/idol_ura-trial.zip) | 本編 | Liar-soft XFL (vnrun) でタイトル (0001.gsc のボタン)→はじめから→操作説明の絵→0500→1010..1030 の背景・イベント CG・立ち絵・名前・文章・ボイス・BGM (liar-run 9000 フレーム)。Chromium でタイトル→本編、Esc のメニューからセーブ→ロードで保存した行に戻る |
| b2022_evoll | [南国プリズン 体験版](http://suezou.dyndns.org/dl2022/evoll/nangoku_prison/Nangoku_Trial.zip) | 本編 | Liar-soft XFL (vnrun) でタイトル (0001.gsc のボタン、ホバー絵)→はじめから→注意書き 9901..9903→0500→1010..1040 の背景・イベント CG・立ち絵 (体 + 顔、サブルーチン)・名前・文章・ボイス・BGM・選択肢→3010 (liar-run 9000 フレーム)。Chromium でタイトル→本編、Esc のメニューからセーブ→ロードで保存した行に戻る |

### CrossOver Adv.exe (vnrun)

| ID | 作品 | 結果 | メモ |
|---|---|---|---|
| b2009_crossover | [美乳淫妹 遥「私にイヤらしいコスプレさせるなんて……この変態！」](http://suezou.dyndns.org/dl2009/crossover/haruka/bnharuka_trial.zip) | 本編 | CrossOver Adv.exe (vnrun) でロゴ→注意書き→タイトル (SYSMENU.SCD のホットスポットとホバー絵)→始めから→OP_01..OP_04 の背景・立ち絵 (形の画像)・イベント CG・名前・文章・ボイス・効果音・BGM・選択肢 2 回→体験版の終わりでタイトルへ (crossover-run 9000 フレーム、スキップ)。Chromium でタイトル→本編、Esc のメニューからセーブ→タイトルの「続きから」でロードし保存した行に戻る |

### Propeller MPK (vnrun)

| ID | 作品 | 結果 | メモ |
|---|---|---|---|
| b2007_propeller | [『Bullet Butlers』Web体験版　ver1.1](http://suezou.dyndns.org/dl2007/propeller/bulletbutlers/BB_Trial.exe) | 本編 | Propeller MPK (vnrun) でロゴ→タイトル→本編 co001〜co007 (背景・立ち絵・イベント CG・名前・ルビ・ボイス・BGM・選択肢 3 回、体験版で塞がれた選択肢は押しても進まない)→体験版の終わりでタイトルへ (propeller-run、スキップ)。Chromium でタイトル→本編、Esc のメニューからセーブ→タイトルの LOAD でロードし保存した行に戻る |
| s2011_propeller_sukima | [すきま桜とうその都会　体験版２](http://suezou.dyndns.org/dl2011/propeller/sukima/sukima_trial2.zip) | 本編 | Propeller MPK (vnrun、2011 の幅の広いスクリプト形式、1024x576) で注意書き→序章 (カットイン窓・名前・ルビ・BGM)→タイトル→はじめから→sa_01〜sa_10 (選択肢多数)→体験版の終わりでタイトルへ、「体験版2」の選択画面から鈴ルートの抜粋も最後まで (propeller-run、スキップ)。Chromium で序章→タイトル→本編、Esc のメニューからセーブ→タイトルの「続きから」でロードし保存した行に戻る |
| dl_VJ01002202 | [Bullet Butlers](https://trial.dlsite.com/professional/VJ01003000/VJ01002202_trial.zip) | 対象外 | PlayDRM の製品版の体験版。PlayDRM の作品は vnbox の Wine で正規の認証を通して遊び、vnrun では動かさない (データが素の MPK でも) |

### SakanaGL (vnrun)

| ID | 作品 | 結果 | メモ |
|---|---|---|---|
| dl_VJ01002468 | [異世界娘と秘密のコンカフェえっち](https://trial.dlsite.com/professional/VJ01003000/VJ01002468_trial.zip) | 本編 | SakanaGL (vnrun、pkg/*.sx と .sxstorage を自前で展開し .ui のビューと .ks の脚本を実行) でタイトル→Start→本編 (背景・名前・メッセージ窓・BGM・ボイス)。Chromium でタイトル→本編、クイックセーブ / セーブ欄に保存→ページを開き直してタイトルの Continue でロードし保存したページに戻る |
| dl_VJ01004064 | [俺の電脳SEX性活 ‐ビッチな君となんちゃって恋愛‐](https://trial.dlsite.com/professional/VJ01005000/VJ01004064_trial.zip) | 本編 | SakanaGL (vnrun、pkg/*.sx と .sxstorage を自前で展開し .ui のビューと .ks の脚本を実行) でタイトル→Start→本編 (背景・メッセージ窓・BGM)。Chromium でタイトル→本編、クイックセーブ / セーブ欄に保存→ページを開き直してタイトルの Continue でロードし保存したページに戻る |
| r1_sakana | [でか乳巫女と巨乳シスター 体験版](http://calcite-soft.com/momohanasoft/mikosister/image/dekamikosistry.zip) | 本編 | SakanaGL (vnrun、pkg/*.sx と .sxstorage を自前で展開し .ui のビューと .ks の脚本を実行) でタイトル→Start→本編 (背景・立ち絵・表情・名前・メッセージ窓・BGM・ボイス・選択肢)。Chromium でタイトル→本編、クイックセーブ / セーブ欄に保存→ページを開き直してタイトルの Continue でロードし保存したページに戻る |
<!-- /matrix -->
