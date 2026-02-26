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

/*
 * Type descriptor for runtime-driven ARC drop.
 *
 * kind 0 = struct/tuple: child_count explicit byte offsets follow in offsets[].
 * kind 1 = array: stride-based layout via array.start / array.stride.
 */
typedef struct {
    const char *type_name;   /* for debug printing (NULL if unnamed) */
    uint8_t  kind;           /* 0 = struct/tuple, 1 = array */
    uint16_t child_count;    /* number of ref-typed children */
    union {
        uint16_t offsets[1]; /* kind=0: byte offsets from object start (flexible) */
        struct {
            uint16_t start;  /* kind=1: first child byte offset (typically 16) */
            uint16_t stride; /* kind=1: bytes between children (typically 8) */
        } array;
    };
} NudlTypeDesc;

/* Compiler-generated globals — indexed by type_tag. */
extern const NudlTypeDesc *__nudl_type_table[];
extern uint32_t __nudl_type_table_len;

/* Allocate a new ARC object. total_size includes the 16-byte header. */
void *__nudl_arc_alloc(uint64_t total_size, uint32_t type_tag);

/* Called when strong_count has already been decremented to 0.
 * Calls drop_fn (if non-null), then frees if weak_count is also 0. */
void __nudl_arc_release_slow(void *ptr, void (*drop_fn)(void *));

/* Generic drop function — walks the type descriptor to release children. */
void __nudl_arc_drop(void *ptr);

/* Abort on reference count overflow (> UINT32_MAX). */
_Noreturn void __nudl_arc_overflow_abort(void);

/* Weak reference operations. */
void __nudl_arc_weak_retain(void *ptr);
void __nudl_arc_weak_release(void *ptr);
void *__nudl_arc_weak_upgrade(void *ptr);

#endif /* NUDL_RT_H */
