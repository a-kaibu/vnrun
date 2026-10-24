"""Writes the tables of docs/verification.md (between its <!-- matrix --> markers) from
targets.tsv (the titles) and status.tsv (id, result, note: judged from the screenshots).

    python3 tools/verify/matrix.py
"""
import collections
import pathlib

HERE = pathlib.Path(__file__).parent
DOC = HERE.parent.parent / "docs" / "verification.md"
ENGINES = {
    "siglus": "SiglusEngine (siglus_rs)",
    "reallive": "RealLive (siglus_rs)",
    "bgi": "BGI (ethornell-rs)",
    "softpal": "Softpal (sena-rs)",
    "artemis": "Artemis (art3m1s-core)",
    "nscripter": "NScripter (ONScripter-Yuri)",
    "html": "HTML (TyranoScript、RPGツクール MV/MZ)",
    "kirikiri": "吉里吉里 (KiriKiri SDL2)",
    "yuris": "YU-RIS (vnrun)",
}
RESULTS = ["本編", "タイトル", "起動", "要再確認", "NG", "対象外", "未確認"]


def rows(name):
    for line in (HERE / name).read_text(encoding="utf-8").splitlines():
        if line.strip():
            yield (line.split("\t") + ["", "", "", ""])[:4]


status = {id_: (result, note) for id_, result, note, _ in rows("status.tsv")}
by_engine = collections.defaultdict(list)
for id_, engine, title, url in rows("targets.tsv"):
    by_engine[engine].append((id_, title, url))

out = ["| エンジン | 本数 | " + " | ".join(RESULTS) + " |", "|---|" + "---|" * (len(RESULTS) + 1)]
for engine, name in ENGINES.items():
    count = collections.Counter(status.get(t[0], ("未確認",))[0] for t in by_engine[engine])
    out.append(f"| {name} | {len(by_engine[engine])} | " + " | ".join(str(count[r] or "") for r in RESULTS) + " |")
for engine, name in ENGINES.items():
    out += ["", f"### {name}", "", "| ID | 作品 | 結果 | メモ |", "|---|---|---|---|"]
    titles = sorted(by_engine[engine], key=lambda t: (RESULTS.index(status.get(t[0], ("未確認",))[0]), t[0]))
    for id_, title, url in titles:
        result, note = status.get(id_, ("未確認", ""))
        label = url.rsplit("/", 1)[-1] if title == id_ else title
        out.append(f"| {id_} | [{label}]({url}) | {result} | {note} |")

doc = DOC.read_text(encoding="utf-8")
head, _, rest = doc.partition("<!-- matrix -->")
_, _, tail = rest.partition("<!-- /matrix -->")
DOC.write_text(head + "<!-- matrix -->\n" + "\n".join(out) + "\n<!-- /matrix -->" + tail, encoding="utf-8")
