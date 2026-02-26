# Task 06: Tuple Types

## Goal
Implement tuple types, literals, and element access so users can write `let t: (i32, String) = (42, "hello"); let x = t.0;`.

## Requirements

### Type System
- Add `TypeKind::Tuple(Vec<TypeId>)` to represent tuple types
- Tuples are **value types** (stack-allocated, copied on assignment) when all elements are value types
- Tuples containing reference types follow ARC semantics for those elements
- Empty tuple `()` is the unit type (already exists)
- Single-element tuples: `(T,)` with trailing comma to disambiguate from parenthesized expressions

### Parsing
- Tuple literal: `(expr1, expr2, ...)` — distinguish from parenthesized expr by comma presence
- Tuple type: `(Type1, Type2, ...)` in type annotations
- Element access: `tuple.0`, `tuple.1`, etc. — numeric field access (already partially handled by field access parsing)

### Type Checking
- Infer tuple type from literal elements
- Validate index bounds for `.N` access (compile-time check since indices are literals)
- Ensure element access returns the correct element type

### IR & Codegen
- Add `Instruction::TupleAlloc { dst, elements: Vec<Reg> }` or reuse struct-like alloc
- Element access: compile to GEP (getelementptr) into a struct type in LLVM
- LLVM representation: anonymous struct type `{ i32, ptr }` etc.
- Value semantics: tuples are passed by value (copied), no ARC on the tuple itself

## Acceptance Criteria
- `tests/core-types/tuples.nudl` compiles and runs
- Tuple creation: `let t = (1, 2, 3);`
- Element access: `t.0`, `t.1`, `t.2`
- Tuple type annotations: `let t: (i32, bool) = (42, true);`
- Tuples as function parameters and return values
- Nested tuples: `let t = ((1, 2), (3, 4)); let x = t.0.1;`

## Technical Notes
- LLVM struct types are the natural representation for tuples
- For value semantics, tuples should be `alloca`'d on the stack and memcpy'd on assignment
- Small tuples (≤2 elements, all primitives) could be passed in registers
- The `.0`, `.1` syntax is already partially handled by the field access parser (numeric identifiers)
- Reference-type elements inside tuples need retain/release when the tuple is copied/dropped
