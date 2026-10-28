// YU-RIS through our own engine (crates/yuris), built by engines/yuris/build.sh into
// public/engines/yuris/. The engine runs the scripts and keeps the layers' pixels; this side
// runs it once a frame, composes its layers with WebGPU (blend modes, masks and transitions as
// in crates/yuris/src/layers.rs), plays its sounds with WebAudio and passes input and saves.
import { importEngine, type EngineContext } from "../engine";
import type { GameFS } from "../gamefs";
import { Presenter } from "../present";
import { TextureCache } from "../texture-cache";

interface Game {
  width(): number;
  height(): number;
  frame(now: number, date: Int32Array): void;
  ended(): boolean;
  draws(): Int32Array;
  texture_version(tex: number): number;
  texture_width(tex: number): number;
  texture_height(tex: number): number;
  texture(tex: number): Uint8Array;
  set_mouse(x: number, y: number): void;
  set_button(code: number, held: boolean): void;
  wheel(delta: number): void;
  events(): GameEvent[];
  sound_ended(id: string): void;
}

interface YurisModule {
  default(): Promise<unknown>;
  Game: new (font: Uint8Array) => Game;
}

type GameEvent =
  | {
      kind: "play";
      id: string;
      data: Uint8Array;
      looped: boolean;
      volume: number;
      fade: number;
      start: number;
    }
  | { kind: "stop"; id: string; fade: number }
  | { kind: "volume"; id: string; volume: number; fade: number }
  | { kind: "pause"; id: string; paused: boolean }
  | { kind: "title"; title: string };

// Saves get their own folder so they never shadow a game file of the same name.
const SAVES = "yuris-save";

function bind(fs: GameFS): void {
  const save = (name: string) => `${SAVES}/${name}`;
  Object.assign(globalThis, {
    yurisFileSize: (path: string) => (fs.exists(path) ? fs.size(path) : -1),
    yurisReadRange: (path: string, offset: number, length: number) =>
      fs.exists(path) ? fs.readRange(path, offset, length) : undefined,
    yurisList: (dir: string) => fs.list(dir),
    yurisReadSave: (name: string) => (fs.exists(save(name)) ? fs.read(save(name)) : undefined),
    yurisWriteSave: (name: string, bytes: Uint8Array) => fs.write(save(name), bytes.slice()),
    yurisRemoveSave: (name: string) => {
      if (fs.exists(save(name))) fs.remove(save(name));
    },
    yurisDialog: (text: string, caption: string, buttons: number) => {
      const message = caption ? `${caption}\n\n${text}` : text;
      if (buttons <= 1) {
        alert(message);
        return 1;
      }
      return confirm(message) ? 1 : 2;
    },
  });
}

// A font for game text: one served with the player, else a font file in the game folder.
function chooseFont(fs: GameFS, fonts: Uint8Array[]): Uint8Array {
  if (fonts.length > 0) return fonts[0];
  const own = fs.list("").find((name) => /\.(ttf|otf|ttc)$/i.test(name));
  return own ? fs.read(own) : new Uint8Array();
}

// One layer drawn over the screen so far: `dst` is the screen, the output replaces it inside
// the scissor rectangle (the layer cut to its clip). Same math as Image::draw.
const SHADER = /* wgsl */ `
struct Draw {
  // layer top left, blend mode, flags (1 mask, 2 rule)
  at: vec4i,
  // mask top left, rule top left
  offsets: vec4i,
  // alpha, rule progress, rule softness
  values: vec4f,
};

@group(0) @binding(0) var<uniform> d: Draw;
@group(0) @binding(1) var src: texture_2d<f32>;
@group(0) @binding(2) var dst: texture_2d<f32>;
@group(0) @binding(3) var mask: texture_2d<f32>;
@group(0) @binding(4) var rule: texture_2d<f32>;

@vertex fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4f {
  let p = vec2f(f32((i << 1u) & 2u), f32(i & 2u));
  return vec4f(p * 2.0 - 1.0, 0.0, 1.0);
}

fn inside(t: texture_2d<f32>, q: vec2i) -> bool {
  let size = vec2i(textureDimensions(t));
  return all(q >= vec2i(0)) && all(q < size);
}

fn ruleAlpha(g: f32, t: f32, soft: f32) -> f32 {
  if (soft <= 0.0) {
    return select(0.0, 1.0, g < t);
  }
  return clamp((t * (1.0 + soft) - g) / soft, 0.0, 1.0);
}

fn mixOne(mode: i32, d: f32, s: f32) -> f32 {
  switch mode {
    case 1: { return min(d + s, 1.0); }
    case 2: { return max(d - s, 0.0); }
    case 3: { return d * s; }
    case 4: { if (s <= 0.0) { return 1.0; } return min(d / s, 1.0); }
    case 5: { return 1.0 - (1.0 - d) * (1.0 - s); }
    case 6: { return max(d, s); }
    case 7: { return min(d, s); }
    case 8: { return abs(d - s); }
    case 9: { return d + s - 2.0 * d * s; }
    case 10: { if (d < 0.5) { return 2.0 * d * s; } return 1.0 - 2.0 * (1.0 - d) * (1.0 - s); }
    case 11: { if (s >= 1.0) { return 1.0; } return min(d / (1.0 - s), 1.0); }
    case 12: { if (s <= 0.0) { return 0.0; } return 1.0 - min((1.0 - d) / s, 1.0); }
    case 13: { return max(d + s - 1.0, 0.0); }
    case 14: { if (s < 0.5) { return 2.0 * d * s; } return 1.0 - 2.0 * (1.0 - d) * (1.0 - s); }
    case 15: {
      if (s < 0.5) { return d - (1.0 - 2.0 * s) * d * (1.0 - d); }
      return d + (2.0 * s - 1.0) * (sqrt(d) - d);
    }
    case 16: {
      if (s < 0.5) {
        if (s <= 0.0) { return 0.0; }
        return 1.0 - min((1.0 - d) / (2.0 * s), 1.0);
      }
      if (s >= 1.0) { return 1.0; }
      return min(d / (2.0 * (1.0 - s)), 1.0);
    }
    case 17: { return clamp(d + 2.0 * s - 1.0, 0.0, 1.0); }
    case 18: { if (s < 0.5) { return min(d, 2.0 * s); } return max(d, 2.0 * s - 1.0); }
    default: { return s; }
  }
}

@fragment fn fs(@builtin(position) position: vec4f) -> @location(0) vec4f {
  let p = vec2i(floor(position.xy));
  let under = textureLoad(dst, p, 0);
  let s = textureLoad(src, p - d.at.xy, 0);
  var sa = s.a * d.values.x;
  if ((d.at.w & 1) != 0) {
    let q = p - d.offsets.xy;
    sa *= select(0.0, textureLoad(mask, q, 0).a, inside(mask, q));
  }
  if ((d.at.w & 2) != 0) {
    let q = p - d.offsets.zw;
    var g = 1.0;
    if (inside(rule, q)) {
      let c = textureLoad(rule, q, 0).rgb * 255.0;
      g = floor((c.r * 77.0 + c.g * 150.0 + c.b * 29.0) / 256.0) / 255.0;
    }
    sa *= ruleAlpha(g, d.values.y, d.values.z);
  }
  if (sa <= 0.0) {
    return under;
  }
  let dc = under.rgb;
  let da = under.a;
  let b = vec3f(mixOne(d.at.z, dc.r, s.r), mixOne(d.at.z, dc.g, s.g), mixOne(d.at.z, dc.b, s.b));
  // Where the screen is see-through, the blend has nothing to work on.
  let c = b * da + s.rgb * (1.0 - da);
  let oa = sa + da * (1.0 - sa);
  return vec4f(clamp((c * sa + dc * da * (1.0 - sa)) / max(oa, 1e-6), vec3f(0.0), vec3f(1.0)), oa);
}
`;

// Numbers per draw in Game.draws (crates/yuris-web).
const FIELDS = 18;
// Uniform slots are 256 bytes apart (minUniformBufferOffsetAlignment).
const SLOT = 256;

interface Texture {
  texture: GPUTexture;
  version: number;
}

class Compositor {
  private readonly textures = new TextureCache<Texture>();
  private readonly pipeline: GPURenderPipeline;
  private readonly layout: GPUBindGroupLayout;
  private readonly none: GPUTexture;
  private uniforms: GPUBuffer;
  private frame = 0;
  // The screen so far, and the target a layer is drawn into before it is copied back.
  private readonly screen: GPUTexture;
  private readonly scratch: GPUTexture;

  constructor(
    private readonly device: GPUDevice,
    readonly width: number,
    readonly height: number,
  ) {
    const module = device.createShaderModule({ code: SHADER });
    const texture = { sampleType: "unfilterable-float" } as const;
    this.layout = device.createBindGroupLayout({
      entries: [
        {
          binding: 0,
          visibility: GPUShaderStage.FRAGMENT,
          buffer: { type: "uniform", hasDynamicOffset: true },
        },
        { binding: 1, visibility: GPUShaderStage.FRAGMENT, texture },
        { binding: 2, visibility: GPUShaderStage.FRAGMENT, texture },
        { binding: 3, visibility: GPUShaderStage.FRAGMENT, texture },
        { binding: 4, visibility: GPUShaderStage.FRAGMENT, texture },
      ],
    });
    this.pipeline = device.createRenderPipeline({
      layout: device.createPipelineLayout({ bindGroupLayouts: [this.layout] }),
      vertex: { module, entryPoint: "vs" },
      fragment: { module, entryPoint: "fs", targets: [{ format: "rgba8unorm" }] },
      primitive: { topology: "triangle-list" },
    });
    this.none = device.createTexture({
      size: [1, 1],
      format: "rgba8unorm",
      usage: GPUTextureUsage.TEXTURE_BINDING,
    });
    const target = (usage: number) =>
      device.createTexture({ size: [width, height], format: "rgba8unorm", usage });
    this.screen = target(
      GPUTextureUsage.TEXTURE_BINDING |
        GPUTextureUsage.RENDER_ATTACHMENT |
        GPUTextureUsage.COPY_DST |
        GPUTextureUsage.COPY_SRC,
    );
    this.scratch = target(GPUTextureUsage.RENDER_ATTACHMENT | GPUTextureUsage.COPY_SRC);
    this.uniforms = this.makeUniforms(64);
  }

  private makeUniforms(slots: number): GPUBuffer {
    return this.device.createBuffer({
      size: slots * SLOT,
      usage: GPUBufferUsage.UNIFORM | GPUBufferUsage.COPY_DST,
    });
  }

  // The GPU copy of a layer's pixels, uploaded again when the engine changed them.
  private texture(game: Game, tex: number, version: number): GPUTexture | null {
    let t = this.textures.get(tex, this.frame);
    if (!t || t.version !== version) {
      const w = game.texture_width(tex);
      const h = game.texture_height(tex);
      if (!w || !h) return null;
      if (!t || t.texture.width !== w || t.texture.height !== h) {
        const texture = this.device.createTexture({
          size: [w, h],
          format: "rgba8unorm",
          usage: GPUTextureUsage.TEXTURE_BINDING | GPUTextureUsage.COPY_DST,
        });
        t = { texture, version };
        this.textures.set(tex, t, this.frame);
      }
      const pixels = game.texture(tex);
      this.device.queue.writeTexture(
        { texture: t.texture },
        pixels as Uint8Array<ArrayBuffer>,
        { bytesPerRow: w * 4 },
        [w, h],
      );
      t.version = version;
    }
    return t.texture;
  }

  /** Draws the engine's layers; returns the screen texture. */
  render(game: Game): GPUTexture {
    this.frame += 1;
    const draws = game.draws();
    const count = draws.length / FIELDS;
    if (this.uniforms.size < count * SLOT) {
      this.uniforms.destroy();
      this.uniforms = this.makeUniforms(Math.max(count, (this.uniforms.size / SLOT) * 2));
    }
    const data = new ArrayBuffer(Math.max(1, count) * SLOT);
    const encoder = this.device.createCommandEncoder();
    encoder
      .beginRenderPass({
        colorAttachments: [
          { view: this.screen.createView(), clearValue: [0, 0, 0, 0], loadOp: "clear", storeOp: "store" },
        ],
      })
      .end();
    const screenView = this.screen.createView();
    const scratchView = this.scratch.createView();
    for (let i = 0; i < count; i += 1) {
      const f = draws.subarray(i * FIELDS, (i + 1) * FIELDS);
      const [tex, version, x, y, a, mode, cx0, cy0, cx1, cy1, mt, mx, my, rt, rx, ry, t, ta] = f;
      const src = this.texture(game, tex, version);
      if (!src) continue;
      // The layer cut to its clip and the screen.
      const x0 = Math.max(x, cx0, 0);
      const y0 = Math.max(y, cy0, 0);
      const x1 = Math.min(x + src.width, cx1, this.width);
      const y1 = Math.min(y + src.height, cy1, this.height);
      if (x1 <= x0 || y1 <= y0 || a <= 0) continue;
      const maskTexture = mt >= 0 ? this.texture(game, mt, game.texture_version(mt)) : null;
      const ruleTexture = rt >= 0 ? this.texture(game, rt, game.texture_version(rt)) : null;
      const ints = new Int32Array(data, i * SLOT, 8);
      ints.set([x, y, mode, (maskTexture ? 1 : 0) | (ruleTexture ? 2 : 0), mx, my, rx, ry]);
      new Float32Array(data, i * SLOT + 32, 4).set([
        Math.min(a, 256) / 256,
        Math.min(Math.max(t, 0), 256) / 256,
        Math.max(ta, 0) / 256,
        0,
      ]);
      const group = this.device.createBindGroup({
        layout: this.layout,
        entries: [
          { binding: 0, resource: { buffer: this.uniforms, size: 48 } },
          { binding: 1, resource: src.createView() },
          { binding: 2, resource: screenView },
          { binding: 3, resource: (maskTexture || this.none).createView() },
          { binding: 4, resource: (ruleTexture || this.none).createView() },
        ],
      });
      const pass = encoder.beginRenderPass({
        colorAttachments: [{ view: scratchView, loadOp: "load", storeOp: "store" }],
      });
      pass.setPipeline(this.pipeline);
      pass.setBindGroup(0, group, [i * SLOT]);
      pass.setScissorRect(x0, y0, x1 - x0, y1 - y0);
      pass.draw(3);
      pass.end();
      encoder.copyTextureToTexture(
        { texture: this.scratch, origin: [x0, y0] },
        { texture: this.screen, origin: [x0, y0] },
        [x1 - x0, y1 - y0],
      );
    }
    this.device.queue.writeBuffer(this.uniforms, 0, data);
    this.device.queue.submit([encoder.finish()]);
    // Over budget, the layers drawn longest ago give their memory back.
    this.textures.trim(this.frame);
    return this.screen;
  }
}

// Sounds by the engine's sound layer id.
class Sounds {
  private readonly context = new AudioContext();
  private readonly playing = new Map<string, { source: AudioBufferSourceNode; gain: GainNode }>();
  // Plays started later than this were replaced before their data was decoded.
  private serial = 0;

  constructor(private readonly game: Game) {
    // Browsers start audio suspended until the first tap or key.
    const resume = () => void this.context.resume();
    window.addEventListener("pointerdown", resume, { once: true });
    window.addEventListener("keydown", resume, { once: true });
  }

  private ramp(param: AudioParam, value: number, fade: number): void {
    const now = this.context.currentTime;
    param.cancelScheduledValues(now);
    param.setValueAtTime(param.value, now);
    param.linearRampToValueAtTime(value, now + Math.max(fade, 0) / 1000);
  }

  private stop(id: string, fade: number): void {
    const s = this.playing.get(id);
    if (!s) return;
    this.playing.delete(id);
    s.source.onended = null;
    this.ramp(s.gain.gain, 0, fade);
    s.source.stop(this.context.currentTime + Math.max(fade, 0) / 1000);
  }

  handle(e: GameEvent): void {
    switch (e.kind) {
      case "play": {
        this.stop(e.id, 0);
        const serial = ++this.serial;
        this.context.decodeAudioData(e.data.slice().buffer).then(
          (buffer) => {
            if (serial !== this.serial && this.playing.has(e.id)) return;
            const source = this.context.createBufferSource();
            source.buffer = buffer;
            source.loop = e.looped;
            const gain = this.context.createGain();
            gain.gain.value = e.fade > 0 ? 0 : e.volume;
            if (e.fade > 0) this.ramp(gain.gain, e.volume, e.fade);
            source.connect(gain).connect(this.context.destination);
            source.onended = () => {
              if (this.playing.get(e.id)?.source !== source) return;
              this.playing.delete(e.id);
              this.game.sound_ended(e.id);
            };
            source.start(0, Math.max(e.start, 0) / 1000);
            this.playing.set(e.id, { source, gain });
          },
          (error: unknown) => {
            console.warn(`sound ${e.id}: ${String(error)}`);
            this.game.sound_ended(e.id);
          },
        );
        break;
      }
      case "stop":
        this.stop(e.id, e.fade);
        break;
      case "volume": {
        const s = this.playing.get(e.id);
        if (s) this.ramp(s.gain.gain, e.volume, e.fade);
        break;
      }
      case "pause": {
        const s = this.playing.get(e.id);
        if (s) this.ramp(s.gain.gain, e.paused ? 0 : 1, 0);
        break;
      }
      case "title":
        if (e.title) document.title = e.title;
        break;
    }
  }
}

// KeyboardEvent.code -> YU-RIS button code (crates/yuris/src/sysvar.rs).
function keyCode(code: string): number | undefined {
  let m = /^Key([A-Z])$/.exec(code);
  if (m) return 21 + m[1].charCodeAt(0) - 65;
  m = /^Digit([0-9])$/.exec(code);
  if (m) return 47 + Number(m[1]);
  m = /^F([0-9]+)$/.exec(code);
  if (m && Number(m[1]) <= 24) return 56 + Number(m[1]);
  m = /^Numpad([0-9])$/.exec(code);
  if (m) return 81 + Number(m[1]);
  const named: Record<string, number> = {
    Backspace: 96,
    Tab: 97,
    Enter: 98,
    NumpadEnter: 98,
    Space: 99,
    ShiftLeft: 100,
    ShiftRight: 100,
    ControlLeft: 101,
    ControlRight: 101,
    AltLeft: 102,
    AltRight: 102,
    ArrowLeft: 103,
    ArrowRight: 104,
    ArrowUp: 105,
    ArrowDown: 106,
    Escape: 112,
    PageUp: 113,
    PageDown: 114,
    Home: 115,
    End: 116,
  };
  return named[code];
}

// Mouse buttons by PointerEvent.button: left, middle, right.
const MOUSE = [1, 9, 5];

// A press and its release can both come between two frames; the release then waits for the
// next frame, so that the scripts see the press.
class Buttons {
  private pressed = new Set<number>();
  private late = new Set<number>();

  constructor(private readonly game: Game) {}

  down(code: number): void {
    this.late.delete(code);
    this.pressed.add(code);
    this.game.set_button(code, true);
  }

  up(code: number): void {
    if (this.pressed.has(code)) this.late.add(code);
    else this.game.set_button(code, false);
  }

  /** Call after each frame. */
  ran(): void {
    this.pressed.clear();
    for (const code of this.late) this.game.set_button(code, false);
    this.late.clear();
  }
}

function attachInput(canvas: HTMLCanvasElement, game: Game, presenter: Presenter): Buttons {
  const buttons = new Buttons(game);
  const move = (event: PointerEvent | WheelEvent) => game.set_mouse(...presenter.toFrame(event));
  canvas.addEventListener("pointermove", move);
  canvas.addEventListener("pointerdown", (event) => {
    canvas.focus();
    canvas.setPointerCapture(event.pointerId);
    move(event);
    const code = MOUSE[event.button];
    if (code) buttons.down(code);
  });
  canvas.addEventListener("pointerup", (event) => {
    move(event);
    const code = MOUSE[event.button];
    if (code) buttons.up(code);
  });
  canvas.addEventListener("contextmenu", (event) => event.preventDefault());
  canvas.addEventListener(
    "wheel",
    (event) => {
      event.preventDefault();
      move(event);
      game.wheel(event.deltaY < 0 ? 1 : -1);
    },
    { passive: false },
  );
  canvas.addEventListener("keydown", (event) => {
    const code = keyCode(event.code);
    if (code === undefined) return;
    event.preventDefault();
    if (!event.repeat) buttons.down(code);
  });
  canvas.addEventListener("keyup", (event) => {
    const code = keyCode(event.code);
    if (code === undefined) return;
    event.preventDefault();
    buttons.up(code);
  });
  return buttons;
}

function localDate(): Int32Array {
  const d = new Date();
  return Int32Array.from([
    d.getFullYear(),
    d.getMonth() + 1,
    d.getDate(),
    d.getDay(),
    d.getHours(),
    d.getMinutes(),
    d.getSeconds(),
    d.getMilliseconds(),
  ]);
}

export async function start({ fs, ui, fonts }: EngineContext): Promise<void> {
  bind(fs);
  const presenter = await Presenter.create(ui.canvas);
  const yuris = await importEngine<YurisModule>("yuris", "yuris_web.js");
  await yuris.default();
  const game = new yuris.Game(chooseFont(fs, fonts));
  let compositor = new Compositor(presenter.device, game.width(), game.height());
  const sounds = new Sounds(game);
  const buttons = attachInput(ui.canvas, game, presenter);
  ui.hide();
  ui.canvas.focus();

  // The scripts count on 60 frames a second.
  const step = 1000 / 60;
  const begin = performance.now();
  let frames = 0;
  const tick = () => {
    try {
      const now = performance.now() - begin;
      // Catch up after a slow frame, but not after the tab slept.
      if (now - frames * step > 250) frames = Math.floor(now / step);
      let ran = false;
      while (frames * step <= now) {
        game.frame(frames * step, localDate());
        buttons.ran();
        frames += 1;
        ran = true;
        for (const e of game.events()) sounds.handle(e);
      }
      if (game.ended()) {
        void ui.exit();
        return;
      }
      if (ran) {
        // The scripts may size the window after the start.
        if (compositor.width !== game.width() || compositor.height !== game.height()) {
          compositor = new Compositor(presenter.device, game.width(), game.height());
        }
        presenter.showTexture(compositor.render(game));
      }
    } catch (error) {
      window.dispatchEvent(new ErrorEvent("error", { error }));
      return;
    }
    requestAnimationFrame(tick);
  };
  requestAnimationFrame(tick);
}
