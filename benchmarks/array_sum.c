// Dynamic array push + sum — measures collection/runtime overhead
#include <stdio.h>
#include <stdlib.h>
#include <stdint.h>
#include <inttypes.h>

int main(void) {
    int64_t n = 10000000;
    int64_t cap = 16;
    int64_t len = 0;
    int64_t *arr = malloc(cap * sizeof(int64_t));

    for (int64_t i = 0; i < n; i++) {
        if (len >= cap) {
            cap *= 2;
            arr = realloc(arr, cap * sizeof(int64_t));
        }
        arr[len++] = i;
    }

    int64_t sum = 0;
    for (int64_t i = 0; i < len; i++) {
        sum += arr[i];
    }

    printf("%" PRId64 "\n", sum);
    free(arr);
    return 0;
}
