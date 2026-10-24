// Artemis through art3m1s-core (third_party/art3m1s-core), built with its browser host
// (engines/art3m1s/host) by engines/art3m1s/build.sh into public/engines/art3m1s/. The engine
// is an Emscripten module: game files appear under /game as lazy files read in blocks from the
// GameFS, saves live under /saves and are copied back to the GameFS, and the engine renders
// with WebGL2 into its own framebuffer whose pixels the presenter shows on the page's canvas.
// Sound is the host's job in art3m1s (media events), played here with Web Audio.
import { importEngine, type EngineContext } from "../engine";
import type { GameFS, Manifest } from "../gamefs";
import { Presenter } from "../present";

// The parts of the Emscripten module and its file system used here.
interface FSNode {
  contents: unknown;
  stream_ops: Record<string, unknown>;
}
interface EmFS {
  mkdirTree(path: string): void;
  createFile(parent: string, name: string, properties: object, canRead: boolean, canWrite: boolean): FSNode;
  writeFile(path: string, data: Uint8Array): void;
  readFile(path: string): Uint8Array;
  readdir(path: string): string[];
  stat(path: string): { mode: number; mtime: Date };
  isDir(mode: number): boolean;
  ErrnoError: new (errno: number) => Error;
}
interface Art3m1sModule {
  FS: EmFS;
  HEAPU8: Uint8Array;
  UTF8ToString(ptr: number): string;
  stringToNewUTF8(text: string): number;
  _free(ptr: number): void;
  _vn_start(archive: number, platform: number): number;
  _vn_error(): number;
  _vn_frame(delta: number): number;
  _vn_pixels(): number;
  _vn_exit_requested(): number;
  _vn_mouse(x: number, y: number): void;
  _vn_button(vk: number, pressed: number): void;
  _vn_key(vk: number, pressed: number): void;
  _vn_poll_events(): number;
  _vn_out(): number;
  _vn_read(path: number): number;
  _vn_sound_finished(id: number): void;
  _vn_video_finished(id: number): void;
  _vn_dialog(accepted: number, text: number): number;
  _vn_lifecycle(state: number): void;
}
type CreateArt3m1s = (options: object) => Promise<Art3m1sModule>;
type Engine = ReturnType<typeof bindEngine>;
type ReadInto = (buffer: Uint8Array, offset: number, length: number, position: number) => number;

// Saves get their own folder so they never shadow a game file of the same name.
const SAVES = "art3m1s-save";
const BLOCK = 256 * 1024;
const CACHE_BLOCKS = 512; // 128MB

// --- Files ------------------------------------------------------------------

// Reads game files in blocks, keeping the most recently used ones.
function blockReader(fs: GameFS) {
  const blocks = new Map<string, Uint8Array>(); // `${path}\n${index}`
  return (path: string, size: number, position: number, out: Uint8Array, offset: number, length: number) => {
    const end = Math.min(size, position + length);
    let at = position;
    while (at < end) {
      const index = Math.floor(at / BLOCK);
      const key = `${path}\n${index}`;
      let block = blocks.get(key);
      if (block) {
        blocks.delete(key);
      } else {
        block = fs.readRange(path, index * BLOCK, BLOCK).slice();
        if (blocks.size >= CACHE_BLOCKS) blocks.delete(blocks.keys().next().value!);
      }
      blocks.set(key, block);
      const from = at - index * BLOCK;
      const count = Math.min(end - at, block.length - from);
      if (count <= 0) break;
      out.set(block.subarray(from, from + count), offset + (at - position));
      at += count;
    }
    return at - position;
  };
}

function parentDir(path: string): string {
  return path.slice(0, path.lastIndexOf("/"));
}

// A read-only file of `size` bytes whose reads go to `read` (after Emscripten's createLazyFile,
// which only works in workers).
function lazyFile(FS: EmFS, path: string, size: number, read: ReadInto): void {
  FS.mkdirTree(parentDir(path));
  const node = FS.createFile(parentDir(path), path.slice(path.lastIndexOf("/") + 1), {}, true, false);
  node.contents = { length: size }; // never read: the stream ops below replace MEMFS's
  Object.defineProperties(node, { usedBytes: { get: () => size } });
  const ops = { ...node.stream_ops };
  ops.read = (_stream: unknown, buffer: Uint8Array, offset: number, length: number, position: number) =>
    read(buffer, offset, length, position);
  ops.mmap = () => {
    throw new FS.ErrnoError(28); // EINVAL
  };
  node.stream_ops = ops;
}

function mountGame(FS: EmFS, fs: GameFS, manifest: Manifest): void {
  const read = blockReader(fs);
  for (const f of manifest.files || []) {
    const size = f.s || 0;
    lazyFile(FS, `/game/${f.p}`, size, (buffer, offset, length, position) =>
      read(f.p, size, position, buffer, offset, length),
    );
  }
}

// The saves folder, copied from the GameFS at start and back whenever a file changes.
class Saves {
  private seen = new Map<string, number>(); // path under /saves -> mtime

  constructor(
    private readonly FS: EmFS,
    private readonly fs: GameFS,
  ) {
    FS.mkdirTree("/saves");
    const prefix = `${SAVES}/`;
    for (const [path, bytes] of fs.saves) {
      if (!path.startsWith(prefix)) continue;
      const local = `/saves/${path.slice(prefix.length)}`;
      FS.mkdirTree(parentDir(local));
      FS.writeFile(local, bytes);
    }
    this.scan(false);
  }

  private files(dir = "/saves", out: [string, number][] = []): [string, number][] {
    for (const name of this.FS.readdir(dir)) {
      if (name === "." || name === "..") continue;
      const path = `${dir}/${name}`;
      const stat = this.FS.stat(path);
      if (this.FS.isDir(stat.mode)) this.files(path, out);
      else out.push([path, stat.mtime.getTime()]);
    }
    return out;
  }

  // Sends files written or removed since the last scan.
  scan(send = true) {
    const now = new Map(this.files());
    for (const [path, mtime] of now) {
      if (send && this.seen.get(path) !== mtime) {
        this.fs.write(`${SAVES}/${path.slice("/saves/".length)}`, this.FS.readFile(path));
      }
    }
    for (const path of this.seen.keys()) {
      if (send && !now.has(path)) {
        try {
          this.fs.remove(`${SAVES}/${path.slice("/saves/".length)}`);
        } catch (error) {
          console.warn(error);
        }
      }
    }
    this.seen = now;
  }
}

// --- Sound (media events, doc/FFI_REFERENCE.md "媒体命令协议") ---------------

// Script gains are 0..1000 and pans -1000..1000; small values are already fractions.
const gainOf = (gain: number | null | undefined) =>
  gain == null ? 1 : Math.min(1, Math.max(0, gain > 1 ? gain / 1000 : gain));
const panOf = (pan: number | null | undefined) =>
  pan == null ? 0 : Math.min(1, Math.max(-1, Math.abs(pan) > 1 ? pan / 1000 : pan));

// A media event's payload (src/host/media.rs); fields depend on the kind.
interface Media {
  [key: string]: unknown;
  id?: string;
  file?: string;
  resolved_file?: string | null;
  loop_file?: string | null;
  resolved_loop_file?: string | null;
  loop?: boolean;
  gain?: number | null;
  pan?: number | null;
  fade_ms?: number;
  time_ms?: number;
  channel?: string;
  value?: number;
}
interface Player {
  generation: number;
  gain: GainNode;
  panner: StereoPannerNode;
  sources: AudioBufferSourceNode[];
  stopped: boolean;
}

class Sound {
  private readonly context = new AudioContext();
  private readonly channels: Record<string, GainNode> = {};
  private readonly master: GainNode;
  private readonly playing = new Map<string, Player>(); // "bgm" or `se:${id}` / `voice:${id}`
  private readonly buffers = new Map<string, Promise<AudioBuffer>>();
  private generation = 0;

  constructor(private readonly engine: Engine) {
    const resume = () => void this.context.resume();
    window.addEventListener("pointerdown", resume);
    window.addEventListener("keydown", resume);
    this.master = this.context.createGain();
    this.master.connect(this.context.destination);
    for (const name of ["bgm", "se", "voice"]) {
      this.channels[name] = this.context.createGain();
      this.channels[name].connect(this.master);
    }
  }

  private buffer(path: string): Promise<AudioBuffer> {
    let buffer = this.buffers.get(path);
    if (!buffer) {
      const bytes = this.engine.read(path);
      buffer = bytes ? this.context.decodeAudioData(bytes.buffer) : Promise.reject(new Error(`no ${path}`));
      if (this.buffers.size > 64) this.buffers.delete(this.buffers.keys().next().value!);
      this.buffers.set(path, buffer);
    }
    return buffer;
  }

  // Plays `file` (then `loopFile` repeatedly, if given) on a channel. `done` runs once when a
  // sound that does not loop ends by itself.
  private play(key: string, channel: string, p: Media, done: () => void): void {
    this.stop(key, 0);
    const generation = ++this.generation;
    const gain = this.context.createGain();
    const panner = this.context.createStereoPanner();
    gain.gain.value = 0;
    panner.pan.value = panOf(p.pan);
    gain.connect(panner).connect(this.channels[channel]);
    const player: Player = { generation, gain, panner, sources: [], stopped: false };
    this.playing.set(key, player);
    const file = p.resolved_file || p.file || "";
    const loopFile = p.resolved_loop_file || p.loop_file;
    const fade = (p.fade_ms || 0) / 1000;
    const level = gainOf(p.gain);
    const start = async () => {
      const first = await this.buffer(file);
      const second = loopFile ? await this.buffer(loopFile) : null;
      if (player.stopped) return;
      const t = this.context.currentTime;
      gain.gain.setValueAtTime(fade > 0 ? 0 : level, t);
      if (fade > 0) gain.gain.linearRampToValueAtTime(level, t + fade);
      const a = this.context.createBufferSource();
      a.buffer = first;
      a.loop = Boolean(p.loop) && !second;
      a.connect(gain);
      a.start(t);
      player.sources.push(a);
      if (second) {
        const b = this.context.createBufferSource();
        b.buffer = second;
        b.loop = true;
        b.connect(gain);
        b.start(t + first.duration);
        player.sources.push(b);
      } else if (!a.loop) {
        a.onended = () => {
          if (player.stopped || this.playing.get(key) !== player) return;
          this.playing.delete(key);
          done();
        };
      }
    };
    start().catch((error: unknown) => {
      console.warn(`art3m1s sound ${file}:`, error);
      // A sound that cannot play still finishes, so a script waiting on it goes on.
      if (!player.stopped && this.playing.get(key) === player) {
        this.playing.delete(key);
        if (!p.loop) done();
      }
    });
  }

  private stop(key: string, fadeMs: number | undefined): void {
    const player = this.playing.get(key);
    if (!player) return;
    this.playing.delete(key);
    player.stopped = true;
    const t = this.context.currentTime;
    const end = t + (fadeMs || 0) / 1000;
    player.gain.gain.cancelScheduledValues(t);
    player.gain.gain.setValueAtTime(player.gain.gain.value, t);
    player.gain.gain.linearRampToValueAtTime(0, end);
    for (const source of player.sources) source.stop(end + 0.01);
  }

  private fade(key: string, gain: number | null | undefined, timeMs: number | undefined): void {
    const player = this.playing.get(key);
    if (!player) return;
    const t = this.context.currentTime;
    player.gain.gain.cancelScheduledValues(t);
    player.gain.gain.setValueAtTime(player.gain.gain.value, t);
    player.gain.gain.linearRampToValueAtTime(gainOf(gain), t + (timeMs || 0) / 1000);
  }

  private pan(key: string, pan: number | null | undefined, timeMs: number | undefined): void {
    const player = this.playing.get(key);
    if (!player) return;
    const t = this.context.currentTime;
    player.panner.pan.linearRampToValueAtTime(panOf(pan), t + (timeMs || 0) / 1000);
  }

  handle(kind: string, p: Media): void {
    const finished = (id: string) => () => this.engine.soundFinished(id);
    switch (kind) {
      case "audio_set_volume": {
        const node = p.channel === "master" ? this.master : this.channels[p.channel ?? ""];
        if (node) node.gain.value = Math.min(1, Math.max(0, p.value ?? 1));
        break;
      }
      case "audio_bgm_play":
      case "audio_bgm_crossfade":
        this.play("bgm", "bgm", { ...p, fade_ms: p.fade_ms ?? p.time_ms }, finished(""));
        break;
      case "audio_bgm_stop":
        this.stop("bgm", p.fade_ms);
        break;
      case "audio_bgm_fade":
        this.fade("bgm", p.gain, p.time_ms);
        break;
      case "audio_bgm_pan":
        this.pan("bgm", p.pan, p.time_ms);
        break;
      case "audio_se_play":
        this.play(`se:${p.id}`, "se", p, finished(p.id ?? ""));
        break;
      case "audio_se_stop":
        this.stop(`se:${p.id}`, p.fade_ms);
        break;
      case "audio_se_fade":
        this.fade(`se:${p.id}`, p.gain, p.time_ms);
        break;
      case "audio_se_pan":
        this.pan(`se:${p.id}`, p.pan, p.time_ms);
        break;
      case "audio_voice_play":
        this.play(`voice:${p.id}`, "voice", p, finished(p.id ?? ""));
        break;
      case "audio_stop_all":
        for (const key of Array.from(this.playing.keys())) this.stop(key, 0);
        break;
      case "video_play":
        // No video decoder in the browser build yet: report it done so the game goes on.
        console.info(`art3m1s: skipping video ${p.file}`);
        this.engine.videoFinished(p.id || "");
        break;
      case "video_stop_all":
        break;
      default:
        console.info(`art3m1s media: ${kind}`, p);
    }
  }
}

// --- Engine -------------------------------------------------------------------

function bindEngine(Module: Art3m1sModule) {
  const text = (value: string) => Module.stringToNewUTF8(value);
  const withText = <T>(value: string, call: (ptr: number) => T): T => {
    const ptr = text(value);
    try {
      return call(ptr);
    } finally {
      Module._free(ptr);
    }
  };
  return {
    start: (archive: string, platform: string) =>
      withText(archive, (a) => withText(platform, (p) => Module._vn_start(a, p))),
    error: () => Module.UTF8ToString(Module._vn_error()),
    frame: (delta: number) => Module._vn_frame(delta),
    pixels: (length: number) => Module.HEAPU8.subarray(Module._vn_pixels(), Module._vn_pixels() + length),
    exitRequested: () => Module._vn_exit_requested() !== 0,
    mouse: (x: number, y: number) => Module._vn_mouse(x, y),
    button: (vk: number, pressed: boolean) => Module._vn_button(vk, pressed ? 1 : 0),
    key: (vk: number, pressed: boolean) => Module._vn_key(vk, pressed ? 1 : 0),
    events: () => {
      const length = Module._vn_poll_events();
      return length ? Module.HEAPU8.slice(Module._vn_out(), Module._vn_out() + length) : null;
    },
    read: (path: string) => {
      const length = withText(path, (p) => Module._vn_read(p));
      return length < 0 ? null : Module.HEAPU8.slice(Module._vn_out(), Module._vn_out() + length);
    },
    soundFinished: (id: string) => withText(id, (p) => Module._vn_sound_finished(p)),
    videoFinished: (id: string) => withText(id, (p) => Module._vn_video_finished(p)),
    dialog: (accepted: boolean, value: string | null) =>
      withText(value || "", (p) => Module._vn_dialog(accepted ? 1 : 0, p)),
    lifecycle: (state: number) => Module._vn_lifecycle(state),
  };
}

// Host events: 24-byte little-endian headers (version, kind, sequence, payload size, aux)
// each followed by its payload. Kinds: 1 log, 2 media, 3 UI.
function* hostEvents(bytes: Uint8Array): Generator<{ kind: number; aux: number; text: string }> {
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const decoder = new TextDecoder();
  for (let at = 0; at + 24 <= bytes.length;) {
    const kind = view.getUint32(at + 4, true);
    const size = view.getUint32(at + 16, true);
    const aux = view.getUint32(at + 20, true);
    yield { kind, aux, text: decoder.decode(bytes.subarray(at + 24, at + 24 + size)) };
    at += 24 + size;
  }
}

interface UiEvent {
  data?: string;
  title?: string;
  message?: string;
  textfield?: boolean;
  hasCancel?: boolean;
  initialText?: string;
}

function handleUi(engine: Engine, kind: string, p: UiEvent): void {
  switch (kind) {
    case "caption":
      if (p.data) document.title = p.data;
      break;
    case "dialog_show":
      // Answered after this frame, never from inside the engine's call.
      setTimeout(() => {
        const message = [p.title, p.message].filter(Boolean).join("\n");
        if (p.textfield) {
          const value = window.prompt(message, p.initialText || "");
          engine.dialog(value !== null, value);
        } else if (p.hasCancel) {
          engine.dialog(window.confirm(message), null);
        } else {
          window.alert(message);
          engine.dialog(true, null);
        }
      });
      break;
    default:
      console.info(`art3m1s ui: ${kind}`, p);
  }
}

function attachInput(canvas: HTMLCanvasElement, presenter: Presenter, engine: Engine): void {
  const vk = (event: PointerEvent) => [1, 4, 2][event.button] || 0; // left, middle, right
  canvas.addEventListener("pointermove", (event) => engine.mouse(...presenter.toFrame(event)));
  canvas.addEventListener("pointerdown", (event) => {
    if (!vk(event)) return;
    canvas.focus();
    canvas.setPointerCapture(event.pointerId);
    engine.mouse(...presenter.toFrame(event));
    engine.button(vk(event), true);
  });
  canvas.addEventListener("pointerup", (event) => {
    if (!vk(event)) return;
    engine.mouse(...presenter.toFrame(event));
    engine.button(vk(event), false);
  });
  canvas.addEventListener("contextmenu", (event) => event.preventDefault());
  // Keys use Windows virtual-key codes, which keyCode still reports.
  const key = (pressed: boolean) => (event: KeyboardEvent) => {
    if (event.metaKey || !event.keyCode) return;
    if ([9, 13, 27, 32, 33, 34, 37, 38, 39, 40].includes(event.keyCode)) event.preventDefault();
    engine.key(event.keyCode, pressed);
  };
  window.addEventListener("keydown", key(true));
  window.addEventListener("keyup", key(false));
}

// The archive to mount: root.pfs (later volumes and patches are found beside it), or none
// for an unpacked game folder.
function archiveOf(manifest: Manifest): string {
  const names = (manifest.files || []).map((f) => f.p).filter((p) => !p.includes("/"));
  return names.find((n) => n.toLowerCase() === "root.pfs") || names.find((n) => /\.pfs$/i.test(n)) || "";
}

function handleEvents(engine: Engine, sound: Sound): void {
  const events = engine.events();
  if (!events) return;
  for (const event of hostEvents(events)) {
    if (event.kind === 1) {
      const level = String.fromCharCode(event.aux);
      const log = level === "E" ? console.error : level === "W" ? console.warn : console.debug;
      log(`art3m1s: ${event.text}`);
    } else if (event.kind === 2) {
      const { kind, payload } = JSON.parse(event.text) as { kind: string; payload?: Media };
      sound.handle(kind, payload || {});
    } else if (event.kind === 3) {
      const { kind, payload } = JSON.parse(event.text) as { kind: string; payload?: UiEvent };
      handleUi(engine, kind, payload || {});
    }
  }
}

export async function start({ fs, manifest, ui }: EngineContext): Promise<void> {
  const createArt3m1s = (await importEngine<{ default: CreateArt3m1s }>("art3m1s", "art3m1s.js")).default;
  const glCanvas = document.createElement("canvas"); // the engine's, never shown
  const logs: string[] = [];
  const Module = await createArt3m1s({
    canvas: glCanvas,
    print: (line: string) => console.info(line),
    printErr: (line: string) => {
      logs.push(line);
      console.warn(line);
    },
  });
  mountGame(Module.FS, fs, manifest);
  const saves = new Saves(Module.FS, fs);
  const engine = bindEngine(Module);
  const size = engine.start(archiveOf(manifest), manifest.platform || "WINDOWS");
  if (!size) throw new Error(engine.error() || logs.slice(-3).join("\n") || "art3m1s を起動できません");
  const width = size >>> 16;
  const height = size & 0xffff;

  const { canvas } = ui;
  const presenter = await Presenter.create(canvas);
  const sound = new Sound(engine);
  attachInput(canvas, presenter, engine);
  document.addEventListener("visibilitychange", () => engine.lifecycle(document.hidden ? 1 : 2));
  window.addEventListener("pagehide", () => {
    engine.lifecycle(0);
    saves.scan();
  });
  ui.hide();
  canvas.focus();

  let last = performance.now();
  let frames = 0;
  const frame = (now: number) => {
    try {
      const delta = Math.max(0, Math.min(100, Math.round(now - last)));
      last = now;
      const length = engine.frame(delta);
      if (length === width * height * 4) presenter.show(engine.pixels(length), width, height);
      handleEvents(engine, sound);
      if ((frames += 1) % 60 === 0) saves.scan();
      if (engine.exitRequested()) {
        saves.scan();
        void ui.exit();
        return;
      }
    } catch (error) {
      window.dispatchEvent(new ErrorEvent("error", { error }));
      return;
    }
    requestAnimationFrame(frame);
  };
  requestAnimationFrame(frame);
}
