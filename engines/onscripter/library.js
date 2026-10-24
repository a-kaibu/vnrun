// Functions the ONScripter build imports from the page (patches/onscripter-yuri).
addToLibrary({
  // A frame of ARGB8888 pixels (BGRA in memory) for the page to show.
  vnrun_present: (pixels, pitch, width, height) => {
    Module.vnrunPresent(HEAPU8.subarray(pixels, pixels + pitch * height), pitch, width, height);
  },
});
