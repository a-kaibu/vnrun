// JS functions the art3m1s-core web build imports (linked with --js-library).
addToLibrary({
  // Makes a WebGL2 context on Module.canvas current; art3m1s renders into its own framebuffer
  // and the page copies the pixels out.
  vnrun_webgl_create__deps: ["$GL"],
  vnrun_webgl_create: function () {
    const handle = GL.createContext(Module.canvas, {
      majorVersion: 2,
      minorVersion: 0,
      alpha: false,
      depth: true,
      stencil: true,
      antialias: false,
      premultipliedAlpha: false,
      preserveDrawingBuffer: false,
    });
    return handle && GL.makeContextCurrent(handle) ? 1 : 0;
  },
});
