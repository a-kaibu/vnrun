// Games that are web pages already: TyranoScript, RPG Maker MV/MZ and other NW.js games. The
// page runs as it is in an iframe and draws itself; the player only keeps its saves. The
// server starts each page of the game with a call to vnrunFrame (docs/protocol.md), which
// swaps the page's localStorage for one kept in the GameFS, before any script of the game runs.
// IndexedDB is left as it is.
import type { EngineContext } from "../engine";
import type { GameFS, Manifest } from "../gamefs";

const SAVES = "localStorage";

// The page to open: package.json's main (NW.js), else www/index.html (RPG Maker MV), else
// index.html.
function startPage(fs: GameFS, manifest: Manifest): string {
  if (fs.exists("package.json")) {
    try {
      const main = JSON.parse(new TextDecoder().decode(fs.read("package.json"))).main;
      const page = typeof main === "string" ? main.replace(/^app:\/\/[^/]*\//, "").replace(/^\.\//, "") : "";
      if (/\.html?$/i.test(page) && fs.exists(page)) return page;
    } catch (error) {
      console.warn("package.json:", error);
    }
  }
  const pages = (manifest.files || []).map((f) => f.p);
  return pages.find((p) => p.toLowerCase() === "www/index.html") || "index.html";
}

// A Storage whose items are save files (one per key) in the GameFS.
function storage(fs: GameFS): Storage {
  const path = (key: string) => `${SAVES}/${encodeURIComponent(key)}`;
  const keys = () =>
    Array.from(fs.saves.keys())
      .filter((p) => p.startsWith(`${SAVES}/`))
      .map((p) => decodeURIComponent(p.slice(SAVES.length + 1)));
  const api: Storage = {
    get length() {
      return keys().length;
    },
    key: (i: number) => keys()[i] ?? null,
    getItem: (key: string) => {
      const bytes = fs.saves.get(path(String(key)));
      return bytes ? new TextDecoder().decode(bytes) : null;
    },
    setItem: (key: string, value: string) =>
      fs.write(path(String(key)), new TextEncoder().encode(String(value))),
    removeItem: (key: string) => {
      if (fs.saves.has(path(String(key)))) fs.remove(path(String(key)));
    },
    clear: () => keys().forEach((k) => fs.remove(path(k))),
  };
  // Pages also use storage["key"] and Object.keys(storage).
  return new Proxy(api, {
    get: (t, k) => (typeof k === "symbol" || k in t ? Reflect.get(t, k) : (t.getItem(k) ?? undefined)),
    set: (t, k, v) => (typeof k === "symbol" || k in t ? false : (t.setItem(k, v), true)),
    deleteProperty: (t, k) => (typeof k === "string" && t.removeItem(k), true),
    has: (t, k) => typeof k === "string" && (k in t || t.getItem(k) !== null),
    ownKeys: () => keys(),
    getOwnPropertyDescriptor: (t, k) =>
      typeof k === "string" && t.getItem(k) !== null
        ? { value: t.getItem(k), writable: true, enumerable: true, configurable: true }
        : undefined,
  });
}

export async function start({ fs, manifest, ui }: EngineContext): Promise<void> {
  const localStorage = storage(fs);
  Object.assign(window, {
    vnrunFrame(win: Window) {
      Object.defineProperty(win, "localStorage", { value: localStorage });
      // RPG Maker MZ refuses to start without IndexedDB and keeps its saves there (localforage,
      // keys named after the game id), so it stays the browser's own.
    },
  });

  const frame = document.createElement("iframe");
  frame.id = "frame";
  frame.allow = "autoplay; fullscreen";
  frame.src = fs.fileUrl(startPage(fs, manifest));
  ui.canvas.hidden = true;
  document.body.append(frame);
  frame.addEventListener("load", () => {
    ui.hide();
    frame.focus();
  });
}
