// The player page: opens the game named by ?game=<base> (docs/protocol.md) and hands it to
// the engine module for its engine.
import "./style.css";
import "./wgpu-compat";
import type { EngineModule, Ui } from "./engine";
import { GameFS, type SaveErrorDetail } from "./gamefs";

// engine id in manifest.json -> module under engines/ that runs it
const ENGINES: Record<string, () => Promise<EngineModule>> = {
  siglus: () => import("./engines/siglus_rs"),
  reallive: () => import("./engines/siglus_rs"),
  avg32: () => import("./engines/siglus_rs"),
  uk2: () => import("./engines/siglus_rs"),
  bgi: () => import("./engines/ethornell"),
  softpal: () => import("./engines/sena"),
  fvp: () => import("./engines/rfvp"),
  artemis: () => import("./engines/art3m1s"),
  nscripter: () => import("./engines/onscripter"),
  kirikiri: () => import("./engines/krkr"),
  yuris: () => import("./engines/yuris"),
  nexas: () => import("./engines/nexas"),
  html: () => import("./engines/html"),
};

const $ = <T extends HTMLElement = HTMLElement>(id: string) => document.getElementById(id) as T;
const overlay = $("overlay");

function show(message: string): void {
  $("message").textContent = message;
  overlay.hidden = false;
}

function fail(error: unknown): void {
  console.error(error);
  overlay.classList.add("failed");
  const text = error instanceof Error && error.stack ? error.stack : String(error);
  show(`起動できませんでした。\n\n${text}`);
}

// Loads every font listed in fonts/fonts.json next to the page (none when it is missing).
async function fonts(): Promise<Uint8Array[]> {
  try {
    const res = await fetch("fonts/fonts.json");
    if (!res.ok) return [];
    const names = (await res.json()) as string[];
    const out: Uint8Array[] = [];
    for (const name of names) {
      const r = await fetch(`fonts/${encodeURIComponent(name)}`);
      if (r.ok) out.push(new Uint8Array(await r.arrayBuffer()));
    }
    return out;
  } catch {
    return [];
  }
}

async function main(): Promise<void> {
  const params = new URLSearchParams(location.search);
  const base = params.get("game");
  if (!base) throw new Error("URL に ?game=<ゲームの URL> がありません");
  show("ゲームの情報を読み込み中…");
  const fs = await GameFS.open(base);
  const { manifest } = fs;
  const title = manifest.title || base;
  document.title = title;
  $("title").textContent = title;
  const load = ENGINES[manifest.engine];
  if (!load) throw new Error(`このエンジンにはまだ対応していません: ${manifest.engine}`);

  const ui: Ui = {
    canvas: $<HTMLCanvasElement>("screen"),
    menu: $<HTMLButtonElement>("menu"),
    skip: $<HTMLButtonElement>("skip"),
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
  $("exit").addEventListener("click", () => void ui.exit());
  window.addEventListener("vnrun-save-error", (e) => {
    const { path } = (e as CustomEvent<SaveErrorDetail>).detail;
    $("title").textContent = `${title} (セーブの送信に失敗: ${path})`;
  });

  show("エンジンを読み込み中…");
  const engine = await load();
  await engine.start({ fs, manifest, ui, fonts: await fonts() });
}

// winit leaves its event loop by throwing this on purpose; it is not a failure.
const controlFlow = (e: unknown) =>
  String(e instanceof Error ? e.message : e).includes("Using exceptions for control flow");
window.addEventListener("error", (e) => {
  const error: unknown = e.error ?? e.message;
  if (!controlFlow(error)) fail(error);
});
window.addEventListener("unhandledrejection", (e) => {
  if (!controlFlow(e.reason)) fail(e.reason);
});
main().catch((e: unknown) => {
  if (!controlFlow(e)) fail(e);
});
