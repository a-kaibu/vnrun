# 実物の作品での動作確認

vnbox に登録されている作品 (vnbox で Wine 上の動作を確かめた 690 本。すべて体験版で、配布元の URL が vnbox の `results/batch/recheck_all.tsv` にある) を vnrun で動かした記録。vnrun に実装があるエンジンの作品を対象にする。

## やり方

- 対象の一覧は `tools/verify/targets.tsv` (id、vnrun のエンジン、作品名、URL)。vnbox の台帳のエンジン名から引いたもの。エンジン名が空の作品 (約 200 本) はまだ入れていない。
- `tools/verify/verify.sh all` が 1 本ずつ「配布元から取得 → 展開 → `vnrun-prep` → `vnrun-devserver` → ヘッドレス Chromium (`tools/shot.js`) で起動 → スクリーンショット → 削除」を回す。45 秒後と 105 秒後に撮り、画面の中央をクリックしてもう 1 枚撮る。
- スクリーンショットを目で見て `tools/verify/status.tsv` に結果を書き、`python3 tools/verify/matrix.py` で下の表を作る。
- 本編まで進めるのは、タイトルのボタンの位置を見て `verify.sh run <id> <steps>` で個別に。
- 環境: クラウドのコンテナ (4 コア、GPU なし)。Chromium の WebGPU は SwiftShader (Vulkan) で動かしているので、実機より大幅に遅い。起動に時間がかかる作品は「要再確認」にして待ち時間を延ばして撮り直す。
- RealLive は文字を描くフォントが要る。`web/public/fonts/` に IPAex ゴシック / 明朝を置いて確かめた (docs/protocol.md の「フォント」)。

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

## 結果

<!-- matrix -->
| エンジン | 本数 | 本編 | タイトル | 起動 | 要再確認 | NG | 対象外 | 未確認 |
|---|---|---|---|---|---|---|---|---|
| SiglusEngine (siglus_rs) | 19 | 5 | 14 |  |  |  |  |  |
| RealLive (siglus_rs) | 8 | 4 | 2 |  | 1 |  | 1 |  |
| BGI (ethornell-rs) | 21 | 1 | 19 |  |  | 1 |  |  |
| Softpal (sena-rs) | 28 | 3 | 19 |  |  | 6 |  |  |
| Artemis (art3m1s-core) | 6 |  | 5 |  |  | 1 |  |  |
| NScripter (ONScripter-Yuri) | 5 |  | 2 |  |  | 1 | 2 |  |
| HTML (TyranoScript、RPGツクール MV/MZ) | 33 | 1 | 6 | 10 | 3 | 13 |  |  |
| 吉里吉里 (KiriKiri SDL2) | 135 |  | 2 |  | 1 | 9 |  | 123 |
| YU-RIS (vnrun) | 29 |  | 6 | 1 | 1 | 5 | 1 | 15 |

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
| s2014_hadashi_3pl | [3Ping Lovers!☆一夫二妻の世界へようこそ♪　体験版](http://suezou.dyndns.org/dl2014/hadashi/3pl/trial.zip) | 本編 | 最初の選択肢まで |
| s2008_sagaplanets_cominghumming | [Coming×Humming!! -カミング・ハミング- （体験版）](http://suezou.dyndns.org/dl2008/sagaplanets/cominghumming/ComingHumming_Trial.zip) | タイトル |  |
| s2009_hadashi_cool | [素直くーる体験版](http://suezou.dyndns.org/dl2009/hadashi/cool/trial.lzh) | タイトル |  |
| s2010_sagaplanets_kisaragi | [キサラギGOLD★STAR](http://suezou.dyndns.org/dl2010/sagaplanets/kisaragi/kisaragi_goldstar_trial.zip) | 要再確認 | 展開後にエンジンの印のファイル (Seen.txt) が見つからない |
| r1_reallive_five | [5 -ファイブ- 体験版](http://ram.product.co.jp/5/DL/Five_trial_setup.exe) | 対象外 | インストーラ形式 (展開できない) |

### BGI (ethornell-rs)

| ID | 作品 | 結果 | メモ |
|---|---|---|---|
| s2016_hooksoft_amenitys_life | [AmenitysLife Ver1.00](http://suezou.dyndns.org/dl2016/hooksoft/amenitys_life/AmenitysLife_trial_02.zip) | 本編 | c27e445 で直った。最初の文章が出る |
| b2006_lumpofsugar | [いつか、届く、あの空に。【体験版】](http://suezou.dyndns.org/dl2006/lumpofsugar/itsusora/itsusora_trial.lzh) | タイトル |  |
| b2010_lumpofsugar | [Hello,good-bye 体験版](http://suezou.dyndns.org/dl2010/lumpofsugar/hello_good-bye/Hello_goodbye_trial_web.zip) | タイトル |  |
| b2011_lumpofsugar | [DiamicDays_Trial.zip](http://suezou.dyndns.org/dl2011/lumpofsugar/diamicdays/DiamicDays_Trial.zip) | タイトル |  |
| b2013_lumpofsugar | [sekachu_trial_WEB2.zip](http://suezou.dyndns.org/dl2013/lumpofsugar/sekachu/sekachu_trial_WEB2.zip) | タイトル | c27e445 で直った |
| b2015_hooksoft | [トラベリングスターズ 体験版02 Ver1.00](http://suezou.dyndns.org/dl2015/hooksoft/traveling_stars/travelingstars_trial_02.zip) | タイトル | c27e445 で直った |
| b2015_lump_of_sugar | [rensou_trial_WEB.zip](http://suezou.dyndns.org/dl2015/lump-of-sugar/rensourelation/rensou_trial_WEB.zip) | タイトル | 3b42c00 で解消 |
| b2019_hooksoft | [どっちのｉが好きですか？　Ｈシーン体験版　Ver1.00](http://suezou.dyndns.org/dl2019/hooksoft/isuki/isuki_h_trial.zip) | タイトル | c27e445 で直った |
| b2019_sukerasparo | [kudan_trial.zip](http://suezou.dyndns.org/dl2019/sukerasparo/kudan/kudan_trial.zip) | タイトル |  |
| b2020_sukerasomero | [oshilove_trial.zip](http://suezou.dyndns.org/dl2020/sukerasomero/oshi_love/oshilove_trial.zip) | タイトル | インストール用の目印ファイルを無視する 3b42c00 で解消 |
| dl_VJ01004381 | [顔のない月 -待宵の双椿-](https://trial.dlsite.com/professional/VJ01005000/VJ01004381_trial.zip) | タイトル | 4c341a3 で直った (体験版の印のファイル NSM_MYST_trial を消して確認) |
| r1_bgi_kudan | [クダンノフォークロア 体験版](http://sukerasparo.com/data/kudan_trial.zip) | タイトル | 本編まで進むことは前回確認済み (handoff) |
| s2010_lumpofsugar_prismrhythm | [PrismRhythm-プリズムリズム- Web特別体験版](http://suezou.dyndns.org/dl2010/lumpofsugar/prismrhythm/PrismRhythm_trial_Web.lzh) | タイトル | 画面が拡大されず左上に小さく出る (800x600 の作品) |
| s2011_lumpofsugar_gackoh | [学☆王 - THE ROYAL SEVEN STARS - 体験入学版](http://suezou.dyndns.org/dl2011/lumpofsugar/gackoh/Gackoh_Trial.zip) | タイトル |  |
| s2014_hooksoft_meltymoment | [MeltyMoment 体験版 Ver1.00](http://suezou.dyndns.org/dl2014/hooksoft/meltymoment/MeltyMoment_trial.zip) | タイトル | c27e445 で直った |
| s2015_smee_pure_connect | [ピュア×コネクト  Ver1.00](http://suezou.dyndns.org/dl2015/smee/pure_connect/pureconnect_trial.zip) | タイトル | c27e445 で直った |
| s2019_lump_of_sugar_wakabairono_quartet | [若葉色のカルテット 体験版](http://suezou.dyndns.org/dl2019/lump-of-sugar/wakabairono_quartet/WakabaironoQuartet_trial.zip) | タイトル | 3b42c00 で解消 |
| s2020_hooksoft_houkago | [放課後シンデレラ 体験版 Ver1.00](http://suezou.dyndns.org/dl2020/hooksoft/houkago/houkago_trial.zip) | タイトル | c27e445 で直った |
| s2021_smee_pureconnect_hd | [pureconnect_fhd_trial.zip](http://suezou.dyndns.org/dl2021/smee/pureconnect_hd/pureconnect_fhd_trial.zip) | タイトル | 3b42c00 で解消 |
| s2023_quince_soft_little_proncess_go | [リトルプリンセスＧＯ！ 体験版](http://suezou.dyndns.org/dl2023/quince-soft/little-proncess_go/LittlePrincessGO_TR.zip) | タイトル | インストール用の目印ファイルを無視する 3b42c00 で解消 |
| s2022_hooksoft_houkago2 | [放課後シンデレラ２ 体験版 Ver1.01](http://suezou.dyndns.org/dl2022/hooksoft/houkago2/HoukagoCinderella2_trial_v1.01.zip) | NG | f08fa25 でも起動から60秒以内に「サポートされていないパラメータ番号 [0x8E]」。背景はタイトル前の画像 (host_runtime_diag:15:1931:5475:144:56:8) |

### Softpal (sena-rs)

| ID | 作品 | 結果 | メモ |
|---|---|---|---|
| b2024_milimili | [ちょっとナマイキなトイレの花子さん ～えっちぃ学園のおばけとエロエロ搾精性活～ 【体験版】](http://suezou.dyndns.org/dl2024/milimili/hanako/hanako_try.zip) | 本編 | プロローグの文章 (r1_softpal_hanako と同じ作品) |
| r1_softpal_hanako | [トイレの花子さん 体験版](https://amusecraft.com/milimili/try/hanako_try.zip) | 本編 | プロローグの文章 |
| s2015_erectlip_maou | [巨乳魔王の敗北 魔王の挑戦](http://suezou.dyndns.org/dl2015/erectlip/maou/maou_try.zip) | 本編 | 最初の文章 |
| b2017_unisonshift | [あなたに恋する恋愛ルセット　【体験版】](http://suezou.dyndns.org/dl2017/unisonshift/recette/recette_try.zip) | タイトル |  |
| b2018_erectlip | [爆乳催眠女教師　【体験版】](http://suezou.dyndns.org/dl2018/erectlip/bakusai/bakusai_try.zip) | タイトル |  |
| b2020_erectlip | [爆乳淫辱霊媒師　【体験版】](http://suezou.dyndns.org/dl2020/erectlip/injyoku_reibaishi/injyoku_reibaishi_try.zip) | タイトル |  |
| b2020_milimili | [shin-neko ～再会した妹との新たな関係～　【体験版】](http://suezou.dyndns.org/dl2020/milimili/shin_neko/shin-neko_try.zip) | タイトル |  |
| b2021_milimili | [ちんかもツインズ２！　～夏だ！海だ！ふたごの妹おっぱい対決だ！～　【体験版】](http://suezou.dyndns.org/dl2021/milimili/chinkamo2/chinkamo2_try.zip) | タイトル |  |
| b2022_amuse_craft_erotica | [黒ギャル子ちゃんとしっぽり温泉旅行 ～Hしまくり三泊四日の旅～　【体験版】](http://suezou.dyndns.org/dl2022/amuse_craft-erotica/kurogal_onsen/kurogal_onsen_try.zip) | タイトル |  |
| b2025_erectlip | [爆乳淫乱ビーチ　体験版](http://suezou.dyndns.org/dl2025/erectlip/inran_beach/inran_beach_try.zip) | タイトル | 654935e / 8c223b7 でタイトルのボタンまで出る |
| b2025_unisonshift | [小日向柚月と初夜したいっ！　【体験版】](http://suezou.dyndns.org/dl2025/unisonshift/kohinata/kohinata_try.zip) | タイトル |  |
| b2026_crystalia | [ディメンション凸ラバース!! 【体験版】](http://suezou.dyndns.org/dl2026/crystalia/totsu_lover/totsulover_try.zip) | タイトル |  |
| dl_VJ01002655 | [爆乳メイド調教 ～夜姦緊縛教育～](https://trial.dlsite.com/professional/VJ01003000/VJ01002655_trial.zip) | タイトル | 654935e 以降でタイトルまで |
| dl_VJ01004424 | [爆乳淫乱ビーチ](https://trial.dlsite.com/professional/VJ01005000/VJ01004424_trial.zip) | タイトル | 654935e / 8c223b7 でタイトルのボタンまで出る (b2025_erectlip と同じ作品) |
| s2013_erectlip_katekyo | [爆乳淫乱家庭教師　【体験版】](http://suezou.dyndns.org/dl2013/erectlip/katekyo/KATEKYO_try.zip) | タイトル |  |
| s2014_erectlip_furyou | [学園の生贄――慰み者と化した巨乳不良少女 ～白濁に侵される褐色＆堕肉の狂宴～【体験版】](http://suezou.dyndns.org/dl2014/erectlip/furyou/furyou_try.zip) | タイトル |  |
| s2014_erectlip_gyokumajyo | [逆転魔女裁判－痴女な魔女に裁かれちゃう－　【体験版】](http://suezou.dyndns.org/dl2014/erectlip/gyokumajyo/gyakumajyo_try.zip) | タイトル | 654935e / 8c223b7 でタイトルのボタンまで出る |
| s2014_qoobrand_majokoi | [魔女こいにっき 体験版 (第一弾)](http://suezou.dyndns.org/dl2014/qoobrand/mazyokoi/MAJOKOI_try.zip) | タイトル | 654935e 以降でタイトルまで |
| s2020_amuse_craft_erotica_kinbaku | [緊縛性奴会長 ～カレに知られたワタシの秘密～　【体験版】](http://suezou.dyndns.org/dl2020/amuse_craft-erotica/kinbaku/kinbaku_s_try.zip) | タイトル |  |
| s2022_amuse_craft_erotica_tsumamomi | [つまもみ ～欲求不満の奥さんとなし崩しオイルマッサージ～　【体験版】](http://suezou.dyndns.org/dl2022/amuse_craft-erotica/tsumamomi/tsumamomi_try.zip) | タイトル |  |
| s2025_crystalia_crack_trick | [CRACK≡TRICK!](http://suezou.dyndns.org/dl2025/crystalia/crack_trick/cratri_try.zip) | タイトル |  |
| s2026_amuse_craft_erotica_youkashi | [養花師の背徳愛～純真天使は抱かれて色めく～ 【体験版】](http://suezou.dyndns.org/dl2026/amuse_craft-erotica/youkashi/youkashi_try.zip) | タイトル |  |
| b2011_erectlip | [りとる†びっち ～Little Bitch Girls in Hog Farm～ 体験版](http://suezou.dyndns.org/dl2011/erectlip/littlebitch/LittleBitch_trial.zip) | NG | 最初の命令 sp_set_ex の引数の形が違い何も出ない (unsupported extcall) |
| b2012_erectlip | [inrakuchikan_trial.zip](http://suezou.dyndns.org/dl2012/erectlip/inrakuchikan/inrakuchikan_trial.zip) | NG | 最初の命令 sp_set_ex の引数の形が違い何も出ない (unsupported extcall) |
| b2015_erectlip | [巨乳大魔王のドスケベクエスト ～完全敗北した少年勇者クンＵＣ～　【体験版】](http://suezou.dyndns.org/dl2015/erectlip/dosukebe/dosukebe_try.zip) | NG | panic: pal-vm runtime.rs:14727 is_char_boundary |
| b2023_erectlip | [爆乳遊廓 ～癒され太夫孕ませの夜鶴～　【体験版】](http://suezou.dyndns.org/dl2023/erectlip/bakunyu_yukaku/yukaku_try.zip) | NG | underflow は直ったが、ブラウザでは起動時に panic (raw_vec capacity_overflow) |
| r1_softpal_tenmao | [転性魔王さま 体験版](https://kumanomi-soft.amusecraft.com/tenmao/dl/tenmao_try.zip) | NG | 起動時に panic (capacity overflow) |
| s2023_milimili_onaho_yokai | [巨乳オナホ妖怪と田舎ライフもHも満喫生活　【体験版】](http://suezou.dyndns.org/dl2023/milimili/onaho_yokai/onaho_yokai_try.zip) | NG | 起動時に panic (capacity overflow) |

### Artemis (art3m1s-core)

| ID | 作品 | 結果 | メモ |
|---|---|---|---|
| r1_artemis | [遥かなるニライカナイ 体験版](https://project-navel.com/niraikanai/niraikanai_Trial.zip) | タイトル |  |
| s2020_wonder_fool_maokatsu | [まおかつ！　体験版 - Ver1.1](http://suezou.dyndns.org/dl2020/wonder-fool/maokatsu/maokatsu_webtrial1.1.zip) | タイトル |  |
| s2023_j_studio_reaero_dt | [リアルエロゲシチュエーション！DT　体験版 - Ver1.0.0](http://suezou.dyndns.org/dl2023/j-studio/reaero_dt/reaerodt_Trial2.7z) | タイトル |  |
| s2024_madosoft_selectoblige | [セレクトオブリージュ　体験版 - Ver1.0.0](http://suezou.dyndns.org/dl2024/madosoft/selectoblige/selectoblige_Trial.zip) | タイトル | f508d03 (欠けた include を飛ばす) でタイトルまで |
| s2025_j_studio_galsfiction | [GALSFICTION_Trial.zip](http://suezou.dyndns.org/dl2025/j-studio/galsfiction/GALSFICTION_Trial.zip) | タイトル | blur_k のシェーダーのコンパイルに失敗 (表示には出ていない) |
| s2021_giga_fuyukiss | [フユキス　体験版 - Ver1.0.0](http://suezou.dyndns.org/dl2021/giga/fuyukiss/fuyukiss_trial.zip) | NG | E-Mote の読み込みで止まる (image/fg/tok_0.psb: missing integer field truncated_width) |

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
| dl_RJ01720685 | [アイドルダークダンジョン](https://trial.dlsite.com/doujin/RJ01721000/RJ01720685_trial.zip) | 本編 | クリックで最初の会話まで |
| dl_RJ01604592 | [HEART OVERLOAD! 欲望でつながる戦術部隊](https://trial.dlsite.com/doujin/RJ01605000/RJ01604592_trial.zip) | タイトル | 416970b (IndexedDB) でタイトルまで |
| dl_RJ01612664 | [雪音の仇討ち物語](https://trial.dlsite.com/doujin/RJ01613000/RJ01612664_trial.zip) | タイトル | RPGツクール。画面が拡大されない |
| dl_RJ01642779 | [戦闘補習3～武闘派冒険者、魔術を学ぶ～](https://trial.dlsite.com/doujin/RJ01643000/RJ01642779_trial.zip) | タイトル | RPGツクール。画面が拡大されない |
| dl_RJ01650072 | [どこでもHな世界～地味子のバイト日記～](https://trial.dlsite.com/doujin/RJ01651000/RJ01650072_trial.zip) | タイトル |  |
| dl_RJ01702800 | [ドット勇者](https://trial.dlsite.com/doujin/RJ01703000/RJ01702800_trial.zip) | タイトル |  |
| dl_RJ01706991 | [ショタおねRPG 爆乳お姉さんたちと催〇でドスケベ三昧 爆乳(クソデカボイン)の桃源郷(パラダイス)](https://trial.dlsite.com/doujin/RJ01707000/RJ01706991_trial.zip) | タイトル | RPGツクール |
| dl_RJ01618747 | [スペースパトロール パニカ](https://trial.dlsite.com/doujin/RJ01619000/RJ01618747_trial.zip) | 起動 | 読み込み中の表示のまま進まない |
| dl_RJ01623688 | [まもの博物館](https://trial.dlsite.com/doujin/RJ01624000/RJ01623688_trial.zip) | 起動 | 読み込み中の表示のまま進まない |
| dl_RJ01686418 | [秘密 女騎士NTR](https://trial.dlsite.com/doujin/RJ01687000/RJ01686418_trial.zip) | 起動 | 黒画面のまま (エラーなし) |
| dl_RJ01698597 | [かんもく:場面緘黙女子](https://trial.dlsite.com/doujin/RJ01699000/RJ01698597_trial.zip) | 起動 | 読み込み中の表示のまま進まない |
| dl_RJ01699765 | [星直しのブレイブロード -淫欲の街と堕落の聖女候補-](https://trial.dlsite.com/doujin/RJ01700000/RJ01699765_trial.zip) | 起動 | 読み込み中の表示のまま進まない |
| dl_RJ01701742 | [【Live2D】逃亡中～ミッションをクリアしてザーメンの放出を阻止せよ！～](https://trial.dlsite.com/doujin/RJ01702000/RJ01701742_trial.zip) | 起動 | 読み込み中の表示のまま進まない |
| dl_RJ01702392 | [いとこのお姉ちゃんに睡眠●シミュレーション](https://trial.dlsite.com/doujin/RJ01703000/RJ01702392_trial.zip) | 起動 | 灰色の画面に大きすぎるタイトル文字だけが出て進まない |
| dl_RJ01730521 | [大魔法少女カサネ](https://trial.dlsite.com/doujin/RJ01731000/RJ01730521_trial.zip) | 起動 | 読み込み中の表示のまま進まない |
| dl_RJ01741863 | [淫らに染めて ～叔母が堕ちるまで～](https://trial.dlsite.com/doujin/RJ01742000/RJ01741863_trial.zip) | 起動 | 読み込み中の表示のまま進まない |
| dl_RJ01746541 | [女魔法使いセーラは俺から逃げられない！ ～便利なエロアイテムと孕むまでヤれる封鎖村～](https://trial.dlsite.com/doujin/RJ01747000/RJ01746541_trial.zip) | 起動 | 黒画面のまま (エラーなし) |
| dl_VJ01000810 | [エッチなお姉ちゃんが、褐色巨乳の悪魔だった件](https://trial.dlsite.com/professional/VJ01001000/VJ01000810_trial.zip) | 要再確認 | NW.js のゲームが exe に埋め込まれている (package.nw が無い) |
| dl_VJ01001309 | [エッチなお姉ちゃんが、褐色巨乳の悪魔だった件2 ～新たな悪魔ミリス登場♪～](https://trial.dlsite.com/professional/VJ01002000/VJ01001309_trial.zip) | 要再確認 | NW.js のゲームが exe に埋め込まれている (package.nw が無い) |
| r1_tyrano | [転生した世界で… 体験版](http://www.gocha-mix.com/gp/gxgp01/dl/gxgp001_trial.zip) | 要再確認 | NW.js のゲームが exe に埋め込まれている (package.nw が無い)。exe を zip として展開すれば読めるはず |
| dl_RJ01529614 | [まぐわれ異世界珍道中](https://trial.dlsite.com/doujin/RJ01530000/RJ01529614_trial.zip) | NG | 「Your browser does not support IndexedDB」 |
| dl_RJ01618221 | [アリナと淫魔の呪い](https://trial.dlsite.com/doujin/RJ01619000/RJ01618221_trial.zip) | NG | 「Your browser does not support IndexedDB」 |
| dl_RJ01648691 | [測り難きは下心](https://trial.dlsite.com/doujin/RJ01649000/RJ01648691_trial.zip) | NG | 「Your browser does not support IndexedDB」 |
| dl_RJ01696742 | [サモンスピリッツ](https://trial.dlsite.com/doujin/RJ01697000/RJ01696742_trial.zip) | NG | 「Your browser does not support IndexedDB」 |
| dl_RJ01699647 | [とろぴかる・ちぇいす！長年追っていた指名手配犯をヌーディストビーチで発見したクール系女刑事](https://trial.dlsite.com/doujin/RJ01700000/RJ01699647_trial.zip) | NG | 「Your browser does not support IndexedDB」 |
| dl_RJ01701172 | [ラッキィ・ホール-ハズレは淫魔の搾精穴-](https://trial.dlsite.com/doujin/RJ01702000/RJ01701172_trial.zip) | NG | 「Your browser does not support IndexedDB」 |
| dl_RJ01703802 | [決着はバトルファックで -初回特典付き-](https://trial.dlsite.com/doujin/RJ01704000/RJ01703802_trial.zip) | NG | 「Your browser does not support IndexedDB」 |
| dl_RJ01720493 | [Moeのファンサービスリフト♡～つり出し](https://trial.dlsite.com/doujin/RJ01721000/RJ01720493_trial.zip) | NG | 「Cannot read properties of undefined (reading 'fs')」(NW.js の Node API) |
| dl_RJ01720958 | [魔法戦士 NANAMI 前編](https://trial.dlsite.com/doujin/RJ01721000/RJ01720958_trial.zip) | NG | 「Your browser does not support IndexedDB」 |
| dl_RJ01722380 | [むぎゅっと☆搾精ナイトメア！](https://trial.dlsite.com/doujin/RJ01723000/RJ01722380_trial.zip) | NG | 「Your browser does not support IndexedDB」 |
| dl_RJ01727123 | [監禁部屋から逃げれない1](https://trial.dlsite.com/doujin/RJ01728000/RJ01727123_trial.zip) | NG | 「Your browser does not support IndexedDB」 |
| dl_RJ01731048 | [ヤリステナイメスブター 世界を救った後の ボクの日常『性』活](https://trial.dlsite.com/doujin/RJ01732000/RJ01731048_trial.zip) | NG | 黒画面。「require is not defined」(NW.js の Node API を使う) |
| dl_RJ357058 | [Last12 - 勇者の恥ずかしい育児日記 -](https://trial.dlsite.com/doujin/RJ358000/RJ357058_trial.zip) | NG | 「Unexpected end of JSON input」 |

### 吉里吉里 (KiriKiri SDL2)

| ID | 作品 | 結果 | メモ |
|---|---|---|---|
| b2006_ninetail | [機械仕掛けのイブ　～Ｄｅａ　Ｅｘ　Ｍａｃｈｉｎａ～](http://suezou.dyndns.org/dl2006/ninetail/eve/0527_eve_trial_ver100.exe) | タイトル | 83905b1 / f086ac2 / ce3775e でタイトルまで。効果音の ogg は「Unknown wave format」で鳴らない (続行はできる) |
| b2021_at_sakura | [愛する彼女は画面の中、他の男の上で腰をふる 体験版](http://suezou.dyndns.org/dl2021/at-sakura/ai_gamen/aspc-0123_taiken_ver1.0.zip) | タイトル | 色がおかしい (肌が青い、赤と青が入れ替わっている?)。効果音と BGM の ogg が鳴らない |
| r1_kag3 | [KAG3 紹介デモ](https://krkrz.github.io/download/kr2_232r2.zip) | 要再確認 | KAG3 のサンプル (xp3 にまとめていない data フォルダ)。開発スレッドでは確認済み |
| b2011_grand_cru | [はっぴぃふぁくとりー　体験版](http://suezou.dyndns.org/dl2011/grand-cru/hf/hp_trial.zip) | NG | kagparser.tjs(96) の lines.load (シナリオの読み込み) で失敗 |
| b2013_hulotte | [妹のおかげでモテすぎてヤバい。 体験版](http://suezou.dyndns.org/dl2013/hulotte/moteyaba/moteyaba_trial.zip) | NG | xp3 が暗号化されている (plugin/moteyaba.tpm) |
| b2015_ammolite | [お兄ちゃん、教えて……～義妹とヒミツの保健実習～　体験版　Ver1.00](http://suezou.dyndns.org/dl2015/ammolite/oni/amml011_tr.zip) | NG | startup.tjs(3) の Scripts.execStorage("startup.tjs") で読む中の startup.tjs が「Cannot convert given narrow string to wide string」 |
| b2015_appetite | [愛娘は、マゾ性隷～娘だからって遠慮しないでね？～　体験版　Ver1.00](http://suezou.dyndns.org/dl2015/appetite/musume/apt054_tr.zip) | NG | startup.tjs(3) の Scripts.execStorage("startup.tjs") で読む中の startup.tjs が「Cannot convert given narrow string to wide string」 |
| b2016_apadash | [満淫オーク電車～くっ！　任務じゃなければこんな場所…～　体験版　Ver1.00](http://suezou.dyndns.org/dl2016/apadash/oak_train/aptd005_tr.zip) | NG | startup.tjs(3) の Scripts.execStorage("startup.tjs") で読む中の startup.tjs が「Cannot convert given narrow string to wide string」 |
| b2016_cube | [間宮くんちの五つ子事情 体験版](http://suezou.dyndns.org/dl2016/cube/mamiya/mamiya_trial.zip) | NG | xp3 が暗号化されている (plugin/mamiya.tpm) |
| b2022_syoku | [触災の島～Island of the Dead～体験版　Ver1.00](http://suezou.dyndns.org/dl2022/syoku/syokusai_island/Syokusai_Island_tr.zip) | NG | startup.tjs から読むスクリプトが「Invalid character '\x16'」(r1_krkr_apt306 と同じ) |
| b2025_monoceros_plus | [Vanilla Android　シコ猿DT大学生の俺と美少女アンドロイドがいろんなプレイでハメパコミッション　体験版](http://suezou.dyndns.org/dl2025/monoceros_plus/vanilla_android/VanillaAndroid-trial.zip) | NG | WindowResizable.ks(85) の win.registerExEvent() で止まる (windowEx プラグイン相当が無い) |
| r1_krkr_apt306 | [先輩、好きです……押忍！ 体験版](https://www.appetite-game.com/apt306/image/apt306_tr.zip) | NG | startup.tjs から読むスクリプトが「Invalid character '\x16'」(独自の暗号化か) |
| b2006_akabeesoft | [その横顔を見つめてしまう ～a profile完全版～ web体験版](http://suezou.dyndns.org/dl2006/akabeesoft/sonoyoko/sonoyoko_trial.zip) | 未確認 |  |
| b2006_mille_feuille | [ソウルレイザー☆ユナ 体験版](http://suezou.dyndns.org/dl2006/mille-feuille/yuna/yuna_taiken.zip) | 未確認 |  |
| b2007_akabeesoft | [g_web_taiken.zip](http://suezou.dyndns.org/dl2007/akabeesoft/gsen/g_web_taiken.zip) | 未確認 |  |
| b2008_akabeesoft | [ＷＬＯ-世界恋愛機構- web用体験版](http://suezou.dyndns.org/dl2008/akabeesoft/wlo/wlo_webtrial.zip) | 未確認 |  |
| b2008_asa_project | [Hime のち Honey - 体験版](http://suezou.dyndns.org/dl2008/asa-project/himehoney/hime_web_trial1.zip) | 未確認 |  |
| b2008_dualtail | [Venus Blood Chimera](http://suezou.dyndns.org/dl2008/dualtail/vbc/VBC_trial_SLGver100.zip) | 未確認 |  |
| b2008_mille_feuille | [魔法少女沙枝 Vol.2 体験版](http://suezou.dyndns.org/dl2008/mille-feuille/sae2/sae2_trial2.zip) | 未確認 |  |
| b2009_dualtail | [魔法少女ラヴィリオン 体験版](http://suezou.dyndns.org/dl2009/dualtail/ravilion/ravi_trial.exe) | 未確認 |  |
| b2010_akabeesoft | [置き場がない！ web体験版](http://suezou.dyndns.org/dl2010/akabeesoft/okiba/okiba_webTRIAL.zip) | 未確認 |  |
| b2010_grand_cru | [Reunion～３日間だけのプチ同棲～　体験版](http://suezou.dyndns.org/dl2010/grand-cru/reunion/Reunion_trial.lzh) | 未確認 |  |
| b2011_nanawind | [ユユカナ 体験版](http://suezou.dyndns.org/dl2011/nanawind/yuyukana/yuyu_taiken01.zip) | 未確認 |  |
| b2012_akabeesoft3 | [JOKER-死線の果ての道化師-](http://suezou.dyndns.org/dl2012/akabeesoft3/joker/joker_web_trial2.zip) | 未確認 |  |
| b2012_alcot | [中の人などいない！　体験版](http://suezou.dyndns.org/dl2012/alcot/naka/nakanohito_trial.exe) | 未確認 |  |
| b2012_cross_over | [美脚隷嬢優奈「脚もお尻もこんなに穢すなんて……絶対に許さない！」体験版](http://suezou.dyndns.org/dl2012/cross-over/yuna/br_yuuna_trial2.zip) | 未確認 |  |
| b2012_kai | [リヴォルバーガール☆ハンマーレディ　体験版](http://suezou.dyndns.org/dl2012/kai/rghl/RGHLtrial-WEB.zip) | 未確認 |  |
| b2012_parasol | [恋妹SWEET☆DAYS 体験版](http://suezou.dyndns.org/dl2012/parasol/koiimo/koiimo_taiken.zip) | 未確認 |  |
| b2013_cross_over | [花嫁狩凌 ～絶対快楽の白濁淫毒～ 体験版](http://suezou.dyndns.org/dl2013/cross-over/hana/hanayome_trial.zip) | 未確認 |  |
| b2013_dualtail | [VenusBlood-GAIA- WEB専用ADV体験版](http://suezou.dyndns.org/dl2013/dualtail/vbg/VBG_WEB_AVGTrial.zip) | 未確認 |  |
| b2013_madosoft | [ナマイキデレーション体験版 WEB Ver.](http://suezou.dyndns.org/dl2013/madosoft/namadere/namadere_trial_web.zip) | 未確認 |  |
| b2014_hibiki_works | [ＰＲＥＴＴＹ×Ｃ∧ＴＩＯＮ](http://suezou.dyndns.org/dl2014/hibiki-works/pretty_cation/pc_web-trial1.01.zip) | 未確認 |  |
| b2014_undermoon | [奪われた学園～罠に落ちた優等生～　体験版](http://suezou.dyndns.org/dl2014/undermoon/ubaware/ubaware_trial.zip) | 未確認 |  |
| b2015_mille_feuille | [極煌戦姫ミストルティア 体験版](http://suezou.dyndns.org/dl2015/mille-feuille/mistletear/blazingangel_mistletear_trial.zip) | 未確認 |  |
| b2015_nanawind | [ALIA’s CARNIVAL! Flowering Sky　体験版(WEB.ver)](http://suezou.dyndns.org/dl2015/nanawind/alias_fs/ALIAs_CARNIVAL_floweringsky_trial.zip) | 未確認 |  |
| b2015_pachipachi_soft | [おしおき生意気ギャル ～エロいオシオキをしたら自分からエッチを求めてきた～ 体験版](http://suezou.dyndns.org/dl2015/pachipachi-soft/oshioki/oshioki_trial.zip) | 未確認 |  |
| b2016_alcot | [よめがみ My Sweet Goddess! リコＨシーン体験版](http://suezou.dyndns.org/dl2016/alcot/yomegami/yomegami_trial3.exe) | 未確認 |  |
| b2016_appetite | [アナル＊ティーチャー～感度良肛！？　逆レッスン～　体験版　Ver1.00](http://suezou.dyndns.org/dl2016/appetite/anaru_teacher/apt068_tr.zip) | 未確認 |  |
| b2017_appetite | [催眠アプリで寝取りハメ～これまでの恨み、ヤり返してやる！　体験版　Ver1.00](http://suezou.dyndns.org/dl2017/appetite/hypnotism_application/apt098_tr.zip) | 未確認 |  |
| b2017_sukerasparo | [ことのは・アムリラート](http://suezou.dyndns.org/dl2017/sukerasparo/kotonoha/kotonoha_trial.zip) | 未確認 |  |
| b2018_appetite | [P.S.あなたを心から愛しています～最期まで気づけなかった妻の苦しみ～　体験版](http://suezou.dyndns.org/dl2018/appetite/ps_iloveyou/apt125_tr.zip) | 未確認 |  |
| b2019_appetite | [女体化パニック！３～お、俺は男だ！　でも、悔しいけど気持ちいいっ……のか！？　体験版](http://suezou.dyndns.org/dl2019/appetite/nyotaika3/apt133_tr.zip) | 未確認 |  |
| b2019_dualtail | [VenusBlood FRONTIER International SLG Trial 日本語版](http://suezou.dyndns.org/dl2019/dualtail/vbfi/VBFI_New_SLG_Trial.zip) | 未確認 |  |
| b2019_sator | [モウソウスピーカー～Ｈな妄想が聴こえてくるよ～　体験版](http://suezou.dyndns.org/dl2019/sator/mousou_speaker/mousou_tr.zip) | 未確認 |  |
| b2019_tinkle_position | [おにギュＨ＿ＨＣＢ体験版](http://suezou.dyndns.org/dl2019/tinkle-position/onigyu_h/onigyuh_hcbtrial.zip) | 未確認 |  |
| b2020_akabeesoft3 | [墓多 DYING ZOMBIES -体験版-](http://suezou.dyndns.org/dl2020/akabeesoft3/hakatadyingzombies/ab3_hakatadyingzombies_webtrial.zip) | 未確認 |  |
| b2020_at_sakura | [優来（ゆら）があいつに抱かれる理由（わけ） 体験版](http://suezou.dyndns.org/dl2020/at-sakura/yura_wake/aspc-0112_taiken_ver1.0.zip) | 未確認 |  |
| b2020_chaos_r | [悪堕ち 魔導処女～絶対支配の触手調教～　体験版](http://suezou.dyndns.org/dl2020/chaos-r/madousyojo/madousyojo_tr.zip) | 未確認 |  |
| b2020_lass_pixy | [リトルシックガール　～桃蜜は妹の香り～　体験版](http://suezou.dyndns.org/dl2020/lass-pixy/lsg03/LSG3_tr.zip) | 未確認 |  |
| b2020_pin_point | [世話焼き奥さんで人の頼みを断れない亜衣さんにお願いして中出しハメ放題のドスケベ妻になってもらった 体験版](http://suezou.dyndns.org/dl2020/pin-point/wife_ai/PP34_trial.zip) | 未確認 |  |
| b2021_chaos_r | [神聖昂燐ダクリュオン～正しい天使の育てかた～　体験版](http://suezou.dyndns.org/dl2021/chaos-r/dakr/dakr_tr.zip) | 未確認 |  |
| b2021_kelp | [アイドル姉妹をエッチにプロデュース！ 体験版](http://suezou.dyndns.org/dl2021/kelp/idol_simai/IDOL_SIMAI_TR.zip) | 未確認 |  |
| b2021_pin_point | [売国王姫～堕落のメス豚母娘～体験版](http://suezou.dyndns.org/dl2021/pin-point/baikoku_ouhi/PP36_trial.zip) | 未確認 |  |
| b2022_at_sakura | [愛する恋人を大嫌いな旧友に寝取られた件 体験版](http://suezou.dyndns.org/dl2022/at-sakura/ai_tomo/aspc-0125_taiken_ver1.0.zip) | 未確認 |  |
| b2022_chaos_r | [進軍ゴブリン軍団～人類総苗床化作戦～ 体験版](http://suezou.dyndns.org/dl2022/chaos-r/gobblin_gundan/GOBBLIN_TR.zip) | 未確認 |  |
| b2022_hending | [リンパにATATA! ～メス牡蠣ミルクどぴゅらっしゅ♡～](http://suezou.dyndns.org/dl2022/hending/atata/atata_trial.zip) | 未確認 |  |
| b2023_apadash | [最強の人妻ヒロインが寝取られるまで～お願い！夫にはバラさないで…～　体験版　Ver1.00](http://suezou.dyndns.org/dl2023/apadash/hitoduma_heroine/aptd027_tr.zip) | 未確認 |  |
| b2023_appetite | [洗脳航路～エイリアンの苗床に導かれるエリート女軍人たち～　体験版　Ver1.00](http://suezou.dyndns.org/dl2023/appetite/senno_kouro/apt246_tr.zip) | 未確認 |  |
| b2023_at_sakura | [堕とされた義姉 体験版](http://suezou.dyndns.org/dl2023/at-sakura/oti_ane/aspc-0138_taiken_ver1.0.zip) | 未確認 |  |
| b2023_chaos_r | [神聖昂燐エストランジェ体験版](http://suezou.dyndns.org/dl2023/chaos-r/strncxe/strncxe_tr.zip) | 未確認 |  |
| b2023_lump_of_sugar_all | [瑠璃桜](http://suezou.dyndns.org/dl2023/lump-of-sugar_all/rurizakura/Rurizakura_trial.zip) | 未確認 |  |
| b2024_at_sakura | [性調教済だった愛する彼女 体験版](http://suezou.dyndns.org/dl2024/at-sakura/seikyo_kano/aspc-0150_taiken_ver1.0.zip) | 未確認 |  |
| b2025_at_sakura | [学生会長・紫藤喜那の淫鬱 体験版](http://suezou.dyndns.org/dl2025/at-sakura/seito_sidou/aspc-0161_taiken_ver1.0.zip) | 未確認 |  |
| dl_RJ01677912 | [とける風花とシロうさぎ](https://trial.dlsite.com/doujin/RJ01678000/RJ01677912_trial.zip) | 未確認 |  |
| dl_VJ01000429 | [【DLsite限定版】進軍ゴブリン軍団2 ～淫獣軍VS.INTRUDER～](https://trial.dlsite.com/professional/VJ01001000/VJ01000429_trial.zip) | 未確認 |  |
| dl_VJ01000497 | [【DLsite限定版】性感エステ 灯 -AKARI- マキ（28）](https://trial.dlsite.com/professional/VJ01001000/VJ01000497_trial.zip) | 未確認 |  |
| dl_VJ01000502 | [堕とされていくお姉ちゃんと、何もできないボク ～憧れの彼女は、淫落し痴態をさらけ出す～](https://trial.dlsite.com/professional/VJ01001000/VJ01000502_trial.zip) | 未確認 |  |
| dl_VJ01000564 | [VenusBlood GAIA International R-18版](https://trial.dlsite.com/professional/VJ01001000/VJ01000564_trial.zip) | 未確認 |  |
| dl_VJ01000639 | [【デラックス版】調教カテイ ～性開発された肢体は元カレを忘れられない～](https://trial.dlsite.com/professional/VJ01001000/VJ01000639_trial.zip) | 未確認 |  |
| dl_VJ01000679 | [居座りカノジョとのドキドキ同居生活 ～ねぇねぇ、おじさんしたいでしょ♪～](https://trial.dlsite.com/professional/VJ01001000/VJ01000679_trial.zip) | 未確認 |  |
| dl_VJ01001109 | [最強の人妻ヒロインが寝取られるまで ～お願い！夫にはバラさないで…～](https://trial.dlsite.com/professional/VJ01002000/VJ01001109_trial.zip) | 未確認 |  |
| dl_VJ01002065 | [快楽セックスで堕とされていた愛する妻 ～他の男の手で身も心もイキ狂っていく最愛妻](https://trial.dlsite.com/professional/VJ01003000/VJ01002065_trial.zip) | 未確認 |  |
| dl_VJ01003070 | [夏色果実 ～えっちなカノジョと溶け合う心～](https://trial.dlsite.com/professional/VJ01004000/VJ01003070_trial.zip) | 未確認 |  |
| dl_VJ01003347 | [【DLsite限定版】NEW BREEDER ～エッチな獣娘を狩って⾃分好みに調教！～](https://trial.dlsite.com/professional/VJ01004000/VJ01003347_trial.zip) | 未確認 |  |
| dl_VJ01004136 | [早乙女結衣の淫らな隠しごと ～隠されたアイドルのビッチな裏事情～](https://trial.dlsite.com/professional/VJ01005000/VJ01004136_trial.zip) | 未確認 |  |
| dl_VJ01004598 | [女王社長の僕争奪戦 ～カラダもリードも譲らない！ で、どっちのが好きなのっ！？～](https://trial.dlsite.com/professional/VJ01005000/VJ01004598_trial.zip) | 未確認 |  |
| dl_VJ01006612 | [背徳の母乳 ～そんなに出しちゃダメぇ～](https://trial.dlsite.com/professional/VJ01007000/VJ01006612_trial.zip) | 未確認 |  |
| s2006_akabeesoft_conboku | [こんな娘がいたら僕はもう～web体験版第2弾～](http://suezou.dyndns.org/dl2006/akabeesoft/conboku/conboku_taiken_02.zip) | 未確認 |  |
| s2008_akabeesoft_gsen | [G線上の魔王](http://suezou.dyndns.org/dl2008/akabeesoft/gsen/g_trial2.zip) | 未確認 |  |
| s2008_dualtail_clarice | [奴隷将校クラリス　－　白濁のグローリエ　－　ＡＤＶパート体験版](http://suezou.dyndns.org/dl2008/dualtail/clarice/Clarice_ADVtrial.exe) | 未確認 |  |
| s2008_ninetail_amatu | [天ツ風　～傀儡陣風帖～](http://suezou.dyndns.org/dl2008/ninetail/amatu/amatu_trial_0901ver100.exe) | 未確認 |  |
| s2009_dualtail_haiironosora | [灰色の空に堕ちた翼　体験版 Ver.1.00](http://suezou.dyndns.org/dl2009/dualtail/haiironosora/TD06_TRIAL100.exe) | 未確認 |  |
| s2010_akabeesoft_lavender | [光輪の町、ラベンダーの少女　体験版](http://suezou.dyndns.org/dl2010/akabeesoft/lavender/lavender_webTRIAL.zip) | 未確認 |  |
| s2011_grand_cru_imostar | [しゅ～てぃんぐ妹スター☆ ～おにいちゃん、覚悟しててよね♪～　体験版](http://suezou.dyndns.org/dl2011/grand-cru/imostar/imosta_trial.zip) | 未確認 |  |
| s2011_hulotte_with_ribbon | [With Ribbon Hシーン体験版](http://suezou.dyndns.org/dl2011/hulotte/with-ribbon/with-ribbon_trial2.zip) | 未確認 |  |
| s2011_parasol_delivara | [でりばらっ！-deliverance of strays- 体験版](http://suezou.dyndns.org/dl2011/parasol/delivara/delivara_taiken.zip) | 未確認 |  |
| s2012_ninetail_gears_of_dragoon | [Gears of Dragoon  -RPG体験版 ver.1.00-](http://suezou.dyndns.org/dl2012/ninetail/gears_of_dragoon/GoD_RPG_trial_100.zip) | 未確認 |  |
| s2013_hibiki_works_lovely_cation2 | [ＬＯＶＥＬＹ×Ｃ∧ＴＩＯＮ２](http://suezou.dyndns.org/dl2013/hibiki-works/lovely_cation2/LxC2_trial_web.zip) | 未確認 |  |
| s2014_cross_over_tori | [鳥籠の姫 ～絶対妊娠の姉妹悦辱～　体験版](http://suezou.dyndns.org/dl2014/cross-over/tori/torikago_trial1_2.zip) | 未確認 |  |
| s2014_cube_koikano | [恋する彼女の不器用な舞台 体験版](http://suezou.dyndns.org/dl2014/cube/koikano/koikanoTrial.zip) | 未確認 |  |
| s2015_appetite_inbo | [発情淫母～疼く熟体は、その欲情を抑えきれない～　体験版　Ver1.00](http://suezou.dyndns.org/dl2015/appetite/inbo/apt050_tr.zip) | 未確認 |  |
| s2015_hulotte_yomeyaba | [嫁探しが捗りすぎてヤバい。 体験版](http://suezou.dyndns.org/dl2015/hulotte/yomeyaba/yomeyaba_trial.zip) | 未確認 |  |
| s2016_akabeesoft3_inosupe | [生命のスペア](http://suezou.dyndns.org/dl2016/akabeesoft3/inosupe/inosupe_webtrialver.zip) | 未確認 |  |
| s2016_ammolite_little_twins | [りとる☆ついんず　～すぐにトリコにしてあげる♪～　体験版　Ver1.00](http://suezou.dyndns.org/dl2016/ammolite/little_twins/amml012_tr.zip) | 未確認 |  |
| s2016_appetite_husband | [夫に言えない義父との姦淫～嫌なのに忘れられなくなりそう～　体験版　Ver1.00](http://suezou.dyndns.org/dl2016/appetite/husband/apt073_tr.zip) | 未確認 |  |
| s2016_appetite_wawatasi | [わ、わたしを縛りたい？～僕と地味娘の秘密の関係～　体験版　Ver1.00](http://suezou.dyndns.org/dl2016/appetite/wawatasi/apt081_tr.zip) | 未確認 |  |
| s2017_akabeesoft3_tonofuta | [少女と年の差、ふたまわり。](http://suezou.dyndns.org/dl2017/akabeesoft3/tonofuta/syoujyo_to_toshinosa_hutamawari_webtrial.zip) | 未確認 |  |
| s2017_appetite_evil_monmusu | [ゆけゆけ悪のモンむす軍団！～怪人娘に搾り取られるえっちで過酷な世界性服大作戦～　体験版　Ver1.00](http://suezou.dyndns.org/dl2017/appetite/evil_monmusu/apt101_tr.zip) | 未確認 |  |
| s2017_appetite_immorality_don | [背徳の母娘丼～淫欲に沈むママといもうと～　体験版　Ver1.00](http://suezou.dyndns.org/dl2017/appetite/immorality_don/apt090_tr.zip) | 未確認 |  |
| s2017_atelier_gh_neneki | [粘液地獄　～巨大ヌルヌル生物に汚された女学生～ 体験版](http://suezou.dyndns.org/dl2017/atelier-gh/neneki/neneki_TR.zip) | 未確認 |  |
| s2017_nanawind_alice_gram | [春音アリス＊グラム　体験版](http://suezou.dyndns.org/dl2017/nanawind/alice_gram/harugra_trial.zip) | 未確認 |  |
| s2018_appetite_boku_mother | [欲求不満なボクのママのＳＥＸレッスン～パパには言えない家庭内情事～　Trial](http://suezou.dyndns.org/dl2018/appetite/boku_mother/apt106_tr.zip) | 未確認 |  |
| s2018_appetite_shokushu_ouji | [触手王子の肉嫁探し～母と姉弟の終わらない絶頂地獄～　体験版](http://suezou.dyndns.org/dl2018/appetite/shokushu_ouji/apt116_tr.zip) | 未確認 |  |
| s2018_pin_point_yakyurare | [彼女が野球部の性処理マネージャーに…～体育会系セックスで寝取られて～体験版](http://suezou.dyndns.org/dl2018/pin-point/yakyurare/yakyurare_trial.zip) | 未確認 |  |
| s2019_apadash_chuni_saimin | [厨二病が催眠術を使えたら～勘違い転校生のエッチな悪神召喚術～　体験版　Ver1.00](http://suezou.dyndns.org/dl2019/apadash/chuni_saimin/aptd015_tr.zip) | 未確認 |  |
| s2019_appetite_aniyome_furin | [兄嫁に誘われて～義姉がエロ過ぎて家庭内不倫～　体験版　Ver1.00](http://suezou.dyndns.org/dl2019/appetite/aniyome_furin/apt144_tr.zip) | 未確認 |  |
| s2019_appetite_dt_kyoiku | [[2019/05/08 01][mode_title.ks - 164]ママの妹の童貞教育～ひきこもった僕は外出せずに中出しする～　体験版](http://suezou.dyndns.org/dl2019/appetite/dt_kyoiku/apt148_tr.zip) | 未確認 |  |
| s2019_parasol_renran_p | [恋嵐スピリッチュ 体験版](https://trial.dlsite.com/professional/VJ013000/VJ012599_trial.zip http://suezou.dyndns.org/dl2019/parasol/renran-p/renran_trial_101.zip) | 未確認 |  |
| s2019_sukerasparo_itsukano | [いつかのメモラージョよりぬき体験版](http://suezou.dyndns.org/dl2019/sukerasparo/itsukano/itsukano_trial2.zip) | 未確認 |  |
| s2020_at_sakura_lover_swapping | [恋人スワッピング 体験版](http://suezou.dyndns.org/dl2020/at-sakura/lover_swapping/aspc-0107_taiken_ver1.0.zip) | 未確認 |  |
| s2020_monoceros_plus_kanosen | [ボクのカノジョはえっちな先生　体験版](http://suezou.dyndns.org/dl2020/monoceros_plus/kanosen/kanosen_TR.zip) | 未確認 |  |
| s2020_pajamas_ex_paizuri | [最高に都合のいいパイズリ上手のマリーさん－体験版－](http://suezou.dyndns.org/dl2020/pajamas-ex/paizuri/paizuri_tr.zip) | 未確認 |  |
| s2020_sagaplanets_kakenuke | [かけぬけ★青春スパーキング！ 体験版](http://suezou.dyndns.org/dl2020/sagaplanets/kakenuke/kakenuke_trial.zip) | 未確認 |  |
| s2021_akabeesoft2_roleplayer | [ROLEPLAYER：小粥姉妹の粘膜ポトレ ぐりぐちゃLIVE！](http://suezou.dyndns.org/dl2021/akabeesoft2/roleplayer/ab2_roleplayer_okayushimai_webtrial.zip) | 未確認 |  |
| s2021_at_sakura_kodane | [親友に子種を注がれる俺の妻 体験版](http://suezou.dyndns.org/dl2021/at-sakura/kodane/aspc-0121_taiken_ver1.0.zip) | 未確認 |  |
| s2021_at_sakura_tuma_10 | [愛する妻と１０人の過去男 体験版](http://suezou.dyndns.org/dl2021/at-sakura/tuma_10/aspc-0117_taiken_ver1.0.zip) | 未確認 |  |
| s2021_dualtail_vbhi | [VenusBlood HOLLOW International SRPG Trial ENG Ver.](http://suezou.dyndns.org/dl2021/dualtail/vbhi/VBHI_SRPGTrial_EN.zip) | 未確認 |  |
| s2021_nanawind_kanade_gram_ch1 | [花鐘カナデ＊グラム Chapter1 -小桜結- 体験版](http://suezou.dyndns.org/dl2021/nanawind/kanade-gram_ch1/kanade-gram_ch1-trial.zip) | 未確認 |  |
| s2021_no_strike_Trap_Genesis | [Jyosou Souseiki Trial Ver.](http://suezou.dyndns.org/dl2021/no-strike/Trap_Genesis/Jyosou_Souseiki_Trial.zip) | 未確認 |  |
| s2022_akabeesoft2_roleplayer_tororo | [ROLEPLAYER：とろろ姉妹の粘膜ポトレ ぐりぐちゃLIVE！](http://suezou.dyndns.org/dl2022/akabeesoft2/roleplayer_tororo/ab2_roleplayer_tororoshimai_webtrial.zip) | 未確認 |  |
| s2022_at_sakura_konya_aitu | [今夜もあいつに抱かれる彼女 体験版](http://suezou.dyndns.org/dl2022/at-sakura/konya_aitu/aspc-0129_taiken_ver1.0.zip) | 未確認 |  |
| s2022_at_sakura_ntr_tuma_miina | [ネトラセ妻、美衣奈 体験版](http://suezou.dyndns.org/dl2022/at-sakura/ntr_tuma-miina/aspc-0132_taiken_ver1.0.zip) | 未確認 |  |
| s2022_mille_feuille_excelchiffon | [燦然可憐エクセルシフォン 体験版](http://suezou.dyndns.org/dl2022/mille-feuille/excelchiffon/excelchiffon_trial.zip) | 未確認 |  |
| s2023_chaos_r_bian_rr | [魔子宮聖女ビアンカ∽ノエイン～終焉なき破瓜と悦楽の煉獄～　体験版](http://suezou.dyndns.org/dl2023/chaos-r/bian_rr/bianRR_TR.zip) | 未確認 |  |
| s2023_cross_over_isekai_ma | [異世界行ってマッサージで生き残ろうとしたら美脚美女とイチャ×2ライフ～転移ボーナスが美味しすぎた件～体験版](http://suezou.dyndns.org/dl2023/cross-over/isekai_ma/isekai_ma_taiken.zip) | 未確認 |  |
| s2023_hending_hassyakusama | [ソープランドのはっしゃくさま](http://suezou.dyndns.org/dl2023/hending/hassyakusama/hending_soapland_no_hassyakusama_webtrial.zip) | 未確認 |  |
| s2024_appetite_inyoku_train | [淫欲の通学電車～嫌なのにされたい私は堕ちていく～　体験版](http://suezou.dyndns.org/dl2024/appetite/inyoku_train/apt249_tr.zip) | 未確認 |  |
| s2024_appetite_isekai_cyoroin | [異世界チョロインは、いとも簡単に堕とされる～あれ、ボクの彼女…寝取られちゃいました？～　体験版](http://suezou.dyndns.org/dl2024/appetite/isekai_cyoroin/apt251_tr.zip) | 未確認 |  |
| s2024_at_sakura_iede_takamura | [家出少女・高邑緋雪の秘蜜 体験版](http://suezou.dyndns.org/dl2024/at-sakura/iede_takamura/aspc-0155_taiken_ver1.0.zip) | 未確認 |  |
| s2024_no_strike_yuribatake | [Jyosou Yuribatake Trial](http://suezou.dyndns.org/dl2024/no-strike/yuribatake/jyosou_yuribatake_japantrial.zip) | 未確認 |  |
| s2025_akabeesoft2_roleplayer_ikura | [Role player：いくら姉妹の粘膜ポトレ　ぐりぐちゃLIVE！　体験版](http://suezou.dyndns.org/dl2025/akabeesoft2/roleplayer_ikura/Role_player_ikura_trial.zip) | 未確認 |  |
| s2025_poison_extasy_paizuri_me | [最高に都合の良いパイズリ上手のマリーさん - MOTION EDITION - 体験版](http://suezou.dyndns.org/dl2025/poison-extasy/paizuri_me/paizuri_me_tr.zip) | 未確認 |  |
| s2026_no_strike_jyosou_yuriarashi | [Jyosou Yuriarasi Trial](http://suezou.dyndns.org/dl2026/no-strike/jyosou_yuriarashi/jyosouyuriarashi_trial.zip) | 未確認 |  |
| s2026_poison_motion_sxs2_me | [姉妹X催眠２ - MOTION EDITION - 体験版](http://suezou.dyndns.org/dl2026/poison-motion/sxs2-me/sxs2me_trial.zip) | 未確認 |  |
| s2026_poison_motion_sxs_me | [姉妹X催眠 - MOTION EDITION - 体験版](http://suezou.dyndns.org/dl2026/poison-motion/sxs-me/sxsme_trial.zip) | 未確認 |  |

### YU-RIS (vnrun)

| ID | 作品 | 結果 | メモ |
|---|---|---|---|
| dl_RJ01721024 | [むかしグラビアアイドルだった団地妻](https://trial.dlsite.com/doujin/RJ01722000/RJ01721024_trial.zip) | タイトル |  |
| dl_VJ01001386 | [【通常版】ボクが当主になるために今日からメイドとHします2](https://trial.dlsite.com/professional/VJ01002000/VJ01001386_trial.zip) | タイトル | 5224635 (YU-RIS 482) でタイトルまで |
| s2020_casket_ayakashi03 | [あやかし遊郭～花魁アマビエ民譚集～体験版](http://suezou.dyndns.org/dl2020/casket/ayakashi03/ayakashi03_trial.zip) | タイトル |  |
| s2021_casket_bellaca | [リンカーベル・アカデミア～落第魔女とヒミツの儀式～体験版](http://suezou.dyndns.org/dl2021/casket/bellaca/bellaca01_trial.zip) | タイトル |  |
| s2023_casket_oshitona_vtuber02 | [画面から出てきた推しVtuberとHな同棲生活 ～庶民派吸血鬼お嬢様は契約したい～体験版](http://suezou.dyndns.org/dl2023/casket/oshitona_vtuber02/oshitona02_trial.zip) | タイトル |  |
| s2023_casket_oshitona_vtuber03 | [画面から出てきた推しVtuberとHな同棲生活 ～夢魔かわASMR配信者は地雷系？～体験版](http://suezou.dyndns.org/dl2023/casket/oshitona_vtuber03/oshitona03_trial.zip) | タイトル |  |
| s2011_whirlpool_maikaze | [舞風のメルト -Where leads to feeling destination- WEB体験版 (ver1.01)](http://suezou.dyndns.org/dl2011/whirlpool/maikaze/webtrial_maikaze_ver101.exe) | 起動 | 5224635 以降は ysc.ybn エラーは出ないが、黒画面のまま (エラーなし) |
| s2019_no_strike_jyosou_jinjya | [josou jinjya_trial](http://suezou.dyndns.org/dl2019/no-strike/jyosou_jinjya/josou_jinjya_trial_TW.zip) | 要再確認 | pac/ フォルダがゲームのルートとして選ばれ、白画面のまま |
| dl_VJ01000315 | [【限定版】エルフェンキング](https://trial.dlsite.com/professional/VJ01001000/VJ01000315_trial.zip) | NG | 「ysbin/ysc.ybn is missing」 |
| dl_VJ01002327 | [竜翼のメロディア −Diva with the blessed dragonol−](https://trial.dlsite.com/professional/VJ01003000/VJ01002327_trial.zip) | NG | 「ysbin/ysc.ybn is missing」 |
| dl_VJ01004582 | [【豪華版】家族は（俺以外）サキュバスでした ～二人の姉と母との性活で精液を搾り取られます～](https://trial.dlsite.com/professional/VJ01005000/VJ01004582_trial.zip) | NG | 「ysbin/ysc.ybn is missing」 |
| s2014_whirlpool_tearstilla | [鯨神のティアスティラ 体験版](http://suezou.dyndns.org/dl2014/whirlpool/tearstilla/kujira_trial_web.zip) | NG | 5224635 以降も「ysbin/ysc.ybn is missing」 |
| s2016_no_strike_jyogaku | [女装学園（孕）　体験版](http://suezou.dyndns.org/dl2016/no-strike/jyogaku/jyogaku_trial.zip) | NG | 「ysbin/ysc.ybn is missing」 |
| r1_yuris | [YU-RIS サンプル](https://yu-ris.net/download/yu-ris_0495_032.zip) | 対象外 | YU-RIS の SDK のサンプル (コンパイル前で ysbin.ypf が無い) |
| b2006_whirlpool | [いな☆こい！ ～お稲荷さまとモテモテのたたり～ 体験版 ver 1.00](http://suezou.dyndns.org/dl2006/whirlpool/inakoi/inakoi_trial_100.exe) | 未確認 |  |
| b2007_whirlpool | [magus_trial.exe](http://suezou.dyndns.org/dl2007/whirlpool/magus/magus_trial.exe) | 未確認 |  |
| b2010_whirlpool | [涼風のメルト -Where wishes are drawn to each other- WEB体験版](http://suezou.dyndns.org/dl2010/whirlpool/melt/webtrial_melt.exe) | 未確認 |  |
| b2013_anim | [息子の友達に堕とされて　体験版](http://suezou.dyndns.org/dl2013/anim/musuko2/musuko2_trial.zip) | 未確認 |  |
| b2014_whirlpool | [G.I.B. ガールズ・イン・ブラック  WEB体験版](http://suezou.dyndns.org/dl2014/whirlpool/gib/gib_webtrial.exe) | 未確認 |  |
| b2017_no_strike | [女装千年王国　体験版](http://suezou.dyndns.org/dl2017/no-strike/jyosoumille/jyosoumille_trial.zip) | 未確認 |  |
| b2018_casket | [彼女が異種族だった場合～クールな先輩ダークエルフとの甘やかし同棲日誌～体験版](http://suezou.dyndns.org/dl2018/casket/dark_elf/dark_elf-trial.zip) | 未確認 |  |
| b2021_whirlpool | [けもの道☆ガーリッシュスクエア　体験版](http://suezou.dyndns.org/dl2021/whirlpool/kemonomichi-gs/kemonomichi-gs_trial.zip) | 未確認 |  |
| b2022_casket | [あやかし遊郭～花魁道中～体験版](http://suezou.dyndns.org/dl2022/casket/ayakashi_pack/ayakashipack_trial.zip) | 未確認 |  |
| dl_RJ01708454 | [こんな安楽椅子探偵は嫌だ！](https://trial.dlsite.com/doujin/RJ01709000/RJ01708454_trial.zip) | 未確認 |  |
| dl_VJ01000407 | [画面から出てきた推しVtuberとHな同棲生活 ～夢魔かわASMR配信者は地雷系？～](https://trial.dlsite.com/professional/VJ01001000/VJ01000407_trial.zip) | 未確認 |  |
| dl_VJ01001385 | [【豪華版】ボクが当主になるために今日からメイドとHします2](https://trial.dlsite.com/professional/VJ01002000/VJ01001385_trial.zip) | 未確認 |  |
| dl_VJ01002338 | [いな☆こい！ ～お稲荷さまとモテモテのたたり～](https://trial.dlsite.com/professional/VJ01003000/VJ01002338_trial.zip) | 未確認 |  |
| dl_VJ01002863 | [【限定版】巨乳軍人支配催眠「私の任務は貴方にご奉仕する事です。色んな穴をご自由にお使い下さい」](https://trial.dlsite.com/professional/VJ01003000/VJ01002863_trial.zip) | 未確認 |  |
| s2021_casket_bellaca02 | [リンカーベル・アカデミア～龍族のフィアンセ～Hシーン体験版](http://suezou.dyndns.org/dl2021/casket/bellaca02/bellaca02_trial.zip) | 未確認 |  |
<!-- /matrix -->
