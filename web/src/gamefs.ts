// Game files and saves served over HTTP (docs/protocol.md), exposed synchronously because
// the engines read files from inside their frame loop.

const CACHE_BYTES = 256 * 1024 * 1024;

/** One file of manifest.json: path and size. */
export interface ManifestFile {
  p: string;
  s?: number;
}

/** manifest.json of a game (docs/protocol.md). */
export interface Manifest {
  engine: string;
  title?: string;
  nls?: string;
  platform?: string;
  files?: ManifestFile[];
}

export function normalizePath(path: string): string {
  return String(path || "")
    .replaceAll("\\", "/")
    .split("/")
    .filter((part) => part.length > 0 && part !== ".")
    .join("/");
}

// Lower-cases ASCII only, like Windows does for the paths these games use.
export function foldPath(path: string): string {
  let out = "";
  for (let i = 0; i < path.length; i += 1) {
    const code = path.charCodeAt(i);
    out += code >= 0x41 && code <= 0x5a ? String.fromCharCode(code + 0x20) : path[i];
  }
  return out;
}

function encodePath(path: string): string {
  return path.split("/").map(encodeURIComponent).join("/");
}

function parentOf(path: string): string {
  const i = path.lastIndexOf("/");
  return i < 0 ? "" : path.slice(0, i);
}

function baseOf(path: string): string {
  const i = path.lastIndexOf("/");
  return i < 0 ? path : path.slice(i + 1);
}

// Reads a URL synchronously as bytes. Synchronous XHR cannot use responseType in a window,
// so the body comes back as a binary string. `range` is [start, end) of the file; a server
// that ignores it sends the whole file.
function getSync(url: string, range?: [number, number]): Uint8Array {
  const xhr = new XMLHttpRequest();
  xhr.open("GET", url, false);
  xhr.overrideMimeType("text/plain; charset=x-user-defined");
  if (range) xhr.setRequestHeader("Range", `bytes=${range[0]}-${range[1] - 1}`);
  xhr.send(null);
  const ok = xhr.status === 200 || (range && xhr.status === 206);
  if (!ok) throw new Error(`GET ${url}: HTTP ${xhr.status}`);
  const text = xhr.responseText || "";
  const out = new Uint8Array(text.length);
  for (let i = 0; i < text.length; i += 1) out[i] = text.charCodeAt(i) & 0xff;
  return range && xhr.status === 200 ? out.subarray(range[0], range[1]) : out;
}

/** Detail of the "vnrun-save-error" event, sent when a save cannot reach the server. */
export interface SaveErrorDetail {
  path: string;
  error: unknown;
}

export class GameFS {
  readonly base: string;
  readonly manifest: Manifest;
  /** Saves written by the game, by path. */
  readonly saves = new Map<string, Uint8Array>();
  private readonly sizes = new Map<string, number>();
  private readonly folded = new Map<string, string>();
  private readonly children = new Map<string, Set<string>>();
  // Whole files, least recently used first.
  private readonly cache = new Map<string, Uint8Array>();
  private cached = 0;
  private pending: Promise<void> = Promise.resolve();

  constructor(base: string, manifest: Manifest) {
    this.base = base.endsWith("/") ? base : `${base}/`;
    this.manifest = manifest;
    for (const f of manifest.files || []) {
      const p = normalizePath(f.p);
      if (!p) continue;
      this.sizes.set(p, f.s || 0);
      this.index(p);
    }
  }

  static async open(base: string): Promise<GameFS> {
    const root = base.endsWith("/") ? base : `${base}/`;
    const res = await fetch(`${root}manifest.json`, { cache: "no-store" });
    if (!res.ok) throw new Error(`manifest.json: HTTP ${res.status}`);
    const fs = new GameFS(root, (await res.json()) as Manifest);
    await fs.loadSaves();
    return fs;
  }

  private index(path: string): void {
    this.folded.set(foldPath(path), path);
    let parent = "";
    for (const part of path.split("/")) {
      let set = this.children.get(parent);
      if (!set) this.children.set(parent, (set = new Set()));
      set.add(part);
      parent = parent ? `${parent}/${part}` : part;
    }
  }

  private async loadSaves(): Promise<void> {
    const res = await fetch(`${this.base}saves/`, { cache: "no-store" });
    if (!res.ok) throw new Error(`saves/: HTTP ${res.status}`);
    const list = (await res.json()) as ManifestFile[];
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

  resolve(path: string): string {
    const p = normalizePath(path);
    if (this.saves.has(p) || this.sizes.has(p)) return p;
    return this.folded.get(foldPath(p)) || p;
  }

  exists(path: string): boolean {
    const p = this.resolve(path);
    return this.saves.has(p) || this.sizes.has(p);
  }

  read(path: string): Uint8Array {
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

  /** Size in bytes, or -1 when there is no such file. */
  size(path: string): number {
    const p = this.resolve(path);
    const saved = this.saves.get(p);
    if (saved) return saved.length;
    return this.sizes.get(p) ?? -1;
  }

  /** Part of a file, without fetching the rest of a large archive. */
  readRange(path: string, offset: number, length: number): Uint8Array {
    const p = this.resolve(path);
    const whole = this.saves.get(p) || this.cache.get(p);
    if (whole) return whole.subarray(offset, offset + length);
    const size = this.sizes.get(p);
    if (size === undefined) throw new Error(`file not found: ${path}`);
    const end = Math.min(size, offset + length);
    if (end <= offset) return new Uint8Array(0);
    return getSync(`${this.base}files/${encodePath(p)}`, [offset, end]);
  }

  list(path: string): string[] {
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

  fileCount(): number {
    return this.sizes.size + this.saves.size;
  }

  /** URL of a game file, for engines that read ranges themselves. */
  fileUrl(path: string): string {
    return `${this.base}files/${encodePath(this.resolve(path))}`;
  }

  /** Game files as the siglus_rs host expects them. */
  fileList(): { path: string; size: number; lastModified: number }[] {
    return Array.from(this.sizes, ([path, size]) => ({ path, size, lastModified: 0 }));
  }

  write(path: string, bytes: Uint8Array): void {
    const p = this.resolve(path);
    const copy = new Uint8Array(bytes); // the view may point into wasm memory
    this.saves.set(p, copy);
    this.index(p);
    this.send("PUT", p, copy);
  }

  remove(path: string): void {
    const p = this.resolve(path);
    if (!this.saves.delete(p)) throw new Error(`not a save file: ${path}`);
    if (!this.sizes.has(p)) {
      this.folded.delete(foldPath(p));
      this.children.get(parentOf(p))?.delete(baseOf(p));
    }
    this.send("DELETE", p);
  }

  // Requests go out one at a time so a later write never lands before an earlier one.
  private send(method: "PUT" | "DELETE", path: string, body?: Uint8Array): void {
    const url = `${this.base}saves/${encodePath(path)}`;
    this.pending = this.pending.then(async () => {
      try {
        const res = await fetch(url, {
          method,
          body: body as BodyInit | undefined,
          keepalive: !body || body.length < 60000,
        });
        if (!res.ok) throw new Error(`HTTP ${res.status}`);
      } catch (error) {
        console.error(`${method} ${url}:`, error);
        const detail: SaveErrorDetail = { path, error };
        window.dispatchEvent(new CustomEvent("vnrun-save-error", { detail }));
      }
    });
  }

  /** Resolves when every save sent so far has reached the server. */
  flush(): Promise<void> {
    return this.pending;
  }
}
