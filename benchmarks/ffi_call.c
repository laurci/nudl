// FFI call benchmark — C baseline (separate compilation unit, no inlining)
#include <stdio.h>
#include <stdint.h>
#include <inttypes.h>
#include "ffi_lib.h"

int main(void) {
    int64_t n = 50000000;
    int64_t sum = 0;
    for (int64_t i = 0; i < n; i++) {
        sum += ffi_compute(i);
    }
    printf("%" PRId64 "\n", sum);
    return 0;
}
