# Task 23: ARC Completion and `defer` Statement

## Goal
Complete ARC memory management (recursive field release on deallocation, sharing via retain on assignment, value type copy semantics) and implement the `defer` statement.

## Requirements

### ARC Deallocation (Recursive Release)
- When ref count drops to 0, the runtime must release all reference-type fields
- For structs: iterate fields, release each reference-type field before freeing
- For enums: based on tag, release payload reference-type fields
- For arrays: iterate elements, release each reference-type element
- For closures: release captured reference-type variables
- Generate per-type destructor functions that the release slow path calls

### ARC Sharing (Retain on Assignment)
- When assigning a reference-type value to a new binding: retain
- When passing to a function: caller retains (already implemented for structs)
- When storing in a field/array element: retain
- When overwriting a field/element: release old, retain new

### Value Type Copy Semantics
- Primitives, tuples, fixed arrays: copied on assignment, no ARC
- Structs are reference types: retain on copy
- Tuples/fixed arrays containing reference types: retain each ref-type element on copy

### `defer` Statement
- `defer expr;` or `defer { block }` — execute at scope exit
- Multiple defers in a scope execute in LIFO order (last defer runs first)
- Defers run on normal exit, return, and break/continue
- Implementation: lower to code at all scope exits

### IR
- `Instruction::Defer { block }` or lower defers to explicit scope-exit code
- Per-type destructor function generation in the lowerer or codegen

## Acceptance Criteria
- `tests/memory-management/arc_sharing.nudl` — retains work on assignment
- `tests/memory-management/arc_deallocation.nudl` — nested structs fully released
- `tests/memory-management/value_type_copy.nudl` — values copied, not shared
- `tests/memory-management/defer.nudl` — defer runs at scope exit in correct order
- No memory leaks for programs creating/destroying reference types
- No use-after-free for shared references

## Technical Notes
- Currently, the compiler emits Retain/Release for struct types (caller-retain, callee-release, scope-exit release)
- Missing: recursive release of fields when refcount hits zero
- The release slow path (`__nudl_arc_release_slow` in nudl_rt.c) currently just frees
- Need to generate or register destructor functions that release fields before freeing
- Approach: generate a `__nudl_Type_destroy(ptr)` function per reference type that releases all ref-type fields, then frees the object
- `defer`: simplest implementation is to duplicate the deferred code at every scope exit point (return, break, end-of-block). More sophisticated: use a defer stack and cleanup function.
- For MVP defer, lowering to explicit code duplication at exits is simplest
