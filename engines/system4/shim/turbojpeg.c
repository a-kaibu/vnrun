/* libturbojpeg's decompression calls (turbojpeg.h) over stb_image. */
#include <stdlib.h>
#include <string.h>
#include "turbojpeg.h"

#define STB_IMAGE_IMPLEMENTATION
#define STBI_ONLY_JPEG
#define STBI_NO_STDIO
#include "stb_image.h"

static char error[128] = "";

tjhandle tjInitDecompress(void)
{
	return (tjhandle)1;
}

int tjDestroy(tjhandle handle)
{
	(void)handle;
	return 0;
}

char *tjGetErrorStr(void)
{
	return error;
}

int tjDecompressHeader2(tjhandle handle, unsigned char *buf, unsigned long size, int *width,
			int *height, int *subsamp)
{
	(void)handle;
	int comp;
	if (!stbi_info_from_memory(buf, (int)size, width, height, &comp)) {
		strncpy(error, stbi_failure_reason(), sizeof(error) - 1);
		return -1;
	}
	*subsamp = 0;
	return 0;
}

int tjDecompress2(tjhandle handle, const unsigned char *buf, unsigned long size, unsigned char *dst,
		  int width, int pitch, int height, int format, int flags)
{
	(void)handle;
	(void)flags;
	int channels = format == TJPF_RGBA ? 4 : 3;
	int w, h, comp;
	unsigned char *pixels = stbi_load_from_memory(buf, (int)size, &w, &h, &comp, channels);
	if (!pixels) {
		strncpy(error, stbi_failure_reason(), sizeof(error) - 1);
		return -1;
	}
	if (pitch == 0)
		pitch = width * channels;
	int rows = h < height ? h : height;
	int cols = (w < width ? w : width) * channels;
	for (int y = 0; y < rows; y++)
		memcpy(dst + (size_t)y * pitch, pixels + (size_t)y * w * channels, cols);
	stbi_image_free(pixels);
	return 0;
}
