// QLIE games through our own engine (crates/qlie), built by engines/qlie/build.sh into
// public/engines/qlie/. The engine reads the GameData/*.pack archives, runs the .s scripts and
// says which pictures to draw where; this side runs it 60 times a second, draws the pictures with
// WebGPU and plays sounds on the channels the engine names (snd3, cue40, ...).
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
  setTime(secs: number): void;
}

interface Sound {
  channel: string;
  data?: Uint8Array;
  volume: number;
  repeat: boolean;
  fadeMs: number;
}

interface QlieModule {
  default(): Promise<unknown>;
  Game: new (font: Uint8Array) => Game;
}

// Saves get their own folder so they never shadow a game file of the same name.
const SAVES = "qlie-save";

function bind(fs: GameFS): void {
  const save = (name: string) => `${SAVES}/${name}`;
  Object.assign(globalThis, {
    qlieFileSize: (path: string) => (fs.exists(path) ? fs.size(path) : -1),
    qlieReadRange: (path: string, offset: number, length: number) =>
      fs.exists(path) ? fs.readRange(path, offset, length) : undefined,
    qlieList: () => fs.list(""),
    qlieReadSave: (name: string) => (fs.exists(save(name)) ? fs.read(save(name)) : undefined),
    qlieWriteSave: (name: string, bytes: Uint8Array) => fs.write(save(name), bytes.slice()),
  });
}

export async function start({ fs, ui, fonts }: EngineContext): Promise<void> {
  bind(fs);
  const presenter = await Presenter.create(ui.canvas);
  const qlie = await importEngine<QlieModule>("qlie", "qlie_web.js");
  await qlie.default();
  const game = new qlie.Game(fonts[0] ?? new Uint8Array());
  // local time for the dates of saves
  const setTime = () => game.setTime(Date.now() / 1000 - new Date().getTimezoneOffset() * 60);
  setTime();
  const compositor = new Compositor(presenter.device, game.width(), game.height());
  const audio = new AudioContext();
  const channels = new Map<string, { source: AudioBufferSourceNode; gain: GainNode; started: boolean }>();
  const stop = (channel: string, fadeMs: number) => {
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
    stop(sound.channel, sound.data ? 0 : sound.fadeMs);
    if (!sound.data) return;
    const source = audio.createBufferSource();
    const gain = audio.createGain();
    const pending = { source, gain, started: false };
    channels.set(sound.channel, pending);
    void audio
      .decodeAudioData(sound.data.slice().buffer)
      .then((buffer) => {
        if (channels.get(sound.channel) !== pending) return;
        source.buffer = buffer;
        source.loop = sound.repeat;
        gain.gain.setValueAtTime(sound.volume, audio.currentTime);
        source.connect(gain).connect(audio.destination);
        source.start();
        pending.started = true;
        source.onended = () => {
          if (channels.get(sound.channel) === pending) channels.delete(sound.channel);
        };
      })
      .catch((error: unknown) => console.warn("QLIE sound:", error));
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
      if (frames % 60 === 0) setTime();
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
