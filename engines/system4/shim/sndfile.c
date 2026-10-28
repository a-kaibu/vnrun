/* sndfile.h over vorbisfile (Ogg Vorbis) and a small RIFF WAVE reader (integer and float PCM). */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <vorbis/vorbisfile.h>
#include "sndfile.h"

struct SNDFILE_tag {
	SF_VIRTUAL_IO vio;
	void *user;
	FILE *fp; /* sf_open: the file behind `vio` */
	int channels;
	sf_count_t frames, frame;
	/* Vorbis */
	int ogg;
	OggVorbis_File vf;
	/* WAVE */
	int bits, is_float, block;
	sf_count_t data_at;
};

static int open_error = SF_ERR_NO_ERROR;

static size_t ov_read_cb(void *ptr, size_t size, size_t n, void *d)
{
	SNDFILE *f = d;
	if (!size)
		return 0;
	return (size_t)f->vio.read(ptr, (sf_count_t)(size * n), f->user) / size;
}

static int ov_seek_cb(void *d, ogg_int64_t off, int whence)
{
	SNDFILE *f = d;
	f->vio.seek(off, whence, f->user);
	return 0;
}

static long ov_tell_cb(void *d)
{
	SNDFILE *f = d;
	return (long)f->vio.tell(f->user);
}

static uint32_t le32(const uint8_t *p)
{
	return p[0] | p[1] << 8 | p[2] << 16 | (uint32_t)p[3] << 24;
}

static int open_wave(SNDFILE *f)
{
	uint8_t h[12];
	f->vio.seek(0, SEEK_SET, f->user);
	if (f->vio.read(h, 12, f->user) != 12 || memcmp(h, "RIFF", 4) || memcmp(h + 8, "WAVE", 4))
		return 0;
	int rate = 0, have_fmt = 0;
	sf_count_t len = f->vio.get_filelen(f->user);
	for (;;) {
		uint8_t c[8];
		if (f->vio.read(c, 8, f->user) != 8)
			return 0;
		uint32_t size = le32(c + 4);
		sf_count_t at = f->vio.tell(f->user);
		if (!memcmp(c, "fmt ", 4)) {
			uint8_t fmt[16];
			if (size < 16 || f->vio.read(fmt, 16, f->user) != 16)
				return 0;
			int tag = fmt[0] | fmt[1] << 8;
			f->channels = fmt[2] | fmt[3] << 8;
			rate = (int)le32(fmt + 4);
			f->block = fmt[12] | fmt[13] << 8;
			f->bits = fmt[14] | fmt[15] << 8;
			if (tag == 0xfffe && size >= 26) {
				uint8_t ext[10];
				if (f->vio.read(ext, 10, f->user) != 10)
					return 0;
				tag = ext[8] | ext[9] << 8;
			}
			f->is_float = tag == 3;
			if ((tag != 1 && tag != 3) || !f->channels || !f->block)
				return 0;
			have_fmt = 1;
		} else if (!memcmp(c, "data", 4)) {
			if (!have_fmt)
				return 0;
			f->data_at = at;
			if (at + size > len)
				size = (uint32_t)(len - at);
			f->frames = size / f->block;
			f->vio.seek(at, SEEK_SET, f->user);
			return rate;
		}
		f->vio.seek(at + size + (size & 1), SEEK_SET, f->user);
	}
}

SNDFILE *sf_open_virtual(SF_VIRTUAL_IO *vio, int mode, SF_INFO *info, void *user)
{
	(void)mode;
	SNDFILE *f = calloc(1, sizeof(SNDFILE));
	f->vio = *vio;
	f->user = user;
	uint8_t magic[4] = { 0 };
	vio->seek(0, SEEK_SET, user);
	vio->read(magic, 4, user);
	vio->seek(0, SEEK_SET, user);
	memset(info, 0, sizeof(*info));
	if (!memcmp(magic, "OggS", 4)) {
		ov_callbacks cb = { ov_read_cb, ov_seek_cb, NULL, ov_tell_cb };
		if (ov_open_callbacks(f, &f->vf, NULL, 0, cb) == 0) {
			vorbis_info *vi = ov_info(&f->vf, -1);
			f->ogg = 1;
			f->channels = vi->channels;
			f->frames = ov_pcm_total(&f->vf, -1);
			info->samplerate = (int)vi->rate;
		}
	} else {
		info->samplerate = open_wave(f);
	}
	if (!f->ogg && !info->samplerate) {
		free(f);
		open_error = SF_ERR_UNRECOGNISED_FORMAT;
		return NULL;
	}
	info->channels = f->channels;
	info->frames = f->frames;
	info->sections = 1;
	info->seekable = 1;
	open_error = SF_ERR_NO_ERROR;
	return f;
}

static sf_count_t file_len(void *d)
{
	FILE *fp = ((SNDFILE *)d)->fp;
	long at = ftell(fp);
	fseek(fp, 0, SEEK_END);
	long len = ftell(fp);
	fseek(fp, at, SEEK_SET);
	return len;
}
static sf_count_t file_seek(sf_count_t off, int whence, void *d)
{
	FILE *fp = ((SNDFILE *)d)->fp;
	fseek(fp, (long)off, whence);
	return ftell(fp);
}
static sf_count_t file_read(void *ptr, sf_count_t n, void *d)
{
	return (sf_count_t)fread(ptr, 1, (size_t)n, ((SNDFILE *)d)->fp);
}
static sf_count_t file_tell(void *d)
{
	return ftell(((SNDFILE *)d)->fp);
}

SNDFILE *sf_open(const char *path, int mode, SF_INFO *info)
{
	FILE *fp = fopen(path, "rb");
	if (!fp) {
		open_error = SF_ERR_UNRECOGNISED_FORMAT;
		return NULL;
	}
	/* The callbacks get the SNDFILE, which only exists once opened: open through a holder. */
	SNDFILE holder = { .fp = fp };
	SF_VIRTUAL_IO vio = { file_len, file_seek, file_read, NULL, file_tell };
	SNDFILE *f = sf_open_virtual(&vio, mode, info, &holder);
	if (!f) {
		fclose(fp);
		return NULL;
	}
	/* Point the callbacks at the file itself from now on. */
	f->fp = fp;
	f->user = f;
	if (f->ogg) {
		/* vorbisfile keeps `holder` as its datasource; reopen with the final one. */
		ov_clear(&f->vf);
		fseek(fp, 0, SEEK_SET);
		ov_callbacks cb = { ov_read_cb, ov_seek_cb, NULL, ov_tell_cb };
		if (ov_open_callbacks(f, &f->vf, NULL, 0, cb) != 0) {
			fclose(fp);
			free(f);
			open_error = SF_ERR_UNRECOGNISED_FORMAT;
			return NULL;
		}
	}
	return f;
}

sf_count_t sf_readf_float(SNDFILE *f, float *out, sf_count_t frames)
{
	sf_count_t done = 0;
	if (f->ogg) {
		while (done < frames) {
			float **pcm;
			int section;
			long n = ov_read_float(&f->vf, &pcm, (int)(frames - done > 4096 ? 4096 : frames - done), &section);
			if (n <= 0)
				break;
			for (long i = 0; i < n; i++)
				for (int c = 0; c < f->channels; c++)
					out[(done + i) * f->channels + c] = pcm[c][i];
			done += n;
		}
	} else {
		if (frames > f->frames - f->frame)
			frames = f->frames - f->frame;
		uint8_t buf[4096];
		int bytes = f->bits / 8;
		while (done < frames) {
			sf_count_t n = (sf_count_t)sizeof(buf) / f->block;
			if (n > frames - done)
				n = frames - done;
			sf_count_t got = f->vio.read(buf, n * f->block, f->user) / f->block;
			if (got <= 0)
				break;
			for (sf_count_t i = 0; i < got; i++) {
				for (int c = 0; c < f->channels; c++) {
					const uint8_t *p = buf + i * f->block + c * bytes;
					float v;
					if (f->is_float && bytes == 4) {
						memcpy(&v, p, 4);
					} else if (bytes == 1) {
						v = (p[0] - 128) / 128.f;
					} else if (bytes == 2) {
						v = (int16_t)(p[0] | p[1] << 8) / 32768.f;
					} else if (bytes == 3) {
						v = (int32_t)((uint32_t)p[0] << 8 | (uint32_t)p[1] << 16 | (uint32_t)p[2] << 24) / 2147483648.f;
					} else {
						v = (int32_t)le32(p) / 2147483648.f;
					}
					out[(done + i) * f->channels + c] = v;
				}
			}
			done += got;
		}
	}
	f->frame += done;
	return done;
}

sf_count_t sf_seek(SNDFILE *f, sf_count_t frames, int whence)
{
	if (whence == SEEK_CUR)
		frames += f->frame;
	else if (whence == SEEK_END)
		frames += f->frames;
	if (frames < 0 || frames > f->frames)
		return -1;
	if (f->ogg) {
		if (ov_pcm_seek(&f->vf, frames) != 0)
			return -1;
	} else {
		f->vio.seek(f->data_at + frames * f->block, SEEK_SET, f->user);
	}
	f->frame = frames;
	return frames;
}

int sf_close(SNDFILE *f)
{
	if (!f)
		return 0;
	if (f->ogg)
		ov_clear(&f->vf);
	if (f->fp)
		fclose(f->fp);
	free(f);
	return 0;
}

int sf_error(SNDFILE *f)
{
	return f ? SF_ERR_NO_ERROR : open_error;
}

const char *sf_strerror(SNDFILE *f)
{
	return sf_error(f) ? "unrecognised audio format" : "no error";
}
