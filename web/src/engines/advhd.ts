// AdvHD (WillPlus) through our own engine (crates/advhd), built by engines/advhd/build.sh into
// public/engines/advhd/. The engine runs the scripts and says which pictures to draw where; this
// side runs it 60 times a second, draws the pictures with WebGPU, plays its sounds with WebAudio
// and passes clicks and saves.
import { importEngine, type EngineContext } from "../engine";
import type { GameFS } from "../gamefs";
import { Compositor } from "../compositor";
import { Presenter } from "../present";

interface Game {
  width(): number;
  height(): number;
  frame(): void;
  ended(): boolean;
  click(x: number, y: number): void;
  move_to(x: number, y: number): void;
  input_wanted(): boolean;
  enter_text(text: string): void;
  draws(): Float32Array;
  texture(id: number): Uint8Array;
  sounds(): SoundEvent[];
}

type SoundEvent =
  | { kind: "play"; ch: string; data: Uint8Array; looped: boolean; volume: number; fade: number }
  | { kind: "stop"; ch: string; fade: number };

interface AdvhdModule {
  default(): Promise<unknown>;
  Game: new (font: Uint8Array) => Game;
}

// Saves get their own folder so they never shadow a game file of the same name.
const SAVES = "advhd-save";

// Sounds by channel ("bgm01", "se01", a character's voice); "*" stops them all.
class Sounds {
  private readonly context = new AudioContext();
  private readonly playing = new Map<string, { source: AudioBufferSourceNode; gain: GainNode }>();

  constructor() {
    // Browsers start audio suspended until the first tap or key.
    const resume = () => void this.context.resume();
    window.addEventListener("pointerdown", resume, { once: true });
    window.addEventListener("keydown", resume, { once: true });
  }

  private stop(ch: string, fade: number): void {
    const s = this.playing.get(ch);
    if (!s) return;
    this.playing.delete(ch);
    const now = this.context.currentTime;
    s.gain.gain.cancelScheduledValues(now);
    s.gain.gain.setValueAtTime(s.gain.gain.value, now);
    s.gain.gain.linearRampToValueAtTime(0, now + fade / 1000);
    s.source.stop(now + fade / 1000);
  }

  handle(e: SoundEvent): void {
    if (e.kind === "stop") {
      for (const ch of e.ch === "*" ? [...this.playing.keys()] : [e.ch]) this.stop(ch, e.fade);
      return;
    }
    this.stop(e.ch, 0);
    // a later play on the channel replaces this one even before it is decoded
    const source = this.context.createBufferSource();
    const gain = this.context.createGain();
    this.playing.set(e.ch, { source, gain });
    this.context.decodeAudioData(e.data.slice().buffer).then(
      (buffer) => {
        if (this.playing.get(e.ch)?.source !== source) return;
        source.buffer = buffer;
        source.loop = e.looped;
        const now = this.context.currentTime;
        gain.gain.setValueAtTime(e.fade > 0 ? 0 : e.volume, now);
        if (e.fade > 0) gain.gain.linearRampToValueAtTime(e.volume, now + e.fade / 1000);
        source.connect(gain).connect(this.context.destination);
        source.onended = () => {
          if (this.playing.get(e.ch)?.source === source) this.playing.delete(e.ch);
        };
        source.start();
      },
      (error: unknown) => console.warn(`sound ${e.ch}: ${String(error)}`),
    );
  }
}

function bind(fs: GameFS): void {
  const save = (name: string) => `${SAVES}/${name}`;
  Object.assign(globalThis, {
    advhdFileSize: (path: string) => (fs.exists(path) ? fs.size(path) : -1),
    advhdReadRange: (path: string, offset: number, length: number) =>
      fs.exists(path) ? fs.readRange(path, offset, length) : undefined,
    advhdList: () => fs.list(""),
    advhdReadSave: (name: string) => (fs.exists(save(name)) ? fs.read(save(name)) : undefined),
    advhdWriteSave: (name: string, bytes: Uint8Array) => fs.write(save(name), bytes.slice()),
  });
}

export async function start({ fs, ui, fonts }: EngineContext): Promise<void> {
  bind(fs);
  const presenter = await Presenter.create(ui.canvas);
  const advhd = await importEngine<AdvhdModule>("advhd", "advhd_web.js");
  await advhd.default();
  const game = new advhd.Game(fonts[0] ?? new Uint8Array());
  const compositor = new Compositor(presenter.device, game.width(), game.height());
  const sounds = new Sounds();
  const { canvas } = ui;
  canvas.addEventListener("pointerdown", (event) => {
    canvas.focus();
    if (event.button === 0) game.click(...presenter.toFrame(event));
  });
  canvas.addEventListener("pointermove", (event) => game.move_to(...presenter.toFrame(event)));
  canvas.addEventListener("contextmenu", (event) => event.preventDefault());
  canvas.addEventListener("keydown", (event) => {
    if (event.code !== "Enter" && event.code !== "Space") return;
    event.preventDefault();
    game.click(-1, -1);
  });
  ui.hide();
  canvas.focus();

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
        game.frame();
        frames += 1;
        ran = true;
      }
      for (const e of game.sounds()) sounds.handle(e);
      if (game.ended()) {
        void ui.exit();
        return;
      }
      // A text field (the hero's name): the browser's own prompt stands in for it.
      if (game.input_wanted()) game.enter_text(prompt("名前を入力してください") ?? "");
      if (ran) presenter.showTexture(compositor.render(game));
    } catch (error) {
      window.dispatchEvent(new ErrorEvent("error", { error }));
      return;
    }
    requestAnimationFrame(tick);
  };
  requestAnimationFrame(tick);
}
