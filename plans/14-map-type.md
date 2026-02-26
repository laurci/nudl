# Task 14: Map Type (`Map<K, V>`)

## Goal
Implement a hash map type with runtime operations, heap-allocated and ARC-managed.

## Requirements

### Type System
- Add `TypeKind::Map(TypeId, TypeId)` — key type K, value type V
- Maps are **reference types** (heap-allocated, ARC'd)
- Key types must be hashable: integers, bools, chars, strings (no struct/enum keys in MVP)
- Value types can be anything

### Runtime (`runtime/nudl_rt.c`)
- `__nudl_map_alloc() -> ptr` — allocate empty map
- `__nudl_map_insert(map: ptr, key: ptr, key_size: i64, value: ptr, value_size: i64)`
- `__nudl_map_get(map: ptr, key: ptr, key_size: i64) -> ptr` — returns null if not found
- `__nudl_map_remove(map: ptr, key: ptr, key_size: i64) -> bool`
- `__nudl_map_contains_key(map: ptr, key: ptr, key_size: i64) -> bool`
- `__nudl_map_len(map: ptr) -> i64`
- Simple hash map implementation (open addressing or chaining)

### Parsing
- Map literal: (decide syntax) e.g., `Map { "key": value, "key2": value2 }` or use builder pattern
- Type annotation: `Map<String, i32>`
- Method-like builtins: `m.insert(k, v)`, `m.get(k)`, `m.remove(k)`, `m.contains_key(k)`, `m.len()`

### IR Instructions
- `MapAlloc { dst }` — create empty map
- `MapInsert { map, key, value }`
- `MapGet { dst, map, key }` — returns Option<V> or panics on missing (TBD)
- `MapRemove { map, key }`
- `MapContainsKey { dst, map, key }`
- `MapLen { dst, map }`

### Type Checking
- Insert: key matches K, value matches V
- Get: returns V (panics if missing, or returns Option<V> if Option exists by then)
- Validate key type is hashable

## Acceptance Criteria
- `tests/core-types/maps.nudl` compiles and runs
- Create: `let mut m: Map<String, i32> = Map {};`
- Insert: `m.insert("age", 30);`
- Get: `let age = m.get("age");`
- Contains: `m.contains_key("name")` returns bool
- Remove: `m.remove("age");`
- Length: `m.len()` returns correct count
- ARC: maps are properly reference counted

## Technical Notes
- The C runtime needs a full hash map implementation — could use a simple open-addressing table
- Hash function: FNV-1a or similar for simplicity
- String keys need special handling (hash the contents, not the pointer)
- For MVP, `get` can panic on missing key (simpler than Option return)
- If Option type is available (Task 17), `get` should return `Option<V>`
- Map methods are compiler builtins (special-cased) until generic methods work
- Memory: each entry stores key+value inline or as pointers depending on size
