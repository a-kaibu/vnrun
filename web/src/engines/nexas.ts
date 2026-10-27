// NeXAS through our own engine (crates/nexas), built by engines/nexas/build.sh into
// public/engines/nexas/. The engine runs the scripts and says which pictures to draw where; this
// side runs it 60 times a second, draws the pictures with WebGPU and passes clicks and saves.
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
  draws(): Float32Array;
  texture(id: number): Uint8Array;
  sounds(): { data: Uint8Array; volume: number }[];
}

interface NexasModule {
  default(): Promise<unknown>;
  Game: new (font: Uint8Array) => Game;
}

// Saves get their own folder so they never shadow a game file of the same name.
const SAVES = "nexas-save";

function bind(fs: GameFS): void {
  const save = (name: string) => `${SAVES}/${name}`;
  Object.assign(globalThis, {
    nexasFileSize: (path: string) => (fs.exists(path) ? fs.size(path) : -1),
    nexasReadRange: (path: string, offset: number, length: number) =>
      fs.exists(path) ? fs.readRange(path, offset, length) : undefined,
    nexasList: () => fs.list(""),
    nexasReadSave: (name: string) => (fs.exists(save(name)) ? fs.read(save(name)) : undefined),
    nexasWriteSave: (name: string, bytes: Uint8Array) => fs.write(save(name), bytes.slice()),
  });
}

export async function start({ fs, ui, fonts }: EngineContext): Promise<void> {
  bind(fs);
  const presenter = await Presenter.create(ui.canvas);
  const nexas = await importEngine<NexasModule>("nexas", "nexas_web.js");
  await nexas.default();
  const game = new nexas.Game(fonts[0] ?? new Uint8Array());
  const compositor = new Compositor(presenter.device, game.width(), game.height());
  const audio = new AudioContext();
  const resumeAudio = () => void audio.resume();
  window.addEventListener("pointerdown", resumeAudio, { once: true });
  window.addEventListener("keydown", resumeAudio, { once: true });
  const { canvas } = ui;
  canvas.addEventListener("pointerdown", (event) => {
    canvas.focus();
    if (event.button === 0) game.click(...presenter.toFrame(event));
  });
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
        for (const sound of game.sounds()) {
          void audio
            .decodeAudioData(sound.data.slice().buffer)
            .then((buffer) => {
              const source = audio.createBufferSource();
              const gain = audio.createGain();
              source.buffer = buffer;
              gain.gain.value = sound.volume;
              source.connect(gain).connect(audio.destination);
              source.start();
            })
            .catch((error: unknown) => console.warn("NeXAS sound:", error));
        }
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
