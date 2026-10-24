// The engines were written against wgpu 0.19/0.20 and naga, which are looser than today's
// browsers. This smooths over the differences for every engine at once:
// - device requests leave out limits that WebGPU has since dropped;
// - WGSL may sample textures in non-uniform control flow (naga allows it, Chromium's WGSL
//   compiler rejects it unless the check is turned off).
const DROPPED_LIMITS = ["maxInterStageShaderComponents"];
const UNIFORMITY_OFF = "diagnostic(off, derivative_uniformity);\n";

const requestDevice = GPUAdapter.prototype.requestDevice;
GPUAdapter.prototype.requestDevice = function (descriptor?: GPUDeviceDescriptor) {
  const limits = descriptor?.requiredLimits as Record<string, unknown> | undefined;
  if (limits) for (const name of DROPPED_LIMITS) delete limits[name];
  return requestDevice.call(this, descriptor);
};

const createShaderModule = GPUDevice.prototype.createShaderModule;
GPUDevice.prototype.createShaderModule = function (descriptor: GPUShaderModuleDescriptor) {
  return createShaderModule.call(this, { ...descriptor, code: UNIFORMITY_OFF + descriptor.code });
};
