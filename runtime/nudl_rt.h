#ifndef NUDL_RT_H
#define NUDL_RT_H

#include <stdint.h>

/*
 * nudl ARC runtime — object header layout (16 bytes):
 *
 *   offset 0:  strong_count  (uint32_t)
 *   offset 4:  weak_count    (uint32_t)
 *   offset 8:  type_tag      (uint32_t)
 *   offset 12: padding       (uint32_t)
 *   offset 16: fields start
 */

typedef struct {
  uint32_t strong_count;
  uint32_t weak_count;
  uint32_t type_tag;
  uint32_t _padding;
} NudlArcHeader;

#define NUDL_ARC_HEADER_SIZE sizeof(NudlArcHeader)

/* Allocate a new ARC object. total_size includes the 16-byte header. */
void *__nudl_arc_alloc(uint64_t total_size, uint32_t type_tag);

/* Called when strong_count has already been decremented to 0.
 * Calls drop_fn (if non-null), then frees if weak_count is also 0. */
void __nudl_arc_release_slow(void *ptr, void (*drop_fn)(void *));

/* Abort on reference count overflow (> UINT32_MAX). */
_Noreturn void __nudl_arc_overflow_abort(void);

/* Weak reference operations. */
void __nudl_arc_weak_retain(void *ptr);
void __nudl_arc_weak_release(void *ptr);
void *__nudl_arc_weak_upgrade(void *ptr);

#endif /* NUDL_RT_H */
