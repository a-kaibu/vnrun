// Functions the xsystem4 build imports from the page (patches/xsystem4).
addToLibrary({
  // A frame of RGBA8 pixels, top row first, for the page to show.
  vnrun_present: (pixels, pitch, width, height) => {
    Module.vnrunPresent(HEAPU8.subarray(pixels, pixels + pitch * height), pitch, width, height);
  },
});
