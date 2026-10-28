/* The part of libturbojpeg that libsys4 uses (jpeg.c, ajp.c), decoded with stb_image. */
#ifndef VNRUN_TURBOJPEG_H
#define VNRUN_TURBOJPEG_H

typedef void *tjhandle;
enum { TJPF_RGB = 0, TJPF_RGBA = 7 };
#define TJFLAG_FASTDCT 2048

tjhandle tjInitDecompress(void);
int tjDestroy(tjhandle handle);
char *tjGetErrorStr(void);
int tjDecompressHeader2(tjhandle handle, unsigned char *jpegBuf, unsigned long jpegSize,
			int *width, int *height, int *jpegSubsamp);
int tjDecompress2(tjhandle handle, const unsigned char *jpegBuf, unsigned long jpegSize,
		  unsigned char *dstBuf, int width, int pitch, int height, int pixelFormat, int flags);

#endif
