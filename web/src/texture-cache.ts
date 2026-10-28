// GPU copies of an engine's pictures, kept up to a total size: the pictures drawn longest ago
// give their memory back first. A 1920x1080 picture is 8 MB, and a game that shows many of them
// (Majiro's transitions) would otherwise hold gigabytes, which loses the GPU device on
// low-memory machines.

/** The default budget: about forty full-screen pictures at 1920x1080. */
export const TEXTURE_BUDGET = 320 * 1024 * 1024;

export class TextureCache<T extends { texture: GPUTexture }> {
  // In order of use, the least recently used first.
  private readonly entries = new Map<number, { value: T; used: number; bytes: number }>();
  private bytes = 0;

  constructor(private readonly budget = TEXTURE_BUDGET) {}

  /** The entry for `id`, marked as used in `frame`. */
  get(id: number, frame: number): T | undefined {
    const e = this.entries.get(id);
    if (!e) return undefined;
    e.used = frame;
    this.entries.delete(id);
    this.entries.set(id, e);
    return e.value;
  }

  /** Keeps `value` for `id`; a different texture held before is destroyed. */
  set(id: number, value: T, frame: number): void {
    const old = this.entries.get(id);
    if (old) {
      this.bytes -= old.bytes;
      if (old.value.texture !== value.texture) old.value.texture.destroy();
      this.entries.delete(id);
    }
    const bytes = value.texture.width * value.texture.height * 4;
    this.entries.set(id, { value, used: frame, bytes });
    this.bytes += bytes;
  }

  /** Destroys the least recently used textures while over budget, never one used in `frame`. */
  trim(frame: number): void {
    for (const [id, e] of this.entries) {
      if (this.bytes <= this.budget || e.used >= frame) break;
      e.value.texture.destroy();
      this.entries.delete(id);
      this.bytes -= e.bytes;
    }
  }
}
