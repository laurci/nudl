# Task 17: Dynamic Arrays (`T[]`)

## Goal
Implement dynamically-sized arrays with runtime push/pop/index operations, heap-allocated and ARC-managed.

## Requirements

### Type System
- Add `TypeKind::Array(TypeId)` — dynamic array of element type T
- Arrays are **reference types** (heap-allocated, ARC'd)
- Array type syntax: `T[]` or `[T]` (TBD — check spec preference)

### Runtime Representation
- Heap-allocated: `{ ref_count: i64, length: i64, capacity: i64, data: ptr }` (or similar)
- Data is a contiguous buffer of elements, reallocated on growth (like Vec)
- ARC'd: retain/release on the array object pointer

### C Runtime Functions (`runtime/nudl_rt.c`)
- `__nudl_array_alloc(element_size: i64) -> ptr` — allocate empty array
- `__nudl_array_push(arr: ptr, element: ptr, element_size: i64)` — append element
- `__nudl_array_pop(arr: ptr, element_size: i64) -> ptr` — remove and return last
- `__nudl_array_len(arr: ptr) -> i64` — get length
- `__nudl_array_get(arr: ptr, index: i64, element_size: i64) -> ptr` — get element by index (with bounds check)
- `__nudl_array_set(arr: ptr, index: i64, element: ptr, element_size: i64)` — set element by index

### Parsing
- Array literal: `[1, 2, 3]` (same syntax as fixed array — disambiguate by context/type annotation)
- Type annotation: `let a: i32[] = [1, 2, 3];`
- Index: `a[i]` (shared with fixed arrays)
- Method-like builtins: `a.push(x)`, `a.pop()`, `a.len()`

### IR Instructions
- `ArrayAlloc { dst, element_type }` — create empty array
- `ArrayPush { array, value }` — push element
- `ArrayPop { dst, array }` — pop element
- `ArrayLen { dst, array }` — get length
- Reuse `IndexLoad`/`IndexStore` from fixed arrays

### Type Checking
- `push(x)`: x must match element type
- `pop()`: returns element type (panics if empty)
- `a[i]`: i must be integer, returns element type
- Array literal type inference: element types must all agree

## Acceptance Criteria
- `tests/core-types/dynamic_arrays.nudl` compiles and runs
- Create: `let mut a: i32[] = [];`
- Push: `a.push(42);`
- Access: `let x = a[0];`
- Length: `let n = a.len();`
- Pop: `let x = a.pop();`
- Arrays of reference types properly retain/release elements
- Bounds checking: panic on out-of-range index
- For-loop iteration over dynamic arrays (extends Task 08)

## Technical Notes
- The runtime functions handle memory management (realloc on growth)
- Element size is needed at runtime for generic-over-size operations
- For reference-type elements, push should retain, pop should not release (caller takes ownership)
- Bounds checking in `__nudl_array_get`/`__nudl_array_set` — abort on out of range
- Disambiguation from fixed arrays: if no `;` in `[...]` and type context is `T[]`, it's dynamic
- Alternative: always infer `[1,2,3]` as fixed array, require explicit `vec![1,2,3]` or similar for dynamic
- The push/pop/len methods can be compiler builtins (special-cased in type checker) until general methods exist
