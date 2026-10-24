// Game files and saves served over HTTP (docs/protocol.md), exposed synchronously because
// the engines read files from inside their frame loop.

const CACHE_BYTES = 256 * 1024 * 1024;

export function normalizePath(path) {
  return String(path || "")
    .replaceAll("\\", "/")
    .split("/")
    .filter((part) => part.length > 0 && part !== ".")
    .join("/");
}

// Lower-cases ASCII only, like Windows does for the paths these games use.
export function foldPath(path) {
  let out = "";
  for (let i = 0; i < path.length; i += 1) {
    const code = path.charCodeAt(i);
    out += code >= 0x41 && code <= 0x5a ? String.fromCharCode(code + 0x20) : path[i];
  }
  return out;
}

function encodePath(path) {
  return path.split("/").map(encodeURIComponent).join("/");
}

function parentOf(path) {
  const i = path.lastIndexOf("/");
  return i < 0 ? "" : path.slice(0, i);
}

function baseOf(path) {
  const i = path.lastIndexOf("/");
  return i < 0 ? path : path.slice(i + 1);
}

// Reads a URL synchronously as bytes. Synchronous XHR cannot use responseType in a window,
// so the body comes back as a binary string.
function getSync(url) {
  const xhr = new XMLHttpRequest();
  xhr.open("GET", url, false);
  xhr.overrideMimeType("text/plain; charset=x-user-defined");
  xhr.send(null);
  if (xhr.status !== 200) throw new Error(`GET ${url}: HTTP ${xhr.status}`);
  const text = xhr.responseText || "";
  const out = new Uint8Array(text.length);
  for (let i = 0; i < text.length; i += 1) out[i] = text.charCodeAt(i) & 0xff;
  return out;
}

export class GameFS {
  constructor(base, manifest) {
    this.base = base.endsWith("/") ? base : `${base}/`;
    this.manifest = manifest;
    this.sizes = new Map(); // path -> size of game files
    this.saves = new Map(); // path -> Uint8Array (written by the game)
    this.folded = new Map(); // folded path -> path
    this.children = new Map(); // dir -> Set(child name)
    this.cache = new Map(); // path -> Uint8Array, least recently used first
    this.cached = 0;
    this.pending = Promise.resolve();
    for (const f of manifest.files || []) {
      const p = normalizePath(f.p);
      if (!p) continue;
      this.sizes.set(p, f.s || 0);
      this.index(p);
    }
  }

  static async open(base) {
    const root = base.endsWith("/") ? base : `${base}/`;
    const res = await fetch(`${root}manifest.json`, { cache: "no-store" });
    if (!res.ok) throw new Error(`manifest.json: HTTP ${res.status}`);
    const fs = new GameFS(root, await res.json());
    await fs.loadSaves();
    return fs;
  }

  index(path) {
    this.folded.set(foldPath(path), path);
    let parent = "";
    for (const part of path.split("/")) {
      if (!this.children.has(parent)) this.children.set(parent, new Set());
      this.children.get(parent).add(part);
      parent = parent ? `${parent}/${part}` : part;
    }
  }

  async loadSaves() {
    const res = await fetch(`${this.base}saves/`, { cache: "no-store" });
    if (!res.ok) throw new Error(`saves/: HTTP ${res.status}`);
    const list = await res.json();
    await Promise.all(
      list.map(async (entry) => {
        const p = normalizePath(entry.p);
        const r = await fetch(`${this.base}saves/${encodePath(p)}`, { cache: "no-store" });
        if (!r.ok) throw new Error(`saves/${p}: HTTP ${r.status}`);
        this.saves.set(p, new Uint8Array(await r.arrayBuffer()));
        this.index(p);
      }),
    );
  }

  resolve(path) {
    const p = normalizePath(path);
    if (this.saves.has(p) || this.sizes.has(p)) return p;
    return this.folded.get(foldPath(p)) || p;
  }

  exists(path) {
    const p = this.resolve(path);
    return this.saves.has(p) || this.sizes.has(p);
  }

  read(path) {
    const p = this.resolve(path);
    const saved = this.saves.get(p);
    if (saved) return saved;
    if (!this.sizes.has(p)) throw new Error(`file not found: ${path}`);
    let bytes = this.cache.get(p);
    if (bytes) {
      this.cache.delete(p);
    } else {
      bytes = getSync(`${this.base}files/${encodePath(p)}`);
      this.cached += bytes.length;
    }
    this.cache.set(p, bytes);
    for (const [k, v] of this.cache) {
      if (this.cached <= CACHE_BYTES || k === p) break;
      this.cache.delete(k);
      this.cached -= v.length;
    }
    return bytes;
  }

  list(path) {
    const p = normalizePath(path);
    let set = this.children.get(p);
    if (!set) {
      const key = foldPath(p);
      for (const [dir, s] of this.children) {
        if (foldPath(dir) === key) {
          set = s;
          break;
        }
      }
    }
    return set ? Array.from(set) : [];
  }

  fileCount() {
    return this.sizes.size + this.saves.size;
  }

  // Game files as the siglus_rs host expects them.
  fileList() {
    return Array.from(this.sizes, ([path, size]) => ({ path, size, lastModified: 0 }));
  }

  write(path, bytes) {
    const p = this.resolve(path);
    const copy = new Uint8Array(bytes); // the view may point into wasm memory
    this.saves.set(p, copy);
    this.index(p);
    this.send("PUT", p, copy);
  }

  remove(path) {
    const p = this.resolve(path);
    if (!this.saves.delete(p)) throw new Error(`not a save file: ${path}`);
    if (!this.sizes.has(p)) {
      this.folded.delete(foldPath(p));
      const siblings = this.children.get(parentOf(p));
      if (siblings) siblings.delete(baseOf(p));
    }
    this.send("DELETE", p);
  }

  // Requests go out one at a time so a later write never lands before an earlier one.
  send(method, path, body) {
    const url = `${this.base}saves/${encodePath(path)}`;
    this.pending = this.pending.then(async () => {
      try {
        const res = await fetch(url, { method, body, keepalive: !body || body.length < 60000 });
        if (!res.ok) throw new Error(`HTTP ${res.status}`);
      } catch (error) {
        console.error(`${method} ${url}:`, error);
        window.dispatchEvent(new CustomEvent("vnbox-save-error", { detail: { path, error } }));
      }
    });
  }

  flush() {
    return this.pending;
  }
}
