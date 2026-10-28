/* ffi.h: calls go through the wrappers gen_hll.py writes (hll_wraps.c lists their tables). */
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "ffi.h"

ffi_type ffi_type_void = { 0 }, ffi_type_sint32 = { 1 }, ffi_type_sint64 = { 2 },
	ffi_type_float = { 3 }, ffi_type_pointer = { 4 };

extern const struct vnrun_hll_wrap *const vnrun_hll_wraps[];

ffi_status ffi_prep_cif(ffi_cif *cif, ffi_abi abi, unsigned nargs, ffi_type *rtype, ffi_type **atypes)
{
	(void)abi;
	cif->nargs = nargs;
	cif->rtype = rtype;
	cif->arg_types = atypes;
	return FFI_OK;
}

/* Open addressing from function address to wrapper. */
static struct vnrun_hll_wrap *map;
static size_t map_size;

static size_t slot(void *fun)
{
	return ((size_t)fun * 2654435761u) & (map_size - 1);
}

static void build_map(void)
{
	size_t n = 0;
	for (int t = 0; vnrun_hll_wraps[t]; t++)
		for (int i = 0; vnrun_hll_wraps[t][i].fun; i++)
			n++;
	for (map_size = 64; map_size < n * 2; map_size *= 2)
		;
	map = calloc(map_size, sizeof(*map));
	for (int t = 0; vnrun_hll_wraps[t]; t++) {
		for (int i = 0; vnrun_hll_wraps[t][i].fun; i++) {
			size_t s = slot(vnrun_hll_wraps[t][i].fun);
			while (map[s].fun && map[s].fun != vnrun_hll_wraps[t][i].fun)
				s = (s + 1) & (map_size - 1);
			map[s] = vnrun_hll_wraps[t][i];
		}
	}
}

static struct vnrun_hll_wrap *find(void *fn)
{
	if (!map)
		build_map();
	size_t s = slot(fn);
	while (map[s].fun && map[s].fun != fn)
		s = (s + 1) & (map_size - 1);
	return &map[s];
}

int vnrun_hll_has_wrapper(void *fun)
{
	return find(fun)->fun != NULL;
}

void ffi_call(ffi_cif *cif, void *fn, void *rvalue, void **avalue)
{
	struct vnrun_hll_wrap *w = find(fn);
	if (!w->fun) {
		fprintf(stderr, "ffi_call: no wrapper for HLL function %p\n", fn);
		abort();
	}
	/* The VM's return slot (union vm_value) is pointer-sized: a narrow result (bool) leaves the
	 * rest zero and a 64-bit one is cut to it, as on other 32-bit targets. */
	uint64_t r = 0;
	w->wrapper(&r, avalue, cif->nargs);
	memcpy(rvalue, &r, sizeof(void *));
}
