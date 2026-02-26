# Task 18: Map Type (`Map<K, V>`)

## Goal
Implement a hash map type with runtime operations, heap-allocated and ARC-managed. With generics (Tasks 13-14) and interfaces (Tasks 15-16) now available, the map type uses generic methods and can enforce key constraints via bounds.

## Requirements

### Type System
- Add `TypeKind::Map(TypeId, TypeId)` — key type K, value type V (built-in generic type, not monomorphized from user code)
- Maps are **reference types** (heap-allocated, ARC'd)
- Key types must be hashable: integers, bools, chars, strings (no struct/enum keys in MVP)
- With interfaces available (Task 15-16), key constraint can be expressed as `K: Eq` bound (and `Hash` if a Hash interface is defined)
- Value types can be anything

### Runtime (`runtime/nudl_rt.c`)
- `__nudl_map_alloc() -> ptr` — allocate empty map
- `__nudl_map_insert(map: ptr, key: ptr, key_size: i64, value: ptr, value_size: i64)`
- `__nudl_map_get(map: ptr, key: ptr, key_size: i64) -> ptr` — returns null if not found
- `__nudl_map_remove(map: ptr, key: ptr, key_size: i64) -> bool`
- `__nudl_map_contains_key(map: ptr, key: ptr, key_size: i64) -> bool`
- `__nudl_map_len(map: ptr) -> i64`
- Simple hash map implementation (open addressing or chaining)

### Methods (via `impl` blocks using generics)
- Methods are defined as `impl<K, V> Map<K, V> { ... }` (compiler-registered generic impl)
- `fn insert(mut self, key: K, value: V)` — insert or update
- `fn get(self, key: K) -> Option<V>` — returns `Option<V>` (Task 21 provides `Option<T>` via generic enums)
- `fn remove(mut self, key: K) -> bool` — remove entry, return whether it existed
- `fn contains_key(self, key: K) -> bool` — check existence
- `fn len(self) -> i64` — number of entries
- Optionally implement `Index<K, V>` interface (Task 16) for `m[key]` syntax

### Parsing
- Map literal: (decide syntax) e.g., `Map { "key": value, "key2": value2 }` or use builder pattern
- Type annotation: `Map<String, i32>`
- Method calls: `m.insert(k, v)`, `m.get(k)`, `m.remove(k)`, `m.contains_key(k)`, `m.len()`

### IR Instructions
- `MapAlloc { dst }` — create empty map
- `MapInsert { map, key, value }`
- `MapGet { dst, map, key }` — returns Option<V> or panics on missing (TBD)
- `MapRemove { map, key }`
- `MapContainsKey { dst, map, key }`
- `MapLen { dst, map }`

### Type Checking
- Insert: key matches K, value matches V
- Get: returns `Option<V>` (Task 21 provides Option via generic enums from Task 14)
- Validate key type is hashable (via interface bounds if available, or hardcoded allowlist for MVP)

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
- `get` returns `Option<V>` — Task 21 (Option/Result) depends on Task 14 (generic enums), and Phase 6 comes after Phase 5 (generics), so `Option<V>` is available
- Methods are registered as a compiler-provided generic impl block (`impl<K, V> Map<K, V> { ... }`), resolved through standard method resolution from Task 15
- Monomorphization (Task 13-14) ensures each `Map<K, V>` instantiation knows concrete key/value sizes at compile time — the runtime functions receive element sizes as parameters
- With `Index<K, V>` interface (Task 16), `m[key]` can desugar to `Index::index(m, key)` consistently
- Memory: each entry stores key+value inline or as pointers depending on size
- Depends on: Task 13-14 (generics for type params and method monomorphization), Task 21 (Option for `get` return type)
