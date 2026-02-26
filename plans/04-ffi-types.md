# Task 04: FFI Types — MutRawPtr and CStr

## Goal
Add `MutRawPtr` and `CStr` types with essential methods for safe C interop, complementing the existing `RawPtr` type.

## Requirements

### MutRawPtr
- Add `TypeKind::MutRawPtr` (mutable raw pointer, maps to `*mut c_void` / LLVM `ptr`)
- Literal: `null_mut()` builtin or cast from `RawPtr`
- Methods/builtins:
  - `.is_null() -> bool`
  - `.as_ptr() -> RawPtr` (immutable cast)
- Support `as MutRawPtr` cast from `RawPtr` and integer types
- Support `as RawPtr` cast from `MutRawPtr`
- LLVM: same as RawPtr — opaque pointer type

### CStr
- Add `TypeKind::CStr` (null-terminated C string, maps to `*const c_char` / LLVM `ptr`)
- Construction:
  - `"literal".as_cstr()` method on String — copies to null-terminated buffer
  - Or direct from C FFI return values
- Methods/builtins:
  - `.to_string() -> String` — copies into nudl managed String
  - `.is_null() -> bool`
  - `.len() -> i64` — calls `strlen`
- LLVM: opaque pointer, with strlen/memcpy calls for conversions

### Integration
- Extern functions returning `*const c_char` or `*mut c_void` should use these types
- Allow these types in function parameters for C calls

## Acceptance Criteria
- C FFI functions using pointer types work correctly
- `.is_null()` checks work on both types
- `String` ↔ `CStr` conversions work
- `RawPtr` ↔ `MutRawPtr` casts work
- Type errors when using pointer types incorrectly

## Technical Notes
- `RawPtr` already exists in TypeKind and codegen — follow the same pattern
- The methods can be implemented as compiler builtins (special-cased in the type checker and lowered to direct IR) rather than requiring the method/impl infrastructure
- CStr→String conversion needs runtime support: allocate nudl string, memcpy from CStr, track length
- String→CStr needs: allocate len+1 bytes, memcpy, null-terminate
- These conversions should use the C runtime functions (malloc/free or the nudl allocator)
