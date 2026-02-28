// Recursive Fibonacci — pure compute benchmark (no heap allocation)
#include <stdio.h>
#include <stdint.h>
#include <inttypes.h>

static int64_t fib(int64_t n) {
    if (n <= 1) return n;
    return fib(n - 1) + fib(n - 2);
}

int main(void) {
    int64_t result = fib(42);
    printf("%" PRId64 "\n", result);
    return 0;
}
