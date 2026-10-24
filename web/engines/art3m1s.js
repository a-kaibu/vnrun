// Artemis through art3m1s-core (third_party/art3m1s-core), built with its browser host
// (engines/art3m1s/host) by engines/art3m1s/build.sh into ./art3m1s/. The engine is an
// Emscripten module: game files appear under /game as lazy files read in blocks from the
// GameFS, saves live under /saves and are copied back to the GameFS, and the engine renders
// with WebGL2 into its own framebuffer whose pixels are drawn onto the page's canvas. Sound
// is the host's job in art3m1s (media events), played here with Web Audio.
import createArt3m1s from "./art3m1s/art3m1s.js";

// Saves get their own folder so they never shadow a game file of the same name.
const SAVES = "art3m1s-save";
const BLOCK = 256 * 1024;
const CACHE_BLOCKS = 512; // 128MB

// --- Files ------------------------------------------------------------------

// Reads game files in blocks, keeping the most recently used ones.
function blockReader(fs) {
  const blocks = new Map(); // `${path}\n${index}` -> Uint8Array
  return (path, size, position, out, offset, length) => {
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
        if (blocks.size >= CACHE_BLOCKS) blocks.delete(blocks.keys().next().value);
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

function parentDir(path) {
  return path.slice(0, path.lastIndexOf("/"));
}

// A read-only file of `size` bytes whose reads go to `read` (after Emscripten's createLazyFile,
// which only works in workers).
function lazyFile(FS, path, size, read) {
  FS.mkdirTree(parentDir(path));
  const node = FS.createFile(parentDir(path), path.slice(path.lastIndexOf("/") + 1), {}, true, false);
  node.contents = { length: size }; // never read: the stream ops below replace MEMFS's
  Object.defineProperties(node, { usedBytes: { get: () => size } });
  const ops = { ...node.stream_ops };
  ops.read = (stream, buffer, offset, length, position) => read(buffer, offset, length, position);
  ops.mmap = () => {
    throw new FS.ErrnoError(28); // EINVAL
  };
  node.stream_ops = ops;
}

function mountGame(FS, fs, manifest) {
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
  constructor(FS, fs) {
    this.FS = FS;
    this.fs = fs;
    this.seen = new Map(); // path under /saves -> mtime
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

  files(dir = "/saves", out = []) {
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
const gainOf = (gain) => (gain == null ? 1 : Math.min(1, Math.max(0, gain > 1 ? gain / 1000 : gain)));
const panOf = (pan) => (pan == null ? 0 : Math.min(1, Math.max(-1, Math.abs(pan) > 1 ? pan / 1000 : pan)));

class Sound {
  constructor(engine) {
    this.engine = engine;
    this.context = new AudioContext();
    const resume = () => this.context.resume();
    window.addEventListener("pointerdown", resume);
    window.addEventListener("keydown", resume);
    this.channels = {};
    this.master = this.context.createGain();
    this.master.connect(this.context.destination);
    for (const name of ["bgm", "se", "voice"]) {
      this.channels[name] = this.context.createGain();
      this.channels[name].connect(this.master);
    }
    this.playing = new Map(); // "bgm" or `se:${id}` / `voice:${id}` -> player
    this.buffers = new Map(); // path -> Promise<AudioBuffer>
    this.generation = 0;
  }

  buffer(path) {
    let buffer = this.buffers.get(path);
    if (!buffer) {
      const bytes = this.engine.read(path);
      buffer = bytes ? this.context.decodeAudioData(bytes.buffer) : Promise.reject(new Error(`no ${path}`));
      if (this.buffers.size > 64) this.buffers.delete(this.buffers.keys().next().value);
      this.buffers.set(path, buffer);
    }
    return buffer;
  }

  // Plays `file` (then `loopFile` repeatedly, if given) on a channel. `done` runs once when a
  // sound that does not loop ends by itself.
  play(key, channel, p, done) {
    this.stop(key, 0);
    const generation = ++this.generation;
    const gain = this.context.createGain();
    const panner = this.context.createStereoPanner();
    gain.gain.value = 0;
    panner.pan.value = panOf(p.pan);
    gain.connect(panner).connect(this.channels[channel]);
    const player = { generation, gain, panner, sources: [], stopped: false };
    this.playing.set(key, player);
    const file = p.resolved_file || p.file;
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
    start().catch((error) => {
      console.warn(`art3m1s sound ${file}:`, error);
      // A sound that cannot play still finishes, so a script waiting on it goes on.
      if (!player.stopped && this.playing.get(key) === player) {
        this.playing.delete(key);
        if (!p.loop) done();
      }
    });
  }

  stop(key, fadeMs) {
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

  fade(key, gain, timeMs) {
    const player = this.playing.get(key);
    if (!player) return;
    const t = this.context.currentTime;
    player.gain.gain.cancelScheduledValues(t);
    player.gain.gain.setValueAtTime(player.gain.gain.value, t);
    player.gain.gain.linearRampToValueAtTime(gainOf(gain), t + (timeMs || 0) / 1000);
  }

  pan(key, pan, timeMs) {
    const player = this.playing.get(key);
    if (!player) return;
    const t = this.context.currentTime;
    player.panner.pan.linearRampToValueAtTime(panOf(pan), t + (timeMs || 0) / 1000);
  }

  handle(kind, p) {
    const finished = (id) => () => this.engine.soundFinished(id);
    switch (kind) {
      case "audio_set_volume": {
        const node = p.channel === "master" ? this.master : this.channels[p.channel];
        if (node) node.gain.value = Math.min(1, Math.max(0, p.value));
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
        this.play(`se:${p.id}`, "se", p, finished(p.id));
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
        this.play(`voice:${p.id}`, "voice", p, finished(p.id));
        break;
      case "audio_stop_all":
        for (const key of [...this.playing.keys()]) this.stop(key, 0);
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

function bindEngine(Module) {
  const text = (value) => Module.stringToNewUTF8(value);
  const withText = (value, call) => {
    const ptr = text(value);
    try {
      return call(ptr);
    } finally {
      Module._free(ptr);
    }
  };
  return {
    start: (archive, platform) =>
      withText(archive, (a) => withText(platform, (p) => Module._vn_start(a, p))),
    error: () => Module.UTF8ToString(Module._vn_error()),
    frame: (delta) => Module._vn_frame(delta),
    pixels: (length) => Module.HEAPU8.subarray(Module._vn_pixels(), Module._vn_pixels() + length),
    exitRequested: () => Module._vn_exit_requested() !== 0,
    mouse: (x, y) => Module._vn_mouse(x, y),
    button: (vk, pressed) => Module._vn_button(vk, pressed ? 1 : 0),
    key: (vk, pressed) => Module._vn_key(vk, pressed ? 1 : 0),
    events: () => {
      const length = Module._vn_poll_events();
      return length ? Module.HEAPU8.slice(Module._vn_out(), Module._vn_out() + length) : null;
    },
    read: (path) => {
      const length = withText(path, (p) => Module._vn_read(p));
      return length < 0 ? null : Module.HEAPU8.slice(Module._vn_out(), Module._vn_out() + length);
    },
    soundFinished: (id) => withText(id, (p) => Module._vn_sound_finished(p)),
    videoFinished: (id) => withText(id, (p) => Module._vn_video_finished(p)),
    dialog: (accepted, value) => withText(value || "", (p) => Module._vn_dialog(accepted ? 1 : 0, p)),
    lifecycle: (state) => Module._vn_lifecycle(state),
  };
}

// Host events: 24-byte little-endian headers (version, kind, sequence, payload size, aux)
// each followed by its payload. Kinds: 1 log, 2 media, 3 UI.
function* hostEvents(bytes) {
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const decoder = new TextDecoder();
  for (let at = 0; at + 24 <= bytes.length; ) {
    const kind = view.getUint32(at + 4, true);
    const size = view.getUint32(at + 16, true);
    const aux = view.getUint32(at + 20, true);
    yield { kind, aux, text: decoder.decode(bytes.subarray(at + 24, at + 24 + size)) };
    at += 24 + size;
  }
}

function handleUi(engine, kind, p) {
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

// Fits the canvas into the area the page gives it, keeping the game's aspect ratio.
function fit(canvas) {
  const area = canvas.parentElement.getBoundingClientRect();
  const bar = document.getElementById("bar");
  const height = window.innerHeight - (bar ? bar.offsetHeight : 0);
  const scale = Math.min(area.width / canvas.width, height / canvas.height);
  const w = Math.floor(canvas.width * scale);
  const h = Math.floor(canvas.height * scale);
  canvas.style.width = `${w}px`;
  canvas.style.height = `${h}px`;
  canvas.style.left = `${Math.floor((area.width - w) / 2)}px`;
  canvas.style.top = `${Math.floor((height - h) / 2)}px`;
}

function attachInput(canvas, engine) {
  const point = (event) => [
    Math.round((event.offsetX * canvas.width) / canvas.clientWidth),
    Math.round((event.offsetY * canvas.height) / canvas.clientHeight),
  ];
  const vk = (event) => [1, 4, 2][event.button] || 0; // left, middle, right
  canvas.addEventListener("pointermove", (event) => engine.mouse(...point(event)));
  canvas.addEventListener("pointerdown", (event) => {
    if (!vk(event)) return;
    canvas.focus();
    canvas.setPointerCapture(event.pointerId);
    engine.mouse(...point(event));
    engine.button(vk(event), true);
  });
  canvas.addEventListener("pointerup", (event) => {
    if (!vk(event)) return;
    engine.mouse(...point(event));
    engine.button(vk(event), false);
  });
  canvas.addEventListener("contextmenu", (event) => event.preventDefault());
  // Keys use Windows virtual-key codes, which keyCode still reports.
  const key = (pressed) => (event) => {
    if (event.metaKey || !event.keyCode) return;
    if ([9, 13, 27, 32, 33, 34, 37, 38, 39, 40].includes(event.keyCode)) event.preventDefault();
    engine.key(event.keyCode, pressed);
  };
  window.addEventListener("keydown", key(true));
  window.addEventListener("keyup", key(false));
}

// The archive to mount: root.pfs (later volumes and patches are found beside it), or none
// for an unpacked game folder.
function archiveOf(manifest) {
  const names = (manifest.files || []).map((f) => f.p).filter((p) => !p.includes("/"));
  return names.find((n) => n.toLowerCase() === "root.pfs") || names.find((n) => /\.pfs$/i.test(n)) || "";
}

export async function start({ fs, manifest, ui }) {
  if (!document.createElement("canvas").getContext("webgl2")) {
    throw new Error("このブラウザは WebGL2 に対応していません");
  }
  const glCanvas = document.createElement("canvas"); // the engine's, never shown
  const logs = [];
  const Module = await createArt3m1s({
    canvas: glCanvas,
    print: (line) => console.info(line),
    printErr: (line) => {
      logs.push(line);
      console.warn(line);
    },
  });
  mountGame(Module.FS, fs, manifest);
  const saves = new Saves(Module.FS, fs);
  const engine = bindEngine(Module);
  const size = engine.start(archiveOf(manifest), manifest.platform || "WINDOWS");
  if (!size) throw new Error(engine.error() || logs.slice(-3).join("\n") || "art3m1s を起動できません");

  const { canvas } = ui;
  canvas.width = size >>> 16;
  canvas.height = size & 0xffff;
  fit(canvas);
  window.addEventListener("resize", () => fit(canvas));
  const context = canvas.getContext("2d");
  const image = context.createImageData(canvas.width, canvas.height);
  const sound = new Sound(engine);
  attachInput(canvas, engine);
  document.addEventListener("visibilitychange", () => engine.lifecycle(document.hidden ? 1 : 2));
  window.addEventListener("pagehide", () => {
    engine.lifecycle(0);
    saves.scan();
  });
  ui.hide();
  canvas.focus();

  let last = performance.now();
  let frames = 0;
  const frame = (now) => {
    try {
      const delta = Math.max(0, Math.min(100, Math.round(now - last)));
      last = now;
      const length = engine.frame(delta);
      if (length === image.data.length) {
        image.data.set(engine.pixels(length));
        context.putImageData(image, 0, 0);
      }
      const events = engine.events();
      if (events) {
        for (const event of hostEvents(events)) {
          if (event.kind === 1) {
            const level = String.fromCharCode(event.aux);
            (level === "E" ? console.error : level === "W" ? console.warn : console.debug)(`art3m1s: ${event.text}`);
          } else if (event.kind === 2 || event.kind === 3) {
            const { kind, payload } = JSON.parse(event.text);
            if (event.kind === 2) sound.handle(kind, payload || {});
            else handleUi(engine, kind, payload || {});
          }
        }
      }
      if ((frames += 1) % 60 === 0) saves.scan();
      if (engine.exitRequested()) {
        saves.scan();
        ui.exit();
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
