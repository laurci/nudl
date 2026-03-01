// Shared C library function called from all FFI benchmarks.
// Compiled as a separate object to prevent cross-module inlining.
#include "ffi_lib.h"

int64_t ffi_compute(int64_t x) {
    return x * x + x;
}
