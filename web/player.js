// The player page: opens the game named by ?game=<base> (docs/protocol.md) and hands it to
// the engine module for its engine.
import { GameFS } from "./vfs.js";

// engine id in manifest.json -> module under engines/ that runs it
const ENGINES = {
  siglus: "siglus_rs",
  reallive: "siglus_rs",
  avg32: "siglus_rs",
  uk2: "siglus_rs",
  bgi: "ethornell",
};

const $ = (id) => document.getElementById(id);
const overlay = $("overlay");

function show(message) {
  $("message").textContent = message;
  overlay.hidden = false;
}

function fail(error) {
  console.error(error);
  overlay.classList.add("failed");
  show(`起動できませんでした。\n\n${error && error.stack ? error.stack : error}`);
}

// Loads every font listed in fonts/fonts.json next to the page (none when it is missing).
async function fonts() {
  try {
    const res = await fetch("fonts/fonts.json");
    if (!res.ok) return [];
    const names = await res.json();
    const out = [];
    for (const name of names) {
      const r = await fetch(`fonts/${encodeURIComponent(name)}`);
      if (r.ok) out.push(new Uint8Array(await r.arrayBuffer()));
    }
    return out;
  } catch (_) {
    return [];
  }
}

async function main() {
  const params = new URLSearchParams(location.search);
  const base = params.get("game");
  if (!base) throw new Error("URL に ?game=<ゲームの URL> がありません");
  show("ゲームの情報を読み込み中…");
  const fs = await GameFS.open(base);
  const { manifest } = fs;
  const title = manifest.title || base;
  document.title = title;
  $("title").textContent = title;
  const id = ENGINES[manifest.engine];
  if (!id) throw new Error(`このエンジンにはまだ対応していません: ${manifest.engine}`);

  const ui = {
    canvas: $("screen"),
    menu: $("menu"),
    skip: $("skip"),
    show,
    hide: () => {
      overlay.hidden = true;
    },
    exit: async () => {
      show("セーブを送信中…");
      await fs.flush();
      const back = params.get("back");
      if (back) location.href = back;
      else if (history.length > 1) history.back();
      else show("終了しました。このタブは閉じて構いません。");
    },
  };
  $("exit").addEventListener("click", () => ui.exit());
  window.addEventListener("vnbox-save-error", (e) => {
    $("title").textContent = `${title} (セーブの送信に失敗: ${e.detail.path})`;
  });

  show("エンジンを読み込み中…");
  const engine = await import(`./engines/${id}.js`);
  await engine.start({ fs, manifest, ui, fonts: await fonts() });
}

// winit leaves its event loop by throwing this on purpose; it is not a failure.
const controlFlow = (e) => String(e && (e.message || e)).includes("Using exceptions for control flow");
window.addEventListener("error", (e) => {
  if (!controlFlow(e.error || e.message)) fail(e.error || e.message);
});
window.addEventListener("unhandledrejection", (e) => {
  if (!controlFlow(e.reason)) fail(e.reason);
});
main().catch((e) => {
  if (!controlFlow(e)) fail(e);
});
