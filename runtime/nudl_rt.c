#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/*
 * nudl ARC runtime — slow paths for reference counting.
 *
 * Object header layout (16 bytes):
 *   offset 0:  strong_count  (uint32_t)
 *   offset 4:  weak_count    (uint32_t)
 *   offset 8:  type_tag      (uint32_t)
 *   offset 12: padding       (uint32_t)
 *   offset 16: fields start
 *
 * Fast paths (retain, release decrement) are emitted as inline LLVM IR
 * by the compiler. Only the slow paths live here.
 */

typedef struct {
    uint32_t strong_count;
    uint32_t weak_count;
    uint32_t type_tag;
    uint32_t _padding;
} NudlArcHeader;

/* Allocate a new ARC object. total_size includes the 16-byte header. */
void *__nudl_arc_alloc(uint64_t total_size, uint32_t type_tag) {
    void *mem = malloc((size_t)total_size);
    if (!mem) {
        fprintf(stderr, "nudl: out of memory (alloc %llu bytes)\n",
                (unsigned long long)total_size);
        abort();
    }
    memset(mem, 0, (size_t)total_size);
    NudlArcHeader *hdr = (NudlArcHeader *)mem;
    hdr->strong_count = 1;
    hdr->weak_count   = 0;
    hdr->type_tag     = type_tag;
    hdr->_padding     = 0;
    return mem;
}

/* Called when strong_count has already been decremented to 0.
 * Calls drop_fn (if non-null) to release fields, then frees if no weak refs. */
void __nudl_arc_release_slow(void *ptr, void (*drop_fn)(void *)) {
    if (!ptr) return;
    if (drop_fn) {
        drop_fn(ptr);
    }
    NudlArcHeader *hdr = (NudlArcHeader *)ptr;
    if (hdr->weak_count == 0) {
        free(ptr);
    }
}

/* Abort on reference count overflow. */
_Noreturn void __nudl_arc_overflow_abort(void) {
    fprintf(stderr, "nudl: ARC reference count overflow\n");
    abort();
}

/* Increment weak reference count. */
void __nudl_arc_weak_retain(void *ptr) {
    if (!ptr) return;
    NudlArcHeader *hdr = (NudlArcHeader *)ptr;
    if (hdr->weak_count == UINT32_MAX) {
        __nudl_arc_overflow_abort();
    }
    hdr->weak_count++;
}

/* Decrement weak reference count; free if both counts are zero. */
void __nudl_arc_weak_release(void *ptr) {
    if (!ptr) return;
    NudlArcHeader *hdr = (NudlArcHeader *)ptr;
    if (hdr->weak_count > 0) {
        hdr->weak_count--;
    }
    if (hdr->strong_count == 0 && hdr->weak_count == 0) {
        free(ptr);
    }
}

/* Try to upgrade a weak reference to a strong reference.
 * Returns ptr if strong_count > 0 (after incrementing), else NULL. */
void *__nudl_arc_weak_upgrade(void *ptr) {
    if (!ptr) return NULL;
    NudlArcHeader *hdr = (NudlArcHeader *)ptr;
    if (hdr->strong_count == 0) {
        return NULL;
    }
    if (hdr->strong_count == UINT32_MAX) {
        __nudl_arc_overflow_abort();
    }
    hdr->strong_count++;
    return ptr;
}
