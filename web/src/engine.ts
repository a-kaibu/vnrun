// What the player hands to an engine module (src/engines/<id>.ts), and how a module finds the
// engine build that engines/<id>/build.sh put under public/engines/<id>/.
import type { GameFS, Manifest } from "./gamefs";

export interface Ui {
  /** The page's canvas; the engine owns it once started. */
  canvas: HTMLCanvasElement;
  /** Bar buttons an engine may show and use. */
  menu: HTMLButtonElement;
  skip: HTMLButtonElement;
  /** Shows a message over the screen. */
  show(message: string): void;
  /** Hides the message once the game draws. */
  hide(): void;
  /** Sends the remaining saves and leaves the page. */
  exit(): Promise<void>;
}

export interface EngineContext {
  fs: GameFS;
  manifest: Manifest;
  ui: Ui;
  /** Fonts from public/fonts/fonts.json, for engines that cannot see the system's. */
  fonts: Uint8Array[];
}

export interface EngineModule {
  start(context: EngineContext): Promise<void>;
}

/** URL of a file of an engine build (public/engines/<id>/<file>). */
export function engineUrl(id: string, file: string): string {
  return new URL(`engines/${id}/${file}`, document.baseURI).href;
}

/**
 * Imports a module of an engine build. Builds are not part of the bundle: they are made by
 * engines/<id>/build.sh and served as they are.
 */
export function importEngine<T>(id: string, file: string): Promise<T> {
  return import(/* @vite-ignore */ engineUrl(id, file)) as Promise<T>;
}
