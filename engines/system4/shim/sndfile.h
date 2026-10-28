/* The part of libsndfile that xsystem4's audio_mixer.c uses: reading Ogg Vorbis (vorbisfile)
 * and PCM WAV as float frames. */
#ifndef VNRUN_SNDFILE_H
#define VNRUN_SNDFILE_H

#include <stdint.h>

typedef int64_t sf_count_t;
typedef struct SNDFILE_tag SNDFILE;

typedef struct {
	sf_count_t frames;
	int samplerate;
	int channels;
	int format;
	int sections;
	int seekable;
} SF_INFO;

typedef sf_count_t (*sf_vio_get_filelen)(void *user_data);
typedef sf_count_t (*sf_vio_seek)(sf_count_t offset, int whence, void *user_data);
typedef sf_count_t (*sf_vio_read)(void *ptr, sf_count_t count, void *user_data);
typedef sf_count_t (*sf_vio_write)(const void *ptr, sf_count_t count, void *user_data);
typedef sf_count_t (*sf_vio_tell)(void *user_data);

typedef struct {
	sf_vio_get_filelen get_filelen;
	sf_vio_seek seek;
	sf_vio_read read;
	sf_vio_write write;
	sf_vio_tell tell;
} SF_VIRTUAL_IO;

enum { SFM_READ = 0x10 };
enum { SF_ERR_NO_ERROR = 0, SF_ERR_UNRECOGNISED_FORMAT = 1 };

SNDFILE *sf_open(const char *path, int mode, SF_INFO *info);
SNDFILE *sf_open_virtual(SF_VIRTUAL_IO *vio, int mode, SF_INFO *info, void *user_data);
sf_count_t sf_readf_float(SNDFILE *file, float *out, sf_count_t frames);
sf_count_t sf_seek(SNDFILE *file, sf_count_t frames, int whence);
int sf_close(SNDFILE *file);
int sf_error(SNDFILE *file);
const char *sf_strerror(SNDFILE *file);

#endif
