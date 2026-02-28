#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <fcntl.h>
#include <unistd.h>
#include <sys/stat.h>

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

/* ================================================================
 * Dynamic Array Runtime
 *
 * A dynamic array is an ARC object with 3 fields after the header:
 *   offset 16: data_ptr (int64_t — pointer to heap buffer of int64_t elements)
 *   offset 24: length   (int64_t)
 *   offset 32: capacity (int64_t)
 *
 * All elements are stored as 64-bit values (int64_t / double / pointer).
 * ================================================================ */

typedef struct {
    NudlArcHeader header;
    int64_t data_ptr;   /* cast to int64_t* for element access */
    int64_t length;
    int64_t capacity;
} NudlDynArray;

/* Allocate a new empty dynamic array. Returns ARC object pointer. */
void *__nudl_array_alloc(void) {
    NudlDynArray *arr = (NudlDynArray *)malloc(sizeof(NudlDynArray));
    if (!arr) {
        fprintf(stderr, "nudl: out of memory (array alloc)\n");
        abort();
    }
    arr->header.strong_count = 1;
    arr->header.weak_count = 0;
    arr->header.type_tag = 0;
    arr->header._padding = 0;
    /* Start with capacity 4 */
    int64_t initial_cap = 4;
    int64_t *buf = (int64_t *)calloc((size_t)initial_cap, sizeof(int64_t));
    if (!buf) {
        fprintf(stderr, "nudl: out of memory (array buffer)\n");
        abort();
    }
    arr->data_ptr = (int64_t)(uintptr_t)buf;
    arr->length = 0;
    arr->capacity = initial_cap;
    return (void *)arr;
}

/* Push a value onto the end of the array. Grows if needed. */
void __nudl_array_push(void *arr_ptr, int64_t value) {
    NudlDynArray *arr = (NudlDynArray *)arr_ptr;
    if (arr->length >= arr->capacity) {
        int64_t new_cap = arr->capacity * 2;
        if (new_cap < 4) new_cap = 4;
        int64_t *old_buf = (int64_t *)(uintptr_t)arr->data_ptr;
        int64_t *new_buf = (int64_t *)realloc(old_buf, (size_t)new_cap * sizeof(int64_t));
        if (!new_buf) {
            fprintf(stderr, "nudl: out of memory (array grow)\n");
            abort();
        }
        arr->data_ptr = (int64_t)(uintptr_t)new_buf;
        arr->capacity = new_cap;
    }
    int64_t *buf = (int64_t *)(uintptr_t)arr->data_ptr;
    buf[arr->length] = value;
    arr->length++;
}

/* Pop the last value from the array. Returns 0 if empty. */
int64_t __nudl_array_pop(void *arr_ptr) {
    NudlDynArray *arr = (NudlDynArray *)arr_ptr;
    if (arr->length <= 0) return 0;
    arr->length--;
    int64_t *buf = (int64_t *)(uintptr_t)arr->data_ptr;
    return buf[arr->length];
}

/* Get the length of the array. */
int64_t __nudl_array_len(void *arr_ptr) {
    NudlDynArray *arr = (NudlDynArray *)arr_ptr;
    return arr->length;
}

/* Get element at index. Panics on out of bounds. */
int64_t __nudl_array_get(void *arr_ptr, int64_t index) {
    NudlDynArray *arr = (NudlDynArray *)arr_ptr;
    if (index < 0 || index >= arr->length) {
        fprintf(stderr, "nudl: array index out of bounds: index %lld, length %lld\n",
                (long long)index, (long long)arr->length);
        abort();
    }
    int64_t *buf = (int64_t *)(uintptr_t)arr->data_ptr;
    return buf[index];
}

/* Set element at index. Panics on out of bounds. */
void __nudl_array_set(void *arr_ptr, int64_t index, int64_t value) {
    NudlDynArray *arr = (NudlDynArray *)arr_ptr;
    if (index < 0 || index >= arr->length) {
        fprintf(stderr, "nudl: array index out of bounds: index %lld, length %lld\n",
                (long long)index, (long long)arr->length);
        abort();
    }
    int64_t *buf = (int64_t *)(uintptr_t)arr->data_ptr;
    buf[index] = value;
}

/* Destroy a dynamic array: optionally release reference-typed elements,
 * then free the data buffer.
 * Called as part of a drop function when a DynArray's ARC refcount reaches 0.
 * elem_drop: drop function for reference-typed elements (NULL for value types). */
void __nudl_array_destroy(void *arr_ptr, void (*elem_drop)(void *)) {
    NudlDynArray *arr = (NudlDynArray *)arr_ptr;
    int64_t *buf = (int64_t *)(uintptr_t)arr->data_ptr;
    if (buf) {
        if (elem_drop) {
            for (int64_t i = 0; i < arr->length; i++) {
                void *elem = (void *)(uintptr_t)buf[i];
                if (elem) {
                    NudlArcHeader *hdr = (NudlArcHeader *)elem;
                    if (hdr->strong_count > 0) {
                        hdr->strong_count--;
                        if (hdr->strong_count == 0) {
                            __nudl_arc_release_slow(elem, elem_drop);
                        }
                    }
                }
            }
        }
        free(buf);
    }
}

/* ================================================================
 * Map Runtime
 *
 * A map is an ARC object with open-addressing linear-probe hash table.
 * Layout after header (16 bytes):
 *   offset 16: entries_ptr  (int64_t — pointer to entry buffer)
 *   offset 24: length       (int64_t — number of live entries)
 *   offset 32: capacity     (int64_t — total slots)
 *   offset 40: (reserved)
 *
 * Each entry is 24 bytes: { int64_t key, int64_t value, int64_t state }
 * state: 0 = empty, 1 = occupied, 2 = tombstone
 * ================================================================ */

typedef struct {
    int64_t key;
    int64_t value;
    int64_t state; /* 0=empty, 1=occupied, 2=tombstone */
} NudlMapEntry;

typedef struct {
    NudlArcHeader header;
    int64_t entries_ptr; /* NudlMapEntry* cast to int64_t */
    int64_t length;
    int64_t capacity;
    int64_t _reserved;
} NudlMap;

static uint64_t nudl_hash_i64(int64_t key) {
    uint64_t h = (uint64_t)key;
    h ^= h >> 33;
    h *= 0xff51afd7ed558ccdULL;
    h ^= h >> 33;
    h *= 0xc4ceb9fe1a85ec53ULL;
    h ^= h >> 33;
    return h;
}

static void nudl_map_grow(NudlMap *map);

/* Allocate a new empty map. */
void *__nudl_map_alloc(void) {
    NudlMap *map = (NudlMap *)malloc(sizeof(NudlMap));
    if (!map) {
        fprintf(stderr, "nudl: out of memory (map alloc)\n");
        abort();
    }
    map->header.strong_count = 1;
    map->header.weak_count = 0;
    map->header.type_tag = 0;
    map->header._padding = 0;
    int64_t initial_cap = 8;
    NudlMapEntry *entries = (NudlMapEntry *)calloc((size_t)initial_cap, sizeof(NudlMapEntry));
    if (!entries) {
        fprintf(stderr, "nudl: out of memory (map entries)\n");
        abort();
    }
    map->entries_ptr = (int64_t)(uintptr_t)entries;
    map->length = 0;
    map->capacity = initial_cap;
    map->_reserved = 0;
    return (void *)map;
}

/* Insert key-value pair. Overwrites if key already exists. */
void __nudl_map_insert(void *map_ptr, int64_t key, int64_t value) {
    NudlMap *map = (NudlMap *)map_ptr;
    /* Grow if load factor > 70% */
    if (map->length * 10 >= map->capacity * 7) {
        nudl_map_grow(map);
    }
    NudlMapEntry *entries = (NudlMapEntry *)(uintptr_t)map->entries_ptr;
    uint64_t h = nudl_hash_i64(key);
    int64_t cap = map->capacity;
    int64_t idx = (int64_t)(h % (uint64_t)cap);
    for (int64_t i = 0; i < cap; i++) {
        int64_t slot = (idx + i) % cap;
        if (entries[slot].state == 0 || entries[slot].state == 2) {
            /* empty or tombstone — insert here */
            entries[slot].key = key;
            entries[slot].value = value;
            entries[slot].state = 1;
            map->length++;
            return;
        }
        if (entries[slot].state == 1 && entries[slot].key == key) {
            /* key already exists — update value */
            entries[slot].value = value;
            return;
        }
    }
    /* Should never reach here if load factor is respected */
    fprintf(stderr, "nudl: map insert failed (table full)\n");
    abort();
}

/* Get value for key. Returns value and sets *found to 1 if found, 0 otherwise. */
int64_t __nudl_map_get(void *map_ptr, int64_t key, int64_t *found) {
    NudlMap *map = (NudlMap *)map_ptr;
    NudlMapEntry *entries = (NudlMapEntry *)(uintptr_t)map->entries_ptr;
    uint64_t h = nudl_hash_i64(key);
    int64_t cap = map->capacity;
    int64_t idx = (int64_t)(h % (uint64_t)cap);
    for (int64_t i = 0; i < cap; i++) {
        int64_t slot = (idx + i) % cap;
        if (entries[slot].state == 0) {
            *found = 0;
            return 0;
        }
        if (entries[slot].state == 1 && entries[slot].key == key) {
            *found = 1;
            return entries[slot].value;
        }
    }
    *found = 0;
    return 0;
}

/* Check if map contains key. Returns 1 if yes, 0 if no. */
int64_t __nudl_map_contains(void *map_ptr, int64_t key) {
    NudlMap *map = (NudlMap *)map_ptr;
    NudlMapEntry *entries = (NudlMapEntry *)(uintptr_t)map->entries_ptr;
    uint64_t h = nudl_hash_i64(key);
    int64_t cap = map->capacity;
    int64_t idx = (int64_t)(h % (uint64_t)cap);
    for (int64_t i = 0; i < cap; i++) {
        int64_t slot = (idx + i) % cap;
        if (entries[slot].state == 0) return 0;
        if (entries[slot].state == 1 && entries[slot].key == key) return 1;
    }
    return 0;
}

/* Get number of entries. */
int64_t __nudl_map_len(void *map_ptr) {
    NudlMap *map = (NudlMap *)map_ptr;
    return map->length;
}

/* Remove key. Returns 1 if removed, 0 if not found. */
int64_t __nudl_map_remove(void *map_ptr, int64_t key) {
    NudlMap *map = (NudlMap *)map_ptr;
    NudlMapEntry *entries = (NudlMapEntry *)(uintptr_t)map->entries_ptr;
    uint64_t h = nudl_hash_i64(key);
    int64_t cap = map->capacity;
    int64_t idx = (int64_t)(h % (uint64_t)cap);
    for (int64_t i = 0; i < cap; i++) {
        int64_t slot = (idx + i) % cap;
        if (entries[slot].state == 0) return 0;
        if (entries[slot].state == 1 && entries[slot].key == key) {
            entries[slot].state = 2; /* tombstone */
            map->length--;
            return 1;
        }
    }
    return 0;
}

static void nudl_map_grow(NudlMap *map) {
    NudlMapEntry *old_entries = (NudlMapEntry *)(uintptr_t)map->entries_ptr;
    int64_t old_cap = map->capacity;
    int64_t new_cap = old_cap * 2;
    NudlMapEntry *new_entries = (NudlMapEntry *)calloc((size_t)new_cap, sizeof(NudlMapEntry));
    if (!new_entries) {
        fprintf(stderr, "nudl: out of memory (map grow)\n");
        abort();
    }
    map->entries_ptr = (int64_t)(uintptr_t)new_entries;
    map->capacity = new_cap;
    map->length = 0;
    /* Re-insert all occupied entries */
    for (int64_t i = 0; i < old_cap; i++) {
        if (old_entries[i].state == 1) {
            /* Use simplified insert that doesn't grow */
            uint64_t h = nudl_hash_i64(old_entries[i].key);
            int64_t idx = (int64_t)(h % (uint64_t)new_cap);
            for (int64_t j = 0; j < new_cap; j++) {
                int64_t slot = (idx + j) % new_cap;
                if (new_entries[slot].state == 0) {
                    new_entries[slot].key = old_entries[i].key;
                    new_entries[slot].value = old_entries[i].value;
                    new_entries[slot].state = 1;
                    map->length++;
                    break;
                }
            }
        }
    }
    free(old_entries);
}

/* ================================================================
 * Closure Runtime
 *
 * A closure is a 2-word fat value stored in a register pair:
 *   word 0: function pointer (as int64_t)
 *   word 1: environment pointer (as int64_t, points to ARC capture struct)
 *
 * The capture struct is an ARC object:
 *   [16-byte header] [captured_var_0: 8 bytes] [captured_var_1: 8 bytes] ...
 *
 * Closure thunk functions have signature:
 *   int64_t thunk(int64_t env_ptr, int64_t arg0, int64_t arg1, ...)
 * ================================================================ */

/* ================================================================
 * String Builtins
 *
 * A "heap string" is an ARC object with layout:
 *   [16-byte ARC header][int64_t length][char data[length]]
 *
 * Offset 16: length (i64)
 * Offset 24: data start (char*)
 * ================================================================ */

/* Concatenate two (ptr, len) string slices into a new heap string. */
void *__nudl_str_concat(const char *a_ptr, int64_t a_len,
                        const char *b_ptr, int64_t b_len) {
    int64_t total_len = a_len + b_len;
    uint64_t alloc_size = 24 + (uint64_t)total_len + 1;
    void *mem = malloc((size_t)alloc_size);
    if (!mem) {
        fprintf(stderr, "nudl: out of memory (str_concat)\n");
        abort();
    }
    NudlArcHeader *hdr = (NudlArcHeader *)mem;
    hdr->strong_count = 1;
    hdr->weak_count = 0;
    hdr->type_tag = 0;
    hdr->_padding = 0;
    int64_t *len_field = (int64_t *)((char *)mem + 16);
    *len_field = total_len;
    char *data = (char *)mem + 24;
    if (a_ptr && a_len > 0) memcpy(data, a_ptr, (size_t)a_len);
    if (b_ptr && b_len > 0) memcpy(data + a_len, b_ptr, (size_t)b_len);
    data[total_len] = '\0';
    return mem;
}

/* Convert i64 to a new heap string. */
void *__nudl_i64_to_str(int64_t val) {
    char buf[32];
    int len = snprintf(buf, sizeof(buf), "%lld", (long long)val);
    uint64_t alloc_size = 24 + (uint64_t)len + 1;
    void *mem = malloc((size_t)alloc_size);
    if (!mem) {
        fprintf(stderr, "nudl: out of memory (i64_to_str)\n");
        abort();
    }
    NudlArcHeader *hdr = (NudlArcHeader *)mem;
    hdr->strong_count = 1;
    hdr->weak_count = 0;
    hdr->type_tag = 0;
    hdr->_padding = 0;
    *(int64_t *)((char *)mem + 16) = (int64_t)len;
    memcpy((char *)mem + 24, buf, (size_t)len);
    *((char *)mem + 24 + len) = '\0';
    return mem;
}

/* Convert f64 to a new heap string. */
void *__nudl_f64_to_str(double val) {
    char buf[64];
    int len = snprintf(buf, sizeof(buf), "%g", val);
    uint64_t alloc_size = 24 + (uint64_t)len + 1;
    void *mem = malloc((size_t)alloc_size);
    if (!mem) {
        fprintf(stderr, "nudl: out of memory (f64_to_str)\n");
        abort();
    }
    NudlArcHeader *hdr = (NudlArcHeader *)mem;
    hdr->strong_count = 1;
    hdr->weak_count = 0;
    hdr->type_tag = 0;
    hdr->_padding = 0;
    *(int64_t *)((char *)mem + 16) = (int64_t)len;
    memcpy((char *)mem + 24, buf, (size_t)len);
    *((char *)mem + 24 + len) = '\0';
    return mem;
}

/* Convert bool (0 or non-zero) to "true" or "false" heap string. */
void *__nudl_bool_to_str(int64_t val) {
    const char *s = val ? "true" : "false";
    int64_t len = val ? 4 : 5;
    uint64_t alloc_size = 24 + (uint64_t)len + 1;
    void *mem = malloc((size_t)alloc_size);
    if (!mem) {
        fprintf(stderr, "nudl: out of memory (bool_to_str)\n");
        abort();
    }
    NudlArcHeader *hdr = (NudlArcHeader *)mem;
    hdr->strong_count = 1;
    hdr->weak_count = 0;
    hdr->type_tag = 0;
    hdr->_padding = 0;
    *(int64_t *)((char *)mem + 16) = len;
    memcpy((char *)mem + 24, s, (size_t)len);
    *((char *)mem + 24 + len) = '\0';
    return mem;
}

/* Convert a char (as i64 code point) to a single-character heap string. */
void *__nudl_char_to_str(int64_t val) {
    char c = (char)val;
    uint64_t alloc_size = 24 + 1 + 1;
    void *mem = malloc((size_t)alloc_size);
    if (!mem) {
        fprintf(stderr, "nudl: out of memory (char_to_str)\n");
        abort();
    }
    NudlArcHeader *hdr = (NudlArcHeader *)mem;
    hdr->strong_count = 1;
    hdr->weak_count = 0;
    hdr->type_tag = 0;
    hdr->_padding = 0;
    *(int64_t *)((char *)mem + 16) = 1;
    *((char *)mem + 24) = c;
    *((char *)mem + 25) = '\0';
    return mem;
}

/* Get byte at index from a string (ptr, len pair). Returns as i64. Panics on OOB. */
int64_t __nudl_str_char_at(const char *ptr, int64_t len, int64_t index) {
    if (index < 0 || index >= len) {
        fprintf(stderr, "nudl: string index out of bounds: index %lld, length %lld\n",
                (long long)index, (long long)len);
        abort();
    }
    return (int64_t)(unsigned char)ptr[index];
}

/* Allocate a new heap string from (ptr, len). Used by string operations and
 * by the compiler when pushing strings to dynamic arrays. */
void *__nudl_str_alloc(const char *data, int64_t len) {
    uint64_t alloc_size = 24 + (uint64_t)len + 1;
    void *mem = malloc((size_t)alloc_size);
    if (!mem) {
        fprintf(stderr, "nudl: out of memory (str_alloc)\n");
        abort();
    }
    NudlArcHeader *hdr = (NudlArcHeader *)mem;
    hdr->strong_count = 1;
    hdr->weak_count = 0;
    hdr->type_tag = 0;
    hdr->_padding = 0;
    *((int64_t *)((char *)mem + 16)) = len;
    if (len > 0) {
        memcpy((char *)mem + 24, data, (size_t)len);
    }
    *((char *)mem + 24 + len) = '\0';
    return mem;
}

/* Substring extraction: returns heap string for s[start..end]. */
void *__nudl_str_substr(const char *ptr, int64_t len, int64_t start, int64_t end) {
    if (start < 0) start = 0;
    if (end > len) end = len;
    if (start >= end) return __nudl_str_alloc("", 0);
    int64_t new_len = end - start;
    return __nudl_str_alloc(ptr + start, new_len);
}

/* Find index of needle in haystack. Returns -1 if not found. */
int64_t __nudl_str_indexof(const char *h_ptr, int64_t h_len,
                           const char *n_ptr, int64_t n_len) {
    if (n_len == 0) return 0;
    if (n_len > h_len) return -1;
    for (int64_t i = 0; i <= h_len - n_len; i++) {
        if (memcmp(h_ptr + i, n_ptr, (size_t)n_len) == 0) {
            return i;
        }
    }
    return -1;
}

/* Trim leading and trailing ASCII whitespace. Returns heap string. */
void *__nudl_str_trim(const char *ptr, int64_t len) {
    int64_t start = 0;
    int64_t end = len;
    while (start < end && (ptr[start] == ' ' || ptr[start] == '\t' ||
           ptr[start] == '\n' || ptr[start] == '\r')) {
        start++;
    }
    while (end > start && (ptr[end - 1] == ' ' || ptr[end - 1] == '\t' ||
           ptr[end - 1] == '\n' || ptr[end - 1] == '\r')) {
        end--;
    }
    return __nudl_str_alloc(ptr + start, end - start);
}

/* Check if haystack contains needle. Returns 1 or 0. */
int64_t __nudl_str_contains(const char *h_ptr, int64_t h_len,
                            const char *n_ptr, int64_t n_len) {
    return __nudl_str_indexof(h_ptr, h_len, n_ptr, n_len) >= 0 ? 1 : 0;
}

/* Check if string starts with prefix. Returns 1 or 0. */
int64_t __nudl_str_starts_with(const char *ptr, int64_t len,
                               const char *p_ptr, int64_t p_len) {
    if (p_len > len) return 0;
    return memcmp(ptr, p_ptr, (size_t)p_len) == 0 ? 1 : 0;
}

/* Check if string ends with suffix. Returns 1 or 0. */
int64_t __nudl_str_ends_with(const char *ptr, int64_t len,
                             const char *s_ptr, int64_t s_len) {
    if (s_len > len) return 0;
    return memcmp(ptr + len - s_len, s_ptr, (size_t)s_len) == 0 ? 1 : 0;
}

/* Convert ASCII string to uppercase. Returns heap string. */
void *__nudl_str_to_upper(const char *ptr, int64_t len) {
    void *result = __nudl_str_alloc(ptr, len);
    char *data = (char *)result + 24;
    for (int64_t i = 0; i < len; i++) {
        if (data[i] >= 'a' && data[i] <= 'z') {
            data[i] -= 32;
        }
    }
    return result;
}

/* Convert ASCII string to lowercase. Returns heap string. */
void *__nudl_str_to_lower(const char *ptr, int64_t len) {
    void *result = __nudl_str_alloc(ptr, len);
    char *data = (char *)result + 24;
    for (int64_t i = 0; i < len; i++) {
        if (data[i] >= 'A' && data[i] <= 'Z') {
            data[i] += 32;
        }
    }
    return result;
}

/* Replace all occurrences of old with new in string. Returns heap string. */
void *__nudl_str_replace(const char *ptr, int64_t len,
                         const char *old_ptr, int64_t old_len,
                         const char *new_ptr, int64_t new_len) {
    if (old_len == 0) return __nudl_str_alloc(ptr, len);

    /* First pass: count occurrences */
    int64_t count = 0;
    for (int64_t i = 0; i <= len - old_len; i++) {
        if (memcmp(ptr + i, old_ptr, (size_t)old_len) == 0) {
            count++;
            i += old_len - 1;
        }
    }
    if (count == 0) return __nudl_str_alloc(ptr, len);

    /* Second pass: build result */
    int64_t result_len = len + count * (new_len - old_len);
    uint64_t alloc_size = 24 + (uint64_t)result_len + 1;
    void *mem = malloc((size_t)alloc_size);
    if (!mem) { fprintf(stderr, "nudl: out of memory (str_replace)\n"); abort(); }
    NudlArcHeader *hdr = (NudlArcHeader *)mem;
    hdr->strong_count = 1; hdr->weak_count = 0; hdr->type_tag = 0; hdr->_padding = 0;
    *((int64_t *)((char *)mem + 16)) = result_len;
    char *dst = (char *)mem + 24;
    int64_t di = 0;
    for (int64_t i = 0; i < len; ) {
        if (i <= len - old_len && memcmp(ptr + i, old_ptr, (size_t)old_len) == 0) {
            memcpy(dst + di, new_ptr, (size_t)new_len);
            di += new_len;
            i += old_len;
        } else {
            dst[di++] = ptr[i++];
        }
    }
    dst[result_len] = '\0';
    return mem;
}

/* Repeat string count times. Returns heap string. */
void *__nudl_str_repeat(const char *ptr, int64_t len, int64_t count) {
    if (count <= 0 || len == 0) return __nudl_str_alloc("", 0);
    int64_t result_len = len * count;
    void *mem = __nudl_str_alloc(ptr, result_len);
    char *data = (char *)mem + 24;
    /* First copy is already done by __nudl_str_alloc, copy the rest */
    for (int64_t i = 1; i < count; i++) {
        memcpy(data + i * len, ptr, (size_t)len);
    }
    return mem;
}

/* ================================================================
 * Closure Runtime
 *
 * A closure is a 2-word fat value stored in a register pair:
 *   word 0: function pointer (as int64_t)
 *   word 1: environment pointer (as int64_t, points to ARC capture struct)
 *
 * The capture struct is an ARC object:
 *   [16-byte header] [captured_var_0: 8 bytes] [captured_var_1: 8 bytes] ...
 *
 * Closure thunk functions have signature:
 *   int64_t thunk(int64_t env_ptr, int64_t arg0, int64_t arg1, ...)
 * ================================================================ */

/* ================================================================
 * File I/O Runtime Helpers
 *
 * These provide portable file operations that can't be done via
 * pure libc FFI (platform-specific O_* flags, buffer allocation).
 * ================================================================ */

/* Portable file open.
 * mode: 0=read, 1=write(create/truncate), 2=append(create).
 * String params are (ptr, len) pairs. Returns fd or -1. */
int64_t __nudl_file_open(const char *path_ptr, int64_t path_len, int64_t mode) {
    /* Null-terminate the path (it may already be, but be safe) */
    char *path = (char *)malloc((size_t)path_len + 1);
    if (!path) return -1;
    memcpy(path, path_ptr, (size_t)path_len);
    path[path_len] = '\0';

    int fd;
    if (mode == 0) {
        fd = open(path, O_RDONLY);
    } else if (mode == 1) {
        fd = open(path, O_WRONLY | O_CREAT | O_TRUNC, 0644);
    } else {
        fd = open(path, O_WRONLY | O_CREAT | O_APPEND, 0644);
    }
    free(path);
    return (int64_t)fd;
}

/* Read up to count bytes from fd. Returns ARC heap string (empty on EOF/error). */
void *__nudl_file_read(int64_t fd, int64_t count) {
    if (count <= 0) return __nudl_str_alloc("", 0);
    char *buf = (char *)malloc((size_t)count);
    if (!buf) return __nudl_str_alloc("", 0);
    ssize_t n = read((int)fd, buf, (size_t)count);
    if (n <= 0) {
        free(buf);
        return __nudl_str_alloc("", 0);
    }
    void *result = __nudl_str_alloc(buf, (int64_t)n);
    free(buf);
    return result;
}

/* Read entire file by path. Returns ARC heap string (empty on error). */
void *__nudl_file_read_all(const char *path_ptr, int64_t path_len) {
    char *path = (char *)malloc((size_t)path_len + 1);
    if (!path) return __nudl_str_alloc("", 0);
    memcpy(path, path_ptr, (size_t)path_len);
    path[path_len] = '\0';

    int fd = open(path, O_RDONLY);
    free(path);
    if (fd < 0) return __nudl_str_alloc("", 0);

    struct stat st;
    if (fstat(fd, &st) < 0 || st.st_size <= 0) {
        close(fd);
        return __nudl_str_alloc("", 0);
    }

    char *buf = (char *)malloc((size_t)st.st_size);
    if (!buf) { close(fd); return __nudl_str_alloc("", 0); }

    ssize_t total = 0;
    while (total < st.st_size) {
        ssize_t n = read(fd, buf + total, (size_t)(st.st_size - total));
        if (n <= 0) break;
        total += n;
    }
    close(fd);

    void *result = __nudl_str_alloc(buf, (int64_t)total);
    free(buf);
    return result;
}

/* Allocate a capture environment with N captured values. */
void *__nudl_closure_env_alloc(int64_t num_captures) {
    uint64_t total_size = 16 + (uint64_t)num_captures * 8;
    void *mem = malloc((size_t)total_size);
    if (!mem) {
        fprintf(stderr, "nudl: out of memory (closure env)\n");
        abort();
    }
    memset(mem, 0, (size_t)total_size);
    NudlArcHeader *hdr = (NudlArcHeader *)mem;
    hdr->strong_count = 1;
    hdr->weak_count = 0;
    hdr->type_tag = 0;
    hdr->_padding = 0;
    return mem;
}
