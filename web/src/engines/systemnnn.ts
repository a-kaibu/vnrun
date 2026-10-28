// System-NNN games (Black Cyc and others) through our own engine (crates/systemnnn), built by
// engines/systemnnn/build.sh into public/engines/systemnnn/. The engine runs the scripts and
// says which pictures to draw where; this side runs it 60 times a second, draws the pictures
// with WebGPU and passes clicks and saves.
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
  cancel(): void;
  wheel(up: boolean): void;
  point(x: number, y: number): void;
  draws(): Float32Array;
  texture(id: number): Uint8Array;
  sounds(): Sound[];
}

interface Sound {
  data: Uint8Array;
  volume: number;
  /** the game's sound channel (-1: all of them) */
  channel: number;
  stop: boolean;
  fadeMs: number;
  looped: boolean;
}

interface SystemNnnModule {
  default(): Promise<unknown>;
  Game: new (font: Uint8Array) => Game;
}

// Saves get their own folder so they never shadow a game file of the same name.
const SAVES = "systemnnn-save";

function bind(fs: GameFS): void {
  const save = (name: string) => `${SAVES}/${name}`;
  Object.assign(globalThis, {
    nnnFileSize: (path: string) => (fs.exists(path) ? fs.size(path) : -1),
    nnnReadRange: (path: string, offset: number, length: number) =>
      fs.exists(path) ? fs.readRange(path, offset, length) : undefined,
    nnnList: () => fs.list(""),
    nnnReadSave: (name: string) => (fs.exists(save(name)) ? fs.read(save(name)) : undefined),
    nnnWriteSave: (name: string, bytes: Uint8Array) => fs.write(save(name), bytes.slice()),
  });
}

export async function start({ fs, ui, fonts }: EngineContext): Promise<void> {
  bind(fs);
  const presenter = await Presenter.create(ui.canvas);
  const engine = await importEngine<SystemNnnModule>("systemnnn", "systemnnn_web.js");
  await engine.default();
  const game = new engine.Game(fonts[0] ?? new Uint8Array());
  const compositor = new Compositor(presenter.device, game.width(), game.height());
  const audio = new AudioContext();
  const channels = new Map<number, { source: AudioBufferSourceNode; gain: GainNode; started: boolean }>();
  const stopChannel = (channel: number, fadeMs: number) => {
    const previous = channels.get(channel);
    channels.delete(channel);
    if (!previous?.started) return;
    const now = audio.currentTime;
    previous.gain.gain.cancelScheduledValues(now);
    previous.gain.gain.setValueAtTime(previous.gain.gain.value, now);
    previous.gain.gain.linearRampToValueAtTime(0, now + fadeMs / 1000);
    previous.source.stop(now + fadeMs / 1000);
  };
  const playSound = (sound: Sound) => {
    const targets = sound.channel < 0 ? [...channels.keys()] : [sound.channel];
    for (const channel of targets) stopChannel(channel, sound.stop ? sound.fadeMs : 0);
    if (sound.stop) return;
    const source = audio.createBufferSource();
    const gain = audio.createGain();
    const pending = { source, gain, started: false };
    channels.set(sound.channel, pending);
    void audio
      .decodeAudioData(sound.data.slice().buffer)
      .then((buffer) => {
        if (channels.get(sound.channel) !== pending) return;
        source.buffer = buffer;
        source.loop = sound.looped;
        gain.gain.setValueAtTime(sound.volume, audio.currentTime);
        source.connect(gain).connect(audio.destination);
        source.start();
        pending.started = true;
        source.onended = () => {
          if (channels.get(sound.channel) === pending) channels.delete(sound.channel);
        };
      })
      .catch((error: unknown) => console.warn("System-NNN sound:", error));
  };
  const resumeAudio = () => void audio.resume();
  window.addEventListener("pointerdown", resumeAudio, { once: true });
  window.addEventListener("keydown", resumeAudio, { once: true });
  const { canvas } = ui;
  canvas.addEventListener("pointerdown", (event) => {
    canvas.focus();
    if (event.button === 0) game.click(...presenter.toFrame(event));
    else if (event.button === 2) game.cancel();
  });
  canvas.addEventListener("pointermove", (event) => game.point(...presenter.toFrame(event)));
  canvas.addEventListener("contextmenu", (event) => event.preventDefault());
  canvas.addEventListener(
    "wheel",
    (event) => {
      event.preventDefault();
      if (event.deltaY !== 0) game.wheel(event.deltaY < 0);
    },
    { passive: false },
  );
  canvas.addEventListener("keydown", (event) => {
    if (event.code === "Escape") {
      event.preventDefault();
      game.cancel();
      return;
    }
    if (event.code === "ArrowUp" || event.code === "ArrowDown") {
      event.preventDefault();
      game.wheel(event.code === "ArrowUp");
      return;
    }
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
        for (const sound of game.sounds()) playSound(sound);
        frames += 1;
        ran = true;
      }
      if (game.ended()) {
        void ui.exit();
        return;
      }
      if (ran) presenter.showTexture(compositor.render(game));
    } catch (error) {
      window.dispatchEvent(new ErrorEvent("error", { error }));
      return;
    }
    requestAnimationFrame(tick);
  };
  requestAnimationFrame(tick);
}
