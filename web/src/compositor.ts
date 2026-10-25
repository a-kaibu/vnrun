// Draws an engine's list of pictures (Game.draws in crates/nexas-web and crates/advhd-web) with
// WebGPU: each picture is a quad from its four corners, sampling a rectangle of its texture,
// laid over or added, clipped to a rectangle of the screen.

/** What the compositor needs from an engine. */
export interface Pictures {
  draws(): Float32Array;
  texture(id: number): Uint8Array;
}

// One picture: a quad from its four corners, sampling a rectangle of its texture.
const SHADER = /* wgsl */ `
struct Draw {
  // top left, top right, bottom left, bottom right (screen pixels)
  corners: array<vec4f, 2>,
  // the rectangle of the texture (pixels): x0 y0 x1 y1
  src: vec4f,
  // texture width, height, opacity, screen width
  size: vec4f,
  // screen height
  screen: vec4f,
};

@group(0) @binding(0) var<uniform> d: Draw;
@group(0) @binding(1) var tex: texture_2d<f32>;
@group(0) @binding(2) var smp: sampler;

struct Out {
  @builtin(position) position: vec4f,
  @location(0) uv: vec2f,
};

@vertex fn vs(@builtin(vertex_index) i: u32) -> Out {
  // two triangles: TL TR BL, BL TR BR
  let corner = array<u32, 6>(0u, 1u, 2u, 2u, 1u, 3u)[i];
  let pair = d.corners[corner / 2u];
  let p = select(pair.xy, pair.zw, (corner & 1u) == 1u);
  let u = select(d.src.x, d.src.z, (corner & 1u) == 1u);
  let v = select(d.src.y, d.src.w, corner >= 2u);
  var out: Out;
  out.position = vec4f(p.x / d.size.w * 2.0 - 1.0, 1.0 - p.y / d.screen.x * 2.0, 0.0, 1.0);
  out.uv = vec2f(u, v) / d.size.xy;
  return out;
}

@fragment fn fs(in: Out) -> @location(0) vec4f {
  let c = textureSample(tex, smp, in.uv);
  return vec4f(c.rgb, c.a * d.size.z);
}
`;

// Numbers per draw in Game.draws (crates/nexas-web, crates/advhd-web).
const FIELDS = 21;
// Uniform slots are 256 bytes apart (minUniformBufferOffsetAlignment).
const SLOT = 256;

export class Compositor {
  private readonly textures = new Map<number, { texture: GPUTexture; used: number }>();
  private readonly normal: GPURenderPipeline;
  private readonly added: GPURenderPipeline;
  private readonly layout: GPUBindGroupLayout;
  private readonly sampler: GPUSampler;
  private uniforms: GPUBuffer;
  private frame = 0;
  readonly screen: GPUTexture;

  constructor(
    private readonly device: GPUDevice,
    readonly width: number,
    readonly height: number,
  ) {
    const module = device.createShaderModule({ code: SHADER });
    this.layout = device.createBindGroupLayout({
      entries: [
        {
          binding: 0,
          visibility: GPUShaderStage.VERTEX | GPUShaderStage.FRAGMENT,
          buffer: { type: "uniform", hasDynamicOffset: true },
        },
        { binding: 1, visibility: GPUShaderStage.FRAGMENT, texture: {} },
        { binding: 2, visibility: GPUShaderStage.FRAGMENT, sampler: {} },
      ],
    });
    const pipeline = (color: GPUBlendComponent, alpha: GPUBlendComponent) =>
      device.createRenderPipeline({
        layout: device.createPipelineLayout({ bindGroupLayouts: [this.layout] }),
        vertex: { module, entryPoint: "vs" },
        fragment: {
          module,
          entryPoint: "fs",
          targets: [{ format: "rgba8unorm", blend: { color, alpha } }],
        },
        primitive: { topology: "triangle-list" },
      });
    this.normal = pipeline(
      { srcFactor: "src-alpha", dstFactor: "one-minus-src-alpha" },
      { srcFactor: "one", dstFactor: "one-minus-src-alpha" },
    );
    this.added = pipeline(
      { srcFactor: "src-alpha", dstFactor: "one" },
      { srcFactor: "zero", dstFactor: "one" },
    );
    this.sampler = device.createSampler({ magFilter: "linear", minFilter: "linear" });
    this.screen = device.createTexture({
      size: [width, height],
      format: "rgba8unorm",
      usage: GPUTextureUsage.TEXTURE_BINDING | GPUTextureUsage.RENDER_ATTACHMENT,
    });
    this.uniforms = this.makeUniforms(64);
  }

  private makeUniforms(slots: number): GPUBuffer {
    return this.device.createBuffer({
      size: slots * SLOT,
      usage: GPUBufferUsage.UNIFORM | GPUBufferUsage.COPY_DST,
    });
  }

  // The GPU copy of a picture; pictures never change, so it is uploaded once.
  private texture(game: Pictures, id: number, w: number, h: number): GPUTexture | null {
    let t = this.textures.get(id);
    if (!t) {
      const pixels = game.texture(id);
      if (!w || !h || pixels.length < w * h * 4) return null;
      const texture = this.device.createTexture({
        size: [w, h],
        format: "rgba8unorm",
        usage: GPUTextureUsage.TEXTURE_BINDING | GPUTextureUsage.COPY_DST,
      });
      this.device.queue.writeTexture({ texture }, pixels as Uint8Array<ArrayBuffer>, { bytesPerRow: w * 4 }, [
        w,
        h,
      ]);
      t = { texture, used: this.frame };
      this.textures.set(id, t);
    }
    t.used = this.frame;
    return t.texture;
  }

  /** Draws the engine's pictures; returns the screen texture. */
  render(game: Pictures): GPUTexture {
    this.frame += 1;
    const draws = game.draws();
    const count = draws.length / FIELDS;
    if (this.uniforms.size < count * SLOT) {
      this.uniforms.destroy();
      this.uniforms = this.makeUniforms(Math.max(count, (this.uniforms.size / SLOT) * 2));
    }
    const data = new Float32Array((Math.max(1, count) * SLOT) / 4);
    const encoder = this.device.createCommandEncoder();
    const pass = encoder.beginRenderPass({
      colorAttachments: [
        {
          view: this.screen.createView(),
          clearValue: [0, 0, 0, 1],
          loadOp: "clear",
          storeOp: "store",
        },
      ],
    });
    for (let i = 0; i < count; i += 1) {
      const f = draws.subarray(i * FIELDS, (i + 1) * FIELDS);
      const [id, w, h] = f;
      const [alpha, add, cx0, cy0, cx1, cy1] = f.subarray(15);
      const x0 = Math.floor(cx0);
      const y0 = Math.floor(cy0);
      const x1 = Math.ceil(cx1);
      const y1 = Math.ceil(cy1);
      if (alpha <= 0 || x1 <= x0 || y1 <= y0) continue;
      const src = this.texture(game, id, w, h);
      if (!src) continue;
      const at = (i * SLOT) / 4;
      data.set(f.subarray(3, 11), at);
      data.set(f.subarray(11, 15), at + 8);
      data.set([w, h, alpha, this.width, this.height], at + 12);
      pass.setPipeline(add ? this.added : this.normal);
      pass.setBindGroup(
        0,
        this.device.createBindGroup({
          layout: this.layout,
          entries: [
            { binding: 0, resource: { buffer: this.uniforms, size: 80 } },
            { binding: 1, resource: src.createView() },
            { binding: 2, resource: this.sampler },
          ],
        }),
        [i * SLOT],
      );
      pass.setScissorRect(x0, y0, x1 - x0, y1 - y0);
      pass.draw(6);
    }
    pass.end();
    this.device.queue.writeBuffer(this.uniforms, 0, data);
    this.device.queue.submit([encoder.finish()]);
    // Pictures not drawn for a while give their memory back.
    for (const [id, t] of this.textures) {
      if (this.frame - t.used > 600) {
        t.texture.destroy();
        this.textures.delete(id);
      }
    }
    return this.screen;
  }
}
