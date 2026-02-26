# Task 07: Fixed-Size Arrays (`[T; N]`)

## Goal
Implement fixed-size arrays with compile-time-known length: `let arr: [i32; 3] = [1, 2, 3];` with index access and value semantics.

## Requirements

### Type System
- Add `TypeKind::FixedArray(TypeId, usize)` — element type + compile-time length
- Fixed arrays are **value types** (stack-allocated, copied on assignment)
- Element type can be any type (primitives, reference types, other arrays)

### Parsing
- Array type syntax: `[T; N]` where N is a literal integer
- Array literal: `[expr1, expr2, ...]` — infer length from element count
- Repeat syntax: `[expr; N]` — create array of N copies of expr
- Index access: `arr[i]` — parse as postfix index operator

### Type Checking
- Validate all elements in literal have the same type
- Validate index expressions are integer types
- Array length is part of the type: `[i32; 3]` ≠ `[i32; 4]`
- Bounds checking: warn on constant out-of-bounds access (runtime check otherwise)

### IR & Codegen
- Array literal → sequence of store instructions into stack alloca
- Index access → GEP + load (with optional bounds check)
- Index store → GEP + store
- LLVM representation: `[N x T]` array type
- Value semantics: memcpy on assignment/pass

### Instructions
- `Instruction::FixedArrayAlloc { dst, element_type, elements: Vec<Reg> }`
- `Instruction::IndexLoad { dst, array, index }` (shared with dynamic arrays later)
- `Instruction::IndexStore { array, index, value }`

## Acceptance Criteria
- `tests/core-types/fixed_arrays.nudl` compiles and runs
- Array literals: `let a = [1, 2, 3];`
- Index access: `a[0]`, `a[1]`
- Index assignment: `a[1] = 42;` (if array is mutable)
- Type annotations: `let a: [bool; 4] = [true, false, true, false];`
- Arrays as function params and return values
- Runtime bounds check on out-of-range index (panic or abort)

## Technical Notes
- LLVM `[N x T]` maps directly to fixed arrays
- Stack allocation: `alloca [N x T]`, access via `getelementptr`
- For bounds checking, emit `icmp uge index, N` → branch to abort
- Reference-type elements need retain on copy, release on overwrite
- Consider: should `len` be a method or field? Likely a compiler builtin since it's compile-time known
