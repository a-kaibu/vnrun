/* The part of libffi that xsystem4's src/ffi.c uses, for the browser build.
 *
 * In WebAssembly an indirect call must match the callee's exact type, so calls cannot be built
 * from the .ain's view of a function. Instead gen_hll.py writes, next to every HLL library, one
 * wrapper per exported function that unpacks libffi-style arguments (args[i] points at the
 * value) into a typed call; ffi_call finds the wrapper by the function's address. */
#ifndef VNRUN_FFI_H
#define VNRUN_FFI_H

#include <stddef.h>

typedef struct { int kind; } ffi_type;
extern ffi_type ffi_type_void, ffi_type_sint32, ffi_type_sint64, ffi_type_float, ffi_type_pointer;

typedef enum { FFI_OK = 0, FFI_BAD_TYPEDEF, FFI_BAD_ABI } ffi_status;
typedef enum { FFI_DEFAULT_ABI = 0 } ffi_abi;

typedef struct {
	unsigned nargs;
	ffi_type *rtype;
	ffi_type **arg_types;
} ffi_cif;

ffi_status ffi_prep_cif(ffi_cif *cif, ffi_abi abi, unsigned nargs, ffi_type *rtype, ffi_type **atypes);
void ffi_call(ffi_cif *cif, void *fn, void *rvalue, void **avalue);

/* What gen_hll.py emits: a wrapper calls `fun` with `nargs` arguments from `args`. */
typedef void (*vnrun_hll_wrapper)(void *ret, void **args, unsigned nargs);
struct vnrun_hll_wrap {
	void *fun;
	vnrun_hll_wrapper wrapper;
};

/* Whether ffi_call has a wrapper for `fun`. */
int vnrun_hll_has_wrapper(void *fun);

#endif
