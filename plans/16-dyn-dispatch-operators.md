# Task 16: Dynamic Dispatch & Operator Overloading

## Goal
Implement `dyn Interface` fat pointers with vtable-based dispatch, define built-in operator interfaces, and add operator desugaring for user-defined types.

## Requirements

### Parsing — `dyn` Types
- `dyn InterfaceName` in type position: `fn print(x: dyn Display) { ... }`
- `dyn InterfaceName<Args>`: `fn process(iter: dyn Iterator<i32>) { ... }`

### AST Changes (`nudl-ast/src/ast.rs`)
- Add `TypeExpr::Dyn { interface_name: Symbol, type_args: Vec<TypeExpr>, span: Span }`

### Parser Changes (`nudl-ast/src/parser.rs`)
- Parse `dyn InterfaceName<Args>` in type position (after `dyn` keyword)

### Type System Changes (`nudl-core/src/types.rs`)
- Add `TypeKind::DynInterface { interface_name: Symbol, type_args: Vec<TypeId> }`
- `dyn Interface` is a **reference type** (fat pointer: data ptr + vtable ptr)

### Type Checker Changes (`nudl-bc/src/checker.rs`)
- Object safety check: an interface is object-safe if no method has method-level type parameters (type params on the interface itself are fine)
- Implicit boxing coercion: `concrete_type` → `dyn Interface` when the concrete type implements the interface
- Register built-in operator interfaces:
  - Arithmetic: `Add<Rhs, Out>`, `Sub<Rhs, Out>`, `Mul<Rhs, Out>`, `Div<Rhs, Out>`, `Rem<Rhs, Out>`
  - Unary: `Neg<Out>`, `Not<Out>`
  - Comparison: `Eq`, `Ord` (returns `Ordering` enum)
  - Indexing: `Index<Idx, Out>`, `IndexMut<Idx, Out>`
- Auto-implement operator interfaces for primitive types (i32, i64, f32, f64, bool)
- Operator desugaring for non-primitive types:
  - `a + b` → `Add::add(a, b)` if `a` is not primitive
  - `a == b` → `Eq::eq(a, b)` if `a` is not primitive
  - `a < b` → `Ord::cmp(a, b) == Ordering::Less` if `a` is not primitive
  - `a[i]` → `Index::index(a, i)` if `a` is not primitive
- Register `Ordering` enum: `enum Ordering { Less, Equal, Greater }`

### IR Changes (`nudl-bc/src/ir.rs`)
- `Instruction::DynBox { dst, value, interface_name }` — box a concrete value into a fat pointer
- `Instruction::DynMethodCall { dst, object, method_index, args }` — indirect call via vtable
- `Instruction::VtableRef { dst, concrete_type, interface_name }` — load vtable pointer

### Lowering Changes (`nudl-bc/src/lower.rs`)
- Lower `dyn` coercion → `DynBox` instruction
- Lower method calls on `dyn Interface` values → `DynMethodCall`

### LLVM Codegen Changes (`nudl-backend-llvm/src/codegen.rs`)
- **Fat pointer layout:** `{ data_ptr: ptr, vtable_ptr: ptr }` — 16 bytes on 64-bit
- **Vtable layout:** `[method_0_ptr, method_1_ptr, ..., drop_fn_ptr, size: u64]` as a static LLVM constant
- Generate one vtable per (concrete_type, interface) pair
- `DynBox` codegen: construct fat pointer from data pointer and vtable constant
- `DynMethodCall` codegen: load vtable ptr from fat pointer → GEP to method slot → indirect call
- Vtable constants are emitted as named globals: `__vtable_TypeName_InterfaceName`

## Acceptance Criteria
- `tests/interfaces/dynamic_dispatch.nudl` — `dyn Interface` values, method calls through vtable
- `tests/interfaces/operator_overloading.nudl` — custom `+`, `==`, `<` operators on user types
- Fat pointer is 16 bytes (two pointers)
- Method calls through `dyn` work correctly at runtime
- Object safety is enforced (error on non-object-safe interfaces used with `dyn`)
- Operator overloading: `a + b` calls `Add::add` for user types
- Comparison operators: `a < b` works with `Ord` implementation
- `Ordering` enum is available as a built-in
- `cargo test --workspace` passes

## Technical Notes
- Fat pointer is the standard approach: Rust, Go, Swift all use similar schemes
- Vtable is generated at compile time as a static constant — no runtime vtable construction
- The drop function in the vtable is called when the `dyn` value is released (ARC reaches 0)
- Object safety restriction: if a method is generic (`fn foo<U>(self, x: U)`), the interface cannot be used with `dyn` because we can't know all possible `U` at vtable generation time
- Operator desugaring happens during type checking — the checker rewrites operator expressions to method calls when the operand type is not primitive
- Built-in operator interfaces are pre-registered in the checker but NOT defined in user code
- Primitive types have built-in implementations that map to existing IR arithmetic instructions (no method call overhead)
- Depends on: Task 15 (interfaces), Task 11 (enums for `Ordering`)
