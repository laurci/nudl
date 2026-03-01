// Struct point accumulation — measures ARC overhead vs stack allocation
#include <stdio.h>
#include <stdint.h>
#include <inttypes.h>

typedef struct {
    int64_t x;
    int64_t y;
} Point;

static Point point_add(Point a, Point b) {
    return (Point){a.x + b.x, a.y + b.y};
}

int main(void) {
    int64_t n = 10000000;
    Point p = {0, 0};
    for (int64_t i = 0; i < n; i++) {
        p = point_add(p, (Point){i, i * 2});
    }
    printf("%" PRId64 ", %" PRId64 "\n", p.x, p.y);
    return 0;
}
