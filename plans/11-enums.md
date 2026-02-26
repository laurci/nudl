# Task 11: Enum Types

## Goal
Implement basic enum types with unit variants, data variants (single payload), and struct variants (named fields). Full pipeline: parsing → type checking → IR → codegen.

## Requirements

### Parsing
- Unit variants: `enum Color { Red, Green, Blue }`
- Data variants: `enum Shape { Circle(f64), Rect(f64, f64) }`
- Struct variants: `enum Event { Click { x: i32, y: i32 }, KeyPress { key: char } }`
- Mixed variants in same enum allowed
- Enum declaration is a top-level item

### Type System
- Add `TypeKind::Enum { name: Symbol, variants: Vec<EnumVariant> }`
- `EnumVariant`: name + optional payload type(s)
- Enums are **reference types** (heap-allocated, ARC'd) because they can contain reference types
- Each variant has a discriminant tag (integer)

### Construction
- Unit: `Color::Red`
- Data: `Shape::Circle(5.0)`
- Struct: `Event::Click { x: 10, y: 20 }`
- Type checker validates variant names and payload types

### Runtime Representation (LLVM)
- Heap-allocated: `{ i32 tag, [payload bytes] }` (or pointer to payload)
- Simplest: tagged union — `{ i32, [max_payload_size x i8] }` where max size is largest variant
- Alternative: `{ i32 tag, ptr payload }` where payload is separately allocated per variant — simpler but more allocations
- ARC'd like structs: reference counted pointer to the tagged union

### IR
- `Instruction::EnumAlloc { dst, enum_type, variant_index, fields: Vec<Reg> }`
- `Instruction::EnumTag { dst, enum_value }` — extract discriminant
- `Instruction::EnumPayload { dst, enum_value, field_index }` — extract payload field
- Retain/Release follows struct pattern

## Acceptance Criteria
- `tests/user-defined-types/enum_unit.nudl` — unit variants compile and run
- `tests/user-defined-types/enum_data.nudl` — data variants compile and run
- `tests/user-defined-types/enum_struct.nudl` — struct variants compile and run
- Enum values can be passed to functions and returned
- ARC works for enum values (no leaks, no use-after-free)

## Technical Notes
- This task does NOT include pattern matching — that's Task 12
- Without pattern matching, enums are only useful for construction and passing around
- But the infra here is critical for pattern matching, Option, Result
- The discriminant tag can be a simple incrementing integer per variant
- Payload layout: for simplicity, allocate each variant's payload as a separate struct-like allocation
- Alternative: use LLVM's tagged union support
- Memory layout must be stable and known at compile time for codegen
