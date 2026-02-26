# Task 23: VM Updates for New IR Instructions

## Goal
Update the register-based VM interpreter to handle all new IR instructions added by Tasks 01–22, ensuring comptime evaluation works with the extended instruction set.

## Requirements

### New Instructions to Handle
From various tasks, the VM needs to execute:

**Task 01 — Casts:**
- `Cast { dst, src, target_type }` — numeric conversions between all types

**Task 02 — Bitwise:**
- `BitAnd`, `BitOr`, `BitXor`, `BitNot` — integer bitwise operations

**Task 06 — Tuples:**
- `TupleAlloc`, tuple element access (if separate from struct field access)

**Task 07 — Fixed Arrays:**
- `FixedArrayAlloc`, `IndexLoad`, `IndexStore`

**Task 11 — Enums:**
- `EnumAlloc`, `EnumTag`, `EnumPayload`

**Task 13 — Dynamic Arrays:**
- `ArrayAlloc`, `ArrayPush`, `ArrayPop`, `ArrayLen`

**Task 14 — Maps:**
- `MapAlloc`, `MapInsert`, `MapGet`, `MapRemove`, `MapContainsKey`, `MapLen`

**Task 15 — Closures:**
- `ClosureAlloc`, `ClosureCall`, `CaptureLoad`

### VM Value Type Extensions
- Add variants to the VM's value type for: tuples, fixed arrays, enums, dynamic arrays, maps, closures
- Tuple value: `Vec<Value>`
- FixedArray value: `Vec<Value>` with fixed length
- Enum value: `{ tag: usize, payload: Vec<Value> }`
- DynamicArray value: `Vec<Value>` (growable)
- Map value: `HashMap<Value, Value>`
- Closure value: `{ function_id, captured: Vec<Value> }`

### Comptime Restrictions
- No I/O operations in comptime
- No raw pointer operations
- Array/map operations are fine (pure computation)
- Step limit still applies

### Error Handling
- Unknown/unimplemented instructions should produce clear error messages
- Type mismatches in the VM should panic with descriptive messages
- Bounds checking for array/tuple access in the VM

## Acceptance Criteria
- All existing comptime tests still pass
- Comptime can evaluate expressions involving:
  - Numeric casts
  - Bitwise operations
  - Tuple creation and access
  - Fixed array creation and index access
  - Enum creation and tag inspection
  - Dynamic array operations (push, pop, get, len)
  - Map operations (insert, get, len)
  - Closure creation and calls (no capture of runtime state)
- Step limit prevents infinite loops in comptime
- Clear errors for unsupported operations (I/O, FFI calls)

## Technical Notes
- The VM (`nudl-vm/src/lib.rs`) is register-based with a simple value type
- Currently handles: arithmetic, comparisons, control flow, function calls, struct alloc/field access
- Each new instruction category needs a match arm in the VM's execute loop
- VM values are Rust enums — extend with new variants
- For closures in comptime: the captured values are comptime values, function body is executed in VM
- Dynamic arrays and maps: use Rust's Vec and HashMap directly in the VM
- This task is incremental — can be partially done alongside each feature task
- Consider: should each feature task include its own VM updates? Could split this way instead
